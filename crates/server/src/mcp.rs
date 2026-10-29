//! MCP clients shared across sessions, keyed by (server name, config JSON).

use crate::hub::{lock, Hub};
use crate::session::Session;
use async_trait::async_trait;
use hn_config::{Config, McpConfig};
use hn_mcp::{McpClient, McpTool};
use hn_tools::{Tool, ToolCtx, ToolOutput};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::OnceCell;

type Cell = Arc<OnceCell<Result<Arc<McpClient>, String>>>;

#[derive(Default)]
pub struct McpManager {
    cells: Mutex<HashMap<String, Cell>>,
}

pub struct McpTools {
    pub tools: Vec<Arc<dyn Tool>>,
    /// (server name, instructions)
    pub instructions: Vec<(String, String)>,
}

impl McpManager {
    /// Tools of every enabled MCP server in `config`, connecting as needed. Pending connections
    /// are awaited (bounded by the server timeout) so the first request already sees the tools.
    /// `retry`: reconnect servers whose previous attempt failed.
    pub async fn session_tools(&self, s: &Session, hub: &Hub, config: &Config, cwd: &str, retry: bool) -> McpTools {
        let mut waits = Vec::new();
        for (name, cfg) in &config.mcp {
            if !cfg.enabled() {
                continue;
            }
            let key = format!("{name}\u{0}{}", serde_json::to_string(cfg).unwrap_or_default());
            let cell = {
                let mut cells = lock(&self.cells);
                let cell = cells.entry(key.clone()).or_default().clone();
                let stale = match cell.get() {
                    Some(Err(_)) => retry,
                    Some(Ok(c)) => c.closed_reason().is_some() && retry,
                    None => false,
                };
                if stale {
                    let fresh: Cell = Default::default();
                    cells.insert(key, fresh.clone());
                    fresh
                } else {
                    cell
                }
            };
            waits.push((name.clone(), cfg.clone(), cell));
        }
        let futs = waits.into_iter().map(|(name, cfg, cell)| async move {
            let r = cell
                .get_or_init(|| async {
                    let r = connect(&name, &cfg, cwd).await;
                    if let Err(e) = &r {
                        tracing::warn!("MCP server {name}: {e}");
                        hub.toast_all(hn_proto::ToastLevel::Error, format!("MCP server {name} failed: {}", crate::util::ellipsize(e, 200)));
                    } else {
                        tracing::info!("MCP server {name} connected");
                    }
                    r
                })
                .await;
            (name, r.clone())
        });
        let results = futures::future::join_all(futs).await;
        let _ = s;
        let mut tools: Vec<Arc<dyn Tool>> = Vec::new();
        let mut instructions = Vec::new();
        for (name, r) in results {
            let Ok(client) = r else { continue };
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
        let cells: Vec<Cell> = lock(&self.cells).drain().map(|(_, c)| c).collect();
        let futs = cells.into_iter().filter_map(|c| c.get().and_then(|r| r.as_ref().ok()).cloned()).map(|c| async move {
            c.shutdown().await;
        });
        futures::future::join_all(futs).await;
    }
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
