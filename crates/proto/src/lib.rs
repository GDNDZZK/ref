//! Wire protocol shared by `hernees-server` (backend) and `hernees` (TUI frontend).
//!
//! Transport: WebSocket, one JSON text frame per message.
//! Client sends [`ClientMsg`] envelopes ([`Request`]), server replies with
//! [`ServerMsg::Response`] carrying the same `id`, and pushes [`ServerMsg::Event`]
//! / [`ServerMsg::Sessions`] / [`ServerMsg::Toast`] at any time.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: u32 = 1;

/// Milliseconds since unix epoch.
pub type Millis = i64;

pub fn now_ms() -> Millis {
    chrono::Utc::now().timestamp_millis()
}

// ---------------------------------------------------------------------------
// Models / agents
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct ModelRef {
    pub provider: String,
    pub model: String,
    /// Variant name (opencode `variants` key, e.g. "high"). None = model default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

impl ModelRef {
    /// Parse `provider/model[#variant]`.
    pub fn parse(s: &str) -> Option<ModelRef> {
        let (pm, variant) = match s.split_once('#') {
            Some((a, b)) => (a, Some(b.to_string()).filter(|v| !v.is_empty())),
            None => (s, None),
        };
        let (p, m) = pm.split_once('/')?;
        if p.is_empty() || m.is_empty() {
            return None;
        }
        Some(ModelRef { provider: p.into(), model: m.into(), variant })
    }
    pub fn key(&self) -> String {
        format!("{}/{}", self.provider, self.model)
    }
}

impl std::fmt::Display for ModelRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.provider, self.model)?;
        if let Some(v) = &self.variant {
            write!(f, "#{v}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub provider: String,
    pub provider_name: String,
    pub model: String,
    pub name: String,
    pub context: u64,
    pub output: u64,
    pub reasoning: bool,
    pub image_input: bool,
    /// Variant names in config order.
    pub variants: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    pub name: String,
    pub description: String,
    pub model: Option<ModelRef>,
    /// May be spawned by other sessions via `create_session`.
    pub spawnable: bool,
    /// May create/close other sessions.
    pub manage_sessions: bool,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandInfo {
    pub name: String,
    pub description: String,
    /// "custom" | "skill" | "mcp"
    pub source: String,
}

// ---------------------------------------------------------------------------
// Sessions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionStatus {
    #[default]
    Idle,
    /// Waiting for / streaming an LLM response or running tools.
    Busy,
    /// LLM call failed; will retry at `next_at`.
    Retry { attempt: u32, next_at: Millis, error: String },
    /// Blocked on a permission prompt.
    Permission,
}

impl SessionStatus {
    pub fn is_active(&self) -> bool {
        !matches!(self, SessionStatus::Idle)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Usage {
    /// Non-cached input tokens.
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub reasoning: u64,
}

impl Usage {
    /// Total tokens occupying the context window after this response.
    pub fn context_tokens(&self) -> u64 {
        self.input + self.cache_read + self.cache_write + self.output
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContextInfo {
    pub used: u64,
    pub max: u64,
    /// Recommended token count at which to compact.
    pub compact_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionInfo {
    pub id: String,
    /// Display/peer name. Any unicode allowed.
    pub name: String,
    pub agent: String,
    pub model: ModelRef,
    pub cwd: String,
    pub created_at: Millis,
    pub updated_at: Millis,
    /// Session that created this one (via create_session tool).
    #[serde(default)]
    pub parent: Option<String>,
    /// Must send a message to `parent` before stopping.
    #[serde(default)]
    pub must_report: bool,
    #[serde(default)]
    pub status: SessionStatus,
    /// Loaded in the backend (live).
    #[serde(default)]
    pub open: bool,
    /// Number of frontends directly attached (foreground if > 0).
    #[serde(default)]
    pub attached: u32,
    #[serde(default)]
    pub context: ContextInfo,
    #[serde(default)]
    pub cost: f64,
    #[serde(default)]
    pub message_count: u32,
}

impl SessionInfo {
    pub fn is_foreground(&self) -> bool {
        self.attached > 0
    }
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
}

/// Where a user-role message came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Origin {
    /// Typed by a human in a frontend.
    #[default]
    Human,
    /// Message from another session.
    Peer { from_id: String, from_name: String },
    /// Automatic harness reminder (e.g. must_report reminder).
    System,
    /// Compaction checkpoint: everything before this message is dropped from LLM context.
    Compaction,
    /// Initial prompt given by a creating session.
    Spawn { from_id: String, from_name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageData {
    pub mime: String,
    /// Base64 (no data: prefix).
    pub data: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolState {
    /// Arguments still streaming.
    #[default]
    Streaming,
    /// Waiting for user approval.
    Permission,
    /// Waiting configured pre-execution delay (see `ToolPart::run_at`).
    Delayed,
    Running,
    Done,
    Error,
    /// Rejected by permission rule or user.
    Denied,
    /// Aborted by user.
    Cancelled,
}

impl ToolState {
    pub fn is_final(&self) -> bool {
        matches!(self, ToolState::Done | ToolState::Error | ToolState::Denied | ToolState::Cancelled)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolPart {
    pub call_id: String,
    pub name: String,
    /// Raw argument JSON text as streamed by the model.
    pub raw_input: String,
    /// Parsed input (includes `purpose`). Null while streaming.
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub purpose: String,
    pub state: ToolState,
    /// Raw output text sent back to the model.
    #[serde(default)]
    pub output: String,
    #[serde(default)]
    pub images: Vec<ImageData>,
    pub time_start: Millis,
    #[serde(default)]
    pub time_end: Option<Millis>,
    /// When state == Delayed: execution timestamp.
    #[serde(default)]
    pub run_at: Option<Millis>,
    /// Short UI title computed by the tool (e.g. "src/main.rs:10-40").
    #[serde(default)]
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Part {
    Text {
        text: String,
        time: Millis,
    },
    Reasoning {
        text: String,
        time_start: Millis,
        #[serde(default)]
        time_end: Option<Millis>,
        /// Provider-specific opaque replay data (signature / encrypted item / details).
        #[serde(default, skip_serializing_if = "Value::is_null")]
        replay: Value,
    },
    Tool(ToolPart),
    Image(ImageData),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub role: Role,
    #[serde(default)]
    pub origin: Origin,
    pub time: Millis,
    #[serde(default)]
    pub completed: Option<Millis>,
    pub parts: Vec<Part>,
    /// Assistant only.
    #[serde(default)]
    pub model: Option<ModelRef>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub usage: Option<Usage>,
    /// Assistant: provider finish reason ("stop", "tool_use", "length", "aborted", "error").
    #[serde(default)]
    pub finish: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    /// Protocol + model the replay data belongs to ("anthropic:claude-x").
    #[serde(default)]
    pub replay_key: Option<String>,
}

impl Message {
    /// Concatenated raw text parts (for "copy raw" button).
    pub fn raw_text(&self) -> String {
        let mut out = String::new();
        for p in &self.parts {
            if let Part::Text { text, .. } = p {
                if !out.is_empty() {
                    out.push_str("\n\n");
                }
                out.push_str(text);
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Client -> server
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    #[serde(flatten)]
    pub msg: ClientMsg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    Once,
    /// Allow this tool for the rest of the session.
    Always,
    Reject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListScope {
    /// Live sessions in this backend.
    Open,
    /// Persisted sessions (includes open ones).
    History,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    /// First message. Registers the frontend and its working directory.
    Hello { client: String, cwd: String, protocol: u32 },
    ListSessions { scope: ListScope },
    /// Create + attach as foreground. Response: SessionInfo.
    CreateSession {
        cwd: String,
        #[serde(default)]
        agent: Option<String>,
        #[serde(default)]
        model: Option<ModelRef>,
        #[serde(default)]
        name: Option<String>,
    },
    /// Attach to a session. `foreground=false` = watch only (does not make it foreground).
    /// Loads it from disk if not open. Server then sends `Event::Snapshot`.
    Attach { session: String, foreground: bool },
    Detach { session: String },
    /// Watch every background session (and new ones as they appear).
    WatchBackground { enabled: bool },
    Prompt {
        session: String,
        text: String,
        #[serde(default)]
        images: Vec<ImageData>,
    },
    /// Run a custom command / skill: `/name args`.
    Command { session: String, name: String, args: String },
    Abort { session: String },
    Permission { session: String, call_id: String, decision: PermissionDecision },
    SetModel { session: String, model: ModelRef },
    SetAgent { session: String, agent: String },
    Rename { session: String, name: String },
    /// Summarize + compact context with the LLM.
    Compact { session: String },
    /// Remove the last user message and everything after it. Response: the removed user text.
    Undo { session: String },
    /// Unload from backend (stays in history). Aborts if busy.
    Close { session: String },
    Delete { session: String },
    ListModels,
    ListAgents,
    ListCommands { session: String },
    /// Export transcript as markdown. Response: string.
    Export { session: String },
    Ping,
}

// ---------------------------------------------------------------------------
// Server -> client
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    Response {
        id: u64,
        #[serde(default)]
        ok: Option<Value>,
        #[serde(default)]
        error: Option<String>,
    },
    Event { session: String, event: Event },
    /// Full list of open sessions, pushed on any change.
    Sessions { sessions: Vec<SessionInfo> },
    Toast { level: ToastLevel, message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToastLevel {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeltaField {
    Text,
    Reasoning,
    ToolInput,
    /// Live output of a running tool (e.g. bash). Final output replaces it via `Event::Part`.
    ToolOutput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// Sent after Attach: full state.
    Snapshot { info: SessionInfo, messages: Vec<Message> },
    Info { info: SessionInfo },
    /// Insert or replace a whole message.
    Message { message: Message },
    /// Insert or replace one part.
    Part { message: String, index: usize, part: Part },
    /// Append streamed text to a part (creates nothing; part must exist).
    Delta { message: String, index: usize, field: DeltaField, text: String },
    /// Messages removed (undo / failed attempt).
    Removed { messages: Vec<String> },
    /// Session closed / deleted on the backend.
    Closed,
}

impl ServerMsg {
    pub fn ok(id: u64, v: impl Serialize) -> Self {
        ServerMsg::Response { id, ok: Some(serde_json::to_value(v).unwrap_or(Value::Null)), error: None }
    }
    pub fn err(id: u64, e: impl std::fmt::Display) -> Self {
        ServerMsg::Response { id, ok: None, error: Some(e.to_string()) }
    }
}

// ---------------------------------------------------------------------------
// Instance registry (shared by both binaries)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceInfo {
    pub pid: u32,
    pub port: u16,
    /// Address the server is bound to.
    pub bind: String,
    /// Allowed client hosts.
    pub allow_hosts: Vec<String>,
    /// Seconds without frontends before exit; 0 = immediately; -1 = never.
    pub idle_timeout: i64,
    pub started_at: Millis,
    pub version: String,
    /// Absolute path of the server executable.
    pub exe: String,
}

/// Directory where running instances register themselves.
pub fn instances_dir() -> std::path::PathBuf {
    state_dir().join("instances")
}

pub fn state_dir() -> std::path::PathBuf {
    if let Ok(d) = std::env::var("HERNEES_STATE_DIR") {
        return d.into();
    }
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| std::env::temp_dir());
    base.join("hernees")
}

pub fn data_dir() -> std::path::PathBuf {
    if let Ok(d) = std::env::var("HERNEES_DATA_DIR") {
        return d.into();
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| std::env::temp_dir());
    base.join("hernees")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_ref_parse() {
        let m = ModelRef::parse("dgn/claude-opus-5.5#high").unwrap();
        assert_eq!(m.provider, "dgn");
        assert_eq!(m.model, "claude-opus-5.5");
        assert_eq!(m.variant.as_deref(), Some("high"));
        let m = ModelRef::parse("openroute/stealth/ox-alpha").unwrap();
        assert_eq!(m.model, "stealth/ox-alpha");
        assert!(ModelRef::parse("nope").is_none());
    }

    #[test]
    fn roundtrip() {
        let r = Request { id: 3, msg: ClientMsg::Abort { session: "s".into() } };
        let s = serde_json::to_string(&r).unwrap();
        let back: Request = serde_json::from_str(&s).unwrap();
        assert_eq!(back.id, 3);
        let ev = ServerMsg::Event {
            session: "x".into(),
            event: Event::Delta { message: "m".into(), index: 0, field: DeltaField::Text, text: "hi".into() },
        };
        let s = serde_json::to_string(&ev).unwrap();
        let _: ServerMsg = serde_json::from_str(&s).unwrap();
    }
}
