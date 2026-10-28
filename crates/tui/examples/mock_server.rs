//! Mock `hernees-server` for TUI development.
//!
//! Speaks the hn-proto WebSocket protocol and simulates assistant turns:
//! streamed reasoning, markdown text (incl. CJK, code, tables), tool calls with
//! streaming input, permission prompts, pre-execution delays, live output, retries.
//! A few background sessions keep working on their own so `hernees -b` has panes.
//!
//! Run: `cargo run -p hernees --example mock_server -- --port 18555 [--background 3] [--register]`
//! Then: `hernees --server ws://127.0.0.1:18555`

use futures::{SinkExt, StreamExt};
use hn_proto::*;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message as WsMsg;

static SEQ: AtomicU64 = AtomicU64::new(1);
fn uid(p: &str) -> String {
    format!("{p}_{:06}", SEQ.fetch_add(1, Ordering::Relaxed))
}

struct Sess {
    info: SessionInfo,
    msgs: Vec<Message>,
    /// client id -> foreground?
    attached: HashMap<u64, bool>,
    cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
    perms: HashMap<String, oneshot::Sender<PermissionDecision>>,
    always: HashSet<String>,
    bg_worker: bool,
}

#[derive(Default)]
struct State {
    sessions: HashMap<String, Sess>,
    history: Vec<SessionInfo>,
    clients: HashMap<u64, mpsc::UnboundedSender<ServerMsg>>,
}

type S = Arc<Mutex<State>>;

impl State {
    fn open_list(&self) -> Vec<SessionInfo> {
        let mut v: Vec<SessionInfo> = self.sessions.values().map(|s| s.info.clone()).collect();
        v.sort_by_key(|s| s.created_at);
        v
    }
    fn push_sessions(&self) {
        let list = self.open_list();
        for tx in self.clients.values() {
            let _ = tx.send(ServerMsg::Sessions { sessions: list.clone() });
        }
    }
    fn emit(&self, sid: &str, ev: Event) {
        if let Some(s) = self.sessions.get(sid) {
            for cid in s.attached.keys() {
                if let Some(tx) = self.clients.get(cid) {
                    let _ = tx.send(ServerMsg::Event { session: sid.into(), event: ev.clone() });
                }
            }
        }
    }
    fn refresh_attached(&mut self, sid: &str) {
        if let Some(s) = self.sessions.get_mut(sid) {
            s.info.attached = s.attached.values().filter(|f| **f).count() as u32;
        }
    }
    fn set_status(&mut self, sid: &str, st: SessionStatus) {
        if let Some(s) = self.sessions.get_mut(sid) {
            s.info.status = st;
            s.info.updated_at = now_ms();
            let info = s.info.clone();
            self.emit(sid, Event::Info { info });
            self.push_sessions();
        }
    }
}

fn models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            provider: "dgn".into(),
            provider_name: "DGN".into(),
            model: "claude-opus-5.5".into(),
            name: "Claude Opus 5.5".into(),
            context: 200_000,
            output: 32_000,
            reasoning: true,
            image_input: true,
            variants: vec!["high".into(), "max".into()],
        },
        ModelInfo {
            provider: "dgn".into(),
            provider_name: "DGN".into(),
            model: "claude-sonnet-5".into(),
            name: "Claude Sonnet 5".into(),
            context: 1_000_000,
            output: 64_000,
            reasoning: true,
            image_input: true,
            variants: vec![],
        },
        ModelInfo {
            provider: "openai".into(),
            provider_name: "OpenAI".into(),
            model: "gpt-6".into(),
            name: "GPT-6".into(),
            context: 400_000,
            output: 128_000,
            reasoning: true,
            image_input: true,
            variants: vec!["low".into(), "medium".into(), "high".into()],
        },
        ModelInfo {
            provider: "deepseek".into(),
            provider_name: "DeepSeek".into(),
            model: "deepseek-v4".into(),
            name: "DeepSeek V4".into(),
            context: 128_000,
            output: 8_000,
            reasoning: false,
            image_input: false,
            variants: vec![],
        },
    ]
}

fn agents() -> Vec<AgentInfo> {
    let m = ModelRef { provider: "dgn".into(), model: "claude-opus-5.5".into(), variant: None };
    vec![
        AgentInfo { name: "build".into(), description: "Default agent with all tools".into(), model: Some(m.clone()), spawnable: false, manage_sessions: true, color: None },
        AgentInfo { name: "plan".into(), description: "Read-only planning".into(), model: Some(ModelRef { model: "claude-sonnet-5".into(), ..m.clone() }), spawnable: false, manage_sessions: false, color: None },
        AgentInfo { name: "review".into(), description: "Code reviewer".into(), model: Some(m), spawnable: true, manage_sessions: false, color: Some("#56b6c2".into()) },
    ]
}

fn agent_model(agent: &str) -> ModelRef {
    agents().into_iter().find(|a| a.name == agent).and_then(|a| a.model).unwrap_or_default()
}

fn new_session(st: &mut State, cwd: &str, agent: Option<String>, model: Option<ModelRef>, name: Option<String>) -> String {
    let id = uid("ses");
    let agent = agent.unwrap_or_else(|| "build".into());
    let model = model.unwrap_or_else(|| agent_model(&agent));
    let now = now_ms();
    let info = SessionInfo {
        id: id.clone(),
        name: name.unwrap_or_default(),
        agent,
        model,
        cwd: cwd.into(),
        created_at: now,
        updated_at: now,
        open: true,
        context: ContextInfo { used: 0, max: 200_000, compact_at: 160_000 },
        ..Default::default()
    };
    st.sessions.insert(
        id.clone(),
        Sess { info, msgs: vec![], attached: HashMap::new(), cancel: None, perms: HashMap::new(), always: HashSet::new(), bg_worker: false },
    );
    id
}

fn user_msg(text: &str, origin: Origin, agent: &str) -> Message {
    Message {
        id: uid("msg"),
        role: Role::User,
        origin,
        time: now_ms(),
        completed: None,
        parts: vec![Part::Text { text: text.into(), time: now_ms() }],
        model: None,
        agent: Some(agent.into()),
        usage: None,
        finish: None,
        error: None,
        replay_key: None,
    }
}

// ---------------------------------------------------------------------------
// Simulated turn
// ---------------------------------------------------------------------------

const REASONING: &str = "The user wants me to look at the project structure first. I should list the files, then read the main entry point to understand how the pieces fit together. 我需要先了解项目结构，然后再决定如何修改代码。Let me consider edge cases: wide characters, very long lines, and streaming updates that arrive faster than the frame rate. I'll run a command to inspect the directory and then summarize my findings with a short table and a code sample.";

const ANSWER: &str = "## Summary\n\nI looked at the project. Here's what I found — **the build is healthy** and the layout is *conventional*:\n\n- `crates/proto` defines the wire protocol\n- `crates/tui` is the terminal frontend\n  - renders markdown with `pulldown-cmark`\n  - wraps 中文 and other wide characters by display width\n- `crates/server` hosts sessions\n\n| crate | purpose | lines |\n|---|---|---:|\n| proto | protocol | 552 |\n| tui | 终端界面 | 4200 |\n\n```rust\nfn main() {\n    // entry point\n    let x = 42;\n    println!(\"hello {x}\");\n}\n```\n\n> Note: streaming is incremental, only the active part re-renders.\n\n1. First step\n2. Second step with a [link](https://example.com)\n\n这是一段很长的中文文本，用来测试自动换行功能是否能够正确处理双宽度字符，并且不会在终端中出现错位的问题。";

async fn sleep_ms(ms: u64) {
    tokio::time::sleep(Duration::from_millis(ms)).await;
}

fn cancelled(c: &Arc<std::sync::atomic::AtomicBool>) -> bool {
    c.load(Ordering::Relaxed)
}

/// Stream text in small chunks as Delta events.
async fn stream(s: &S, sid: &str, mid: &str, idx: usize, field: DeltaField, text: &str, chunk: usize, ms: u64, c: &Arc<std::sync::atomic::AtomicBool>) {
    let chars: Vec<char> = text.chars().collect();
    for piece in chars.chunks(chunk) {
        if cancelled(c) {
            return;
        }
        let t: String = piece.iter().collect();
        {
            let mut st = s.lock().unwrap();
            if let Some(m) = st.sessions.get_mut(sid).and_then(|x| x.msgs.iter_mut().find(|m| m.id == mid)) {
                match (&mut m.parts[idx], field) {
                    (Part::Text { text, .. }, DeltaField::Text) => text.push_str(&t),
                    (Part::Reasoning { text, .. }, DeltaField::Reasoning) => text.push_str(&t),
                    (Part::Tool(tp), DeltaField::ToolInput) => tp.raw_input.push_str(&t),
                    (Part::Tool(tp), DeltaField::ToolOutput) => tp.output.push_str(&t),
                    _ => {}
                }
            }
            st.emit(sid, Event::Delta { message: mid.into(), index: idx, field, text: t });
        }
        sleep_ms(ms).await;
    }
}

fn set_part(s: &S, sid: &str, mid: &str, idx: usize, part: Part) {
    let mut st = s.lock().unwrap();
    if let Some(m) = st.sessions.get_mut(sid).and_then(|x| x.msgs.iter_mut().find(|m| m.id == mid)) {
        if idx < m.parts.len() {
            m.parts[idx] = part.clone();
        } else {
            m.parts.push(part.clone());
        }
    }
    st.emit(sid, Event::Part { message: mid.into(), index: idx, part });
}

fn get_tool(s: &S, sid: &str, mid: &str, idx: usize) -> ToolPart {
    let st = s.lock().unwrap();
    st.sessions
        .get(sid)
        .and_then(|x| x.msgs.iter().find(|m| m.id == mid))
        .and_then(|m| match &m.parts[idx] {
            Part::Tool(t) => Some(t.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

async fn tool_call(s: &S, sid: &str, mid: &str, idx: usize, name: &str, input: Value, output: &str, ask: bool, delay_ms: i64, c: &Arc<std::sync::atomic::AtomicBool>) -> bool {
    let purpose = input.get("purpose").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let tp = ToolPart { call_id: uid("call"), name: name.into(), state: ToolState::Streaming, time_start: now_ms(), ..Default::default() };
    set_part(s, sid, mid, idx, Part::Tool(tp));
    let raw = serde_json::to_string(&input).unwrap();
    stream(s, sid, mid, idx, DeltaField::ToolInput, &raw, 6, 25, c).await;
    let mut tp = get_tool(s, sid, mid, idx);
    tp.input = input.clone();
    tp.purpose = purpose;
    tp.title = input.get("path").or(input.get("command")).and_then(|v| v.as_str()).unwrap_or("").to_string();
    if cancelled(c) {
        tp.state = ToolState::Cancelled;
        tp.time_end = Some(now_ms());
        set_part(s, sid, mid, idx, Part::Tool(tp));
        return false;
    }
    let always = s.lock().unwrap().sessions.get(sid).is_some_and(|x| x.always.contains(name));
    if ask && !always {
        tp.state = ToolState::Permission;
        let (tx, rx) = oneshot::channel();
        {
            let mut st = s.lock().unwrap();
            if let Some(x) = st.sessions.get_mut(sid) {
                x.perms.insert(tp.call_id.clone(), tx);
            }
        }
        set_part(s, sid, mid, idx, Part::Tool(tp.clone()));
        s.lock().unwrap().set_status(sid, SessionStatus::Permission);
        // Abort drains `perms` and sends Reject, so this always resolves.
        let d = match tokio::time::timeout(Duration::from_secs(600), rx).await {
            Ok(Ok(d)) => d,
            _ => PermissionDecision::Reject,
        };
        s.lock().unwrap().set_status(sid, SessionStatus::Busy);
        if d == PermissionDecision::Always {
            if let Some(x) = s.lock().unwrap().sessions.get_mut(sid) {
                x.always.insert(name.into());
            }
        }
        if d == PermissionDecision::Reject {
            tp.state = if cancelled(c) { ToolState::Cancelled } else { ToolState::Denied };
            tp.output = "User rejected this tool call.".into();
            tp.time_end = Some(now_ms());
            set_part(s, sid, mid, idx, Part::Tool(tp));
            return !cancelled(c);
        }
    }
    if delay_ms > 0 {
        tp.state = ToolState::Delayed;
        tp.run_at = Some(now_ms() + delay_ms);
        set_part(s, sid, mid, idx, Part::Tool(tp.clone()));
        let end = now_ms() + delay_ms;
        while now_ms() < end {
            if cancelled(c) {
                tp.state = ToolState::Cancelled;
                tp.time_end = Some(now_ms());
                set_part(s, sid, mid, idx, Part::Tool(tp));
                return false;
            }
            sleep_ms(50).await;
        }
        tp.run_at = None;
    }
    tp.state = ToolState::Running;
    set_part(s, sid, mid, idx, Part::Tool(tp.clone()));
    for line in output.lines() {
        stream(s, sid, mid, idx, DeltaField::ToolOutput, &format!("{line}\n"), 200, 120, c).await;
    }
    let mut tp = get_tool(s, sid, mid, idx);
    tp.time_end = Some(now_ms());
    if cancelled(c) {
        tp.state = ToolState::Cancelled;
        set_part(s, sid, mid, idx, Part::Tool(tp));
        return false;
    }
    tp.state = if name == "fail" { ToolState::Error } else { ToolState::Done };
    tp.output = output.to_string();
    set_part(s, sid, mid, idx, Part::Tool(tp));
    true
}

async fn turn(s: S, sid: String, prompt: String, quick: bool) {
    let c = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (agent, model) = {
        let mut st = s.lock().unwrap();
        let Some(x) = st.sessions.get_mut(&sid) else { return };
        if x.cancel.is_some() {
            return; // already busy; ignore (real server would queue)
        }
        x.cancel = Some(c.clone());
        (x.info.agent.clone(), x.info.model.clone())
    };
    s.lock().unwrap().set_status(&sid, SessionStatus::Busy);
    let p = prompt.to_lowercase();
    // Optional retry simulation.
    if p.contains("retry") {
        for attempt in 1..=2 {
            let next_at = now_ms() + 3000;
            s.lock().unwrap().set_status(&sid, SessionStatus::Retry { attempt, next_at, error: "429 Too Many Requests: rate limit exceeded".into() });
            sleep_ms(3000).await;
        }
        s.lock().unwrap().set_status(&sid, SessionStatus::Busy);
    }
    sleep_ms(300).await;
    let mid = uid("msg");
    let start = now_ms();
    let msg = Message {
        id: mid.clone(),
        role: Role::Assistant,
        origin: Origin::Human,
        time: start,
        completed: None,
        parts: vec![],
        model: Some(model),
        agent: Some(agent),
        usage: None,
        finish: None,
        error: None,
        replay_key: None,
    };
    {
        let mut st = s.lock().unwrap();
        if let Some(x) = st.sessions.get_mut(&sid) {
            x.msgs.push(msg.clone());
        }
        st.emit(&sid, Event::Message { message: msg });
    }
    let mut idx = 0;
    let speed = if quick { 3 } else { 1 };
    // Reasoning
    set_part(&s, &sid, &mid, idx, Part::Reasoning { text: String::new(), time_start: now_ms(), time_end: None, replay: Value::Null });
    stream(&s, &sid, &mid, idx, DeltaField::Reasoning, REASONING, 3 * speed, 30, &c).await;
    set_part(&s, &sid, &mid, idx, Part::Reasoning { text: REASONING.into(), time_start: start, time_end: Some(now_ms()), replay: Value::Null });
    idx += 1;
    let mut ok = !cancelled(&c);
    if ok && !p.contains("notool") {
        set_part(&s, &sid, &mid, idx, Part::Text { text: String::new(), time: now_ms() });
        stream(&s, &sid, &mid, idx, DeltaField::Text, "Let me look at the project layout first.", 4, 30, &c).await;
        idx += 1;
        let out = "total 48\ndrwxr-xr-x  5 user user 4096 Oct 29 10:00 .\n-rw-r--r--  1 user user 1234 Cargo.toml\ndrwxr-xr-x  9 user user 4096 crates\n-rw-r--r--  1 user user  35K LICENSE\n中文文件名.md";
        let ask = !quick && !p.contains("noperm");
        ok = tool_call(&s, &sid, &mid, idx, "bash", json!({"command": "ls -la", "workdir": ".", "purpose": "List the project root to see its layout"}), out, ask, if quick { 0 } else { 4000 }, &c).await;
        idx += 1;
        if ok {
            let out2 = (1..=40).map(|i| format!("{i:>4}  line {i} of src/main.rs")).collect::<Vec<_>>().join("\n");
            ok = tool_call(&s, &sid, &mid, idx, "read", json!({"path": "crates/tui/src/main.rs", "offset": 1, "limit": 40, "purpose": "Read the TUI entry point"}), &out2, false, 0, &c).await;
            idx += 1;
        }
    }
    if ok && !cancelled(&c) {
        set_part(&s, &sid, &mid, idx, Part::Text { text: String::new(), time: now_ms() });
        let ans = if quick { "Background task progressing: checked files, all good. 一切正常。" } else { ANSWER };
        stream(&s, &sid, &mid, idx, DeltaField::Text, ans, 5 * speed, 25, &c).await;
    }
    // Finish
    let was_cancelled = cancelled(&c);
    {
        let mut st = s.lock().unwrap();
        let mut final_msg = None;
        if let Some(x) = st.sessions.get_mut(&sid) {
            x.cancel = None;
            x.info.context.used = (x.info.context.used + 12_345).min(x.info.context.max);
            x.info.cost += 0.0123;
            x.info.message_count = x.msgs.len() as u32;
            if let Some(m) = x.msgs.iter_mut().find(|m| m.id == mid) {
                m.completed = Some(now_ms());
                m.finish = Some(if was_cancelled { "aborted".into() } else { "stop".into() });
                m.usage = Some(Usage { input: 1000, output: 500, cache_read: 10000, cache_write: 0, reasoning: 200 });
                final_msg = Some(m.clone());
            }
            if x.info.name.is_empty() {
                x.info.name = title_for(&prompt);
            }
        }
        if let Some(m) = final_msg {
            st.emit(&sid, Event::Message { message: m });
        }
        st.set_status(&sid, SessionStatus::Idle);
    }
}

fn title_for(p: &str) -> String {
    let t: String = p.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(40).collect();
    if t.is_empty() { "Untitled".into() } else { t }
}

fn add_user(s: &S, sid: &str, text: &str, origin: Origin) {
    let mut st = s.lock().unwrap();
    let Some(x) = st.sessions.get_mut(sid) else { return };
    let m = user_msg(text, origin, &x.info.agent.clone());
    x.msgs.push(m.clone());
    st.emit(sid, Event::Message { message: m });
}

// ---------------------------------------------------------------------------
// Connection handling
// ---------------------------------------------------------------------------

fn handle(s: &S, cid: u64, req: Request) -> ServerMsg {
    let id = req.id;
    let mut st = s.lock().unwrap();
    let r: Result<Value, String> = match req.msg {
        ClientMsg::Hello { protocol, .. } => {
            if protocol != PROTOCOL_VERSION {
                Err(format!("protocol mismatch: server {PROTOCOL_VERSION}, client {protocol}"))
            } else {
                let list = st.open_list();
                if let Some(tx) = st.clients.get(&cid) {
                    let _ = tx.send(ServerMsg::Sessions { sessions: list });
                }
                Ok(json!({"server": "mock", "version": "0.0.0"}))
            }
        }
        ClientMsg::ListSessions { scope } => {
            let mut v = st.open_list();
            if scope == ListScope::History {
                v.extend(st.history.iter().cloned());
            }
            Ok(serde_json::to_value(v).unwrap())
        }
        ClientMsg::CreateSession { cwd, agent, model, name } => {
            let sid = new_session(&mut st, &cwd, agent, model, name);
            let x = st.sessions.get_mut(&sid).unwrap();
            x.attached.insert(cid, true);
            st.refresh_attached(&sid);
            let info = st.sessions[&sid].info.clone();
            st.push_sessions();
            Ok(serde_json::to_value(info).unwrap())
        }
        ClientMsg::Attach { session, foreground } => {
            if !st.sessions.contains_key(&session) {
                if let Some(pos) = st.history.iter().position(|h| h.id == session) {
                    let mut info = st.history.remove(pos);
                    info.open = true;
                    let sid = info.id.clone();
                    st.sessions.insert(sid, Sess { info, msgs: vec![user_msg("(restored from history)", Origin::Human, "build")], attached: HashMap::new(), cancel: None, perms: HashMap::new(), always: HashSet::new(), bg_worker: false });
                }
            }
            match st.sessions.get_mut(&session) {
                None => Err(format!("no such session: {session}")),
                Some(x) => {
                    x.attached.insert(cid, foreground);
                    st.refresh_attached(&session);
                    let x = &st.sessions[&session];
                    let snap = Event::Snapshot { info: x.info.clone(), messages: x.msgs.clone() };
                    let info = x.info.clone();
                    if let Some(tx) = st.clients.get(&cid) {
                        let _ = tx.send(ServerMsg::Event { session: session.clone(), event: snap });
                    }
                    st.push_sessions();
                    Ok(serde_json::to_value(info).unwrap())
                }
            }
        }
        ClientMsg::Detach { session } => {
            if let Some(x) = st.sessions.get_mut(&session) {
                x.attached.remove(&cid);
            }
            st.refresh_attached(&session);
            st.push_sessions();
            Ok(Value::Null)
        }
        ClientMsg::WatchBackground { .. } => Ok(Value::Null),
        ClientMsg::Prompt { session, text, images } => {
            if !st.sessions.contains_key(&session) {
                Err("no such session".into())
            } else {
                drop(st);
                let mut t = text.clone();
                if !images.is_empty() {
                    t.push_str(&format!("\n\n[{} image(s) attached]", images.len()));
                }
                add_user(s, &session, &t, Origin::Human);
                tokio::spawn(turn(s.clone(), session, text, false));
                return ServerMsg::ok(id, Value::Null);
            }
        }
        ClientMsg::Command { session, name, args } => {
            drop(st);
            add_user(s, &session, &format!("/{name} {args}"), Origin::Human);
            tokio::spawn(turn(s.clone(), session, format!("{name} notool"), false));
            return ServerMsg::ok(id, Value::Null);
        }
        ClientMsg::Abort { session } => {
            if let Some(x) = st.sessions.get_mut(&session) {
                if let Some(c) = &x.cancel {
                    c.store(true, Ordering::Relaxed);
                }
                for (_, tx) in x.perms.drain() {
                    let _ = tx.send(PermissionDecision::Reject);
                }
            }
            Ok(Value::Null)
        }
        ClientMsg::Permission { session, call_id, decision } => {
            match st.sessions.get_mut(&session).and_then(|x| x.perms.remove(&call_id)) {
                Some(tx) => {
                    let _ = tx.send(decision);
                    Ok(Value::Null)
                }
                None => Err("no pending permission".into()),
            }
        }
        ClientMsg::SetModel { session, model } => {
            if let Some(x) = st.sessions.get_mut(&session) {
                x.info.model = model;
                let info = x.info.clone();
                st.emit(&session, Event::Info { info });
                st.push_sessions();
            }
            Ok(Value::Null)
        }
        ClientMsg::SetAgent { session, agent } => {
            if let Some(x) = st.sessions.get_mut(&session) {
                x.info.model = agent_model(&agent);
                x.info.agent = agent;
                let info = x.info.clone();
                st.emit(&session, Event::Info { info });
                st.push_sessions();
            }
            Ok(Value::Null)
        }
        ClientMsg::Rename { session, name } => {
            if let Some(x) = st.sessions.get_mut(&session) {
                x.info.name = name;
                let info = x.info.clone();
                st.emit(&session, Event::Info { info });
                st.push_sessions();
            }
            Ok(Value::Null)
        }
        ClientMsg::Compact { session } => {
            if let Some(x) = st.sessions.get_mut(&session) {
                let mut m = user_msg("Summary: the user asked to inspect the project; we listed files and read main.rs.", Origin::Compaction, &x.info.agent.clone());
                m.time = now_ms();
                x.msgs.push(m.clone());
                x.info.context.used = 3000;
                let info = x.info.clone();
                st.emit(&session, Event::Message { message: m });
                st.emit(&session, Event::Info { info });
            }
            Ok(Value::Null)
        }
        ClientMsg::Undo { session } => {
            let Some(x) = st.sessions.get_mut(&session) else { return ServerMsg::err(id, "no session") };
            match x.msgs.iter().rposition(|m| m.role == Role::User && m.origin == Origin::Human) {
                None => Err("nothing to undo".into()),
                Some(pos) => {
                    let removed: Vec<Message> = x.msgs.drain(pos..).collect();
                    let text = removed[0].raw_text();
                    let ids = removed.into_iter().map(|m| m.id).collect();
                    st.emit(&session, Event::Removed { messages: ids });
                    Ok(Value::String(text))
                }
            }
        }
        ClientMsg::Close { session } => {
            if let Some(mut x) = st.sessions.remove(&session) {
                if let Some(c) = &x.cancel {
                    c.store(true, Ordering::Relaxed);
                }
                for cid2 in x.attached.keys() {
                    if let Some(tx) = st.clients.get(cid2) {
                        let _ = tx.send(ServerMsg::Event { session: session.clone(), event: Event::Closed });
                    }
                }
                x.info.open = false;
                x.info.attached = 0;
                st.history.push(x.info);
            }
            st.push_sessions();
            Ok(Value::Null)
        }
        ClientMsg::Delete { session } => {
            st.history.retain(|h| h.id != session);
            st.sessions.remove(&session);
            st.push_sessions();
            Ok(Value::Null)
        }
        ClientMsg::ListModels => Ok(serde_json::to_value(models()).unwrap()),
        ClientMsg::ListAgents => Ok(serde_json::to_value(agents()).unwrap()),
        ClientMsg::ListCommands { .. } => Ok(json!([
            {"name": "init", "description": "Create AGENTS.md for this project", "source": "custom"},
            {"name": "review", "description": "Review the current changes", "source": "custom"},
            {"name": "pdf", "description": "Work with PDF files", "source": "skill"}
        ])),
        ClientMsg::Export { session } => match st.sessions.get(&session) {
            None => Err("no session".into()),
            Some(x) => {
                let mut md = format!("# {}\n\n", x.info.name);
                for m in &x.msgs {
                    md.push_str(&format!("## {:?}\n\n{}\n\n", m.role, m.raw_text()));
                }
                Ok(Value::String(md))
            }
        },
        ClientMsg::Ping => Ok(json!("pong")),
    };
    match r {
        Ok(v) => ServerMsg::ok(id, v),
        Err(e) => ServerMsg::err(id, e),
    }
}

async fn conn(s: S, stream: tokio::net::TcpStream, cid: u64) {
    let Ok(ws) = tokio_tungstenite::accept_async(stream).await else { return };
    let (mut sink, mut rx_ws) = ws.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerMsg>();
    s.lock().unwrap().clients.insert(cid, tx.clone());
    let writer = tokio::spawn(async move {
        while let Some(m) = rx.recv().await {
            let t = serde_json::to_string(&m).unwrap();
            if sink.send(WsMsg::text(t)).await.is_err() {
                break;
            }
        }
    });
    while let Some(Ok(m)) = rx_ws.next().await {
        if let WsMsg::Text(t) = m {
            match serde_json::from_str::<Request>(t.as_str()) {
                Ok(req) => {
                    let resp = handle(&s, cid, req);
                    let _ = tx.send(resp);
                }
                Err(e) => eprintln!("bad request: {e}: {t}"),
            }
        }
    }
    let mut st = s.lock().unwrap();
    st.clients.remove(&cid);
    let ids: Vec<String> = st.sessions.keys().cloned().collect();
    for sid in ids {
        if let Some(x) = st.sessions.get_mut(&sid) {
            x.attached.remove(&cid);
        }
        st.refresh_attached(&sid);
    }
    st.push_sessions();
    writer.abort();
}

/// Background sessions that start turns by themselves every so often.
async fn bg_worker(s: S, sid: String, n: u64) {
    sleep_ms(1500 + n * 2500).await;
    let tasks = ["Refactor the parser module", "调查内存泄漏问题", "Write integration tests for the API", "Update dependencies", "Review PR #42"];
    let mut i = n as usize;
    loop {
        let exists = s.lock().unwrap().sessions.get(&sid).is_some_and(|x| x.bg_worker && x.info.attached == 0);
        if !s.lock().unwrap().sessions.contains_key(&sid) {
            return;
        }
        if exists {
            let task = tasks[i % tasks.len()];
            add_user(&s, &sid, task, Origin::Spawn { from_id: "ses_parent".into(), from_name: "orchestrator".into() });
            turn(s.clone(), sid.clone(), format!("{task} quick"), true).await;
        }
        i += 1;
        sleep_ms(12_000 + n * 3000).await;
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let get = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let port: u16 = get("--port").and_then(|p| p.parse().ok()).unwrap_or(18555);
    let nbg: usize = get("--background").and_then(|p| p.parse().ok()).unwrap_or(3);
    let register = args.iter().any(|a| a == "--register");
    // Exit after N seconds (to test client reconnects).
    if let Some(secs) = get("--die-after").and_then(|p| p.parse::<u64>().ok()) {
        tokio::spawn(async move {
            sleep_ms(secs * 1000).await;
            eprintln!("mock server: --die-after reached, exiting");
            std::process::exit(0);
        });
    }
    let s: S = Arc::new(Mutex::new(State::default()));
    {
        let mut st = s.lock().unwrap();
        let names = ["parser-refactor", "内存泄漏调查", "api-tests", "deps-update", "pr-review", "docs", "bench", "ci-fix"];
        let cwd = std::env::current_dir().unwrap().to_string_lossy().to_string();
        for i in 0..nbg {
            let agent = ["build", "review", "plan"][i % 3].to_string();
            let sid = new_session(&mut st, &cwd, Some(agent), None, Some(names[i % names.len()].into()));
            let x = st.sessions.get_mut(&sid).unwrap();
            x.bg_worker = true;
            x.info.parent = Some("ses_parent".into());
            x.info.created_at -= (nbg - i) as i64 * 1000;
        }
        for i in 0..6 {
            let t = now_ms() - (i as i64) * 86_400_000 / 2;
            st.history.push(SessionInfo {
                id: uid("ses"),
                name: format!("Old session {i} · 历史会话"),
                agent: "build".into(),
                model: agent_model("build"),
                cwd: cwd.clone(),
                created_at: t,
                updated_at: t,
                ..Default::default()
            });
        }
    }
    let ids: Vec<String> = s.lock().unwrap().sessions.keys().cloned().collect();
    for (n, sid) in ids.into_iter().enumerate() {
        tokio::spawn(bg_worker(s.clone(), sid, n as u64));
    }
    let listener = TcpListener::bind(("127.0.0.1", port)).await.expect("bind");
    if register {
        let dir = instances_dir();
        let _ = std::fs::create_dir_all(&dir);
        let info = InstanceInfo { pid: std::process::id(), port, bind: "127.0.0.1".into(), allow_hosts: vec![], idle_timeout: -1, started_at: now_ms(), version: "mock".into(), exe: std::env::current_exe().unwrap().to_string_lossy().into() };
        let _ = std::fs::write(dir.join(format!("{}.json", std::process::id())), serde_json::to_string(&info).unwrap());
    }
    eprintln!("mock server on ws://127.0.0.1:{port}");
    let mut cid = 0;
    loop {
        let Ok((stream, _)) = listener.accept().await else { continue };
        cid += 1;
        tokio::spawn(conn(s.clone(), stream, cid));
    }
}
