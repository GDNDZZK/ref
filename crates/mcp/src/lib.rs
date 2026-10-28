//! Minimal MCP (Model Context Protocol) client: stdio and Streamable HTTP transports
//! (with legacy HTTP+SSE fallback). Supports initialize, tools/list (paginated),
//! tools/call, and `notifications/tools/list_changed`.
//!
//! Public API in this file is a contract used by `ref-server`; keep it stable.

use ref_proto::ImageData;
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

mod client;
mod http;
mod stdio;

pub use client::McpClient;

#[derive(Debug, Clone)]
pub enum Transport {
    /// Spawn a child process speaking newline-delimited JSON-RPC on stdio.
    Local { command: Vec<String>, env: BTreeMap<String, String>, cwd: Option<String> },
    /// Streamable HTTP endpoint (falls back to legacy SSE transport on 4xx for POST-initialize).
    Remote { url: String, headers: BTreeMap<String, String> },
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub name: String,
    pub transport: Transport,
    /// Timeout for startup / list requests (default 30s).
    pub timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct McpTool {
    /// Original tool name on the server.
    pub name: String,
    pub description: String,
    /// JSON schema object for arguments.
    pub input_schema: Value,
}

#[derive(Debug, Clone, Default)]
pub struct CallResult {
    /// Concatenated text content (text items, resource text, JSON of structuredContent if no text).
    pub text: String,
    pub images: Vec<ImageData>,
    pub is_error: bool,
}

// McpClient API (implemented in client.rs):
//   pub async fn connect(cfg: &ServerConfig) -> anyhow::Result<McpClient>
//   pub fn name(&self) -> &str
//   pub fn instructions(&self) -> Option<String>          // from initialize result
//   pub async fn list_tools(&self) -> anyhow::Result<Vec<McpTool>>
//   pub async fn call_tool(&self, name: &str, args: Value, cancel: &CancellationToken) -> anyhow::Result<CallResult>
//       (on cancel: sends notifications/cancelled and returns Err("cancelled"))
//   pub fn tools_changed(&self) -> tokio::sync::watch::Receiver<u64>  // bumps on list_changed
//   pub async fn close(&self)
#[allow(dead_code)]
fn _assert_api(_c: &CancellationToken) {}
