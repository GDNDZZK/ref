//! `MockBackend::shutdown` lets `ref-server --mock` delete its temporary directory, which a
//! SIGKILL cannot.

use std::path::{Path, PathBuf};
use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use ref_client::MockBackend;
use tokio_tungstenite::tungstenite::Message;

/// The variable `std::env::temp_dir()` reads.
const TEMP_DIR_VAR: &str = if cfg!(windows) { "TMP" } else { "TMPDIR" };
const MOCK_DIR_PREFIX: &str = "ref-mock-";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

fn hello_frame() -> String {
    format!(
        r#"{{"id":1,"op":"hello","client":"cleanup-test","version":{},"cwd":"/tmp"}}"#,
        ref_proto::PROTOCOL_VERSION
    )
}

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
    // concurrently. The mock inherits the variable and creates its directory there.
    unsafe { std::env::set_var(TEMP_DIR_VAR, &tmp) };

    let mock = MockBackend::spawn_with(Path::new(env!("CARGO_BIN_EXE_ref-server")))
        .await
        .unwrap();
    // A connected client keeps the mock running until we stop it.
    let mut ws = ref_client::connect(mock.endpoint(), CONNECT_TIMEOUT)
        .await
        .unwrap();
    ws.send(Message::text(hello_frame())).await.unwrap();
    ws.next().await.unwrap().unwrap();
    assert_eq!(mock_dirs(&tmp).len(), 1);

    mock.shutdown().await.unwrap();

    assert_eq!(mock_dirs(&tmp), Vec::<PathBuf>::new());
    drop(ws);
    let _ = std::fs::remove_dir_all(&tmp);
}
