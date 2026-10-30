//! ref-web against the real `ref-server --mock` and the real shared backend.

mod common;

use common::*;
use futures_util::SinkExt as _;
use tokio_tungstenite::tungstenite::Message;

const HELLO: &str = r#"{"id":1,"op":"hello","client":"ref-web-test","version":2,"cwd":"/tmp"}"#;
const MARKER_SESSION: &str = "tab-one-marker";

/// Sends Hello and returns the response and the live session names of the Sessions frame.
async fn hello(ws: &mut Ws) -> (serde_json::Value, Vec<String>) {
    ws.send(Message::text(HELLO)).await.unwrap();
    let mut response = None;
    let mut sessions = None;
    while response.is_none() || sessions.is_none() {
        let frame: serde_json::Value = serde_json::from_str(&next_text(ws).await).unwrap();
        match frame["kind"].as_str() {
            Some("response") if frame["id"] == 1 => response = Some(frame),
            Some("sessions") => {
                let names = frame["sessions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|s| s["name"].as_str().unwrap().to_string())
                    .collect();
                sessions = Some(names);
            }
            _ => {}
        }
    }
    (response.unwrap(), sessions.unwrap())
}

async fn create_session(ws: &mut Ws, name: &str) {
    let request = format!(r#"{{"id":2,"op":"create_session","cwd":"/tmp","name":"{name}"}}"#);
    ws.send(Message::text(request)).await.unwrap();
    loop {
        let frame: serde_json::Value = serde_json::from_str(&next_text(ws).await).unwrap();
        if frame["id"] == 2 {
            assert_eq!(frame["ok"], true, "{frame}");
            return;
        }
    }
}

#[tokio::test]
async fn mock_mode_serves_tabs_from_a_private_backend() {
    require_ref_server();
    let env = Env::new("mock");
    let web = env.start(&["--mock"]).await;
    let info: serde_json::Value = web.get("/api/info").await.json().await.unwrap();
    assert_eq!(info["mock"], true);

    let mut ws = web.ws().await;
    let (response, _) = hello(&mut ws).await;
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["result"]["protocol"], ref_proto::PROTOCOL_VERSION);
    // Nothing is registered for other frontends to find.
    assert!(!env.state_dir().join("instances").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn a_mock_that_exited_is_started_again_for_the_next_tab() {
    require_ref_server();
    let env = Env::new("mock-restart");
    let web = env.start(&["--mock"]).await;

    let mut first_tab = web.ws().await;
    hello(&mut first_tab).await;
    create_session(&mut first_tab, MARKER_SESSION).await;
    let first_mock = child_pids(web.pid());
    assert_eq!(
        first_mock.len(),
        1,
        "ref-web started one mock: {first_mock:?}"
    );

    // The mock exits when its last client leaves.
    drop(first_tab);
    wait_until("the first mock to exit", || process_gone(first_mock[0])).await;

    let mut second_tab = web.ws().await;
    let (response, sessions) = hello(&mut second_tab).await;
    assert_eq!(response["ok"], true, "{response}");
    assert!(
        !sessions.iter().any(|n| n == MARKER_SESSION),
        "{sessions:?}"
    );
    let second_mock: Vec<u32> = child_pids(web.pid())
        .into_iter()
        .filter(|pid| !process_gone(*pid))
        .collect();
    assert_eq!(second_mock.len(), 1, "{second_mock:?}");
    assert_ne!(second_mock[0], first_mock[0], "a new mock process");
}

#[cfg(unix)]
#[tokio::test]
async fn the_mock_does_not_outlive_ref_web() {
    require_ref_server();
    let env = Env::new("mock-orphan");
    let mut web = env.start(&["--mock"]).await;
    let mut tab = web.ws().await;
    hello(&mut tab).await;
    let mock = child_pids(web.pid());
    assert_eq!(mock.len(), 1, "{mock:?}");

    // SIGKILL leaves no chance to stop the child: it notices that its stdin closed.
    web.kill_hard();
    wait_until("the mock to exit", || process_gone(mock[0])).await;
}

#[cfg(unix)]
#[tokio::test]
async fn without_an_explicit_server_ref_web_starts_the_backend_beside_it() {
    require_ref_server();
    let env = Env::new("auto");
    let web = env.start(&[]).await;
    let mut tab = web.ws().await;
    let (response, _) = hello(&mut tab).await;
    assert_eq!(response["ok"], true, "{response}");
    // The shared backend announces itself in the state directory (ours is private).
    let backend = backend_pid(&env);
    // It exits after its last client leaves (started with --idle-timeout 0).
    drop(tab);
    wait_until("the backend to exit", || process_gone(backend)).await;
}

/// The pid in the one instance file of the private state directory.
#[cfg(unix)]
fn backend_pid(env: &Env) -> u32 {
    let files: Vec<_> = std::fs::read_dir(env.state_dir().join("instances"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "json"))
        .collect();
    assert_eq!(files.len(), 1, "{files:?}");
    let info: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&files[0]).unwrap()).unwrap();
    info["pid"].as_u64().unwrap() as u32
}

#[cfg(unix)]
#[tokio::test]
async fn stopping_ref_web_stops_the_mock_and_removes_its_directory() {
    require_ref_server();
    let env = Env::new("mock-cleanup");
    let mut web = env.start(&["--mock"]).await;
    let mut tab = web.ws().await;
    hello(&mut tab).await;
    let mock_dirs = |env: &Env| {
        std::fs::read_dir(env.tmp_dir())
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("ref-mock-")
            })
            .count()
    };
    assert_eq!(mock_dirs(&env), 1);

    assert!(ref_sys::terminate(web.pid()));
    let status = web.wait_exit().await;
    assert!(status.success(), "{status}");
    assert_eq!(
        mock_dirs(&env),
        0,
        "the mock removed its temporary directory"
    );
}
