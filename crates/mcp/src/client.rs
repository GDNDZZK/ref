//! JSON-RPC 2.0 core (request correlation, server->client requests, notifications)
//! and the public [`McpClient`].

use crate::http::HttpTransport;
use crate::stdio::StdioTransport;
use crate::{CallResult, McpTool, ServerConfig, Transport};
use anyhow::{Result, anyhow, bail};
use ref_proto::ImageData;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;

pub(crate) const PROTOCOL_VERSION: &str = "2025-06-18";

type Reply = oneshot::Sender<Result<Value>>;

/// State shared between the client and its transport's reader tasks.
pub(crate) struct Shared {
    pub(crate) name: String,
    pending: std::sync::Mutex<HashMap<String, Reply>>,
    tools_tx: watch::Sender<u64>,
    /// Outgoing messages produced asynchronously (responses to server requests,
    /// cancellation notifications). Drained by the client's writer task.
    out_tx: mpsc::UnboundedSender<Value>,
}

/// Correlation key for a JSON-RPC id (number or string).
pub(crate) fn id_key(id: &Value) -> String {
    match id {
        Value::String(s) => format!("s:{s}"),
        Value::Number(n) => match n.as_i64() {
            Some(i) => format!("n:{i}"),
            None => format!("n:{n}"),
        },
        other => format!("o:{other}"),
    }
}

/// Convert a JSON-RPC error object into an anyhow error.
pub(crate) fn rpc_error(err: &Value) -> anyhow::Error {
    let code = err.get("code").and_then(Value::as_i64).unwrap_or(0);
    let message = err
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("unknown error");
    anyhow!("MCP error {code}: {message}")
}

/// True if `msg` (single message or batch) contains the response for `id`.
pub(crate) fn contains_response(msg: &Value, id: &Value) -> bool {
    match msg {
        Value::Array(items) => items.iter().any(|m| contains_response(m, id)),
        Value::Object(o) => {
            o.get("method").is_none()
                && o.get("id").is_some_and(|i| id_key(i) == id_key(id))
                && (o.contains_key("result") || o.contains_key("error"))
        }
        _ => false,
    }
}

impl Shared {
    pub(crate) fn new(
        name: String,
        out_tx: mpsc::UnboundedSender<Value>,
    ) -> (Arc<Self>, watch::Receiver<u64>) {
        let (tools_tx, tools_rx) = watch::channel(0u64);
        let shared = Arc::new(Shared {
            name,
            pending: Default::default(),
            tools_tx,
            out_tx,
        });
        (shared, tools_rx)
    }

    fn register(&self, id: &Value) -> oneshot::Receiver<Result<Value>> {
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id_key(id), tx);
        rx
    }

    fn unregister(&self, id: &Value) {
        self.pending.lock().unwrap().remove(&id_key(id));
    }

    /// Fail one pending request (no-op if it already completed).
    pub(crate) fn fail_pending(&self, id: &Value, err: &str) {
        if let Some(tx) = self.pending.lock().unwrap().remove(&id_key(id)) {
            let _ = tx.send(Err(anyhow!("{err}")));
        }
    }

    /// Fail every pending request (transport closed).
    pub(crate) fn fail_all(&self, err: &str) {
        let drained: Vec<Reply> = self
            .pending
            .lock()
            .unwrap()
            .drain()
            .map(|(_, v)| v)
            .collect();
        for tx in drained {
            let _ = tx.send(Err(anyhow!("{err}")));
        }
    }

    pub(crate) fn send_async(&self, msg: Value) {
        let _ = self.out_tx.send(msg);
    }

    /// Handle one incoming message (or batch) from the server.
    pub(crate) fn dispatch(&self, msg: Value) {
        let obj = match msg {
            Value::Array(items) => {
                for m in items {
                    self.dispatch(m);
                }
                return;
            }
            Value::Object(o) => o,
            other => {
                tracing::debug!(server = %self.name, "ignoring non-object JSON-RPC message: {other}");
                return;
            }
        };
        let id = obj.get("id").filter(|v| !v.is_null()).cloned();
        match (obj.get("method").and_then(Value::as_str), id) {
            (Some(method), Some(id)) => self.handle_server_request(method, id),
            (Some(method), None) => self.handle_notification(method, obj.get("params")),
            (None, Some(id)) => {
                let Some(tx) = self.pending.lock().unwrap().remove(&id_key(&id)) else {
                    tracing::debug!(server = %self.name, "response for unknown id {id}");
                    return;
                };
                let res = if let Some(err) = obj.get("error") {
                    Err(rpc_error(err))
                } else {
                    Ok(obj.get("result").cloned().unwrap_or(Value::Null))
                };
                let _ = tx.send(res);
            }
            (None, None) => {
                tracing::debug!(server = %self.name, "ignoring malformed JSON-RPC message")
            }
        }
    }

    fn handle_server_request(&self, method: &str, id: Value) {
        let reply = match method {
            "ping" => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
            "roots/list" => json!({"jsonrpc": "2.0", "id": id, "result": {"roots": []}}),
            _ => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": format!("Method not found: {method}")}
            }),
        };
        self.send_async(reply);
    }

    fn handle_notification(&self, method: &str, params: Option<&Value>) {
        match method {
            "notifications/tools/list_changed" => self.tools_tx.send_modify(|v| *v += 1),
            "notifications/message" => {
                let p = params.map(|p| p.to_string()).unwrap_or_default();
                tracing::debug!(server = %self.name, "server log: {p}")
            }
            _ => tracing::trace!(server = %self.name, "notification {method}"),
        }
    }
}

pub(crate) enum Conn {
    Stdio(Box<StdioTransport>),
    Http(Box<HttpTransport>),
}

impl Conn {
    async fn send(&self, msg: &Value) -> Result<()> {
        match self {
            Conn::Stdio(t) => t.send(msg).await,
            Conn::Http(t) => t.send(msg).await,
        }
    }

    fn set_protocol_version(&self, v: &str) {
        if let Conn::Http(t) = self {
            t.set_protocol_version(v);
        }
    }

    fn after_initialized(&self) {
        if let Conn::Http(t) = self {
            t.start_get_stream();
        }
    }

    async fn close(&self) {
        match self {
            Conn::Stdio(t) => t.close().await,
            Conn::Http(t) => t.close().await,
        }
    }
}

/// Connected MCP client. Cheap to share behind an `Arc`; all methods take `&self`.
pub struct McpClient {
    name: String,
    shared: Arc<Shared>,
    conn: Arc<Conn>,
    next_id: AtomicI64,
    timeout: Duration,
    instructions: Option<String>,
    tools_rx: watch::Receiver<u64>,
    writer: tokio::task::JoinHandle<()>,
    closed: AtomicBool,
}

/// Removes a pending entry if the request future is dropped early.
struct PendingGuard<'a> {
    shared: &'a Shared,
    id: Value,
}

impl Drop for PendingGuard<'_> {
    fn drop(&mut self) {
        self.shared.unregister(&self.id);
    }
}

impl McpClient {
    /// Start the transport, perform the initialize handshake and return a ready client.
    pub async fn connect(cfg: &ServerConfig) -> Result<McpClient> {
        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Value>();
        let (shared, tools_rx) = Shared::new(cfg.name.clone(), out_tx);
        let conn =
            match &cfg.transport {
                Transport::Local { command, env, cwd } => Conn::Stdio(Box::new(
                    StdioTransport::spawn(&cfg.name, command, env, cwd.as_deref(), shared.clone())?,
                )),
                Transport::Remote { url, headers } => Conn::Http(Box::new(HttpTransport::new(
                    url,
                    headers,
                    shared.clone(),
                    cfg.timeout,
                )?)),
            };
        let conn = Arc::new(conn);
        let writer = {
            let conn = conn.clone();
            let name = cfg.name.clone();
            tokio::spawn(async move {
                while let Some(msg) = out_rx.recv().await {
                    let conn = conn.clone();
                    let name = name.clone();
                    tokio::spawn(async move {
                        if let Err(e) = conn.send(&msg).await {
                            tracing::debug!(server = %name, "failed to send message: {e:#}");
                        }
                    });
                }
            })
        };
        let mut client = McpClient {
            name: cfg.name.clone(),
            shared,
            conn,
            next_id: AtomicI64::new(1),
            timeout: cfg.timeout,
            instructions: None,
            tools_rx,
            writer,
            closed: AtomicBool::new(false),
        };
        match client.initialize().await {
            Ok(()) => Ok(client),
            Err(e) => {
                client.close().await;
                Err(e.context(format!("MCP server \"{}\": initialize failed", cfg.name)))
            }
        }
    }

    async fn initialize(&mut self) -> Result<()> {
        let params = json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {"roots": {"listChanged": false}},
            "clientInfo": {"name": "ref", "version": env!("CARGO_PKG_VERSION")},
        });
        let res = self
            .request("initialize", params, Some(self.timeout), None)
            .await?;
        let version = res
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or(PROTOCOL_VERSION);
        self.conn.set_protocol_version(version);
        self.instructions = res
            .get("instructions")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from);
        let note = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        tokio::time::timeout(self.timeout, self.conn.send(&note))
            .await
            .map_err(|_| anyhow!("timed out sending notifications/initialized"))??;
        self.conn.after_initialized();
        Ok(())
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn instructions(&self) -> Option<String> {
        self.instructions.clone()
    }

    /// Receiver whose value increments on every `notifications/tools/list_changed`.
    pub fn tools_changed(&self) -> watch::Receiver<u64> {
        self.tools_rx.clone()
    }

    /// Send a request and await its response.
    async fn request(
        &self,
        method: &str,
        params: Value,
        timeout: Option<Duration>,
        cancel: Option<&CancellationToken>,
    ) -> Result<Value> {
        if self.closed.load(Ordering::SeqCst) {
            bail!("MCP server \"{}\" is closed", self.name);
        }
        let id = json!(self.next_id.fetch_add(1, Ordering::SeqCst));
        let rx = self.shared.register(&id);
        let _guard = PendingGuard {
            shared: &self.shared,
            id: id.clone(),
        };
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let fut = async {
            self.conn.send(&msg).await?;
            rx.await.map_err(|_| anyhow!("MCP connection closed"))?
        };
        let fut = async {
            match timeout {
                Some(t) => match tokio::time::timeout(t, fut).await {
                    Ok(r) => r,
                    Err(_) => {
                        // The spec forbids cancelling initialize.
                        if method != "initialize" {
                            self.send_cancelled(&id, "timeout");
                        }
                        Err(anyhow!(
                            "MCP request {method} timed out after {}s",
                            t.as_secs_f64()
                        ))
                    }
                },
                None => fut.await,
            }
        };
        match cancel {
            Some(cancel) => tokio::select! {
                r = fut => r,
                _ = cancel.cancelled() => {
                    self.send_cancelled(&id, "cancelled by user");
                    Err(anyhow!("cancelled"))
                }
            },
            None => fut.await,
        }
    }

    fn send_cancelled(&self, id: &Value, reason: &str) {
        self.shared.send_async(json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": {"requestId": id, "reason": reason},
        }));
    }

    /// List all tools (follows `nextCursor` pagination).
    pub async fn list_tools(&self) -> Result<Vec<McpTool>> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        for _page in 0..1000 {
            let params = match &cursor {
                Some(c) => json!({"cursor": c}),
                None => json!({}),
            };
            let res = self
                .request("tools/list", params, Some(self.timeout), None)
                .await?;
            for t in res
                .get("tools")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                match parse_tool(t) {
                    Some(tool) => tools.push(tool),
                    None => {
                        tracing::warn!(server = %self.name, "skipping invalid tool definition: {t}")
                    }
                }
            }
            let next = res
                .get("nextCursor")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty());
            match next {
                Some(n) if cursor.as_deref() != Some(n) => cursor = Some(n.to_string()),
                _ => return Ok(tools),
            }
        }
        Ok(tools)
    }

    /// Call a tool. No timeout; `cancel` aborts the call and notifies the server.
    pub async fn call_tool(
        &self,
        name: &str,
        args: Value,
        cancel: &CancellationToken,
    ) -> Result<CallResult> {
        let args = if args.is_null() { json!({}) } else { args };
        let res = self
            .request(
                "tools/call",
                json!({"name": name, "arguments": args}),
                None,
                Some(cancel),
            )
            .await?;
        Ok(parse_call_result(&res))
    }

    /// Shut down the transport. Pending requests fail.
    pub async fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        self.conn.close().await;
        self.shared.fail_all("MCP client closed");
        self.writer.abort();
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        // The writer task holds an Arc<Conn>; abort it so the transport (and child
        // process, via kill_on_drop) is released.
        self.writer.abort();
    }
}

fn parse_tool(t: &Value) -> Option<McpTool> {
    let name = t.get("name")?.as_str()?.to_string();
    if name.is_empty() {
        return None;
    }
    let description = t
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let input_schema = match t.get("inputSchema") {
        Some(s @ Value::Object(_)) => s.clone(),
        _ => json!({"type": "object", "properties": {}}),
    };
    Some(McpTool {
        name,
        description,
        input_schema,
    })
}

pub(crate) fn parse_call_result(res: &Value) -> CallResult {
    let mut texts: Vec<String> = Vec::new();
    let mut images = Vec::new();
    for item in res
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let str_field = |k: &str| item.get(k).and_then(Value::as_str);
        match str_field("type").unwrap_or("") {
            "text" => texts.push(str_field("text").unwrap_or_default().to_string()),
            "image" => images.push(ImageData {
                mime: str_field("mimeType").unwrap_or("image/png").to_string(),
                data: str_field("data").unwrap_or_default().to_string(),
                ..Default::default()
            }),
            "resource" => {
                let r = item.get("resource").cloned().unwrap_or(Value::Null);
                let uri = r.get("uri").and_then(Value::as_str).unwrap_or("");
                let mime = r
                    .get("mimeType")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown type");
                match r.get("text").and_then(Value::as_str) {
                    Some(text) => texts.push(text.to_string()),
                    None => texts.push(format!("[Binary resource {uri} ({mime}) omitted]")),
                }
            }
            "resource_link" => {
                let uri = str_field("uri").unwrap_or("");
                match str_field("name") {
                    Some(n) => texts.push(format!("[Resource link: {n} <{uri}>]")),
                    None => texts.push(format!("[Resource link: {uri}]")),
                }
            }
            "audio" => {
                let mime = str_field("mimeType").unwrap_or("audio");
                texts.push(format!("[Audio content ({mime}) omitted]"));
            }
            other => texts.push(format!("[Unsupported content type: {other}]")),
        }
    }
    let mut text = texts.join("\n");
    if text.is_empty()
        && let Some(sc) = res.get("structuredContent").filter(|v| !v.is_null())
    {
        text = serde_json::to_string(sc).unwrap_or_default();
    }
    let is_error = res.get("isError").and_then(Value::as_bool).unwrap_or(false);
    CallResult {
        text,
        images,
        is_error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_result_parsing() {
        let r = parse_call_result(&json!({
            "content": [
                {"type": "text", "text": "a"},
                {"type": "image", "mimeType": "image/jpeg", "data": "QUJD"},
                {"type": "resource", "resource": {"uri": "file:///x", "text": "res"}},
                {"type": "audio", "mimeType": "audio/wav", "data": ""},
            ],
            "isError": true
        }));
        assert_eq!(r.text, "a\nres\n[Audio content (audio/wav) omitted]");
        assert_eq!(r.images.len(), 1);
        assert_eq!(r.images[0].mime, "image/jpeg");
        assert!(r.is_error);
        let r = parse_call_result(&json!({"content": [], "structuredContent": {"a": 1}}));
        assert_eq!(r.text, r#"{"a":1}"#);
        assert!(!r.is_error);
    }

    #[test]
    fn response_detection() {
        let id = json!(3);
        assert!(contains_response(
            &json!({"jsonrpc":"2.0","id":3,"result":{}}),
            &id
        ));
        assert!(contains_response(
            &json!([{"method":"x"},{"id":3,"error":{}}]),
            &id
        ));
        assert!(!contains_response(&json!({"id":3,"method":"ping"}), &id));
        assert!(!contains_response(&json!({"id":"3","result":{}}), &id));
    }
}
