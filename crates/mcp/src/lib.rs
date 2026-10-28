//! Minimal MCP client: stdio (local) and Streamable HTTP with SSE fallback (remote).
//! Exposes MCP tools as [`hn_tools::Tool`]-like descriptors the server wraps.

use hn_config::McpConfig;
use serde_json::Value;
use std::sync::Arc;

mod client;
mod http;
mod rpc;
mod sse;
mod stdio;

pub use client::{McpClient, DEFAULT_TIMEOUT_MS, PROTOCOL_VERSION};

#[derive(Debug, Clone)]
pub struct McpTool {
    /// Server name as configured.
    pub server: String,
    /// Original tool name on the server.
    pub tool: String,
    /// Exposed name: sanitize(server) + "_" + sanitize(tool).
    pub name: String,
    pub description: String,
    /// inputSchema from the server.
    pub schema: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum McpStatus {
    Connecting,
    Connected { tools: usize },
    Disabled,
    Failed(String),
}

/// Result of a tools/call.
#[derive(Debug, Clone, Default)]
pub struct McpCallResult {
    pub text: String,
    pub images: Vec<hn_proto::ImageData>,
    pub is_error: bool,
}

/// Connect to one server (spawn/connect + initialize + tools/list), bounded by
/// `cfg.timeout()` ms (default 30s). Remote servers use Streamable HTTP, falling back to
/// legacy HTTP+SSE when the POST is rejected with a 4xx (other than 401/403/429).
pub async fn connect(name: &str, cfg: &McpConfig, cwd: &std::path::Path) -> anyhow::Result<Arc<McpClient>> {
    client::connect(name, cfg, cwd).await
}

pub fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}
