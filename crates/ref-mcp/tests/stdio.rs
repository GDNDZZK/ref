use std::collections::BTreeMap;
use std::time::Duration;

use ref_mcp::{McpManager, McpServerConfig};
use serde_json::json;
use tokio_util::sync::CancellationToken;

fn script() -> String {
    format!("{}/tests/fake_server.py", env!("CARGO_MANIFEST_DIR"))
}

fn local(env: &[(&str, &str)]) -> McpServerConfig {
    McpServerConfig::Local {
        command: vec!["/usr/bin/python3".into(), "-u".into(), script()],
        environment: env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        cwd: None,
        enabled: None,
        timeout: Some(10_000),
    }
}

fn servers(list: Vec<(&str, McpServerConfig)>) -> BTreeMap<String, McpServerConfig> {
    list.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

const T: Duration = Duration::from_secs(10);

fn log_path(tag: &str) -> String {
    let p = std::env::temp_dir().join(format!("ref-mcp-test-{}-{tag}.log", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p.to_string_lossy().into_owned()
}

#[tokio::test]
async fn stdio_full_flow() {
    let log = log_path("flow");
    let mgr = McpManager::start(servers(vec![
        ("fake srv", local(&[("FAKE_LOG", &log)])),
        ("off", McpServerConfig::Remote { url: "http://127.0.0.1:1/".into(), headers: BTreeMap::new(), enabled: Some(false), timeout: None }),
        ("broken", local(&[("FAKE_INIT_FAIL", "1")])),
    ]))
    .await;

    let status = mgr.status();
    let by = |n: &str| status.iter().find(|s| s.name == n).unwrap().clone();
    assert_eq!(by("fake srv").status, "connected", "{status:?}");
    assert_eq!(by("fake srv").tools, 7);
    assert_eq!(by("off").status, "disabled");
    let broken = by("broken");
    assert_eq!(broken.status, "failed");
    assert!(broken.error.as_deref().unwrap().contains("missing API key"), "{broken:?}");

    let tools = mgr.tools();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        names,
        ["fake_srv_change", "fake_srv_crash", "fake_srv_echo", "fake_srv_fail", "fake_srv_img", "fake_srv_ping_me", "fake_srv_slow"]
    );
    let echo = tools.iter().find(|t| t.tool == "echo").unwrap();
    assert_eq!(echo.def.description, "Echo text");
    assert_eq!(echo.def.parameters["properties"]["text"]["type"], "string");
    let img = tools.iter().find(|t| t.tool == "img").unwrap();
    assert_eq!(img.def.description, "Image tool");
    assert_eq!(img.def.parameters, json!({"type": "object", "properties": {}}));
    assert_eq!(tools.iter().find(|t| t.tool == "fail").unwrap().def.description, "fail");

    let r = mgr.call("fake_srv_echo", json!({"text": "hi"}), CancellationToken::new(), T).await;
    assert_eq!(r.text, "echo: hi");
    assert!(!r.is_error);

    let r = mgr.call("fake_srv_img", json!({}), CancellationToken::new(), T).await;
    assert_eq!(r.text, "here");
    assert_eq!(r.images.len(), 1);
    assert_eq!(r.images[0].mime, "image/png");
    assert_eq!(r.images[0].data, "iVBORw0KGgo=");

    let r = mgr.call("fake_srv_fail", json!({}), CancellationToken::new(), T).await;
    assert!(r.is_error);
    assert_eq!(r.text, "bad thing");

    let r = mgr.call("nope", json!({}), CancellationToken::new(), T).await;
    assert!(r.is_error);
    assert!(r.text.contains("Unknown MCP tool"));

    // server → client requests: ping answered, sampling rejected with -32601.
    let r = mgr.call("fake_srv_ping_me", json!({}), CancellationToken::new(), T).await;
    assert_eq!(r.text, "pong ok; sampling -32601");

    // Timeout → notifications/cancelled.
    let r = mgr.call("fake_srv_slow", json!({"secs": 5}), CancellationToken::new(), Duration::from_millis(300)).await;
    assert!(r.is_error);
    assert!(r.text.contains("timed out"), "{}", r.text);

    // Abort → notifications/cancelled with reason.
    let abort = CancellationToken::new();
    let a2 = abort.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        a2.cancel();
    });
    let r = mgr.call("fake_srv_slow", json!({"secs": 5}), abort, T).await;
    assert!(r.is_error);
    assert!(r.text.contains("aborted"), "{}", r.text);

    // list_changed → re-list.
    let r = mgr.call("fake_srv_change", json!({}), CancellationToken::new(), T).await;
    assert_eq!(r.text, "changed");
    let mut ok = false;
    for _ in 0..50 {
        if mgr.tools().iter().any(|t| t.name == "fake_srv_added") {
            ok = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(ok, "tool list not refreshed");

    tokio::time::sleep(Duration::from_millis(200)).await;
    let logged = std::fs::read_to_string(&log).unwrap();
    let lines: Vec<&str> = logged.lines().collect();
    assert_eq!(lines[0], "initialize");
    assert_eq!(lines[1], "notifications/initialized (notification)");
    assert_eq!(lines[2], "tools/list");
    assert_eq!(lines[3], "tools/list");
    let cancels: Vec<&&str> = lines.iter().filter(|l| l.starts_with("notifications/cancelled")).collect();
    assert_eq!(cancels.len(), 2, "{logged}");
    assert!(cancels[1].contains("cancelled by user"), "{logged}");

    mgr.shutdown().await;
    let r = mgr.call("fake_srv_echo", json!({"text": "hi"}), CancellationToken::new(), T).await;
    assert!(r.is_error);
    let _ = std::fs::remove_file(&log);
}

#[tokio::test]
async fn stdio_crash_and_reconnect() {
    let log = log_path("crash");
    let mgr = McpManager::start(servers(vec![("s", local(&[("FAKE_LOG", &log)]))])).await;
    assert_eq!(mgr.status()[0].status, "connected");

    let r = mgr.call("s_crash", json!({}), CancellationToken::new(), T).await;
    assert!(r.is_error);
    assert!(r.text.contains("crashing on purpose"), "{}", r.text);

    // Watcher marks it failed with the stderr tail.
    let mut st = mgr.status();
    for _ in 0..50 {
        if st[0].status == "failed" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        st = mgr.status();
    }
    assert_eq!(st[0].status, "failed");
    let err = st[0].error.clone().unwrap();
    assert!(err.contains("exited") && err.contains("crashing on purpose"), "{err}");
    // Tools stay listed so the model's tool list remains stable.
    assert_eq!(mgr.tools().len(), 7);

    // Next call lazily reconnects.
    let r = mgr.call("s_echo", json!({"text": "back"}), CancellationToken::new(), T).await;
    assert_eq!(r.text, "echo: back", "{}", r.text);
    assert_eq!(mgr.status()[0].status, "connected");
    mgr.shutdown().await;
    let _ = std::fs::remove_file(&log);
}

#[tokio::test]
async fn spawn_failure_and_restart() {
    let bad = McpServerConfig::Local {
        command: vec!["/nonexistent/binary-xyz".into()],
        environment: BTreeMap::new(),
        cwd: None,
        enabled: None,
        timeout: Some(2000),
    };
    let mgr = McpManager::start(servers(vec![("bad", bad.clone()), ("a", local(&[]))])).await;
    let st = mgr.status();
    assert_eq!(st[0].name, "a");
    assert_eq!(st[0].status, "connected");
    assert_eq!(st[1].status, "failed");
    assert!(st[1].error.as_deref().unwrap().contains("failed to spawn"));
    let r = mgr.call("a_echo", json!({"text": "1"}), CancellationToken::new(), T).await;
    assert_eq!(r.text, "echo: 1");

    // Restart: keep "a", drop "bad", add "b".
    mgr.restart(servers(vec![("a", local(&[])), ("b", local(&[]))])).await;
    let st = mgr.status();
    assert_eq!(st.len(), 2);
    assert!(st.iter().all(|s| s.status == "connected"), "{st:?}");
    let r = mgr.call("b_echo", json!({"text": "2"}), CancellationToken::new(), T).await;
    assert_eq!(r.text, "echo: 2");

    // Disable "a".
    let mut off = local(&[]);
    if let McpServerConfig::Local { enabled, .. } = &mut off {
        *enabled = Some(false);
    }
    mgr.restart(servers(vec![("a", off), ("b", local(&[]))])).await;
    let st = mgr.status();
    assert_eq!(st[0].status, "disabled");
    assert!(mgr.tools().iter().all(|t| t.server == "b"));
    let r = mgr.call("a_echo", json!({"text": "1"}), CancellationToken::new(), T).await;
    assert!(r.is_error);
    mgr.shutdown().await;
}
