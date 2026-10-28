//! JSON-RPC core shared by all transports: request ids, pending responses, server→client
//! requests and notifications.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::sync::{oneshot, Notify};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub(crate) enum RpcErr {
    /// JSON-RPC error object returned by the server.
    Rpc { code: i64, message: String },
    /// Non-success HTTP status.
    Http { status: u16, body: String },
    /// I/O / network / protocol failure.
    Transport(String),
    /// The connection is closed (process exited, stream ended, shut down).
    Closed(String),
    Timeout(Duration),
    Cancelled,
}

impl fmt::Display for RpcErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RpcErr::Rpc { code, message } => write!(f, "MCP error {code}: {message}"),
            RpcErr::Http { status, body } => {
                if body.trim().is_empty() {
                    write!(f, "HTTP {status}")
                } else {
                    write!(f, "HTTP {status}: {}", body.trim())
                }
            }
            RpcErr::Transport(m) => write!(f, "transport error: {m}"),
            RpcErr::Closed(m) => write!(f, "connection closed: {m}"),
            RpcErr::Timeout(d) => write!(f, "timed out after {:.1}s", d.as_secs_f64()),
            RpcErr::Cancelled => write!(f, "cancelled"),
        }
    }
}

type Pending = oneshot::Sender<Result<Value, RpcErr>>;

pub(crate) struct Core {
    next_id: AtomicU64,
    pending: Mutex<HashMap<u64, Pending>>,
    /// Cancelled when the connection is closed; also stops all background tasks.
    pub closed: CancellationToken,
    close_reason: Mutex<Option<String>>,
    /// Signalled on `notifications/tools/list_changed`.
    pub list_changed: Notify,
}

impl Core {
    pub fn new() -> Self {
        Core {
            next_id: AtomicU64::new(1),
            pending: Mutex::new(HashMap::new()),
            closed: CancellationToken::new(),
            close_reason: Mutex::new(None),
            list_changed: Notify::new(),
        }
    }

    pub fn is_closed(&self) -> bool {
        self.closed.is_cancelled()
    }

    pub fn close_reason(&self) -> String {
        self.close_reason.lock().clone().unwrap_or_else(|| "connection closed".to_string())
    }

    /// Close the connection: fail all pending requests and stop background tasks.
    /// The first reason wins.
    pub fn close(&self, reason: impl Into<String>) {
        {
            let mut r = self.close_reason.lock();
            if r.is_none() {
                *r = Some(reason.into());
            }
        }
        self.closed.cancel();
        let reason = self.close_reason();
        let pending: Vec<Pending> = self.pending.lock().drain().map(|(_, v)| v).collect();
        for p in pending {
            let _ = p.send(Err(RpcErr::Closed(reason.clone())));
        }
    }

    pub fn register(&self) -> (u64, oneshot::Receiver<Result<Value, RpcErr>>) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(id, tx);
        (id, rx)
    }

    pub fn unregister(&self, id: u64) {
        self.pending.lock().remove(&id);
    }

    pub fn is_pending(&self, id: u64) -> bool {
        self.pending.lock().contains_key(&id)
    }

    /// Resolve a pending request with an error (e.g. HTTP failure of the POST carrying it).
    pub fn fail(&self, id: u64, err: RpcErr) {
        if let Some(p) = self.pending.lock().remove(&id) {
            let _ = p.send(Err(err));
        }
    }

    /// Handle one incoming JSON-RPC value (message or batch). Returns replies to send back
    /// for server→client requests.
    pub fn handle(&self, msg: Value) -> Vec<Value> {
        match msg {
            Value::Array(items) => items.into_iter().flat_map(|m| self.handle_one(m)).collect(),
            other => self.handle_one(other).into_iter().collect(),
        }
    }

    /// Parse raw text (one JSON value) and handle it.
    pub fn handle_text(&self, text: &str) -> Vec<Value> {
        let text = text.trim();
        if text.is_empty() {
            return Vec::new();
        }
        match serde_json::from_str::<Value>(text) {
            Ok(v) => self.handle(v),
            Err(e) => {
                tracing::debug!("mcp: ignoring non-JSON message ({e}): {}", truncate(text, 200));
                Vec::new()
            }
        }
    }

    fn handle_one(&self, msg: Value) -> Option<Value> {
        let obj = msg.as_object()?;
        let id = obj.get("id").filter(|v| !v.is_null()).cloned();
        if let Some(method) = obj.get("method").and_then(Value::as_str) {
            return match id {
                Some(id) => Some(server_request(id, method)),
                None => {
                    if method == "notifications/tools/list_changed" {
                        self.list_changed.notify_one();
                    }
                    None
                }
            };
        }
        // Response.
        let id = id.and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))?;
        let p = self.pending.lock().remove(&id)?;
        let res = if let Some(err) = obj.get("error") {
            Err(RpcErr::Rpc {
                code: err.get("code").and_then(Value::as_i64).unwrap_or(0),
                message: err
                    .get("message")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| err.to_string()),
            })
        } else {
            Ok(obj.get("result").cloned().unwrap_or(Value::Null))
        };
        let _ = p.send(res);
        None
    }
}

/// Minimal handling of server→client requests.
fn server_request(id: Value, method: &str) -> Value {
    match method {
        "ping" => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
        "roots/list" => json!({"jsonrpc": "2.0", "id": id, "result": {"roots": []}}),
        _ => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": -32601, "message": format!("Method not found: {method}")}
        }),
    }
}

pub(crate) fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}
