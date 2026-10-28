//! Transport-independent JSON-RPC 2.0 core: request ids, pending map, dispatch of
//! incoming messages (responses, server→client requests, notifications).

use crate::http::{LegacySse, Streamable};
use crate::stdio::Stdio;
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

/// Messages produced by transports.
pub(crate) enum Incoming {
    /// A parsed JSON-RPC message (object or batch array).
    Msg(Value),
    /// The underlying connection is gone; fail everything pending.
    Closed(String),
}

pub(crate) type InTx = mpsc::UnboundedSender<Incoming>;
pub(crate) type InRx = mpsc::UnboundedReceiver<Incoming>;

/// JSON-RPC error object returned by the server.
#[derive(Debug, Clone)]
pub(crate) struct RpcError {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MCP error {}: {}", self.code, self.message)?;
        if let Some(d) = &self.data {
            if !d.is_null() {
                write!(f, " ({})", truncate(&d.to_string(), 500))?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for RpcError {}

pub(crate) const METHOD_NOT_FOUND: i64 = -32601;

pub(crate) enum Transport {
    Stdio(Stdio),
    Http(Streamable),
    Sse(LegacySse),
}

impl Transport {
    /// Send one message. `req_id` is set when `msg` is a request we expect a response to.
    /// For Streamable HTTP the response may be read inline (and pushed to the dispatcher)
    /// before this returns.
    pub async fn send(&self, msg: &Value, req_id: Option<i64>) -> Result<()> {
        match self {
            Transport::Stdio(t) => t.send(msg),
            Transport::Http(t) => t.send(msg, req_id).await,
            Transport::Sse(t) => t.send(msg).await,
        }
    }

    pub async fn close(&self) {
        match self {
            Transport::Stdio(t) => t.close().await,
            Transport::Http(t) => t.close().await,
            Transport::Sse(t) => t.close(),
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Transport::Stdio(_) => "stdio",
            Transport::Http(_) => "streamable-http",
            Transport::Sse(_) => "sse",
        }
    }
}

type Reply = Result<Value>;

#[derive(Default)]
struct State {
    pending: HashMap<i64, oneshot::Sender<Reply>>,
    closed: Option<String>,
}

pub(crate) struct Rpc {
    pub transport: Transport,
    label: String,
    state: Mutex<State>,
    next_id: AtomicI64,
    notif_tx: mpsc::UnboundedSender<Value>,
}

impl Rpc {
    /// Wrap a transport. Returns the core and the stream of server notifications.
    pub fn start(label: &str, transport: Transport, in_rx: InRx) -> (Arc<Rpc>, mpsc::UnboundedReceiver<Value>) {
        let (notif_tx, notif_rx) = mpsc::unbounded_channel();
        let rpc = Arc::new(Rpc {
            transport,
            label: label.to_string(),
            state: Mutex::new(State::default()),
            next_id: AtomicI64::new(1),
            notif_tx,
        });
        tokio::spawn(dispatch_loop(Arc::downgrade(&rpc), in_rx));
        (rpc, notif_rx)
    }

    /// Send a request and await its response.
    pub async fn request(
        self: &Arc<Self>,
        method: &str,
        params: Value,
        cancel: Option<&CancellationToken>,
        timeout: Option<Duration>,
    ) -> Result<Value> {
        if cancel.is_some_and(|c| c.is_cancelled()) {
            bail!("cancelled");
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        {
            let mut st = self.state.lock().unwrap();
            if let Some(r) = &st.closed {
                bail!("connection closed: {r}");
            }
            st.pending.insert(id, tx);
        }
        let _guard = PendingGuard { rpc: self, id };
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let work = async {
            self.transport.send(&msg, Some(id)).await?;
            match rx.await {
                Ok(r) => r,
                Err(_) => Err(anyhow!("connection closed")),
            }
        };
        tokio::select! {
            biased;
            _ = async {
                match cancel {
                    Some(c) => c.cancelled().await,
                    None => std::future::pending().await,
                }
            } => {
                self.send_cancel(id, "cancelled by client");
                bail!("cancelled")
            }
            _ = async {
                match timeout {
                    Some(t) => tokio::time::sleep(t).await,
                    None => std::future::pending().await,
                }
            } => {
                self.send_cancel(id, "timeout");
                bail!("`{method}` timed out after {}ms", timeout.unwrap_or_default().as_millis())
            }
            r = work => r,
        }
    }

    /// Send a notification (no response expected).
    pub async fn notify(&self, method: &str, params: Option<Value>) -> Result<()> {
        let mut msg = json!({"jsonrpc": "2.0", "method": method});
        if let Some(p) = params {
            msg["params"] = p;
        }
        self.transport.send(&msg, None).await
    }

    fn send_cancel(self: &Arc<Self>, id: i64, reason: &str) {
        let rpc = self.clone();
        let params = json!({"requestId": id, "reason": reason});
        tokio::spawn(async move {
            let fut = rpc.notify("notifications/cancelled", Some(params));
            match tokio::time::timeout(Duration::from_secs(10), fut).await {
                Ok(Err(e)) => tracing::debug!(server = %rpc.label, "failed to send cancel: {e:#}"),
                Err(_) => tracing::debug!(server = %rpc.label, "timed out sending cancel"),
                Ok(Ok(())) => {}
            }
        });
    }

    pub fn closed_reason(&self) -> Option<String> {
        self.state.lock().unwrap().closed.clone()
    }

    /// Mark the connection closed and fail every pending request.
    pub fn close_with(&self, reason: &str) {
        let pending: Vec<_> = {
            let mut st = self.state.lock().unwrap();
            if st.closed.is_none() {
                st.closed = Some(reason.to_string());
            }
            st.pending.drain().collect()
        };
        for (_, tx) in pending {
            let _ = tx.send(Err(anyhow!("connection closed: {reason}")));
        }
    }

    fn dispatch(self: &Arc<Self>, msg: Value) {
        let obj = match msg {
            Value::Array(items) => {
                for m in items {
                    self.dispatch(m);
                }
                return;
            }
            Value::Object(o) => o,
            other => {
                tracing::debug!(server = %self.label, "ignoring non-object message: {}", truncate(&other.to_string(), 200));
                return;
            }
        };
        let method = obj.get("method").and_then(Value::as_str).map(str::to_string);
        let id = obj.get("id").filter(|v| !v.is_null()).cloned();
        match (method, id) {
            (Some(m), Some(id)) => self.handle_server_request(&m, id),
            (Some(_), None) => {
                let _ = self.notif_tx.send(Value::Object(obj));
            }
            (None, Some(id)) => {
                let Some(id) = id.as_i64().or_else(|| id.as_str().and_then(|s| s.parse().ok())) else {
                    tracing::debug!(server = %self.label, "response with unknown id {id}");
                    return;
                };
                let reply = if let Some(err) = obj.get("error") {
                    Err(anyhow::Error::new(RpcError {
                        code: err.get("code").and_then(Value::as_i64).unwrap_or(0),
                        message: err.get("message").and_then(Value::as_str).unwrap_or("unknown error").to_string(),
                        data: err.get("data").cloned(),
                    }))
                } else {
                    Ok(obj.get("result").cloned().unwrap_or(Value::Null))
                };
                let tx = self.state.lock().unwrap().pending.remove(&id);
                match tx {
                    Some(tx) => {
                        let _ = tx.send(reply);
                    }
                    None => tracing::debug!(server = %self.label, "response for unknown/finished request {id}"),
                }
            }
            (None, None) => {
                let raw = Value::Object(obj).to_string();
                tracing::debug!(server = %self.label, "ignoring malformed message: {}", truncate(&raw, 200));
            }
        }
    }

    fn handle_server_request(self: &Arc<Self>, method: &str, id: Value) {
        let reply = match method {
            "ping" => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
            "roots/list" => json!({"jsonrpc": "2.0", "id": id, "result": {"roots": []}}),
            _ => json!({"jsonrpc": "2.0", "id": id, "error": {"code": METHOD_NOT_FOUND, "message": format!("Method not found: {method}")}}),
        };
        tracing::debug!(server = %self.label, "server request `{method}`");
        let rpc = self.clone();
        tokio::spawn(async move {
            if let Err(e) = rpc.transport.send(&reply, None).await {
                tracing::debug!(server = %rpc.label, "failed to reply to server request: {e:#}");
            }
        });
    }
}

struct PendingGuard<'a> {
    rpc: &'a Rpc,
    id: i64,
}

impl Drop for PendingGuard<'_> {
    fn drop(&mut self) {
        self.rpc.state.lock().unwrap().pending.remove(&self.id);
    }
}

async fn dispatch_loop(weak: Weak<Rpc>, mut rx: InRx) {
    while let Some(m) = rx.recv().await {
        let Some(rpc) = weak.upgrade() else { return };
        match m {
            Incoming::Msg(v) => rpc.dispatch(v),
            Incoming::Closed(reason) => {
                tracing::debug!(server = %rpc.label, "connection closed: {reason}");
                rpc.close_with(&reason);
            }
        }
    }
    if let Some(rpc) = weak.upgrade() {
        rpc.close_with("connection closed");
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
