//! Wire protocol shared by `ref-server` (backend) and `ref` (TUI frontend).
//!
//! Transport: WebSocket, one JSON object per text frame.
//! Client -> server: [`ClientFrame`]. Server -> client: [`ServerFrame`].
//! All timestamps are unix epoch milliseconds.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

/// Version of the wire protocol, reported in the `Hello` response (`"protocol"`).
/// Adding fields or requests does not change it. Version 1 clients are still served.
pub const PROTOCOL_VERSION: u32 = 3;

fn home() -> std::path::PathBuf {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(Into::into)
        .or_else(dirs::home_dir)
        .unwrap_or_else(std::env::temp_dir)
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
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default, TS)]
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
        Some(ModelRef {
            provider: p.into(),
            model: m.into(),
            variant,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default, TS)]
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
    /// Blocked on the user's answer to a `question` tool call.
    WaitingQuestion,
    /// LLM request failed; will retry at `next_at`.
    Retrying {
        attempt: u32,
        next_at: i64,
        error: String,
    },
    /// Automatic compaction in progress.
    Compacting,
}

impl SessionStatus {
    pub fn is_idle(&self) -> bool {
        matches!(self, SessionStatus::Idle)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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
    /// When the current run started (epoch ms); `None` while idle. Frontends show the working
    /// time from this, so it survives switching sessions and covers unattached ones.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub busy_since: Option<i64>,
    /// Error that ended the last run; cleared when the next run starts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, TS)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    #[default]
    Pending,
    InProgress,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, TS)]
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, TS)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
}

/// Where a user-role message came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MessageSource {
    /// Typed by a human in a frontend.
    #[default]
    Human,
    /// Delivered from another session through `send_message`.
    Peer {
        from_id: String,
        from_name: String,
        msg_id: String,
    },
    /// Prompt given by the creating session through `create_session`.
    Spawn { from_id: String, from_name: String },
    /// Automatic reminder injected by the harness (e.g. must_report).
    Reminder,
    /// Summary that replaced older context (compaction). Messages BEFORE the last Compaction
    /// message stay in the transcript (visible to users) but are no longer sent to the model.
    Compaction,
    /// Output of a `!command` shell invocation by the user.
    Shell,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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

    /// Replaces every image by a stub (see [`ImageData`]).
    pub fn strip_image_data(&mut self) {
        for (i, p) in self.parts.iter_mut().enumerate() {
            p.strip_image_data(&self.id, i);
        }
    }

    /// Images attached to a prompt (`Image` parts) with their names, in order.
    pub fn prompt_images(&self) -> Vec<NamedImage> {
        self.parts
            .iter()
            .filter_map(|p| match p {
                Part::Image { image, name } => Some(NamedImage {
                    image: image.clone(),
                    name: name.clone(),
                }),
                _ => None,
            })
            .collect()
    }

    /// The image a stub id (see [`ImageData::id`]) of this message points to.
    pub fn image(&self, image_id: &str) -> Option<&ImageData> {
        let mut it = image_id.rsplitn(3, ':');
        let (n, part, msg) = (
            it.next()?.parse::<usize>().ok()?,
            it.next()?.parse::<usize>().ok()?,
            it.next()?,
        );
        if msg != self.id {
            return None;
        }
        match self.parts.get(part)? {
            Part::Image { image, .. } if n == 0 => Some(image),
            Part::Tool { images, .. } => images.get(n),
            _ => None,
        }
    }
}

/// Message id part of an image stub id (see [`ImageData::id`]).
pub fn image_message_id(image_id: &str) -> Option<&str> {
    image_id.rsplitn(3, ':').nth(2)
}

/// Frontends never receive image bytes in messages: the server replaces each image by a stub
/// (empty `data`, with `id` and `size`) to keep snapshots and events small. `GetImage` with
/// the `id` returns the bytes; `Undo` / `RemoveQueued` return those of the prompt they remove.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
pub struct ImageData {
    pub mime: String,
    /// Base64 (standard alphabet, no data: prefix). Empty in stubs.
    pub data: String,
    /// Stubs only: opaque id for `GetImage` (`<message id>:<part index>:<image index>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Stubs only: decoded size in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

impl ImageData {
    pub fn new(mime: impl Into<String>, data: impl Into<String>) -> Self {
        ImageData {
            mime: mime.into(),
            data: data.into(),
            id: None,
            size: None,
        }
    }

    /// True if the bytes were left out (see the type docs).
    pub fn is_stub(&self) -> bool {
        self.data.is_empty()
    }

    /// Stub of this image with the given id.
    fn make_stub(&mut self, id: String) {
        if !self.is_stub() {
            let pad = self.data.bytes().rev().take_while(|&b| b == b'=').count();
            self.size = Some((self.data.len() / 4 * 3).saturating_sub(pad) as u64);
            self.data = String::new();
        }
        self.id = Some(id);
    }
}

/// An image with its optional file name, as returned by `Undo` / `RemoveQueued`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct NamedImage {
    #[serde(flatten)]
    pub image: ImageData,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
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
        matches!(
            self,
            ToolState::Done | ToolState::Error | ToolState::Denied | ToolState::Cancelled
        )
    }
}

// `Part::Tool` is much larger than the other variants. Boxing it would change every place that
// builds or matches parts for a small memory saving, so the size difference is accepted.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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

impl Part {
    /// Replaces the images of part `index` of message `message_id` by stubs (see [`ImageData`]).
    pub fn strip_image_data(&mut self, message_id: &str, index: usize) {
        match self {
            Part::Image { image, .. } => image.make_stub(format!("{message_id}:{index}:0")),
            Part::Tool { images, .. } => {
                for (n, i) in images.iter_mut().enumerate() {
                    i.make_stub(format!("{message_id}:{index}:{n}"));
                }
            }
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Delta {
    Text { text: String },
    Reasoning { text: String },
    ToolInput { text: String },
}

/// Kind of system notification a frontend can ask for (see `Request::SetNotify`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum NotifyKind {
    /// A tool call is waiting for a permission decision.
    Permission,
    /// The model asked the user a question (`detail`: the first question, shortened).
    Question,
    /// A run finished normally and the session is idle again.
    TurnDone,
    /// A run ended with a final error (retries do not count; user aborts are not reported).
    TurnError,
}

/// Pending permission request shown to the user.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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

/// One choice of a [`Question`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct QuestionOption {
    /// Display text, one to five words.
    pub label: String,
    /// What choosing it means.
    pub description: String,
    /// Preselected when the question is shown (single choice: where the cursor starts).
    #[serde(default)]
    pub default: bool,
}

/// One question of a `question` tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Question {
    /// The complete question.
    pub question: String,
    /// Very short label (at most 30 characters), used for the tab of a multi-question dialog.
    pub header: String,
    pub options: Vec<QuestionOption>,
    /// More than one option may be chosen.
    #[serde(default)]
    pub multiple: bool,
    /// The user may type an answer of their own, shown as a last row after the options.
    #[serde(default = "default_true")]
    pub custom: bool,
}

fn default_true() -> bool {
    true
}

/// A pending `question` tool call shown to the user.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct QuestionRequest {
    pub session_id: String,
    pub message_id: String,
    pub call_id: String,
    pub questions: Vec<Question>,
    pub time: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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
        ModelRef {
            provider: self.provider.clone(),
            model: self.id.clone(),
            variant,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct CommandInfo {
    pub name: String,
    pub description: String,
    /// Source: "builtin" | "config" | "skill" | "mcp".
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct McpServerStatus {
    pub name: String,
    /// "connected" | "connecting" | "failed" | "disabled"
    pub status: String,
    #[serde(default)]
    pub error: Option<String>,
    /// Tool names as exposed to the model (`<server>_<tool>`).
    #[serde(default)]
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct SessionSnapshot {
    pub info: SessionInfo,
    /// The newest messages (one page, see `Attach`); older ones via `Messages`.
    pub messages: Vec<Message>,
    /// Older messages exist before `messages[0]`.
    #[serde(default)]
    pub more_before: bool,
    pub pending_permissions: Vec<PermissionRequest>,
    #[serde(default)]
    pub pending_questions: Vec<QuestionRequest>,
    /// Messages waiting in the session's queue (not yet in `messages`), any source; human
    /// prompts are removable with `RemoveQueued`.
    #[serde(default)]
    pub queued: Vec<Message>,
}

// ---------------------------------------------------------------------------
// Client -> Server
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ClientFrame {
    /// Correlation id; server answers with `ServerFrame::Response { id }`.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub id: Option<u64>,
    #[serde(flatten)]
    pub req: Request,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "op", rename_all = "snake_case")]
#[ts(optional_fields = nullable)]
pub enum Request {
    /// Must be the first frame. Registers the frontend and its working directory.
    Hello {
        client: String,
        version: u32,
        cwd: String,
        #[serde(default)]
        token: Option<String>,
        /// Notification kinds this connection is willing to show; empty = none.
        #[serde(default)]
        notify: Vec<NotifyKind>,
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
    /// -> `{snapshot: SessionSnapshot}`, then events stream. The snapshot holds the newest
    /// `limit` messages (default and max: see `Messages`), fewer if they are large.
    Attach {
        session_id: String,
        foreground: bool,
        #[serde(default)]
        limit: Option<u32>,
    },
    /// A page of older messages: up to `limit` (default 100, max 1000; fewer once the page
    /// exceeds about 1 MiB) ending right before message `before`.
    /// -> `{messages: [Message], more_before}`
    Messages {
        session_id: String,
        before: String,
        #[serde(default)]
        limit: Option<u32>,
    },
    /// Bytes of an image stub (see `ImageData`). -> `{image: ImageData}`
    GetImage {
        session_id: String,
        image_id: String,
    },
    Detach {
        session_id: String,
    },
    /// Subscribe to every live background session (split view). -> `{sessions: [SessionInfo]}`
    WatchAll {
        enabled: bool,
    },
    /// -> `{queued, message_id}`. `queued` = the session was busy and the message waits in its
    /// queue until the next safe point (it can be withdrawn with `RemoveQueued` until then).
    Prompt {
        session_id: String,
        text: String,
        #[serde(default)]
        images: Vec<ImageData>,
    },
    /// Withdraw a queued (not yet delivered) message. -> `{removed, text, images: [NamedImage]}`;
    /// `removed` is false when it was already delivered (or is unknown).
    RemoveQueued {
        session_id: String,
        message_id: String,
    },
    /// Run `!command` on behalf of the user; output added to the conversation.
    Shell {
        session_id: String,
        command: String,
    },
    /// Stop the current turn (esc).
    Abort {
        session_id: String,
    },
    PermissionReply {
        session_id: String,
        call_id: String,
        allow: bool,
        /// Remember for the rest of the session for this tool.
        #[serde(default)]
        always: bool,
    },
    /// Answer a pending question. `answers` has one entry per question, in order; each entry
    /// lists the chosen labels or the typed text. `None` dismisses the question. The first
    /// reply wins; a later one fails with "No pending question with that id."
    QuestionReply {
        session_id: String,
        call_id: String,
        #[serde(default)]
        answers: Option<Vec<Vec<String>>>,
    },
    /// Change model for this session only (not remembered for new sessions).
    SetModel {
        session_id: String,
        model: ModelRef,
    },
    SetAgent {
        session_id: String,
        agent: String,
    },
    Rename {
        session_id: String,
        name: String,
    },
    /// Change the session's working directory (`/cd`). `path` is absolute or relative to the
    /// current one; `~` is expanded. Rejected while the session is busy. A reminder telling the
    /// model about the change is appended (no LLM turn). -> `{session: SessionInfo}`
    SetCwd {
        session_id: String,
        path: String,
    },
    /// Manual compaction (/compact).
    Compact {
        session_id: String,
    },
    /// Remove the last human prompt and everything after it — or, with `message_id`, that
    /// message and everything after it. Files are not restored. -> `{text, images: [NamedImage]}`
    /// of the removed prompt (both empty unless it was a human prompt).
    Undo {
        session_id: String,
        #[serde(default)]
        message_id: Option<String>,
    },
    /// Unload a live session (it remains in history). Allowed for any session from a frontend.
    Close {
        session_id: String,
    },
    /// Delete a session from history permanently.
    Delete {
        session_id: String,
    },
    /// Expand a user-defined command into prompt text. -> `{text}`
    ExpandCommand {
        name: String,
        args: String,
        cwd: String,
    },
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
    Export {
        session_id: String,
        path: String,
    },
    /// Cross-session slash commands run for the user without an LLM turn.
    /// `command`: "peers" | "peers-name" | "peers-inbox" | "peers-outbox". -> `{text}`
    PeerCommand {
        session_id: String,
        command: String,
        #[serde(default)]
        args: String,
    },
    /// MCP servers for the connection's cwd config (or `cwd`). -> `{servers: [McpServerStatus]}`
    McpStatus {
        #[serde(default)]
        cwd: Option<String>,
    },
    /// Replace the notification kinds this connection wants (see `Hello.notify`). -> `{}`
    SetNotify {
        kinds: Vec<NotifyKind>,
    },
    /// Tell the backend whether the window / tab is in the foreground and which sessions it
    /// shows (one in a single-session view, all tiles in the split view). Send on every change.
    /// A `ServerFrame::Notify` is sent only when no connection shows the session. -> `{}`
    SetFocus {
        focused: bool,
        sessions: Vec<String>,
    },
    /// Sub-directories of `path` for a directory picker: `~` is expanded and a relative path
    /// is taken from the connection's cwd. -> `{path: <absolute, normalized>, dirs: [String]}`
    /// (names only, sorted)
    ListDirs {
        path: String,
    },
    /// Shut the backend down (used by `ref-server --stop`).
    Shutdown,
    Ping,
}

// ---------------------------------------------------------------------------
// Server -> Client
// ---------------------------------------------------------------------------

// `ServerFrame::Event` carries a whole `SessionEvent`; frames are short-lived, so it stays unboxed.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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
    Event {
        session_id: String,
        event: SessionEvent,
    },
    /// Live session list changed (sent to every client).
    Sessions { sessions: Vec<SessionInfo> },
    /// Transient notice for the user.
    Toast { level: ToastLevel, text: String },
    /// A system notification for a connection that asked for `notify_kind` and whose user is not
    /// looking at the session. `detail`: tool name (permission), error text (turn_error),
    /// null (turn_done). Clients build the notification text themselves.
    Notify {
        session_id: String,
        /// Not called `kind`: that is the tag of `ServerFrame`.
        notify_kind: NotifyKind,
        session_title: String,
        detail: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ToastLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionEvent {
    Info {
        info: SessionInfo,
    },
    /// New message appended (or full replacement of an existing one with the same id).
    Message {
        message: Message,
    },
    /// Part appended / replaced at `index` of `message_id`.
    Part {
        message_id: String,
        index: usize,
        part: Part,
    },
    /// Streaming delta for part `index`.
    Delta {
        message_id: String,
        index: usize,
        delta: Delta,
    },
    /// Message removed (undo, retry reset).
    Removed {
        message_id: String,
    },
    /// Whole history replaced (compaction, undo) — client should replace its list. Like a
    /// snapshot this carries only the newest page.
    Reset {
        messages: Vec<Message>,
        #[serde(default)]
        more_before: bool,
    },
    Permission {
        request: PermissionRequest,
    },
    /// The model asks the user a question (`WaitingQuestion`); see `QuestionReply`.
    Question {
        request: QuestionRequest,
    },
    /// The question with this call id was answered, dismissed or cancelled.
    QuestionResolved {
        call_id: String,
    },
    PermissionResolved {
        call_id: String,
    },
    /// The session's queue changed: every message (any source) still waiting for the next safe
    /// point, in delivery order. Delivered messages arrive as `Message` events.
    Queue {
        messages: Vec<Message>,
    },
    /// Human-readable error for the session (also stored in message.error when relevant).
    Error {
        text: String,
    },
    /// Session closed / unloaded.
    Closed,
}

#[cfg(test)]
mod tests;
