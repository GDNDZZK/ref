//! `ref --mock`: an in-process fake backend producing realistic streaming so the UI can be
//! exercised without `ref-server`. Speaks the same channel interface as `net::run`.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ref_proto::*;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};

use crate::app::cmd;
use crate::net::NetEvent;
use crate::util::{ABORTED_ERROR, mcp_status};

struct MockSession {
    info: SessionInfo,
    messages: Vec<Message>,
    perms: Vec<PermissionRequest>,
    abort: Arc<AtomicBool>,
    /// Prompts waiting for the session to become idle.
    queue: Vec<Message>,
}

#[derive(Default)]
struct World {
    sessions: HashMap<String, MockSession>,
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
    world: Arc<Mutex<World>>,
    tx: mpsc::UnboundedSender<NetEvent>,
}

/// How a text is streamed: the delta kind, characters per delta and the pause between deltas.
struct StreamSpec {
    make_delta: fn(String) -> Delta,
    chunk: usize,
    delay_ms: u64,
}

impl StreamSpec {
    fn text(chunk: usize, delay_ms: u64) -> Self {
        StreamSpec {
            make_delta: |text| Delta::Text { text },
            chunk,
            delay_ms,
        }
    }

    fn reasoning(chunk: usize, delay_ms: u64) -> Self {
        StreamSpec {
            make_delta: |text| Delta::Reasoning { text },
            chunk,
            delay_ms,
        }
    }
}

impl Mock {
    /// Locks the world; a poisoned lock means another handler panicked.
    fn world(&self) -> std::sync::MutexGuard<'_, World> {
        self.world.lock().expect("mock world poisoned")
    }
}

static SEQ: AtomicU64 = AtomicU64::new(1);

fn id(prefix: &str) -> String {
    format!(
        "{prefix}_{:x}{:04}",
        now_ms(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    )
}

fn model() -> ModelRef {
    ModelRef {
        provider: "dgn".into(),
        model: "claude-opus-5.5".into(),
        variant: Some("high".into()),
    }
}

fn info(name: &str, agent: &str, cwd: &str) -> SessionInfo {
    let t = now_ms();
    SessionInfo {
        name: name.into(),
        agent: agent.into(),
        model: model(),
        created_at: t,
        updated_at: t,
        context: ContextUsage {
            used: 0,
            max: 1_000_000,
            recommend: 600_000,
            auto: 850_000,
        },
        ..crate::app::placeholder_info(&id("ses"), cwd)
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
            AgentInfo {
                name: "build".into(),
                description: "Default agent with all tools".into(),
                model: None,
                creatable: true,
                can_spawn: true,
                color: None,
            },
            AgentInfo {
                name: "plan".into(),
                description: "Read-only planning agent".into(),
                model: None,
                creatable: true,
                can_spawn: false,
                color: None,
            },
            AgentInfo {
                name: "review".into(),
                description: "Reviews diffs and reports issues".into(),
                model: Some(ModelRef {
                    provider: "openai".into(),
                    model: "gpt-6".into(),
                    variant: None,
                }),
                creatable: true,
                can_spawn: false,
                color: Some("#56b6c2".into()),
            },
        ],
        default_agent: "build".into(),
        models: vec![
            m(
                "dgn",
                "DGN Gateway",
                "claude-opus-5.5",
                "Claude Opus 5.5",
                1_000_000,
                &["low", "medium", "high"],
            ),
            m(
                "dgn",
                "DGN Gateway",
                "claude-sonnet-5",
                "Claude Sonnet 5",
                400_000,
                &["low", "high"],
            ),
            m(
                "openai",
                "OpenAI",
                "gpt-6",
                "GPT-6",
                512_000,
                &["minimal", "low", "medium", "high"],
            ),
            m(
                "local",
                "Local",
                "qwen4-coder",
                "Qwen4 Coder 32B",
                128_000,
                &[],
            ),
        ],
        commands: vec![
            CommandInfo {
                name: "review".into(),
                description: "Review uncommitted changes".into(),
                source: "config".into(),
            },
            CommandInfo {
                name: "init".into(),
                description: "Create AGENTS.md for this project".into(),
                source: "builtin".into(),
            },
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
            let w = self.world();
            w.order
                .iter()
                .filter_map(|i| w.sessions.get(i))
                .map(|s| s.info.clone())
                .collect()
        };
        self.send(ServerFrame::Sessions { sessions: list });
    }

    /// Applies an event to the world and forwards it if subscribed.
    fn emit(&self, sid: &str, ev: SessionEvent) {
        let deliver = {
            let mut w = self.world();
            let watch_all = w.watch_all;
            let subscribed = w.subs.contains(sid);
            let Some(s) = w.sessions.get_mut(sid) else {
                return;
            };
            match &ev {
                SessionEvent::Info { info } => s.info = info.clone(),
                SessionEvent::Message { message } => {
                    match s.messages.iter_mut().find(|m| m.id == message.id) {
                        Some(m) => *m = message.clone(),
                        None => s.messages.push(message.clone()),
                    }
                }
                SessionEvent::Part {
                    message_id,
                    index,
                    part,
                } => {
                    if let Some(m) = s.messages.iter_mut().find(|m| &m.id == message_id) {
                        if *index < m.parts.len() {
                            m.parts[*index] = part.clone();
                        } else {
                            m.parts.push(part.clone());
                        }
                    }
                }
                SessionEvent::Delta {
                    message_id,
                    index,
                    delta,
                } => {
                    if let Some(m) = s.messages.iter_mut().find(|m| &m.id == message_id) {
                        crate::state::apply_delta(&mut m.parts, *index, delta.clone());
                    }
                }
                SessionEvent::Removed { message_id } => s.messages.retain(|m| &m.id != message_id),
                SessionEvent::Reset { messages, .. } => s.messages = messages.clone(),
                SessionEvent::Permission { request } => s.perms.push(request.clone()),
                SessionEvent::PermissionResolved { call_id } => {
                    s.perms.retain(|p| &p.call_id != call_id)
                }
                _ => {}
            }
            subscribed || (watch_all && s.info.attached == 0)
        };
        if deliver {
            self.send(ServerFrame::Event {
                session_id: sid.to_string(),
                event: ev,
            });
        }
    }

    fn set_status(&self, sid: &str, st: SessionStatus) {
        let info = self.update_info(sid, |info| {
            // Like the server: the run start, kept while busy (Busy / Running / ...).
            if st.is_idle() {
                info.busy_since = None;
            } else if info.status.is_idle() || info.busy_since.is_none() {
                info.busy_since = Some(now_ms());
            }
            info.status = st;
            info.updated_at = now_ms();
        });
        let Some(info) = info else {
            return;
        };
        self.emit(sid, SessionEvent::Info { info });
        self.broadcast_sessions();
    }

    fn add_context(&self, sid: &str, n: u64) {
        let Some(info) = self.update_info(sid, |info| info.context.used += n) else {
            return;
        };
        self.emit(sid, SessionEvent::Info { info });
    }

    fn aborted(&self, sid: &str) -> bool {
        self.world()
            .sessions
            .get(sid)
            .is_none_or(|s| s.abort.load(Ordering::Relaxed))
    }

    fn create(&self, name: &str, agent: &str, cwd: &str) -> SessionInfo {
        let mut w = self.world();
        let mut name = name.to_string();
        let mut k = 2;
        while w.sessions.values().any(|s| s.info.name == name) {
            name = format!(
                "{}-{k}",
                name.trim_end_matches(char::is_numeric)
                    .trim_end_matches('-')
            );
            k += 1;
        }
        let i = info(&name, agent, cwd);
        w.order.push(i.id.clone());
        w.sessions.insert(
            i.id.clone(),
            MockSession {
                info: i.clone(),
                messages: Vec::new(),
                perms: Vec::new(),
                abort: Arc::new(AtomicBool::new(false)),
                queue: Vec::new(),
            },
        );
        i
    }

    fn user_message(&self, sid: &str, text: &str, source: MessageSource, images: Vec<ImageData>) {
        self.user_message_id(sid, id("msg"), text, source, images);
    }

    fn user_message_id(
        &self,
        sid: &str,
        mid: String,
        text: &str,
        source: MessageSource,
        images: Vec<ImageData>,
    ) {
        let m = user_msg(mid, text, source, images);
        {
            let mut w = self.world();
            if let Some(s) = w.sessions.get_mut(sid)
                && s.info.title.is_empty()
            {
                s.info.title = text.lines().next().unwrap_or("").chars().take(50).collect();
            }
        }
        self.emit(sid, SessionEvent::Message { message: m });
    }

    fn agent_of(&self, sid: &str) -> String {
        self.world()
            .sessions
            .get(sid)
            .map(|s| s.info.agent.clone())
            .unwrap_or_default()
    }

    fn new_assistant(&self, sid: &str) -> String {
        let model = self.world().sessions.get(sid).map(|s| s.info.model.clone());
        let mid = id("msg");
        let m = Message {
            model,
            agent: Some(self.agent_of(sid)),
            ..new_message(
                mid.clone(),
                Role::Assistant,
                now_ms(),
                MessageSource::Human,
                vec![],
            )
        };
        self.emit(sid, SessionEvent::Message { message: m });
        mid
    }

    fn message(&self, sid: &str, mid: &str) -> Option<Message> {
        self.world()
            .sessions
            .get(sid)?
            .messages
            .iter()
            .find(|m| m.id == mid)
            .cloned()
    }

    fn finish(&self, sid: &str, mid: &str, usage: Usage, error: Option<String>) {
        if let Some(mut m) = self.message(sid, mid) {
            m.time_end = Some(now_ms());
            m.usage = Some(usage);
            m.error = error;
            if m.error.is_some() {
                for p in &mut m.parts {
                    if let Part::Tool {
                        state,
                        output,
                        time_end,
                        ..
                    } = p
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
    async fn stream(&self, sid: &str, mid: &str, idx: usize, text: &str, spec: StreamSpec) -> bool {
        let chars: Vec<char> = text.chars().collect();
        for c in chars.chunks(spec.chunk.max(1)) {
            if self.aborted(sid) {
                return false;
            }
            let delta = (spec.make_delta)(c.iter().collect());
            self.emit(
                sid,
                SessionEvent::Delta {
                    message_id: mid.to_string(),
                    index: idx,
                    delta,
                },
            );
            tokio::time::sleep(Duration::from_millis(spec.delay_ms)).await;
        }
        true
    }

    async fn sleep_or_abort(&self, sid: &str, ms: u64) -> bool {
        let steps = ms / ABORT_POLL_MS;
        for _ in 0..steps.max(1) {
            if self.aborted(sid) {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(ABORT_POLL_MS)).await;
        }
        !self.aborted(sid)
    }
}

/// How often sleeping mock steps check for an abort.
const ABORT_POLL_MS: u64 = 50;
const PERMISSION_POLL: Duration = Duration::from_millis(100);
const STARTUP_DELAY: Duration = Duration::from_millis(150);
/// Age gap between consecutive seeded history sessions (7 hours).
const HISTORY_SPACING_MS: i64 = 7 * 3_600_000;
/// Length of a seeded history session.
const HISTORY_SESSION_SPAN_MS: i64 = 600_000;
const SEEDED_WORKER_CONTEXT_TOKENS: u64 = 120_000;
const DOCS_WRITER_DELAY: Duration = Duration::from_secs(6);
/// The idle pause between worker rounds, and after an aborted round.
const WORKER_PAUSE: Duration = Duration::from_secs(4);
const WORKER_ABORTED_PAUSE: Duration = Duration::from_secs(20);
const WORKER_TOOL_MS: u64 = 1500;
const DEMO_TURN_CONTEXT_TOKENS: u64 = 45_000;
const WORKER_ROUND_CONTEXT_TOKENS: u64 = 3_000;
const DEMO_USAGE: Usage = Usage {
    input: 1834,
    output: 912,
    cache_read: 40_210,
    cache_write: 2048,
    reasoning: 230,
};
const WORKER_USAGE: Usage = Usage {
    input: 500,
    output: 120,
    cache_read: 9000,
    cache_write: 0,
    reasoning: 40,
};
const REPLY_USAGE: Usage = Usage {
    input: 100,
    output: 30,
    cache_read: 0,
    cache_write: 0,
    reasoning: 0,
};

// Part indices of the demo turn.
const PART_REASONING: usize = 0;
const PART_INTRO: usize = 1;
const PART_READ: usize = 2;
const PART_GREP: usize = 3;
const PART_EDIT: usize = 4;
const PART_BASH: usize = 5;
const PART_ANSWER: usize = 6;
// Part indices of a worker round (the reasoning part is `PART_REASONING`).
const WORKER_TOOL_PART: usize = 1;
const WORKER_TEXT_PART: usize = 2;

// Pauses of the demo turn.
const THINK_DELAY: Duration = Duration::from_millis(400);
const READ_MS: u64 = 600;
const GREP_MS: u64 = 400;
const EDIT_RUN_MS: u64 = 300;
const BASH_START_MS: u64 = 2000;
const BASH_LINE_MS: u64 = 120;
const TOOL_INPUT_DELAY: Duration = Duration::from_millis(30);
const TOOL_INPUT_CHUNK_BYTES: usize = 8;

// The file the demo turn reads and edits.
const PARSER_PATH: &str = "src/parser.rs";
const PARSER_TAG: &str = "a1f3";
const BUGGY_LINE: usize = 42;
const READ_LINES: usize = 260;
const BASH_TEST_COUNT: usize = 24;
const EDIT_PURPOSE: &str = "Fix the off-by-one in parse_items";
const DENIED_OUTPUT: &str = "The user denied this tool call.";

const PEERS_TEXT: &str = "You are \"mock\" (ses_mock).\nOther live sessions (1):\n  [idle] worker · \"demo\" · general · background · /tmp · started 2m ago";
const PEERS_INBOX_TEXT: &str = "Inbox is empty: no messages from other sessions.";
const PEERS_OUTBOX_TEXT: &str = "Outbox is empty: this session has not sent any messages.";
const PEER_NAME_TEXT: &str = "Current name: \"mock\"";
const COMPACTION_SUMMARY: &str = "[Context compacted. Summary of the conversation so far:]\n\n## Goal\nFix the parser off-by-one.\n\n## Done\n- Fixed loop bound\n- Tests pass\n\n[Continue from here.]";
const MOCK_FILES: [&str; 9] = [
    "src/main.rs",
    "src/app.rs",
    "src/parser.rs",
    "src/ui/",
    "src/ui/chat.rs",
    "src/ui/markdown.rs",
    "Cargo.toml",
    "README.md",
    "docs/设计.md",
];
/// Context size right after a compaction.
const COMPACTED_CONTEXT_TOKENS: u64 = 4_000;
const COMPACT_DELAY: Duration = Duration::from_millis(1500);
/// The "docs" MCP server finishes connecting this long after the first status request.
const MCP_CONNECT_DELAY_MS: i64 = 8_000;
/// A queued prompt waits up to `QUEUE_WAIT_POLLS * QUEUE_POLL` (60 s) for the session to be idle.
const QUEUE_WAIT_POLLS: u32 = 600;
const QUEUE_POLL: Duration = Duration::from_millis(100);
const NAME_HASH_MULTIPLIER: u64 = 7919;
const NAME_HASH_MODULUS: u64 = 65536;

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
        self.emit(
            sid,
            SessionEvent::Part {
                message_id: mid.into(),
                index: idx,
                part,
            },
        );
    }

    fn part_mut(&self, sid: &str, mid: &str, idx: usize, f: impl FnOnce(&mut Part)) {
        if let Some(m) = self.message(sid, mid)
            && let Some(mut p) = m.parts.get(idx).cloned()
        {
            f(&mut p);
            self.set_part(sid, mid, idx, p);
        }
    }

    fn start_text_part(&self, sid: &str, mid: &str, idx: usize) {
        let part = Part::Text {
            text: String::new(),
            time: now_ms(),
        };
        self.set_part(sid, mid, idx, part);
    }

    fn start_reasoning_part(&self, sid: &str, mid: &str, idx: usize) {
        let part = Part::Reasoning {
            text: String::new(),
            time: now_ms(),
            time_end: None,
            provider_data: None,
        };
        self.set_part(sid, mid, idx, part);
    }

    fn end_reasoning_part(&self, sid: &str, mid: &str, idx: usize) {
        self.part_mut(sid, mid, idx, |p| {
            if let Part::Reasoning { time_end, .. } = p {
                *time_end = Some(now_ms());
            }
        });
    }

    fn finish_tool(
        &self,
        sid: &str,
        mid: &str,
        idx: usize,
        state: ToolState,
        output: &str,
        meta: Option<Value>,
    ) {
        self.part_mut(sid, mid, idx, |p| {
            if let Part::Tool {
                state: s,
                output: o,
                meta: m,
                time_end,
                ..
            } = p
            {
                *s = state;
                *o = Some(output.into());
                *m = meta;
                *time_end = Some(now_ms());
            }
        });
    }

    fn set_tool_state(&self, sid: &str, mid: &str, idx: usize, state: ToolState) {
        self.part_mut(sid, mid, idx, |p| {
            if let Part::Tool { state: s, .. } = p {
                *s = state;
            }
        });
    }

    async fn ask_permission(
        &self,
        sid: &str,
        mid: &str,
        idx: usize,
        tool: &str,
        purpose: &str,
        input: Value,
    ) -> Option<bool> {
        self.set_tool_state(sid, mid, idx, ToolState::AwaitingPermission);
        self.set_status(sid, SessionStatus::WaitingPermission);
        let call_id = match self
            .message(sid, mid)
            .and_then(|m| m.parts.get(idx).cloned())
        {
            Some(Part::Tool { call_id, .. }) => call_id,
            _ => return None,
        };
        let (tx, mut rx) = oneshot::channel();
        self.world().perm_waiters.insert(call_id.clone(), tx);
        let req = PermissionRequest {
            session_id: sid.into(),
            message_id: mid.into(),
            call_id: call_id.clone(),
            tool: tool.into(),
            purpose: purpose.into(),
            input,
            time: now_ms(),
        };
        self.emit(sid, SessionEvent::Permission { request: req });
        let res = loop {
            tokio::select! {
                r = &mut rx => break r.ok(),
                _ = tokio::time::sleep(PERMISSION_POLL) => {
                    if self.aborted(sid) { break None; }
                }
            }
        };
        self.world().perm_waiters.remove(&call_id);
        self.emit(sid, SessionEvent::PermissionResolved { call_id });
        self.set_status(sid, SessionStatus::Running);
        res
    }

    /// The main demo turn.
    async fn demo_turn(&self, sid: &str, prompt: &str) {
        self.set_status(sid, SessionStatus::Busy);
        let mid = self.new_assistant(sid);
        let ok = self.turn_body(sid, &mid, prompt).await;
        self.add_context(sid, DEMO_TURN_CONTEXT_TOKENS);
        self.finish(
            sid,
            &mid,
            DEMO_USAGE,
            (!ok).then(|| ABORTED_ERROR.to_string()),
        );
        self.set_status(sid, SessionStatus::Idle);
    }

    /// Plays every step in order; each step returns false when aborted.
    async fn turn_body(&self, sid: &str, mid: &str, prompt: &str) -> bool {
        tokio::time::sleep(THINK_DELAY).await;
        self.reasoning_step(sid, mid).await
            && self.intro_step(sid, mid, prompt).await
            && self.read_step(sid, mid).await
            && self.grep_step(sid, mid).await
            && self.edit_step(sid, mid).await
            && self.bash_step(sid, mid).await
            && self.answer_step(sid, mid).await
    }

    async fn reasoning_step(&self, sid: &str, mid: &str) -> bool {
        self.start_reasoning_part(sid, mid, PART_REASONING);
        let spec = StreamSpec::reasoning(3, 25);
        if !self.stream(sid, mid, PART_REASONING, REASONING, spec).await {
            return false;
        }
        self.end_reasoning_part(sid, mid, PART_REASONING);
        true
    }

    async fn intro_step(&self, sid: &str, mid: &str, prompt: &str) -> bool {
        self.start_text_part(sid, mid, PART_INTRO);
        let intro = if prompt.to_lowercase().contains("hello") {
            "Hi! Let me take a look at the project first."
        } else {
            "Let me look at the parser first."
        };
        self.stream(sid, mid, PART_INTRO, intro, StreamSpec::text(2, 20))
            .await
    }

    /// Tool: read (done).
    async fn read_step(&self, sid: &str, mid: &str) -> bool {
        self.set_status(sid, SessionStatus::Running);
        let input = json!({"path": PARSER_PATH, "offset": 1, "limit": 200});
        let part = tool_part(
            "call_read",
            "read",
            "Read the parser to find the loop bounds",
            input,
            ToolState::Running,
        );
        self.set_part(sid, mid, PART_READ, part);
        if !self.sleep_or_abort(sid, READ_MS).await {
            return false;
        }
        let file: String = (1..=READ_LINES)
            .map(|i| {
                let line = if i == BUGGY_LINE {
                    "    for i in 0..=items.len() {"
                } else {
                    "    // ..."
                };
                format!("{i:>4}│ {line}\n")
            })
            .collect();
        self.finish_tool(
            sid,
            mid,
            PART_READ,
            ToolState::Done,
            &format!("[{PARSER_PATH}#{PARSER_TAG}]\n{file}"),
            Some(json!({"lines": READ_LINES})),
        );
        true
    }

    /// Tool: grep (error).
    async fn grep_step(&self, sid: &str, mid: &str) -> bool {
        let part = tool_part(
            "call_grep",
            "grep",
            "Search for other inclusive ranges",
            json!({"pattern": "0..=", "path": "src/lib"}),
            ToolState::Running,
        );
        self.set_part(sid, mid, PART_GREP, part);
        if !self.sleep_or_abort(sid, GREP_MS).await {
            return false;
        }
        self.finish_tool(
            sid,
            mid,
            PART_GREP,
            ToolState::Error,
            "Path not found: src/lib\nDid you mean src/?",
            None,
        );
        true
    }

    /// Tool: edit (streaming input, then permission, then done with a diff).
    async fn edit_step(&self, sid: &str, mid: &str) -> bool {
        let mut part = tool_part("call_edit", "edit", "", Value::Null, ToolState::Streaming);
        if let Part::Tool { raw_input, .. } = &mut part {
            raw_input.clear();
        }
        self.set_part(sid, mid, PART_EDIT, part);
        if !self.stream_edit_input(sid, mid).await {
            return false;
        }
        let input = json!({"path": PARSER_PATH, "tag": PARSER_TAG, "edits": [{"op": "replace", "start": BUGGY_LINE, "end": BUGGY_LINE, "lines": ["    for i in 0..items.len() {"]}]});
        self.part_mut(sid, mid, PART_EDIT, |p| {
            if let Part::Tool {
                input: i, purpose, ..
            } = p
            {
                *i = input.clone();
                *purpose = EDIT_PURPOSE.into();
            }
        });
        let allowed = self
            .ask_permission(sid, mid, PART_EDIT, "edit", EDIT_PURPOSE, input)
            .await;
        match allowed {
            None => false,
            Some(false) => {
                let state = ToolState::Denied;
                self.finish_tool(sid, mid, PART_EDIT, state, DENIED_OUTPUT, None);
                true
            }
            Some(true) => self.apply_edit(sid, mid).await,
        }
    }

    /// Streams the raw JSON input of the edit call in small chunks.
    async fn stream_edit_input(&self, sid: &str, mid: &str) -> bool {
        let raw = format!(
            r#"{{"purpose": "{EDIT_PURPOSE}", "path": "{PARSER_PATH}", "tag": "{PARSER_TAG}", "edits": [{{"op":"replace","start":{BUGGY_LINE},"end":{BUGGY_LINE},"lines":["    for i in 0..items.len() {{"]}}]}}"#
        );
        for c in raw.as_bytes().chunks(TOOL_INPUT_CHUNK_BYTES) {
            if self.aborted(sid) {
                return false;
            }
            let delta = Delta::ToolInput {
                text: String::from_utf8_lossy(c).into(),
            };
            self.emit(
                sid,
                SessionEvent::Delta {
                    message_id: mid.into(),
                    index: PART_EDIT,
                    delta,
                },
            );
            tokio::time::sleep(TOOL_INPUT_DELAY).await;
        }
        true
    }

    /// Runs the allowed edit and records its diff.
    async fn apply_edit(&self, sid: &str, mid: &str) -> bool {
        self.set_tool_state(sid, mid, PART_EDIT, ToolState::Running);
        if !self.sleep_or_abort(sid, EDIT_RUN_MS).await {
            return false;
        }
        let diff = format!(
            "--- a/{PARSER_PATH}\n+++ b/{PARSER_PATH}\n@@ -40,5 +40,5 @@\n fn parse_items(items: &[Item]) -> Result<Vec<Out>> {{\n     let mut out = Vec::new();\n-    for i in 0..=items.len() {{\n+    for i in 0..items.len() {{\n         let item = &items[i];"
        );
        let output = format!(
            "Edited {PARSER_PATH} [{PARSER_PATH}#b7c2]\n  {BUGGY_LINE} |     for i in 0..items.len() {{"
        );
        let meta = json!({"path": PARSER_PATH, "diff": diff, "added": 1, "removed": 1});
        self.finish_tool(sid, mid, PART_EDIT, ToolState::Done, &output, Some(meta));
        true
    }

    /// Tool: bash with a start delay, then live output.
    async fn bash_step(&self, sid: &str, mid: &str) -> bool {
        let input =
            json!({"command": "cargo test -p parser -- --nocapture\necho done", "timeout": 120});
        let mut part = tool_part(
            "call_bash",
            "bash",
            "Run the unit tests",
            input,
            ToolState::Delayed,
        );
        if let Part::Tool { run_at, .. } = &mut part {
            *run_at = Some(now_ms() + BASH_START_MS as i64);
        }
        self.set_part(sid, mid, PART_BASH, part);
        if !self.sleep_or_abort(sid, BASH_START_MS).await {
            return false;
        }
        self.set_tool_state(sid, mid, PART_BASH, ToolState::Running);
        let mut out = String::new();
        for i in 1..=BASH_TEST_COUNT {
            out.push_str(&format!(
                "test parser::tests::case_{i:02} ... \x1b[32mok\x1b[0m\n"
            ));
            let live = out.clone();
            self.part_mut(sid, mid, PART_BASH, |p| {
                if let Part::Tool { meta, .. } = p {
                    *meta = Some(json!({"live_output": live}));
                }
            });
            if !self.sleep_or_abort(sid, BASH_LINE_MS).await {
                return false;
            }
        }
        out.push_str("\ntest result: ok. 42 passed; 0 failed; 0 ignored\ndone\n");
        let meta = json!({"exit_code": 0, "duration_ms": 3120, "truncated": false});
        self.finish_tool(sid, mid, PART_BASH, ToolState::Done, &out, Some(meta));
        true
    }

    async fn answer_step(&self, sid: &str, mid: &str) -> bool {
        self.set_status(sid, SessionStatus::Busy);
        self.start_text_part(sid, mid, PART_ANSWER);
        self.stream(sid, mid, PART_ANSWER, ANSWER, StreamSpec::text(4, 18))
            .await
    }

    /// Background worker: loops forever doing small tool calls.
    async fn worker_loop(&self, sid: String, label: &'static str) {
        let mut round = 0u32;
        loop {
            if !self.world().sessions.contains_key(&sid) {
                return;
            }
            round += 1;
            if let Some(s) = self.world().sessions.get(&sid) {
                s.abort.store(false, Ordering::Relaxed);
            }
            let ok = self.worker_round(&sid, label, round).await;
            // After an abort the worker idles longer, waiting for a new prompt.
            let pause = if ok {
                WORKER_PAUSE
            } else {
                WORKER_ABORTED_PAUSE
            };
            tokio::time::sleep(pause).await;
        }
    }

    async fn worker_round(&self, sid: &str, label: &str, round: u32) -> bool {
        self.set_status(sid, SessionStatus::Busy);
        let mid = self.new_assistant(sid);
        let ok = self.worker_steps(sid, &mid, label, round).await;
        self.add_context(sid, WORKER_ROUND_CONTEXT_TOKENS);
        self.finish(
            sid,
            &mid,
            WORKER_USAGE,
            (!ok).then(|| ABORTED_ERROR.to_string()),
        );
        self.set_status(sid, SessionStatus::Idle);
        ok
    }

    /// Reasoning, one bash call, then a short text. Returns false when aborted.
    async fn worker_steps(&self, sid: &str, mid: &str, label: &str, round: u32) -> bool {
        self.start_reasoning_part(sid, mid, PART_REASONING);
        let thought = format!(
            "Round {round} for {label}: checking the next module, scanning for unused imports and dead code, then running the linter on the changed files to verify."
        );
        let spec = StreamSpec::reasoning(2, 40);
        if !self.stream(sid, mid, PART_REASONING, &thought, spec).await {
            return false;
        }
        self.end_reasoning_part(sid, mid, PART_REASONING);
        self.set_status(sid, SessionStatus::Running);
        let tool = tool_part(
            &id("call"),
            "bash",
            &format!("Lint module #{round}"),
            json!({"command": format!("cargo clippy -p mod{round}")}),
            ToolState::Running,
        );
        self.set_part(sid, mid, WORKER_TOOL_PART, tool);
        if !self.sleep_or_abort(sid, WORKER_TOOL_MS).await {
            return false;
        }
        let meta = json!({"exit_code": 0});
        self.finish_tool(
            sid,
            mid,
            WORKER_TOOL_PART,
            ToolState::Done,
            "Finished: 0 warnings",
            Some(meta),
        );
        self.start_text_part(sid, mid, WORKER_TEXT_PART);
        let text = format!("Module #{round} is **clean**. Moving on to the next one.");
        self.stream(sid, mid, WORKER_TEXT_PART, &text, StreamSpec::text(3, 40))
            .await
    }

    async fn reply_turn(&self, sid: &str, text: &str) {
        self.set_status(sid, SessionStatus::Busy);
        let mid = self.new_assistant(sid);
        self.start_text_part(sid, &mid, 0);
        let ans = format!(
            "Got it: \"{}\". I'll take that into account.",
            crate::ui::text::truncate(&crate::ui::text::one_line(text), 60)
        );
        let ok = self
            .stream(sid, &mid, 0, &ans, StreamSpec::text(2, 30))
            .await;
        self.finish(
            sid,
            &mid,
            REPLY_USAGE,
            (!ok).then(|| ABORTED_ERROR.to_string()),
        );
        self.set_status(sid, SessionStatus::Idle);
    }
}

impl Mock {
    fn ok(&self, id: Option<u64>, result: Value) {
        if let Some(id) = id {
            self.send(ServerFrame::Response {
                id,
                ok: true,
                result,
                error: None,
            });
        }
    }

    fn err(&self, id: Option<u64>, e: &str) {
        if let Some(id) = id {
            self.send(ServerFrame::Response {
                id,
                ok: false,
                result: Value::Null,
                error: Some(e.into()),
            });
        }
    }

    fn with_session<R>(&self, sid: &str, f: impl FnOnce(&mut MockSession) -> R) -> Option<R> {
        self.world().sessions.get_mut(sid).map(f)
    }

    /// Edits a session's info and returns the updated copy (None for an unknown session).
    fn update_info(&self, sid: &str, f: impl FnOnce(&mut SessionInfo)) -> Option<SessionInfo> {
        self.with_session(sid, |s| {
            f(&mut s.info);
            s.info.clone()
        })
    }

    fn handle(&self, f: ClientFrame) {
        let request_id = f.id;
        match f.req {
            Request::Hello { .. }
            | Request::Ping
            | Request::SetNotify { .. }
            | Request::SetFocus { .. }
            | Request::Shutdown => self.ok(request_id, json!({})),
            Request::ListDirs { .. } => self.err(request_id, "not supported by the mock backend"),
            Request::GetConfig => self.ok(request_id, json!({ "config": config() })),
            Request::ListSessions => self.on_list_sessions(request_id),
            Request::ListHistory { .. } => self.on_list_history(request_id),
            Request::CreateSession {
                cwd,
                agent,
                name,
                model,
            } => self.on_create_session(request_id, &cwd, agent, name, model),
            Request::Attach {
                session_id,
                foreground,
                ..
            } => self.on_attach(request_id, &session_id, foreground),
            Request::Detach { session_id } => self.on_detach(request_id, &session_id),
            Request::WatchAll { enabled } => {
                self.world().watch_all = enabled;
                self.ok(request_id, json!({}));
            }
            Request::Prompt {
                session_id,
                text,
                images,
            } => self.on_prompt(request_id, session_id, text, images),
            Request::RemoveQueued {
                session_id,
                message_id,
            } => self.on_remove_queued(request_id, &session_id, &message_id),
            Request::Shell {
                session_id,
                command,
            } => {
                self.ok(request_id, json!({}));
                let out = format!(
                    "I ran this command:\n```\n$ {command}\n(mock) command output line 1\n(mock) line 2\n```"
                );
                self.user_message(&session_id, &out, MessageSource::Shell, vec![]);
            }
            Request::Abort { session_id } => {
                self.with_session(&session_id, |s| s.abort.store(true, Ordering::Relaxed));
                self.ok(request_id, json!({}));
            }
            Request::PermissionReply { call_id, allow, .. } => {
                if let Some(tx) = self.world().perm_waiters.remove(&call_id) {
                    let _ = tx.send(allow);
                }
                self.ok(request_id, json!({}));
            }
            // The in-process mock asks no questions.
            Request::QuestionReply { .. } => self.ok(request_id, json!({})),
            Request::SetModel { session_id, model } => {
                if let Some(info) = self.update_info(&session_id, |i| i.model = model) {
                    self.reply_with_session(request_id, &session_id, info);
                }
            }
            Request::SetAgent { session_id, agent } => {
                let agent_model = config()
                    .agents
                    .iter()
                    .find(|a| a.name == agent)
                    .and_then(|a| a.model.clone());
                let info = self.update_info(&session_id, |i| {
                    i.agent = agent;
                    i.model = agent_model.unwrap_or_else(model);
                });
                if let Some(info) = info {
                    self.reply_with_session(request_id, &session_id, info);
                }
            }
            Request::SetVariant {
                session_id,
                variant,
            } => self.on_set_variant(request_id, &session_id, variant),
            Request::PeerCommand { command, .. } => {
                let text = match command.as_str() {
                    cmd::PEERS => PEERS_TEXT,
                    cmd::PEERS_INBOX => PEERS_INBOX_TEXT,
                    cmd::PEERS_OUTBOX => PEERS_OUTBOX_TEXT,
                    _ => PEER_NAME_TEXT,
                };
                self.ok(request_id, json!({ "text": text }));
            }
            Request::Rename { session_id, name } => {
                if let Some(info) = self.update_info(&session_id, |i| i.name = name) {
                    self.emit(&session_id, SessionEvent::Info { info });
                }
                self.ok(request_id, json!({}));
                self.broadcast_sessions();
            }
            Request::SetCwd { session_id, path } => {
                let info = self.update_info(&session_id, |i| {
                    let joined = std::path::Path::new(&i.cwd).join(&path);
                    i.cwd = joined.to_string_lossy().into_owned();
                });
                match info {
                    Some(info) => self.reply_with_session(request_id, &session_id, info),
                    None => self.err(request_id, "no such session"),
                }
            }
            Request::Compact { session_id } => {
                self.ok(request_id, json!({}));
                let m = self.clone();
                tokio::spawn(async move { m.compact(&session_id).await });
            }
            Request::Undo {
                session_id,
                message_id,
            } => self.on_undo(request_id, &session_id, message_id.as_deref()),
            Request::Close { session_id } => self.on_close(request_id, &session_id),
            Request::Delete { session_id } => {
                self.world().history.retain(|h| h.id != session_id);
                self.ok(request_id, json!({}));
            }
            Request::ExpandCommand { name, args, .. } => {
                self.ok(
                    request_id,
                    json!({ "text": format!("(expanded /{name}) Please do the {name} task. {args}") }),
                );
            }
            Request::ListFiles { query, .. } => self.on_list_files(request_id, &query),
            Request::Export { path, .. } => self.ok(request_id, json!({ "path": path })),
            // The demo sends whole histories with image bytes.
            Request::Messages { .. } => {
                self.ok(request_id, json!({ "messages": [], "more_before": false }))
            }
            Request::GetImage { .. } => self.err(request_id, "Image not found"),
            Request::McpStatus { .. } => self.on_mcp_status(request_id),
        }
    }

    /// Emits the updated info, answers with the session and broadcasts the session list.
    fn reply_with_session(&self, request_id: Option<u64>, sid: &str, info: SessionInfo) {
        self.emit(sid, SessionEvent::Info { info: info.clone() });
        self.ok(request_id, json!({ "session": info }));
        self.broadcast_sessions();
    }

    fn on_list_sessions(&self, request_id: Option<u64>) {
        let w = self.world();
        let list: Vec<&SessionInfo> = w
            .order
            .iter()
            .filter_map(|i| w.sessions.get(i))
            .map(|s| &s.info)
            .collect();
        self.ok(request_id, json!({ "sessions": list }));
    }

    fn on_list_history(&self, request_id: Option<u64>) {
        let w = self.world();
        let mut list: Vec<SessionInfo> = w.history.clone();
        list.extend(
            w.order
                .iter()
                .filter_map(|i| w.sessions.get(i))
                .map(|s| s.info.clone()),
        );
        list.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
        self.ok(request_id, json!({ "sessions": list }));
    }

    fn on_create_session(
        &self,
        request_id: Option<u64>,
        cwd: &str,
        agent: Option<String>,
        name: Option<String>,
        model: Option<ModelRef>,
    ) {
        let base = std::path::Path::new(cwd)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "ref".into());
        let name = name.unwrap_or_else(|| {
            let hash = SEQ.load(Ordering::Relaxed) * NAME_HASH_MULTIPLIER % NAME_HASH_MODULUS;
            format!("{base}-{hash:04x}")
        });
        let mut created = self.create(&name, agent.as_deref().unwrap_or("build"), cwd);
        if let Some(m) = model {
            created = self
                .update_info(&created.id, |i| i.model = m)
                .expect("session was just created");
        }
        self.ok(request_id, json!({ "session": created }));
        self.broadcast_sessions();
    }

    fn on_detach(&self, request_id: Option<u64>, sid: &str) {
        {
            let mut w = self.world();
            w.subs.remove(sid);
            if let Some(s) = w.sessions.get_mut(sid) {
                s.info.attached = s.info.attached.saturating_sub(1);
            }
        }
        self.ok(request_id, json!({}));
        self.broadcast_sessions();
    }

    /// Moves a history-only session back into the live set, with a short canned transcript.
    fn revive_from_history(&self, sid: &str) {
        let hist = {
            let mut w = self.world();
            let pos = w.history.iter().position(|h| h.id == sid);
            pos.map(|p| w.history.remove(p))
        };
        let Some(mut h) = hist else {
            return;
        };
        h.live = true;
        let user = new_message(
            id("msg"),
            Role::User,
            h.created_at,
            MessageSource::Human,
            vec![Part::Text {
                text: "Summarise the README".into(),
                time: h.created_at,
            }],
        );
        let assistant = Message {
            time_end: Some(h.created_at + 5000),
            model: Some(model()),
            agent: Some("build".into()),
            usage: Some(Usage {
                input: 300,
                output: 40,
                ..Default::default()
            }),
            ..new_message(
                id("msg"),
                Role::Assistant,
                h.created_at + 1000,
                MessageSource::Human,
                vec![Part::Text {
                    text: "The README describes **ref**, a coding agent harness with a TUI frontend and a backend server.".into(),
                    time: h.created_at + 1000,
                }],
            )
        };
        let mut w = self.world();
        w.order.push(h.id.clone());
        w.sessions.insert(
            h.id.clone(),
            MockSession {
                info: h,
                messages: vec![user, assistant],
                perms: vec![],
                abort: Arc::new(AtomicBool::new(false)),
                queue: Vec::new(),
            },
        );
    }

    fn on_attach(&self, request_id: Option<u64>, sid: &str, foreground: bool) {
        // History-only session → load it live.
        self.revive_from_history(sid);
        let snap = {
            let mut w = self.world();
            w.subs.insert(sid.to_string());
            w.sessions.get_mut(sid).map(|s| {
                if foreground {
                    s.info.attached += 1;
                }
                SessionSnapshot {
                    info: s.info.clone(),
                    messages: s.messages.clone(),
                    more_before: false,
                    pending_permissions: s.perms.clone(),
                    pending_questions: Vec::new(),
                    queued: s.queue.clone(),
                }
            })
        };
        let Some(snap) = snap else {
            self.err(request_id, "no such session");
            return;
        };
        self.ok(request_id, json!({ "snapshot": snap }));
        self.broadcast_sessions();
    }

    fn on_prompt(
        &self,
        request_id: Option<u64>,
        sid: String,
        text: String,
        images: Vec<ImageData>,
    ) {
        let mid = id("msg");
        let queued = self.with_session(&sid, |s| {
            s.abort.store(false, Ordering::Relaxed);
            let busy = !s.info.status.is_idle();
            if busy {
                s.queue.push(user_msg(
                    mid.clone(),
                    &text,
                    MessageSource::Human,
                    images.clone(),
                ));
            }
            busy
        }) == Some(true);
        self.ok(request_id, json!({ "queued": queued, "message_id": mid }));
        let m = self.clone();
        tokio::spawn(async move { m.deliver_prompt(&sid, mid, &text, images, queued).await });
    }

    /// Adds the prompt to the transcript (after the queue wait, if it was queued) and runs the turn.
    async fn deliver_prompt(
        &self,
        sid: &str,
        mid: String,
        text: &str,
        images: Vec<ImageData>,
        was_queued: bool,
    ) {
        if was_queued {
            // The client shows the prompt as queued meanwhile.
            self.wait_until_idle(sid).await;
            // Withdrawn with `RemoveQueued` meanwhile: never delivered.
            if self.take_queued(sid, &mid) != Some(true) {
                return;
            }
        }
        self.user_message_id(sid, mid, text, MessageSource::Human, images);
        let first = self
            .with_session(sid, |s| {
                s.messages.iter().filter(|x| x.role == Role::User).count() <= 1
            })
            .unwrap_or(false);
        if first || text.contains("demo") {
            self.demo_turn(sid, text).await;
        } else {
            self.reply_turn(sid, text).await;
        }
    }

    async fn wait_until_idle(&self, sid: &str) {
        for _ in 0..QUEUE_WAIT_POLLS {
            tokio::time::sleep(QUEUE_POLL).await;
            if self
                .with_session(sid, |s| s.info.status.is_idle())
                .unwrap_or(true)
            {
                return;
            }
        }
    }

    /// Removes a queued prompt; Some(true) if it was still queued.
    fn take_queued(&self, sid: &str, mid: &str) -> Option<bool> {
        self.with_session(sid, |s| {
            let i = s.queue.iter().position(|q| q.id == mid);
            i.map(|i| s.queue.remove(i)).is_some()
        })
    }

    fn on_remove_queued(&self, request_id: Option<u64>, sid: &str, mid: &str) {
        match self.take_queued(sid, mid) {
            Some(removed) => self.ok(request_id, json!({ "removed": removed })),
            None => self.err(request_id, "no such session"),
        }
    }

    fn on_set_variant(&self, request_id: Option<u64>, sid: &str, variant: Option<String>) {
        let cfg = config();
        let info = self.update_info(sid, |i| {
            let variants = cfg
                .models
                .iter()
                .find(|m| m.provider == i.model.provider && m.id == i.model.model)
                .map(|m| m.variants.clone())
                .unwrap_or_default();
            i.model.variant =
                variant.or_else(|| next_variant(i.model.variant.as_deref(), &variants));
        });
        // Unlike the other session edits, the session list is not re-broadcast.
        if let Some(info) = info {
            self.emit(sid, SessionEvent::Info { info: info.clone() });
            self.ok(request_id, json!({ "session": info }));
        }
    }

    /// Compacts the context: a summary message is appended and the context counter drops.
    async fn compact(&self, sid: &str) {
        self.set_status(sid, SessionStatus::Compacting);
        tokio::time::sleep(COMPACT_DELAY).await;
        let summary = new_message(
            id("msg"),
            Role::User,
            now_ms(),
            MessageSource::Compaction,
            vec![Part::Text {
                text: COMPACTION_SUMMARY.into(),
                time: now_ms(),
            }],
        );
        // Append-only: older messages stay visible; the model context starts here.
        self.emit(sid, SessionEvent::Message { message: summary });
        self.with_session(sid, |s| s.info.context.used = COMPACTED_CONTEXT_TOKENS);
        self.set_status(sid, SessionStatus::Idle);
    }

    fn on_undo(&self, request_id: Option<u64>, sid: &str, message_id: Option<&str>) {
        let undone = self.with_session(sid, |s| {
            let pos = match message_id {
                Some(mid) => s.messages.iter().position(|m| m.id == mid)?,
                None => s
                    .messages
                    .iter()
                    .rposition(|m| m.role == Role::User && m.source == MessageSource::Human)?,
            };
            let text = if s.messages[pos].source == MessageSource::Human {
                s.messages[pos].raw_text()
            } else {
                String::new()
            };
            s.messages.truncate(pos);
            Some((text, s.messages.clone()))
        });
        let Some((text, messages)) = undone.flatten() else {
            self.err(request_id, "nothing to undo");
            return;
        };
        self.emit(
            sid,
            SessionEvent::Reset {
                messages,
                more_before: false,
            },
        );
        self.ok(request_id, json!({ "text": text }));
    }

    fn on_close(&self, request_id: Option<u64>, sid: &str) {
        let info = {
            let mut w = self.world();
            w.order.retain(|x| x != sid);
            w.sessions.remove(sid).map(|s| s.info)
        };
        if let Some(mut i) = info {
            i.live = false;
            i.attached = 0;
            self.send(ServerFrame::Event {
                session_id: sid.to_string(),
                event: SessionEvent::Closed,
            });
            self.world().history.push(i);
        }
        self.ok(request_id, json!({}));
        self.broadcast_sessions();
    }

    fn on_list_files(&self, request_id: Option<u64>, query: &str) {
        let mut scored: Vec<(i64, &str)> = MOCK_FILES
            .iter()
            .filter_map(|f| crate::ui::text::fuzzy_score(f, query).map(|s| (s, *f)))
            .collect();
        scored.sort_by_key(|x| std::cmp::Reverse(x.0));
        let files: Vec<&str> = scored.into_iter().map(|x| x.1).collect();
        self.ok(request_id, json!({ "files": files }));
    }

    fn on_mcp_status(&self, request_id: Option<u64>) {
        let started = {
            let mut w = self.world();
            if w.mcp_started == 0 {
                w.mcp_started = now_ms();
            }
            w.mcp_started
        };
        let docs_ready = now_ms() - started > MCP_CONNECT_DELAY_MS;
        let server =
            |name: &str, status: &str, error: Option<&str>, tools: &[&str]| McpServerStatus {
                name: name.into(),
                status: status.into(),
                error: error.map(Into::into),
                tools: tools.iter().map(|t| t.to_string()).collect(),
            };
        let servers = vec![
            server(
                "context7",
                mcp_status::CONNECTED,
                None,
                &["context7_resolve-library-id", "context7_get-library-docs"],
            ),
            if docs_ready {
                server("docs", mcp_status::CONNECTED, None, &["docs_search"])
            } else {
                server("docs", mcp_status::CONNECTING, None, &[])
            },
            server(
                "github",
                mcp_status::FAILED,
                Some("HTTP 401 Unauthorized: missing or invalid bearer token (set GITHUB_TOKEN)"),
                &[],
            ),
            server("sqlite", mcp_status::DISABLED, None, &[]),
        ];
        self.ok(request_id, json!({ "servers": servers }));
    }

    /// Seeds background sessions and history.
    fn seed(&self, cwd: &str) {
        let a = self.create("worker-1", "build", cwd);
        let b = self.create("reviewer", "review", cwd);
        let c = self.create("docs-写手", "plan", cwd);
        for (s, t) in [
            (&a, "Clean up lints across modules"),
            (&b, "Review the parser change"),
            (&c, "Draft the design doc"),
        ] {
            self.with_session(&s.id, |x| {
                x.info.title = t.into();
                x.info.parent = Some("ses_parent".into());
                x.info.context.used = SEEDED_WORKER_CONTEXT_TOKENS;
            });
        }
        let now = now_ms();
        for (i, (t, n)) in [
            ("Refactor the config loader", "ref-1a2b"),
            ("Investigate flaky websocket test", "ref-9f00"),
            ("实现会话列表", "ref-77cc"),
        ]
        .iter()
        .enumerate()
        {
            let mut h = info(n, "build", cwd);
            h.title = t.to_string();
            h.live = false;
            h.created_at = now - (i as i64 + 1) * HISTORY_SPACING_MS;
            h.updated_at = h.created_at + HISTORY_SESSION_SPAN_MS;
            self.world().history.push(h);
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
            tokio::time::sleep(DOCS_WRITER_DELAY).await;
            m.user_message(
                &cid,
                "Write docs/design.md describing the architecture",
                MessageSource::Spawn {
                    from_id: "ses_parent".into(),
                    from_name: "main".into(),
                },
                vec![],
            );
            m.set_status(&cid, SessionStatus::Busy);
            let mid = m.new_assistant(&cid);
            m.start_text_part(&cid, &mid, 0);
            m.stream(
                &cid,
                &mid,
                0,
                "I'll write the design document now.",
                StreamSpec::text(3, 40),
            )
            .await;
            let input = json!({"path": "docs/design.md", "content": "# Design\n\nref consists of a backend server and TUI frontends.\n"});
            m.set_part(
                &cid,
                &mid,
                1,
                tool_part(
                    "call_write_doc",
                    "write",
                    "Create the design document",
                    input.clone(),
                    ToolState::Streaming,
                ),
            );
            let r = m
                .ask_permission(&cid, &mid, 1, "write", "Create the design document", input)
                .await;
            match r {
                Some(true) => m.finish_tool(&cid, &mid, 1, ToolState::Done, "Wrote docs/design.md (3 lines)", Some(json!({"diff": "+++ b/docs/design.md\n+# Design\n+\n+ref consists of a backend server and TUI frontends.", "created": true}))),
                Some(false) => m.finish_tool(&cid, &mid, 1, ToolState::Denied, "The user denied this tool call.", None),
                None => {}
            }
            m.finish(
                &cid,
                &mid,
                Usage {
                    input: 800,
                    output: 90,
                    ..Default::default()
                },
                r.is_none().then(|| ABORTED_ERROR.to_string()),
            );
            m.set_status(&cid, SessionStatus::Idle);
        });
    }
}

/// The variant after `current` in `variants`; None after the last one (back to "default").
fn next_variant(current: Option<&str>, variants: &[String]) -> Option<String> {
    let pos = current.and_then(|v| variants.iter().position(|x| x == v));
    match pos {
        None => variants.first().cloned(),
        Some(i) if i + 1 < variants.len() => Some(variants[i + 1].clone()),
        Some(_) => None,
    }
}

/// A message with no end time, model, agent, usage or error; callers set the differing fields.
fn new_message(
    id: String,
    role: Role,
    time: i64,
    source: MessageSource,
    parts: Vec<Part>,
) -> Message {
    Message {
        id,
        role,
        time,
        time_end: None,
        source,
        parts,
        model: None,
        agent: None,
        usage: None,
        error: None,
    }
}

fn user_msg(mid: String, text: &str, source: MessageSource, images: Vec<ImageData>) -> Message {
    let mut parts = vec![Part::Text {
        text: text.to_string(),
        time: now_ms(),
    }];
    for (i, img) in images.into_iter().enumerate() {
        parts.push(Part::Image {
            image: img,
            name: Some(format!("image-{}", i + 1)),
        });
    }
    new_message(mid, Role::User, now_ms(), source, parts)
}

/// Runs the mock backend.
pub async fn run(
    cwd: String,
    mut rx: mpsc::UnboundedReceiver<ClientFrame>,
    tx: mpsc::UnboundedSender<NetEvent>,
) {
    let m = Mock {
        world: Arc::new(Mutex::new(World::default())),
        tx: tx.clone(),
    };
    m.seed(&cwd);
    tokio::time::sleep(STARTUP_DELAY).await;
    let _ = tx.send(NetEvent::Connected);
    while let Some(f) = rx.recv().await {
        m.handle(f);
    }
}
