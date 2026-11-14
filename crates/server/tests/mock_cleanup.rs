//! `MockBackend::shutdown` lets `ref-server --mock` delete its temporary directory, which a
//! SIGKILL cannot.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use ref_client::MockBackend;
use tokio::io::{AsyncBufReadExt as _, BufReader};
use tokio_tungstenite::tungstenite::Message;
/// The variable `std::env::temp_dir()` reads.
const TEMP_DIR_VAR: &str = if cfg!(windows) { "TMP" } else { "TMPDIR" };
const MOCK_DIR_PREFIX: &str = "ref-mock-";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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

struct TempDirGuard {
    prev: Option<std::ffi::OsString>,
    dir: PathBuf,
}

impl TempDirGuard {
    fn new(prefix: &str) -> Self {
        let prev = std::env::var_os(TEMP_DIR_VAR);
        let dir = std::env::temp_dir().join(format!("{prefix}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: ENV_LOCK serializes every test in this file while mutating TEMP_DIR_VAR.
        unsafe { std::env::set_var(TEMP_DIR_VAR, &dir) };
        TempDirGuard { prev, dir }
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        // SAFETY: ENV_LOCK is held for the lifetime of this guard.
        unsafe {
            match &self.prev {
                Some(v) => std::env::set_var(TEMP_DIR_VAR, v),
                None => std::env::remove_var(TEMP_DIR_VAR),
            }
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn shutdown_removes_the_mock_directory() {
    let _lock = ENV_LOCK.lock().await;
    let guard = TempDirGuard::new("ref-mock-cleanup");
    let tmp = guard.dir.clone();
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
}

#[tokio::test]
async fn killed_mock_dir_is_cleaned_up_by_next_mock() {
    let _lock = ENV_LOCK.lock().await;
    let guard = TempDirGuard::new("ref-mock-stale-int");
    let tmp = guard.dir.clone();
    // Start first mock directly so we can SIGKILL it and wait for OS lock release.
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_ref-server"))
        .arg("--mock")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _stdin1 = child.stdin.take().unwrap();
    let mut stdout1 = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    tokio::time::timeout(CONNECT_TIMEOUT, stdout1.read_line(&mut line))
        .await
        .unwrap()
        .unwrap();
    assert!(!line.is_empty());
    let dirs = mock_dirs(&tmp);
    assert_eq!(dirs.len(), 1);
    let killed_dir = dirs[0].clone();

    child.kill().await.unwrap();
    let _ = child.wait().await;
    assert!(killed_dir.exists(), "SIGKILL leaves mock dir behind");
    assert_eq!(mock_dirs(&tmp), vec![killed_dir.clone()]);

    // Starting another mock removes the dead mock's dir and keeps its own intact.
    let mock2 = MockBackend::spawn_with(Path::new(env!("CARGO_BIN_EXE_ref-server")))
        .await
        .unwrap();
    let mut ws = ref_client::connect(mock2.endpoint(), CONNECT_TIMEOUT)
        .await
        .unwrap();
    ws.send(Message::text(hello_frame())).await.unwrap();
    ws.next().await.unwrap().unwrap();

    assert!(!killed_dir.exists(), "next mock should remove stale dir");
    let dirs_after = mock_dirs(&tmp);
    assert_eq!(dirs_after.len(), 1);
    assert_ne!(dirs_after[0], killed_dir);

    drop(ws);
    mock2.shutdown().await.unwrap();
    assert_eq!(mock_dirs(&tmp), Vec::<PathBuf>::new());
}
