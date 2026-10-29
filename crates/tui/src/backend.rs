//! Find a running backend via the instance registry, or spawn `hernees-server`.

use crate::client;
use hn_proto::InstanceInfo;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

pub struct Backend {
    pub url: String,
    pub ws: Ws,
    pub spawned: bool,
}

fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    // SAFETY: kill with signal 0 only checks for existence/permission.
    let r = unsafe { libc::kill(pid as libc::pid_t, 0) };
    r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

fn host_for(info: &InstanceInfo) -> String {
    match info.bind.as_str() {
        "" | "0.0.0.0" | "::" | "[::]" => "127.0.0.1".to_string(),
        b if b.contains(':') && !b.starts_with('[') => format!("[{b}]"),
        b => b.to_string(),
    }
}

/// Live instances, newest first.
fn instances() -> Vec<InstanceInfo> {
    let dir = hn_proto::instances_dir();
    let mut v: Vec<InstanceInfo> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|s| serde_json::from_str::<InstanceInfo>(&s).ok())
        .filter(|i| pid_alive(i.pid))
        .collect();
    v.sort_by_key(|i| std::cmp::Reverse(i.started_at));
    v
}

fn find_server_exe() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("HERNEES_SERVER") {
        return Some(PathBuf::from(p));
    }
    if let Ok(me) = std::env::current_exe() {
        if let Some(dir) = me.parent() {
            let p = dir.join("hernees-server");
            if p.is_file() {
                return Some(p);
            }
        }
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join("hernees-server")).find(|p| p.is_file())
}

fn free_port() -> u16 {
    use std::net::TcpListener;
    let seed = std::process::id() as u64 ^ hn_proto::now_ms() as u64;
    for i in 0..200u64 {
        let p = 10000 + ((seed.wrapping_mul(2654435761).wrapping_add(i * 7919)) % 50000) as u16;
        if TcpListener::bind(("127.0.0.1", p)).is_ok() {
            return p;
        }
    }
    // Fall back to an OS-assigned port (may be < 10000 on odd systems).
    TcpListener::bind(("127.0.0.1", 0)).and_then(|l| l.local_addr()).map(|a| a.port()).unwrap_or(17777)
}

fn spawn_server(exe: &Path, port: u16) -> anyhow::Result<PathBuf> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    let logs = hn_proto::state_dir().join("logs");
    std::fs::create_dir_all(&logs)?;
    let log_path = logs.join(format!("server-{}.log", chrono::Local::now().format("%Y%m%d-%H%M%S")));
    let log = std::fs::OpenOptions::new().create(true).append(true).open(&log_path)?;
    let mut cmd = Command::new(exe);
    // Exit as soon as the last frontend leaves. A frontend connecting while it shuts down
    // gets HTTP 503 (or a closed socket) and spawns a new backend (see `connect`).
    cmd.args(["--port", &port.to_string(), "--idle-timeout", "0"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log));
    // SAFETY: setsid is async-signal-safe; detaches from our controlling terminal.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let child = cmd.spawn()?;
    // Don't keep a zombie around: reap in a background thread.
    std::thread::spawn(move || {
        let mut c = child;
        let _ = c.wait();
    });
    Ok(log_path)
}

/// Connect to an explicit URL, or discover/spawn a local backend.
///
/// Discovery tries every live instance (newest first); an instance that refuses (HTTP 503
/// while shutting down), resets, or closes the socket during Hello is skipped. If none
/// works, a new backend is spawned. The whole scan is retried a few times because an
/// instance can disappear between being listed and being connected to.
pub async fn connect(explicit: Option<&str>, cwd: &str) -> anyhow::Result<Backend> {
    if let Some(u) = explicit {
        let u = if u.contains("://") { u.to_string() } else { format!("ws://{u}") };
        let (ws, url) = client::connect_hello(&u, cwd, Duration::from_secs(8))
            .await
            .map_err(|e| anyhow::anyhow!("cannot connect to {u}: {e}"))?;
        return Ok(Backend { url, ws, spawned: false });
    }
    let mut last = anyhow::anyhow!("no backend");
    for _ in 0..3 {
        match discover(cwd).await {
            Ok(b) => return Ok(b),
            Err(e) => last = e,
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Err(last)
}

async fn discover(cwd: &str) -> anyhow::Result<Backend> {
    let t = Duration::from_secs(3);
    for inst in instances() {
        let base = format!("ws://{}:{}", host_for(&inst), inst.port);
        if let Ok((ws, url)) = client::connect_hello(&base, cwd, t).await {
            return Ok(Backend { url, ws, spawned: false });
        }
    }
    let exe = find_server_exe().ok_or_else(|| {
        anyhow::anyhow!("no running backend found and `hernees-server` is not next to this binary or in PATH")
    })?;
    let port = free_port();
    let log = spawn_server(&exe, port)?;
    let base = format!("ws://127.0.0.1:{port}");
    let start = Instant::now();
    let mut last = String::new();
    while start.elapsed() < Duration::from_secs(10) {
        tokio::time::sleep(Duration::from_millis(150)).await;
        match client::connect_hello(&base, cwd, Duration::from_secs(2)).await {
            Ok((ws, url)) => return Ok(Backend { url, ws, spawned: true }),
            Err(e) => last = e,
        }
    }
    anyhow::bail!("started {} on port {port} but it did not accept connections ({last}); see {}", exe.display(), log.display())
}
