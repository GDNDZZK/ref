//! Instance registry: `instances_dir()/<pid>.json`.

use hn_proto::InstanceInfo;
use std::path::PathBuf;

pub fn pid_alive(pid: u32) -> bool {
    if pid == 0 || pid > i32::MAX as u32 {
        return false;
    }
    // SAFETY: kill with signal 0 only checks for existence/permission.
    let r = unsafe { libc::kill(pid as libc::pid_t, 0) };
    r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

pub fn instance_path(pid: u32) -> PathBuf {
    hn_proto::instances_dir().join(format!("{pid}.json"))
}

pub fn register(info: &InstanceInfo) -> anyhow::Result<PathBuf> {
    let path = instance_path(info.pid);
    let bytes = serde_json::to_vec_pretty(info)?;
    crate::persist::write_atomic(&path, &bytes)?;
    Ok(path)
}

pub fn unregister(pid: u32) {
    let _ = std::fs::remove_file(instance_path(pid));
}

/// Live instances (stale files are removed), oldest first.
pub fn live_instances() -> Vec<InstanceInfo> {
    let dir = hn_proto::instances_dir();
    let Ok(rd) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().is_none_or(|x| x != "json") {
            continue;
        }
        let parsed = std::fs::read_to_string(&p).ok().and_then(|s| serde_json::from_str::<InstanceInfo>(&s).ok());
        match parsed {
            Some(i) if pid_alive(i.pid) => out.push(i),
            _ => {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
    out.sort_by_key(|i| i.started_at);
    out
}
