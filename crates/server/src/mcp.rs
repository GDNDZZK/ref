//! MCP manager: one shared client per (server name, config); status tracking; tool lists.

use crate::util::lock;
use ref_config::{Config, McpConfig};
use ref_mcp::{McpClient, McpTool, ServerConfig, Transport};
use ref_proto::{MCP_CONNECTED, MCP_CONNECTING, MCP_DISABLED, MCP_FAILED};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long `close_all` waits for one server to close.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(3);

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
        lock(&self.state)
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
        self.client()
            .and_then(|c| c.instructions())
            .filter(|s| !s.trim().is_empty())
    }
    pub(crate) async fn wait_ready(&self, timeout: Duration) {
        let mut rx = self.ready.subscribe();
        if tokio::time::timeout(timeout, rx.wait_for(|v| *v))
            .await
            .is_err()
        {
            tracing::warn!(server = %self.name, "MCP server not ready after {timeout:?}");
        }
    }
}

#[derive(Default)]
pub struct McpManager {
    servers: Mutex<HashMap<String, Arc<McpServer>>>,
}

fn key(name: &str, cfg: &McpConfig) -> String {
    format!("{name}\u{0}{cfg:?}")
}

/// Request timeout of a server whose config sets none.
const DEFAULT_TIMEOUT_MS: u64 = 30_000;

fn server_config(name: &str, cfg: &McpConfig) -> ServerConfig {
    let (transport, timeout_ms) = match cfg {
        McpConfig::Local {
            command,
            environment,
            cwd,
            timeout_ms,
            ..
        } => (
            Transport::Local {
                command: command.clone(),
                env: environment.clone(),
                cwd: cwd.clone(),
            },
            *timeout_ms,
        ),
        McpConfig::Remote {
            url,
            headers,
            timeout_ms,
            ..
        } => (
            Transport::Remote {
                url: url.clone(),
                headers: headers.clone(),
            },
            *timeout_ms,
        ),
    };
    ServerConfig {
        name: name.to_string(),
        transport,
        timeout: Duration::from_millis(timeout_ms.filter(|t| *t > 0).unwrap_or(DEFAULT_TIMEOUT_MS)),
    }
}

impl McpManager {
    /// Servers (enabled) for a config, sorted by name; starts connecting new ones in the background.
    pub(crate) fn servers_for(&self, cfg: &Config) -> Vec<Arc<McpServer>> {
        let mut out = Vec::new();
        let mut map = lock(&self.servers);
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
                state: Mutex::new(McpState {
                    status: McpStatus::Connecting,
                    client: None,
                    tools: Vec::new(),
                }),
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

    /// Detailed status of every configured server (sorted by name), with tool names as
    /// exposed to the model.
    pub fn status(&self, cfg: &Config) -> Vec<ref_proto::McpServerStatus> {
        let servers = self.servers_for(cfg);
        let mut out = Vec::new();
        for (name, sc) in &cfg.mcp {
            let mut st = ref_proto::McpServerStatus {
                name: name.clone(),
                status: MCP_DISABLED.into(),
                error: None,
                tools: Vec::new(),
            };
            if sc.enabled() {
                match servers.iter().find(|s| &s.name == name) {
                    Some(s) => match s.status() {
                        McpStatus::Connected => {
                            st.status = MCP_CONNECTED.into();
                            let mut tools: Vec<String> = s
                                .tools()
                                .iter()
                                .map(|t| crate::toolset::mcp_tool_name(name, &t.name))
                                .collect();
                            tools.sort();
                            st.tools = tools;
                        }
                        McpStatus::Connecting => st.status = MCP_CONNECTING.into(),
                        McpStatus::Failed(e) => {
                            st.status = MCP_FAILED.into();
                            st.error = Some(e);
                        }
                    },
                    None => st.status = MCP_CONNECTING.into(),
                }
            }
            out.push(st);
        }
        out
    }

    /// Status lines for GetConfig: (name, status text).
    pub(crate) fn status_lines(&self, cfg: &Config) -> Vec<(String, String)> {
        self.status(cfg)
            .into_iter()
            .map(|s| {
                let text = match (s.status.as_str(), &s.error) {
                    (MCP_CONNECTED, _) => format!("{MCP_CONNECTED} ({} tools)", s.tools.len()),
                    (MCP_FAILED, Some(e)) => format!("{MCP_FAILED}: {e}"),
                    (other, _) => other.to_string(),
                };
                (s.name, text)
            })
            .collect()
    }

    pub(crate) async fn close_all(&self) {
        let all: Vec<Arc<McpServer>> = lock(&self.servers).drain().map(|(_, v)| v).collect();
        for s in all {
            if let Some(c) = s.client()
                && tokio::time::timeout(CLOSE_TIMEOUT, c.close())
                    .await
                    .is_err()
            {
                tracing::warn!(server = %s.name, "MCP server did not close in {CLOSE_TIMEOUT:?}");
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
            server.ready.send_replace(true);
            tracing::info!(server = %server.name, "MCP server connected");
            let weak = Arc::downgrade(&client);
            drop(client);
            while rx.changed().await.is_ok() {
                let Some(c) = weak.upgrade() else { break };
                match c.list_tools().await {
                    Ok(t) => server.lock().tools = t,
                    Err(e) => {
                        tracing::warn!(server = %server.name, "MCP tools refresh failed: {e:#}")
                    }
                }
            }
        }
        Err(e) => {
            tracing::warn!(server = %server.name, "MCP connect failed: {e:#}");
            server.lock().status = McpStatus::Failed(format!("{e:#}"));
            server.ready.send_replace(true);
        }
    }
}
