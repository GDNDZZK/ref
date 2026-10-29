//! The agent loop: one driver task per open session runs turns and compactions.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use futures::StreamExt;
use ref_config::Config;
use ref_llm::{FinishReason, LlmError, ModelSpec, Request, StreamEvent};
use ref_mcp::McpManager;
use ref_proto::spec::ToolDef;
use ref_proto::{
    Message, MessageOrigin, NoticeLevel, Part, ReasoningPart, Role, ServerMsg, SessionStatus, TextPart, ToolPart,
    ToolState,
};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use super::{exec, prompt, tools, user_message, SessionCore, State};
use crate::{peers, util};

const DELTA_BATCH: Duration = Duration::from_millis(30);
const PERSIST_EVERY: Duration = Duration::from_secs(3);
const MAX_REMINDERS: u32 = 3;
const TITLE_TIMEOUT: Duration = Duration::from_secs(60);

/// Everything needed for one LLM step.
pub struct StepEnv {
    pub cfg: Arc<Config>,
    pub spec: ModelSpec,
    pub agent: String,
    pub mcp: Option<Arc<McpManager>>,
    pub mcp_names: HashSet<String>,
    pub session_tools: HashSet<String>,
    pub request: Request,
}

pub(crate) struct Aborted;

impl State {
    /// Mark the session busy; returns the work token.
    pub fn begin_work(&mut self, hub: &crate::hub::Hub) -> CancellationToken {
        if let Some(t) = &self.turn {
            return t.clone();
        }
        let t = CancellationToken::new();
        self.turn = Some(t.clone());
        self.set_status(hub, SessionStatus::Busy);
        t
    }
}

pub fn spawn_driver(core: &Arc<SessionCore>) {
    let c = core.clone();
    let h = tokio::spawn(async move {
        driver(c).await;
    });
    core.set_driver(h);
}

async fn driver(core: Arc<SessionCore>) {
    loop {
        let notified = core.wake.notified();
        let work = {
            let mut st = core.st.lock();
            if st.turn.as_ref().is_some_and(|t| t.is_cancelled()) {
                // Aborted before the driver picked the work up.
                st.turn = None;
                st.set_status(&core.hub, SessionStatus::Idle);
                core.idle.notify_waiters();
            }
            if !st.closing && st.turn.is_none() && !st.queue.is_empty() {
                st.drain_queue();
                st.run_requested = true;
            }
            if st.closing {
                None
            } else if st.run_requested || st.compact_requested {
                let tok = st.begin_work(&core.hub);
                let run = std::mem::take(&mut st.run_requested);
                let compact = std::mem::take(&mut st.compact_requested);
                Some((tok, run, compact))
            } else {
                if st.turn.take().is_some() {
                    st.set_status(&core.hub, SessionStatus::Idle);
                    core.idle.notify_waiters();
                }
                None
            }
        };
        match work {
            Some((tok, run, compact)) => {
                if compact && !tok.is_cancelled() {
                    manual_compaction(&core, &tok).await;
                }
                if run && !tok.is_cancelled() {
                    run_turn(&core, &tok).await;
                }
                finish_work(&core);
            }
            None => {
                if core.closed.is_cancelled() || core.st.lock().closing {
                    break;
                }
                tokio::select! {
                    _ = notified => {}
                    _ = core.closed.cancelled() => break,
                }
            }
        }
    }
    let mut st = core.st.lock();
    st.turn = None;
    drop(st);
    core.idle.notify_waiters();
}

/// End of a work item: go idle, or start a new turn for inputs that arrived meanwhile.
fn finish_work(core: &Arc<SessionCore>) {
    {
        let mut st = core.st.lock();
        st.turn = None;
        if !st.queue.is_empty() && !st.closing {
            st.drain_queue();
            st.run_requested = true;
            st.begin_work(&core.hub);
            st.info_changed(&core.hub);
        } else if !st.run_requested && !st.compact_requested {
            st.set_status(&core.hub, SessionStatus::Idle);
        }
    }
    core.persist();
    core.idle.notify_waiters();
}

fn builtin_defs() -> &'static [ToolDef] {
    static DEFS: OnceLock<Vec<ToolDef>> = OnceLock::new();
    DEFS.get_or_init(|| ref_tools::builtin_defs().into_iter().map(prompt::with_purpose).collect())
}

/// Build the request and execution environment for the next step.
pub(crate) async fn prepare(core: &Arc<SessionCore>) -> Result<StepEnv, String> {
    let hub = &core.hub;
    let (cwd, agent, model, variant) = {
        let st = core.st.lock();
        (st.info.cwd.clone(), st.info.agent.clone(), st.info.model.clone(), st.info.variant.clone())
    };
    let cfg = hub.config(&cwd).map_err(|e| format!("Configuration error: {e:#}"))?;
    let spec = cfg.resolve_for_agent(&agent, &model, variant.as_deref()).map_err(|e| format!("Model error: {e:#}"))?;
    let cached = core.st.lock().system.clone();
    let (system, skills) = match cached {
        Some(s) => (s, core.st.lock().skills.clone()),
        None => {
            let instructions = ref_config::instructions(&cfg, std::path::Path::new(&cwd));
            let skills = Arc::new(ref_config::skills(&cfg, std::path::Path::new(&cwd)));
            let mut st = core.st.lock();
            let blocks = prompt::system_blocks(&prompt::PromptInputs {
                cwd: &cwd,
                created: st.info.created,
                session_id: &st.info.id,
                session_name: &st.info.name,
                agent: &agent,
                config: &cfg,
                instructions: &instructions,
                skills: &skills,
            });
            let blocks = Arc::new(blocks);
            st.system = Some(blocks.clone());
            st.skills = skills.clone();
            (blocks, skills)
        }
    };
    let can_spawn = cfg.agents.get(&agent).is_none_or(|a| a.can_spawn);
    let mut defs: Vec<ToolDef> = builtin_defs().to_vec();
    let sdefs = tools::defs(can_spawn, !skills.is_empty());
    let session_tools: HashSet<String> = sdefs.iter().map(|d| d.name.clone()).collect();
    defs.extend(sdefs.into_iter().map(prompt::with_purpose));
    let mcp = hub.mcp_for(&cfg).await;
    let mut mcp_names = HashSet::new();
    if let Some(m) = &mcp {
        let taken: HashSet<String> = defs.iter().map(|d| d.name.clone()).collect();
        for t in m.tools() {
            if taken.contains(&t.name) {
                continue;
            }
            mcp_names.insert(t.name.clone());
            defs.push(prompt::with_purpose(t.def));
        }
    }
    let messages = {
        let st = core.st.lock();
        let start = prompt::context_start(&st.messages);
        st.messages[start..].to_vec()
    };
    Ok(StepEnv {
        request: Request {
            system: system.as_ref().clone(),
            messages,
            tools: defs,
            session_id: core.id.clone(),
            tool_choice_none: false,
        },
        cfg,
        spec,
        agent,
        mcp,
        mcp_names,
        session_tools,
    })
}

/// Show `Retrying` and sleep the backoff. Returns Err if aborted.
pub(crate) async fn wait_retry(
    core: &Arc<SessionCore>,
    tok: &CancellationToken,
    attempt: u32,
    retry_after: Option<Duration>,
    error: String,
    max: Duration,
) -> Result<(), Aborted> {
    let delay = util::backoff(attempt, retry_after, max);
    let at = ref_proto::now_ms() + delay.as_millis() as i64;
    tracing::warn!("session {}: attempt {attempt} failed: {error}; retrying in {:.1}s", core.id, delay.as_secs_f64());
    {
        let mut st = core.st.lock();
        st.set_status(&core.hub, SessionStatus::Retrying { attempt, at, error: util::ellipsize(&error, 2000) });
    }
    let woken = core.retry_wake.notified();
    let r = tokio::select! {
        _ = tokio::time::sleep(delay) => Ok(()),
        _ = woken => Ok(()),
        _ = tok.cancelled() => Err(Aborted),
    };
    if r.is_ok() {
        let mut st = core.st.lock();
        st.set_status(&core.hub, SessionStatus::Busy);
    }
    r
}

enum StepResult {
    Finished { msg_id: String, has_tools: bool },
    Aborted,
    Failed(LlmError),
}

/// Apply step-boundary changes: compaction from the `compact` tool and queued inputs.
fn step_boundary(core: &Arc<SessionCore>) {
    let mut st = core.st.lock();
    if let Some(summary) = st.compact_summary.take() {
        let m = user_message(MessageOrigin::Compaction, prompt::compaction_text(&summary, true));
        st.upsert_message(&core.hub, m);
        st.info.context.used = st.estimate_context();
        st.info_changed(&core.hub);
    }
    if st.drain_queue() > 0 {
        st.info_changed(&core.hub);
    }
}

fn needs_auto_compaction(core: &SessionCore, env: &StepEnv) -> bool {
    if !env.cfg.compaction.auto || env.request.messages.len() < 3 {
        return false;
    }
    let used = core.st.lock().info.context.used;
    let max = env.spec.context;
    max > 0 && used > 0 && used + env.cfg.reserved_tokens(&env.spec) >= max
}

async fn run_turn(core: &Arc<SessionCore>, tok: &CancellationToken) {
    let mut attempt: u32 = 0;
    let mut overflow_compacted = false;
    loop {
        if tok.is_cancelled() {
            return;
        }
        step_boundary(core);
        let manual = std::mem::take(&mut core.st.lock().compact_requested);
        let env = match prepare(core).await {
            Ok(env) => env,
            Err(e) => {
                attempt += 1;
                let max = core.hub.config(&core.cwd()).map(|c| c.retry_max_delay).unwrap_or(Duration::from_secs(600));
                if wait_retry(core, tok, attempt, None, e, max).await.is_err() {
                    return;
                }
                continue;
            }
        };
        if manual || needs_auto_compaction(core, &env) {
            if compact_context(core, tok, &env, true).await.is_err() {
                return;
            }
            continue;
        }
        match stream_step(core, tok, &env).await {
            StepResult::Aborted => return,
            StepResult::Failed(e) => {
                if e.is_context_overflow() && !overflow_compacted && env.request.messages.len() > 1 {
                    overflow_compacted = true;
                    tracing::info!("session {}: context overflow; compacting", core.id);
                    if compact_context(core, tok, &env, true).await.is_err() {
                        return;
                    }
                    continue;
                }
                attempt += 1;
                if wait_retry(core, tok, attempt, e.retry_after(), e.to_string(), env.cfg.retry_max_delay).await.is_err()
                {
                    return;
                }
            }
            StepResult::Finished { msg_id, has_tools } => {
                attempt = 0;
                overflow_compacted = false;
                if has_tools {
                    exec::run_tools(core, tok, &env, &msg_id).await;
                    if tok.is_cancelled() {
                        let mut st = core.st.lock();
                        if let Some(m) = st.msg_mut(&msg_id) {
                            m.finish = Some("aborted".into());
                        }
                        st.emit_message(&core.hub, &msg_id);
                    }
                    core.persist();
                    continue;
                }
                if !turn_end_continue(core).await {
                    return;
                }
            }
        }
    }
}

/// At the end of a turn (no tool calls): inject a must_report reminder or queued inputs.
/// Returns true if the turn continues.
async fn turn_end_continue(core: &Arc<SessionCore>) -> bool {
    let (must_report, parent, reported, reminders) = {
        let st = core.st.lock();
        (st.info.must_report, st.info.parent.clone(), st.reported, st.reminders)
    };
    if must_report && !reported && reminders < MAX_REMINDERS {
        if let Some(p) = parent.and_then(|p| core.hub.session(&p)) {
            let (pid, pname) = (p.id.clone(), p.name());
            let mut st = core.st.lock();
            if !st.reported {
                st.reminders += 1;
                let m = user_message(MessageOrigin::Reminder, peers::reminder_text(&pname, &pid));
                st.upsert_message(&core.hub, m);
                st.info_changed(&core.hub);
                return true;
            }
        }
    }
    let st = core.st.lock();
    if st.compact_summary.is_some() || !st.queue.is_empty() {
        return true;
    }
    false
}

/// Kinds of parts that receive streamed deltas.
#[derive(Clone, Copy, PartialEq)]
enum DeltaKind {
    Text,
    Reasoning,
    ToolRaw,
}

/// Coalesces stream deltas and applies them to the state + broadcasts in ~30ms batches.
struct Deltas {
    msg_id: String,
    pending: Vec<(String, DeltaKind, String)>,
    deadline: Option<Instant>,
}

impl Deltas {
    fn push(&mut self, part_id: &str, kind: DeltaKind, text: &str) {
        if text.is_empty() {
            return;
        }
        match self.pending.last_mut() {
            Some((p, k, s)) if p == part_id && *k == kind => s.push_str(text),
            _ => self.pending.push((part_id.to_string(), kind, text.to_string())),
        }
        if self.deadline.is_none() {
            self.deadline = Some(Instant::now() + DELTA_BATCH);
        }
    }

    fn flush(&mut self, core: &SessionCore, st: &mut State) {
        self.deadline = None;
        if self.pending.is_empty() {
            return;
        }
        let sid = core.id.clone();
        for (part_id, kind, text) in self.pending.drain(..) {
            let Some(p) = st.part_mut(&self.msg_id, &part_id) else { continue };
            match (p, kind) {
                (Part::Text(t), DeltaKind::Text) => t.text.push_str(&text),
                (Part::Reasoning(r), DeltaKind::Reasoning) => r.text.push_str(&text),
                (Part::Tool(t), DeltaKind::ToolRaw) => t.input_raw.push_str(&text),
                _ => continue,
            }
            st.bcast(
                &core.hub,
                &ServerMsg::PartDelta { session_id: sid.clone(), message_id: self.msg_id.clone(), part_id, delta: text },
            );
        }
        st.mark_dirty(&self.msg_id);
    }
}

async fn stream_step(core: &Arc<SessionCore>, tok: &CancellationToken, env: &StepEnv) -> StepResult {
    let hub = &core.hub;
    let mut msg = Message::new(Role::Assistant, MessageOrigin::User);
    msg.model = Some(env.spec.full_id());
    msg.variant = core.st.lock().info.variant.clone();
    msg.agent = Some(env.agent.clone());
    let msg_id = msg.id.clone();
    core.st.lock().upsert_message(hub, msg);

    let started = tokio::select! {
        r = ref_llm::stream(&hub.http, &env.spec, &env.request) => r,
        _ = tok.cancelled() => {
            finalize_aborted(core, &msg_id);
            return StepResult::Aborted;
        }
    };
    let mut events = match started {
        Ok(s) => s,
        Err(e) => {
            core.st.lock().remove_message(hub, &msg_id);
            return StepResult::Failed(e);
        }
    };
    let mut deltas = Deltas { msg_id: msg_id.clone(), pending: Vec::new(), deadline: None };
    // stream index → (part id, kind)
    let mut parts: HashMap<usize, (String, DeltaKind)> = HashMap::new();
    let mut finish: Option<FinishReason> = None;
    let mut usage: Option<ref_proto::Usage> = None;
    let mut last_persist = Instant::now();
    let failure: Option<LlmError> = loop {
        let flush_at = deltas.deadline;
        let ev = tokio::select! {
            ev = events.next() => ev,
            _ = tok.cancelled() => {
                let mut st = core.st.lock();
                deltas.flush(core, &mut st);
                drop(st);
                finalize_aborted(core, &msg_id);
                return StepResult::Aborted;
            }
            _ = async { match flush_at { Some(t) => tokio::time::sleep_until(t).await, None => std::future::pending().await } } => {
                let mut st = core.st.lock();
                deltas.flush(core, &mut st);
                drop(st);
                if last_persist.elapsed() >= PERSIST_EVERY {
                    last_persist = Instant::now();
                    core.persist();
                }
                continue;
            }
        };
        let ev = match ev {
            None => break None,
            Some(Err(e)) => break Some(e),
            Some(Ok(ev)) => ev,
        };
        match ev {
            StreamEvent::TextDelta { index, text } | StreamEvent::ReasoningDelta { index, text }
                if parts.contains_key(&index) =>
            {
                let (pid, kind) = parts[&index].clone();
                deltas.push(&pid, kind, &text);
            }
            StreamEvent::TextDelta { index, text } => {
                let part = Part::Text(TextPart { id: ref_proto::new_id("prt"), text, time: ref_proto::now_ms(), meta: None });
                parts.insert(index, (part.id().to_string(), DeltaKind::Text));
                let mut st = core.st.lock();
                deltas.flush(core, &mut st);
                st.upsert_part(hub, &msg_id, part);
            }
            StreamEvent::ReasoningDelta { index, text } => {
                let part = Part::Reasoning(ReasoningPart {
                    id: ref_proto::new_id("prt"),
                    text,
                    time: ref_proto::now_ms(),
                    time_end: None,
                    meta: None,
                });
                parts.insert(index, (part.id().to_string(), DeltaKind::Reasoning));
                let mut st = core.st.lock();
                deltas.flush(core, &mut st);
                st.upsert_part(hub, &msg_id, part);
            }
            StreamEvent::TextEnd { index, meta } | StreamEvent::ReasoningEnd { index, meta } => {
                let mut st = core.st.lock();
                deltas.flush(core, &mut st);
                match parts.get(&index).cloned() {
                    Some((pid, _)) => st.update_part(hub, &msg_id, &pid, |p| match p {
                        Part::Text(t) => t.meta = meta,
                        Part::Reasoning(r) => {
                            r.meta = meta;
                            r.time_end = Some(ref_proto::now_ms());
                        }
                        _ => {}
                    }),
                    None if meta.is_some() => {
                        // Block without deltas (e.g. redacted reasoning): keep its replay data.
                        let part = Part::Reasoning(ReasoningPart {
                            id: ref_proto::new_id("prt"),
                            text: String::new(),
                            time: ref_proto::now_ms(),
                            time_end: Some(ref_proto::now_ms()),
                            meta,
                        });
                        parts.insert(index, (part.id().to_string(), DeltaKind::Reasoning));
                        st.upsert_part(hub, &msg_id, part);
                    }
                    None => {}
                }
            }
            StreamEvent::ToolCallStart { index, call_id, name } => {
                let part = Part::Tool(new_tool_part(call_id, name));
                parts.insert(index, (part.id().to_string(), DeltaKind::ToolRaw));
                let mut st = core.st.lock();
                deltas.flush(core, &mut st);
                st.upsert_part(hub, &msg_id, part);
            }
            StreamEvent::ToolCallDelta { index, json } => {
                if let Some((pid, _)) = parts.get(&index) {
                    deltas.push(&pid.clone(), DeltaKind::ToolRaw, &json);
                }
            }
            StreamEvent::ToolCallEnd { index, call_id, name, input, meta } => {
                let mut st = core.st.lock();
                deltas.flush(core, &mut st);
                let pid = match parts.get(&index) {
                    Some((pid, _)) => pid.clone(),
                    None => {
                        let part = Part::Tool(new_tool_part(call_id.clone(), name.clone()));
                        let pid = part.id().to_string();
                        parts.insert(index, (pid.clone(), DeltaKind::ToolRaw));
                        st.upsert_part(hub, &msg_id, part);
                        pid
                    }
                };
                st.update_part(hub, &msg_id, &pid, |p| {
                    if let Part::Tool(t) = p {
                        t.call_id = call_id;
                        t.name = name;
                        t.meta = meta;
                        match input {
                            Value::Object(_) => {
                                let (purpose, _) = prompt::split_purpose(&input);
                                t.purpose = purpose;
                                t.input = input;
                                t.input_raw.clear();
                            }
                            Value::String(raw) => {
                                // Invalid JSON: keep the raw text; the call fails with an error.
                                if t.input_raw.is_empty() {
                                    t.input_raw = raw;
                                }
                                t.input = Value::Object(Default::default());
                            }
                            other => {
                                t.input_raw = other.to_string();
                                t.input = Value::Object(Default::default());
                            }
                        }
                    }
                });
            }
            StreamEvent::Usage(u) => usage = Some(u),
            StreamEvent::Finish { reason } => finish = Some(reason),
        }
    };
    let mut st = core.st.lock();
    deltas.flush(core, &mut st);
    if let Some(e) = failure {
        st.remove_message(hub, &msg_id);
        return StepResult::Failed(e);
    }
    let Some(i) = st.msg_index(&msg_id) else { return StepResult::Aborted };
    let has_tools = st.messages[i].parts.iter().any(|p| matches!(p, Part::Tool(_)));
    let m = &mut st.messages[i];
    m.time_end = Some(ref_proto::now_ms());
    m.finish = Some(
        match finish {
            Some(FinishReason::ToolUse) => "tool_use",
            _ if has_tools => "tool_use",
            Some(FinishReason::Length) => "length",
            Some(FinishReason::Error) => "error",
            Some(FinishReason::Stop) | None => "stop",
        }
        .to_string(),
    );
    if finish == Some(FinishReason::Error) && !has_tools {
        m.error = Some("The provider stopped the response with an error (refusal or content filter)".into());
    }
    if let Some(u) = &usage {
        m.usage = Some(u.clone());
    }
    st.emit_message(hub, &msg_id);
    if let Some(u) = usage {
        st.info.cost += u.cost;
        let used = u.context_tokens();
        st.info.context.used = if used > 0 { used } else { st.estimate_context() };
    } else {
        st.info.context.used = st.estimate_context();
    }
    st.info_changed(hub);
    drop(st);
    core.persist();
    StepResult::Finished { msg_id, has_tools }
}

use serde_json::Value;

fn new_tool_part(call_id: String, name: String) -> ToolPart {
    ToolPart {
        id: ref_proto::new_id("prt"),
        call_id,
        name,
        purpose: String::new(),
        input: Value::Null,
        input_raw: String::new(),
        state: ToolState::Streaming,
        output: String::new(),
        images: Vec::new(),
        time: ref_proto::now_ms(),
        time_start: None,
        time_end: None,
        run_at: None,
        meta: None,
    }
}

pub const ABORTED_OUTPUT: &str = "Aborted by user";

/// Mark an assistant message as aborted: non-final tool parts become Cancelled.
pub(crate) fn finalize_aborted(core: &SessionCore, msg_id: &str) {
    let mut st = core.st.lock();
    let Some(m) = st.msg_mut(msg_id) else { return };
    if m.parts.is_empty() {
        st.remove_message(&core.hub, msg_id);
        return;
    }
    let now = ref_proto::now_ms();
    for p in &mut m.parts {
        if let Part::Tool(t) = p {
            if !t.state.is_final() {
                t.state = ToolState::Cancelled;
                t.output = ABORTED_OUTPUT.into();
                t.time_end = Some(now);
                if t.input.is_null() {
                    t.input = ref_llm::parse_tool_input(&t.input_raw);
                    if !t.input.is_object() {
                        t.input = Value::Object(Default::default());
                    }
                }
            }
        }
    }
    if m.finish.is_none() {
        m.finish = Some("aborted".into());
    }
    if m.time_end.is_none() {
        m.time_end = Some(now);
    }
    st.emit_message(&core.hub, msg_id);
}

/// One tool-less LLM call returning the response text.
async fn collect_text(core: &SessionCore, spec: &ModelSpec, req: &Request, tok: &CancellationToken) -> Result<String, Option<LlmError>> {
    let fut = async {
        let mut events = ref_llm::stream(&core.hub.http, spec, req).await?;
        let mut text = String::new();
        while let Some(ev) = events.next().await {
            if let StreamEvent::TextDelta { text: t, .. } = ev? {
                text.push_str(&t);
            }
        }
        Ok::<String, LlmError>(text)
    };
    tokio::select! {
        r = fut => r.map_err(Some),
        _ = tok.cancelled() => Err(None),
    }
}

/// Summarize the current context and start a new one with the summary.
/// `continue_task`: append "Continue the task from here." (mid-turn compaction).
pub(crate) async fn compact_context(
    core: &Arc<SessionCore>,
    tok: &CancellationToken,
    env: &StepEnv,
    continue_task: bool,
) -> Result<(), Aborted> {
    let hub = &core.hub;
    let mut history = env.request.messages.clone();
    // A trailing user prompt is kept verbatim after the summary instead of being summarized.
    let trailing = match history.last() {
        Some(m) if m.role == Role::User && m.origin != MessageOrigin::Compaction && history.len() > 1 => history.pop(),
        _ => None,
    };
    if history.is_empty() {
        return Ok(());
    }
    core.notice(NoticeLevel::Info, "Compacting context…");
    let mut attempt = 0;
    let mut drop_front = 0usize;
    let summary = loop {
        let mut msgs: Vec<Message> = history[drop_front.min(history.len().saturating_sub(1))..].to_vec();
        msgs.push(user_message(MessageOrigin::User, prompt::SUMMARY_INSTRUCTION.to_string()));
        let req = Request {
            system: env.request.system.clone(),
            messages: msgs,
            tools: env.request.tools.clone(),
            session_id: env.request.session_id.clone(),
            tool_choice_none: true,
        };
        match collect_text(core, &env.spec, &req, tok).await {
            Ok(t) if !t.trim().is_empty() => break t,
            Ok(_) => {
                attempt += 1;
                if wait_retry(core, tok, attempt, None, "Compaction returned an empty summary".into(), env.cfg.retry_max_delay)
                    .await
                    .is_err()
                {
                    return Err(Aborted);
                }
            }
            Err(None) => return Err(Aborted),
            Err(Some(e)) => {
                if e.is_context_overflow() && history.len() - drop_front > 2 {
                    // Summarize less: drop the oldest quarter.
                    drop_front += ((history.len() - drop_front) / 4).max(1);
                    continue;
                }
                attempt += 1;
                if wait_retry(core, tok, attempt, e.retry_after(), format!("Compaction failed: {e}"), env.cfg.retry_max_delay)
                    .await
                    .is_err()
                {
                    return Err(Aborted);
                }
            }
        }
    };
    let mut st = core.st.lock();
    st.set_status(hub, SessionStatus::Busy);
    if let Some(t) = &trailing {
        st.remove_message(hub, &t.id);
    }
    let m = user_message(MessageOrigin::Compaction, prompt::compaction_text(&summary, continue_task && trailing.is_none()));
    st.upsert_message(hub, m);
    if let Some(mut t) = trailing {
        t.time = ref_proto::now_ms();
        st.upsert_message(hub, t);
    }
    st.info.context.used = st.estimate_context();
    st.info_changed(hub);
    drop(st);
    core.persist();
    core.notice(NoticeLevel::Success, "Context compacted");
    Ok(())
}

async fn manual_compaction(core: &Arc<SessionCore>, tok: &CancellationToken) {
    let empty = {
        let st = core.st.lock();
        let start = prompt::context_start(&st.messages);
        st.messages.len() <= start + 1
    };
    if empty {
        core.notice(NoticeLevel::Info, "Nothing to compact");
        return;
    }
    let mut attempt = 0;
    let env = loop {
        match prepare(core).await {
            Ok(env) => break env,
            Err(e) => {
                attempt += 1;
                if wait_retry(core, tok, attempt, None, e, Duration::from_secs(600)).await.is_err() {
                    return;
                }
            }
        }
    };
    let _ = compact_context(core, tok, &env, false).await;
}

/// Generate a session title from the first prompt (background, never fails loudly).
pub(crate) async fn generate_title(core: Arc<SessionCore>, first: String) {
    let (cwd, agent, model, variant) = {
        let st = core.st.lock();
        (st.info.cwd.clone(), st.info.agent.clone(), st.info.model.clone(), st.info.variant.clone())
    };
    let Ok(cfg) = core.hub.config(&cwd) else { return };
    let spec = cfg
        .small_model
        .as_deref()
        .and_then(|m| cfg.resolve_model(m, None).ok())
        .or_else(|| cfg.resolve_for_agent(&agent, &model, variant.as_deref()).ok());
    let Some(spec) = spec else { return };
    let text = util::ellipsize(&first, 4000);
    let res = tokio::select! {
        r = tokio::time::timeout(TITLE_TIMEOUT, ref_llm::complete_text(&core.hub.http, &spec, prompt::TITLE_SYSTEM, &text)) => r,
        _ = core.closed.cancelled() => return,
    };
    let title = match res {
        Ok(Ok(t)) => prompt::clean_title(&t),
        Ok(Err(e)) => {
            tracing::debug!("session {}: title generation failed: {e}", core.id);
            None
        }
        Err(_) => None,
    };
    let Some(title) = title else { return };
    if !core.st.lock().auto_name {
        return; // renamed meanwhile
    }
    match core.rename(&title, false, false) {
        Ok(name) => tracing::debug!("session {} titled {name:?}", core.id),
        Err(e) => tracing::debug!("session {}: title rename failed: {e}", core.id),
    }
}
