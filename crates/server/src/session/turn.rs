//! The agent loop for one turn: steps (LLM call + tool calls) until the model stops.

use super::history;
use super::tools::{self as stools, CallSpec, StepCtx};
use super::{cancel_open_tools, Input, Session};
use crate::hub::Hub;
use hn_config::Config;
use hn_llm::{ChatRequest, ErrorKind, LlmError, ModelSpec, StreamEvent, ToolDef};
use hn_proto::*;
use hn_tools::Tool;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const MAX_REMINDERS: u32 = 3;
const MAX_OVERFLOW: u32 = 4;

pub async fn run_turn(s: &Arc<Session>, hub: &Arc<Hub>) {
    let cancel = CancellationToken::new();
    {
        let mut st = s.lock();
        st.busy = true;
        st.cancel = Some(cancel.clone());
        st.retry = None;
        s.refresh_status(&mut st);
    }
    s.busy_tx.send_replace(true);
    let mut t = Turn { s: s.clone(), hub: hub.clone(), cancel: cancel.clone(), reminders: 0, overflow: 0, attempt: 0 };
    t.run().await;
    {
        let mut st = s.lock();
        st.busy = false;
        st.cancel = None;
        st.retry = None;
        st.perms.clear();
        st.checkpoint = None;
        if cancel.is_cancelled() {
            st.paused = true;
        }
        s.refresh_status(&mut st);
        s.info_changed(&mut st);
    }
    s.busy_tx.send_replace(false);
}

/// Per-step environment: config, model, tools, system prompt.
pub struct Env {
    pub config: Arc<Config>,
    pub spec: ModelSpec,
    pub model: ModelRef,
    pub agent: String,
    pub cwd: String,
    pub tools: Vec<Arc<dyn Tool>>,
    pub defs: Vec<ToolDef>,
    pub system: Vec<String>,
}

pub struct Turn {
    pub s: Arc<Session>,
    pub hub: Arc<Hub>,
    pub cancel: CancellationToken,
    reminders: u32,
    overflow: u32,
    attempt: u32,
}

enum Step {
    Tools(Vec<CallSpec>),
    Done,
    Again,
    Stop,
}

impl Turn {
    async fn run(&mut self) {
        // True when the history ends in something the model has not answered yet.
        let mut need_step = false;
        loop {
            if self.cancel.is_cancelled() || self.s.is_closed() {
                return;
            }
            if self.drain_inputs() > 0 {
                need_step = true;
            }
            let compact = std::mem::take(&mut self.s.lock().pending_compact);
            if compact {
                if let Some(env) = self.env().await {
                    self.compact(&env, true).await;
                }
                if self.cancel.is_cancelled() {
                    return;
                }
            }
            if !need_step {
                return;
            }
            let Some(env) = self.env().await else { return };
            // Hard limit: compact before the request would overflow.
            let (used, max) = {
                let st = self.s.lock();
                (history::used_tokens(&st.msgs), env.config.context_limits(&env.spec, &env.model).0)
            };
            let reserve = 16_000u64.min(max * 15 / 100);
            if env.config.compaction.auto && max > 0 && used >= max.saturating_sub(reserve) {
                tracing::info!("session {}: context {used}/{max}, auto-compacting", self.s.id);
                self.compact(&env, false).await;
                if self.cancel.is_cancelled() {
                    return;
                }
            }
            match self.step(&env).await {
                Step::Tools(calls) => {
                    self.run_tools(&env, calls).await;
                    self.apply_checkpoint(&env.agent);
                }
                Step::Again => {}
                Step::Stop => return,
                Step::Done => {
                    need_step = false;
                    if self.cancel.is_cancelled() {
                        return;
                    }
                    let queued = !self.s.lock().queue.is_empty();
                    if !queued && self.reminders < MAX_REMINDERS {
                        if let Some(text) = self.report_reminder() {
                            self.reminders += 1;
                            let msg = Input::System(text).into_message(&env.agent);
                            let mut st = self.s.lock();
                            self.s.push_msg(&mut st, msg);
                            need_step = true;
                        }
                    }
                }
            }
        }
    }

    /// Append queued inputs as user messages. Returns how many were added.
    fn drain_inputs(&self) -> usize {
        let mut st = self.s.lock();
        let inputs: Vec<Input> = st.queue.drain(..).collect();
        let n = inputs.len();
        let agent = st.info.agent.clone();
        for input in inputs {
            let title_text = match &input {
                Input::Human { text, .. } if !st.meta.titled && !st.meta.name_explicit && !text.trim().is_empty() => {
                    Some(text.clone())
                }
                Input::Spawn { text, .. } if !st.meta.titled && !st.meta.name_explicit => Some(text.clone()),
                _ => None,
            };
            let msg = input.into_message(&agent);
            self.s.push_msg(&mut st, msg);
            if let Some(text) = title_text {
                st.meta.titled = true;
                let (s, hub) = (self.s.clone(), self.hub.clone());
                tokio::spawn(async move { crate::title::generate(s, hub, text).await });
            }
        }
        n
    }

    /// Resolve config/model/tools/system for the next request. On failure the turn ends with
    /// an error message.
    pub async fn env(&self) -> Option<Env> {
        let info = self.s.info();
        let config = self.hub.config(&info.cwd).await;
        let spec = match config.resolve_model(&info.model) {
            Ok(s) => s,
            Err(e) => {
                self.fatal(&format!("Cannot use model {}: {e:#}", info.model), None);
                return None;
            }
        };
        let retry_mcp = {
            let mut st = self.s.lock();
            !std::mem::replace(&mut st.mcp_tried, true)
        };
        let mcp = self.hub.mcp.session_tools(&self.s, &self.hub, &config, &info.cwd, retry_mcp).await;
        let tools = crate::peer::session_tools(&self.hub, &config, &info.agent, mcp.tools);
        let defs = stools::tool_defs(&tools);
        let system = crate::prompt::system(&config, &info, &mcp.instructions, &tools);
        Some(Env { config, spec, model: info.model.clone(), agent: info.agent.clone(), cwd: info.cwd.clone(), tools, defs, system })
    }

    /// End the turn with an error: shown as an assistant message with `error`, plus a toast.
    fn fatal(&self, error: &str, msg_id: Option<&str>) {
        tracing::warn!("session {}: {error}", self.s.id);
        let mut st = self.s.lock();
        let now = now_ms();
        match msg_id.and_then(|id| st.msgs.iter().rposition(|m| m.id == id)) {
            Some(i) => {
                let m = &mut st.msgs[i];
                m.error = Some(error.to_string());
                m.completed = Some(now);
                m.finish = Some("error".into());
                cancel_open_tools(m, now);
                let m = m.clone();
                self.s.emit(&st, Event::Message { message: m });
                self.s.msgs_changed(&mut st);
            }
            None => {
                let m = Message {
                    id: crate::util::new_id("msg"),
                    role: Role::Assistant,
                    origin: Origin::Human,
                    time: now,
                    completed: Some(now),
                    parts: Vec::new(),
                    model: Some(st.info.model.clone()),
                    agent: Some(st.info.agent.clone()),
                    usage: None,
                    finish: Some("error".into()),
                    error: Some(error.to_string()),
                    replay_key: None,
                };
                self.s.push_msg(&mut st, m);
            }
        }
        let name = st.info.name.clone();
        drop(st);
        self.hub.toast_attached(&self.s.id, ToastLevel::Error, &format!("{name}: {}", crate::util::ellipsize(error, 300)));
    }

    /// One LLM request, streamed into a new assistant message.
    async fn step(&mut self, env: &Env) -> Step {
        let (messages, msg) = {
            let st = self.s.lock();
            let start = history::context_start(&st.msgs);
            let messages = history::build(&st.msgs[start..], &env.spec);
            let msg = Message {
                id: crate::util::new_id("msg"),
                role: Role::Assistant,
                origin: Origin::Human,
                time: now_ms(),
                completed: None,
                parts: Vec::new(),
                model: Some(env.model.clone()),
                agent: Some(env.agent.clone()),
                usage: None,
                finish: None,
                error: None,
                replay_key: Some(env.spec.replay_key()),
            };
            (messages, msg)
        };
        let req = ChatRequest {
            system: env.system.clone(),
            messages,
            tools: env.defs.clone(),
            session_id: self.s.id.clone(),
            max_tokens: None,
        };
        let msg_id = msg.id.clone();
        {
            let mut st = self.s.lock();
            self.s.push_msg(&mut st, msg);
        }
        let mut sink = StreamSink { s: &self.s, msg_id: msg_id.clone(), tools: HashMap::new(), usage: None };
        let result = {
            let mut on_event = |ev: StreamEvent| sink.apply(ev);
            crate::llm::stream(&self.hub.http, &env.spec, &req, &self.cancel, &mut on_event).await
        };
        let usage = sink.usage.take();
        match result {
            Ok(finish) => {
                self.attempt = 0;
                self.overflow = 0;
                let calls = self.finalize(env, &msg_id, finish.as_str(), usage);
                if calls.is_empty() {
                    Step::Done
                } else {
                    Step::Tools(calls)
                }
            }
            Err(e) if e.kind == ErrorKind::Aborted || self.cancel.is_cancelled() => {
                self.finalize(env, &msg_id, "aborted", usage);
                Step::Stop
            }
            Err(e) => match e.kind {
                ErrorKind::Retryable => {
                    self.discard(&msg_id);
                    if self.wait_retry(env, &e).await {
                        Step::Again
                    } else {
                        Step::Stop
                    }
                }
                ErrorKind::ContextOverflow => {
                    self.discard(&msg_id);
                    self.overflow += 1;
                    if self.overflow > MAX_OVERFLOW {
                        self.fatal(&format!("Context window exceeded even after compaction: {}", e.message), None);
                        return Step::Stop;
                    }
                    tracing::info!("session {}: context overflow ({}), compacting", self.s.id, e.message);
                    if self.overflow == 1 {
                        self.compact(env, false).await;
                    } else {
                        self.drop_oldest(env, "the context window was exceeded");
                    }
                    if self.cancel.is_cancelled() {
                        Step::Stop
                    } else {
                        Step::Again
                    }
                }
                _ => {
                    self.fatal(&e.message, Some(&msg_id));
                    Step::Stop
                }
            },
        }
    }

    /// Complete the assistant message: finish reason, usage, context, cost. Returns tool calls.
    fn finalize(&self, env: &Env, msg_id: &str, finish: &str, usage: Option<Usage>) -> Vec<CallSpec> {
        let now = now_ms();
        let mut st = self.s.lock();
        let Some(i) = st.msgs.iter().rposition(|m| m.id == msg_id) else { return Vec::new() };
        let aborted = finish == "aborted";
        let mut calls = Vec::new();
        {
            let m = &mut st.msgs[i];
            m.completed = Some(now);
            m.finish = Some(finish.to_string());
            if usage.is_some() {
                m.usage = usage.clone();
            }
            for p in m.parts.iter_mut() {
                if let Part::Reasoning { time_end, .. } = p {
                    time_end.get_or_insert(now);
                }
            }
            if aborted {
                cancel_open_tools(m, now);
            } else {
                for (index, p) in m.parts.iter().enumerate() {
                    if let Part::Tool(tp) = p {
                        calls.push(CallSpec {
                            msg_id: msg_id.to_string(),
                            index,
                            call_id: tp.call_id.clone(),
                            name: tp.name.clone(),
                            raw_input: tp.raw_input.clone(),
                        });
                    }
                }
            }
            let m = m.clone();
            self.s.emit(&st, Event::Message { message: m });
        }
        if let Some(u) = &usage {
            let (max, compact_at) = env.config.context_limits(&env.spec, &env.model);
            st.info.context = ContextInfo { used: u.context_tokens(), max, compact_at };
            st.info.cost += crate::cost::cost(&env.config, &env.model, u);
        }
        st.info.updated_at = now;
        self.s.msgs_changed(&mut st);
        self.s.info_changed(&mut st);
        calls
    }

    /// Remove a failed partial assistant message before retrying.
    fn discard(&self, msg_id: &str) {
        let mut st = self.s.lock();
        self.s.remove_msgs(&mut st, vec![msg_id.to_string()]);
    }

    /// Back off before retrying. Returns false if aborted meanwhile.
    pub async fn wait_retry(&mut self, env: &Env, e: &LlmError) -> bool {
        let delay = backoff(&env.config.retry, self.attempt, e.retry_after);
        self.attempt += 1;
        let next_at = now_ms() + delay.as_millis() as i64;
        tracing::warn!("session {}: attempt {} failed: {}; retrying in {:?}", self.s.id, self.attempt, e.message, delay);
        {
            let mut st = self.s.lock();
            st.retry = Some(SessionStatus::Retry { attempt: self.attempt, next_at, error: e.message.clone() });
            self.s.refresh_status(&mut st);
        }
        let ok = tokio::select! {
            _ = tokio::time::sleep(delay) => true,
            _ = self.cancel.cancelled() => false,
        };
        let mut st = self.s.lock();
        st.retry = None;
        self.s.refresh_status(&mut st);
        ok
    }

    async fn run_tools(&self, env: &Env, calls: Vec<CallSpec>) {
        let ctx = Arc::new(StepCtx {
            session: self.s.clone(),
            hub: self.hub.clone(),
            config: env.config.clone(),
            agent: env.agent.clone(),
            cwd: std::path::PathBuf::from(&env.cwd),
            image_input: env.spec.image_input,
            tools: env.tools.clone(),
            cancel: self.cancel.clone(),
        });
        let futs = calls.into_iter().map(|c| stools::execute(ctx.clone(), c));
        futures::future::join_all(futs).await;
    }

    /// Apply a checkpoint requested via the `compact_context` tool.
    fn apply_checkpoint(&self, agent: &str) {
        let mut st = self.s.lock();
        let Some(summary) = st.checkpoint.take() else { return };
        let text = format!(
            "<conversation-checkpoint>\nThe earlier conversation was compacted. Summary written by you before compaction:\n<summary>\n{summary}\n</summary>\n</conversation-checkpoint>\nContinue the work from here."
        );
        let mut msg = Input::System(text).into_message(agent);
        msg.origin = Origin::Compaction;
        self.s.push_msg(&mut st, msg);
        let used = history::used_tokens(&st.msgs);
        st.info.context.used = used;
        self.s.info_changed(&mut st);
    }

    /// must_report: reminder text if the session still owes its parent a message.
    fn report_reminder(&self) -> Option<String> {
        let (parent, must, reported) = {
            let st = self.s.lock();
            (st.info.parent.clone(), st.info.must_report, st.meta.reported)
        };
        if !must || reported {
            return None;
        }
        let parent = self.hub.get(&parent?)?;
        let pname = parent.info().name;
        Some(format!(
            "<system-reminder>You were asked to report back to session \"{pname}\" (id {}) but have not sent it a message yet. Use send_message to report your result before stopping.</system-reminder>",
            parent.id
        ))
    }
}

/// Exponential backoff with jitter; never below retry_after; capped at max_delay.
pub fn backoff(cfg: &hn_config::RetryConfig, attempt: u32, retry_after: Option<Duration>) -> Duration {
    use rand::Rng;
    let max = Duration::from_secs_f64((cfg.max_delay as f64).max(0.01));
    let initial = if cfg.initial_delay.is_finite() && cfg.initial_delay > 0.0 { cfg.initial_delay } else { 2.0 };
    let exp = initial * 2f64.powi(attempt.min(30) as i32);
    let jitter = rand::thread_rng().gen_range(0.8..1.2);
    let mut d = Duration::from_secs_f64((exp * jitter).min(max.as_secs_f64()));
    if let Some(ra) = retry_after {
        d = d.max(ra.min(max));
    }
    d
}

/// Applies stream events to the in-flight assistant message and emits protocol events.
struct StreamSink<'a> {
    s: &'a Session,
    msg_id: String,
    tools: HashMap<String, usize>,
    usage: Option<Usage>,
}

impl StreamSink<'_> {
    fn apply(&mut self, ev: StreamEvent) {
        let s = self.s;
        let mut st = s.lock();
        let Some(mi) = st.msgs.iter().rposition(|m| m.id == self.msg_id) else { return };
        let now = now_ms();
        let mut events: Vec<Event> = Vec::new();
        {
            let m = &mut st.msgs[mi];
            let id = m.id.clone();
            match ev {
                StreamEvent::TextDelta(d) => {
                    if d.is_empty() {
                        return;
                    }
                    close_reasoning(m, now, &mut events);
                    match m.parts.last_mut() {
                        Some(Part::Text { text, .. }) => {
                            text.push_str(&d);
                            events.push(Event::Delta { message: id, index: m.parts.len() - 1, field: DeltaField::Text, text: d });
                        }
                        _ => {
                            let part = Part::Text { text: d, time: now };
                            m.parts.push(part.clone());
                            events.push(Event::Part { message: id, index: m.parts.len() - 1, part });
                        }
                    }
                }
                StreamEvent::ReasoningDelta(d) => {
                    if d.is_empty() {
                        return;
                    }
                    match m.parts.last_mut() {
                        Some(Part::Reasoning { text, time_end: None, .. }) => {
                            text.push_str(&d);
                            events.push(Event::Delta {
                                message: id,
                                index: m.parts.len() - 1,
                                field: DeltaField::Reasoning,
                                text: d,
                            });
                        }
                        _ => {
                            let part = Part::Reasoning { text: d, time_start: now, time_end: None, replay: serde_json::Value::Null };
                            m.parts.push(part.clone());
                            events.push(Event::Part { message: id, index: m.parts.len() - 1, part });
                        }
                    }
                }
                StreamEvent::ReasoningEnd { replay } => {
                    let idx = match m.parts.last() {
                        Some(Part::Reasoning { time_end: None, .. }) => m.parts.len() - 1,
                        _ => {
                            m.parts.push(Part::Reasoning {
                                text: String::new(),
                                time_start: now,
                                time_end: None,
                                replay: serde_json::Value::Null,
                            });
                            m.parts.len() - 1
                        }
                    };
                    if let Some(Part::Reasoning { replay: r, time_end, .. }) = m.parts.get_mut(idx) {
                        *r = replay;
                        *time_end = Some(now);
                    }
                    events.push(Event::Part { message: id, index: idx, part: m.parts[idx].clone() });
                }
                StreamEvent::ToolCallStart { id: call_id, name } => {
                    close_reasoning(m, now, &mut events);
                    let part = Part::Tool(ToolPart {
                        call_id: call_id.clone(),
                        name,
                        state: ToolState::Streaming,
                        time_start: now,
                        ..Default::default()
                    });
                    m.parts.push(part.clone());
                    let idx = m.parts.len() - 1;
                    self.tools.insert(call_id, idx);
                    events.push(Event::Part { message: id, index: idx, part });
                }
                StreamEvent::ToolCallDelta { id: call_id, delta } => {
                    if let Some(&idx) = self.tools.get(&call_id) {
                        if let Some(Part::Tool(tp)) = m.parts.get_mut(idx) {
                            tp.raw_input.push_str(&delta);
                            events.push(Event::Delta { message: id, index: idx, field: DeltaField::ToolInput, text: delta });
                        }
                    }
                }
                StreamEvent::ToolCallEnd { id: call_id, arguments } => {
                    if let Some(&idx) = self.tools.get(&call_id) {
                        if let Some(Part::Tool(tp)) = m.parts.get_mut(idx) {
                            tp.raw_input = arguments;
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&tp.raw_input) {
                                tp.purpose = stools::split_purpose(&v).0;
                                tp.input = v;
                            }
                            events.push(Event::Part { message: id, index: idx, part: Part::Tool(tp.clone()) });
                        }
                    }
                }
                StreamEvent::Usage(u) => {
                    self.usage = Some(u);
                }
            }
        }
        for ev in events {
            s.emit(&st, ev);
        }
    }
}

fn close_reasoning(m: &mut Message, now: Millis, events: &mut Vec<Event>) {
    let n = m.parts.len();
    if let Some(Part::Reasoning { time_end, .. }) = m.parts.last_mut() {
        if time_end.is_none() {
            *time_end = Some(now);
            events.push(Event::Part { message: m.id.clone(), index: n - 1, part: m.parts[n - 1].clone() });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_and_caps() {
        let cfg = hn_config::RetryConfig { max_delay: 10, initial_delay: 1.0 };
        let d0 = backoff(&cfg, 0, None);
        assert!(d0 >= Duration::from_millis(800) && d0 <= Duration::from_millis(1200));
        assert!(backoff(&cfg, 20, None) <= Duration::from_secs(10));
        assert!(backoff(&cfg, 0, Some(Duration::from_secs(5))) >= Duration::from_secs(5));
        assert!(backoff(&cfg, 0, Some(Duration::from_secs(500))) <= Duration::from_secs(10));
    }
}
