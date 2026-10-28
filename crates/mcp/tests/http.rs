//! Streamable HTTP + legacy SSE tests against a local axum mock server.

use axum::body::{Body, Bytes};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use hn_config::McpConfig;
use indexmap::IndexMap;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct Mock {
    session_counter: u64,
    valid_sessions: Vec<String>,
    inits: u32,
    cancelled: Vec<Value>,
    /// Waiting for the client's reply to a server->client request.
    awaiting: HashMap<String, oneshot::Sender<Value>>,
    protocol_headers: Vec<Option<String>>,
    legacy_tx: Option<mpsc::UnboundedSender<String>>,
    seen_auth: Vec<Option<String>>,
}

type S = Arc<Mutex<Mock>>;

fn tools() -> Value {
    json!([
        {"name": "echo", "description": "Echo", "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}}},
        {"name": "sse_echo", "description": "Echo via SSE"},
        {"name": "ask_client", "description": "server->client ping mid-call"},
        {"name": "slow", "description": "never finishes"},
        {"name": "expire", "description": "invalidate the session"},
        {"name": "stats", "description": "server stats"},
    ])
}

fn sse_body(rx: mpsc::UnboundedReceiver<String>) -> Body {
    let stream = futures::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|s| (Ok::<_, std::convert::Infallible>(Bytes::from(s)), rx))
    });
    Body::from_stream(stream)
}

fn sse_response(rx: mpsc::UnboundedReceiver<String>) -> Response {
    ([("content-type", "text/event-stream")], sse_body(rx)).into_response()
}

fn ev(v: &Value) -> String {
    format!("event: message\nid: x\ndata: {}\n\n", v)
}

fn rpc_result(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn text(t: &str) -> Value {
    json!({"content": [{"type": "text", "text": t}]})
}

async fn mcp_post(State(st): State<S>, headers: HeaderMap, body: Bytes) -> Response {
    let msg: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return (StatusCode::BAD_REQUEST, "bad json").into_response(),
    };
    let auth = headers.get("authorization").and_then(|v| v.to_str().ok()).map(str::to_string);
    if auth.as_deref() != Some("Bearer secret") {
        return (StatusCode::UNAUTHORIZED, "no auth").into_response();
    }
    let accept = headers.get("accept").and_then(|v| v.to_str().ok()).unwrap_or("");
    if !(accept.contains("application/json") && accept.contains("text/event-stream")) {
        return (StatusCode::NOT_ACCEPTABLE, "bad accept").into_response();
    }
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("").to_string();
    let id = msg.get("id").cloned().unwrap_or(Value::Null);
    let session = headers.get("mcp-session-id").and_then(|v| v.to_str().ok()).map(str::to_string);

    if method == "initialize" {
        let mut s = st.lock().unwrap();
        s.inits += 1;
        s.session_counter += 1;
        let sid = format!("sess-{}", s.session_counter);
        s.valid_sessions.push(sid.clone());
        let result = rpc_result(&id, json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "http-mock", "version": "0"},
            "instructions": "HTTP instructions",
        }));
        return ([("mcp-session-id", sid.as_str()), ("content-type", "application/json")], result.to_string()).into_response();
    }
    {
        let mut s = st.lock().unwrap();
        match &session {
            Some(sid) if s.valid_sessions.contains(sid) => {}
            Some(_) => return (StatusCode::NOT_FOUND, "session expired").into_response(),
            None => return (StatusCode::BAD_REQUEST, "missing session").into_response(),
        }
        s.protocol_headers.push(headers.get("mcp-protocol-version").and_then(|v| v.to_str().ok()).map(str::to_string));
        s.seen_auth.push(auth);
    }

    // client response to our server->client request, or a notification
    if method.is_empty() {
        let key = id.to_string();
        if let Some(tx) = st.lock().unwrap().awaiting.remove(&key) {
            let _ = tx.send(msg);
        }
        return StatusCode::ACCEPTED.into_response();
    }
    if method == "notifications/cancelled" {
        st.lock().unwrap().cancelled.push(msg["params"]["requestId"].clone());
        return StatusCode::ACCEPTED.into_response();
    }
    if msg.get("id").is_none() {
        return StatusCode::ACCEPTED.into_response();
    }

    match method.as_str() {
        "tools/list" => {
            // SSE with an interleaved notification and a trailing event without blank line
            let (tx, rx) = mpsc::unbounded_channel();
            tx.send(": keepalive\n\n".into()).unwrap();
            tx.send(ev(&json!({"jsonrpc": "2.0", "method": "notifications/message", "params": {"level": "info", "data": "listing"}}))).unwrap();
            let r = rpc_result(&id, json!({"tools": tools()}));
            tx.send(format!("data: {r}")).unwrap();
            drop(tx);
            sse_response(rx)
        }
        "tools/call" => {
            let name = msg["params"]["name"].as_str().unwrap_or("").to_string();
            let args = msg["params"]["arguments"].clone();
            match name.as_str() {
                "echo" => ([("content-type", "application/json")], rpc_result(&id, text(args["text"].as_str().unwrap_or(""))).to_string()).into_response(),
                "sse_echo" => {
                    let (tx, rx) = mpsc::unbounded_channel();
                    // split an event across chunks, include an unrelated response id first
                    let r = rpc_result(&id, text(&format!("sse:{}", args["text"].as_str().unwrap_or(""))));
                    let s = ev(&r);
                    let (a, b) = s.split_at(s.len() / 2);
                    tx.send(ev(&rpc_result(&json!(987654), text("not yours")))).unwrap();
                    tx.send(a.to_string()).unwrap();
                    tx.send(b.to_string()).unwrap();
                    drop(tx);
                    sse_response(rx)
                }
                "ask_client" => {
                    let (tx, rx) = mpsc::unbounded_channel();
                    let (otx, orx) = oneshot::channel();
                    let req_id = json!("srv-1");
                    st.lock().unwrap().awaiting.insert(req_id.to_string(), otx);
                    tx.send(ev(&json!({"jsonrpc": "2.0", "id": req_id, "method": "ping"}))).unwrap();
                    tokio::spawn(async move {
                        let reply = tokio::time::timeout(Duration::from_secs(5), orx).await.ok().and_then(|r| r.ok());
                        let reply = reply.unwrap_or(json!("no reply"));
                        let _ = tx.send(ev(&rpc_result(&id, text(&reply.to_string()))));
                    });
                    sse_response(rx)
                }
                "slow" => {
                    let (tx, rx) = mpsc::unbounded_channel();
                    tokio::spawn(async move {
                        // keep the stream open, never answer
                        loop {
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            if tx.send(": ping\n\n".into()).is_err() {
                                return;
                            }
                        }
                    });
                    sse_response(rx)
                }
                "expire" => {
                    st.lock().unwrap().valid_sessions.clear();
                    ([("content-type", "application/json")], rpc_result(&id, text("expired")).to_string()).into_response()
                }
                "stats" => {
                    let s = st.lock().unwrap();
                    let stats = json!({"inits": s.inits, "cancelled": s.cancelled, "protocol": s.protocol_headers, "auth_ok": s.seen_auth.iter().all(|a| a.as_deref() == Some("Bearer secret"))});
                    ([("content-type", "application/json")], rpc_result(&id, text(&stats.to_string())).to_string()).into_response()
                }
                _ => ([("content-type", "application/json")], json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32602, "message": "unknown tool"}}).to_string()).into_response(),
            }
        }
        _ => ([("content-type", "application/json")], json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "nope"}}).to_string()).into_response(),
    }
}

// ---- legacy SSE server ----

async fn legacy_get(State(st): State<S>) -> Response {
    let (tx, rx) = mpsc::unbounded_channel();
    tx.send("event: endpoint\ndata: /legacy/messages?sessionId=abc\n\n".into()).unwrap();
    st.lock().unwrap().legacy_tx = Some(tx);
    sse_response(rx)
}

#[derive(serde::Deserialize)]
struct Sid {
    #[serde(rename = "sessionId")]
    session_id: String,
}

async fn legacy_post(State(st): State<S>, Query(q): Query<Sid>, body: Bytes) -> Response {
    assert_eq!(q.session_id, "abc");
    let msg: Value = serde_json::from_slice(&body).unwrap();
    let tx = st.lock().unwrap().legacy_tx.clone().unwrap();
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let reply = match (method, id) {
        ("initialize", Some(id)) => Some(rpc_result(&id, json!({"protocolVersion": "2024-11-05", "capabilities": {}, "serverInfo": {"name": "legacy"}}))),
        ("tools/list", Some(id)) => Some(rpc_result(&id, json!({"tools": [{"name": "echo", "description": "legacy echo"}]}))),
        ("tools/call", Some(id)) => {
            // ping the client first; its reply comes back via POST and is ignored
            let _ = tx.send(ev(&json!({"jsonrpc": "2.0", "id": 77, "method": "ping"})));
            Some(rpc_result(&id, text(&format!("legacy:{}", msg["params"]["arguments"]["text"].as_str().unwrap_or("")))))
        }
        _ => None,
    };
    if let Some(r) = reply {
        let _ = tx.send(format!("event: message\ndata: {r}\n\n"));
    }
    StatusCode::ACCEPTED.into_response()
}

async fn method_not_allowed() -> Response {
    StatusCode::METHOD_NOT_ALLOWED.into_response()
}

async fn serve() -> (String, S) {
    let st: S = Arc::default();
    let app = Router::new()
        .route("/mcp", post(mcp_post).get(method_not_allowed))
        .route("/legacy", get(legacy_get).post(method_not_allowed))
        .route("/legacy/messages", post(legacy_post))
        .with_state(st.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), st)
}

fn remote(url: String, auth: Option<&str>) -> McpConfig {
    let mut headers = IndexMap::new();
    if let Some(a) = auth {
        headers.insert("Authorization".to_string(), a.to_string());
    }
    McpConfig::Remote { url, headers, enabled: None, timeout: Some(5_000), oauth: None }
}

#[tokio::test]
async fn streamable_http() {
    let (base, _st) = serve().await;
    let client = hn_mcp::connect("web", &remote(format!("{base}/mcp"), Some("Bearer secret")), std::path::Path::new("/")).await.unwrap();
    assert_eq!(client.transport(), "streamable-http");
    assert_eq!(client.instructions.as_deref(), Some("HTTP instructions"));
    let names: Vec<String> = client.tools().into_iter().map(|t| t.name).collect();
    assert_eq!(names, ["web_echo", "web_sse_echo", "web_ask_client", "web_slow", "web_expire", "web_stats"]);

    let none = CancellationToken::new();
    assert_eq!(client.call("echo", json!({"text": "a"}), &none).await.unwrap().text, "a");
    assert_eq!(client.call("sse_echo", json!({"text": "b"}), &none).await.unwrap().text, "sse:b");
    let r = client.call("ask_client", json!({}), &none).await.unwrap();
    let reply: Value = serde_json::from_str(&r.text).unwrap();
    assert_eq!(reply["id"], json!("srv-1"));
    assert_eq!(reply["result"], json!({}));

    // concurrent calls, mixed JSON/SSE
    let (a, b) = tokio::join!(client.call("sse_echo", json!({"text": "x"}), &none), client.call("echo", json!({"text": "y"}), &none));
    assert_eq!(a.unwrap().text, "sse:x");
    assert_eq!(b.unwrap().text, "y");

    // cancellation of a call whose SSE stream never answers
    let cancel = CancellationToken::new();
    let c2 = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        c2.cancel();
    });
    let err = client.call("slow", json!({}), &cancel).await.unwrap_err();
    assert!(format!("{err:#}").contains("cancelled"));
    tokio::time::sleep(Duration::from_millis(200)).await;

    // session expiry -> transparent re-initialize
    assert_eq!(client.call("expire", json!({}), &none).await.unwrap().text, "expired");
    assert_eq!(client.call("echo", json!({"text": "after"}), &none).await.unwrap().text, "after");

    let stats: Value = serde_json::from_str(&client.call("stats", json!({}), &none).await.unwrap().text).unwrap();
    assert_eq!(stats["inits"], json!(2));
    assert_eq!(stats["cancelled"].as_array().unwrap().len(), 1);
    assert!(stats["cancelled"][0].is_i64());
    assert!(stats["protocol"].as_array().unwrap().iter().all(|p| p == "2025-06-18"), "{stats}");
    assert_eq!(stats["auth_ok"], json!(true));

    let err = client.call("missing", json!({}), &none).await.unwrap_err();
    assert!(format!("{err:#}").contains("unknown tool"));
    client.shutdown().await;
}

#[tokio::test]
async fn http_auth_failure_is_reported_without_fallback() {
    let (base, _st) = serve().await;
    let err = hn_mcp::connect("web", &remote(format!("{base}/mcp"), Some("Bearer wrong")), std::path::Path::new("/")).await.unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("401"), "{msg}");
    assert!(!msg.contains("SSE fallback"), "{msg}");
    assert!(!msg.contains("wrong"), "header value leaked: {msg}");
}

#[tokio::test]
async fn legacy_sse_fallback() {
    let (base, _st) = serve().await;
    let client = hn_mcp::connect("old", &remote(format!("{base}/legacy"), None), std::path::Path::new("/")).await.unwrap();
    assert_eq!(client.transport(), "sse");
    assert_eq!(client.protocol_version, "2024-11-05");
    assert_eq!(client.tools().len(), 1);
    let none = CancellationToken::new();
    assert_eq!(client.call("echo", json!({"text": "hi"}), &none).await.unwrap().text, "legacy:hi");
    let (a, b) = tokio::join!(client.call("echo", json!({"text": "1"}), &none), client.call("echo", json!({"text": "2"}), &none));
    assert_eq!((a.unwrap().text, b.unwrap().text), ("legacy:1".into(), "legacy:2".into()));
    client.shutdown().await;
}

#[tokio::test]
async fn connection_refused() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let err = hn_mcp::connect("dead", &remote(format!("http://127.0.0.1:{port}/mcp"), None), std::path::Path::new("/")).await.unwrap_err();
    // loopback bypasses any configured proxy, so this is a direct connect error
    assert!(format!("{err:#}").contains("POST"), "{err:#}");
}
