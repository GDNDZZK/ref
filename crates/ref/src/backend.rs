//! Backend discovery: find a running `refd` via `refd --status --json`, or spawn one.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Instance {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub bind: String,
    #[serde(default)]
    pub port: u16,
}

impl Instance {
    /// Full websocket URL including the token query.
    pub fn ws_url(&self) -> String {
        let base = match &self.url {
            Some(u) if !u.is_empty() => u.clone(),
            _ => {
                let host = if self.bind.is_empty() || self.bind == "0.0.0.0" || self.bind == "::" {
                    "127.0.0.1".to_string()
                } else {
                    self.bind.clone()
                };
                format!("ws://{host}:{}/ws", self.port)
            }
        };
        with_token(&base, self.token.as_deref())
    }
}

/// Normalize a user-supplied URL (`ws://host:port`, optional `/ws`) and add `token`.
pub fn with_token(url: &str, token: Option<&str>) -> String {
    let mut u = url.trim().to_string();
    if u.starts_with("http://") {
        u = u.replacen("http://", "ws://", 1);
    } else if u.starts_with("https://") {
        u = u.replacen("https://", "wss://", 1);
    } else if !u.starts_with("ws://") && !u.starts_with("wss://") {
        u = format!("ws://{u}");
    }
    let (path_part, query) = match u.split_once('?') {
        Some((a, b)) => (a.to_string(), Some(b.to_string())),
        None => (u.clone(), None),
    };
    let scheme_end = path_part.find("://").map(|p| p + 3).unwrap_or(0);
    let mut path_part = path_part;
    if !path_part[scheme_end..].contains('/') {
        path_part.push_str("/ws");
    } else if path_part.ends_with('/') {
        path_part.push_str("ws");
    }
    let has_token = query.as_deref().map(|q| q.split('&').any(|kv| kv.starts_with("token="))).unwrap_or(false);
    let mut out = path_part;
    let mut q: Vec<String> = query.map(|q| vec![q]).unwrap_or_default();
    if let (Some(t), false) = (token, has_token) {
        if !t.is_empty() {
            q.push(format!("token={}", urlencode(t)));
        }
    }
    if !q.is_empty() {
        out.push('?');
        out.push_str(&q.join("&"));
    }
    out
}

fn urlencode(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => o.push(b as char),
            _ => o.push_str(&format!("%{b:02X}")),
        }
    }
    o
}

/// `refd` next to the current executable, else from PATH.
pub fn refd_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("refd");
            if p.is_file() {
                return p;
            }
        }
    }
    PathBuf::from("refd")
}

pub async fn status() -> Result<Vec<Instance>> {
    let out = tokio::process::Command::new(refd_path())
        .args(["--status", "--json"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .await
        .context("running `refd --status --json` (is refd installed?)")?;
    let s = String::from_utf8_lossy(&out.stdout);
    let s = s.trim();
    if s.is_empty() {
        return Ok(vec![]);
    }
    // Accept either an array or a single object.
    if let Ok(v) = serde_json::from_str::<Vec<Instance>>(s) {
        return Ok(v);
    }
    if let Ok(v) = serde_json::from_str::<Instance>(s) {
        return Ok(vec![v]);
    }
    Ok(vec![])
}

fn free_port() -> Option<u16> {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    for _ in 0..200 {
        let p: u16 = rng.gen_range(10000..60000);
        if std::net::TcpListener::bind(("127.0.0.1", p)).is_ok() {
            return Some(p);
        }
    }
    None
}

fn spawn_refd(port: u16) -> Result<()> {
    let dir = crate::util::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let log = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("refd.log"));
    let mut cmd = std::process::Command::new(refd_path());
    cmd.args(["--port", &port.to_string(), "--idle-timeout", "0"]).stdin(Stdio::null()).stdout(Stdio::null());
    match log {
        Ok(f) => cmd.stderr(Stdio::from(f)),
        Err(_) => cmd.stderr(Stdio::null()),
    };
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // New session: fully detached from our terminal / process group.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    let child = cmd.spawn().context("spawning refd")?;
    // Reap in the background so it never becomes a zombie of ours if it exits early.
    std::thread::spawn(move || {
        let mut c = child;
        let _ = c.wait();
    });
    Ok(())
}

/// Find a running backend, spawning one if none is alive. Returns the ws URL (with token).
pub async fn discover_or_spawn() -> Result<String> {
    let list = status().await.unwrap_or_default();
    if let Some(i) = list.first() {
        return Ok(i.ws_url());
    }
    // Don't spawn a new daemon on every reconnect attempt if a recent spawn is still starting up.
    static LAST_SPAWN: std::sync::Mutex<Option<(Instant, u16)>> = std::sync::Mutex::new(None);
    let recent = LAST_SPAWN.lock().unwrap().filter(|(t, _)| t.elapsed() < Duration::from_secs(30));
    let port = match recent {
        Some((_, p)) => p,
        None => {
            let port = free_port().ok_or_else(|| anyhow!("no free port found"))?;
            spawn_refd(port)?;
            *LAST_SPAWN.lock().unwrap() = Some((Instant::now(), port));
            port
        }
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        tokio::time::sleep(Duration::from_millis(150)).await;
        if let Ok(list) = status().await {
            if let Some(i) = list.iter().find(|i| i.port == port).or(list.first()) {
                return Ok(i.ws_url());
            }
        }
        if Instant::now() > deadline {
            bail!(
                "refd did not come up within 10s (see {})",
                crate::util::data_dir().join("refd.log").display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(with_token("ws://h:1", Some("t")), "ws://h:1/ws?token=t");
        assert_eq!(with_token("h:1", None), "ws://h:1/ws");
        assert_eq!(with_token("ws://h:1/ws?token=x", Some("t")), "ws://h:1/ws?token=x");
        assert_eq!(with_token("http://h:1/", Some("a b")), "ws://h:1/ws?token=a%20b");
        let i = Instance { url: None, token: Some("k".into()), bind: "0.0.0.0".into(), port: 5 };
        assert_eq!(i.ws_url(), "ws://127.0.0.1:5/ws?token=k");
    }
}
