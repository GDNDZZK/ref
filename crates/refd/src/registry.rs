//! Instance registry: `<data_dir>/instances/<pid>.json` (0600), `--status` and `--stop`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use ref_proto::InstanceInfo;
use serde::{Deserialize, Serialize};

use crate::util;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryEntry {
    #[serde(flatten)]
    pub instance: InstanceInfo,
    pub token: String,
}

impl RegistryEntry {
    /// Host to connect to locally.
    pub fn connect_host(&self) -> String {
        connect_host(&self.instance.bind)
    }

    pub fn url(&self) -> String {
        format!("ws://{}:{}/ws", self.connect_host(), self.instance.port)
    }
}

pub fn connect_host(bind: &str) -> String {
    match bind {
        "" | "0.0.0.0" => "127.0.0.1".to_string(),
        "::" | "[::]" => "[::1]".to_string(),
        b if b.contains(':') && !b.starts_with('[') => format!("[{b}]"),
        b => b.to_string(),
    }
}

pub fn instances_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("instances")
}

pub fn entry_path(data_dir: &Path, pid: u32) -> PathBuf {
    instances_dir(data_dir).join(format!("{pid}.json"))
}

/// Write the registry file of this process. Returns its path.
pub fn write_entry(data_dir: &Path, entry: &RegistryEntry) -> std::io::Result<PathBuf> {
    let dir = instances_dir(data_dir);
    std::fs::create_dir_all(&dir)?;
    let path = entry_path(data_dir, entry.instance.pid);
    let data = serde_json::to_vec_pretty(entry).map_err(std::io::Error::other)?;
    util::write_atomic(&path, &data)?;
    Ok(path)
}

/// Read all registry entries (unparsable files are returned as `Err(path)`).
pub fn read_entries(data_dir: &Path) -> Vec<Result<RegistryEntry, PathBuf>> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(instances_dir(data_dir)) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        match std::fs::read(&p).ok().and_then(|b| serde_json::from_slice::<RegistryEntry>(&b).ok()) {
            Some(r) => out.push(Ok(r)),
            None => out.push(Err(p)),
        }
    }
    out
}

/// Result of probing an instance's `/status` endpoint.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StatusReply {
    pub instance: InstanceInfo,
    pub sessions: u64,
}

/// GET `/status?token=` with a short timeout.
pub async fn probe(entry: &RegistryEntry) -> Option<StatusReply> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .no_proxy()
        .build()
        .ok()?;
    let url = format!(
        "http://{}:{}/status?token={}",
        entry.connect_host(),
        entry.instance.port,
        entry.token
    );
    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.json::<StatusReply>().await.ok()
}

/// Live instances (stale registry files are removed). Sorted by start time.
pub async fn live_instances(data_dir: &Path) -> Vec<(RegistryEntry, StatusReply)> {
    let mut out = Vec::new();
    let entries = read_entries(data_dir);
    let probes = futures::future::join_all(entries.into_iter().map(|e| async move {
        match e {
            Err(path) => Err(path),
            Ok(e) => {
                if !util::pid_alive(e.instance.pid) {
                    return Err(entry_path(data_dir, e.instance.pid));
                }
                match probe(&e).await {
                    Some(s) if s.instance.pid == e.instance.pid => Ok((e, s)),
                    _ => Err(entry_path(data_dir, e.instance.pid)),
                }
            }
        }
    }))
    .await;
    for p in probes {
        match p {
            Ok(x) => out.push(x),
            Err(path) => {
                tracing::debug!("removing stale registry file {}", path.display());
                let _ = std::fs::remove_file(path);
            }
        }
    }
    out.sort_by_key(|(e, _)| e.instance.started);
    out
}

fn fmt_duration(ms: i64) -> String {
    let s = (ms / 1000).max(0);
    let (d, h, m, s) = (s / 86400, (s / 3600) % 24, (s / 60) % 60, s % 60);
    if d > 0 {
        format!("{d}d {h}h {m}m")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

fn fmt_time(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|t| t.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| ms.to_string())
}

/// `refd --status [--json]`. Exit code 0 if at least one instance is alive.
pub async fn status_cmd(json: bool) -> i32 {
    let data_dir = ref_config::data_dir();
    let live = live_instances(&data_dir).await;
    let now = ref_proto::now_ms();
    if json {
        let arr: Vec<serde_json::Value> = live
            .iter()
            .map(|(e, s)| {
                let mut v = serde_json::to_value(&e.instance).unwrap_or_default();
                if let Some(o) = v.as_object_mut() {
                    o.insert("url".into(), e.url().into());
                    o.insert("token".into(), e.token.clone().into());
                    o.insert("sessions".into(), s.sessions.into());
                    o.insert("uptime_secs".into(), ((now - e.instance.started) / 1000).max(0).into());
                }
                v
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr).unwrap_or_else(|_| "[]".into()));
    } else if live.is_empty() {
        println!("No running refd instance.");
    } else {
        for (i, (e, s)) in live.iter().enumerate() {
            if i > 0 {
                println!();
            }
            let inst = &e.instance;
            let idle = match inst.idle_timeout {
                t if t < 0 => "never".to_string(),
                0 => "0 (exit when no frontend is connected)".to_string(),
                t => format!("{t}s"),
            };
            println!("refd pid {} (version {})", inst.pid, inst.version);
            println!("  url:           ws://{}:{}", inst.bind, inst.port);
            println!(
                "  allow-hosts:   {}",
                if inst.allow_hosts.is_empty() { "(loopback only)".to_string() } else { inst.allow_hosts.join(", ") }
            );
            println!("  idle-timeout:  {idle}");
            println!("  started:       {}", fmt_time(inst.started));
            println!("  uptime:        {}", fmt_duration(now - inst.started));
            println!("  open sessions: {}", s.sessions);
            println!("  data dir:      {}", inst.data_dir);
        }
    }
    if live.is_empty() {
        1
    } else {
        0
    }
}

/// `refd --stop [PID]`: SIGTERM the given instance (or all live instances).
pub async fn stop_cmd(pid: Option<u32>) -> i32 {
    let data_dir = ref_config::data_dir();
    let live = live_instances(&data_dir).await;
    let targets: Vec<u32> = match pid {
        Some(p) => {
            if !live.iter().any(|(e, _)| e.instance.pid == p) {
                eprintln!("refd: no running refd instance with pid {p}");
                return 1;
            }
            vec![p]
        }
        None => live.iter().map(|(e, _)| e.instance.pid).collect(),
    };
    if targets.is_empty() {
        eprintln!("refd: no running refd instance");
        return 1;
    }
    let mut code = 0;
    for p in targets {
        // SAFETY: plain kill(2) on a pid we found in our registry.
        let r = unsafe { libc::kill(p as i32, libc::SIGTERM) };
        if r != 0 {
            eprintln!("refd: failed to signal {p}: {}", std::io::Error::last_os_error());
            code = 1;
            continue;
        }
        // Wait up to 10s for it to exit.
        let mut gone = false;
        for _ in 0..100 {
            if !util::pid_alive(p) {
                gone = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        if gone {
            println!("Stopped refd {p}");
        } else {
            eprintln!("refd: {p} did not exit within 10s");
            code = 1;
        }
    }
    code
}
