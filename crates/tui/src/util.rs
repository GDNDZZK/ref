//! Names and pure helpers shared by the client layers and the in-process mock backend.

use ref_proto::{ModelRef, SessionInfo, SessionStatus};

/// Error text a message carries when its turn was aborted (matched case-insensitively).
pub const ABORTED_ERROR: &str = "Aborted";

/// Status strings of an MCP server as the backend reports them.
pub mod mcp_status {
    pub const CONNECTED: &str = "connected";
    pub const CONNECTING: &str = "connecting";
    pub const FAILED: &str = "failed";
    pub const DISABLED: &str = "disabled";
}

/// Slash-command names, shared by the `COMMANDS` table and everything that dispatches on them.
pub mod cmd {
    pub const NEW: &str = "new";
    pub const SESSIONS: &str = "sessions";
    pub const MODELS: &str = "models";
    pub const AGENTS: &str = "agents";
    pub const VARIANT: &str = "variant";
    pub const COMPACT: &str = "compact";
    pub const UNDO: &str = "undo";
    pub const RENAME: &str = "rename";
    pub const CD: &str = "cd";
    pub const CLOSE: &str = "close";
    pub const CLOSE_SESSION: &str = "close-session";
    pub const PEERS: &str = "peers";
    pub const PEERS_NAME: &str = "peers-name";
    pub const PEERS_INBOX: &str = "peers-inbox";
    pub const PEERS_OUTBOX: &str = "peers-outbox";
    pub const EXPORT: &str = "export";
    pub const COPY: &str = "copy";
    pub const THINKING: &str = "thinking";
    pub const DETAILS: &str = "details";
    pub const WATCH: &str = "watch";
    pub const SIDEBAR: &str = "sidebar";
    pub const MCP: &str = "mcp";
    pub const EDITOR: &str = "editor";
    pub const HELP: &str = "help";
    pub const EXIT: &str = "exit";
}

/// Session info for a session the client has not heard about yet.
pub(crate) fn placeholder_info(sid: &str, cwd: &str) -> SessionInfo {
    SessionInfo {
        id: sid.to_string(),
        name: String::new(),
        title: String::new(),
        agent: String::new(),
        model: ModelRef::default(),
        cwd: cwd.to_string(),
        created_at: 0,
        updated_at: 0,
        status: SessionStatus::Idle,
        attached: 0,
        live: true,
        parent: None,
        must_report: false,
        context: Default::default(),
        todos: Vec::new(),
        busy_since: None,
        last_error: None,
    }
}
