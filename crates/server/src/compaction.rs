//! Context compaction: model-generated summary (manual / automatic / overflow) or a
//! summary supplied by the model via `compact_context`.

use crate::engine::{Prep, backoff};
use crate::hub::Hub;
use crate::session::{Session, user_message};
use ref_llm::{AssistantBlock, ChatMessage, UserContent};
use ref_proto::{MessageSource, Role, SessionEvent, SessionStatus};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub const COMPACTION_PROMPT: &str = "Summarize the conversation above into a structured summary that will be given to another agent (you, with no other memory) to continue the work.

Use this format (omit sections that do not apply):

## Goal
- What the user is trying to accomplish.

## Instructions and constraints
- Requirements, preferences and scope boundaries stated by the user.

## Discoveries
- Facts learned that are needed to continue (exact paths, symbols, commands, error strings).

## Accomplished
- What has been completed.

## In progress
- What was being worked on when the summary was made, and its exact state.

## Next steps
1. Ordered list of next actions.

## Relevant files
- `path`: why it matters (at most 15, most important first).

Rules: be concise (terse single-line bullets), preserve exact identifiers, carry forward unanswered user requests verbatim, mention whether changes are uncommitted/committed. Do not continue the task and do not call tools. Reply with the summary only.";

/// Text of the user message that replaces the history.
pub fn summary_text(summary: &str) -> String {
    format!("[Context compacted. Summary of the conversation so far:]\n\n{}\n\n[Continue from here.]", summary.trim())
}

/// Replace the whole history with one compaction message and broadcast Reset.
pub fn replace_history(hub: &Arc<Hub>, s: &Arc<Session>, summary: &str) {
    replace_prefix(hub, s, summary, usize::MAX);
}

/// Replace `messages[..upto]` (clamped) with one compaction message; later messages are kept.
fn replace_prefix(hub: &Arc<Hub>, s: &Arc<Session>, summary: &str, upto: usize) {
    let msg = user_message(MessageSource::Compaction, summary_text(summary), vec![]);
    let mut st = s.lock();
    let upto = upto.min(st.messages.len());
    let tail = st.messages.split_off(upto);
    st.messages.clear();
    st.messages.push(msg);
    st.messages.extend(tail);
    st.info.context.used = 0;
    st.info.updated_at = ref_proto::now_ms();
    // Rebuild the system prompt on the next request (fresh instructions); still stable afterwards.
    st.system = None;
    hub.emit(&s.id, SessionEvent::Reset { messages: st.messages.clone() });
    hub.emit_info(&st);
}

fn clip_outputs(msgs: &mut [ChatMessage], max: usize) {
    for m in msgs {
        if let ChatMessage::ToolResult { content, images, .. } = m {
            if content.len() > max {
                *content = crate::util::truncate_middle(content, max);
            }
            images.clear();
        }
    }
}

/// Ask the model for a summary of the current history, then replace the history.
pub async fn compact(hub: &Arc<Hub>, s: &Arc<Session>, prep: &Prep, cancel: &CancellationToken) -> Result<(), String> {
    // Summarize everything up to the last assistant message; trailing user messages
    // (the prompt being answered) are kept verbatim after the summary.
    let (prev_status, upto) = {
        let mut st = s.lock();
        let upto = st.messages.iter().rposition(|m| m.role == Role::Assistant).map(|i| i + 1).unwrap_or(0);
        if upto == 0 {
            return Err("Nothing to compact.".into());
        }
        let prev = st.info.status.clone();
        hub.set_status(&mut st, SessionStatus::Compacting);
        (prev, upto)
    };
    let res = summarize(hub, s, prep, upto, cancel).await;
    {
        let mut st = s.lock();
        let back = if matches!(prev_status, SessionStatus::Idle) { SessionStatus::Idle } else { SessionStatus::Busy };
        hub.set_status(&mut st, back);
    }
    let summary = res?;
    replace_prefix(hub, s, &summary, upto);
    crate::ops::persist(hub, s);
    Ok(())
}

async fn summarize(
    hub: &Arc<Hub>,
    s: &Arc<Session>,
    prep: &Prep,
    upto: usize,
    cancel: &CancellationToken,
) -> Result<String, String> {
    let history: Vec<ref_proto::Message> = {
        let st = s.lock();
        st.messages[..upto.min(st.messages.len())].to_vec()
    };
    let mut req = ref_llm::Request {
        system: prep.system.clone(),
        messages: crate::convert::to_chat(&history, prep.model.images),
        tools: vec![],
        session_id: s.id.clone(),
    };
    req.messages.push(ChatMessage::User(vec![UserContent::Text(COMPACTION_PROMPT.into())]));
    let max = Duration::from_secs(if prep.cfg.retry_max_delay > 0 { prep.cfg.retry_max_delay } else { 600 });
    let mut attempt = 0u32;
    let mut hard_failures = 0u32;
    let mut clipped = false;
    loop {
        let mut sink = |_ev: ref_llm::StreamEvent| {};
        match ref_llm::stream(&prep.provider, &prep.model, &req, cancel, &mut sink).await {
            Ok(resp) => {
                let text: String = resp
                    .blocks
                    .iter()
                    .filter_map(|b| match b {
                        AssistantBlock::Text(t) => Some(t.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if text.trim().is_empty() {
                    return Err("the model returned an empty summary".into());
                }
                return Ok(text.trim().to_string());
            }
            Err(e) if e.cancelled || cancel.is_cancelled() => return Err("aborted".into()),
            Err(e) if e.context_overflow => {
                if clipped {
                    return Err(format!("history too large to summarize: {}", e.message));
                }
                clipped = true;
                clip_outputs(&mut req.messages, 1000);
            }
            Err(e) => {
                attempt += 1;
                if !e.retryable {
                    hard_failures += 1;
                    if hard_failures >= 3 {
                        return Err(e.message);
                    }
                }
                let d = backoff(hub.opts.retry_base, attempt, max, e.retry_after, rand::random_range(-0.2..=0.2));
                tracing::warn!(session = %s.id, "compaction request failed ({}); retrying in {:?}", e.message, d);
                tokio::select! {
                    _ = tokio::time::sleep(d) => {}
                    _ = cancel.cancelled() => return Err("aborted".into()),
                }
            }
        }
    }
}

/// Manual `/compact` on an idle session (runs as the session's run task).
pub async fn manual(hub: Arc<Hub>, s: Arc<Session>, cancel: CancellationToken) {
    let prep = match crate::engine::prepare(&hub, &s, &cancel).await {
        Ok(p) => p,
        Err(e) => {
            hub.emit(&s.id, SessionEvent::Error { text: format!("Compaction failed: {e}") });
            return;
        }
    };
    if let Err(e) = compact(&hub, &s, &prep, &cancel).await {
        hub.emit(&s.id, SessionEvent::Error { text: format!("Compaction failed: {e}") });
    }
}
