//! Backend discovery / launch.

use std::path::{Path, PathBuf};
use std::time::Duration;

use ref_proto::InstanceInfo;

/// Where to connect.
#[derive(Debug, Clone)]
pub enum Target {
    /// Discover a running backend or spawn one.
    Auto,
    /// Explicit ws URL (+ optional token); never spawns.
    Explicit { url: String, token: Option<String> },
}

/// A resolved endpoint.
#[derive(Debug, Clone)]
pub struct Endpoint {
    pub url: String,
    pub token: Option<String>,
}

pub fn pid_alive(pid: u32) -> bool {
    if pid == 0 || pid > i32::MAX as u32 {
        return false;
    }
    // SAFETY: kill with signal 0 only checks for existence / permission.
    let r = unsafe { libc::kill(pid as i32, 0) };
    r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Parses an instance file.
pub fn parse_instance(s: &str) -> Option<InstanceInfo> {
    serde_json::from_str(s).ok()
}

/// Reads all instance files (live pids only, newest first). Removes files of dead pids.
pub fn instances(dir: &Path) -> Vec<InstanceInfo> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let Ok(s) = std::fs::read_to_string(&p) else { continue };
        match parse_instance(&s) {
            Some(i) if pid_alive(i.pid) => out.push(i),
            Some(_) => {
                let _ = std::fs::remove_file(&p);
            }
            None => {}
        }
    }
    out.sort_by_key(|i| std::cmp::Reverse(i.started_at));
    out
}

pub fn endpoint_for(i: &InstanceInfo) -> Endpoint {
    let host = match i.bind.as_str() {
        "0.0.0.0" | "" | "::" | "[::]" => "127.0.0.1".to_string(),
        h if h.contains(':') && !h.starts_with('[') => format!("[{h}]"),
        h => h.to_string(),
    };
    Endpoint { url: format!("ws://{host}:{}/", i.port), token: Some(i.token.clone()) }
}

/// Random free port in 10000..=65535 (bind-tested on 127.0.0.1).
pub fn pick_port() -> Option<u16> {
    use rand::Rng;
    let mut rng = rand::rng();
    for _ in 0..200 {
        let p: u16 = rng.random_range(10000..=65535);
        if std::net::TcpListener::bind(("127.0.0.1", p)).is_ok() {
            return Some(p);
        }
    }
    None
}

pub fn server_binary() -> PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let sib = dir.join("ref-server");
        if sib.is_file() {
            return sib;
        }
    }
    PathBuf::from("ref-server")
}

/// Spawns a detached backend on `port`.
pub fn spawn_server(port: u16) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(server_binary());
    cmd.args(["--port", &port.to_string(), "--idle-timeout", "0"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // SAFETY: setsid is async-signal-safe; detaches from our terminal session.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = cmd.spawn()?;
    // Reap it if it exits while we're alive.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Waits for an instance file announcing `port` (up to `timeout`).
pub async fn wait_for_instance(dir: &Path, port: u16, timeout: Duration) -> Option<InstanceInfo> {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if let Some(i) = instances(dir).into_iter().find(|i| i.port == port) {
            return Some(i);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_endpoint() {
        let s = r#"{"pid":1,"port":12345,"bind":"0.0.0.0","allow_hosts":["*"],"idle_timeout":0,"started_at":5,"version":"0.1","token":"abc"}"#;
        let i = parse_instance(s).unwrap();
        let e = endpoint_for(&i);
        assert_eq!(e.url, "ws://127.0.0.1:12345/");
        assert_eq!(e.token.as_deref(), Some("abc"));
        assert!(parse_instance("{bad").is_none());
    }

    #[test]
    fn instance_dir_scan() {
        let dir = std::env::temp_dir().join(format!("ref-tui-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let me = std::process::id();
        let live = format!(r#"{{"pid":{me},"port":1,"bind":"127.0.0.1","allow_hosts":[],"idle_timeout":0,"started_at":1,"version":"x","token":"t"}}"#);
        std::fs::write(dir.join("a.json"), live).unwrap();
        let dead = r#"{"pid":2147483000,"port":2,"bind":"127.0.0.1","allow_hosts":[],"idle_timeout":0,"started_at":2,"version":"x","token":"t"}"#;
        std::fs::write(dir.join("b.json"), dead).unwrap();
        let v = instances(&dir);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].port, 1);
        assert!(!dir.join("b.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn port_pick() {
        let p = pick_port().unwrap();
        assert!(p >= 10000);
    }
}
