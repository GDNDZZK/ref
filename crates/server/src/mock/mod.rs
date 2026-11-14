//! Mock mode (`ref-server --mock`): a complete backend with a scripted "LLM", for developing
//! and testing frontends without a provider account.
//!
//! ```text
//!  frontend --ws--> Hub --chat completions over loopback HTTP--> llm.rs --> scripts/
//!                    |
//!                    +-- everything lives in one temporary directory (see `Mock::root`)
//! ```
//!
//! Nothing outside that directory is read or written: no user or project configuration, no
//! real working directories (see `HubOptions::sandbox_cwd`), no real session history.
//!
//! Known limits, not enforced: `Export` writes to a path the client chooses, and `Shell`
//! (`!command` in a frontend) runs a real command; both can reach outside the workspace.
//! Tools called by the scripts only touch the workspace.

mod attempts;
mod config;
mod history;
mod llm;
mod png;
mod script;
mod scripts;
mod workspace;

use crate::hub::DEFAULT_RETRY_BASE;
use crate::{HubOptions, Running, ws};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// The mock exits as soon as its last client leaves unless told otherwise.
pub const DEFAULT_IDLE_TIMEOUT: i64 = ref_proto::IDLE_EXIT_NOW;
const ROOT_PREFIX: &str = "ref-mock-";
const ROOT_SUFFIX_LEN: usize = 8;
const LOG_FILE: &str = "server.log";
const LOCK_FILE: &str = ".lock";

/// A running mock backend. Dropping it deletes its temporary directory.
pub struct Mock {
    pub running: Running,
    root: PathBuf,
    _lock: Option<std::fs::File>,
}
impl Mock {
    pub fn log_path(&self) -> PathBuf {
        self.root.join("logs").join(LOG_FILE)
    }

    /// The working directory every session gets.
    pub fn workspace(&self) -> PathBuf {
        self.root.join("workspace")
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl Drop for Mock {
    fn drop(&mut self) {
        // The hub outlives this drop and its session database is open in the directory.
        if let Err(e) = self.running.hub.store.close() {
            eprintln!("ref-server: cannot close the session store: {e}");
        }
        // Release the lock handle before removing the directory (required on Windows).
        let _ = self._lock.take();
        if let Err(e) = std::fs::remove_dir_all(&self.root) {
            eprintln!("ref-server: cannot remove {}: {e}", self.root.display());
        }
    }
}

/// Creates `<tmp>/ref-mock-XXXXXXXX/` and everything in it, and starts listening on
/// 127.0.0.1:`port` (0: any free port).
pub async fn start(port: u16, token: String) -> anyhow::Result<Mock> {
    start_with(port, token, DEFAULT_IDLE_TIMEOUT).await
}

/// Like [`start`], with the idle timeout of the hub (`--idle-timeout`: -1 keeps the mock running
/// after its last client leaves).
pub async fn start_with(port: u16, token: String, idle_timeout: i64) -> anyhow::Result<Mock> {
    start_all(port, token, idle_timeout, DEFAULT_RETRY_BASE).await
}

/// Like [`start`], with a shorter base delay of the retry backoff (2 s in production), so tests
/// of `outage` need not wait 30 seconds.
pub async fn start_with_retry_base(retry_base: Duration) -> anyhow::Result<Mock> {
    start_all(0, "token".into(), DEFAULT_IDLE_TIMEOUT, retry_base).await
}

async fn start_all(
    port: u16,
    token: String,
    idle_timeout: i64,
    retry_base: Duration,
) -> anyhow::Result<Mock> {
    let tmp = std::env::temp_dir();
    remove_stale_dirs(&tmp);
    let name = format!("{ROOT_PREFIX}{}", crate::util::random_hex(ROOT_SUFFIX_LEN));
    let root = tmp.join(name);
    std::fs::create_dir(&root)?;
    let lock = match acquire_lock(&root) {
        Ok(f) => f,
        Err(e) => {
            if let Err(clean) = std::fs::remove_dir_all(&root) {
                eprintln!("ref-server: cannot remove {}: {clean}", root.display());
            }
            return Err(e.into());
        }
    };
    match start_in(&root, port, token, idle_timeout, retry_base).await {
        Ok(running) => Ok(Mock {
            running,
            root,
            _lock: Some(lock),
        }),
        Err(e) => {
            drop(lock);
            // The start error is the one to report; a failed cleanup is only logged.
            if let Err(clean) = std::fs::remove_dir_all(&root) {
                eprintln!("ref-server: cannot remove {}: {clean}", root.display());
            }
            Err(e)
        }
    }
}

fn acquire_lock(root: &Path) -> std::io::Result<std::fs::File> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(root.join(LOCK_FILE))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(std::fs::TryLockError::WouldBlock) => Err(std::io::Error::new(
            std::io::ErrorKind::WouldBlock,
            "mock lock already held",
        )),
        Err(std::fs::TryLockError::Error(e)) => Err(e),
    }
}

fn is_mock_dir_name(name: &str) -> bool {
    let Some(hex) = name.strip_prefix(ROOT_PREFIX) else {
        return false;
    };
    hex.len() == ROOT_SUFFIX_LEN && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

pub(crate) fn remove_stale_dirs(temp_dir: &Path) {
    let entries = match std::fs::read_dir(temp_dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::debug!("cannot read temp dir {}: {e}", temp_dir.display());
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                tracing::debug!("cannot read temp dir entry in {}: {e}", temp_dir.display());
                continue;
            }
        };
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if !is_mock_dir_name(name_str) {
            continue;
        }
        let path = entry.path();
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                tracing::debug!("cannot stat mock dir {}: {e}", path.display());
                continue;
            }
        };
        if !meta.file_type().is_dir() || meta.file_type().is_symlink() {
            continue;
        }
        let lock_path = path.join(LOCK_FILE);
        let lock_meta = match std::fs::symlink_metadata(&lock_path) {
            Ok(m) => m,
            Err(e) => {
                tracing::debug!(
                    "skipping mock dir {} without {LOCK_FILE}: {e}",
                    path.display()
                );
                continue;
            }
        };
        if !lock_meta.file_type().is_file() || lock_meta.file_type().is_symlink() {
            continue;
        }
        let file = match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&lock_path)
        {
            Ok(f) => f,
            Err(e) => {
                tracing::debug!("cannot open {}: {e}", lock_path.display());
                continue;
            }
        };
        match file.try_lock() {
            Ok(()) => {
                drop(file);
                if let Err(e) = std::fs::remove_dir_all(&path) {
                    tracing::debug!("cannot remove stale mock dir {}: {e}", path.display());
                }
            }
            Err(std::fs::TryLockError::WouldBlock) => {}
            Err(std::fs::TryLockError::Error(e)) => {
                tracing::debug!("cannot lock {}: {e}", lock_path.display());
            }
        }
    }
}

async fn start_in(
    root: &Path,
    port: u16,
    token: String,
    idle_timeout: i64,
    retry_base: Duration,
) -> anyhow::Result<Running> {
    let workspace = root.join("workspace");
    std::fs::create_dir_all(root.join("logs"))?;
    std::fs::create_dir_all(&workspace)?;
    let workspace = dunce::canonicalize(workspace)?;
    workspace::create(&workspace)?;

    let llm = llm::start().await?;
    let cfg = config::load(&root.join("config"), &workspace, llm.port)?;

    let data_dir = root.join("data");
    std::fs::create_dir_all(&data_dir)?;
    let store = crate::store::Store::new(data_dir.clone());
    history::save(&store, &cfg, &workspace.to_string_lossy()).map_err(anyhow::Error::msg)?;
    drop(store);

    let mut opts = HubOptions::new(data_dir);
    opts.home = None;
    opts.config_loader = Arc::new(move |_| cfg.clone());
    opts.idle_timeout = idle_timeout;
    opts.retry_base = retry_base;
    opts.sandbox_cwd = Some(workspace);
    let allow = vec![ref_proto::LOOPBACK_V4.to_string()];
    let (auth, _) = ws::resolve_hosts(&allow, token);
    crate::start(opts, ref_proto::LOOPBACK_V4, port, auth).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_stale_dirs_keeps_live_unlocked_no_lock_non_matching_and_symlinks() {
        let base = std::env::temp_dir().join(format!("ref-mock-stale-unit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        // (a) matching name, .lock held -> kept
        let held_dir = base.join("ref-mock-01234567");
        std::fs::create_dir(&held_dir).unwrap();
        let held_lock = acquire_lock(&held_dir).unwrap();

        // (b) matching name, .lock present and unlocked -> removed
        let stale_dir = base.join("ref-mock-abcdef89");
        std::fs::create_dir(&stale_dir).unwrap();
        let stale_lock = acquire_lock(&stale_dir).unwrap();
        drop(stale_lock);

        // (c) matching name without .lock -> kept
        let no_lock_dir = base.join("ref-mock-deadbeef");
        std::fs::create_dir(&no_lock_dir).unwrap();

        // (d) non-matching names -> kept
        let cfg_dir = base.join("ref-mock-cfg-12345678");
        std::fs::create_dir(&cfg_dir).unwrap();
        let _cfg_lock = acquire_lock(&cfg_dir).unwrap();
        let other_dir = base.join("other-dir");
        std::fs::create_dir(&other_dir).unwrap();
        let upper_dir = base.join("ref-mock-ABCDEF12");
        std::fs::create_dir(&upper_dir).unwrap();
        let _upper_lock = acquire_lock(&upper_dir).unwrap();

        // (e) symlink named like a mock directory -> kept (target also untouched)
        #[cfg(unix)]
        let symlink_dir = {
            let target = base.join("symlink-target");
            std::fs::create_dir(&target).unwrap();
            let _target_lock = acquire_lock(&target).unwrap();
            drop(_target_lock);
            let link = base.join("ref-mock-11223344");
            std::os::unix::fs::symlink(&target, &link).unwrap();
            (link, target)
        };

        remove_stale_dirs(&base);

        assert!(held_dir.exists());
        assert!(!stale_dir.exists());
        assert!(no_lock_dir.exists());
        assert!(cfg_dir.exists());
        assert!(other_dir.exists());
        assert!(upper_dir.exists());
        #[cfg(unix)]
        {
            assert!(symlink_dir.0.symlink_metadata().is_ok());
            assert!(symlink_dir.1.exists());
        }

        drop(held_lock);
        let _ = std::fs::remove_dir_all(&base);
    }
}
