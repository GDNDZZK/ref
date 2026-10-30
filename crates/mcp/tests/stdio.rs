//! Integration tests for the stdio transport using a python3 mock MCP server.

use ref_mcp::{McpClient, ServerConfig, Transport};
use serde_json::json;
use std::collections::BTreeMap;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const SERVER: &str = r#"
import sys, json, threading, time, os

lock = threading.Lock()
waiters = {}
cancelled = {}
last_cancel = [None]

def send(msg):
    with lock:
        sys.stdout.write(json.dumps(msg) + "\n")
        sys.stdout.flush()

def result(id, res):
    send({"jsonrpc": "2.0", "id": id, "result": res})

def text(t):
    return {"content": [{"type": "text", "text": t}]}

def ask_client(rid, method):
    ev = threading.Event()
    waiters[rid] = [ev, None]
    send({"jsonrpc": "2.0", "id": rid, "method": method})
    ev.wait(5)
    return waiters[rid][1]

def call(id, name, args):
    if name == "echo":
        result(id, text(args.get("msg", "")))
    elif name == "env":
        result(id, text(os.environ.get("MCP_TEST_VAR", "") + "|" + os.getcwd() + "|" + ("PATH" in os.environ and "path" or "nopath")))
    elif name == "mixed":
        result(id, {"isError": True, "content": [
            {"type": "text", "text": "hello"},
            {"type": "image", "mimeType": "image/png", "data": "iVBORw0K"},
            {"type": "resource", "resource": {"uri": "file:///a", "text": "rtext"}},
            {"type": "audio", "mimeType": "audio/wav", "data": "AAAA"}]})
    elif name == "structured":
        result(id, {"content": [], "structuredContent": {"n": 42}})
    elif name == "notify":
        send({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"})
        result(id, text("ok"))
    elif name == "ask":
        r1 = ask_client("srv-ping", "ping")
        r2 = ask_client("srv-roots", "roots/list")
        r3 = ask_client(77, "sampling/createMessage")
        result(id, text(json.dumps([r1, r2, r3])))
    elif name == "slow":
        ev = threading.Event()
        cancelled[id] = ev
        if not ev.wait(float(args.get("secs", 10))):
            result(id, text("finished"))
    elif name == "last_cancel":
        result(id, text(json.dumps(last_cancel[0])))
    elif name == "noisy":
        for i in range(2000):
            sys.stderr.write("x" * 200 + "\n")
        sys.stderr.flush()
        result(id, text("quiet now"))
    else:
        send({"jsonrpc": "2.0", "id": id, "error": {"code": -32602, "message": "Unknown tool: " + name}})

print("banner line that is not json", flush=True)
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    msg = json.loads(line)
    method = msg.get("method")
    id = msg.get("id")
    if method is None:
        w = waiters.get(id)
        if w:
            w[1] = msg
            w[0].set()
        continue
    if method == "initialize":
        assert msg["params"]["clientInfo"]["name"] == "ref"
        assert msg["params"]["capabilities"] == {"roots": {"listChanged": False}}
        result(id, {"protocolVersion": "2025-03-26", "capabilities": {"tools": {"listChanged": True}},
                    "serverInfo": {"name": "mock", "version": "1"}, "instructions": "Use the mock wisely."})
    elif method == "notifications/initialized":
        sys.stderr.write("got initialized\n")
    elif method == "notifications/cancelled":
        last_cancel[0] = msg["params"]
        ev = cancelled.get(msg["params"]["requestId"])
        if ev:
            ev.set()
    elif method == "tools/list":
        cursor = (msg.get("params") or {}).get("cursor")
        if cursor is None:
            result(id, {"tools": [{"name": "echo", "description": "Echo", "inputSchema": {"type": "object"}},
                                  {"description": "invalid, no name"}], "nextCursor": "page2"})
        else:
            assert cursor == "page2"
            result(id, {"tools": [{"name": "slow"}]})
    elif method == "tools/call":
        p = msg["params"]
        threading.Thread(target=call, args=(id, p["name"], p.get("arguments") or {}), daemon=True).start()
    elif id is not None:
        send({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "Method not found"}})
"#;

fn cfg(env: BTreeMap<String, String>, cwd: Option<String>) -> ServerConfig {
    ServerConfig {
        name: "mock".into(),
        transport: Transport::Local {
            command: vec!["python3".into(), "-u".into(), "-c".into(), SERVER.into()],
            env,
            cwd,
        },
        timeout: Duration::from_secs(10),
    }
}

async fn connect() -> McpClient {
    McpClient::connect(&cfg(BTreeMap::new(), None))
        .await
        .expect("connect")
}

#[tokio::test]
async fn initialize_and_list_tools() {
    let c = connect().await;
    assert_eq!(c.name(), "mock");
    assert_eq!(c.instructions().as_deref(), Some("Use the mock wisely."));
    let tools = c.list_tools().await.unwrap();
    let names: Vec<_> = tools.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["echo", "slow"]);
    assert_eq!(tools[0].description, "Echo");
    assert_eq!(tools[1].input_schema["type"], "object");
    c.close().await;
}

#[tokio::test]
async fn call_tools_and_content_mapping() {
    let c = connect().await;
    let tok = CancellationToken::new();
    let r = c
        .call_tool("echo", json!({"msg": "hi"}), &tok)
        .await
        .unwrap();
    assert_eq!(r.text, "hi");
    assert!(!r.is_error);

    let r = c.call_tool("mixed", json!({}), &tok).await.unwrap();
    assert!(r.is_error);
    assert!(r.text.starts_with("hello\nrtext\n"), "{}", r.text);
    assert!(r.text.contains("audio/wav"));
    assert_eq!(r.images.len(), 1);
    assert_eq!(r.images[0].mime, "image/png");
    assert_eq!(r.images[0].data, "iVBORw0K");

    let r = c
        .call_tool("structured", serde_json::Value::Null, &tok)
        .await
        .unwrap();
    assert_eq!(r.text, r#"{"n":42}"#);

    let err = c.call_tool("nope", json!({}), &tok).await.unwrap_err();
    assert_eq!(err.to_string(), "MCP error -32602: Unknown tool: nope");
    c.close().await;
}

#[tokio::test]
async fn concurrent_calls() {
    let c = connect().await;
    let tok = CancellationToken::new();
    let slow = c.call_tool("slow", json!({"secs": 0.5}), &tok);
    let fast = async {
        let mut out = Vec::new();
        for i in 0..20 {
            out.push(
                c.call_tool("echo", json!({"msg": i.to_string()}), &tok)
                    .await
                    .unwrap()
                    .text,
            );
        }
        out
    };
    let (slow, fast) = tokio::join!(slow, fast);
    assert_eq!(slow.unwrap().text, "finished");
    assert_eq!(fast, (0..20).map(|i| i.to_string()).collect::<Vec<_>>());
    let many = futures_util::future::join_all(
        (0..30).map(|i| c.call_tool("echo", json!({"msg": format!("m{i}")}), &tok)),
    )
    .await;
    for (i, r) in many.into_iter().enumerate() {
        assert_eq!(r.unwrap().text, format!("m{i}"));
    }
    c.close().await;
}

#[tokio::test]
async fn cancellation_sends_notification() {
    let c = connect().await;
    let tok = CancellationToken::new();
    let t2 = tok.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        t2.cancel();
    });
    let err = c
        .call_tool("slow", json!({"secs": 30}), &tok)
        .await
        .unwrap_err();
    assert_eq!(err.to_string(), "cancelled");
    // Notification is sent asynchronously; poll the server for it.
    let fresh = CancellationToken::new();
    let mut seen = String::new();
    for _ in 0..50 {
        seen = c
            .call_tool("last_cancel", json!({}), &fresh)
            .await
            .unwrap()
            .text;
        if seen != "null" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let v: serde_json::Value = serde_json::from_str(&seen).unwrap();
    assert!(v["requestId"].is_number(), "{seen}");
    assert!(v["reason"].is_string());
    c.close().await;
}

#[tokio::test]
async fn notifications_and_server_requests() {
    let c = connect().await;
    let mut rx = c.tools_changed();
    assert_eq!(*rx.borrow(), 0);
    let tok = CancellationToken::new();
    c.call_tool("notify", json!({}), &tok).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), rx.changed())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(*rx.borrow(), 1);

    let r = c.call_tool("ask", json!({}), &tok).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&r.text).unwrap();
    assert_eq!(
        v[0],
        json!({"jsonrpc": "2.0", "id": "srv-ping", "result": {}})
    );
    assert_eq!(v[1]["result"], json!({"roots": []}));
    assert_eq!(v[2]["id"], 77);
    assert_eq!(v[2]["error"]["code"], -32601);
    c.close().await;
}

#[tokio::test]
async fn env_cwd_and_stderr_drain() {
    let dir = std::env::temp_dir().canonicalize().unwrap();
    let env = BTreeMap::from([("MCP_TEST_VAR".to_string(), "val".to_string())]);
    let c = McpClient::connect(&cfg(env, Some(dir.to_string_lossy().into_owned())))
        .await
        .unwrap();
    let tok = CancellationToken::new();
    let r = c.call_tool("env", json!({}), &tok).await.unwrap();
    // canonicalize() adds a `\\?\` prefix on Windows that the server's getcwd() does not report.
    let shown = dir
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_string();
    assert_eq!(r.text, format!("val|{shown}|path"));
    // ~400KB of stderr must not block the server.
    let r = tokio::time::timeout(
        Duration::from_secs(10),
        c.call_tool("noisy", json!({}), &tok),
    )
    .await
    .unwrap();
    assert_eq!(r.unwrap().text, "quiet now");
    c.close().await;
    let err = c.call_tool("echo", json!({}), &tok).await.unwrap_err();
    assert!(err.to_string().contains("closed"), "{err}");
}

#[tokio::test]
async fn spawn_failure_and_early_exit() {
    let bad = ServerConfig {
        name: "bad".into(),
        transport: Transport::Local {
            command: vec!["/nonexistent/ref-mcp-bin".into()],
            env: BTreeMap::new(),
            cwd: None,
        },
        timeout: Duration::from_secs(5),
    };
    let err = McpClient::connect(&bad).await.err().unwrap();
    assert!(format!("{err:#}").contains("failed to spawn"), "{err:#}");

    let exits = ServerConfig {
        name: "exits".into(),
        transport: Transport::Local {
            command: vec![
                "python3".into(),
                "-c".into(),
                "import sys; sys.exit(3)".into(),
            ],
            env: BTreeMap::new(),
            cwd: None,
        },
        timeout: Duration::from_secs(5),
    };
    let start = std::time::Instant::now();
    let err = McpClient::connect(&exits).await.err().unwrap();
    assert!(start.elapsed() < Duration::from_secs(4));
    assert!(format!("{err:#}").contains("initialize failed"), "{err:#}");
}
