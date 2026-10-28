//! Minimal MCP client: stdio (local) and Streamable HTTP with SSE fallback (remote).
//! Only the `tools` capability is used (tools/list, tools/call, notifications/tools/list_changed).

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
pub use ref_proto::spec::{McpServerConfig, ToolDef};
use ref_proto::{ImagePart, McpStatus};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

mod rpc;
mod sse;
mod transport;

use transport::{Conn, RawTool};

/// Default connect (spawn + initialize + tools/list) timeout.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_millis(30_000);
const MAX_NAME_LEN: usize = 64;
const MIN_RETRY_DELAY: Duration = Duration::from_secs(1);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(300);
/// A connection that lived at least this long resets the reconnect backoff.
const STABLE_CONNECTION: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
pub struct McpTool {
    pub server: String,
    /// Original MCP tool name.
    pub tool: String,
    /// Exposed name: `server_tool` sanitized to `[a-zA-Z0-9_-]`, max 64 chars.
    pub name: String,
    pub def: ToolDef,
}

#[derive(Debug, Clone)]
pub struct McpCallResult {
    pub text: String,
    pub images: Vec<ImagePart>,
    pub is_error: bool,
}

impl McpCallResult {
    fn error(text: impl Into<String>) -> Self {
        McpCallResult { text: text.into(), images: Vec::new(), is_error: true }
    }
}

/// Manages connections to all configured MCP servers. Cheap to share behind an `Arc`;
/// all methods take `&self`.
pub struct McpManager {
    servers: Mutex<BTreeMap<String, Arc<Server>>>,
    /// Serializes `restart` / `shutdown`.
    admin: tokio::sync::Mutex<()>,
}

impl McpManager {
    /// Connect to all enabled servers concurrently (each with its own timeout, default 30s).
    /// Failures are recorded in status, never returned as errors.
    pub async fn start(servers: BTreeMap<String, McpServerConfig>) -> Self {
        let map: BTreeMap<String, Arc<Server>> =
            servers.into_iter().map(|(name, cfg)| (name.clone(), Server::new(name, cfg))).collect();
        let to_start: Vec<Arc<Server>> = map.values().filter(|s| s.enabled()).cloned().collect();
        let mgr = McpManager { servers: Mutex::new(map), admin: tokio::sync::Mutex::new(()) };
        connect_all(to_start).await;
        mgr
    }

    /// Diff-restart: unchanged connected (or disabled) servers are kept, new/changed/failed
    /// servers are (re)started, removed servers are stopped. Waits for all connections.
    pub async fn restart(&self, servers: BTreeMap<String, McpServerConfig>) {
        let _admin = self.admin.lock().await;
        let mut stop = Vec::new();
        let mut start = Vec::new();
        {
            let mut cur = self.servers.lock();
            let mut next = BTreeMap::new();
            for (name, cfg) in servers {
                match cur.remove(&name) {
                    Some(old) if old.cfg == cfg && (!old.enabled() || old.is_alive_or_connecting()) => {
                        next.insert(name, old);
                    }
                    old => {
                        stop.extend(old);
                        let s = Server::new(name.clone(), cfg);
                        if s.enabled() {
                            start.push(s.clone());
                        }
                        next.insert(name, s);
                    }
                }
            }
            stop.extend(std::mem::take(&mut *cur).into_values());
            *cur = next;
        }
        futures::future::join_all(stop.iter().map(|s| s.stop())).await;
        connect_all(start).await;
    }

    /// All tools of connected servers (stable order: by server then tool name).
    ///
    /// Tools of a server whose connection died are still listed (calls trigger a reconnect).
    pub fn tools(&self) -> Vec<McpTool> {
        let servers = self.snapshot();
        let mut used = HashSet::new();
        let mut out = Vec::new();
        for s in &servers {
            let tools = s.state.lock().tools.clone();
            for t in tools {
                let name = unique_name(&s.name, &t.name, &mut used);
                out.push(McpTool {
                    server: s.name.clone(),
                    tool: t.name.clone(),
                    def: ToolDef { name: name.clone(), description: t.description, parameters: t.schema },
                    name,
                });
            }
        }
        out
    }

    pub fn status(&self) -> Vec<McpStatus> {
        self.snapshot().iter().map(|s| s.status()).collect()
    }

    /// Call a tool by exposed name. Never fails: errors become `is_error` results.
    pub async fn call(&self, name: &str, args: Value, abort: CancellationToken, timeout: Duration) -> McpCallResult {
        let Some((server, tool)) = self.resolve(name) else {
            return McpCallResult::error(format!("Unknown MCP tool: {name}"));
        };
        if abort.is_cancelled() {
            return McpCallResult::error("Tool call aborted");
        }
        let deadline = Instant::now() + timeout;
        let conn = match server.live_conn() {
            Some(c) => c,
            None => match server.reconnect(timeout.min(server.connect_timeout()), &abort).await {
                Ok(c) => c,
                Err(e) => {
                    return McpCallResult::error(format!("MCP server \"{}\" is unavailable: {e}", server.name));
                }
            },
        };
        let args = match args {
            Value::Null => json!({}),
            other => other,
        };
        let params = json!({"name": tool, "arguments": args});
        let remaining = deadline.saturating_duration_since(Instant::now());
        match conn.request("tools/call", params, remaining, &abort).await {
            Ok(v) => convert_result(&v),
            Err(rpc::RpcErr::Cancelled) => McpCallResult::error("Tool call aborted"),
            Err(e) => {
                if conn.core.is_closed() {
                    server.mark_dead(&conn);
                }
                let mut msg = format!("MCP tool \"{tool}\" (server \"{}\") failed: {e}", server.name);
                let tail = conn.stderr_tail();
                if conn.core.is_closed() && !tail.is_empty() && !msg.contains(&tail) {
                    msg.push_str(&format!("\nstderr: {tail}"));
                }
                McpCallResult::error(msg)
            }
        }
    }

    /// Kill local servers / close sessions. Servers stay listed (status `failed`, "shut down").
    pub async fn shutdown(&self) {
        let _admin = self.admin.lock().await;
        let servers = self.snapshot();
        futures::future::join_all(servers.iter().map(|s| s.stop())).await;
    }

    fn snapshot(&self) -> Vec<Arc<Server>> {
        self.servers.lock().values().cloned().collect()
    }

    fn resolve(&self, name: &str) -> Option<(Arc<Server>, String)> {
        let servers = self.snapshot();
        let mut used = HashSet::new();
        for s in &servers {
            let tools: Vec<String> = s.state.lock().tools.iter().map(|t| t.name.clone()).collect();
            for t in tools {
                if unique_name(&s.name, &t, &mut used) == name {
                    return Some((s.clone(), t));
                }
            }
        }
        None
    }
}

async fn connect_all(servers: Vec<Arc<Server>>) {
    futures::future::join_all(servers.into_iter().map(|s| async move {
        let _guard = s.connect_lock.lock().await;
        let timeout = s.connect_timeout();
        let _ = s.connect(timeout).await;
    }))
    .await;
}

// ───────────────────────────── per-server state ─────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Phase {
    Disabled,
    Connecting,
    Connected,
    Failed(String),
}

struct State {
    phase: Phase,
    conn: Option<Arc<Conn>>,
    /// Last known tool list (kept after a crash so the exposed tool list stays stable).
    tools: Vec<RawTool>,
    connected_at: Option<Instant>,
    /// Earliest time of the next reconnect attempt.
    retry_at: Option<Instant>,
    backoff: Duration,
    /// Set once the server was stopped (shutdown / removed); no more reconnects.
    stopped: bool,
    /// Incremented on every new connection; watchers of old connections ignore themselves.
    generation: u64,
}

struct Server {
    name: String,
    cfg: McpServerConfig,
    state: Mutex<State>,
    connect_lock: tokio::sync::Mutex<()>,
}

impl Server {
    fn new(name: String, cfg: McpServerConfig) -> Arc<Self> {
        let enabled = match &cfg {
            McpServerConfig::Local { enabled, .. } | McpServerConfig::Remote { enabled, .. } => enabled.unwrap_or(true),
        };
        Arc::new(Server {
            name,
            cfg,
            state: Mutex::new(State {
                phase: if enabled { Phase::Connecting } else { Phase::Disabled },
                conn: None,
                tools: Vec::new(),
                connected_at: None,
                retry_at: None,
                backoff: MIN_RETRY_DELAY,
                stopped: false,
                generation: 0,
            }),
            connect_lock: tokio::sync::Mutex::new(()),
        })
    }

    fn enabled(&self) -> bool {
        self.state.lock().phase != Phase::Disabled
    }

    fn connect_timeout(&self) -> Duration {
        let ms = match &self.cfg {
            McpServerConfig::Local { timeout, .. } | McpServerConfig::Remote { timeout, .. } => *timeout,
        };
        ms.filter(|ms| *ms > 0).map(Duration::from_millis).unwrap_or(DEFAULT_CONNECT_TIMEOUT)
    }

    fn is_alive_or_connecting(&self) -> bool {
        let st = self.state.lock();
        !st.stopped
            && match st.phase {
                Phase::Connecting => true,
                Phase::Connected => st.conn.as_ref().is_some_and(|c| !c.core.is_closed()),
                _ => false,
            }
    }

    fn status(&self) -> McpStatus {
        let st = self.state.lock();
        let (status, error) = match &st.phase {
            Phase::Disabled => ("disabled", None),
            Phase::Connecting => ("connecting", None),
            Phase::Connected => ("connected", None),
            Phase::Failed(e) => ("failed", Some(e.clone())),
        };
        let tools = if st.phase == Phase::Connected { st.tools.len() as u32 } else { 0 };
        McpStatus { name: self.name.clone(), status: status.to_string(), tools, error }
    }

    fn live_conn(&self) -> Option<Arc<Conn>> {
        let st = self.state.lock();
        st.conn.as_ref().filter(|c| !c.core.is_closed()).cloned()
    }

    /// Record that `conn` died (if it is still the current connection).
    fn mark_dead(&self, conn: &Arc<Conn>) {
        let mut st = self.state.lock();
        if st.conn.as_ref().is_some_and(|c| Arc::ptr_eq(c, conn)) {
            st.conn = None;
            if !st.stopped {
                st.phase = Phase::Failed(conn.closed_reason());
            }
            let stable = st.connected_at.is_some_and(|t| t.elapsed() >= STABLE_CONNECTION);
            if stable {
                st.backoff = MIN_RETRY_DELAY;
            }
            // First reconnect after a crash may happen immediately.
            st.retry_at = None;
        }
    }

    /// Initial connect (also used by reconnect). Records the outcome in state.
    async fn connect(self: &Arc<Self>, timeout: Duration) -> Result<Arc<Conn>, String> {
        {
            let mut st = self.state.lock();
            if st.stopped {
                return Err("server stopped".to_string());
            }
            st.phase = Phase::Connecting;
        }
        // Outer guard in case a transport step is not bounded by the handshake deadline.
        let res = match tokio::time::timeout(timeout + Duration::from_secs(1), transport::connect(&self.cfg, timeout)).await
        {
            Ok(r) => r,
            Err(_) => Err(format!("timed out after {:.1}s while connecting", timeout.as_secs_f64())),
        };
        let mut st = self.state.lock();
        match res {
            Ok((conn, tools)) => {
                if st.stopped {
                    drop(st);
                    conn.core.close("server stopped");
                    return Err("server stopped".to_string());
                }
                tracing::info!(
                    "mcp: connected to {} via {} ({} tools)",
                    self.name,
                    conn.transport_name(),
                    tools.len()
                );
                st.generation += 1;
                st.phase = Phase::Connected;
                st.conn = Some(conn.clone());
                st.tools = tools;
                st.connected_at = Some(Instant::now());
                st.retry_at = None;
                let generation = st.generation;
                drop(st);
                tokio::spawn(watch(Arc::downgrade(self), Arc::downgrade(&conn), conn.core.clone(), generation));
                Ok(conn)
            }
            Err(e) => {
                tracing::warn!("mcp: failed to connect to {}: {e}", self.name);
                if !st.stopped {
                    st.phase = Phase::Failed(e.clone());
                }
                st.retry_at = Some(Instant::now() + st.backoff);
                st.backoff = (st.backoff * 2).min(MAX_RETRY_DELAY);
                Err(e)
            }
        }
    }

    /// Lazy reconnect used by `call` after the connection died. Respects the backoff;
    /// concurrent callers share one attempt.
    async fn reconnect(self: &Arc<Self>, timeout: Duration, abort: &CancellationToken) -> Result<Arc<Conn>, String> {
        let _guard = tokio::select! {
            g = self.connect_lock.lock() => g,
            _ = abort.cancelled() => return Err("aborted".to_string()),
        };
        if let Some(c) = self.live_conn() {
            return Ok(c);
        }
        {
            let st = self.state.lock();
            if st.stopped {
                return Err("server stopped".to_string());
            }
            match &st.phase {
                Phase::Disabled => return Err("server disabled".to_string()),
                Phase::Failed(e) => {
                    if let Some(at) = st.retry_at.filter(|at| *at > Instant::now()) {
                        let secs = at.saturating_duration_since(Instant::now()).as_secs_f64();
                        return Err(format!("{e} (next reconnect attempt in {secs:.0}s)"));
                    }
                }
                _ => {}
            }
        }
        tokio::select! {
            r = self.connect(timeout) => r,
            _ = abort.cancelled() => Err("aborted".to_string()),
        }
    }

    async fn stop(&self) {
        let conn = {
            let mut st = self.state.lock();
            st.stopped = true;
            if st.phase != Phase::Disabled {
                st.phase = Phase::Failed("stopped".to_string());
            }
            st.tools.clear();
            st.conn.take()
        };
        if let Some(conn) = conn {
            conn.close("server stopped").await;
        }
    }
}

/// Per-connection background task: re-list tools on `list_changed`, mark the server failed
/// when the connection dies. Holds only weak references so dropping the manager closes
/// connections (and kills local servers).
async fn watch(server: Weak<Server>, conn: Weak<Conn>, core: Arc<rpc::Core>, generation: u64) {
    loop {
        tokio::select! {
            _ = core.closed.cancelled() => break,
            _ = core.list_changed.notified() => {
                let (Some(s), Some(conn)) = (server.upgrade(), conn.upgrade()) else { return };
                match transport::list_tools(&conn, s.connect_timeout()).await {
                    Ok(tools) => {
                        let mut st = s.state.lock();
                        if st.generation == generation {
                            tracing::info!("mcp: {} tool list changed ({} tools)", s.name, tools.len());
                            st.tools = tools;
                        }
                    }
                    Err(e) => tracing::warn!("mcp: {}: failed to re-list tools: {e}", s.name),
                }
            }
        }
    }
    if let (Some(s), Some(conn)) = (server.upgrade(), conn.upgrade()) {
        tracing::warn!("mcp: {} disconnected: {}", s.name, conn.closed_reason());
        s.mark_dead(&conn);
    }
}

// ───────────────────────────── helpers ─────────────────────────────

fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}

/// Exposed tool name, unique within `used` (which it is added to).
fn unique_name(server: &str, tool: &str, used: &mut HashSet<String>) -> String {
    let mut base = format!("{}_{}", sanitize(server), sanitize(tool));
    base.truncate(MAX_NAME_LEN);
    if used.insert(base.clone()) {
        return base;
    }
    for n in 2u32.. {
        let suffix = format!("_{n}");
        let mut name = base.clone();
        name.truncate(MAX_NAME_LEN - suffix.len());
        name.push_str(&suffix);
        if used.insert(name.clone()) {
            return name;
        }
    }
    unreachable!("u32 suffix space exhausted")
}

fn convert_result(v: &Value) -> McpCallResult {
    let mut texts: Vec<String> = Vec::new();
    let mut images = Vec::new();
    for item in v.get("content").and_then(Value::as_array).into_iter().flatten() {
        let str_of = |o: &Value, k: &str| o.get(k).and_then(Value::as_str).map(str::to_string);
        match item.get("type").and_then(Value::as_str).unwrap_or("") {
            "text" => texts.push(str_of(item, "text").unwrap_or_default()),
            "image" => match str_of(item, "data") {
                Some(data) => images.push(ImagePart {
                    id: ref_proto::new_id("prt"),
                    mime: str_of(item, "mimeType").unwrap_or_else(|| "image/png".to_string()),
                    data,
                    name: None,
                }),
                None => texts.push("[image without data]".to_string()),
            },
            "audio" => texts.push("[audio omitted]".to_string()),
            "resource" => {
                let res = item.get("resource").unwrap_or(&Value::Null);
                match str_of(res, "text") {
                    Some(t) => texts.push(t),
                    None => texts.push(format!("[resource {}]", str_of(res, "uri").unwrap_or_default())),
                }
            }
            "resource_link" => texts.push(format!("[resource {}]", str_of(item, "uri").unwrap_or_default())),
            _ => texts.push(item.to_string()),
        }
    }
    let mut text = texts.join("\n");
    if text.trim().is_empty() {
        if let Some(sc) = v.get("structuredContent").filter(|s| !s.is_null()) {
            text = serde_json::to_string_pretty(sc).unwrap_or_else(|_| sc.to_string());
        }
    }
    let is_error = v.get("isError").and_then(Value::as_bool).unwrap_or(false);
    McpCallResult { text, images, is_error }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn names() {
        let mut used = HashSet::new();
        assert_eq!(unique_name("my server", "do.it", &mut used), "my_server_do_it");
        assert_eq!(unique_name("my.server", "do it", &mut used), "my_server_do_it_2");
        let long = "x".repeat(100);
        let n = unique_name("s", &long, &mut used);
        assert_eq!(n.len(), 64);
        let n2 = unique_name("s", &long, &mut used);
        assert_eq!(n2.len(), 64);
        assert!(n2.ends_with("_2"));
        assert_eq!(unique_name("文件", "a", &mut used), "___a");
    }

    #[test]
    fn convert() {
        let r = convert_result(&json!({
            "content": [
                {"type": "text", "text": "a"},
                {"type": "image", "data": "AAAA", "mimeType": "image/jpeg"},
                {"type": "resource", "resource": {"uri": "file:///x", "text": "body"}},
                {"type": "resource", "resource": {"uri": "file:///y", "blob": "AA"}},
                {"type": "audio", "data": "AA", "mimeType": "audio/wav"}
            ],
            "isError": true
        }));
        assert_eq!(r.text, "a\nbody\n[resource file:///y]\n[audio omitted]");
        assert_eq!(r.images.len(), 1);
        assert_eq!(r.images[0].mime, "image/jpeg");
        assert!(r.images[0].id.starts_with("prt_"));
        assert!(r.is_error);
        let r = convert_result(&json!({"content": [], "structuredContent": {"k": 1}}));
        assert_eq!(r.text, "{\n  \"k\": 1\n}");
        assert!(!r.is_error);
    }
}
