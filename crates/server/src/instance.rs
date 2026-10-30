//! Instance files, port selection, `--status` and `--stop`.

use crate::util::{fmt_duration, fmt_time, pid_alive, write_atomic};
use futures_util::{SinkExt, StreamExt};
use ref_proto::{ClientFrame, InstanceInfo, Request, ServerFrame};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message as WsMsg;

pub fn instance_path(dir: &Path, pid: u32) -> PathBuf {
    dir.join(format!("{pid}.json"))
}

pub fn write_instance(dir: &Path, info: &InstanceInfo) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    let p = instance_path(dir, info.pid);
    let data = serde_json::to_vec_pretty(info).map_err(std::io::Error::other)?;
    write_atomic(&p, &data, true)?;
    Ok(p)
}

/// Live instances (stale files of dead processes are removed), newest first.
pub fn list_instances(dir: &Path) -> Vec<InstanceInfo> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let Ok(raw) = std::fs::read(&p) else { continue };
        match serde_json::from_slice::<InstanceInfo>(&raw) {
            Ok(i) if pid_alive(i.pid) => out.push(i),
            Ok(_) => {
                let _ = std::fs::remove_file(&p);
            }
            Err(_) => {}
        }
    }
    out.sort_by_key(|i| std::cmp::Reverse(i.started_at));
    out
}

/// Random free port in 10000..=65535 (bind-tested on `host`).
pub fn pick_port(host: &str) -> Option<u16> {
    use rand::Rng;
    let mut rng = rand::rng();
    for _ in 0..500 {
        let p: u16 = rng.random_range(10000..=65535);
        if std::net::TcpListener::bind((host, p)).is_ok() {
            return Some(p);
        }
    }
    None
}

pub fn connect_url(i: &InstanceInfo) -> String {
    let host = match i.bind.as_str() {
        "0.0.0.0" | "" | "::" | "[::]" => "127.0.0.1".to_string(),
        h if h.contains(':') && !h.starts_with('[') => format!("[{h}]"),
        h => h.to_string(),
    };
    format!("ws://{host}:{}/", i.port)
}

fn idle_meaning(t: i64) -> String {
    match t {
        t if t < 0 => "never exits when idle".into(),
        0 => "exits as soon as the last frontend disconnects".into(),
        t => format!("exits {t}s after the last frontend disconnects"),
    }
}

/// Connects, says Hello, sends `req`, returns the response result.
pub async fn request(
    i: &InstanceInfo,
    req: Request,
    timeout: Duration,
) -> Result<serde_json::Value, String> {
    let fut = async {
        let url = format!("{}?token={}", connect_url(i), i.token);
        // Responses (session lists, snapshots) can exceed the 16 MiB default frame limit.
        let cfg = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
            .max_message_size(None)
            .max_frame_size(None);
        let (mut ws, _) = tokio_tungstenite::connect_async_with_config(url, Some(cfg), false)
            .await
            .map_err(|e| e.to_string())?;
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "/".into());
        let frames = [
            ClientFrame {
                id: Some(1),
                req: Request::Hello {
                    client: crate::ws::CONTROL_CLIENT.into(),
                    version: ref_proto::PROTOCOL_VERSION,
                    cwd,
                    token: Some(i.token.clone()),
                },
            },
            ClientFrame { id: Some(2), req },
        ];
        for f in frames {
            let t = serde_json::to_string(&f).map_err(|e| e.to_string())?;
            ws.send(WsMsg::text(t)).await.map_err(|e| e.to_string())?;
        }
        while let Some(m) = ws.next().await {
            let Ok(WsMsg::Text(t)) = m else {
                if m.is_err() {
                    break;
                }
                continue;
            };
            if let Ok(ServerFrame::Response {
                id,
                ok,
                result,
                error,
            }) = serde_json::from_str::<ServerFrame>(&t)
            {
                if id == 1 && !ok {
                    return Err(error.unwrap_or_else(|| "hello rejected".into()));
                }
                if id == 2 {
                    let _ = ws.close(None).await;
                    return if ok {
                        Ok(result)
                    } else {
                        Err(error.unwrap_or_default())
                    };
                }
            }
        }
        Err("connection closed".to_string())
    };
    tokio::time::timeout(timeout, fut)
        .await
        .map_err(|_| "timed out".to_string())?
}

/// `--status`: exit code 0 if any instance is running.
pub async fn status(json_out: bool) -> i32 {
    let dir = ref_proto::instances_dir();
    let list = list_instances(&dir);
    let mut rows = Vec::new();
    for i in &list {
        let sessions = request(i, Request::ListSessions, Duration::from_secs(3))
            .await
            .ok()
            .and_then(|v| {
                v.get("sessions")
                    .and_then(|s| s.as_array())
                    .map(|a| a.len())
            });
        rows.push((i.clone(), sessions));
    }
    if json_out {
        let arr: Vec<serde_json::Value> = rows
            .iter()
            .map(|(i, n)| {
                let mut v = serde_json::to_value(i).unwrap_or_default();
                if let Some(o) = v.as_object_mut() {
                    o.insert("sessions".into(), serde_json::json!(n));
                    o.insert("url".into(), serde_json::json!(connect_url(i)));
                }
                v
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&arr).unwrap_or_else(|_| "[]".into())
        );
    } else if rows.is_empty() {
        println!(
            "No ref-server instances running (instances dir: {}).",
            dir.display()
        );
    } else {
        let now = ref_proto::now_ms();
        for (k, (i, n)) in rows.iter().enumerate() {
            if k > 0 {
                println!();
            }
            println!("ref-server pid {}", i.pid);
            println!("  url:           {}", connect_url(i));
            println!("  bind:          {}:{}", i.bind, i.port);
            println!("  allowed hosts: {}", i.allow_hosts.join(", "));
            println!(
                "  idle timeout:  {} ({})",
                i.idle_timeout,
                idle_meaning(i.idle_timeout)
            );
            println!(
                "  started:       {} (up {})",
                fmt_time(i.started_at),
                fmt_duration((now - i.started_at) / 1000)
            );
            println!("  version:       {}", i.version);
            match n {
                Some(n) => println!("  live sessions: {n}"),
                None => println!("  live sessions: unknown (could not connect)"),
            }
            println!("  instance file: {}", instance_path(&dir, i.pid).display());
        }
    }
    if rows.is_empty() { 1 } else { 0 }
}

/// `--stop [PID]`.
pub async fn stop(pid: Option<u32>) -> i32 {
    let dir = ref_proto::instances_dir();
    let list: Vec<InstanceInfo> = list_instances(&dir)
        .into_iter()
        .filter(|i| pid.is_none_or(|p| p == i.pid))
        .collect();
    if list.is_empty() {
        match pid {
            Some(p) => eprintln!("No ref-server instance with pid {p}."),
            None => eprintln!("No ref-server instances running."),
        }
        return 1;
    }
    let mut code = 0;
    for i in list {
        match request(&i, Request::Shutdown, Duration::from_secs(5)).await {
            Ok(_) => {
                let mut gone = false;
                for _ in 0..100 {
                    if !pid_alive(i.pid) {
                        gone = true;
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                if gone {
                    println!("Stopped ref-server pid {}.", i.pid);
                } else {
                    println!(
                        "Shutdown requested for ref-server pid {} (still exiting).",
                        i.pid
                    );
                }
            }
            Err(e) => {
                eprintln!(
                    "Could not reach ref-server pid {} ({e}); terminating it.",
                    i.pid
                );
                if !ref_sys::terminate(i.pid) {
                    code = 1;
                }
            }
        }
    }
    code
}
