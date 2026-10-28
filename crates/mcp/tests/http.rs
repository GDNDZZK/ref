//! Integration tests for the Streamable HTTP and legacy HTTP+SSE transports
//! against a minimal in-process HTTP/1.1 server.

use ref_mcp::{McpClient, ServerConfig, Transport};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc};
use tokio_util::sync::CancellationToken;

struct Req {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

async fn read_req(s: &mut TcpStream) -> Option<Req> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        let n = s.read(&mut tmp).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_string();
    let path = first.next()?.to_string();
    let headers: HashMap<String, String> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let len: usize = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < len {
        let n = s.read(&mut tmp).await.ok()?;
        if n == 0 {
            return None;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    Some(Req { method, path, headers, body })
}

async fn respond(s: &mut TcpStream, status: &str, extra: &[(&str, String)], ctype: &str, body: &str) {
    let mut out = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
    if !ctype.is_empty() {
        out.push_str(&format!("Content-Type: {ctype}\r\n"));
    }
    for (k, v) in extra {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str("\r\n");
    out.push_str(body);
    let _ = s.write_all(out.as_bytes()).await;
    let _ = s.shutdown().await;
}

async fn sse_head(s: &mut TcpStream, extra: &[(&str, String)]) {
    let mut out = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n".to_string();
    for (k, v) in extra {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str("\r\n");
    let _ = s.write_all(out.as_bytes()).await;
}

async fn sse_send(s: &mut TcpStream, msg: &Value) -> bool {
    // Split data over two writes to exercise incremental parsing.
    let text = format!("id: 1\r\nevent: message\r\ndata: {msg}\r\n\r\n");
    let (a, b) = text.split_at(text.len() / 2);
    s.write_all(a.as_bytes()).await.is_ok() && s.flush().await.is_ok() && s.write_all(b.as_bytes()).await.is_ok()
}

fn text(t: &str) -> Value {
    json!({"content": [{"type": "text", "text": t}]})
}

fn ok(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

// ---------------------------------------------------------------------------
// Streamable HTTP mock
// ---------------------------------------------------------------------------

/// (method, Mcp-Session-Id, MCP-Protocol-Version, tool name) per POST; None = header missing.
type Seen = (String, Option<String>, Option<String>, Option<String>);

#[derive(Default)]
struct State {
    session: u32,
    inits: u32,
    initialized_notes: u32,
    seen: Vec<Seen>,
    cancels: Vec<Value>,
    get_streams: u32,
}

struct Streamable {
    state: Mutex<State>,
    /// Client responses to server-initiated requests.
    replies: broadcast::Sender<Value>,
    get_enabled: bool,
}

async fn streamable_conn(mut s: TcpStream, srv: Arc<Streamable>) {
    let Some(req) = read_req(&mut s).await else { return };
    let sid_hdr = req.headers.get("mcp-session-id").cloned();
    let current = format!("sess-{}", srv.state.lock().unwrap().session);
    if req.method == "GET" {
        if !srv.get_enabled {
            respond(&mut s, "405 Method Not Allowed", &[], "", "").await;
            return;
        }
        srv.state.lock().unwrap().get_streams += 1;
        sse_head(&mut s, &[]).await;
        let _ = s.write_all(b": keepalive\n\n").await;
        sse_send(&mut s, &json!({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"})).await;
        tokio::time::sleep(Duration::from_secs(30)).await;
        return;
    }
    if req.method == "DELETE" {
        respond(&mut s, "200 OK", &[], "", "").await;
        return;
    }
    assert_eq!(req.method, "POST");
    assert_eq!(req.path, "/mcp");
    assert_eq!(req.headers.get("content-type").map(String::as_str), Some("application/json"));
    let accept = req.headers.get("accept").cloned().unwrap_or_default();
    assert!(accept.contains("application/json") && accept.contains("text/event-stream"), "{accept}");
    if req.headers.get("authorization").map(String::as_str) != Some("Bearer secret") {
        respond(&mut s, "401 Unauthorized", &[], "", "no auth").await;
        return;
    }
    let msg: Value = serde_json::from_slice(&req.body).unwrap();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("").to_string();
    let id = msg.get("id").cloned().unwrap_or(Value::Null);
    srv.state.lock().unwrap().seen.push((
        method.clone(),
        sid_hdr.clone(),
        req.headers.get("mcp-protocol-version").cloned(),
        msg.pointer("/params/name").and_then(Value::as_str).map(String::from),
    ));

    if method == "initialize" {
        let sid = {
            let mut st = srv.state.lock().unwrap();
            st.session += 1;
            st.inits += 1;
            format!("sess-{}", st.session)
        };
        let body = ok(&id, json!({"protocolVersion": "2025-06-18", "capabilities": {"tools": {}},
            "serverInfo": {"name": "h", "version": "1"}, "instructions": "http instructions"}));
        respond(&mut s, "200 OK", &[("Mcp-Session-Id", sid)], "application/json", &body.to_string()).await;
        return;
    }
    if sid_hdr.as_deref() != Some(current.as_str()) {
        respond(&mut s, "404 Not Found", &[], "", "session not found").await;
        return;
    }
    if method.is_empty() {
        // Client response to a server request.
        let _ = srv.replies.send(msg);
        respond(&mut s, "202 Accepted", &[], "", "").await;
        return;
    }
    if id.is_null() {
        {
            let mut st = srv.state.lock().unwrap();
            match method.as_str() {
                "notifications/initialized" => st.initialized_notes += 1,
                "notifications/cancelled" => st.cancels.push(msg["params"].clone()),
                _ => {}
            }
        }
        respond(&mut s, "202 Accepted", &[], "", "").await;
        return;
    }
    match method.as_str() {
        "tools/list" => {
            // SSE response with a notification before the result, paginated.
            sse_head(&mut s, &[]).await;
            let _ = s.write_all(b": comment\r\n\r\n").await;
            sse_send(&mut s, &json!({"jsonrpc": "2.0", "method": "notifications/message", "params": {"level": "info"}})).await;
            let page = match msg.pointer("/params/cursor").and_then(Value::as_str) {
                None => ok(&id, json!({"tools": [{"name": "a", "description": "A",
                    "inputSchema": {"type": "object", "properties": {"x": {"type": "string"}}}}], "nextCursor": "c2"})),
                Some("c2") => ok(&id, json!({"tools": [{"name": "b"}]})),
                Some(other) => panic!("bad cursor {other}"),
            };
            sse_send(&mut s, &page).await;
            let _ = s.shutdown().await;
        }
        "tools/call" => {
            let name = msg.pointer("/params/name").and_then(Value::as_str).unwrap_or("");
            let args = msg.pointer("/params/arguments").cloned().unwrap_or(Value::Null);
            match name {
                "echo" => {
                    let body = ok(&id, text(args["msg"].as_str().unwrap_or("")));
                    respond(&mut s, "200 OK", &[], "application/json; charset=utf-8", &body.to_string()).await;
                }
                "batch" => {
                    let body = json!([
                        {"jsonrpc": "2.0", "method": "notifications/tools/list_changed"},
                        ok(&id, text("batched")),
                    ]);
                    respond(&mut s, "200 OK", &[], "application/json", &body.to_string()).await;
                }
                "ask" => {
                    // Server->client ping on the response stream, wait for the
                    // client's POSTed reply, then answer.
                    let mut replies = srv.replies.subscribe();
                    sse_head(&mut s, &[]).await;
                    sse_send(&mut s, &json!({"jsonrpc": "2.0", "id": "p1", "method": "ping"})).await;
                    let reply = tokio::time::timeout(Duration::from_secs(5), replies.recv()).await.unwrap().unwrap();
                    sse_send(&mut s, &ok(&id, text(&reply.to_string()))).await;
                    let _ = s.shutdown().await;
                }
                "expire" => {
                    srv.state.lock().unwrap().session += 100; // invalidate session
                    let body = ok(&id, text("expired"));
                    respond(&mut s, "200 OK", &[], "application/json", &body.to_string()).await;
                }
                "slow" => {
                    sse_head(&mut s, &[]).await;
                    tokio::time::sleep(Duration::from_secs(30)).await;
                }
                "fail" => {
                    let body = json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32000, "message": "boom"}});
                    respond(&mut s, "200 OK", &[], "application/json", &body.to_string()).await;
                }
                _ => panic!("unknown tool {name}"),
            }
        }
        other => panic!("unexpected method {other}"),
    }
}

async fn start_streamable(get_enabled: bool) -> (String, Arc<Streamable>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let srv = Arc::new(Streamable { state: Mutex::default(), replies: broadcast::channel(16).0, get_enabled });
    let s2 = srv.clone();
    tokio::spawn(async move {
        while let Ok((sock, _)) = listener.accept().await {
            tokio::spawn(streamable_conn(sock, s2.clone()));
        }
    });
    (format!("http://{addr}/mcp"), srv)
}

fn remote(url: &str) -> ServerConfig {
    ServerConfig {
        name: "http".into(),
        transport: Transport::Remote {
            url: url.into(),
            headers: BTreeMap::from([("Authorization".to_string(), "Bearer secret".to_string())]),
        },
        timeout: Duration::from_secs(5),
    }
}

#[tokio::test]
async fn streamable_basic_flow() {
    let (url, srv) = start_streamable(false).await;
    let c = McpClient::connect(&remote(&url)).await.unwrap();
    assert_eq!(c.instructions().as_deref(), Some("http instructions"));
    let tools = c.list_tools().await.unwrap();
    assert_eq!(tools.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    assert_eq!(tools[0].input_schema["properties"]["x"]["type"], "string");

    let tok = CancellationToken::new();
    assert_eq!(c.call_tool("echo", json!({"msg": "yo"}), &tok).await.unwrap().text, "yo");
    let err = c.call_tool("fail", json!({}), &tok).await.unwrap_err();
    assert_eq!(err.to_string(), "MCP error -32000: boom");

    let mut rx = c.tools_changed();
    assert_eq!(c.call_tool("batch", json!({}), &tok).await.unwrap().text, "batched");
    assert_eq!(*rx.borrow_and_update(), 1);

    let r = c.call_tool("ask", json!({}), &tok).await.unwrap();
    let reply: Value = serde_json::from_str(&r.text).unwrap();
    assert_eq!(reply, json!({"jsonrpc": "2.0", "id": "p1", "result": {}}));

    {
        let st = srv.state.lock().unwrap();
        assert_eq!(st.initialized_notes, 1);
        let (m, sid, ver, _) = &st.seen[0];
        assert_eq!((m.as_str(), sid, ver), ("initialize", &None, &None));
        for (m, sid, ver, _) in &st.seen[1..] {
            assert_eq!(sid.as_deref(), Some("sess-1"), "{m}");
            assert_eq!(ver.as_deref(), Some("2025-06-18"), "{m}");
        }
    }
    c.close().await;
}

#[tokio::test]
async fn streamable_session_expiry_reinitializes() {
    let (url, srv) = start_streamable(false).await;
    let c = McpClient::connect(&remote(&url)).await.unwrap();
    let tok = CancellationToken::new();
    assert_eq!(c.call_tool("expire", json!({}), &tok).await.unwrap().text, "expired");
    // Old session is now unknown -> 404 -> re-init -> retry succeeds.
    assert_eq!(c.call_tool("echo", json!({"msg": "again"}), &tok).await.unwrap().text, "again");
    let st = srv.state.lock().unwrap();
    assert_eq!(st.inits, 2);
    assert_eq!(st.initialized_notes, 2);
    assert_eq!(st.seen.last().unwrap().1.as_deref(), Some("sess-102"));
}

#[tokio::test]
async fn streamable_cancel_and_get_stream() {
    let (url, srv) = start_streamable(true).await;
    let c = McpClient::connect(&remote(&url)).await.unwrap();
    let mut rx = c.tools_changed();
    tokio::time::timeout(Duration::from_secs(5), rx.changed()).await.unwrap().unwrap();
    assert_eq!(srv.state.lock().unwrap().get_streams, 1);

    let tok = CancellationToken::new();
    let t2 = tok.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        t2.cancel();
    });
    let err = c.call_tool("slow", json!({}), &tok).await.unwrap_err();
    assert_eq!(err.to_string(), "cancelled");
    for _ in 0..100 {
        if !srv.state.lock().unwrap().cancels.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let st = srv.state.lock().unwrap();
    assert_eq!(st.cancels.len(), 1);
    assert!(st.cancels[0]["requestId"].is_number());
}

#[tokio::test]
async fn streamable_auth_error() {
    let (url, _srv) = start_streamable(false).await;
    let mut cfg = remote(&url);
    cfg.transport = Transport::Remote { url, headers: BTreeMap::new() };
    let err = McpClient::connect(&cfg).await.err().unwrap();
    assert!(format!("{err:#}").contains("401"), "{err:#}");
}

// ---------------------------------------------------------------------------
// Legacy HTTP+SSE mock
// ---------------------------------------------------------------------------

struct Legacy {
    /// Per-connection SSE sender (only one client in tests).
    sse: Mutex<Option<mpsc::UnboundedSender<Value>>>,
    posts: Mutex<Vec<String>>,
}

async fn legacy_conn(mut s: TcpStream, srv: Arc<Legacy>) {
    let Some(req) = read_req(&mut s).await else { return };
    match (req.method.as_str(), req.path.as_str()) {
        ("POST", "/sse") => respond(&mut s, "405 Method Not Allowed", &[], "", "").await,
        ("GET", "/sse") => {
            assert_eq!(req.headers.get("accept").map(String::as_str), Some("text/event-stream"));
            let (tx, mut rx) = mpsc::unbounded_channel();
            *srv.sse.lock().unwrap() = Some(tx);
            sse_head(&mut s, &[]).await;
            let _ = s.write_all(b"event: endpoint\ndata: messages?sid=abc\n\n").await;
            while let Some(msg) = rx.recv().await {
                if !sse_send(&mut s, &msg).await {
                    break;
                }
            }
        }
        ("POST", path) if path.starts_with("/messages?sid=abc") => {
            let msg: Value = serde_json::from_slice(&req.body).unwrap();
            srv.posts.lock().unwrap().push(msg["method"].as_str().unwrap_or("").to_string());
            respond(&mut s, "202 Accepted", &[], "", "Accepted").await;
            let tx = srv.sse.lock().unwrap().clone().unwrap();
            let id = msg.get("id").cloned().unwrap_or(Value::Null);
            match msg["method"].as_str() {
                Some("initialize") => {
                    let _ = tx.send(ok(&id, json!({"protocolVersion": "2024-11-05", "capabilities": {}})));
                }
                Some("tools/list") => {
                    let _ = tx.send(json!({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"}));
                    let _ = tx.send(ok(&id, json!({"tools": [{"name": "legacy_tool", "description": "L"}]})));
                }
                Some("tools/call") => {
                    let _ = tx.send(ok(&id, text("legacy result")));
                }
                _ => {}
            }
        }
        _ => respond(&mut s, "404 Not Found", &[], "", "").await,
    }
}

#[tokio::test]
async fn legacy_sse_fallback() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let srv = Arc::new(Legacy { sse: Mutex::new(None), posts: Mutex::default() });
    let s2 = srv.clone();
    tokio::spawn(async move {
        while let Ok((sock, _)) = listener.accept().await {
            tokio::spawn(legacy_conn(sock, s2.clone()));
        }
    });
    let mut cfg = remote(&format!("http://{addr}/sse"));
    cfg.transport = Transport::Remote { url: format!("http://{addr}/sse"), headers: BTreeMap::new() };
    let c = McpClient::connect(&cfg).await.unwrap();
    assert_eq!(c.instructions(), None);
    let rx = c.tools_changed();
    let tools = c.list_tools().await.unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "legacy_tool");
    assert_eq!(tools[0].input_schema["type"], "object");
    assert_eq!(*rx.borrow(), 1);
    let tok = CancellationToken::new();
    assert_eq!(c.call_tool("legacy_tool", json!({}), &tok).await.unwrap().text, "legacy result");
    assert_eq!(
        *srv.posts.lock().unwrap(),
        ["initialize", "notifications/initialized", "tools/list", "tools/call"]
    );
    c.close().await;
}
