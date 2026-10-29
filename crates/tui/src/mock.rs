//! `ref --mock`: an in-process fake backend producing realistic streaming so the UI can be
//! exercised without `ref-server`. Speaks the same channel interface as `net::run`.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ref_proto::*;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};

use crate::net::NetEvent;

struct MSession {
    info: SessionInfo,
    messages: Vec<Message>,
    perms: Vec<PermissionRequest>,
    abort: Arc<AtomicBool>,
    /// Prompts waiting for the session to become idle.
    queue: Vec<Message>,
}

#[derive(Default)]
struct World {
    sessions: HashMap<String, MSession>,
    order: Vec<String>,
    /// Sessions the client is subscribed to.
    subs: HashSet<String>,
    watch_all: bool,
    perm_waiters: HashMap<String, oneshot::Sender<bool>>,
    history: Vec<SessionInfo>,
    /// When the first MCP status was asked for (simulates a slow-connecting server).
    mcp_started: i64,
}

#[derive(Clone)]
struct Mock {
    w: Arc<Mutex<World>>,
    tx: mpsc::UnboundedSender<NetEvent>,
}

static SEQ: AtomicU64 = AtomicU64::new(1);

fn id(prefix: &str) -> String {
    format!("{prefix}_{:x}{:04}", now_ms(), SEQ.fetch_add(1, Ordering::Relaxed))
}

fn model() -> ModelRef {
    ModelRef { provider: "dgn".into(), model: "claude-opus-5.5".into(), variant: Some("high".into()) }
}

fn info(name: &str, agent: &str, cwd: &str) -> SessionInfo {
    let t = now_ms();
    SessionInfo {
        id: id("ses"),
        name: name.into(),
        title: String::new(),
        agent: agent.into(),
        model: model(),
        cwd: cwd.into(),
        created_at: t,
        updated_at: t,
        status: SessionStatus::Idle,
        attached: 0,
        live: true,
        parent: None,
        must_report: false,
        context: ContextUsage { used: 0, max: 1_000_000, recommend: 600_000, auto: 850_000 },
        todos: Vec::new(),
        busy_since: None,
    }
}

fn config() -> ServerConfigInfo {
    let m = |p: &str, pn: &str, id: &str, name: &str, ctx: u64, variants: &[&str]| ModelInfo {
        provider: p.into(),
        provider_name: pn.into(),
        id: id.into(),
        name: name.into(),
        context: ctx,
        output: 64_000,
        images: true,
        reasoning: !variants.is_empty(),
        variants: variants.iter().map(|s| s.to_string()).collect(),
    };
    ServerConfigInfo {
        agents: vec![
            AgentInfo { name: "build".into(), description: "Default agent with all tools".into(), model: None, creatable: true, can_spawn: true, color: None },
            AgentInfo { name: "plan".into(), description: "Read-only planning agent".into(), model: None, creatable: true, can_spawn: false, color: None },
            AgentInfo { name: "review".into(), description: "Reviews diffs and reports issues".into(), model: Some(ModelRef { provider: "openai".into(), model: "gpt-6".into(), variant: None }), creatable: true, can_spawn: false, color: Some("#56b6c2".into()) },
        ],
        default_agent: "build".into(),
        models: vec![
            m("dgn", "DGN Gateway", "claude-opus-5.5", "Claude Opus 5.5", 1_000_000, &["low", "medium", "high"]),
            m("dgn", "DGN Gateway", "claude-sonnet-5", "Claude Sonnet 5", 400_000, &["low", "high"]),
            m("openai", "OpenAI", "gpt-6", "GPT-6", 512_000, &["minimal", "low", "medium", "high"]),
            m("local", "Local", "qwen4-coder", "Qwen4 Coder 32B", 128_000, &[]),
        ],
        commands: vec![
            CommandInfo { name: "review".into(), description: "Review uncommitted changes".into(), source: "config".into() },
            CommandInfo { name: "init".into(), description: "Create AGENTS.md for this project".into(), source: "builtin".into() },
        ],
        mcp: vec![("context7".into(), "connected (2 tools)".into())],
        tui: Value::Null,
    }
}

impl Mock {
    fn send(&self, f: ServerFrame) {
        let _ = self.tx.send(NetEvent::Frame(f));
    }

    fn broadcast_sessions(&self) {
        let list: Vec<SessionInfo> = {
            let w = self.w.lock().unwrap();
            w.order.iter().filter_map(|i| w.sessions.get(i)).map(|s| s.info.clone()).collect()
        };
        self.send(ServerFrame::Sessions { sessions: list });
    }

    /// Applies an event to the world and forwards it if subscribed.
    fn emit(&self, sid: &str, ev: SessionEvent) {
        let deliver = {
            let mut w = self.w.lock().unwrap();
            let watch_all = w.watch_all;
            let subscribed = w.subs.contains(sid);
            let Some(s) = w.sessions.get_mut(sid) else { return };
            match &ev {
                SessionEvent::Info { info } => s.info = info.clone(),
                SessionEvent::Message { message } => match s.messages.iter_mut().find(|m| m.id == message.id) {
                    Some(m) => *m = message.clone(),
                    None => s.messages.push(message.clone()),
                },
                SessionEvent::Part { message_id, index, part } => {
                    if let Some(m) = s.messages.iter_mut().find(|m| &m.id == message_id) {
                        if *index < m.parts.len() {
                            m.parts[*index] = part.clone();
                        } else {
                            m.parts.push(part.clone());
                        }
                    }
                }
                SessionEvent::Delta { message_id, index, delta } => {
                    if let Some(m) = s.messages.iter_mut().find(|m| &m.id == message_id) {
                        crate::state::apply_delta(&mut m.parts, *index, delta.clone());
                    }
                }
                SessionEvent::Removed { message_id } => s.messages.retain(|m| &m.id != message_id),
                SessionEvent::Reset { messages } => s.messages = messages.clone(),
                SessionEvent::Permission { request } => s.perms.push(request.clone()),
                SessionEvent::PermissionResolved { call_id } => s.perms.retain(|p| &p.call_id != call_id),
                _ => {}
            }
            subscribed || (watch_all && s.info.attached == 0)
        };
        if deliver {
            self.send(ServerFrame::Event { session_id: sid.to_string(), event: ev });
        }
    }

    fn set_status(&self, sid: &str, st: SessionStatus) {
        let info = {
            let mut w = self.w.lock().unwrap();
            let Some(s) = w.sessions.get_mut(sid) else { return };
            // Like the server: the run start, kept while busy (Busy / Running / ...).
            if st.is_idle() {
                s.info.busy_since = None;
            } else if s.info.status.is_idle() || s.info.busy_since.is_none() {
                s.info.busy_since = Some(now_ms());
            }
            s.info.status = st;
            s.info.updated_at = now_ms();
            s.info.clone()
        };
        self.emit(sid, SessionEvent::Info { info });
        self.broadcast_sessions();
    }

    fn add_context(&self, sid: &str, n: u64) {
        let info = {
            let mut w = self.w.lock().unwrap();
            let Some(s) = w.sessions.get_mut(sid) else { return };
            s.info.context.used += n;
            s.info.clone()
        };
        self.emit(sid, SessionEvent::Info { info });
    }

    fn aborted(&self, sid: &str) -> bool {
        self.w.lock().unwrap().sessions.get(sid).is_none_or(|s| s.abort.load(Ordering::Relaxed))
    }

    fn create(&self, name: &str, agent: &str, cwd: &str) -> SessionInfo {
        let mut w = self.w.lock().unwrap();
        let mut name = name.to_string();
        let mut k = 2;
        while w.sessions.values().any(|s| s.info.name == name) {
            name = format!("{}-{k}", name.trim_end_matches(char::is_numeric).trim_end_matches('-'));
            k += 1;
        }
        let i = info(&name, agent, cwd);
        w.order.push(i.id.clone());
        w.sessions.insert(i.id.clone(), MSession { info: i.clone(), messages: Vec::new(), perms: Vec::new(), abort: Arc::new(AtomicBool::new(false)), queue: Vec::new() });
        i
    }

    fn user_message(&self, sid: &str, text: &str, source: MessageSource, images: Vec<ImageData>) {
        self.user_message_id(sid, id("msg"), text, source, images);
    }

    fn user_message_id(&self, sid: &str, mid: String, text: &str, source: MessageSource, images: Vec<ImageData>) {
        let m = user_msg(mid, text, source, images);
        {
            let mut w = self.w.lock().unwrap();
            if let Some(s) = w.sessions.get_mut(sid)
                && s.info.title.is_empty()
            {
                s.info.title = text.lines().next().unwrap_or("").chars().take(50).collect();
            }
        }
        self.emit(sid, SessionEvent::Message { message: m });
    }

    fn agent_of(&self, sid: &str) -> String {
        self.w.lock().unwrap().sessions.get(sid).map(|s| s.info.agent.clone()).unwrap_or_default()
    }

    fn new_assistant(&self, sid: &str) -> String {
        let model = self.w.lock().unwrap().sessions.get(sid).map(|s| s.info.model.clone());
        let mid = id("msg");
        let m = Message { id: mid.clone(), role: Role::Assistant, time: now_ms(), time_end: None, source: MessageSource::Human, parts: vec![], model, agent: Some(self.agent_of(sid)), usage: None, error: None };
        self.emit(sid, SessionEvent::Message { message: m });
        mid
    }

    fn message(&self, sid: &str, mid: &str) -> Option<Message> {
        self.w.lock().unwrap().sessions.get(sid)?.messages.iter().find(|m| m.id == mid).cloned()
    }

    fn finish(&self, sid: &str, mid: &str, usage: Usage, error: Option<String>) {
        if let Some(mut m) = self.message(sid, mid) {
            m.time_end = Some(now_ms());
            m.usage = Some(usage);
            m.error = error;
            if m.error.is_some() {
                for p in &mut m.parts {
                    if let Part::Tool { state, output, time_end, .. } = p
                        && !state.is_final()
                    {
                        *state = ToolState::Cancelled;
                        *output = Some("Cancelled by user.".into());
                        *time_end = Some(now_ms());
                    }
                    if let Part::Reasoning { time_end, .. } = p
                        && time_end.is_none()
                    {
                        *time_end = Some(now_ms());
                    }
                }
            }
            self.emit(sid, SessionEvent::Message { message: m });
        }
    }

    /// Streams `text` as deltas into part `idx`. Returns false if aborted.
    #[allow(clippy::too_many_arguments)]
    async fn stream(&self, sid: &str, mid: &str, idx: usize, text: &str, reasoning: bool, chunk: usize, delay: u64) -> bool {
        let chars: Vec<char> = text.chars().collect();
        for c in chars.chunks(chunk.max(1)) {
            if self.aborted(sid) {
                return false;
            }
            let t: String = c.iter().collect();
            let delta = if reasoning { Delta::Reasoning { text: t } } else { Delta::Text { text: t } };
            self.emit(sid, SessionEvent::Delta { message_id: mid.to_string(), index: idx, delta });
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        true
    }

    async fn sleep_or_abort(&self, sid: &str, ms: u64) -> bool {
        let steps = ms / 50;
        for _ in 0..steps.max(1) {
            if self.aborted(sid) {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        !self.aborted(sid)
    }
}

const REASONING: &str = "The user wants me to look at the parser. First I should read the file to understand the current structure, \
then find where the off-by-one happens. The loop in `parse_items` iterates with `0..=len`, which reads one element past the end \
when the input is not terminated. I'll fix the bound, then run the unit tests to make sure nothing else breaks. \
It might also be worth checking whether 中文输入 (CJK input) is handled correctly by the tokenizer, since widths differ.";

const ANSWER: &str = "I found the problem. The loop in `parse_items` used an **inclusive** range, so it read one element \
past the end of the buffer:

```rust
for i in 0..=items.len() {
    let item = &items[i]; // panics when i == len
    out.push(parse(item)?);
}
```

## What I changed

1. Switched to an exclusive range (`0..items.len()`)
2. Added a regression test for *unterminated* input
3. Ran the full test suite

| check | before | after |
|---|---|---|
| unit tests | 41 / 42 | 42 / 42 |
| clippy | 2 warnings | clean |

> Note: the tokenizer already handles 中文 and emoji 👍 widths correctly.

Everything passes now — see the [docs](https://example.com/parser) for the parser contract.";

fn tool_part(call_id: &str, name: &str, purpose: &str, input: Value, state: ToolState) -> Part {
    Part::Tool {
        call_id: call_id.into(),
        name: name.into(),
        purpose: purpose.into(),
        raw_input: serde_json::to_string(&json!({"purpose": purpose})).unwrap_or_default(),
        input,
        state,
        output: None,
        meta: None,
        images: vec![],
        time: now_ms(),
        time_end: None,
        run_at: None,
        provider_data: None,
    }
}

impl Mock {
    fn set_part(&self, sid: &str, mid: &str, idx: usize, part: Part) {
        self.emit(sid, SessionEvent::Part { message_id: mid.into(), index: idx, part });
    }

    fn tool_mut(&self, sid: &str, mid: &str, idx: usize, f: impl FnOnce(&mut Part)) {
        if let Some(m) = self.message(sid, mid)
            && let Some(mut p) = m.parts.get(idx).cloned()
        {
            f(&mut p);
            self.set_part(sid, mid, idx, p);
        }
    }

    fn finish_tool(&self, sid: &str, mid: &str, idx: usize, state: ToolState, output: &str, meta: Option<Value>) {
        self.tool_mut(sid, mid, idx, |p| {
            if let Part::Tool { state: s, output: o, meta: m, time_end, .. } = p {
                *s = state;
                *o = Some(output.into());
                *m = meta;
                *time_end = Some(now_ms());
            }
        });
    }

    fn set_tool_state(&self, sid: &str, mid: &str, idx: usize, state: ToolState) {
        self.tool_mut(sid, mid, idx, |p| {
            if let Part::Tool { state: s, .. } = p {
                *s = state;
            }
        });
    }

    async fn ask_permission(&self, sid: &str, mid: &str, idx: usize, tool: &str, purpose: &str, input: Value) -> Option<bool> {
        self.set_tool_state(sid, mid, idx, ToolState::AwaitingPermission);
        self.set_status(sid, SessionStatus::WaitingPermission);
        let call_id = match self.message(sid, mid).and_then(|m| m.parts.get(idx).cloned()) {
            Some(Part::Tool { call_id, .. }) => call_id,
            _ => return None,
        };
        let (tx, mut rx) = oneshot::channel();
        self.w.lock().unwrap().perm_waiters.insert(call_id.clone(), tx);
        let req = PermissionRequest { session_id: sid.into(), message_id: mid.into(), call_id: call_id.clone(), tool: tool.into(), purpose: purpose.into(), input, time: now_ms() };
        self.emit(sid, SessionEvent::Permission { request: req });
        let res = loop {
            tokio::select! {
                r = &mut rx => break r.ok(),
                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                    if self.aborted(sid) { break None; }
                }
            }
        };
        self.w.lock().unwrap().perm_waiters.remove(&call_id);
        self.emit(sid, SessionEvent::PermissionResolved { call_id });
        self.set_status(sid, SessionStatus::Running);
        res
    }

    /// The main demo turn.
    async fn demo_turn(&self, sid: &str, prompt: &str) {
        self.set_status(sid, SessionStatus::Busy);
        let mid = self.new_assistant(sid);
        let ok = self.turn_body(sid, &mid, prompt).await;
        let usage = Usage { input: 1834, output: 912, cache_read: 40_210, cache_write: 2048, reasoning: 230 };
        self.add_context(sid, 45_000);
        self.finish(sid, &mid, usage, (!ok).then(|| "Aborted".to_string()));
        self.set_status(sid, SessionStatus::Idle);
    }

    async fn turn_body(&self, sid: &str, mid: &str, prompt: &str) -> bool {
        tokio::time::sleep(Duration::from_millis(400)).await;
        // Reasoning.
        self.set_part(sid, mid, 0, Part::Reasoning { text: String::new(), time: now_ms(), time_end: None, provider_data: None });
        if !self.stream(sid, mid, 0, REASONING, true, 3, 25).await {
            return false;
        }
        self.tool_mut(sid, mid, 0, |p| {
            if let Part::Reasoning { time_end, .. } = p {
                *time_end = Some(now_ms());
            }
        });
        // Short text.
        self.set_part(sid, mid, 1, Part::Text { text: String::new(), time: now_ms() });
        let intro = if prompt.to_lowercase().contains("hello") { "Hi! Let me take a look at the project first." } else { "Let me look at the parser first." };
        if !self.stream(sid, mid, 1, intro, false, 2, 20).await {
            return false;
        }
        // Tool: read (done).
        self.set_status(sid, SessionStatus::Running);
        self.set_part(sid, mid, 2, tool_part("call_read", "read", "Read the parser to find the loop bounds", json!({"path": "src/parser.rs", "offset": 1, "limit": 200}), ToolState::Running));
        if !self.sleep_or_abort(sid, 600).await {
            return false;
        }
        let file: String = (1..=260).map(|i| format!("{i:>4}│ {}\n", if i == 42 { "    for i in 0..=items.len() {" } else { "    // ..." })).collect();
        self.finish_tool(sid, mid, 2, ToolState::Done, &format!("[src/parser.rs#a1f3]\n{file}"), Some(json!({"lines": 260})));
        // Tool: grep (error).
        self.set_part(sid, mid, 3, tool_part("call_grep", "grep", "Search for other inclusive ranges", json!({"pattern": "0..=", "path": "src/lib"}), ToolState::Running));
        if !self.sleep_or_abort(sid, 400).await {
            return false;
        }
        self.finish_tool(sid, mid, 3, ToolState::Error, "Path not found: src/lib\nDid you mean src/?", None);
        // Tool: edit (streaming → permission → done with diff).
        let edit_input = json!({"path": "src/parser.rs", "tag": "a1f3", "edits": [{"op": "replace", "start": 42, "end": 42, "lines": ["    for i in 0..items.len() {"]}]});
        let mut p = tool_part("call_edit", "edit", "", Value::Null, ToolState::Streaming);
        if let Part::Tool { raw_input, .. } = &mut p {
            raw_input.clear();
        }
        self.set_part(sid, mid, 4, p);
        let raw = r#"{"purpose": "Fix the off-by-one in parse_items", "path": "src/parser.rs", "tag": "a1f3", "edits": [{"op":"replace","start":42,"end":42,"lines":["    for i in 0..items.len() {"]}]}"#;
        for c in raw.as_bytes().chunks(8) {
            if self.aborted(sid) {
                return false;
            }
            self.emit(sid, SessionEvent::Delta { message_id: mid.into(), index: 4, delta: Delta::ToolInput { text: String::from_utf8_lossy(c).into() } });
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
        self.tool_mut(sid, mid, 4, |p| {
            if let Part::Tool { input, purpose, .. } = p {
                *input = edit_input.clone();
                *purpose = "Fix the off-by-one in parse_items".into();
            }
        });
        let allowed = self.ask_permission(sid, mid, 4, "edit", "Fix the off-by-one in parse_items", edit_input.clone()).await;
        match allowed {
            None => return false,
            Some(false) => self.finish_tool(sid, mid, 4, ToolState::Denied, "The user denied this tool call.", None),
            Some(true) => {
                self.set_tool_state(sid, mid, 4, ToolState::Running);
                if !self.sleep_or_abort(sid, 300).await {
                    return false;
                }
                let diff = "--- a/src/parser.rs\n+++ b/src/parser.rs\n@@ -40,5 +40,5 @@\n fn parse_items(items: &[Item]) -> Result<Vec<Out>> {\n     let mut out = Vec::new();\n-    for i in 0..=items.len() {\n+    for i in 0..items.len() {\n         let item = &items[i];";
                self.finish_tool(sid, mid, 4, ToolState::Done, "Edited src/parser.rs [src/parser.rs#b7c2]\n  42 |     for i in 0..items.len() {", Some(json!({"path": "src/parser.rs", "diff": diff, "added": 1, "removed": 1})));
            }
        }
        // Tool: bash with delay then live output.
        let bash_input = json!({"command": "cargo test -p parser -- --nocapture\necho done", "timeout": 120});
        let mut p = tool_part("call_bash", "bash", "Run the unit tests", bash_input, ToolState::Delayed);
        if let Part::Tool { run_at, .. } = &mut p {
            *run_at = Some(now_ms() + 2000);
        }
        self.set_part(sid, mid, 5, p);
        if !self.sleep_or_abort(sid, 2000).await {
            return false;
        }
        self.set_tool_state(sid, mid, 5, ToolState::Running);
        let mut out = String::new();
        for i in 1..=24 {
            out.push_str(&format!("test parser::tests::case_{i:02} ... \x1b[32mok\x1b[0m\n"));
            let o = out.clone();
            self.tool_mut(sid, mid, 5, |p| {
                if let Part::Tool { meta, .. } = p {
                    *meta = Some(json!({"live_output": o}));
                }
            });
            if !self.sleep_or_abort(sid, 120).await {
                return false;
            }
        }
        out.push_str("\ntest result: ok. 42 passed; 0 failed; 0 ignored\ndone\n");
        self.finish_tool(sid, mid, 5, ToolState::Done, &out, Some(json!({"exit_code": 0, "duration_ms": 3120, "truncated": false})));
        // Final answer.
        self.set_status(sid, SessionStatus::Busy);
        self.set_part(sid, mid, 6, Part::Text { text: String::new(), time: now_ms() });
        self.stream(sid, mid, 6, ANSWER, false, 4, 18).await
    }

    /// Background worker: loops forever doing small tool calls.
    async fn worker_loop(&self, sid: String, label: &'static str) {
        let mut round = 0u32;
        loop {
            if !self.w.lock().unwrap().sessions.contains_key(&sid) {
                return;
            }
            round += 1;
            if let Some(s) = self.w.lock().unwrap().sessions.get(&sid) {
                s.abort.store(false, Ordering::Relaxed);
            }
            self.set_status(&sid, SessionStatus::Busy);
            let mid = self.new_assistant(&sid);
            self.set_part(&sid, &mid, 0, Part::Reasoning { text: String::new(), time: now_ms(), time_end: None, provider_data: None });
            let thought = format!("Round {round} for {label}: checking the next module, scanning for unused imports and dead code, then running the linter on the changed files to verify.");
            let mut ok = self.stream(&sid, &mid, 0, &thought, true, 2, 40).await;
            if ok {
                self.tool_mut(&sid, &mid, 0, |p| {
                    if let Part::Reasoning { time_end, .. } = p {
                        *time_end = Some(now_ms());
                    }
                });
                self.set_status(&sid, SessionStatus::Running);
                self.set_part(&sid, &mid, 1, tool_part(&id("call"), "bash", &format!("Lint module #{round}"), json!({"command": format!("cargo clippy -p mod{round}")}), ToolState::Running));
                ok = self.sleep_or_abort(&sid, 1500).await;
                if ok {
                    self.finish_tool(&sid, &mid, 1, ToolState::Done, "Finished: 0 warnings", Some(json!({"exit_code": 0})));
                    self.set_part(&sid, &mid, 2, Part::Text { text: String::new(), time: now_ms() });
                    ok = self.stream(&sid, &mid, 2, &format!("Module #{round} is **clean**. Moving on to the next one."), false, 3, 40).await;
                }
            }
            self.add_context(&sid, 3_000);
            self.finish(&sid, &mid, Usage { input: 500, output: 120, cache_read: 9000, cache_write: 0, reasoning: 40 }, (!ok).then(|| "Aborted".to_string()));
            self.set_status(&sid, SessionStatus::Idle);
            if !ok {
                // Aborted: wait for a new prompt (handled by Prompt) — idle for a while.
                tokio::time::sleep(Duration::from_secs(20)).await;
            } else {
                tokio::time::sleep(Duration::from_secs(4)).await;
            }
        }
    }

    async fn reply_turn(&self, sid: &str, text: &str) {
        self.set_status(sid, SessionStatus::Busy);
        let mid = self.new_assistant(sid);
        self.set_part(sid, mid.as_str(), 0, Part::Text { text: String::new(), time: now_ms() });
        let ans = format!("Got it: \"{}\". I'll take that into account.", crate::ui::text::truncate(&crate::ui::text::one_line(text), 60));
        let ok = self.stream(sid, &mid, 0, &ans, false, 2, 30).await;
        self.finish(sid, &mid, Usage { input: 100, output: 30, cache_read: 0, cache_write: 0, reasoning: 0 }, (!ok).then(|| "Aborted".to_string()));
        self.set_status(sid, SessionStatus::Idle);
    }
}

impl Mock {
    fn ok(&self, id: Option<u64>, result: Value) {
        if let Some(id) = id {
            self.send(ServerFrame::Response { id, ok: true, result, error: None });
        }
    }

    fn err(&self, id: Option<u64>, e: &str) {
        if let Some(id) = id {
            self.send(ServerFrame::Response { id, ok: false, result: Value::Null, error: Some(e.into()) });
        }
    }

    fn with_session<R>(&self, sid: &str, f: impl FnOnce(&mut MSession) -> R) -> Option<R> {
        self.w.lock().unwrap().sessions.get_mut(sid).map(f)
    }

    fn handle(&self, f: ClientFrame) {
        let id = f.id;
        match f.req {
            Request::Hello { .. } | Request::Ping => self.ok(id, json!({})),
            Request::GetConfig => self.ok(id, json!({ "config": config() })),
            Request::ListSessions => {
                let w = self.w.lock().unwrap();
                let list: Vec<&SessionInfo> = w.order.iter().filter_map(|i| w.sessions.get(i)).map(|s| &s.info).collect();
                self.ok(id, json!({ "sessions": list }));
            }
            Request::ListHistory { .. } => {
                let w = self.w.lock().unwrap();
                let mut list: Vec<SessionInfo> = w.history.clone();
                list.extend(w.order.iter().filter_map(|i| w.sessions.get(i)).map(|s| s.info.clone()));
                list.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
                self.ok(id, json!({ "sessions": list }));
            }
            Request::CreateSession { cwd, agent, name, model } => {
                let base = std::path::Path::new(&cwd).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "ref".into());
                let i = self.create(&name.unwrap_or_else(|| format!("{base}-{:04x}", SEQ.load(Ordering::Relaxed) * 7919 % 65536)), agent.as_deref().unwrap_or("build"), &cwd);
                if let Some(m) = model {
                    self.with_session(&i.id, |s| s.info.model = m);
                }
                let i = self.with_session(&i.id, |s| s.info.clone()).unwrap_or(i);
                self.ok(id, json!({ "session": i }));
                self.broadcast_sessions();
            }
            Request::Attach { session_id, foreground } => {
                // History-only session → load it live.
                let hist = {
                    let mut w = self.w.lock().unwrap();
                    let pos = w.history.iter().position(|h| h.id == session_id);
                    pos.map(|p| w.history.remove(p))
                };
                if let Some(mut h) = hist {
                    h.live = true;
                    let msgs = vec![
                        Message { id: id_s("msg"), role: Role::User, time: h.created_at, time_end: None, source: MessageSource::Human, parts: vec![Part::Text { text: "Summarise the README".into(), time: h.created_at }], model: None, agent: None, usage: None, error: None },
                        Message { id: id_s("msg"), role: Role::Assistant, time: h.created_at + 1000, time_end: Some(h.created_at + 5000), source: MessageSource::Human, parts: vec![Part::Text { text: "The README describes **ref**, a coding agent harness with a TUI frontend and a backend server.".into(), time: h.created_at + 1000 }], model: Some(model()), agent: Some("build".into()), usage: Some(Usage { input: 300, output: 40, ..Default::default() }), error: None },
                    ];
                    let mut w = self.w.lock().unwrap();
                    w.order.push(h.id.clone());
                    w.sessions.insert(h.id.clone(), MSession { info: h, messages: msgs, perms: vec![], abort: Arc::new(AtomicBool::new(false)), queue: Vec::new() });
                }
                let snap = {
                    let mut w = self.w.lock().unwrap();
                    w.subs.insert(session_id.clone());
                    match w.sessions.get_mut(&session_id) {
                        Some(s) => {
                            if foreground {
                                s.info.attached += 1;
                            }
                            Some(SessionSnapshot { info: s.info.clone(), messages: s.messages.clone(), pending_permissions: s.perms.clone(), queued: s.queue.clone() })
                        }
                        None => None,
                    }
                };
                match snap {
                    Some(s) => {
                        self.ok(id, json!({ "snapshot": s }));
                        self.broadcast_sessions();
                    }
                    None => self.err(id, "no such session"),
                }
            }
            Request::Detach { session_id } => {
                {
                    let mut w = self.w.lock().unwrap();
                    w.subs.remove(&session_id);
                    if let Some(s) = w.sessions.get_mut(&session_id) {
                        s.info.attached = s.info.attached.saturating_sub(1);
                    }
                }
                self.ok(id, json!({}));
                self.broadcast_sessions();
            }
            Request::WatchAll { enabled } => {
                self.w.lock().unwrap().watch_all = enabled;
                self.ok(id, json!({}));
            }
            Request::Prompt { session_id, text, images } => {
                let mid = id_s("msg");
                let busy = self.with_session(&session_id, |s| {
                    s.abort.store(false, Ordering::Relaxed);
                    let busy = !s.info.status.is_idle();
                    if busy {
                        s.queue.push(user_msg(mid.clone(), &text, MessageSource::Human, images.clone()));
                    }
                    busy
                });
                self.ok(id, json!({ "queued": busy == Some(true), "message_id": mid }));
                let m = self.clone();
                tokio::spawn(async move {
                    // Queued while busy: wait until idle (the client shows it as queued meanwhile).
                    if busy == Some(true) {
                        for _ in 0..600 {
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            if m.with_session(&session_id, |s| s.info.status.is_idle()).unwrap_or(true) {
                                break;
                            }
                        }
                    }
                    if busy == Some(true) {
                        // Withdrawn with `RemoveQueued` meanwhile: never delivered.
                        let queued = m.with_session(&session_id, |s| {
                            let i = s.queue.iter().position(|q| q.id == mid);
                            i.map(|i| s.queue.remove(i)).is_some()
                        });
                        if queued != Some(true) {
                            return;
                        }
                    }
                    m.user_message_id(&session_id, mid, &text, MessageSource::Human, images);
                    let first = m.with_session(&session_id, |s| s.messages.iter().filter(|x| x.role == Role::User).count() <= 1).unwrap_or(false);
                    if first || text.contains("demo") {
                        m.demo_turn(&session_id, &text).await;
                    } else {
                        m.reply_turn(&session_id, &text).await;
                    }
                });
            }
            Request::RemoveQueued { session_id, message_id } => {
                let removed = self.with_session(&session_id, |s| {
                    let i = s.queue.iter().position(|q| q.id == message_id);
                    i.map(|i| s.queue.remove(i)).is_some()
                });
                match removed {
                    Some(removed) => self.ok(id, json!({ "removed": removed })),
                    None => self.err(id, "no such session"),
                }
            }
            Request::Shell { session_id, command } => {
                self.ok(id, json!({}));
                let out = format!("I ran this command:\n```\n$ {command}\n(mock) command output line 1\n(mock) line 2\n```");
                self.user_message(&session_id, &out, MessageSource::Shell, vec![]);
            }
            Request::Abort { session_id } => {
                self.with_session(&session_id, |s| s.abort.store(true, Ordering::Relaxed));
                self.ok(id, json!({}));
            }
            Request::PermissionReply { call_id, allow, .. } => {
                if let Some(tx) = self.w.lock().unwrap().perm_waiters.remove(&call_id) {
                    let _ = tx.send(allow);
                }
                self.ok(id, json!({}));
            }
            Request::SetModel { session_id, model } => {
                let info = self.with_session(&session_id, |s| {
                    s.info.model = model;
                    s.info.clone()
                });
                if let Some(info) = info {
                    self.emit(&session_id, SessionEvent::Info { info: info.clone() });
                    self.ok(id, json!({ "session": info }));
                    self.broadcast_sessions();
                }
            }
            Request::SetAgent { session_id, agent } => {
                let am = config().agents.iter().find(|a| a.name == agent).and_then(|a| a.model.clone());
                let info = self.with_session(&session_id, |s| {
                    s.info.agent = agent;
                    s.info.model = am.unwrap_or_else(model);
                    s.info.clone()
                });
                if let Some(info) = info {
                    self.emit(&session_id, SessionEvent::Info { info: info.clone() });
                    self.ok(id, json!({ "session": info }));
                    self.broadcast_sessions();
                }
            }
            Request::SetVariant { session_id, variant } => {
                let cfg = config();
                let info = self.with_session(&session_id, |s| {
                    let vs = cfg.models.iter().find(|m| m.provider == s.info.model.provider && m.id == s.info.model.model).map(|m| m.variants.clone()).unwrap_or_default();
                    s.info.model.variant = match variant {
                        Some(v) => Some(v),
                        None => {
                            let i = s.info.model.variant.as_ref().and_then(|v| vs.iter().position(|x| x == v));
                            match i {
                                None => vs.first().cloned(),
                                Some(i) if i + 1 < vs.len() => Some(vs[i + 1].clone()),
                                Some(_) => None,
                            }
                        }
                    };
                    s.info.clone()
                });
                if let Some(info) = info {
                    self.emit(&session_id, SessionEvent::Info { info: info.clone() });
                    self.ok(id, json!({ "session": info }));
                }
            }
            Request::PeerCommand { command, .. } => {
                let text = match command.as_str() {
                    "peers" => "You are \"mock\" (ses_mock).\nOther live sessions (1):\n  [idle] worker · \"demo\" · general · background · /tmp · started 2m ago".to_string(),
                    "peers-inbox" => "Inbox is empty: no messages from other sessions.".to_string(),
                    "peers-outbox" => "Outbox is empty: this session has not sent any messages.".to_string(),
                    _ => "Current name: \"mock\"".to_string(),
                };
                self.ok(id, json!({ "text": text }));
            }
            Request::Rename { session_id, name } => {
                let info = self.with_session(&session_id, |s| {
                    s.info.name = name;
                    s.info.clone()
                });
                if let Some(info) = info {
                    self.emit(&session_id, SessionEvent::Info { info });
                }
                self.ok(id, json!({}));
                self.broadcast_sessions();
            }
            Request::Compact { session_id } => {
                self.ok(id, json!({}));
                let m = self.clone();
                tokio::spawn(async move {
                    m.set_status(&session_id, SessionStatus::Compacting);
                    tokio::time::sleep(Duration::from_millis(1500)).await;
                    let summary = Message {
                        id: id_s("msg"),
                        role: Role::User,
                        time: now_ms(),
                        time_end: None,
                        source: MessageSource::Compaction,
                        parts: vec![Part::Text { text: "[Context compacted. Summary of the conversation so far:]\n\n## Goal\nFix the parser off-by-one.\n\n## Done\n- Fixed loop bound\n- Tests pass\n\n[Continue from here.]".into(), time: now_ms() }],
                        model: None,
                        agent: None,
                        usage: None,
                        error: None,
                    };
                    // Append-only: older messages stay visible; the model context starts here.
                    m.emit(&session_id, SessionEvent::Message { message: summary });
                    m.with_session(&session_id, |s| s.info.context.used = 4_000);
                    m.set_status(&session_id, SessionStatus::Idle);
                });
            }
            Request::Undo { session_id, message_id } => {
                let r = self.with_session(&session_id, |s| {
                    let pos = match &message_id {
                        Some(mid) => s.messages.iter().position(|m| m.id == *mid)?,
                        None => s.messages.iter().rposition(|m| m.role == Role::User && m.source == MessageSource::Human)?,
                    };
                    let text = if s.messages[pos].source == MessageSource::Human { s.messages[pos].raw_text() } else { String::new() };
                    s.messages.truncate(pos);
                    Some((text, s.messages.clone()))
                });
                match r.flatten() {
                    Some((text, msgs)) => {
                        self.emit(&session_id, SessionEvent::Reset { messages: msgs });
                        self.ok(id, json!({ "text": text }));
                    }
                    None => self.err(id, "nothing to undo"),
                }
            }
            Request::Close { session_id } => {
                let info = {
                    let mut w = self.w.lock().unwrap();
                    w.order.retain(|x| *x != session_id);
                    w.sessions.remove(&session_id).map(|s| s.info)
                };
                if let Some(mut i) = info {
                    i.live = false;
                    i.attached = 0;
                    self.send(ServerFrame::Event { session_id: session_id.clone(), event: SessionEvent::Closed });
                    self.w.lock().unwrap().history.push(i);
                }
                self.ok(id, json!({}));
                self.broadcast_sessions();
            }
            Request::Delete { session_id } => {
                self.w.lock().unwrap().history.retain(|h| h.id != session_id);
                self.ok(id, json!({}));
            }
            Request::ExpandCommand { name, args, .. } => {
                self.ok(id, json!({ "text": format!("(expanded /{name}) Please do the {name} task. {args}") }));
            }
            Request::ListFiles { query, .. } => {
                let all = ["src/main.rs", "src/app.rs", "src/parser.rs", "src/ui/", "src/ui/chat.rs", "src/ui/markdown.rs", "Cargo.toml", "README.md", "docs/设计.md"];
                let mut v: Vec<(i64, &str)> = all.iter().filter_map(|f| crate::ui::text::fuzzy_score(f, &query).map(|s| (s, *f))).collect();
                v.sort_by_key(|x| std::cmp::Reverse(x.0));
                let files: Vec<&str> = v.into_iter().map(|x| x.1).collect();
                self.ok(id, json!({ "files": files }));
            }
            Request::Export { path, .. } => self.ok(id, json!({ "path": path })),
            Request::Shutdown => self.ok(id, json!({})),
            Request::McpStatus { .. } => {
                let started = {
                    let mut w = self.w.lock().unwrap();
                    if w.mcp_started == 0 {
                        w.mcp_started = now_ms();
                    }
                    w.mcp_started
                };
                let docs_ready = now_ms() - started > 8_000;
                let servers = vec![
                    McpServerStatus { name: "context7".into(), status: "connected".into(), error: None, tools: vec!["context7_resolve-library-id".into(), "context7_get-library-docs".into()] },
                    McpServerStatus {
                        name: "docs".into(),
                        status: if docs_ready { "connected" } else { "connecting" }.into(),
                        error: None,
                        tools: if docs_ready { vec!["docs_search".into()] } else { vec![] },
                    },
                    McpServerStatus { name: "github".into(), status: "failed".into(), error: Some("HTTP 401 Unauthorized: missing or invalid bearer token (set GITHUB_TOKEN)".into()), tools: vec![] },
                    McpServerStatus { name: "sqlite".into(), status: "disabled".into(), error: None, tools: vec![] },
                ];
                self.ok(id, json!({ "servers": servers }));
            }
        }
    }

    /// Seeds background sessions and history.
    fn seed(&self, cwd: &str) {
        let a = self.create("worker-1", "build", cwd);
        let b = self.create("reviewer", "review", cwd);
        let c = self.create("docs-写手", "plan", cwd);
        for (s, t) in [(&a, "Clean up lints across modules"), (&b, "Review the parser change"), (&c, "Draft the design doc")] {
            self.with_session(&s.id, |x| {
                x.info.title = t.into();
                x.info.parent = Some("ses_parent".into());
                x.info.context.used = 120_000;
            });
        }
        let now = now_ms();
        for (i, (t, n)) in [("Refactor the config loader", "ref-1a2b"), ("Investigate flaky websocket test", "ref-9f00"), ("实现会话列表", "ref-77cc")].iter().enumerate() {
            let mut h = info(n, "build", cwd);
            h.title = t.to_string();
            h.live = false;
            h.created_at = now - (i as i64 + 1) * 3_600_000 * 7;
            h.updated_at = h.created_at + 600_000;
            self.w.lock().unwrap().history.push(h);
        }
        // Peer message into the reviewer.
        self.user_message(&b.id, "[Message from session \"worker-1\" (id ses_x)]\nPlease review the parser change in src/parser.rs.\n[Reply with send_message to \"ses_x\" if a reply is needed.]", MessageSource::Peer { from_id: a.id.clone(), from_name: "worker-1".into(), msg_id: "msg_p".into() }, vec![]);
        for (sid, label) in [(a.id.clone(), "lint"), (b.id.clone(), "review")] {
            let m = self.clone();
            tokio::spawn(async move { m.worker_loop(sid, label).await });
        }
        // The third one asks for permission periodically.
        let m = self.clone();
        let cid = c.id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(6)).await;
            m.user_message(&cid, "Write docs/design.md describing the architecture", MessageSource::Spawn { from_id: "ses_parent".into(), from_name: "main".into() }, vec![]);
            m.set_status(&cid, SessionStatus::Busy);
            let mid = m.new_assistant(&cid);
            m.set_part(&cid, &mid, 0, Part::Text { text: String::new(), time: now_ms() });
            m.stream(&cid, &mid, 0, "I'll write the design document now.", false, 3, 40).await;
            let input = json!({"path": "docs/design.md", "content": "# Design\n\nref consists of a backend server and TUI frontends.\n"});
            m.set_part(&cid, &mid, 1, tool_part("call_write_doc", "write", "Create the design document", input.clone(), ToolState::Streaming));
            let r = m.ask_permission(&cid, &mid, 1, "write", "Create the design document", input).await;
            match r {
                Some(true) => m.finish_tool(&cid, &mid, 1, ToolState::Done, "Wrote docs/design.md (3 lines)", Some(json!({"diff": "+++ b/docs/design.md\n+# Design\n+\n+ref consists of a backend server and TUI frontends.", "created": true}))),
                Some(false) => m.finish_tool(&cid, &mid, 1, ToolState::Denied, "The user denied this tool call.", None),
                None => {}
            }
            m.finish(&cid, &mid, Usage { input: 800, output: 90, ..Default::default() }, r.is_none().then(|| "Aborted".to_string()));
            m.set_status(&cid, SessionStatus::Idle);
        });
    }
}

fn user_msg(mid: String, text: &str, source: MessageSource, images: Vec<ImageData>) -> Message {
    let mut parts = vec![Part::Text { text: text.to_string(), time: now_ms() }];
    for (i, img) in images.into_iter().enumerate() {
        parts.push(Part::Image { image: img, name: Some(format!("image-{}", i + 1)) });
    }
    Message { id: mid, role: Role::User, time: now_ms(), time_end: None, source, parts, model: None, agent: None, usage: None, error: None }
}

fn id_s(p: &str) -> String {
    id(p)
}

/// Runs the mock backend.
pub async fn run(cwd: String, mut rx: mpsc::UnboundedReceiver<ClientFrame>, tx: mpsc::UnboundedSender<NetEvent>) {
    let m = Mock { w: Arc::new(Mutex::new(World::default())), tx: tx.clone() };
    m.seed(&cwd);
    tokio::time::sleep(Duration::from_millis(150)).await;
    let _ = tx.send(NetEvent::Connected);
    while let Some(f) = rx.recv().await {
        m.handle(f);
    }
}
