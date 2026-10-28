//! Minimal MCP client: stdio (local) and Streamable HTTP with SSE fallback (remote).
//! Only the `tools` capability is used (tools/list, tools/call, notifications/tools/list_changed).

use std::collections::BTreeMap;
use std::time::Duration;

pub use ref_proto::spec::{McpServerConfig, ToolDef};
use ref_proto::{ImagePart, McpStatus};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

pub struct McpTool {
    pub server: String,
    /// Original MCP tool name.
    pub tool: String,
    /// Exposed name: `server_tool` sanitized to `[a-zA-Z0-9_-]`, max 64 chars.
    pub name: String,
    pub def: ToolDef,
}

pub struct McpCallResult {
    pub text: String,
    pub images: Vec<ImagePart>,
    pub is_error: bool,
}

pub struct McpManager {
    // implementation-defined
}

impl McpManager {
    /// Connect to all enabled servers concurrently (each with its own timeout, default 30s).
    /// Failures are recorded in status, never returned as errors.
    pub async fn start(servers: BTreeMap<String, McpServerConfig>) -> Self {
        let _ = servers;
        todo!()
    }

    /// All tools of connected servers (stable order: by server then tool name).
    pub fn tools(&self) -> Vec<McpTool> {
        todo!()
    }

    pub fn status(&self) -> Vec<McpStatus> {
        todo!()
    }

    /// Call a tool by exposed name.
    pub async fn call(&self, name: &str, args: Value, abort: CancellationToken, timeout: Duration) -> McpCallResult {
        let _ = (name, args, abort, timeout);
        todo!()
    }

    /// Kill local servers / close sessions.
    pub async fn shutdown(&self) {}
}
