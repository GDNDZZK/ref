//! Tool dispatch for one assistant step: purpose injection, permission, delay, execution.

use super::{Session, State};
use crate::hub::Hub;
use hn_config::{Action, Config};
use hn_llm::ToolDef;
use hn_proto::*;
use hn_tools::{Tool, ToolCtx, ToolOutput};
use serde_json::{Map, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub const PURPOSE_DESC: &str = "Short description of why you are calling this tool (shown to the user).";

/// Add the required `purpose` string property (first) to a tool's input schema.
pub fn inject_purpose(schema: &Value) -> Value {
    let mut obj = match schema {
        Value::Object(m) => m.clone(),
        _ => Map::new(),
    };
    obj.insert("type".into(), Value::String("object".into()));
    let mut props = Map::new();
    props.insert("purpose".into(), serde_json::json!({"type": "string", "description": PURPOSE_DESC}));
    if let Some(Value::Object(p)) = obj.get("properties") {
        for (k, v) in p {
            if k != "purpose" {
                props.insert(k.clone(), v.clone());
            }
        }
    }
    obj.insert("properties".into(), Value::Object(props));
    let mut req = vec![Value::String("purpose".into())];
    if let Some(Value::Array(r)) = obj.get("required") {
        req.extend(r.iter().filter(|v| v.as_str() != Some("purpose")).cloned());
    }
    obj.insert("required".into(), Value::Array(req));
    Value::Object(obj)
}

pub fn tool_defs(tools: &[Arc<dyn Tool>]) -> Vec<ToolDef> {
    tools
        .iter()
        .map(|t| ToolDef { name: t.name().to_string(), description: t.description(), parameters: inject_purpose(&t.parameters()) })
        .collect()
}

/// Split model input into (purpose, input without purpose).
pub fn split_purpose(input: &Value) -> (String, Value) {
    match input {
        Value::Object(m) => {
            let purpose = m.get("purpose").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
            let mut rest = m.clone();
            rest.remove("purpose");
            (purpose, Value::Object(rest))
        }
        other => (String::new(), other.clone()),
    }
}

pub struct CallSpec {
    pub msg_id: String,
    pub index: usize,
    pub call_id: String,
    pub name: String,
    pub raw_input: String,
}

/// Everything a tool call needs from the step.
pub struct StepCtx {
    pub session: Arc<Session>,
    pub hub: Arc<Hub>,
    pub config: Arc<Config>,
    pub agent: String,
    pub cwd: std::path::PathBuf,
    pub image_input: bool,
    pub tools: Vec<Arc<dyn Tool>>,
    pub cancel: CancellationToken,
}

impl Session {
    /// Update a tool part in place and emit it.
    pub(crate) fn update_tool(&self, msg_id: &str, index: usize, f: impl FnOnce(&mut ToolPart)) {
        let mut st = self.lock();
        let Some(mi) = st.msgs.iter().rposition(|m| m.id == msg_id) else { return };
        let part = {
            let Some(Part::Tool(tp)) = st.msgs[mi].parts.get_mut(index) else { return };
            f(tp);
            Part::Tool(tp.clone())
        };
        self.emit(&st, Event::Part { message: msg_id.to_string(), index, part });
        self.msgs_changed(&mut st);
    }

    /// Like [`update_tool`](Self::update_tool) for transient state (live output): emitted but
    /// not persisted on its own (the final result is).
    pub(crate) fn update_tool_quiet(&self, msg_id: &str, index: usize, f: impl FnOnce(&mut ToolPart)) {
        let st = &mut *self.lock();
        let Some(mi) = st.msgs.iter().rposition(|m| m.id == msg_id) else { return };
        let Some(Part::Tool(tp)) = st.msgs[mi].parts.get_mut(index) else { return };
        f(tp);
        let part = Part::Tool(tp.clone());
        self.emit(st, Event::Part { message: msg_id.to_string(), index, part });
    }
}

fn finish(ctx: &StepCtx, c: &CallSpec, state: ToolState, out: ToolOutput) {
    let now = now_ms();
    ctx.session.update_tool(&c.msg_id, c.index, |tp| {
        tp.state = state;
        tp.output = out.text;
        tp.images = if ctx.image_input { out.images } else { Vec::new() };
        if !out.title.is_empty() {
            tp.title = out.title;
        }
        tp.time_end = Some(now);
        tp.run_at = None;
    });
}

fn cancelled(ctx: &StepCtx, c: &CallSpec) {
    finish(ctx, c, ToolState::Cancelled, ToolOutput::error("Tool call was cancelled by the user."));
}

/// Run one tool call to completion (never fails: errors become error results).
pub async fn execute(ctx: Arc<StepCtx>, c: CallSpec) {
    if ctx.cancel.is_cancelled() {
        return cancelled(&ctx, &c);
    }
    // Parse arguments.
    let raw = if c.raw_input.trim().is_empty() { "{}" } else { c.raw_input.as_str() };
    let parsed: Value = match serde_json::from_str(raw) {
        Ok(v @ Value::Object(_)) => v,
        Ok(other) => {
            let msg = format!("Invalid JSON arguments: expected a JSON object, got {other}");
            return finish(&ctx, &c, ToolState::Error, ToolOutput::error(msg));
        }
        Err(e) => {
            let msg = format!("Invalid JSON arguments: {e}. Re-send the tool call with valid JSON.");
            return finish(&ctx, &c, ToolState::Error, ToolOutput::error(msg));
        }
    };
    let (purpose, input) = split_purpose(&parsed);
    let Some(tool) = ctx.tools.iter().find(|t| t.name() == c.name).cloned() else {
        let mut names: Vec<&str> = ctx.tools.iter().map(|t| t.name()).collect();
        names.sort();
        let msg = format!("Unknown tool `{}`. Available tools: {}", c.name, names.join(", "));
        ctx.session.update_tool(&c.msg_id, c.index, |tp| {
            tp.input = parsed.clone();
            tp.purpose = purpose.clone();
        });
        return finish(&ctx, &c, ToolState::Error, ToolOutput::error(msg));
    };
    let title = tool.title(&input);
    ctx.session.update_tool(&c.msg_id, c.index, |tp| {
        tp.input = parsed.clone();
        tp.purpose = purpose.clone();
        tp.title = title.clone();
    });

    // Permission.
    let subject = tool.permission_subject(&input);
    let always = ctx.session.lock().always.contains(&c.name);
    let action = if always { Action::Allow } else { ctx.config.permission(&ctx.agent, &c.name, &subject) };
    match action {
        Action::Allow => {}
        Action::Deny => {
            let msg = format!("Permission denied: tool `{}` is not allowed by configuration.", c.name);
            return finish(&ctx, &c, ToolState::Denied, ToolOutput::error(msg));
        }
        Action::Ask => match ask_permission(&ctx, &c).await {
            Some(PermissionDecision::Once) => {}
            Some(PermissionDecision::Always) => {
                ctx.session.lock().always.insert(c.name.clone());
            }
            Some(PermissionDecision::Reject) => {
                return finish(&ctx, &c, ToolState::Denied, ToolOutput::error("The user rejected this tool call."));
            }
            None => return cancelled(&ctx, &c),
        },
    }

    // Pre-execution delay.
    let delay = ctx.config.delay(&ctx.agent, &c.name);
    if !delay.is_zero() {
        let run_at = now_ms() + delay.as_millis() as i64;
        ctx.session.update_tool(&c.msg_id, c.index, |tp| {
            tp.state = ToolState::Delayed;
            tp.run_at = Some(run_at);
        });
        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            _ = ctx.cancel.cancelled() => return cancelled(&ctx, &c),
        }
    }

    // Run.
    ctx.session.update_tool(&c.msg_id, c.index, |tp| {
        tp.state = ToolState::Running;
        tp.run_at = None;
        tp.output.clear();
    });
    let live = Arc::new(Mutex::new(LiveBuf::default()));
    let live2 = live.clone();
    let tool_ctx = ToolCtx {
        cwd: ctx.cwd.clone(),
        session_id: ctx.session.id.clone(),
        call_id: c.call_id.clone(),
        cancel: ctx.cancel.child_token(),
        store: ctx.session.store.clone(),
        on_output: Arc::new(move |s: &str| crate::hub::lock(&live2).push(s)),
        image_input: ctx.image_input,
    };
    let name = c.name.clone();
    let mut handle = tokio::spawn(async move { tool.run(input, &tool_ctx).await });
    let mut tick = tokio::time::interval(LIVE_TICK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut flusher = LiveFlusher::default();
    let result = loop {
        tokio::select! {
            r = &mut handle => break r,
            _ = tick.tick() => flusher.flush(&ctx, &c, &live, false),
        }
    };
    flusher.flush(&ctx, &c, &live, true);
    let (state, out) = match result {
        Ok(Ok(out)) => {
            if out.is_error {
                (ToolState::Error, out)
            } else {
                (ToolState::Done, out)
            }
        }
        Ok(Err(e)) => (ToolState::Error, ToolOutput::error(format!("Error: {e:#}"))),
        Err(je) => {
            tracing::error!("tool {name} panicked: {je}");
            (ToolState::Error, ToolOutput::error(format!("Error: tool `{name}` crashed: {je}")))
        }
    };
    if ctx.cancel.is_cancelled() && state != ToolState::Done {
        let mut out = out;
        if out.text.is_empty() {
            out.text = "Tool call was cancelled by the user.".into();
        }
        return finish(&ctx, &c, ToolState::Cancelled, out);
    }
    finish(&ctx, &c, state, out);
}

/// Live output flush interval (at most this many Delta events per second per tool, x20).
const LIVE_TICK: Duration = Duration::from_millis(50);
/// Live output streamed as Deltas, per call. Beyond this, the part shows a refreshed tail.
pub const LIVE_STREAM_CAP: usize = 256 * 1024;
/// Size of the tail shown once the stream cap is exceeded.
pub const LIVE_TAIL: usize = 16 * 1024;
/// Tail refresh interval in tail mode.
const LIVE_TAIL_EVERY: Duration = Duration::from_secs(1);

/// Output reported by a running tool, not yet flushed to clients. Bounded: once the total
/// exceeds the stream cap only a tail is kept (`dropped` tells the flusher).
#[derive(Default)]
pub struct LiveBuf {
    buf: String,
    total: usize,
    dropped: bool,
}

impl LiveBuf {
    pub fn push(&mut self, s: &str) {
        self.total += s.len();
        self.buf.push_str(s);
        if self.total > LIVE_STREAM_CAP && self.buf.len() > 4 * LIVE_TAIL {
            keep_tail(&mut self.buf, LIVE_TAIL);
            self.dropped = true;
        }
    }
}

/// Keep the last ~`n` bytes of `s` (on a char boundary).
fn keep_tail(s: &mut String, n: usize) {
    if s.len() > n {
        let mut cut = s.len() - n;
        while !s.is_char_boundary(cut) {
            cut += 1;
        }
        s.drain(..cut);
    }
}

/// Moves live output into the tool part: Deltas up to [`LIVE_STREAM_CAP`], then a tail that
/// replaces the part output at most every [`LIVE_TAIL_EVERY`] (the final output replaces it).
#[derive(Default)]
struct LiveFlusher {
    streamed: usize,
    total: usize,
    tail: Option<String>,
    last_tail: Option<std::time::Instant>,
}

impl LiveFlusher {
    fn flush(&mut self, ctx: &StepCtx, c: &CallSpec, live: &Mutex<LiveBuf>, last: bool) {
        let (chunk, dropped, total) = {
            let mut l = crate::hub::lock(live);
            let d = std::mem::take(&mut l.dropped);
            (std::mem::take(&mut l.buf), d, l.total)
        };
        self.total = total;
        if self.tail.is_none() && !dropped && self.streamed + chunk.len() <= LIVE_STREAM_CAP {
            if chunk.is_empty() {
                return;
            }
            self.streamed += chunk.len();
            let s = &ctx.session;
            let mut st = s.lock();
            let Some(mi) = st.msgs.iter().rposition(|m| m.id == c.msg_id) else { return };
            if let Some(Part::Tool(tp)) = st.msgs[mi].parts.get_mut(c.index) {
                tp.output.push_str(&chunk);
            }
            s.emit(&st, Event::Delta { message: c.msg_id.clone(), index: c.index, field: DeltaField::ToolOutput, text: chunk });
            return;
        }
        // Tail mode.
        let tail = self.tail.get_or_insert_with(String::new);
        tail.push_str(&chunk);
        keep_tail(tail, LIVE_TAIL);
        let due = self.last_tail.is_none_or(|t| t.elapsed() >= LIVE_TAIL_EVERY);
        if (chunk.is_empty() && !last) || (!due && !last) {
            return;
        }
        self.last_tail = Some(std::time::Instant::now());
        let text = format!(
            "[live output: {} so far, showing the last {}]\n{}",
            human_bytes(self.total),
            human_bytes(tail.len()),
            tail
        );
        ctx.session.update_tool_quiet(&c.msg_id, c.index, |tp| tp.output = text);
    }
}

fn human_bytes(n: usize) -> String {
    if n >= 1 << 20 {
        format!("{:.1} MB", n as f64 / (1 << 20) as f64)
    } else if n >= 1 << 10 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

async fn ask_permission(ctx: &StepCtx, c: &CallSpec) -> Option<PermissionDecision> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    ctx.session.update_tool(&c.msg_id, c.index, |tp| tp.state = ToolState::Permission);
    let background = {
        let mut st: std::sync::MutexGuard<'_, State> = ctx.session.lock();
        st.perms.insert(c.call_id.clone(), tx);
        ctx.session.refresh_status(&mut st);
        (st.info.attached == 0).then(|| st.info.name.clone())
    };
    if let Some(name) = background {
        ctx.hub.toast_all(ToastLevel::Warning, format!("Session {name} is waiting for permission"));
    }
    let r = tokio::select! {
        r = rx => r.ok(),
        _ = ctx.cancel.cancelled() => None,
    };
    let mut st = ctx.session.lock();
    st.perms.remove(&c.call_id);
    ctx.session.refresh_status(&mut st);
    drop(st);
    if r.is_some() {
        ctx.session.update_tool(&c.msg_id, c.index, |tp| tp.state = ToolState::Running);
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn purpose_first_and_required() {
        let s = inject_purpose(&json!({"type":"object","properties":{"a":{"type":"string"}},"required":["a"]}));
        let keys: Vec<&String> = s["properties"].as_object().unwrap().keys().collect();
        assert_eq!(keys, ["purpose", "a"]);
        assert_eq!(s["required"], json!(["purpose", "a"]));
        let s = inject_purpose(&json!({}));
        assert_eq!(s["required"], json!(["purpose"]));
    }

    #[test]
    fn purpose_is_stripped() {
        let (p, rest) = split_purpose(&json!({"purpose":" why ","command":"ls"}));
        assert_eq!(p, "why");
        assert_eq!(rest, json!({"command":"ls"}));
    }
}
