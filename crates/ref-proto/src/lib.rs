//! Shared data model and WebSocket protocol between `refd` (backend) and `ref` (TUI frontend).
//!
//! Everything that crosses the wire is JSON, tagged with `"type"`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod spec;

pub type SessionId = String;
pub type MessageId = String;
pub type PartId = String;

pub const PROTOCOL_VERSION: u32 = 1;

/// Milliseconds since unix epoch.
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Time-ordered unique id with a short prefix, e.g. `msg_0192…`.
pub fn new_id(prefix: &str) -> String {
    format!("{}_{}", prefix, uuid::Uuid::now_v7().simple())
}

// ───────────────────────────── messages ─────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
}

/// Where a user-role message came from. Only `User` messages are typed by a human.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MessageOrigin {
    #[default]
    User,
    /// Message delivered from another session.
    Peer { from_session: SessionId, from_name: String },
    /// Automatic reminder injected by the backend (e.g. must-report reminder).
    Reminder,
    /// Summary that starts a new context window after compaction.
    Compaction,
    /// Initial prompt passed by the session that spawned this one.
    Spawn { from_session: SessionId, from_name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub role: Role,
    /// Creation time (ms).
    pub time: i64,
    #[serde(default)]
    pub origin: MessageOrigin,
    #[serde(default)]
    pub parts: Vec<Part>,
    /// Assistant only: `provider/model` that produced it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// Assistant only: completion time (ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_end: Option<i64>,
    /// Assistant only: stop reason (`stop`, `tool_use`, `length`, `aborted`, `error`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Message {
    pub fn new(role: Role, origin: MessageOrigin) -> Self {
        Message {
            id: new_id("msg"),
            role,
            time: now_ms(),
            origin,
            parts: vec![],
            model: None,
            variant: None,
            agent: None,
            usage: None,
            time_end: None,
            finish: None,
            error: None,
        }
    }

    /// Concatenated raw text of all text parts (what "copy raw" copies).
    pub fn raw_text(&self) -> String {
        let mut out = String::new();
        for p in &self.parts {
            if let Part::Text(t) = p {
                if !out.is_empty() {
                    out.push_str("\n\n");
                }
                out.push_str(&t.text);
            }
        }
        out
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Part {
    Text(TextPart),
    Reasoning(ReasoningPart),
    Tool(ToolPart),
    Image(ImagePart),
}

impl Part {
    pub fn id(&self) -> &str {
        match self {
            Part::Text(p) => &p.id,
            Part::Reasoning(p) => &p.id,
            Part::Tool(p) => &p.id,
            Part::Image(p) => &p.id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextPart {
    pub id: PartId,
    pub text: String,
    pub time: i64,
    /// Opaque provider data needed for faithful replay (e.g. Responses message item id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningPart {
    pub id: PartId,
    pub text: String,
    pub time: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_end: Option<i64>,
    /// Opaque provider data for replay: anthropic signature / redacted data,
    /// responses encrypted reasoning item, chat reasoning_details, ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImagePart {
    pub id: PartId,
    /// e.g. `image/png`
    pub mime: String,
    /// base64 (no data: prefix)
    pub data: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolState {
    /// Arguments still streaming from the model.
    Streaming,
    /// Waiting for user's permission decision.
    AwaitingPermission,
    /// Waiting the configured pre-execution delay (see `ToolPart::run_at`).
    Delayed,
    Running,
    Done,
    Error,
    Denied,
    Cancelled,
}

impl ToolState {
    pub fn is_final(self) -> bool {
        matches!(self, ToolState::Done | ToolState::Error | ToolState::Denied | ToolState::Cancelled)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPart {
    pub id: PartId,
    /// Provider tool-call id.
    pub call_id: String,
    pub name: String,
    /// The `purpose` argument the model gave (also present inside `input`).
    #[serde(default)]
    pub purpose: String,
    /// Parsed arguments (raw, as sent by the model, including `purpose`).
    #[serde(default)]
    pub input: Value,
    /// Raw argument string as streamed (kept while streaming / if JSON was invalid).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub input_raw: String,
    pub state: ToolState,
    /// Raw tool output text returned to the model.
    #[serde(default)]
    pub output: String,
    /// Images returned by the tool (sent to the model as well).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ImagePart>,
    pub time: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_start: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_end: Option<i64>,
    /// When state == Delayed: time (ms) at which execution starts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_at: Option<i64>,
    /// Opaque provider data for replay (e.g. Responses `fc_` item id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    /// Uncached input tokens.
    #[serde(default)]
    pub input: u64,
    #[serde(default)]
    pub output: u64,
    #[serde(default)]
    pub cache_read: u64,
    #[serde(default)]
    pub cache_write: u64,
    #[serde(default)]
    pub reasoning: u64,
    /// USD cost of this request if known.
    #[serde(default)]
    pub cost: f64,
}

impl Usage {
    /// Tokens occupying the context window after this response.
    pub fn context_tokens(&self) -> u64 {
        self.input + self.cache_read + self.cache_write + self.output
    }
}

// ───────────────────────────── sessions ─────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SessionStatus {
    Idle,
    Busy,
    /// LLM request failed; retrying at `at` (ms).
    Retrying { attempt: u32, at: i64, error: String },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ContextInfo {
    /// Tokens currently in the context window (from last response usage, or estimate).
    pub used: u64,
    /// Context window size of the model.
    pub max: u64,
    /// Recommended token count at which to compact.
    pub compact_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: SessionId,
    /// Display / peer name. Any characters allowed; unique among open sessions.
    pub name: String,
    pub cwd: String,
    pub agent: String,
    /// `provider/model`
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    pub created: i64,
    pub updated: i64,
    /// Loaded in backend memory (open). Non-open sessions are history.
    pub open: bool,
    /// Number of frontends directly attached. >0 means foreground session.
    pub attached: u32,
    pub status: SessionStatus,
    #[serde(default)]
    pub context: ContextInfo,
    /// Total cost so far (USD).
    #[serde(default)]
    pub cost: f64,
    /// Session that created this one (via the create_session tool).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<SessionId>,
    #[serde(default)]
    pub must_report: bool,
    #[serde(default)]
    pub message_count: u32,
    /// Tool calls waiting for a permission decision.
    #[serde(default)]
    pub pending_permissions: u32,
}

impl SessionInfo {
    pub fn is_foreground(&self) -> bool {
        self.attached > 0
    }
}

// ───────────────────────────── catalog ─────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntry {
    /// `provider/model`
    pub id: String,
    pub provider_name: String,
    pub name: String,
    #[serde(default)]
    pub variants: Vec<String>,
    #[serde(default)]
    pub context: u64,
    #[serde(default)]
    pub image_input: bool,
    #[serde(default)]
    pub reasoning: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEntry {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Default model `provider/model` (resolved).
    #[serde(default)]
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default)]
    pub spawnable: bool,
    #[serde(default)]
    pub can_spawn: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpStatus {
    pub name: String,
    /// `connected`, `failed`, `disabled`, `connecting`
    pub status: String,
    #[serde(default)]
    pub tools: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    pub models: Vec<ModelEntry>,
    pub agents: Vec<AgentEntry>,
    pub default_agent: String,
    #[serde(default)]
    pub mcp: Vec<McpStatus>,
    /// Custom slash commands (from opencode `command` config / command dirs).
    #[serde(default)]
    pub commands: Vec<CommandEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandEntry {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// Information describing a running backend instance (also the registry file contents).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceInfo {
    pub pid: u32,
    pub version: String,
    /// Address the server binds to (e.g. `127.0.0.1`).
    pub bind: String,
    pub port: u16,
    /// Hosts allowed to connect (empty = only loopback).
    pub allow_hosts: Vec<String>,
    /// Idle timeout seconds: 0 = exit as soon as no frontend is connected, -1 = never.
    pub idle_timeout: i64,
    pub started: i64,
    #[serde(default)]
    pub data_dir: String,
}

// ───────────────────────────── protocol ─────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    /// Allow this call only.
    Once,
    /// Allow this tool for the rest of the session.
    Always,
    Deny,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub session_id: SessionId,
    pub message_id: MessageId,
    pub part_id: PartId,
    pub call_id: String,
    pub tool: String,
    pub purpose: String,
    /// Full raw input arguments.
    pub input: Value,
    pub time: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageInput {
    pub mime: String,
    /// base64
    pub data: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Frontend → backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    /// First message. `cwd` is the frontend's working directory.
    Hello { client: String, cwd: String, protocol: u32 },
    /// Create a new session (foreground for this client: it is attached automatically).
    CreateSession {
        req: u64,
        cwd: String,
        #[serde(default)]
        agent: Option<String>,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        variant: Option<String>,
        #[serde(default)]
        name: Option<String>,
    },
    /// Attach (foreground) to an open or historical session (historical ones are opened).
    Attach { req: u64, session_id: SessionId },
    Detach { session_id: SessionId },
    /// Observe a session without attaching (split-view of background sessions).
    /// Does not make the session foreground.
    Watch { session_id: SessionId },
    Unwatch { session_id: SessionId },
    /// Subscribe to the session list (SessionList pushes on any change).
    ListSessions { req: u64 },
    Prompt {
        session_id: SessionId,
        text: String,
        #[serde(default)]
        images: Vec<ImageInput>,
    },
    Abort { session_id: SessionId },
    Permission { session_id: SessionId, call_id: String, decision: PermissionDecision },
    SetModel {
        session_id: SessionId,
        model: String,
        #[serde(default)]
        variant: Option<String>,
    },
    SetVariant { session_id: SessionId, variant: Option<String> },
    SetAgent { session_id: SessionId, agent: String },
    Rename { session_id: SessionId, name: String },
    Compact { session_id: SessionId },
    /// Remove the last user message and everything after it (no file changes reverted).
    /// Backend replies with SessionSnapshot and `Undone{text}`.
    Undo { session_id: SessionId },
    /// Close an open session (abort + unload). Allowed for any session from a frontend.
    Close { session_id: SessionId },
    /// Delete a session permanently from history.
    Delete { session_id: SessionId },
    /// Run a custom command (from `Catalog.commands`) with arguments.
    Command { session_id: SessionId, name: String, arguments: String },
    /// Reload configuration (models, agents, MCP).
    Reload,
    /// Request backend shutdown.
    Shutdown,
    Ping { req: u64 },
}

/// Backend → frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    Welcome { instance: InstanceInfo, catalog: Catalog, protocol: u32 },
    Catalog { catalog: Catalog },
    /// Reply to ListSessions and pushed on every change afterwards.
    SessionList { req: u64, sessions: Vec<SessionInfo> },
    /// Full state of a session (reply to CreateSession / Attach / Watch, or after Undo).
    SessionSnapshot {
        req: u64,
        info: SessionInfo,
        messages: Vec<Message>,
        #[serde(default)]
        permissions: Vec<PermissionRequest>,
    },
    SessionUpdated { info: SessionInfo },
    SessionClosed { session_id: SessionId },
    /// Insert or replace a whole message.
    MessageUpsert { session_id: SessionId, message: Message },
    /// Insert or replace a whole part.
    PartUpsert { session_id: SessionId, message_id: MessageId, part: Part },
    /// Append text to a text/reasoning part (or to a tool part's `input_raw`).
    PartDelta { session_id: SessionId, message_id: MessageId, part_id: PartId, delta: String },
    PermissionAsk { request: PermissionRequest },
    PermissionResolved { session_id: SessionId, call_id: String },
    Undone { session_id: SessionId, text: String },
    /// Informational toast.
    Notice {
        #[serde(default)]
        session_id: Option<SessionId>,
        level: NoticeLevel,
        text: String,
    },
    /// Reply to a request that failed.
    Error {
        #[serde(default)]
        req: u64,
        text: String,
    },
    Pong { req: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeLevel {
    Info,
    Success,
    Warning,
    Error,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let m = ClientMsg::Prompt { session_id: "s".into(), text: "hi".into(), images: vec![] };
        let s = serde_json::to_string(&m).unwrap();
        assert!(s.contains("\"type\":\"prompt\""));
        let _: ClientMsg = serde_json::from_str(&s).unwrap();
        let mut msg = Message::new(Role::User, MessageOrigin::Peer { from_session: "a".into(), from_name: "名字".into() });
        msg.parts.push(Part::Text(TextPart { id: "p".into(), text: "x".into(), time: 0, meta: None }));
        let s = serde_json::to_string(&msg).unwrap();
        let back: Message = serde_json::from_str(&s).unwrap();
        assert_eq!(back.raw_text(), "x");
    }
}
