//! `MockBackend::shutdown` lets `ref-server --mock` delete its temporary directory, which a
//! SIGKILL cannot.

use std::path::{Path, PathBuf};

use futures_util::{SinkExt as _, StreamExt as _};
use ref_client::MockBackend;
use tokio_tungstenite::tungstenite::Message;

const MOCK_DIR_PREFIX: &str = "ref-mock-";
const HELLO: &str = r#"{"id":1,"op":"hello","client":"cleanup-test","version":2,"cwd":"/tmp"}"#;

fn mock_dirs(tmp: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(tmp)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with(MOCK_DIR_PREFIX))
        })
        .collect()
}

#[tokio::test]
async fn shutdown_removes_the_mock_directory() {
    let tmp = std::env::temp_dir().join(format!("ref-mock-cleanup-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    // SAFETY: this file holds a single test, so nothing else reads or writes the environment
    // concurrently. The mock inherits TMPDIR and creates its directory there.
    unsafe { std::env::set_var("TMPDIR", &tmp) };

    let mock = MockBackend::spawn_with(Path::new(env!("CARGO_BIN_EXE_ref-server")))
        .await
        .unwrap();
    // A connected client keeps the mock running until we stop it.
    let mut ws = ref_client::connect(mock.endpoint(), std::time::Duration::from_secs(10))
        .await
        .unwrap();
    ws.send(Message::text(HELLO)).await.unwrap();
    ws.next().await.unwrap().unwrap();
    assert_eq!(mock_dirs(&tmp).len(), 1);

    mock.shutdown().await;

    assert_eq!(mock_dirs(&tmp), Vec::<PathBuf>::new());
    drop(ws);
    let _ = std::fs::remove_dir_all(&tmp);
}
