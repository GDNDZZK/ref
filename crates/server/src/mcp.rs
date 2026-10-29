//! MCP manager: one shared client per (server name, config); status tracking; tool lists.

use ref_config::{Config, McpConfig};
use ref_mcp::{McpClient, McpTool, ServerConfig, Transport};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum McpStatus {
    Connecting,
    Connected,
    Failed(String),
}

struct McpState {
    status: McpStatus,
    client: Option<Arc<McpClient>>,
    tools: Vec<McpTool>,
}

pub struct McpServer {
    pub name: String,
    state: Mutex<McpState>,
    ready: tokio::sync::watch::Sender<bool>,
}

impl McpServer {
    fn lock(&self) -> std::sync::MutexGuard<'_, McpState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn status(&self) -> McpStatus {
        self.lock().status.clone()
    }
    pub fn tools(&self) -> Vec<McpTool> {
        self.lock().tools.clone()
    }
    pub fn client(&self) -> Option<Arc<McpClient>> {
        self.lock().client.clone()
    }
    pub fn instructions(&self) -> Option<String> {
        self.client().and_then(|c| c.instructions()).filter(|s| !s.trim().is_empty())
    }
    pub async fn wait_ready(&self, timeout: Duration) {
        let mut rx = self.ready.subscribe();
        let _ = tokio::time::timeout(timeout, rx.wait_for(|v| *v)).await;
    }
}

#[derive(Default)]
pub struct McpManager {
    servers: Mutex<HashMap<String, Arc<McpServer>>>,
}

fn key(name: &str, cfg: &McpConfig) -> String {
    format!("{name}\u{0}{cfg:?}")
}

fn server_config(name: &str, cfg: &McpConfig) -> ServerConfig {
    let (transport, timeout_ms) = match cfg {
        McpConfig::Local { command, environment, cwd, timeout_ms, .. } => (
            Transport::Local { command: command.clone(), env: environment.clone(), cwd: cwd.clone() },
            *timeout_ms,
        ),
        McpConfig::Remote { url, headers, timeout_ms, .. } => {
            (Transport::Remote { url: url.clone(), headers: headers.clone() }, *timeout_ms)
        }
    };
    ServerConfig {
        name: name.to_string(),
        transport,
        timeout: Duration::from_millis(timeout_ms.filter(|t| *t > 0).unwrap_or(30_000)),
    }
}

impl McpManager {
    /// Servers (enabled) for a config, sorted by name; starts connecting new ones in the background.
    pub fn servers_for(&self, cfg: &Config) -> Vec<Arc<McpServer>> {
        let mut out = Vec::new();
        let mut map = self.servers.lock().unwrap_or_else(|e| e.into_inner());
        for (name, sc) in &cfg.mcp {
            if !sc.enabled() {
                continue;
            }
            let k = key(name, sc);
            if let Some(s) = map.get(&k) {
                out.push(s.clone());
                continue;
            }
            let (tx, _) = tokio::sync::watch::channel(false);
            let server = Arc::new(McpServer {
                name: name.clone(),
                state: Mutex::new(McpState { status: McpStatus::Connecting, client: None, tools: Vec::new() }),
                ready: tx,
            });
            map.insert(k, server.clone());
            out.push(server.clone());
            let conf = server_config(name, sc);
            tokio::spawn(connect(server, conf));
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// Status lines for GetConfig.
    pub fn status_lines(&self, cfg: &Config) -> Vec<(String, String)> {
        let servers = self.servers_for(cfg);
        let mut out = Vec::new();
        for (name, sc) in &cfg.mcp {
            if !sc.enabled() {
                out.push((name.clone(), "disabled".into()));
                continue;
            }
            let st = servers.iter().find(|s| &s.name == name).map(|s| (s.status(), s.tools().len()));
            let text = match st {
                Some((McpStatus::Connected, n)) => format!("connected ({n} tools)"),
                Some((McpStatus::Connecting, _)) | None => "connecting".into(),
                Some((McpStatus::Failed(e), _)) => format!("failed: {e}"),
            };
            out.push((name.clone(), text));
        }
        out
    }

    pub async fn close_all(&self) {
        let all: Vec<Arc<McpServer>> =
            self.servers.lock().unwrap_or_else(|e| e.into_inner()).drain().map(|(_, v)| v).collect();
        for s in all {
            if let Some(c) = s.client() {
                let _ = tokio::time::timeout(Duration::from_secs(3), c.close()).await;
            }
        }
    }
}

async fn connect(server: Arc<McpServer>, conf: ServerConfig) {
    let res = async {
        let client = McpClient::connect(&conf).await?;
        let tools = client.list_tools().await?;
        anyhow::Ok((client, tools))
    }
    .await;
    match res {
        Ok((client, tools)) => {
            let client = Arc::new(client);
            let mut rx = client.tools_changed();
            {
                let mut st = server.lock();
                st.status = McpStatus::Connected;
                st.client = Some(client.clone());
                st.tools = tools;
            }
            let _ = server.ready.send(true);
            tracing::info!(server = %server.name, "MCP server connected");
            let weak = Arc::downgrade(&client);
            drop(client);
            while rx.changed().await.is_ok() {
                let Some(c) = weak.upgrade() else { break };
                match c.list_tools().await {
                    Ok(t) => server.lock().tools = t,
                    Err(e) => tracing::warn!(server = %server.name, "MCP tools refresh failed: {e:#}"),
                }
            }
        }
        Err(e) => {
            tracing::warn!(server = %server.name, "MCP connect failed: {e:#}");
            server.lock().status = McpStatus::Failed(format!("{e:#}"));
            let _ = server.ready.send(true);
        }
    }
}
