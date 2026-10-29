//! MCP clients shared across sessions, keyed by (server name, config JSON).
//!
//! Connections run in background tasks (started when a session is created or opened, and
//! at the latest by its first step), so no request handler ever waits for an MCP server.
//! Only a session's first LLM step waits for connections still in progress, and at most
//! [`FIRST_STEP_WAIT`]; later steps use whatever is connected by then.

use crate::hub::{lock, Hub};
use async_trait::async_trait;
use hn_config::{Config, McpConfig};
use hn_mcp::{McpClient, McpTool};
use hn_tools::{Tool, ToolCtx, ToolOutput};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use tokio::sync::watch;

/// Longest a session's first step waits for MCP servers that are still connecting.
pub const FIRST_STEP_WAIT: Duration = Duration::from_secs(10);

type ConnResult = Option<Result<Arc<McpClient>, String>>;

/// One server connection: None while connecting.
type Entry = watch::Receiver<ConnResult>;

#[derive(Default)]
pub struct McpManager {
    entries: Mutex<HashMap<String, Entry>>,
}

pub struct McpTools {
    pub tools: Vec<Arc<dyn Tool>>,
    /// (server name, instructions)
    pub instructions: Vec<(String, String)>,
}

impl McpManager {
    /// Start connecting every enabled server of `config` that is not connected or connecting.
    /// `retry`: also reconnect servers whose previous attempt failed or whose connection closed.
    /// Never waits.
    pub fn start(&self, hub: &Arc<Hub>, config: &Config, cwd: &str, retry: bool) -> Vec<(String, Entry)> {
        let mut out = Vec::new();
        let mut entries = lock(&self.entries);
        for (name, cfg) in &config.mcp {
            if !cfg.enabled() {
                continue;
            }
            let key = format!("{name}\u{0}{}", serde_json::to_string(cfg).unwrap_or_default());
            let stale = match entries.get(&key).map(|rx| rx.borrow().clone()) {
                None => true,
                Some(None) => false,
                Some(Some(Err(_))) => retry,
                Some(Some(Ok(c))) => retry && c.closed_reason().is_some(),
            };
            if stale {
                let (tx, rx) = watch::channel(None);
                entries.insert(key.clone(), rx);
                spawn_connect(Arc::downgrade(hub), name.clone(), cfg.clone(), cwd.to_string(), tx);
            }
            if let Some(rx) = entries.get(&key) {
                out.push((name.clone(), rx.clone()));
            }
        }
        out
    }

    /// Tools of the enabled MCP servers in `config`, starting connections as needed. Waits
    /// up to `wait` for servers still connecting; those not ready by then are left out.
    pub async fn session_tools(&self, hub: &Arc<Hub>, config: &Config, cwd: &str, retry: bool, wait: Duration) -> McpTools {
        let entries = self.start(hub, config, cwd, retry);
        let futs = entries.into_iter().map(|(name, mut rx)| async move {
            if rx.borrow().is_none() && !wait.is_zero() {
                let _ = tokio::time::timeout(wait, rx.wait_for(|v| v.is_some())).await;
            }
            let r = rx.borrow().clone();
            (name, r)
        });
        let results = futures::future::join_all(futs).await;
        let mut tools: Vec<Arc<dyn Tool>> = Vec::new();
        let mut instructions = Vec::new();
        for (name, r) in results {
            let client = match r {
                Some(Ok(c)) => c,
                Some(Err(_)) => continue,
                None => {
                    tracing::info!("MCP server {name} still connecting; its tools are not available for this step");
                    continue;
                }
            };
            if let Some(i) = client.instructions.as_ref().filter(|i| !i.trim().is_empty()) {
                instructions.push((name.clone(), i.clone()));
            }
            for t in client.tools() {
                tools.push(Arc::new(McpToolWrapper { client: client.clone(), tool: t }));
            }
        }
        tools.sort_by(|a, b| a.name().cmp(b.name()));
        tools.dedup_by(|a, b| a.name() == b.name());
        instructions.sort();
        McpTools { tools, instructions }
    }

    pub async fn shutdown_all(&self) {
        let entries: Vec<Entry> = lock(&self.entries).drain().map(|(_, c)| c).collect();
        let futs = entries.into_iter().filter_map(|rx| rx.borrow().clone().and_then(|r| r.ok())).map(|c| async move {
            c.shutdown().await;
        });
        futures::future::join_all(futs).await;
    }
}

fn spawn_connect(hub: Weak<Hub>, name: String, cfg: McpConfig, cwd: String, tx: watch::Sender<ConnResult>) {
    tokio::spawn(async move {
        let start = std::time::Instant::now();
        let r = connect(&name, &cfg, &cwd).await;
        match &r {
            Ok(_) => tracing::info!("MCP server {name} connected in {:.1}s", start.elapsed().as_secs_f64()),
            Err(e) => {
                tracing::warn!("MCP server {name}: {e}");
                if let Some(h) = hub.upgrade() {
                    h.toast_all(hn_proto::ToastLevel::Error, format!("MCP server {name} failed: {}", crate::util::ellipsize(e, 200)));
                }
            }
        }
        // Shut down a client nobody can use any more (replaced or manager drained).
        if tx.send(Some(r.clone())).is_err() {
            if let Ok(c) = r {
                c.shutdown().await;
            }
        }
    });
}

async fn connect(name: &str, cfg: &McpConfig, cwd: &str) -> Result<Arc<McpClient>, String> {
    let ms = cfg.timeout().unwrap_or(hn_mcp::DEFAULT_TIMEOUT_MS).max(1000);
    // hn_mcp bounds the handshake itself; add a margin as a safety net.
    match tokio::time::timeout(Duration::from_millis(ms + 5000), hn_mcp::connect(name, cfg, std::path::Path::new(cwd))).await {
        Ok(Ok(c)) => Ok(c),
        Ok(Err(e)) => Err(format!("{e:#}")),
        Err(_) => Err(format!("connection timed out after {ms}ms")),
    }
}

struct McpToolWrapper {
    client: Arc<McpClient>,
    tool: McpTool,
}

#[async_trait]
impl Tool for McpToolWrapper {
    fn name(&self) -> &str {
        &self.tool.name
    }
    fn description(&self) -> String {
        self.tool.description.clone()
    }
    fn parameters(&self) -> Value {
        match &self.tool.schema {
            Value::Object(_) => self.tool.schema.clone(),
            _ => serde_json::json!({"type": "object", "properties": {}}),
        }
    }
    fn title(&self, _input: &Value) -> String {
        format!("{} · {}", self.tool.server, self.tool.tool)
    }
    async fn run(&self, input: Value, ctx: &ToolCtx) -> anyhow::Result<ToolOutput> {
        let r = self.client.call(&self.tool.tool, input, &ctx.cancel).await?;
        Ok(ToolOutput { text: r.text, images: r.images, title: String::new(), is_error: r.is_error })
    }
}
