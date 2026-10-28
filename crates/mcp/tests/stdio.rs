use hn_config::McpConfig;
use indexmap::IndexMap;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mock_server.py")
}

fn local(cmd: Vec<String>, cwd: Option<String>) -> McpConfig {
    let mut environment = IndexMap::new();
    environment.insert("HN_MOCK_VAR".to_string(), "from-config".to_string());
    McpConfig::Local { command: cmd, environment, cwd, enabled: None, timeout: Some(10_000) }
}

fn mock_cfg() -> McpConfig {
    local(vec!["python3".into(), fixture().display().to_string()], None)
}

#[tokio::test]
async fn stdio_end_to_end() {
    let tmp = std::env::temp_dir();
    let client = hn_mcp::connect("mock srv", &mock_cfg(), &tmp).await.expect("connect");
    assert_eq!(client.name, "mock srv");
    assert_eq!(client.instructions.as_deref(), Some("Use the mock tools wisely."));
    assert_eq!(client.transport(), "stdio");

    // pagination: 12 tools over 6 pages
    let tools = client.tools();
    assert_eq!(tools.len(), 12, "{tools:?}");
    let echo = tools.iter().find(|t| t.tool == "echo").unwrap();
    assert_eq!(echo.name, "mock_srv_echo");
    assert_eq!(echo.server, "mock srv");
    assert_eq!(echo.description, "Echo text");
    assert_eq!(echo.schema["required"], json!(["text"]));
    let img = tools.iter().find(|t| t.tool == "image").unwrap();
    assert_eq!(img.description, "Image tool");
    assert_eq!(img.schema, json!({"type": "object", "properties": {}}));
    assert!(tools.iter().any(|t| t.name == "mock_srv_dotted_name"));

    let none = CancellationToken::new();
    let r = client.call("echo", json!({"text": "hi"}), &none).await.unwrap();
    assert_eq!(r.text, "hi");
    assert!(!r.is_error);
    // exposed name also accepted
    let r = client.call("mock_srv_echo", json!({"text": "again"}), &none).await.unwrap();
    assert_eq!(r.text, "again");

    let r = client.call("image", Value::Null, &none).await.unwrap();
    assert_eq!(r.text, "an image");
    assert_eq!(r.images.len(), 1);
    assert_eq!(r.images[0].mime, "image/png");

    let r = client.call("fail", json!({}), &none).await.unwrap();
    assert!(r.is_error);
    assert_eq!(r.text, "boom");

    let r = client.call("structured", json!({}), &none).await.unwrap();
    assert_eq!(serde_json::from_str::<Value>(&r.text).unwrap(), json!({"answer": 42}));

    let err = client.call("nope", json!({}), &none).await.unwrap_err();
    assert!(format!("{err:#}").contains("unknown tool"), "{err:#}");

    let err = client.call("echo", json!([1]), &none).await.unwrap_err();
    assert!(format!("{err:#}").contains("must be a JSON object"));

    // env + cwd
    let r = client.call("env", json!({"name": "HN_MOCK_VAR"}), &none).await.unwrap();
    assert_eq!(r.text, "from-config");
    let r = client.call("env", json!({"name": "PATH"}), &none).await.unwrap();
    assert_ne!(r.text, "<unset>");
    let r = client.call("cwd", json!({}), &none).await.unwrap();
    assert_eq!(Path::new(&r.text).canonicalize().unwrap(), tmp.canonicalize().unwrap());

    // server -> client requests
    let r = client.call("ping_client", json!({}), &none).await.unwrap();
    let replies: Value = serde_json::from_str(&r.text).unwrap();
    assert_eq!(replies[0]["result"], json!({}));
    assert_eq!(replies[1]["result"], json!({"roots": []}));
    assert_eq!(replies[2]["error"]["code"], json!(-32601));

    // concurrent in-flight calls
    let (a, b, c) = tokio::join!(
        client.call("sleep", json!({"ms": 300}), &none),
        client.call("echo", json!({"text": "fast"}), &none),
        client.call("sleep", json!({"ms": 200}), &none),
    );
    assert_eq!(a.unwrap().text, "slept");
    assert_eq!(b.unwrap().text, "fast");
    assert_eq!(c.unwrap().text, "slept");

    // cancellation
    let cancel = CancellationToken::new();
    let c2 = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        c2.cancel();
    });
    let start = std::time::Instant::now();
    let err = client.call("sleep", json!({"ms": 20_000}), &cancel).await.unwrap_err();
    assert!(start.elapsed() < Duration::from_secs(5));
    assert!(format!("{err:#}").contains("cancelled"), "{err:#}");
    // already-cancelled token fails fast
    assert!(client.call("echo", json!({"text": "x"}), &cancel).await.is_err());
    tokio::time::sleep(Duration::from_millis(200)).await;
    let r = client.call("cancelled", json!({}), &none).await.unwrap();
    let ids: Vec<Value> = serde_json::from_str(&r.text).unwrap();
    assert_eq!(ids.len(), 1, "{ids:?}");
    assert!(ids[0].is_i64());

    // list_changed -> refresh
    let mut changed = client.tools_changed();
    let r = client.call("add_tool", json!({}), &none).await.unwrap();
    assert_eq!(r.text, "added");
    tokio::time::timeout(Duration::from_secs(5), changed.changed()).await.unwrap().unwrap();
    assert!(client.tools().iter().any(|t| t.name == "mock_srv_extra"));
    assert_eq!(client.call("extra", json!({}), &none).await.unwrap().text, "extra!");

    // shutdown kills the process group, including grandchildren
    let r = client.call("spawn_child", json!({}), &none).await.unwrap();
    let pid: i32 = r.text.parse().unwrap();
    assert!(alive(pid));
    client.shutdown().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!alive(pid), "grandchild {pid} survived shutdown");
    let err = client.call("echo", json!({"text": "x"}), &none).await.unwrap_err();
    assert!(format!("{err:#}").contains("closed"), "{err:#}");
    assert!(client.closed_reason().is_some());
}

fn alive(pid: i32) -> bool {
    // zombie processes count as dead
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(s) => !s.rsplit(')').next().unwrap_or("").trim_start().starts_with('Z'),
        Err(_) => false,
    }
}

#[tokio::test]
async fn stdio_relative_cwd() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    let cfg = local(vec!["python3".into(), fixture().display().to_string()], Some("tests".into()));
    let client = hn_mcp::connect("m", &cfg, base).await.unwrap();
    let r = client.call("cwd", json!({}), &CancellationToken::new()).await.unwrap();
    assert_eq!(Path::new(&r.text).canonicalize().unwrap(), base.join("tests").canonicalize().unwrap());
    client.shutdown().await;
}

#[tokio::test]
async fn stdio_failures() {
    let tmp = std::env::temp_dir();
    // nonexistent binary
    let err = hn_mcp::connect("x", &local(vec!["/nonexistent/hn-mcp-bin".into()], None), &tmp).await.unwrap_err();
    assert!(format!("{err:#}").contains("failed to spawn"), "{err:#}");
    // empty command
    let err = hn_mcp::connect("x", &local(vec![], None), &tmp).await.unwrap_err();
    assert!(format!("{err:#}").contains("empty"), "{err:#}");
    // process exits immediately; stderr tail included
    let cmd = vec!["sh".into(), "-c".into(), "echo fatal-oops >&2; exit 3".into()];
    let err = hn_mcp::connect("x", &local(cmd, None), &tmp).await.unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("exited") && msg.contains("fatal-oops"), "{msg}");
    // never answers -> timeout
    let cfg = McpConfig::Local {
        command: vec!["sleep".into(), "30".into()],
        environment: IndexMap::new(),
        cwd: None,
        enabled: None,
        timeout: Some(300),
    };
    let start = std::time::Instant::now();
    let err = hn_mcp::connect("x", &cfg, &tmp).await.unwrap_err();
    assert!(start.elapsed() < Duration::from_secs(3));
    assert!(format!("{err:#}").contains("timed out"), "{err:#}");
    // disabled
    let cfg = McpConfig::Local { command: vec!["true".into()], environment: IndexMap::new(), cwd: None, enabled: Some(false), timeout: None };
    assert!(hn_mcp::connect("x", &cfg, &tmp).await.is_err());
    // garbage output only
    let cmd = vec!["sh".into(), "-c".into(), "cat >/dev/null & while true; do echo '{bad json'; echo '[1,2]'; echo '{\"id\":\"zz\"}'; sleep 0.05; done".into()];
    let cfg = McpConfig::Local { command: cmd, environment: IndexMap::new(), cwd: None, enabled: None, timeout: Some(500) };
    let err = hn_mcp::connect("x", &cfg, &tmp).await.unwrap_err();
    assert!(format!("{err:#}").contains("timed out"), "{err:#}");
}
