//! Agent loop: prompt submission, LLM streaming with retry, tool batch execution
//! (permission, delay, abort), must_report reminders, compaction hand-off.

use crate::convert::{parse_tool_input, partial_purpose, to_chat};
use crate::hub::Hub;
use crate::session::{
    PendingPerm, PermReply, REMINDER_PREFIX, RunHandle, Session, SessionState, cancel_unfinished,
    new_message, user_message,
};
use crate::toolset::{self, Entry, Kind};
use crate::util::lock;
use ref_config::{Action, Config};
use ref_llm::{AssistantBlock, BlockKind, LlmError, Model, Provider, StreamEvent};
use ref_proto::{
    Delta, Message, MessageSource, ModelRef, NotifyKind, Part, PermissionRequest, Role,
    SessionEvent, SessionStatus, ToolState, now_ms,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

pub(crate) const CANCELLED_OUTPUT: &str = "Cancelled by user.";

/// Error text stored on an assistant message whose turn was aborted.
const ABORTED_ERROR: &str = "Aborted";
/// Error text of a turn or compaction that was aborted.
pub(crate) const ABORTED: &str = "aborted";
const MAX_REMINDERS: u32 = 3;

/// Characters of the first user line used as the provisional session title.
const TITLE_MAX_CHARS: usize = 50;
const NON_RETRYABLE_ATTEMPTS: u32 = 3;

/// Characters of a provider error shown in the retry status.
const SHORT_ERROR_CHARS: usize = 500;

/// Characters of the raw tool arguments quoted in a parse error.
const MAX_RAW_ARGS_IN_ERROR: usize = 2000;

/// Longest pre-execution delay of a tool, in seconds.
const MAX_TOOL_DELAY_SECS: f64 = 86_400.0;

/// How long a cancelled tool gets to finish before it is aborted.
const TOOL_CANCEL_GRACE: Duration = Duration::from_secs(5);

/// `meta` key that carries the live output of a running tool.
const LIVE_OUTPUT_KEY: &str = "live_output";

/// The live output buffer is cut to `LIVE_OUTPUT_BUFFER_KEEP` bytes once it grows past this.
const LIVE_OUTPUT_BUFFER_MAX: usize = 16 * 1024;
const LIVE_OUTPUT_BUFFER_KEEP: usize = 8 * 1024;

/// Bytes of the output tail shown while a tool runs.
const LIVE_OUTPUT_TAIL: usize = 4096;

/// Minimum time between two live output updates.
const LIVE_OUTPUT_INTERVAL: Duration = Duration::from_millis(200);

/// Stop reasons of a finished response (normalized by `ref_llm`).
const STOP_END: &str = "stop";
const STOP_TOOL_CALLS: &str = "tool_calls";
const STOP_END_TURN: &str = "end_turn";
const STOP_LENGTH: &str = "length";
/// Largest exponent of the retry backoff, so `2^n` stays finite.
pub(crate) const MAX_BACKOFF_EXPONENT: u32 = 40;
/// Largest relative jitter applied to a retry delay.
pub(crate) const JITTER_FRACTION: f64 = 0.2;

/// Everything needed for one LLM request.
pub(crate) struct Prep {
    pub(crate) cfg: Arc<Config>,
    pub(crate) provider: Provider,
    pub(crate) model: Model,
    pub(crate) model_ref: ModelRef,
    pub(crate) agent: String,
    pub(crate) system: Vec<String>,
    pub(crate) tools: Vec<Entry>,
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
            emit_queue(hub, s, &st);
            true
        } else {
            hub.emit_message(&s.id, &msg);
            st.push_message(msg);
            start_run_locked(hub, s, &mut st);
            false
        }
    };
    crate::ops::persist(hub, s);
    queued
}

/// Withdraw a queued message before it is delivered. Returns `None` when it is no longer queued
/// (already delivered, or unknown). The queue is drained under the same lock, so a message is
/// either removed here or delivered, never both.
pub(crate) fn remove_queued(hub: &Arc<Hub>, s: &Arc<Session>, message_id: &str) -> Option<Message> {
    let removed = {
        let mut st = s.lock();
        let removed = st
            .queue
            .iter()
            .position(|m| m.id == message_id)
            .and_then(|i| st.queue.remove(i));
        if removed.is_some() {
            emit_queue(hub, s, &st);
        }
        removed
    };
    if removed.is_some() {
        // Saves include the queue: drop the withdrawn message from disk too.
        crate::ops::persist(hub, s);
    }
    removed
}

/// Append a message without starting a run (shell output on an idle session).
pub(crate) fn append_only(hub: &Arc<Hub>, s: &Arc<Session>, msg: Message) {
    {
        let mut st = s.lock();
        if st.run.is_some() {
            st.queue.push_back(msg);
            emit_queue(hub, s, &st);
        } else {
            hub.emit_message(&s.id, &msg);
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
    st.run = Some(RunHandle {
        cancel: cancel.clone(),
        id,
    });
    st.user_aborted = false;
    st.info.last_error = None;
    st.info.busy_since = Some(now_ms());
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
    st.run = Some(RunHandle {
        cancel: cancel.clone(),
        id,
    });
    st.user_aborted = false;
    st.info.last_error = None;
    st.info.busy_since = Some(now_ms());
    // `busy_since` is only set while not idle, so the status changes in the same Info.
    hub.set_status(&mut st, SessionStatus::Busy);
    let (hub2, s2) = (hub.clone(), s.clone());
    tokio::spawn(async move {
        f(hub2.clone(), s2.clone(), cancel).await;
        finish_run(&hub2, &s2, id, false);
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
            // A gone receiver means the waiting tool call already ended.
            let _ = tx.send(PermReply {
                allow: false,
                always: false,
            });
        }
        hub.emit(
            &s.id,
            SessionEvent::PermissionResolved {
                call_id: p.request.call_id,
            },
        );
    }
    crate::question::clear(hub, s, &mut st);
}

/// Wait (bounded) until the session has no run.
pub(crate) async fn wait_idle(s: &Arc<Session>, timeout: Duration) -> bool {
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
            m.error = Some(ABORTED_ERROR.into());
            m.time_end.get_or_insert(now_ms());
            hub.emit_message(&s.id, m);
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
        let mut m = new_message(Role::Assistant, MessageSource::Human, vec![]);
        m.time_end = Some(m.time);
        m.model = Some(model);
        m.agent = Some(agent);
        m.error = Some(text.clone());
        let mut st = s.lock();
        hub.emit_message(&s.id, &m);
        st.messages.push(m);
        hub.emit(&s.id, SessionEvent::Error { text: text.clone() });
        st.info.last_error = Some(text.clone());
        if !st.stopped_by_user() {
            hub.notify(&st, NotifyKind::TurnError, Some(text.clone()));
        }
    }
    finish_run(&hub, &s, id, matches!(outcome, Outcome::Done));
}

/// Ends the run `id`. `completed`: the turn ended normally (the user is told if the session
/// goes idle).
fn finish_run(hub: &Arc<Hub>, s: &Arc<Session>, id: u64, completed: bool) {
    {
        let mut st = s.lock();
        if st.run.as_ref().map(|r| r.id) != Some(id) {
            return;
        }
        st.run = None;
        if cancel_unfinished(&mut st.messages, CANCELLED_OUTPUT) {
            let msgs = st.messages.clone();
            if let Some(last) = msgs.last().filter(|m| m.role == Role::Assistant) {
                hub.emit_message(&s.id, last);
            }
        }
        crate::agent_switch::deliver(hub, &mut st);
        let perms: Vec<PendingPerm> = std::mem::take(&mut st.perms);
        for p in perms {
            hub.emit(
                &s.id,
                SessionEvent::PermissionResolved {
                    call_id: p.request.call_id,
                },
            );
        }
        crate::question::clear(hub, s, &mut st);
        st.pending_compact = None;
        if std::mem::take(&mut st.notice_pending) {
            // Aborted / failed before the notice was delivered: allow it again later.
            st.context_notice = false;
        }
        st.info.busy_since = None;
        hub.set_status(&mut st, SessionStatus::Idle);
        let mut restart = false;
        if !st.queue.is_empty() {
            // After a user abort, a prompt the user typed while busy is the redirect they want
            // answered ("type a correction, then interrupt"); peer messages alone do not restart.
            let has_human = st.queue.iter().any(|m| m.source == MessageSource::Human);
            if st.closed || (st.user_aborted && !has_human) {
                while let Some(m) = st.queue.pop_front() {
                    hub.emit_message(&s.id, &m);
                    st.push_message(m);
                }
                emit_queue(hub, s, &st);
            } else {
                restart = true;
            }
        }
        hub.emit_info(&st);
        if completed && !restart && !st.stopped_by_user() {
            hub.notify(&st, NotifyKind::TurnDone, None);
        }
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
            Err(e) => {
                return if cancel.is_cancelled() {
                    Outcome::Aborted
                } else {
                    Outcome::Failed(e)
                };
            }
        };
        // Automatic compaction.
        let (used, auto) = {
            let st = s.lock();
            (st.info.context.used, st.info.context.auto)
        };
        let compactable = crate::compaction::compactable_upto(&s.lock().messages).is_some();
        let should_compact = prep.cfg.compaction.auto && auto > 0 && used >= auto && compactable;
        if should_compact && let Err(e) = crate::compaction::compact(hub, s, &prep, cancel).await {
            if cancel.is_cancelled() {
                return Outcome::Aborted;
            }
            hub.emit(
                &s.id,
                SessionEvent::Error {
                    text: format!("Automatic compaction failed: {e}"),
                },
            );
        }
        let prep = if s.lock().system.is_none() {
            match prepare(hub, s, cancel).await {
                Ok(p) => p,
                Err(e) => {
                    return if cancel.is_cancelled() {
                        Outcome::Aborted
                    } else {
                        Outcome::Failed(e)
                    };
                }
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
                .map(|i| {
                    st.messages[i]
                        .parts
                        .iter()
                        .any(|p| matches!(p, Part::Tool { .. }))
                })
                .unwrap_or(false)
        };
        if has_tools {
            execute_batch(hub, s, &prep, &msg_id, cancel).await;
            crate::ops::persist(hub, s);
            if cancel.is_cancelled() {
                return Outcome::Aborted;
            }
            apply_pending_compact(hub, s, &prep);
            context_notice(hub, s, &prep);
            continue;
        }
        // Without another request in this run the notice goes out with the next prompt.
        context_notice(hub, s, &prep);
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
        hub.emit_message(&s.id, &m);
        st.push_message(m);
        any = true;
    }
    if any {
        emit_queue(hub, s, &st);
        hub.emit_info(&st);
    }
}

/// Tells frontends what is waiting in the queue now.
pub(crate) fn emit_queue(hub: &Arc<Hub>, s: &Arc<Session>, st: &SessionState) {
    hub.emit(
        &s.id,
        SessionEvent::Queue {
            messages: st.queue.iter().cloned().collect(),
        },
    );
}

fn apply_pending_compact(hub: &Arc<Hub>, s: &Arc<Session>, prep: &Prep) {
    let summary = s.lock().pending_compact.take();
    if let Some(summary) = summary {
        crate::compaction::insert_summary(hub, s, prep, &summary, None);
        crate::ops::persist(hub, s);
    }
}

/// Text of the one-time recommend-compaction notice.
fn context_notice_text(c: &ref_proto::ContextUsage) -> String {
    let pct = if c.max > 0 {
        c.used as f64 * 100.0 / c.max as f64
    } else {
        0.0
    };
    format!(
        "[Context notice] This conversation has used {} of {} tokens ({pct:.0}%), past the recommended compaction point ({} tokens). At a good moment (e.g. after finishing the current step), call compact_context with a thorough summary to continue with a fresh context.",
        c.used, c.max, c.recommend
    )
}

/// Appends the recommend-compaction notice if it became due (see `finalize_message`).
fn context_notice(hub: &Arc<Hub>, s: &Arc<Session>, prep: &Prep) {
    {
        let mut st = s.lock();
        if !std::mem::take(&mut st.notice_pending) {
            return;
        }
        if !prep
            .tools
            .iter()
            .any(|e| e.def.name == toolset::COMPACT_CONTEXT)
        {
            return;
        }
        let msg = user_message(
            MessageSource::Reminder,
            context_notice_text(&st.info.context),
            vec![],
        );
        hub.emit_message(&s.id, &msg);
        st.push_message(msg);
        hub.emit_info(&st);
    }
    crate::ops::persist(hub, s);
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
    let Some(ps) = hub.session(&parent) else {
        return false;
    };
    let pname = ps.lock().info.name.clone();
    let text = format!(
        "{REMINDER_PREFIX} You stopped without reporting. Send your result (or what blocks you) to \"{pname}\" with send_message (to: \"{parent}\")."
    );
    let msg = user_message(MessageSource::Reminder, text, vec![]);
    {
        let mut st = s.lock();
        st.reminders += 1;
        hub.emit_message(&s.id, &msg);
        st.push_message(msg);
    }
    crate::ops::persist(hub, s);
    true
}

/// Resolve config, model, system prompt (cached) and tools for the next request.
pub(crate) async fn prepare(
    hub: &Arc<Hub>,
    s: &Arc<Session>,
    cancel: &CancellationToken,
) -> Result<Prep, String> {
    let (cfg, agent, cwd, model_ref, cached_system, date) = {
        let mut st = s.lock();
        // The agent is read here for the request, so a switch made during the run is told now.
        crate::agent_switch::deliver(hub, &mut st);
        (
            st.cfg.clone(),
            st.info.agent.clone(),
            st.info.cwd.clone(),
            st.info.model.clone(),
            st.system.clone(),
            st.date.clone(),
        )
    };
    let servers = hub.mcp.servers_for(&cfg);
    for srv in &servers {
        tokio::select! {
            _ = srv.wait_ready(hub.opts.mcp_wait) => {}
            _ = cancel.cancelled() => return Err(ABORTED.into()),
        }
    }
    let tools = toolset::assemble(&cfg, &agent, &servers);
    let system = if let Some(system) = cached_system {
        system
    } else {
        let mcp_instructions: Vec<(String, String)> = servers
            .iter()
            .filter_map(|x| x.instructions().map(|i| (x.name.clone(), i)))
            .collect();
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
    Ok(Prep {
        cfg,
        provider,
        model,
        model_ref,
        agent,
        system,
        tools,
    })
}

fn build_request(s: &Arc<Session>, prep: &Prep) -> ref_llm::Request {
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

const DEFAULT_RETRY_MAX_DELAY_SECS: u64 = 600;

/// Longest wait between retries: the configured `retry_max_delay`, or the default when it is 0.
pub(crate) fn retry_max_delay(cfg: &Config) -> Duration {
    match cfg.retry_max_delay {
        0 => Duration::from_secs(DEFAULT_RETRY_MAX_DELAY_SECS),
        secs => Duration::from_secs(secs),
    }
}

/// Backoff delay before retry `attempt` (1-based): `base * 2^(attempt-1)` capped at `max`,
/// with `jitter` (within `JITTER_FRACTION` of zero) applied, at least `retry_after`, never above `max`.
pub fn backoff(
    base: Duration,
    attempt: u32,
    max: Duration,
    retry_after: Option<Duration>,
    jitter: f64,
) -> Duration {
    let exp = 2f64.powi(attempt.saturating_sub(1).min(MAX_BACKOFF_EXPONENT) as i32);
    let raw = (base.as_secs_f64() * exp).min(max.as_secs_f64());
    let jittered = raw * (1.0 + jitter.clamp(-JITTER_FRACTION, JITTER_FRACTION));
    let d = Duration::from_secs_f64(jittered);
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
    let max = retry_max_delay(&prep.cfg);
    let mut prep_owned: Option<Prep> = None;
    loop {
        let p = prep_owned.as_ref().unwrap_or(prep);
        let req = crate::images::clamp_request(build_request(s, p), &p.provider, &p.model).await;
        let mut m = new_message(Role::Assistant, MessageSource::Human, vec![]);
        m.model = Some(p.model_ref.clone());
        m.agent = Some(p.agent.clone());
        let msg_id = m.id.clone();
        {
            let mut st = s.lock();
            hub.emit_message(&s.id, &m);
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
                // The provider stated a stricter image limit: the next request is clamped to it.
                if crate::images::learn(&p.provider, &p.model, &e.message) {
                    continue;
                }
                if e.context_overflow {
                    let np = recover_from_overflow(hub, s, p, cancel, &mut overflow_compacted, &e)
                        .await?;
                    prep_owned = Some(np);
                    continue;
                }
                attempt += 1;
                if !e.retryable {
                    non_retryable += 1;
                    if non_retryable >= NON_RETRYABLE_ATTEMPTS {
                        return Err(Outcome::Failed(e.message.clone()));
                    }
                }
                let jitter = rand::random_range(-JITTER_FRACTION..=JITTER_FRACTION);
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

/// The context overflowed: compacts (once per request) and prepares the request again.
async fn recover_from_overflow(
    hub: &Arc<Hub>,
    s: &Arc<Session>,
    p: &Prep,
    cancel: &CancellationToken,
    overflow_compacted: &mut bool,
    e: &LlmError,
) -> Result<Prep, Outcome> {
    if *overflow_compacted {
        return Err(Outcome::Failed(format!(
            "The conversation exceeds the model's context window even after compaction: {}",
            e.message
        )));
    }
    *overflow_compacted = true;
    if let Err(ce) = crate::compaction::compact(hub, s, p, cancel).await {
        if cancel.is_cancelled() {
            return Err(Outcome::Aborted);
        }
        return Err(Outcome::Failed(format!(
            "Context window exceeded ({}) and compaction failed: {ce}",
            e.message
        )));
    }
    prepare(hub, s, cancel).await.map_err(Outcome::Failed)
}

fn short_error(e: &LlmError) -> String {
    crate::util::truncate_chars(e.message.trim(), SHORT_ERROR_CHARS)
}

fn remove_message(hub: &Arc<Hub>, s: &Arc<Session>, id: &str) {
    let mut st = s.lock();
    if let Some(i) = st.find_message(id) {
        st.messages.remove(i);
        hub.emit(
            &s.id,
            SessionEvent::Removed {
                message_id: id.to_string(),
            },
        );
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
        hub.emit(
            &s.id,
            SessionEvent::Removed {
                message_id: id.to_string(),
            },
        );
        return;
    }
    m.time_end = Some(now_ms());
    m.error = Some(ABORTED_ERROR.into());
    for p in &mut m.parts {
        if let Part::Reasoning { time_end, .. } = p
            && time_end.is_none()
        {
            *time_end = Some(now_ms());
        }
    }
    hub.emit_message(&s.id, m);
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
                (
                    Some(Part::Reasoning {
                        text,
                        provider_data,
                        time_end,
                        ..
                    }),
                    AssistantBlock::Reasoning {
                        text: t,
                        provider_data: pd,
                    },
                ) => {
                    *text = t.clone();
                    *provider_data = pd.clone();
                    time_end.get_or_insert(now);
                }
                (
                    Some(Part::Tool {
                        raw_input,
                        provider_data,
                        input,
                        purpose,
                        call_id,
                        ..
                    }),
                    AssistantBlock::ToolCall {
                        arguments,
                        provider_data: pd,
                        id,
                        ..
                    },
                ) => {
                    *raw_input = arguments.clone();
                    *provider_data = pd.clone();
                    *call_id = id.clone();
                    apply_parsed_input(arguments, input, purpose);
                }
                (None, b) => m.parts.push(block_to_part(b, now)),
                _ => {}
            }
        }
        m.time_end = Some(now);
        m.usage = Some(resp.usage);
        match resp.stop_reason.as_str() {
            STOP_END | STOP_TOOL_CALLS | STOP_END_TURN => {}
            STOP_LENGTH => {
                if !m.parts.iter().any(|p| matches!(p, Part::Tool { .. })) {
                    m.error = Some("The response hit the output token limit.".into());
                }
            }
            other => m.error = Some(format!("Response ended: {other}")),
        }
    }
    hub.emit_message(&s.id, &st.messages[i]);
    let mut ctx = crate::models::context_for(&p.cfg, &p.model_ref);
    ctx.used = resp.usage.total_context();
    st.info.context = ctx;
    // First time per context epoch past the recommended point: notify the model at the next
    // safe point (after the current tool batch).
    // Skipped when automatic compaction is about to run anyway.
    let auto_due = p.cfg.compaction.auto && ctx.auto > 0 && ctx.used >= ctx.auto;
    let recommend_due = ctx.recommend > 0 && ctx.used >= ctx.recommend;
    if !st.context_notice && !auto_due && recommend_due {
        st.context_notice = true;
        st.notice_pending = true;
    }
    st.info.updated_at = now;
    hub.emit_info(&st);
}

/// Sets `input` and `purpose` from raw tool arguments. Malformed arguments leave them as they
/// are: the call reports the parse error when it executes (see `run_one`).
fn apply_parsed_input(raw: &str, input: &mut Value, purpose: &mut String) {
    if let Ok((v, p)) = parse_tool_input(raw) {
        *input = v;
        *purpose = p;
    }
}

fn block_to_part(b: &AssistantBlock, now: i64) -> Part {
    match b {
        AssistantBlock::Text(t) => Part::Text {
            text: t.clone(),
            time: now,
        },
        AssistantBlock::Reasoning {
            text,
            provider_data,
        } => Part::Reasoning {
            text: text.clone(),
            time: now,
            time_end: Some(now),
            provider_data: provider_data.clone(),
        },
        AssistantBlock::ToolCall {
            id,
            name,
            arguments,
            provider_data,
        } => {
            let mut input = Value::Null;
            let mut purpose = String::new();
            apply_parsed_input(arguments, &mut input, &mut purpose);
            tool_part(
                id.clone(),
                name.clone(),
                purpose,
                input,
                arguments.clone(),
                provider_data.clone(),
                now,
            )
        }
    }
}

fn tool_part(
    call_id: String,
    name: String,
    purpose: String,
    input: Value,
    raw_input: String,
    provider_data: Option<Value>,
    now: i64,
) -> Part {
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
        let Some(mi) = st.find_message(&self.msg_id) else {
            return;
        };
        let now = now_ms();
        let sid = self.s.id.clone();
        let msg = &mut st.messages[mi];
        match ev {
            StreamEvent::BlockStart { index, kind } => {
                let part = match kind {
                    BlockKind::Text => Part::Text {
                        text: String::new(),
                        time: now,
                    },
                    BlockKind::Reasoning => Part::Reasoning {
                        text: String::new(),
                        time: now,
                        time_end: None,
                        provider_data: None,
                    },
                    BlockKind::ToolCall { id, name } => tool_part(
                        id,
                        name,
                        String::new(),
                        Value::Null,
                        String::new(),
                        None,
                        now,
                    ),
                };
                // Block indices are sequential; pad defensively.
                while msg.parts.len() < index {
                    msg.parts.push(Part::Text {
                        text: String::new(),
                        time: now,
                    });
                }
                if index < msg.parts.len() {
                    msg.parts[index] = part.clone();
                } else {
                    msg.parts.push(part.clone());
                }
                self.hub.emit(
                    &sid,
                    SessionEvent::Part {
                        message_id: self.msg_id.clone(),
                        index,
                        part,
                    },
                );
            }
            StreamEvent::Delta { index, text } => {
                let Some(part) = msg.parts.get_mut(index) else {
                    return;
                };
                let delta = match part {
                    Part::Text { text: t, .. } => {
                        t.push_str(&text);
                        Delta::Text { text }
                    }
                    Part::Reasoning { text: t, .. } => {
                        t.push_str(&text);
                        Delta::Reasoning { text }
                    }
                    Part::Tool {
                        raw_input, purpose, ..
                    } => {
                        raw_input.push_str(&text);
                        if purpose.is_empty()
                            && let Some(p) = partial_purpose(raw_input)
                        {
                            *purpose = p;
                            let part = part.clone();
                            self.hub.emit(
                                &sid,
                                SessionEvent::Part {
                                    message_id: self.msg_id.clone(),
                                    index,
                                    part,
                                },
                            );
                            return;
                        }
                        Delta::ToolInput { text }
                    }
                    Part::Image { .. } => return,
                };
                self.hub.emit(
                    &sid,
                    SessionEvent::Delta {
                        message_id: self.msg_id.clone(),
                        index,
                        delta,
                    },
                );
            }
            StreamEvent::BlockEnd { index } => {
                let Some(part) = msg.parts.get_mut(index) else {
                    return;
                };
                match part {
                    Part::Reasoning { time_end, .. } => *time_end = Some(now),
                    Part::Tool {
                        raw_input,
                        input,
                        purpose,
                        ..
                    } => {
                        apply_parsed_input(raw_input, input, purpose);
                    }
                    _ => {}
                }
                let part = part.clone();
                self.hub.emit(
                    &sid,
                    SessionEvent::Part {
                        message_id: self.msg_id.clone(),
                        index,
                        part,
                    },
                );
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
    let Some(mi) = st.find_message(msg_id) else {
        return;
    };
    let Some(part) = st.messages[mi].parts.get_mut(index) else {
        return;
    };
    f(part);
    let part = part.clone();
    hub.emit(
        &s.id,
        SessionEvent::Part {
            message_id: msg_id.to_string(),
            index,
            part,
        },
    );
}

pub(crate) fn set_state(
    hub: &Hub,
    s: &Session,
    msg_id: &str,
    index: usize,
    new: ToolState,
    run_at: Option<i64>,
) {
    update_tool(hub, s, msg_id, index, |p| {
        if let Part::Tool {
            state,
            run_at: ra,
            time,
            ..
        } = p
        {
            *state = new;
            *ra = run_at;
            // Durations shown to the user measure execution only, not approval/delay waits.
            if new == ToolState::Running {
                *time = now_ms();
            }
        }
    });
}

pub(crate) fn finish_tool(
    hub: &Hub,
    s: &Session,
    msg_id: &str,
    index: usize,
    new: ToolState,
    out: ref_tools::ToolOutput,
) {
    update_tool(hub, s, msg_id, index, |p| {
        if let Part::Tool {
            state,
            output,
            meta,
            images,
            time_end,
            run_at,
            ..
        } = p
        {
            *state = new;
            *output = Some(out.text);
            *meta = match (meta.take(), out.meta) {
                (_, Some(m)) => Some(m),
                (Some(Value::Object(mut old)), None) => {
                    old.remove(LIVE_OUTPUT_KEY);
                    if old.is_empty() {
                        None
                    } else {
                        Some(Value::Object(old))
                    }
                }
                (old, None) => old,
            };
            *images = out.images;
            *time_end = Some(now_ms());
            *run_at = None;
        }
    });
}

async fn execute_batch(
    hub: &Arc<Hub>,
    s: &Arc<Session>,
    prep: &Prep,
    msg_id: &str,
    cancel: &CancellationToken,
) {
    let calls: Vec<Call> = {
        let st = s.lock();
        let Some(mi) = st.find_message(msg_id) else {
            return;
        };
        st.messages[mi]
            .parts
            .iter()
            .enumerate()
            .filter_map(|(index, p)| match p {
                Part::Tool {
                    call_id,
                    name,
                    raw_input,
                    state,
                    ..
                } if !state.is_final() => Some(Call {
                    index,
                    call_id: call_id.clone(),
                    name: name.clone(),
                    raw: raw_input.clone(),
                }),
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

/// One tool call of a batch with what its steps need.
struct CallCtx<'a> {
    hub: &'a Arc<Hub>,
    s: &'a Arc<Session>,
    msg_id: &'a str,
    call: &'a Call,
    cancel: &'a CancellationToken,
}

impl CallCtx<'_> {
    /// Ends the call with `state` and `out`.
    fn finish(&self, state: ToolState, out: ref_tools::ToolOutput) {
        finish_tool(self.hub, self.s, self.msg_id, self.call.index, state, out);
    }

    /// Ends the call as cancelled by the user.
    fn finish_cancelled(&self) {
        let out = ref_tools::ToolOutput::err(CANCELLED_OUTPUT);
        self.finish(ToolState::Cancelled, out);
    }

    fn set_state(&self, new: ToolState, run_at: Option<i64>) {
        set_state(self.hub, self.s, self.msg_id, self.call.index, new, run_at);
    }

    /// Applies the permission rules and asks the user when needed. Returns false when the
    /// call was refused (it is finished then).
    async fn authorize(&self, prep: &Prep, input: &Value, purpose: &str) -> bool {
        let call = self.call;
        let subject = toolset::subject(&call.name, input);
        let mut action = prep.cfg.permission_for(&prep.agent, &call.name, &subject);
        if action == Action::Ask && self.s.lock().always.contains(&call.name) {
            action = Action::Allow;
        }
        match action {
            Action::Allow => true,
            Action::Deny => {
                let out = ref_tools::ToolOutput::err(format!(
                    "Permission denied by configuration for tool {}.",
                    call.name
                ));
                self.finish(ToolState::Denied, out);
                false
            }
            Action::Ask => self.ask_user(input, purpose).await,
        }
    }

    /// Asks the user for permission and waits for the answer. Returns false when the call
    /// was denied or cancelled (it is finished then).
    async fn ask_user(&self, input: &Value, purpose: &str) -> bool {
        let (hub, s, call) = (self.hub, self.s, self.call);
        let (tx, rx) = tokio::sync::oneshot::channel();
        {
            let mut st = s.lock();
            let request = PermissionRequest {
                session_id: s.id.clone(),
                message_id: self.msg_id.to_string(),
                call_id: call.call_id.clone(),
                tool: call.name.clone(),
                purpose: purpose.to_string(),
                input: input.clone(),
                time: now_ms(),
            };
            st.perms.push(PendingPerm {
                request: request.clone(),
                tx: Some(tx),
            });
            hub.set_status(&mut st, SessionStatus::WaitingPermission);
            hub.emit(&s.id, SessionEvent::Permission { request });
            hub.notify(&st, NotifyKind::Permission, Some(call.name.clone()));
        }
        self.set_state(ToolState::AwaitingPermission, None);
        let reply = tokio::select! {
            r = rx => r.ok(),
            _ = self.cancel.cancelled() => None,
        };
        {
            let mut st = s.lock();
            st.perms.retain(|p| p.request.call_id != call.call_id);
            hub.emit(
                &s.id,
                SessionEvent::PermissionResolved {
                    call_id: call.call_id.clone(),
                },
            );
            if let Some(PermReply {
                allow: true,
                always: true,
            }) = reply
            {
                st.always.insert(call.name.clone());
            }
        }
        if let Some(PermReply { allow: true, .. }) = reply {
            // Allowed: the tool may still wait out a delay, but the session no longer waits
            // for the user.
            let mut st = s.lock();
            hub.set_status(&mut st, SessionStatus::Running);
            return true;
        }
        if self.cancel.is_cancelled() {
            self.finish_cancelled();
            return false;
        }
        let out = ref_tools::ToolOutput::err("The user denied this tool call.");
        self.finish(ToolState::Denied, out);
        false
    }

    /// Waits out the configured pre-execution delay. Returns false when cancelled meanwhile
    /// (the call is finished then).
    async fn wait_delay(&self, prep: &Prep) -> bool {
        let delay = prep.cfg.delay_for(&prep.agent, &self.call.name);
        if !(delay > 0.0 && delay.is_finite()) {
            return true;
        }
        let d = Duration::from_secs_f64(delay.min(MAX_TOOL_DELAY_SECS));
        self.set_state(ToolState::Delayed, Some(now_ms() + d.as_millis() as i64));
        tokio::select! {
            _ = tokio::time::sleep(d) => true,
            _ = self.cancel.cancelled() => {
                self.finish_cancelled();
                false
            }
        }
    }

    /// Runs the tool and ends the call with its result.
    async fn execute(&self, entry: &Entry, input: Value) {
        let (hub, s, call) = (self.hub, self.s, self.call);
        {
            let mut st = s.lock();
            hub.set_status(&mut st, SessionStatus::Running);
        }
        self.set_state(ToolState::Running, None);
        let on_output = live_output_sink(
            hub,
            s,
            self.msg_id,
            call.index,
            matches!(entry.kind, Kind::Builtin),
        );
        let tool_cancel = self.cancel.child_token();
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
            _ = self.cancel.cancelled() => {
                match tokio::time::timeout(TOOL_CANCEL_GRACE, &mut handle).await {
                    Ok(r) => r,
                    Err(_) => { handle.abort(); Ok(ref_tools::ToolOutput::err(CANCELLED_OUTPUT)) }
                }
            }
        };
        let (state, out) = match res {
            _ if self.cancel.is_cancelled() => {
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
            Err(e) => (
                ToolState::Error,
                ref_tools::ToolOutput::err(format!("Tool crashed: {e}")),
            ),
        };
        self.finish(state, out);
        let mut st = s.lock();
        hub.set_status(&mut st, SessionStatus::Busy);
    }
}

async fn run_one(
    hub: &Arc<Hub>,
    s: &Arc<Session>,
    prep: &Prep,
    msg_id: &str,
    call: Call,
    cancel: &CancellationToken,
) {
    let ctx = CallCtx {
        hub,
        s,
        msg_id,
        call: &call,
        cancel,
    };
    let (input, purpose) = match parse_tool_input(&call.raw) {
        Ok(x) => x,
        Err(e) => {
            let raw = crate::util::truncate_chars(&call.raw, MAX_RAW_ARGS_IN_ERROR);
            let out =
                ref_tools::ToolOutput::err(format!("Invalid JSON arguments: {e}; raw: {raw}"));
            ctx.finish(ToolState::Error, out);
            return;
        }
    };
    let Some(entry) = prep.tools.iter().find(|e| e.def.name == call.name) else {
        let out = ref_tools::ToolOutput::err(format!("Unknown tool: {}", call.name));
        ctx.finish(ToolState::Error, out);
        return;
    };
    if !ctx.authorize(prep, &input, &purpose).await {
        return;
    }
    if !ctx.wait_delay(prep).await {
        return;
    }
    // The question tool waits for the user instead of running.
    if call.name == crate::question::TOOL && matches!(entry.kind, Kind::Harness) {
        crate::question::run(hub, s, msg_id, call.index, &call.call_id, &input, cancel).await;
        return;
    }
    ctx.execute(entry, input).await;
}

/// Throttled live output (bash): updates the part's `meta.live_output` with the output tail.
fn live_output_sink(
    hub: &Arc<Hub>,
    s: &Arc<Session>,
    msg_id: &str,
    index: usize,
    enabled: bool,
) -> toolset::OutputSink {
    if !enabled {
        return None;
    }
    let buf = Arc::new(Mutex::new((
        String::new(),
        // As if the last update was a full interval ago: the first chunk goes out at once.
        Instant::now() - LIVE_OUTPUT_INTERVAL,
    )));
    let (hub, s, msg_id) = (Arc::downgrade(hub), Arc::downgrade(s), msg_id.to_string());
    Some(Arc::new(move |chunk: &str| {
        let tail = {
            let mut b = lock(&buf);
            b.0.push_str(chunk);
            if b.0.len() > LIVE_OUTPUT_BUFFER_MAX {
                let keep = crate::util::tail_bytes(&b.0, LIVE_OUTPUT_BUFFER_KEEP).to_string();
                b.0 = keep;
            }
            if b.1.elapsed() < LIVE_OUTPUT_INTERVAL {
                return;
            }
            b.1 = Instant::now();
            crate::util::tail_bytes(&b.0, LIVE_OUTPUT_TAIL).to_string()
        };
        let (Some(hub), Some(s)) = (hub.upgrade(), s.upgrade()) else {
            return;
        };
        update_tool(&hub, &s, &msg_id, index, |p| {
            if let Part::Tool { meta, .. } = p {
                let mut m = match meta.take() {
                    Some(Value::Object(m)) => m,
                    _ => serde_json::Map::new(),
                };
                m.insert(LIVE_OUTPUT_KEY.into(), json!(tail));
                *meta = Some(Value::Object(m));
            }
        });
    }))
}

// ---------------------------------------------------------------------------
// Title
// ---------------------------------------------------------------------------

fn maybe_title(hub: &Arc<Hub>, s: &Arc<Session>, st: &mut SessionState, msg: &Message) {
    if st.title_started
        || !matches!(
            msg.source,
            MessageSource::Human | MessageSource::Spawn { .. }
        )
    {
        return;
    }
    let text = msg.raw_text();
    let Some(first) = text.lines().map(str::trim).find(|l| !l.is_empty()) else {
        return;
    };
    st.title_started = true;
    st.info.title = crate::util::truncate_chars(first, TITLE_MAX_CHARS);
    hub.emit_info(st);
    tokio::spawn(crate::title::generate(hub.clone(), s.clone(), text));
}
