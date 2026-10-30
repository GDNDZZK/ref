//! A private `ref-server --mock` child process.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use crate::{ConnectError, Endpoint, spawn};

const ANNOUNCE_TIMEOUT: Duration = Duration::from_secs(10);
/// How long `shutdown` lets the mock clean up before killing it.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

/// The one JSON line `ref-server --mock` prints once it listens.
#[derive(Deserialize)]
struct Announcement {
    port: u16,
    token: String,
}

/// A running `ref-server --mock`. It exits by itself when its stdin closes (we hold the write
/// end), so it never outlives a parent that died abruptly, and when its last client
/// disconnects.
///
/// Stop it with [`MockBackend::shutdown`]: the mock then deletes its temporary directory.
/// Dropping it kills the process, which leaves that directory behind.
pub struct MockBackend {
    endpoint: Endpoint,
    child: Child,
    _stdin: ChildStdin,
    /// Kept open: a closed pipe would make the server's writes fail.
    _stdout: BufReader<ChildStdout>,
}

impl MockBackend {
    /// Starts the `ref-server` next to the running executable in mock mode.
    pub async fn spawn() -> Result<MockBackend, ConnectError> {
        Self::spawn_with(&spawn::server_binary()).await
    }

    /// Starts the given `ref-server` binary in mock mode.
    pub async fn spawn_with(binary: &Path) -> Result<MockBackend, ConnectError> {
        let mut command = Command::new(binary);
        command.arg("--mock");
        Self::spawn_command(command).await
    }

    async fn spawn_command(mut command: Command) -> Result<MockBackend, ConnectError> {
        let program = command
            .as_std()
            .get_program()
            .to_string_lossy()
            .into_owned();
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| ConnectError::MockStartFailed(format!("{program}: {e}")))?;
        let stdin = child.stdin.take().expect("stdin is piped");
        let mut stdout = BufReader::new(child.stdout.take().expect("stdout is piped"));
        let mut line = String::new();
        let read = tokio::time::timeout(ANNOUNCE_TIMEOUT, stdout.read_line(&mut line)).await;
        match read {
            Err(_) => {
                return Err(ConnectError::MockStartFailed(format!(
                    "no port announced within {}s",
                    ANNOUNCE_TIMEOUT.as_secs()
                )));
            }
            Ok(Err(e)) => return Err(ConnectError::MockStartFailed(e.to_string())),
            Ok(Ok(0)) => {
                return Err(ConnectError::MockStartFailed(
                    "exited before announcing its port".into(),
                ));
            }
            Ok(Ok(_)) => {}
        }
        let announced: Announcement = serde_json::from_str(&line).map_err(|e| {
            ConnectError::MockStartFailed(format!("unexpected first line {:?}: {e}", line.trim()))
        })?;
        Ok(MockBackend {
            endpoint: Endpoint {
                url: format!("ws://127.0.0.1:{}/", announced.port),
                token: Some(announced.token),
            },
            child,
            _stdin: stdin,
            _stdout: stdout,
        })
    }

    /// Closes the mock's stdin and waits for it to exit, killing it if that takes too long.
    pub async fn shutdown(self) {
        let MockBackend {
            mut child,
            _stdin,
            _stdout,
            ..
        } = self;
        drop(_stdin);
        let exited = tokio::time::timeout(SHUTDOWN_TIMEOUT, child.wait()).await;
        if exited.is_err() {
            let _ = child.kill().await;
        }
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// True once the process has exited (for example after its last client left).
    pub fn has_exited(&mut self) -> std::io::Result<bool> {
        Ok(self.child.try_wait()?.is_some())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// A shell script standing in for `ref-server --mock`.
    fn fake_server(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        command
    }

    #[tokio::test]
    async fn reads_announcement_and_exits_when_stdin_closes() {
        let script = r#"echo '{"port":4242,"token":"secret"}'; cat >/dev/null"#;
        let mut mock = MockBackend::spawn_command(fake_server(script))
            .await
            .unwrap();
        assert_eq!(mock.endpoint().url, "ws://127.0.0.1:4242/");
        assert_eq!(mock.endpoint().token.as_deref(), Some("secret"));
        assert!(!mock.has_exited().unwrap());

        let MockBackend {
            mut child, _stdin, ..
        } = mock;
        drop(_stdin);
        let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .expect("child exits when stdin closes")
            .unwrap();
        assert!(status.success());
    }

    #[tokio::test]
    async fn kills_child_on_drop() {
        let script = r#"echo '{"port":1,"token":"t"}'; exec sleep 60"#;
        let mock = MockBackend::spawn_command(fake_server(script))
            .await
            .unwrap();
        let pid = mock.child.id().unwrap();
        drop(mock);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while ref_sys::pid_alive(pid) && std::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(!ref_sys::pid_alive(pid));
    }

    #[tokio::test]
    async fn reports_exit_before_announcement() {
        let err = MockBackend::spawn_command(fake_server("exit 3"))
            .await
            .err()
            .unwrap();
        assert!(matches!(err, ConnectError::MockStartFailed(m) if m.contains("before announcing")));
    }

    #[tokio::test]
    async fn reports_garbage_first_line() {
        let err = MockBackend::spawn_command(fake_server("echo hello; exec sleep 5"))
            .await
            .err()
            .unwrap();
        assert!(matches!(err, ConnectError::MockStartFailed(m) if m.contains("hello")));
    }

    #[tokio::test]
    async fn reports_missing_binary() {
        let err = MockBackend::spawn_command(Command::new("/nonexistent/ref-server"))
            .await
            .err()
            .unwrap();
        assert!(matches!(err, ConnectError::MockStartFailed(m) if m.contains("/nonexistent")));
    }
}
