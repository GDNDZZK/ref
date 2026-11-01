//! Records of the running ref-web instances: `<state dir>/web/instances/<pid>.json`.
//!
//! A second start reads them to find the instance it should open instead of starting
//! another one. A record holds no secret.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

use crate::token;

const INSTANCES_DIR: &str = "instances";
const RECORD_EXTENSION: &str = "json";
#[cfg(unix)]
const PRIVATE_DIR_MODE: u32 = 0o700;
#[cfg(unix)]
const PRIVATE_FILE_MODE: u32 = 0o600;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub pid: u32,
    pub port: u16,
    /// The `--host` value the instance listens on.
    pub host: String,
    #[serde(default)]
    pub tls: bool,
    /// Start time, ms since the epoch.
    pub started_at: i64,
    pub version: String,
}

impl Record {
    /// The record of this process, listening on `host:port`.
    pub fn current(host: &str, port: u16, tls: bool) -> Self {
        Record {
            pid: std::process::id(),
            port,
            host: host.to_string(),
            tls,
            started_at: ref_proto::now_ms(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

pub fn instances_dir() -> PathBuf {
    token::state_subdir().join(INSTANCES_DIR)
}

fn record_path(dir: &Path, pid: u32) -> PathBuf {
    dir.join(format!("{pid}.{RECORD_EXTENSION}"))
}

/// Writes the record atomically (a reader never sees half of it) and returns its path.
pub fn write(record: &Record) -> anyhow::Result<PathBuf> {
    write_in(&instances_dir(), record)
}

fn write_in(dir: &Path, record: &Record) -> anyhow::Result<PathBuf> {
    create_private_dir(dir)?;
    let path = record_path(dir, record.pid);
    let tmp = path.with_extension(format!("tmp{}", token::random_hex()));
    let data = serde_json::to_vec_pretty(record).expect("a record serializes");
    let written = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(PRIVATE_FILE_MODE);
        }
        let mut file = options.open(&tmp)?;
        file.write_all(&data)?;
        drop(file);
        std::fs::rename(&tmp, &path)
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("cannot write {}", path.display()));
    }
    Ok(path)
}

fn create_private_dir(dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(PRIVATE_DIR_MODE))
            .with_context(|| format!("cannot restrict {}", dir.display()))?;
    }
    Ok(())
}

/// Deletes the record of a stopping instance. Failing is harmless: the next lister removes
/// the file once the pid is gone.
pub fn remove(path: &Path) {
    if let Err(e) = std::fs::remove_file(path) {
        tracing::debug!("cannot remove {}: {e}", path.display());
    }
}

/// The records of live instances, newest first. Files of dead processes are removed;
/// files that do not parse are left alone.
pub fn list_live() -> Vec<Record> {
    list_live_in(&instances_dir())
}

fn list_live_in(dir: &Path) -> Vec<Record> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut live = Vec::new();
    for path in entries.flatten().map(|e| e.path()) {
        if path.extension().and_then(|e| e.to_str()) != Some(RECORD_EXTENSION) {
            continue;
        }
        let Ok(raw) = std::fs::read(&path) else {
            continue;
        };
        let Ok(record) = serde_json::from_slice::<Record>(&raw) else {
            tracing::debug!("ignoring {}: not a record", path.display());
            continue;
        };
        if ref_sys::pid_alive(record.pid) {
            live.push(record);
        } else {
            remove(&path);
        }
    }
    live.sort_by_key(|r| std::cmp::Reverse(r.started_at));
    live
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ref-web-instance-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn record(pid: u32, port: u16, started_at: i64) -> Record {
        Record {
            pid,
            port,
            host: "127.0.0.1".into(),
            tls: false,
            started_at,
            version: "0.0.0".into(),
        }
    }

    #[test]
    fn a_written_record_is_listed() {
        let dir = temp_dir("write");
        let mine = record(std::process::id(), 4242, 5);
        let path = write_in(&dir, &mine).unwrap();
        assert_eq!(path, record_path(&dir, mine.pid));
        assert_eq!(list_live_in(&dir), [mine]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A pid that existed and is gone.
    #[cfg(unix)]
    fn dead_pid() -> u32 {
        let mut child = std::process::Command::new("true").spawn().unwrap();
        child.wait().unwrap();
        child.id()
    }

    #[cfg(unix)]
    #[test]
    fn live_records_come_newest_first_and_stale_ones_are_removed() {
        let dir = temp_dir("order");
        let mut other = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let stale = dead_pid();
        write_in(&dir, &record(std::process::id(), 1, 10)).unwrap();
        write_in(&dir, &record(other.id(), 2, 20)).unwrap();
        let stale_path = write_in(&dir, &record(stale, 3, 30)).unwrap();
        let ports: Vec<u16> = list_live_in(&dir).iter().map(|r| r.port).collect();
        assert_eq!(ports, [2, 1]);
        assert!(!stale_path.exists(), "stale record removed");
        other.kill().unwrap();
        other.wait().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn files_that_are_not_records_are_ignored() {
        let dir = temp_dir("junk");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("1.json"), "not json").unwrap();
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        assert!(list_live_in(&dir).is_empty());
        assert!(dir.join("1.json").exists(), "left alone");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn the_directory_and_the_file_are_private() {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let dir = temp_dir("mode");
        let path = write_in(&dir, &record(std::process::id(), 1, 1)).unwrap();
        assert_eq!(mode(&dir), PRIVATE_DIR_MODE);
        assert_eq!(mode(&path), PRIVATE_FILE_MODE);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
