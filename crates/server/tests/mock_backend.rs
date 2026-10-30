//! `MockBackend` against the real `ref-server --mock`.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use ref_client::{MockBackend, Ws};
use tokio::io::{AsyncBufReadExt as _, BufReader};
use tokio_tungstenite::tungstenite::Message;

const STEP: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(20);
const HELLO: &str = r#"{"id":1,"op":"hello","client":"ref-client-test","version":2,"cwd":"/tmp"}"#;

fn ref_server_binary() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_ref-server"))
}

async fn hello(ws: &mut Ws) -> serde_json::Value {
    ws.send(Message::text(HELLO)).await.unwrap();
    loop {
        let message = tokio::time::timeout(STEP, ws.next())
            .await
            .expect("no response")
            .expect("socket ended")
            .expect("socket error");
        let Message::Text(text) = message else {
            continue;
        };
        let frame: serde_json::Value = serde_json::from_str(text.as_str()).unwrap();
        if frame["id"] == 1 {
            return frame;
        }
    }
}

#[tokio::test]
async fn announces_an_endpoint_that_accepts_its_token() {
    let mock = MockBackend::spawn_with(ref_server_binary()).await.unwrap();
    let mut ws = ref_client::connect(mock.endpoint(), STEP).await.unwrap();
    let response = hello(&mut ws).await;
    assert_eq!(response["ok"], true, "{response}");
}

#[tokio::test]
async fn exits_when_its_last_client_disconnects() {
    let mut mock = MockBackend::spawn_with(ref_server_binary()).await.unwrap();
    let mut ws = ref_client::connect(mock.endpoint(), STEP).await.unwrap();
    hello(&mut ws).await;
    assert!(!mock.has_exited().unwrap());
    drop(ws);

    let deadline = std::time::Instant::now() + STEP;
    while !mock.has_exited().unwrap() {
        assert!(
            std::time::Instant::now() < deadline,
            "the mock stayed up after its last client left"
        );
        tokio::time::sleep(POLL).await;
    }
}

/// The contract `MockBackend` relies on, checked on the bare process: one JSON line on stdout,
/// and exit when stdin closes even though no client ever connected.
#[tokio::test]
async fn exits_when_stdin_closes_and_prints_only_the_announcement() {
    let mut child = tokio::process::Command::new(ref_server_binary())
        .arg("--mock")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let line = tokio::time::timeout(STEP, lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .expect("an announcement line");
    let announcement: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert!(
        announcement["port"].as_u64().is_some_and(|p| p > 0),
        "{line}"
    );
    assert!(
        announcement["token"]
            .as_str()
            .is_some_and(|t| !t.is_empty()),
        "{line}"
    );

    // Stays up while stdin is open.
    assert!(
        tokio::time::timeout(Duration::from_millis(500), child.wait())
            .await
            .is_err()
    );
    drop(stdin);
    let status = tokio::time::timeout(STEP, child.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(status.success(), "{status}");
    let rest = tokio::time::timeout(STEP, lines.next_line())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(rest, None, "nothing else on stdout");
}
