//! Wire protocol shared by `ref-server` (backend) and `ref` (TUI frontend).
//!
//! Transport: WebSocket, one JSON object per text frame.
//! Client -> server: [`ClientFrame`]. Server -> client: [`ServerFrame`].
//! All timestamps are unix epoch milliseconds.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: u32 = 1;

fn home() -> std::path::PathBuf {
    std::env::var_os("HOME").map(Into::into).unwrap_or_else(|| "/tmp".into())
}

/// Runtime state dir: `$REF_STATE_DIR` | `$XDG_STATE_HOME/ref` | `~/.local/state/ref`.
/// Instance files live in `<state_dir>/instances/<pid>.json`.
pub fn state_dir() -> std::path::PathBuf {
    if let Some(d) = std::env::var_os("REF_STATE_DIR") {
        return d.into();
    }
    match std::env::var_os("XDG_STATE_HOME") {
        Some(x) if !x.is_empty() => std::path::PathBuf::from(x).join("ref"),
        _ => home().join(".local/state/ref"),
    }
}

/// Persistent data dir: `$REF_DATA_DIR` | `$XDG_DATA_HOME/ref` | `~/.local/share/ref`.
/// Sessions live in `<data_dir>/sessions/<id>.json`.
pub fn data_dir() -> std::path::PathBuf {
    if let Some(d) = std::env::var_os("REF_DATA_DIR") {
        return d.into();
    }
    match std::env::var_os("XDG_DATA_HOME") {
        Some(x) if !x.is_empty() => std::path::PathBuf::from(x).join("ref"),
        _ => home().join(".local/share/ref"),
    }
}

pub fn instances_dir() -> std::path::PathBuf {
    state_dir().join("instances")
}

/// Epoch milliseconds now.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Instance discovery
// ---------------------------------------------------------------------------

/// Written by the server to `<state_dir>/instances/<pid>.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceInfo {
    pub pid: u32,
    pub port: u16,
    /// Address the server is bound to (e.g. `127.0.0.1` or `0.0.0.0`).
    pub bind: String,
    /// Hosts allowed to connect (IPs / hostnames, `*` = any).
    pub allow_hosts: Vec<String>,
    /// Idle timeout in seconds; 0 = exit as soon as the last frontend leaves, -1 = never.
    pub idle_timeout: i64,
    pub started_at: i64,
    pub version: String,
    /// Shared secret clients must present (`?token=` query or `Hello.token`).
    pub token: String,
}

// ---------------------------------------------------------------------------
// Core data model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct ModelRef {
    pub provider: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

impl ModelRef {
    /// `provider/model` (variant not included).
    pub fn key(&self) -> String {
        format!("{}/{}", self.provider, self.model)
    }
    /// Parses `provider/model[:variant]` or `provider/model#variant`.
    pub fn parse(s: &str) -> Option<ModelRef> {
        let (pm, variant) = match s.rsplit_once('#') {
            Some((a, b)) => (a, Some(b.to_string())),
            None => (s, None),
        };
        let (p, m) = pm.split_once('/')?;
        if p.is_empty() || m.is_empty() {
            return None;
        }
        Some(ModelRef { provider: p.into(), model: m.into(), variant })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SessionStatus {
    #[default]
    Idle,
    /// Waiting for / streaming an LLM response.
    Busy,
    /// Executing tools.
    Running,
    /// Blocked on a user permission decision.
    WaitingPermission,
    /// LLM request failed; will retry at `next_at`.
    Retrying { attempt: u32, next_at: i64, error: String },
    /// Automatic compaction in progress.
    Compacting,
}

impl SessionStatus {
    pub fn is_idle(&self) -> bool {
        matches!(self, SessionStatus::Idle)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    /// Peer name; arbitrary unicode, unique among live sessions.
    pub name: String,
    pub title: String,
    pub agent: String,
    pub model: ModelRef,
    pub cwd: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub status: SessionStatus,
    /// Number of frontends directly attached (foreground if > 0). Watchers are not counted.
    pub attached: u32,
    /// Whether the session is currently loaded in the backend (live) or only on disk.
    pub live: bool,
    /// Session that created this one via `create_session`, if any.
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub must_report: bool,
    #[serde(default)]
    pub context: ContextUsage,
    /// Current todo list (set by the `todo` tool).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub todos: Vec<Todo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    #[default]
    Pending,
    InProgress,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Todo {
    pub content: String,
    #[serde(default)]
    pub status: TodoStatus,
}

impl SessionInfo {
    pub fn is_foreground(&self) -> bool {
        self.attached > 0
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq)]
pub struct ContextUsage {
    /// Tokens used by the last request (input + cache + output).
    pub used: u64,
    /// Max context window.
    pub max: u64,
    /// Recommended compaction threshold.
    pub recommend: u64,
    /// Threshold at which automatic compaction kicks in.
    pub auto: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq)]
pub struct Usage {
    /// Uncached input tokens.
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    #[serde(default)]
    pub reasoning: u64,
}

impl Usage {
    pub fn total_context(&self) -> u64 {
        self.input + self.output + self.cache_read + self.cache_write
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
}

/// Where a user-role message came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MessageSource {
    /// Typed by a human in a frontend.
    #[default]
    Human,
    /// Delivered from another session through `send_message`.
    Peer { from_id: String, from_name: String, msg_id: String },
    /// Prompt given by the creating session through `create_session`.
    Spawn { from_id: String, from_name: String },
    /// Automatic reminder injected by the harness (e.g. must_report).
    Reminder,
    /// Summary that replaced older context (compaction).
    Compaction,
    /// Output of a `!command` shell invocation by the user.
    Shell,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub role: Role,
    /// Creation time.
    pub time: i64,
    /// Completion time (assistant), if finished.
    #[serde(default)]
    pub time_end: Option<i64>,
    #[serde(default)]
    pub source: MessageSource,
    pub parts: Vec<Part>,
    /// Model that produced this message (assistant only).
    #[serde(default)]
    pub model: Option<ModelRef>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub usage: Option<Usage>,
    /// Error text if the turn ended with an error / abort.
    #[serde(default)]
    pub error: Option<String>,
}

impl Message {
    /// Raw text of the message (text parts joined), used for "copy raw".
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageData {
    pub mime: String,
    /// Base64 (standard alphabet, no data: prefix).
    pub data: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolState {
    /// Arguments still streaming from the model.
    Streaming,
    /// Waiting for user approval.
    AwaitingPermission,
    /// Waiting the configured pre-execution delay.
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
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Part {
    Text {
        text: String,
        #[serde(default)]
        time: i64,
    },
    Reasoning {
        text: String,
        #[serde(default)]
        time: i64,
        #[serde(default)]
        time_end: Option<i64>,
        /// Opaque provider data needed to replay this block (signature, encrypted content, ...).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_data: Option<Value>,
    },
    Image {
        #[serde(flatten)]
        image: ImageData,
        #[serde(default)]
        name: Option<String>,
    },
    Tool {
        call_id: String,
        name: String,
        #[serde(default)]
        purpose: String,
        /// Parsed arguments (without `purpose`). While streaming this may be `Null`.
        #[serde(default)]
        input: Value,
        /// Raw argument JSON as streamed by the model.
        #[serde(default)]
        raw_input: String,
        state: ToolState,
        #[serde(default)]
        output: Option<String>,
        /// Extra structured info for display (e.g. diff, exit code).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        meta: Option<Value>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<ImageData>,
        #[serde(default)]
        time: i64,
        #[serde(default)]
        time_end: Option<i64>,
        /// When state == Delayed: epoch ms at which execution starts.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run_at: Option<i64>,
        /// Opaque provider data (e.g. responses API item id).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_data: Option<Value>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Delta {
    Text { text: String },
    Reasoning { text: String },
    ToolInput { text: String },
}

/// Pending permission request shown to the user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub session_id: String,
    pub message_id: String,
    pub call_id: String,
    pub tool: String,
    pub purpose: String,
    /// Full input parameters (purpose stripped).
    pub input: Value,
    pub time: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    pub name: String,
    pub description: String,
    pub model: Option<ModelRef>,
    /// Can be created by other sessions.
    pub creatable: bool,
    /// Can create / close other sessions.
    pub can_spawn: bool,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub provider: String,
    pub provider_name: String,
    pub id: String,
    pub name: String,
    pub context: u64,
    pub output: u64,
    pub images: bool,
    pub reasoning: bool,
    pub variants: Vec<String>,
}

impl ModelInfo {
    pub fn model_ref(&self, variant: Option<String>) -> ModelRef {
        ModelRef { provider: self.provider.clone(), model: self.id.clone(), variant }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandInfo {
    pub name: String,
    pub description: String,
    /// Source: "builtin" | "config" | "skill" | "mcp".
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfigInfo {
    pub agents: Vec<AgentInfo>,
    pub default_agent: String,
    pub models: Vec<ModelInfo>,
    /// User-defined slash commands (config `command`), expanded server side.
    pub commands: Vec<CommandInfo>,
    /// MCP server status lines: (name, status text).
    pub mcp: Vec<(String, String)>,
    /// TUI-related settings passed through from config (`tui` key), free-form.
    #[serde(default)]
    pub tui: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub info: SessionInfo,
    pub messages: Vec<Message>,
    pub pending_permissions: Vec<PermissionRequest>,
}

// ---------------------------------------------------------------------------
// Client -> Server
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientFrame {
    /// Correlation id; server answers with `ServerFrame::Response { id }`.
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(flatten)]
    pub req: Request,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// Must be the first frame. Registers the frontend and its working directory.
    Hello {
        client: String,
        version: u32,
        cwd: String,
        #[serde(default)]
        token: Option<String>,
    },
    /// -> `{config: ServerConfigInfo}`
    GetConfig,
    /// Live sessions. -> `{sessions: [SessionInfo]}`
    ListSessions,
    /// Persisted sessions (newest first). -> `{sessions: [SessionInfo]}`
    ListHistory {
        #[serde(default)]
        cwd: Option<String>,
        #[serde(default)]
        limit: Option<usize>,
    },
    /// -> `{session: SessionInfo}`. Does not attach.
    CreateSession {
        cwd: String,
        #[serde(default)]
        agent: Option<String>,
        #[serde(default)]
        model: Option<ModelRef>,
        #[serde(default)]
        name: Option<String>,
    },
    /// Attach to a session (loading it from disk if necessary).
    /// `foreground = true` counts as a direct connection; `false` = watch only.
    /// -> `{snapshot: SessionSnapshot}`, then events stream.
    Attach { session_id: String, foreground: bool },
    Detach { session_id: String },
    /// Subscribe to every live background session (split view). -> `{sessions: [SessionInfo]}`
    WatchAll { enabled: bool },
    Prompt {
        session_id: String,
        text: String,
        #[serde(default)]
        images: Vec<ImageData>,
    },
    /// Run `!command` on behalf of the user; output added to the conversation.
    Shell { session_id: String, command: String },
    /// Stop the current turn (esc).
    Abort { session_id: String },
    PermissionReply {
        session_id: String,
        call_id: String,
        allow: bool,
        /// Remember for the rest of the session for this tool.
        #[serde(default)]
        always: bool,
    },
    /// Change model for this session only (not remembered for new sessions).
    SetModel { session_id: String, model: ModelRef },
    SetAgent { session_id: String, agent: String },
    Rename { session_id: String, name: String },
    /// Manual compaction (/compact).
    Compact { session_id: String },
    /// Remove the last human prompt and everything after it — or, with `message_id`, that
    /// message and everything after it. Files are not restored. -> `{text}` of the removed prompt.
    Undo {
        session_id: String,
        #[serde(default)]
        message_id: Option<String>,
    },
    /// Unload a live session (it remains in history). Allowed for any session from a frontend.
    Close { session_id: String },
    /// Delete a session from history permanently.
    Delete { session_id: String },
    /// Expand a user-defined command into prompt text. -> `{text}`
    ExpandCommand { name: String, args: String, cwd: String },
    /// Fuzzy file search for `@` mentions (gitignore-aware). -> `{files: [String]}` (relative paths, dirs end with `/`)
    ListFiles {
        cwd: String,
        query: String,
        #[serde(default)]
        limit: Option<usize>,
    },
    /// Rotate the session to the next variant of its model (or a given one). -> `{session: SessionInfo}`
    SetVariant {
        session_id: String,
        #[serde(default)]
        variant: Option<String>,
    },
    /// Write the transcript as markdown to `path` (server side). -> `{path}`
    Export { session_id: String, path: String },
    /// Cross-session slash commands run for the user without an LLM turn.
    /// `command`: "peers" | "peers-name" | "peers-inbox" | "peers-outbox". -> `{text}`
    PeerCommand {
        session_id: String,
        command: String,
        #[serde(default)]
        args: String,
    },
    /// Shut the backend down (used by `ref-server --stop`).
    Shutdown,
    Ping,
}

// ---------------------------------------------------------------------------
// Server -> Client
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServerFrame {
    Response {
        id: u64,
        #[serde(default)]
        ok: bool,
        #[serde(default)]
        result: Value,
        #[serde(default)]
        error: Option<String>,
    },
    /// Event scoped to a session the client is attached to / watching.
    Event { session_id: String, event: SessionEvent },
    /// Live session list changed (sent to every client).
    Sessions { sessions: Vec<SessionInfo> },
    /// Transient notice for the user.
    Toast { level: ToastLevel, text: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToastLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionEvent {
    Info { info: SessionInfo },
    /// New message appended (or full replacement of an existing one with the same id).
    Message { message: Message },
    /// Part appended / replaced at `index` of `message_id`.
    Part { message_id: String, index: usize, part: Part },
    /// Streaming delta for part `index`.
    Delta { message_id: String, index: usize, delta: Delta },
    /// Message removed (undo, retry reset).
    Removed { message_id: String },
    /// Whole history replaced (compaction, undo) — client should replace its list.
    Reset { messages: Vec<Message> },
    Permission { request: PermissionRequest },
    PermissionResolved { call_id: String },
    /// Human-readable error for the session (also stored in message.error when relevant).
    Error { text: String },
    /// Session closed / unloaded.
    Closed,
}
