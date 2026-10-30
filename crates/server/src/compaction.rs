//! Context compaction: model-generated summary (manual / automatic / overflow) or a
//! summary supplied by the model via `compact_context`.

use crate::engine::{Prep, backoff};
use crate::hub::Hub;
use crate::session::{Session, user_message};
use ref_llm::{AssistantBlock, ChatMessage, UserContent};
use ref_config::COMPACTION_AGENT;
use ref_proto::{MessageSource, ModelRef, Role, SessionEvent, SessionStatus};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Text of the compaction summary message.
pub fn summary_text(summary: &str) -> String {
    format!("[Context compacted. Summary of the conversation so far:]\n\n{}\n\n[Continue from here.]", summary.trim())
}

/// Request text appended after the history in a compaction request.
pub const SUMMARY_REQUEST: &str =
    "Summarize the conversation above as instructed. Do not call tools; reply with the summary only.";

/// Rough token estimate (≈4 bytes per token, fixed cost per image) of a request.
pub fn estimate_tokens(system: &[String], tools: &[ref_llm::ToolDef], msgs: &[ChatMessage]) -> u64 {
    const IMAGE: usize = 4 * 1500;
    let mut bytes: usize = system.iter().map(String::len).sum();
    bytes += tools.iter().map(|t| t.name.len() + t.description.len() + t.parameters.to_string().len()).sum::<usize>();
    for m in msgs {
        bytes += 16;
        match m {
            ChatMessage::User(c) => {
                for x in c {
                    bytes += match x {
                        UserContent::Text(t) => t.len(),
                        UserContent::Image(_) => IMAGE,
                    };
                }
            }
            ChatMessage::Assistant { blocks, .. } => {
                for b in blocks {
                    bytes += match b {
                        AssistantBlock::Text(t) => t.len(),
                        AssistantBlock::Reasoning { text, .. } => text.len(),
                        AssistantBlock::ToolCall { name, arguments, .. } => name.len() + arguments.len(),
                    };
                }
            }
            ChatMessage::ToolResult { content, images, .. } => bytes += content.len() + images.len() * IMAGE,
        }
    }
    (bytes / 4) as u64
}

/// Insert a compaction summary message at `at` (clamped; `None` = append). The transcript
/// is never shortened: everything before the summary stays visible but is no longer sent
/// to the model. Messages after `at` (e.g. the prompt being answered) remain in context.
pub fn insert_summary(hub: &Arc<Hub>, s: &Arc<Session>, prep: &Prep, summary: &str, at: Option<usize>) {
    let msg = user_message(MessageSource::Compaction, summary_text(summary), vec![]);
    let mut st = s.lock();
    let len = st.messages.len();
    let at = at.unwrap_or(len).min(len);
    st.messages.insert(at, msg.clone());
    if at == len {
        hub.emit(&s.id, SessionEvent::Message { message: msg });
    } else {
        hub.emit(&s.id, st.reset_event());
    }
    // Estimated from the new effective context until the next real usage arrives.
    let chat = crate::convert::to_chat(&st.messages, prep.model.images);
    let defs: Vec<ref_llm::ToolDef> = prep.tools.iter().map(|e| e.def.clone()).collect();
    st.info.context.used = estimate_tokens(&prep.system, &defs, &chat);
    // A new context epoch: the recommend-compaction notice may fire again.
    st.context_notice = false;
    st.notice_pending = false;
    st.info.updated_at = ref_proto::now_ms();
    // Rebuild the system prompt on the next request (fresh instructions); still stable afterwards.
    st.system = None;
    hub.emit_info(&st);
}

/// Index after the last assistant message, if there is one in the effective context
/// (i.e. something new to summarize).
pub fn compactable_upto(messages: &[ref_proto::Message]) -> Option<usize> {
    let start = crate::convert::effective_start(messages);
    let last = messages.iter().rposition(|m| m.role == Role::Assistant && !m.parts.is_empty())?;
    (last >= start).then_some(last + 1)
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

/// Ask the compaction agent for a summary of the effective history and insert it.
pub async fn compact(hub: &Arc<Hub>, s: &Arc<Session>, prep: &Prep, cancel: &CancellationToken) -> Result<(), String> {
    // Summarize everything up to the last assistant message; trailing user messages
    // (the prompt being answered) are kept verbatim after the summary.
    let (prev_status, upto) = {
        let mut st = s.lock();
        let Some(upto) = compactable_upto(&st.messages) else {
            return Err("Nothing to compact.".into());
        };
        let prev = st.info.status.clone();
        hub.set_status(&mut st, SessionStatus::Compacting);
        (prev, upto)
    };
    let res = summarize(hub, s, prep, upto, cancel).await;
    if let Ok(summary) = &res {
        insert_summary(hub, s, prep, summary, Some(upto));
        crate::ops::persist(hub, s);
    }
    {
        let mut st = s.lock();
        let back = if matches!(prev_status, SessionStatus::Idle) { SessionStatus::Idle } else { SessionStatus::Busy };
        hub.set_status(&mut st, back);
    }
    res.map(|_| ())
}

/// Model, provider and system prompt for a compaction request: the hidden `compaction`
/// agent's prompt / model / variant / options, falling back to the session's model.
pub struct CompactionSetup {
    pub provider: ref_llm::Provider,
    pub model: ref_llm::Model,
    pub model_ref: ModelRef,
    pub system: Vec<String>,
}

pub fn setup(prep: &Prep) -> Result<CompactionSetup, String> {
    let cfg = &prep.cfg;
    let agent = cfg.agents.get(COMPACTION_AGENT);
    let prompt = agent.map(|a| a.prompt.trim()).filter(|p| !p.is_empty()).unwrap_or(ref_config::COMPACTION_PROMPT);
    let mut model_ref = match agent.and_then(|a| a.model.clone()) {
        Some(m) if cfg.model(&m).is_some() => m,
        Some(m) => {
            tracing::warn!("compaction agent model {} is not configured; using the session model", m.key());
            prep.model_ref.clone()
        }
        None => prep.model_ref.clone(),
    };
    if let Some(v) = agent.and_then(|a| a.variant.clone()) {
        model_ref.variant = Some(v);
    }
    // Agent options / temperature come from the compaction agent (if it exists).
    let opts_agent = if agent.is_some() { COMPACTION_AGENT } else { prep.agent.as_str() };
    let (provider, model) = crate::models::resolve(cfg, &model_ref, opts_agent)?;
    Ok(CompactionSetup { provider, model, model_ref, system: vec![prompt.to_string()] })
}

async fn summarize(
    hub: &Arc<Hub>,
    s: &Arc<Session>,
    prep: &Prep,
    upto: usize,
    cancel: &CancellationToken,
) -> Result<String, String> {
    let cs = setup(prep)?;
    let history: Vec<ref_proto::Message> = {
        let st = s.lock();
        st.messages[..upto.min(st.messages.len())].to_vec()
    };
    let mut req = ref_llm::Request {
        system: cs.system.clone(),
        messages: crate::convert::to_chat(&history, cs.model.images),
        tools: vec![],
        session_id: s.id.clone(),
    };
    req.messages.push(ChatMessage::User(vec![UserContent::Text(SUMMARY_REQUEST.into())]));
    let mut req = crate::images::clamp_request(req, &cs.provider, &cs.model).await;
    let max = Duration::from_secs(if prep.cfg.retry_max_delay > 0 { prep.cfg.retry_max_delay } else { 600 });
    let mut attempt = 0u32;
    let mut hard_failures = 0u32;
    let mut clipped = false;
    loop {
        let mut sink = |_ev: ref_llm::StreamEvent| {};
        match ref_llm::stream(&cs.provider, &cs.model, &req, cancel, &mut sink).await {
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
            // The provider stated a stricter image limit (recorded by `learn`): clamp again and resend.
            Err(e) if crate::images::learn(&cs.provider, &cs.model, &e.message) => {
                req = crate::images::clamp_request(req, &cs.provider, &cs.model).await;
            }
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
