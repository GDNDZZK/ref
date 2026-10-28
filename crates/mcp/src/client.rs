use crate::http::{HttpStatusError, LegacySse, Streamable};
use crate::rpc::{Rpc, RpcError, Transport, METHOD_NOT_FOUND};
use crate::stdio::Stdio;
use crate::*;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::json;
use std::sync::{RwLock, Weak};
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

pub const PROTOCOL_VERSION: &str = "2025-06-18";
pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;
const MAX_LIST_PAGES: usize = 100;

pub struct McpClient {
    /// Server name as configured.
    pub name: String,
    /// Server-provided `instructions` from initialize.
    pub instructions: Option<String>,
    /// `serverInfo` from initialize (`{name, version, ...}`), if any.
    pub server_info: Option<Value>,
    /// Protocol version negotiated with the server.
    pub protocol_version: String,
    tools: RwLock<Vec<McpTool>>,
    tools_gen: watch::Sender<u64>,
    list_timeout: Duration,
    rpc: Arc<Rpc>,
}

impl McpClient {
    /// Current tool list (refreshed on `notifications/tools/list_changed`).
    pub fn tools(&self) -> Vec<McpTool> {
        self.tools.read().unwrap().clone()
    }

    /// Generation counter bumped every time the tool list is refreshed.
    pub fn tools_changed(&self) -> watch::Receiver<u64> {
        self.tools_gen.subscribe()
    }

    /// Transport in use: "stdio" | "streamable-http" | "sse".
    pub fn transport(&self) -> &'static str {
        self.rpc.transport.kind()
    }

    /// Why the connection is closed, or None while it is alive.
    pub fn closed_reason(&self) -> Option<String> {
        self.rpc.closed_reason()
    }

    /// Re-run tools/list and replace the cached tool list.
    pub async fn refresh_tools(&self) -> Result<Vec<McpTool>> {
        let tools = list_tools(&self.rpc, &self.name, self.list_timeout).await?;
        *self.tools.write().unwrap() = tools.clone();
        self.tools_gen.send_modify(|g| *g += 1);
        Ok(tools)
    }

    /// Call a tool. `tool` may be the server's original tool name or the exposed
    /// (`server_tool`) name. No timeout; cancelling `cancel` aborts the call and sends
    /// `notifications/cancelled` to the server. JSON-RPC / transport failures are `Err`;
    /// tool-level failures (`isError: true`) are `Ok` with `is_error` set.
    pub async fn call(&self, tool: &str, args: Value, cancel: &CancellationToken) -> Result<McpCallResult> {
        let original = {
            let tools = self.tools.read().unwrap();
            if tools.iter().any(|t| t.tool == tool) {
                tool.to_string()
            } else {
                tools.iter().find(|t| t.name == tool).map(|t| t.tool.clone()).unwrap_or_else(|| tool.to_string())
            }
        };
        let arguments = match args {
            Value::Null => json!({}),
            Value::Object(_) => args,
            other => bail!("tool arguments must be a JSON object, got {}", type_name(&other)),
        };
        let result = self
            .rpc
            .request("tools/call", json!({"name": original, "arguments": arguments}), Some(cancel), None)
            .await
            .with_context(|| format!("MCP server `{}`: tools/call `{original}` failed", self.name))?;
        Ok(parse_call_result(&result))
    }

    /// Close the connection (kills a local server's process group).
    pub async fn shutdown(&self) {
        self.rpc.close_with("client shut down");
        self.rpc.transport.close().await;
    }
}

impl std::fmt::Debug for McpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpClient")
            .field("name", &self.name)
            .field("transport", &self.transport())
            .field("tools", &self.tools.read().unwrap().len())
            .finish_non_exhaustive()
    }
}

pub(crate) async fn connect(name: &str, cfg: &McpConfig, cwd: &std::path::Path) -> Result<Arc<McpClient>> {
    if !cfg.enabled() {
        bail!("MCP server `{name}` is disabled");
    }
    let timeout = Duration::from_millis(cfg.timeout().filter(|t| *t > 0).unwrap_or(DEFAULT_TIMEOUT_MS));
    let fut = connect_inner(name, cfg, cwd, timeout);
    match tokio::time::timeout(timeout, fut).await {
        Ok(r) => r.with_context(|| format!("MCP server `{name}`: connection failed")),
        Err(_) => bail!("MCP server `{name}`: connect/initialize timed out after {}ms", timeout.as_millis()),
    }
}

async fn connect_inner(name: &str, cfg: &McpConfig, cwd: &std::path::Path, timeout: Duration) -> Result<Arc<McpClient>> {
    match cfg {
        McpConfig::Local { command, environment, cwd: dir, .. } => {
            let dir = match dir {
                Some(d) => cwd.join(expand_home(d)),
                None => cwd.to_path_buf(),
            };
            let (tx, rx) = mpsc::unbounded_channel();
            let t = Stdio::spawn(name, command, environment, &dir, tx)?;
            let (rpc, notif) = Rpc::start(name, Transport::Stdio(t), rx);
            handshake(name, rpc, notif, timeout).await
        }
        McpConfig::Remote { url, headers, .. } => {
            let (tx, rx) = mpsc::unbounded_channel();
            let t = Streamable::new(name, url, headers, tx)?;
            let (rpc, notif) = Rpc::start(name, Transport::Http(t), rx);
            let err = match handshake(name, rpc.clone(), notif, timeout).await {
                Ok(c) => return Ok(c),
                Err(e) => e,
            };
            rpc.transport.close().await;
            let fallback = err
                .chain()
                .find_map(|e| e.downcast_ref::<HttpStatusError>())
                .is_some_and(|h| h.suggests_sse_fallback());
            if !fallback {
                return Err(err);
            }
            tracing::debug!(server = %name, "Streamable HTTP failed ({err:#}); trying legacy SSE");
            let (tx, rx) = mpsc::unbounded_channel();
            let sse = LegacySse::connect(name, url, headers, tx)
                .await
                .map_err(|e2| anyhow!("Streamable HTTP: {err:#}; SSE fallback: {e2:#}"))?;
            let (rpc, notif) = Rpc::start(name, Transport::Sse(sse), rx);
            handshake(name, rpc, notif, timeout)
                .await
                .map_err(|e2| anyhow!("Streamable HTTP: {err:#}; SSE fallback: {e2:#}"))
        }
    }
}

async fn handshake(
    name: &str,
    rpc: Arc<Rpc>,
    notif: mpsc::UnboundedReceiver<Value>,
    timeout: Duration,
) -> Result<Arc<McpClient>> {
    let params = json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": {},
        "clientInfo": {"name": "hernees", "version": env!("CARGO_PKG_VERSION")},
    });
    let init = rpc.request("initialize", params, None, Some(timeout)).await.context("initialize failed")?;
    if !init.is_object() {
        bail!("initialize returned a non-object result: {}", rpc::truncate(&init.to_string(), 200));
    }
    let protocol_version = init
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(PROTOCOL_VERSION)
        .to_string();
    if let Transport::Http(h) = &rpc.transport {
        h.set_protocol_version(&protocol_version);
    }
    rpc.notify("notifications/initialized", None).await.context("notifications/initialized failed")?;
    let instructions = init
        .get("instructions")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let tools = list_tools(&rpc, name, timeout).await?;
    let client = Arc::new(McpClient {
        name: name.to_string(),
        instructions,
        server_info: init.get("serverInfo").cloned(),
        protocol_version,
        tools: RwLock::new(tools),
        tools_gen: watch::channel(0).0,
        list_timeout: timeout,
        rpc,
    });
    tokio::spawn(notification_loop(Arc::downgrade(&client), notif));
    Ok(client)
}

async fn notification_loop(weak: Weak<McpClient>, mut rx: mpsc::UnboundedReceiver<Value>) {
    while let Some(n) = rx.recv().await {
        let method = n.get("method").and_then(Value::as_str).unwrap_or("");
        match method {
            "notifications/tools/list_changed" => {
                // Coalesce bursts of list_changed notifications.
                while let Ok(n2) = rx.try_recv() {
                    if n2.get("method").and_then(Value::as_str) != Some("notifications/tools/list_changed") {
                        log_notification(&weak, &n2);
                    }
                }
                let Some(client) = weak.upgrade() else { return };
                match client.refresh_tools().await {
                    Ok(t) => tracing::debug!(server = %client.name, "tools list changed: {} tools", t.len()),
                    Err(e) => tracing::warn!(server = %client.name, "failed to refresh tools: {e:#}"),
                }
            }
            _ => log_notification(&weak, &n),
        }
    }
}

fn log_notification(weak: &Weak<McpClient>, n: &Value) {
    let server = weak.upgrade().map(|c| c.name.clone()).unwrap_or_default();
    let method = n.get("method").and_then(Value::as_str).unwrap_or("");
    if method == "notifications/message" {
        let p = n.get("params").cloned().unwrap_or(Value::Null);
        let level = p.get("level").and_then(Value::as_str).unwrap_or("info");
        let data = p.get("data").map(|d| d.to_string()).unwrap_or_default();
        tracing::debug!(server = %server, "log [{level}]: {}", rpc::truncate(&data, 500));
    } else {
        tracing::trace!(server = %server, "notification `{method}`");
    }
}

async fn list_tools(rpc: &Arc<Rpc>, server: &str, timeout: Duration) -> Result<Vec<McpTool>> {
    let mut tools = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..MAX_LIST_PAGES {
        let params = match &cursor {
            Some(c) => json!({"cursor": c}),
            None => json!({}),
        };
        let res = match rpc.request("tools/list", params, None, Some(timeout)).await {
            Ok(r) => r,
            Err(e) if e.downcast_ref::<RpcError>().is_some_and(|r| r.code == METHOD_NOT_FOUND) => {
                return Ok(tools);
            }
            Err(e) => return Err(e.context("tools/list failed")),
        };
        for t in res.get("tools").and_then(Value::as_array).into_iter().flatten() {
            match parse_tool(server, t) {
                Some(t) => tools.push(t),
                None => tracing::debug!(server = %server, "skipping malformed tool: {}", rpc::truncate(&t.to_string(), 200)),
            }
        }
        cursor = res.get("nextCursor").and_then(Value::as_str).filter(|c| !c.is_empty()).map(str::to_string);
        if cursor.is_none() {
            return Ok(tools);
        }
    }
    tracing::warn!(server = %server, "tools/list pagination exceeded {MAX_LIST_PAGES} pages; truncating");
    Ok(tools)
}

pub(crate) fn parse_tool(server: &str, t: &Value) -> Option<McpTool> {
    let tool = t.get("name")?.as_str()?.to_string();
    if tool.is_empty() {
        return None;
    }
    let str_field = |v: Option<&Value>| v.and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let description = str_field(t.get("description"))
        .or_else(|| str_field(t.get("title")))
        .or_else(|| str_field(t.get("annotations").and_then(|a| a.get("title"))))
        .unwrap_or_default();
    let mut schema = match t.get("inputSchema") {
        Some(Value::Object(o)) => Value::Object(o.clone()),
        _ => json!({"type": "object", "properties": {}}),
    };
    if let Some(o) = schema.as_object_mut() {
        o.entry("type").or_insert_with(|| json!("object"));
        if o.get("type").and_then(Value::as_str) == Some("object") {
            o.entry("properties").or_insert_with(|| json!({}));
        }
    }
    Some(McpTool { server: server.to_string(), name: format!("{}_{}", sanitize(server), sanitize(&tool)), tool, description, schema })
}

pub(crate) fn parse_call_result(r: &Value) -> McpCallResult {
    let mut texts: Vec<String> = Vec::new();
    let mut images = Vec::new();
    for c in r.get("content").and_then(Value::as_array).into_iter().flatten() {
        let s = |k: &str| c.get(k).and_then(Value::as_str);
        match s("type").unwrap_or("") {
            "text" => texts.push(s("text").unwrap_or("").to_string()),
            "image" => match (s("data"), s("mimeType")) {
                (Some(data), mime) => images.push(hn_proto::ImageData {
                    mime: mime.unwrap_or("image/png").to_string(),
                    data: strip_data_url(data).to_string(),
                }),
                (None, _) => texts.push("[image without data]".into()),
            },
            "audio" => texts.push(format!("[audio: {}]", s("mimeType").unwrap_or("unknown type"))),
            "resource" => {
                let res = c.get("resource").cloned().unwrap_or(Value::Null);
                let rs = |k: &str| res.get(k).and_then(Value::as_str);
                if let Some(t) = rs("text") {
                    texts.push(t.to_string());
                } else if let (Some(blob), Some(mime)) = (rs("blob"), rs("mimeType").filter(|m| m.starts_with("image/"))) {
                    images.push(hn_proto::ImageData { mime: mime.to_string(), data: blob.to_string() });
                } else {
                    texts.push(format!(
                        "[resource: {}{}]",
                        rs("uri").unwrap_or("?"),
                        rs("mimeType").map(|m| format!(" ({m})")).unwrap_or_default()
                    ));
                }
            }
            "resource_link" => texts.push(format!(
                "[resource link: {} {}]",
                s("name").or(s("title")).unwrap_or(""),
                s("uri").unwrap_or("?")
            )),
            _ => texts.push(c.to_string()),
        }
    }
    let mut text = texts.join("\n");
    if texts.is_empty() && images.is_empty() {
        if let Some(sc) = r.get("structuredContent").filter(|v| !v.is_null()) {
            text = serde_json::to_string_pretty(sc).unwrap_or_default();
        }
    }
    McpCallResult { text, images, is_error: r.get("isError").and_then(Value::as_bool).unwrap_or(false) }
}

fn strip_data_url(s: &str) -> &str {
    match s.strip_prefix("data:").and_then(|r| r.split_once(";base64,")) {
        Some((_, d)) => d,
        None => s,
    }
}

fn expand_home(p: &str) -> std::path::PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return std::path::PathBuf::from(home).join(rest);
        }
    }
    std::path::PathBuf::from(p)
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_parsing() {
        let t = parse_tool("my server", &json!({"name": "do.it", "title": "Do it"})).unwrap();
        assert_eq!(t.name, "my_server_do_it");
        assert_eq!(t.tool, "do.it");
        assert_eq!(t.description, "Do it");
        assert_eq!(t.schema, json!({"type": "object", "properties": {}}));
        assert!(parse_tool("s", &json!({"description": "x"})).is_none());
        let t = parse_tool("s", &json!({"name": "a", "description": "d", "inputSchema": {"type": "object", "properties": {"q": {"type": "string"}}, "required": ["q"]}})).unwrap();
        assert_eq!(t.schema["required"], json!(["q"]));
    }

    #[test]
    fn call_result_parsing() {
        let r = parse_call_result(&json!({
            "content": [
                {"type": "text", "text": "a"},
                {"type": "image", "data": "QUJD", "mimeType": "image/jpeg"},
                {"type": "resource", "resource": {"uri": "file:///x", "text": "b"}},
                {"type": "resource", "resource": {"uri": "file:///y", "blob": "zz", "mimeType": "application/pdf"}},
            ],
            "isError": true
        }));
        assert_eq!(r.text, "a\nb\n[resource: file:///y (application/pdf)]");
        assert_eq!(r.images.len(), 1);
        assert_eq!(r.images[0].mime, "image/jpeg");
        assert!(r.is_error);
        let r = parse_call_result(&json!({"content": [], "structuredContent": {"x": 1}}));
        assert!(r.text.contains("\"x\": 1"));
        assert!(!r.is_error);
        let r = parse_call_result(&json!("garbage"));
        assert_eq!(r.text, "");
    }
}
