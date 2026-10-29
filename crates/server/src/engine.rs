//! Agent loop: prompt submission, LLM streaming with retry, tool batch execution
//! (permission, delay, abort), must_report reminders, compaction hand-off.

use crate::convert::{parse_tool_input, partial_purpose, to_chat};
use crate::hub::Hub;
use crate::session::{PendingPerm, RunHandle, Session, SessionState, cancel_unfinished, user_message};
use crate::toolset::{self, Entry, Kind};
use ref_config::{Action, Config};
use ref_llm::{AssistantBlock, BlockKind, LlmError, Model, Provider, StreamEvent};
use ref_proto::{
    Delta, Message, MessageSource, ModelRef, Part, PermissionRequest, Role, SessionEvent, SessionStatus, ToolState,
    now_ms,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

pub const CANCELLED_OUTPUT: &str = "Cancelled by user.";
const MAX_REMINDERS: u32 = 3;
const NON_RETRYABLE_ATTEMPTS: u32 = 3;

/// Everything needed for one LLM request.
pub struct Prep {
    pub cfg: Arc<Config>,
    pub provider: Provider,
    pub model: Model,
    pub model_ref: ModelRef,
    pub agent: String,
    pub system: Vec<String>,
    pub tools: Vec<Entry>,
}

enum Outcome {
    Done,
    Aborted,
    Failed(String),
}

// ---------------------------------------------------------------------------
// Submission / run lifecycle
// ---------------------------------------------------------------------------

/// Deliver a user-role message. Idle sessions start a run; busy ones queue it until the next
/// safe point. Returns true when queued.
pub fn submit(hub: &Arc<Hub>, s: &Arc<Session>, msg: Message) -> bool {
    let queued = {
        let mut st = s.lock();
        maybe_title(hub, s, &mut st, &msg);
        if st.run.is_some() {
            st.queue.push_back(msg);
            true
        } else {
            hub.emit(&s.id, SessionEvent::Message { message: msg.clone() });
            st.push_message(msg);
            start_run_locked(hub, s, &mut st);
            false
        }
    };
    crate::ops::persist(hub, s);
    queued
}

/// Append a message without starting a run (shell output on an idle session).
pub fn append_only(hub: &Arc<Hub>, s: &Arc<Session>, msg: Message) {
    {
        let mut st = s.lock();
        if st.run.is_some() {
            st.queue.push_back(msg);
        } else {
            hub.emit(&s.id, SessionEvent::Message { message: msg.clone() });
            st.push_message(msg);
            hub.emit_info(&st);
        }
    }
    crate::ops::persist(hub, s);
}

fn start_run_locked(hub: &Arc<Hub>, s: &Arc<Session>, st: &mut SessionState) {
    if st.run.is_some() || st.closed {
        return;
    }
    let cancel = CancellationToken::new();
    let id = hub.next_run_id();
    st.run = Some(RunHandle { cancel: cancel.clone(), id });
    st.user_aborted = false;
    hub.set_status(st, SessionStatus::Busy);
    tokio::spawn(run_task(hub.clone(), s.clone(), cancel, id));
}

/// Start an arbitrary task as the session's run (used for manual compaction).
pub fn start_custom_run<F, Fut>(hub: &Arc<Hub>, s: &Arc<Session>, f: F) -> Result<(), String>
where
    F: FnOnce(Arc<Hub>, Arc<Session>, CancellationToken) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let mut st = s.lock();
    if st.run.is_some() {
        return Err("Session is busy.".into());
    }
    let cancel = CancellationToken::new();
    let id = hub.next_run_id();
    st.run = Some(RunHandle { cancel: cancel.clone(), id });
    st.user_aborted = false;
    let (hub2, s2) = (hub.clone(), s.clone());
    tokio::spawn(async move {
        f(hub2.clone(), s2.clone(), cancel).await;
        finish_run(&hub2, &s2, id);
    });
    Ok(())
}

/// Abort the current run (Esc). Pending permissions resolve as denied.
pub fn abort(hub: &Arc<Hub>, s: &Arc<Session>, by_user: bool) {
    let mut st = s.lock();
    if let Some(r) = &st.run {
        r.cancel.cancel();
    }
    if by_user {
        st.user_aborted = true;
    }
    let perms: Vec<PendingPerm> = std::mem::take(&mut st.perms);
    for mut p in perms {
        if let Some(tx) = p.tx.take() {
            let _ = tx.send((false, false));
        }
        hub.emit(&s.id, SessionEvent::PermissionResolved { call_id: p.request.call_id });
    }
}

/// Wait (bounded) until the session has no run.
pub async fn wait_idle(s: &Arc<Session>, timeout: Duration) -> bool {
    let start = Instant::now();
    while s.lock().run.is_some() {
        if start.elapsed() > timeout {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    true
}

async fn run_task(hub: Arc<Hub>, s: Arc<Session>, cancel: CancellationToken, id: u64) {
    let outcome = run_loop(&hub, &s, &cancel).await;
    if let Outcome::Aborted = &outcome {
        let mut st = s.lock();
        cancel_unfinished(&mut st.messages, CANCELLED_OUTPUT);
        if let Some(m) = st.messages.last_mut()
            && m.role == Role::Assistant
            && m.error.is_none()
        {
            m.error = Some("Aborted".into());
            m.time_end.get_or_insert(now_ms());
            let m = m.clone();
            hub.emit(&s.id, SessionEvent::Message { message: m });
        }
    }
    if let Outcome::Failed(text) = &outcome {
        tracing::warn!(session = %s.id, "turn failed: {text}");
        // Keep a visible record of the failure (an assistant message without parts is never
        // sent to the model).
        let (model, agent) = {
            let st = s.lock();
            (st.info.model.clone(), st.info.agent.clone())
        };
        let m = Message {
            id: crate::util::new_id("msg"),
            role: Role::Assistant,
            time: now_ms(),
            time_end: Some(now_ms()),
            source: MessageSource::Human,
            parts: vec![],
            model: Some(model),
            agent: Some(agent),
            usage: None,
            error: Some(text.clone()),
        };
        let mut st = s.lock();
        hub.emit(&s.id, SessionEvent::Message { message: m.clone() });
        st.messages.push(m);
        hub.emit(&s.id, SessionEvent::Error { text: text.clone() });
    }
    finish_run(&hub, &s, id);
}

fn finish_run(hub: &Arc<Hub>, s: &Arc<Session>, id: u64) {
    {
        let mut st = s.lock();
        if st.run.as_ref().map(|r| r.id) != Some(id) {
            return;
        }
        st.run = None;
        if cancel_unfinished(&mut st.messages, CANCELLED_OUTPUT) {
            let msgs = st.messages.clone();
            if let Some(last) = msgs.last().filter(|m| m.role == Role::Assistant) {
                hub.emit(&s.id, SessionEvent::Message { message: last.clone() });
            }
        }
        let perms: Vec<PendingPerm> = std::mem::take(&mut st.perms);
        for p in perms {
            hub.emit(&s.id, SessionEvent::PermissionResolved { call_id: p.request.call_id });
        }
        st.pending_compact = None;
        hub.set_status(&mut st, SessionStatus::Idle);
        let mut restart = false;
        if !st.queue.is_empty() {
            // After a user abort, a prompt the user typed while busy is the redirect they want
            // answered ("type a correction, then interrupt"); peer messages alone do not restart.
            let has_human = st.queue.iter().any(|m| m.source == MessageSource::Human);
            if st.closed || (st.user_aborted && !has_human) {
                while let Some(m) = st.queue.pop_front() {
                    hub.emit(&s.id, SessionEvent::Message { message: m.clone() });
                    st.push_message(m);
                }
            } else {
                restart = true;
            }
        }
        hub.emit_info(&st);
        if restart {
            start_run_locked(hub, s, &mut st);
        }
    }
    crate::ops::persist(hub, s);
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

async fn run_loop(hub: &Arc<Hub>, s: &Arc<Session>, cancel: &CancellationToken) -> Outcome {
    loop {
        if cancel.is_cancelled() {
            return Outcome::Aborted;
        }
        drain_queue(hub, s);
        let prep = match prepare(hub, s, cancel).await {
            Ok(p) => p,
            Err(e) => return if cancel.is_cancelled() { Outcome::Aborted } else { Outcome::Failed(e) },
        };
        // Automatic compaction.
        let (used, auto) = {
            let st = s.lock();
            (st.info.context.used, st.info.context.auto)
        };
        if prep.cfg.compaction.auto && auto > 0 && used >= auto {
            if let Err(e) = crate::compaction::compact(hub, s, &prep, cancel).await {
                if cancel.is_cancelled() {
                    return Outcome::Aborted;
                }
                hub.emit(&s.id, SessionEvent::Error { text: format!("Automatic compaction failed: {e}") });
            }
        }
        let prep = if s.lock().system.is_none() {
            match prepare(hub, s, cancel).await {
                Ok(p) => p,
                Err(e) => return if cancel.is_cancelled() { Outcome::Aborted } else { Outcome::Failed(e) },
            }
        } else {
            prep
        };
        let msg_id = match stream_with_retry(hub, s, &prep, cancel).await {
            Ok(id) => id,
            Err(o) => return o,
        };
        let has_tools = {
            let st = s.lock();
            st.find_message(&msg_id)
                .map(|i| st.messages[i].parts.iter().any(|p| matches!(p, Part::Tool { .. })))
                .unwrap_or(false)
        };
        if has_tools {
            execute_batch(hub, s, &prep, &msg_id, cancel).await;
            crate::ops::persist(hub, s);
            if cancel.is_cancelled() {
                return Outcome::Aborted;
            }
            apply_pending_compact(hub, s);
            continue;
        }
        if !s.lock().queue.is_empty() {
            continue;
        }
        if must_report_reminder(hub, s) {
            continue;
        }
        return Outcome::Done;
    }
}

fn drain_queue(hub: &Arc<Hub>, s: &Arc<Session>) {
    let mut st = s.lock();
    let mut any = false;
    while let Some(m) = st.queue.pop_front() {
        hub.emit(&s.id, SessionEvent::Message { message: m.clone() });
        st.push_message(m);
        any = true;
    }
    if any {
        hub.emit_info(&st);
    }
}

fn apply_pending_compact(hub: &Arc<Hub>, s: &Arc<Session>) {
    let summary = s.lock().pending_compact.take();
    if let Some(summary) = summary {
        crate::compaction::replace_history(hub, s, &summary);
        crate::ops::persist(hub, s);
    }
}

/// Appends a must_report reminder when needed. Returns true if one was added.
fn must_report_reminder(hub: &Arc<Hub>, s: &Arc<Session>) -> bool {
    let parent = {
        let st = s.lock();
        if !st.info.must_report || st.reported || st.reminders >= MAX_REMINDERS {
            return false;
        }
        match &st.info.parent {
            Some(p) => p.clone(),
            None => return false,
        }
    };
    let Some(ps) = hub.session(&parent) else { return false };
    let pname = ps.lock().info.name.clone();
    let text = format!(
        "[Reminder] You stopped without reporting. Send your result (or what blocks you) to \"{pname}\" with send_message (to: \"{parent}\")."
    );
    let msg = user_message(MessageSource::Reminder, text, vec![]);
    {
        let mut st = s.lock();
        st.reminders += 1;
        hub.emit(&s.id, SessionEvent::Message { message: msg.clone() });
        st.push_message(msg);
    }
    crate::ops::persist(hub, s);
    true
}

/// Resolve config, model, system prompt (cached) and tools for the next request.
pub async fn prepare(hub: &Arc<Hub>, s: &Arc<Session>, cancel: &CancellationToken) -> Result<Prep, String> {
    let (cfg, agent, cwd, model_ref, has_system, date) = {
        let st = s.lock();
        (st.cfg.clone(), st.info.agent.clone(), st.info.cwd.clone(), st.info.model.clone(), st.system.is_some(), st.date.clone())
    };
    let servers = hub.mcp.servers_for(&cfg);
    for srv in &servers {
        tokio::select! {
            _ = srv.wait_ready(hub.opts.mcp_wait) => {}
            _ = cancel.cancelled() => return Err("aborted".into()),
        }
    }
    let tools = toolset::assemble(&cfg, &agent, &servers);
    let system = if has_system {
        s.lock().system.clone().unwrap_or_default()
    } else {
        let mcp_instructions: Vec<(String, String)> =
            servers.iter().filter_map(|x| x.instructions().map(|i| (x.name.clone(), i))).collect();
        let home = hub.opts.home.clone();
        let cfg2 = cfg.clone();
        let agent2 = agent.clone();
        let parts = tokio::task::spawn_blocking(move || {
            crate::prompt::build(&crate::prompt::PromptInput {
                cfg: &cfg2,
                agent: &agent2,
                cwd: std::path::Path::new(&cwd),
                home: home.as_deref(),
                date: &date,
                mcp_instructions,
            })
        })
        .await
        .map_err(|e| format!("building the system prompt failed: {e}"))?;
        s.lock().system = Some(parts.clone());
        parts
    };
    let (provider, model) = crate::models::resolve(&cfg, &model_ref, &agent)?;
    Ok(Prep { cfg, provider, model, model_ref, agent, system, tools })
}

pub fn build_request(s: &Arc<Session>, prep: &Prep) -> ref_llm::Request {
    let st = s.lock();
    ref_llm::Request {
        system: prep.system.clone(),
        messages: to_chat(&st.messages, prep.model.images),
        tools: prep.tools.iter().map(|e| e.def.clone()).collect(),
        session_id: s.id.clone(),
    }
}

// ---------------------------------------------------------------------------
// Streaming + retry
// ---------------------------------------------------------------------------

/// Backoff delay before retry `attempt` (1-based): `base * 2^(attempt-1)` capped at `max`,
/// with `jitter` in [-0.2, 0.2] applied, at least `retry_after`, never above `max`.
pub fn backoff(base: Duration, attempt: u32, max: Duration, retry_after: Option<Duration>, jitter: f64) -> Duration {
    let exp = 2f64.powi(attempt.saturating_sub(1).min(40) as i32);
    let raw = (base.as_secs_f64() * exp).min(max.as_secs_f64());
    let jittered = raw * (1.0 + jitter.clamp(-0.2, 0.2));
    let d = Duration::from_secs_f64(jittered.max(0.0));
    let d = match retry_after {
        Some(r) if r > d => r,
        _ => d,
    };
    d.min(max)
}

/// Streams one assistant message, retrying failures. Returns the finished message id.
async fn stream_with_retry(
    hub: &Arc<Hub>,
    s: &Arc<Session>,
    prep: &Prep,
    cancel: &CancellationToken,
) -> Result<String, Outcome> {
    let mut attempt: u32 = 0;
    let mut non_retryable: u32 = 0;
    let mut overflow_compacted = false;
    let max = Duration::from_secs(if prep.cfg.retry_max_delay > 0 { prep.cfg.retry_max_delay } else { 600 });
    let mut prep_owned: Option<Prep> = None;
    loop {
        let p = prep_owned.as_ref().unwrap_or(prep);
        let req = build_request(s, p);
        let msg_id = crate::util::new_id("msg");
        {
            let mut st = s.lock();
            let m = Message {
                id: msg_id.clone(),
                role: Role::Assistant,
                time: now_ms(),
                time_end: None,
                source: MessageSource::Human,
                parts: vec![],
                model: Some(p.model_ref.clone()),
                agent: Some(p.agent.clone()),
                usage: None,
                error: None,
            };
            hub.emit(&s.id, SessionEvent::Message { message: m.clone() });
            st.messages.push(m);
            hub.set_status(&mut st, SessionStatus::Busy);
        }
        let mut streamer = Streamer::new(hub.clone(), s.clone(), msg_id.clone());
        let mut on_event = |ev: StreamEvent| streamer.handle(ev);
        let res = ref_llm::stream(&p.provider, &p.model, &req, cancel, &mut on_event).await;
        match res {
            Ok(resp) => {
                finalize_message(hub, s, p, &msg_id, resp);
                crate::ops::persist(hub, s);
                return Ok(msg_id);
            }
            Err(e) if e.cancelled || cancel.is_cancelled() => {
                keep_aborted_partial(hub, s, &msg_id);
                return Err(Outcome::Aborted);
            }
            Err(e) => {
                remove_message(hub, s, &msg_id);
                if e.context_overflow {
                    if overflow_compacted {
                        return Err(Outcome::Failed(format!(
                            "The conversation exceeds the model's context window even after compaction: {}",
                            e.message
                        )));
                    }
                    overflow_compacted = true;
                    if let Err(ce) = crate::compaction::compact(hub, s, p, cancel).await {
                        if cancel.is_cancelled() {
                            return Err(Outcome::Aborted);
                        }
                        return Err(Outcome::Failed(format!(
                            "Context window exceeded ({}) and compaction failed: {ce}",
                            e.message
                        )));
                    }
                    match prepare(hub, s, cancel).await {
                        Ok(np) => prep_owned = Some(np),
                        Err(pe) => return Err(Outcome::Failed(pe)),
                    }
                    continue;
                }
                attempt += 1;
                if !e.retryable {
                    non_retryable += 1;
                    if non_retryable >= NON_RETRYABLE_ATTEMPTS {
                        return Err(Outcome::Failed(e.message.clone()));
                    }
                }
                let jitter = rand::random_range(-0.2..=0.2);
                let delay = backoff(hub.opts.retry_base, attempt, max, e.retry_after, jitter);
                tracing::warn!(session = %s.id, attempt, "LLM request failed ({}); retrying in {:?}", e.message, delay);
                {
                    let mut st = s.lock();
                    let status = SessionStatus::Retrying {
                        attempt,
                        next_at: now_ms() + delay.as_millis() as i64,
                        error: short_error(&e),
                    };
                    hub.set_status(&mut st, status);
                }
                tokio::select! {
                    _ = tokio::time::sleep(delay) => {}
                    _ = cancel.cancelled() => return Err(Outcome::Aborted),
                }
            }
        }
    }
}

fn short_error(e: &LlmError) -> String {
    crate::util::truncate_chars(e.message.trim(), 500)
}

fn remove_message(hub: &Arc<Hub>, s: &Arc<Session>, id: &str) {
    let mut st = s.lock();
    if let Some(i) = st.find_message(id) {
        st.messages.remove(i);
        hub.emit(&s.id, SessionEvent::Removed { message_id: id.to_string() });
    }
}

/// On abort mid-stream: keep partial text/reasoning (marked "Aborted"), drop unfinished tool calls.
fn keep_aborted_partial(hub: &Arc<Hub>, s: &Arc<Session>, id: &str) {
    let mut st = s.lock();
    let Some(i) = st.find_message(id) else { return };
    let m = &mut st.messages[i];
    m.parts.retain(|p| match p {
        Part::Tool { .. } => false,
        Part::Text { text, .. } => !text.is_empty(),
        _ => true,
    });
    if m.parts.is_empty() {
        st.messages.remove(i);
        hub.emit(&s.id, SessionEvent::Removed { message_id: id.to_string() });
        return;
    }
    m.time_end = Some(now_ms());
    m.error = Some("Aborted".into());
    for p in &mut m.parts {
        if let Part::Reasoning { time_end, .. } = p
            && time_end.is_none()
        {
            *time_end = Some(now_ms());
        }
    }
    let msg = m.clone();
    hub.emit(&s.id, SessionEvent::Message { message: msg });
}

fn finalize_message(hub: &Arc<Hub>, s: &Arc<Session>, p: &Prep, id: &str, resp: ref_llm::Response) {
    let mut st = s.lock();
    let Some(i) = st.find_message(id) else { return };
    let now = now_ms();
    {
        let m = &mut st.messages[i];
        // Reconcile streamed parts with the assembled response (provider data, final args).
        for (bi, block) in resp.blocks.iter().enumerate() {
            match (m.parts.get_mut(bi), block) {
                (Some(Part::Text { text, .. }), AssistantBlock::Text(t)) => *text = t.clone(),
                (Some(Part::Reasoning { text, provider_data, time_end, .. }), AssistantBlock::Reasoning { text: t, provider_data: pd }) => {
                    *text = t.clone();
                    *provider_data = pd.clone();
                    time_end.get_or_insert(now);
                }
                (
                    Some(Part::Tool { raw_input, provider_data, input, purpose, call_id, .. }),
                    AssistantBlock::ToolCall { arguments, provider_data: pd, id, .. },
                ) => {
                    *raw_input = arguments.clone();
                    *provider_data = pd.clone();
                    *call_id = id.clone();
                    if let Ok((v, pu)) = parse_tool_input(arguments) {
                        *input = v;
                        *purpose = pu;
                    }
                }
                (None, b) => m.parts.push(block_to_part(b, now)),
                _ => {}
            }
        }
        m.time_end = Some(now);
        m.usage = Some(resp.usage);
        match resp.stop_reason.as_str() {
            "stop" | "tool_calls" | "end_turn" => {}
            "length" => {
                if !m.parts.iter().any(|p| matches!(p, Part::Tool { .. })) {
                    m.error = Some("The response hit the output token limit.".into());
                }
            }
            other => m.error = Some(format!("Response ended: {other}")),
        }
    }
    let msg = st.messages[i].clone();
    hub.emit(&s.id, SessionEvent::Message { message: msg });
    let mut ctx = crate::models::context_for(&p.cfg, &p.model_ref);
    ctx.used = resp.usage.total_context();
    st.info.context = ctx;
    st.info.updated_at = now;
    hub.emit_info(&st);
}

fn block_to_part(b: &AssistantBlock, now: i64) -> Part {
    match b {
        AssistantBlock::Text(t) => Part::Text { text: t.clone(), time: now },
        AssistantBlock::Reasoning { text, provider_data } => {
            Part::Reasoning { text: text.clone(), time: now, time_end: Some(now), provider_data: provider_data.clone() }
        }
        AssistantBlock::ToolCall { id, name, arguments, provider_data } => {
            let (input, purpose) = parse_tool_input(arguments).unwrap_or((Value::Null, String::new()));
            tool_part(id.clone(), name.clone(), purpose, input, arguments.clone(), provider_data.clone(), now)
        }
    }
}

fn tool_part(call_id: String, name: String, purpose: String, input: Value, raw_input: String, provider_data: Option<Value>, now: i64) -> Part {
    Part::Tool {
        call_id,
        name,
        purpose,
        input,
        raw_input,
        state: ToolState::Streaming,
        output: None,
        meta: None,
        images: vec![],
        time: now,
        time_end: None,
        run_at: None,
        provider_data,
    }
}

/// Applies stream events to the in-progress assistant message (synchronously, short locks).
struct Streamer {
    hub: Arc<Hub>,
    s: Arc<Session>,
    msg_id: String,
}

impl Streamer {
    fn new(hub: Arc<Hub>, s: Arc<Session>, msg_id: String) -> Streamer {
        Streamer { hub, s, msg_id }
    }

    fn handle(&mut self, ev: StreamEvent) {
        let mut st = self.s.lock();
        let Some(mi) = st.find_message(&self.msg_id) else { return };
        let now = now_ms();
        let sid = self.s.id.clone();
        let msg = &mut st.messages[mi];
        match ev {
            StreamEvent::BlockStart { index, kind } => {
                let part = match kind {
                    BlockKind::Text => Part::Text { text: String::new(), time: now },
                    BlockKind::Reasoning => Part::Reasoning { text: String::new(), time: now, time_end: None, provider_data: None },
                    BlockKind::ToolCall { id, name } => tool_part(id, name, String::new(), Value::Null, String::new(), None, now),
                };
                // Block indices are sequential; pad defensively.
                while msg.parts.len() < index {
                    msg.parts.push(Part::Text { text: String::new(), time: now });
                }
                if index < msg.parts.len() {
                    msg.parts[index] = part.clone();
                } else {
                    msg.parts.push(part.clone());
                }
                self.hub.emit(&sid, SessionEvent::Part { message_id: self.msg_id.clone(), index, part });
            }
            StreamEvent::Delta { index, text } => {
                let Some(part) = msg.parts.get_mut(index) else { return };
                let delta = match part {
                    Part::Text { text: t, .. } => {
                        t.push_str(&text);
                        Delta::Text { text }
                    }
                    Part::Reasoning { text: t, .. } => {
                        t.push_str(&text);
                        Delta::Reasoning { text }
                    }
                    Part::Tool { raw_input, purpose, .. } => {
                        raw_input.push_str(&text);
                        if purpose.is_empty()
                            && let Some(p) = partial_purpose(raw_input)
                        {
                            *purpose = p;
                            let part = part.clone();
                            self.hub.emit(&sid, SessionEvent::Part { message_id: self.msg_id.clone(), index, part });
                            return;
                        }
                        Delta::ToolInput { text }
                    }
                    Part::Image { .. } => return,
                };
                self.hub.emit(&sid, SessionEvent::Delta { message_id: self.msg_id.clone(), index, delta });
            }
            StreamEvent::BlockEnd { index } => {
                let Some(part) = msg.parts.get_mut(index) else { return };
                match part {
                    Part::Reasoning { time_end, .. } => *time_end = Some(now),
                    Part::Tool { raw_input, input, purpose, .. } => {
                        if let Ok((v, p)) = parse_tool_input(raw_input) {
                            *input = v;
                            *purpose = p;
                        }
                    }
                    _ => {}
                }
                let part = part.clone();
                self.hub.emit(&sid, SessionEvent::Part { message_id: self.msg_id.clone(), index, part });
            }
            StreamEvent::Usage(u) => msg.usage = Some(u),
        }
    }
}

// ---------------------------------------------------------------------------
// Tool execution
// ---------------------------------------------------------------------------

struct Call {
    index: usize,
    call_id: String,
    name: String,
    raw: String,
}

/// Update a tool part in place and broadcast it.
fn update_tool(hub: &Hub, s: &Session, msg_id: &str, index: usize, f: impl FnOnce(&mut Part)) {
    let mut st = s.lock();
    let Some(mi) = st.find_message(msg_id) else { return };
    let Some(part) = st.messages[mi].parts.get_mut(index) else { return };
    f(part);
    let part = part.clone();
    hub.emit(&s.id, SessionEvent::Part { message_id: msg_id.to_string(), index, part });
}

fn set_state(hub: &Hub, s: &Session, msg_id: &str, index: usize, new: ToolState, run_at: Option<i64>) {
    update_tool(hub, s, msg_id, index, |p| {
        if let Part::Tool { state, run_at: ra, time, .. } = p {
            *state = new;
            *ra = run_at;
            // Durations shown to the user measure execution only, not approval/delay waits.
            if new == ToolState::Running {
                *time = now_ms();
            }
        }
    });
}

fn finish_tool(hub: &Hub, s: &Session, msg_id: &str, index: usize, new: ToolState, out: ref_tools::ToolOutput) {
    update_tool(hub, s, msg_id, index, |p| {
        if let Part::Tool { state, output, meta, images, time_end, run_at, .. } = p {
            *state = new;
            *output = Some(out.text);
            *meta = match (meta.take(), out.meta) {
                (_, Some(m)) => Some(m),
                (Some(Value::Object(mut old)), None) => {
                    old.remove("live_output");
                    if old.is_empty() { None } else { Some(Value::Object(old)) }
                }
                (old, None) => old,
            };
            *images = out.images;
            *time_end = Some(now_ms());
            *run_at = None;
        }
    });
}

async fn execute_batch(hub: &Arc<Hub>, s: &Arc<Session>, prep: &Prep, msg_id: &str, cancel: &CancellationToken) {
    let calls: Vec<Call> = {
        let st = s.lock();
        let Some(mi) = st.find_message(msg_id) else { return };
        st.messages[mi]
            .parts
            .iter()
            .enumerate()
            .filter_map(|(index, p)| match p {
                Part::Tool { call_id, name, raw_input, state, .. } if !state.is_final() => {
                    Some(Call { index, call_id: call_id.clone(), name: name.clone(), raw: raw_input.clone() })
                }
                _ => None,
            })
            .collect()
    };
    for call in calls {
        if cancel.is_cancelled() {
            return;
        }
        run_one(hub, s, prep, msg_id, call, cancel).await;
    }
}

async fn run_one(hub: &Arc<Hub>, s: &Arc<Session>, prep: &Prep, msg_id: &str, call: Call, cancel: &CancellationToken) {
    let idx = call.index;
    let (input, purpose) = match parse_tool_input(&call.raw) {
        Ok(x) => x,
        Err(e) => {
            let raw = crate::util::truncate_chars(&call.raw, 2000);
            let out = ref_tools::ToolOutput::err(format!("Invalid JSON arguments: {e}; raw: {raw}"));
            finish_tool(hub, s, msg_id, idx, ToolState::Error, out);
            return;
        }
    };
    let Some(entry) = prep.tools.iter().find(|e| e.def.name == call.name).cloned() else {
        let out = ref_tools::ToolOutput::err(format!("Unknown tool: {}", call.name));
        finish_tool(hub, s, msg_id, idx, ToolState::Error, out);
        return;
    };

    // Permission.
    let subject = toolset::subject(&call.name, &input);
    let mut action = prep.cfg.permission_for(&prep.agent, &call.name, &subject);
    if action == Action::Ask && s.lock().always.contains(&call.name) {
        action = Action::Allow;
    }
    match action {
        Action::Allow => {}
        Action::Deny => {
            let out = ref_tools::ToolOutput::err(format!("Permission denied by configuration for tool {}.", call.name));
            finish_tool(hub, s, msg_id, idx, ToolState::Denied, out);
            return;
        }
        Action::Ask => {
            let (tx, rx) = tokio::sync::oneshot::channel();
            {
                let mut st = s.lock();
                let request = PermissionRequest {
                    session_id: s.id.clone(),
                    message_id: msg_id.to_string(),
                    call_id: call.call_id.clone(),
                    tool: call.name.clone(),
                    purpose: purpose.clone(),
                    input: input.clone(),
                    time: now_ms(),
                };
                st.perms.push(PendingPerm { request: request.clone(), tx: Some(tx) });
                hub.set_status(&mut st, SessionStatus::WaitingPermission);
                hub.emit(&s.id, SessionEvent::Permission { request });
            }
            set_state(hub, s, msg_id, idx, ToolState::AwaitingPermission, None);
            let reply = tokio::select! {
                r = rx => r.ok(),
                _ = cancel.cancelled() => None,
            };
            {
                let mut st = s.lock();
                st.perms.retain(|p| p.request.call_id != call.call_id);
                hub.emit(&s.id, SessionEvent::PermissionResolved { call_id: call.call_id.clone() });
                if let Some((true, true)) = reply {
                    st.always.insert(call.name.clone());
                }
            }
            match reply {
                Some((true, _)) => {}
                _ if cancel.is_cancelled() => {
                    finish_tool(hub, s, msg_id, idx, ToolState::Cancelled, ref_tools::ToolOutput::err(CANCELLED_OUTPUT));
                    return;
                }
                _ => {
                    finish_tool(hub, s, msg_id, idx, ToolState::Denied, ref_tools::ToolOutput::err("The user denied this tool call."));
                    return;
                }
            }
        }
    }

    // Pre-execution delay.
    let delay = prep.cfg.delay_for(&prep.agent, &call.name);
    if delay > 0.0 && delay.is_finite() {
        let d = Duration::from_secs_f64(delay.min(86_400.0));
        set_state(hub, s, msg_id, idx, ToolState::Delayed, Some(now_ms() + d.as_millis() as i64));
        tokio::select! {
            _ = tokio::time::sleep(d) => {}
            _ = cancel.cancelled() => {
                finish_tool(hub, s, msg_id, idx, ToolState::Cancelled, ref_tools::ToolOutput::err(CANCELLED_OUTPUT));
                return;
            }
        }
    }

    // Execute.
    {
        let mut st = s.lock();
        hub.set_status(&mut st, SessionStatus::Running);
    }
    set_state(hub, s, msg_id, idx, ToolState::Running, None);
    let on_output = live_output_sink(hub, s, msg_id, idx, matches!(entry.kind, Kind::Builtin));
    let tool_cancel = cancel.child_token();
    let mut handle = tokio::spawn(toolset::dispatch(
        hub.clone(),
        s.clone(),
        entry.kind.clone(),
        call.name.clone(),
        input,
        tool_cancel.clone(),
        on_output,
    ));
    let res = tokio::select! {
        r = &mut handle => r,
        _ = cancel.cancelled() => {
            match tokio::time::timeout(Duration::from_secs(5), &mut handle).await {
                Ok(r) => r,
                Err(_) => { handle.abort(); Ok(ref_tools::ToolOutput::err(CANCELLED_OUTPUT)) }
            }
        }
    };
    let (state, out) = match res {
        _ if cancel.is_cancelled() => {
            let mut o = res.unwrap_or_else(|_| ref_tools::ToolOutput::err(CANCELLED_OUTPUT));
            if o.text.trim().is_empty() {
                o.text = CANCELLED_OUTPUT.into();
            } else if o.text != CANCELLED_OUTPUT {
                o.text = format!("{}\n\n{CANCELLED_OUTPUT}", o.text);
            }
            (ToolState::Cancelled, o)
        }
        Ok(o) if o.is_error => (ToolState::Error, o),
        Ok(o) => (ToolState::Done, o),
        Err(e) => (ToolState::Error, ref_tools::ToolOutput::err(format!("Tool crashed: {e}"))),
    };
    finish_tool(hub, s, msg_id, idx, state, out);
    let mut st = s.lock();
    hub.set_status(&mut st, SessionStatus::Busy);
}

/// Throttled live output (bash): updates the part's `meta.live_output` (tail 4KB) every ~200ms.
fn live_output_sink(hub: &Arc<Hub>, s: &Arc<Session>, msg_id: &str, index: usize, enabled: bool) -> toolset::OutputSink {
    if !enabled {
        return None;
    }
    let buf = Arc::new(Mutex::new((String::new(), Instant::now() - Duration::from_secs(1))));
    let (hub, s, msg_id) = (Arc::downgrade(hub), Arc::downgrade(s), msg_id.to_string());
    Some(Arc::new(move |chunk: &str| {
        let tail = {
            let mut b = buf.lock().unwrap_or_else(|e| e.into_inner());
            b.0.push_str(chunk);
            if b.0.len() > 16 * 1024 {
                let keep = crate::util::tail_bytes(&b.0, 8 * 1024).to_string();
                b.0 = keep;
            }
            if b.1.elapsed() < Duration::from_millis(200) {
                return;
            }
            b.1 = Instant::now();
            crate::util::tail_bytes(&b.0, 4096).to_string()
        };
        let (Some(hub), Some(s)) = (hub.upgrade(), s.upgrade()) else { return };
        update_tool(&hub, &s, &msg_id, index, |p| {
            if let Part::Tool { meta, .. } = p {
                let mut m = match meta.take() {
                    Some(Value::Object(m)) => m,
                    _ => serde_json::Map::new(),
                };
                m.insert("live_output".into(), json!(tail));
                *meta = Some(Value::Object(m));
            }
        });
    }))
}

// ---------------------------------------------------------------------------
// Title
// ---------------------------------------------------------------------------

fn maybe_title(hub: &Arc<Hub>, s: &Arc<Session>, st: &mut SessionState, msg: &Message) {
    if st.title_started || !matches!(msg.source, MessageSource::Human | MessageSource::Spawn { .. }) {
        return;
    }
    let text = msg.raw_text();
    if text.trim().is_empty() {
        return;
    }
    st.title_started = true;
    let first = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string();
    st.info.title = crate::util::truncate_chars(&first, 50);
    hub.emit_info(st);
    tokio::spawn(crate::title::generate(hub.clone(), s.clone(), text));
}
