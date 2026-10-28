use std::collections::BTreeMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::stream;
use parking_lot::Mutex;
use ref_mcp::{McpManager, McpServerConfig};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

const T: Duration = Duration::from_secs(10);

/// (method, session header, protocol version header, auth header)
type Seen1 = (String, Option<String>, Option<String>, Option<String>);

#[derive(Default)]
struct Seen {
    requests: Vec<Seen1>,
}

type Shared = Arc<Mutex<Seen>>;

fn hdr(h: &HeaderMap, k: &str) -> Option<String> {
    h.get(k).and_then(|v| v.to_str().ok()).map(str::to_string)
}

fn handle(msg: &Value) -> Option<Value> {
    let id = msg.get("id")?.clone();
    let method = msg["method"].as_str().unwrap_or("");
    let result = match method {
        "initialize" => json!({"protocolVersion": "2025-03-26", "capabilities": {"tools": {}}, "serverInfo": {"name": "h", "version": "1"}}),
        "tools/list" => json!({"tools": [{"name": "add", "description": "Add", "inputSchema": {"type": "object"}}, {"name": "sse_echo"}]}),
        "tools/call" => {
            let args = &msg["params"]["arguments"];
            match msg["params"]["name"].as_str() {
                Some("add") => {
                    let s = args["a"].as_i64().unwrap_or(0) + args["b"].as_i64().unwrap_or(0);
                    json!({"content": [], "structuredContent": {"sum": s}})
                }
                _ => json!({"content": [{"type": "text", "text": format!("sse: {}", args["text"].as_str().unwrap_or(""))}]}),
            }
        }
        _ => return Some(json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "no"}})),
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

async fn streamable(State(seen): State<Shared>, headers: HeaderMap, Json(msg): Json<Value>) -> Response {
    let method = msg["method"].as_str().unwrap_or("").to_string();
    let sid = hdr(&headers, "mcp-session-id");
    seen.lock().requests.push((
        method.clone(),
        sid.clone(),
        hdr(&headers, "mcp-protocol-version"),
        hdr(&headers, "authorization"),
    ));
    let accept = hdr(&headers, "accept").unwrap_or_default();
    if !accept.contains("text/event-stream") || !accept.contains("application/json") {
        return (StatusCode::NOT_ACCEPTABLE, "bad accept").into_response();
    }
    if method == "initialize" {
        let mut resp = Json(handle(&msg).unwrap()).into_response();
        resp.headers_mut().insert("mcp-session-id", "sess-123".parse().unwrap());
        return resp;
    }
    if sid.as_deref() != Some("sess-123") {
        return (StatusCode::BAD_REQUEST, "missing session").into_response();
    }
    let Some(reply) = handle(&msg) else {
        return StatusCode::ACCEPTED.into_response();
    };
    let sse_reply = method == "tools/list"
        || (method == "tools/call" && msg["params"]["name"] == "sse_echo");
    if sse_reply {
        // A server→client ping and a notification precede the actual response.
        let events = vec![
            Ok::<_, Infallible>(Event::default().data(json!({"jsonrpc": "2.0", "id": "p1", "method": "ping"}).to_string())),
            Ok(Event::default().data(json!({"jsonrpc": "2.0", "method": "notifications/message", "params": {}}).to_string())),
            Ok(Event::default().data(reply.to_string())),
        ];
        Sse::new(stream::iter(events)).into_response()
    } else {
        Json(reply).into_response()
    }
}

async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{addr}")
}

fn remote(url: String) -> McpServerConfig {
    let mut headers = BTreeMap::new();
    headers.insert("Authorization".to_string(), "Bearer tok".to_string());
    McpServerConfig::Remote { url, headers, enabled: None, timeout: Some(5000) }
}

#[tokio::test]
async fn streamable_http_json_and_sse() {
    let seen: Shared = Default::default();
    let app = Router::new()
        .route("/mcp", post(streamable).get(|| async { StatusCode::METHOD_NOT_ALLOWED }))
        .with_state(seen.clone());
    let base = serve(app).await;
    let mgr = McpManager::start([("h".to_string(), remote(format!("{base}/mcp")))].into()).await;
    let st = mgr.status();
    assert_eq!(st[0].status, "connected", "{st:?}");
    assert_eq!(st[0].tools, 2);
    let tools = mgr.tools();
    assert_eq!(tools[0].name, "h_add");
    assert_eq!(tools[0].def.parameters, json!({"type": "object", "properties": {}}));

    let r = mgr.call("h_add", json!({"a": 2, "b": 3}), CancellationToken::new(), T).await;
    assert!(!r.is_error, "{}", r.text);
    assert_eq!(r.text, "{\n  \"sum\": 5\n}");
    let r = mgr.call("h_sse_echo", json!({"text": "yo"}), CancellationToken::new(), T).await;
    assert_eq!(r.text, "sse: yo");

    tokio::time::sleep(Duration::from_millis(200)).await;
    let reqs = seen.lock().requests.clone();
    let methods: Vec<&str> = reqs.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(methods[..3], ["initialize", "notifications/initialized", "tools/list"]);
    // initialize has no session; everything after carries it plus the negotiated version.
    assert_eq!(reqs[0].1, None);
    assert_eq!(reqs[0].2, None);
    for r in &reqs[1..] {
        assert_eq!(r.1.as_deref(), Some("sess-123"), "{r:?}");
        assert_eq!(r.2.as_deref(), Some("2025-03-26"), "{r:?}");
    }
    assert!(reqs.iter().all(|r| r.3.as_deref() == Some("Bearer tok")));
    // The ping inside the SSE stream got answered (posted back as a response without method).
    assert!(methods.contains(&""), "{methods:?}");
    mgr.shutdown().await;
}

// ───────────── legacy SSE ─────────────

#[derive(Clone, Default)]
struct Legacy {
    tx: Arc<Mutex<Option<mpsc::UnboundedSender<String>>>>,
}

async fn legacy_get(State(st): State<Legacy>) -> impl IntoResponse {
    let (tx, rx) = mpsc::unbounded_channel::<String>();
    *st.tx.lock() = Some(tx);
    let first = stream::iter(vec![Ok::<_, Infallible>(Event::default().event("endpoint").data("/messages?sid=abc"))]);
    let rest = stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|m| (Ok::<_, Infallible>(Event::default().event("message").data(m)), rx))
    });
    use futures::StreamExt;
    Sse::new(first.chain(rest))
}

async fn legacy_post(State(st): State<Legacy>, Json(msg): Json<Value>) -> StatusCode {
    if let Some(reply) = handle(&msg) {
        if let Some(tx) = st.tx.lock().as_ref() {
            let _ = tx.send(reply.to_string());
        }
    }
    StatusCode::ACCEPTED
}

#[tokio::test]
async fn legacy_sse_fallback() {
    let st = Legacy::default();
    let app = Router::new()
        .route("/sse", get(legacy_get).post(|| async { StatusCode::METHOD_NOT_ALLOWED }))
        .route("/messages", post(legacy_post))
        .with_state(st);
    let base = serve(app).await;
    let mgr = McpManager::start([("old".to_string(), remote(format!("{base}/sse")))].into()).await;
    let s = mgr.status();
    assert_eq!(s[0].status, "connected", "{s:?}");
    let r = mgr.call("old_sse_echo", json!({"text": "legacy"}), CancellationToken::new(), T).await;
    assert_eq!(r.text, "sse: legacy");
    mgr.shutdown().await;
}

#[tokio::test]
async fn remote_unreachable_fails() {
    let mgr = McpManager::start(
        [("x".to_string(), McpServerConfig::Remote {
            url: "http://127.0.0.1:9/mcp".into(),
            headers: BTreeMap::new(),
            enabled: None,
            timeout: Some(2000),
        })]
        .into(),
    )
    .await;
    let s = mgr.status();
    assert_eq!(s[0].status, "failed");
    assert!(s[0].error.is_some());
    let r = mgr.call("x_anything", json!({}), CancellationToken::new(), T).await;
    assert!(r.is_error);
}
