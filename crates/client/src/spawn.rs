//! Starting a shared backend.

use std::path::{Path, PathBuf};
use std::time::Duration;

use ref_proto::InstanceInfo;

use crate::instance;

const SPAWN_LOCK_FILE: &str = ".spawn.lock";
const SPAWN_LOCK_POLL: Duration = Duration::from_millis(50);
const INSTANCE_POLL: Duration = Duration::from_millis(50);
const PORT_RANGE: std::ops::RangeInclusive<u16> = 10000..=65535;
const PORT_ATTEMPTS: u32 = 200;

/// Random free port (bind-tested on 127.0.0.1).
pub(crate) fn pick_port() -> Option<u16> {
    use rand::Rng;
    let mut rng = rand::rng();
    for _ in 0..PORT_ATTEMPTS {
        let p: u16 = rng.random_range(PORT_RANGE);
        if std::net::TcpListener::bind((ref_proto::LOOPBACK_V4, p)).is_ok() {
            return Some(p);
        }
    }
    None
}

/// `ref-server` next to the running executable, else the one on PATH.
pub(crate) fn server_binary() -> PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let sib = dir.join(format!("ref-server{}", std::env::consts::EXE_SUFFIX));
        if sib.is_file() {
            return sib;
        }
    }
    PathBuf::from("ref-server")
}

/// Spawns a detached backend on `port`.
pub(crate) fn spawn_server(port: u16) -> std::io::Result<()> {
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(server_binary());
    let idle = ref_proto::IDLE_EXIT_NOW.to_string();
    cmd.args(["--port", &port.to_string(), "--idle-timeout", &idle])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    ref_sys::detach(&mut cmd);
    let mut child = cmd.spawn()?;
    // Reap it if it exits while we're alive.
    std::thread::spawn(move || {
        // Only reaping matters; the exit status of a detached backend is nobody's business.
        let _ = child.wait();
    });
    Ok(())
}

/// Exclusive advisory lock serialising backend spawns across frontends (released on drop).
/// Without it, every frontend that loses its backend at the same moment spawns its own.
pub(crate) struct SpawnLock {
    _file: std::fs::File,
}

/// Why a [`SpawnLock`] could not be taken.
#[derive(Debug)]
pub(crate) enum SpawnLockError {
    /// Another frontend held the lock for the whole wait.
    Timeout,
    /// The directory, the lock file or the lock itself failed.
    Io(std::io::Error),
}

impl SpawnLock {
    /// Acquires `<dir>/.spawn.lock`, polling until `timeout`.
    pub(crate) async fn acquire(
        dir: &Path,
        timeout: Duration,
    ) -> Result<SpawnLock, SpawnLockError> {
        std::fs::create_dir_all(dir).map_err(SpawnLockError::Io)?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join(SPAWN_LOCK_FILE))
            .map_err(SpawnLockError::Io)?;
        let start = std::time::Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(SpawnLock { _file: file }),
                Err(std::fs::TryLockError::WouldBlock) => {}
                Err(std::fs::TryLockError::Error(e)) => return Err(SpawnLockError::Io(e)),
            }
            if start.elapsed() > timeout {
                return Err(SpawnLockError::Timeout);
            }
            tokio::time::sleep(SPAWN_LOCK_POLL).await;
        }
    }
}

/// Waits for an instance file announcing `port` (up to `timeout`).
pub(crate) async fn wait_for_instance(
    dir: &Path,
    port: u16,
    timeout: Duration,
) -> Option<InstanceInfo> {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if let Some(i) = instance::instances(dir)
            .into_iter()
            .find(|i| i.port == port)
        {
            return Some(i);
        }
        tokio::time::sleep(INSTANCE_POLL).await;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn spawn_lock_is_exclusive() {
        let dir = std::env::temp_dir().join(format!("ref-client-lock-{}", std::process::id()));
        let a = SpawnLock::acquire(&dir, Duration::from_millis(100)).await;
        assert!(a.is_ok());
        // The lock is per open file: a second open in the same process conflicts too.
        assert!(
            SpawnLock::acquire(&dir, Duration::from_millis(120))
                .await
                .is_err()
        );
        drop(a);
        assert!(
            SpawnLock::acquire(&dir, Duration::from_millis(100))
                .await
                .is_ok()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn port_pick() {
        let p = pick_port().unwrap();
        assert!(*PORT_RANGE.start() <= p);
    }
}
