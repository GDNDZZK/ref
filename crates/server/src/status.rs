//! `--status`: list running instances.

use crate::registry;
use futures::{SinkExt, StreamExt};
use hn_proto::*;
use serde_json::{json, Value};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message as WsMsg;

fn host_for(info: &InstanceInfo) -> String {
    match info.bind.as_str() {
        "" | "0.0.0.0" | "::" | "[::]" => "127.0.0.1".to_string(),
        "localhost" => "127.0.0.1".to_string(),
        b if b.contains(':') && !b.starts_with('[') => format!("[{b}]"),
        b => b.to_string(),
    }
}

pub fn ws_url(info: &InstanceInfo) -> String {
    format!("ws://{}:{}/", host_for(info), info.port)
}

pub fn idle_human(t: i64) -> String {
    match t {
        0 => "0 (exit when no frontend)".into(),
        t if t < 0 => "-1 (never)".into(),
        t => format!("{t}s"),
    }
}

/// Quick probe: (open sessions, connected frontends excluding us, sessions busy).
async fn probe(url: &str) -> Option<(usize, u64, usize)> {
    let fut = async {
        let (mut ws, _) = tokio_tungstenite::connect_async(url).await.ok()?;
        let send = |id: u64, msg: ClientMsg| serde_json::to_string(&Request { id, msg }).ok();
        let hello = send(0, ClientMsg::Hello { client: "status".into(), cwd: String::new(), protocol: PROTOCOL_VERSION })?;
        ws.send(WsMsg::text(hello)).await.ok()?;
        ws.send(WsMsg::text(send(1, ClientMsg::ListSessions { scope: ListScope::Open })?)).await.ok()?;
        let mut clients = 0u64;
        loop {
            let Some(Ok(WsMsg::Text(t))) = ws.next().await else { return None };
            match serde_json::from_str::<ServerMsg>(t.as_str()) {
                Ok(ServerMsg::Response { id: 0, ok, .. }) => {
                    clients = ok.as_ref().and_then(|v| v.get("clients")).and_then(|c| c.as_u64()).unwrap_or(0);
                }
                Ok(ServerMsg::Response { id: 1, ok, .. }) => {
                    let list: Vec<SessionInfo> = serde_json::from_value(ok.unwrap_or(Value::Null)).ok()?;
                    let busy = list.iter().filter(|s| s.status.is_active()).count();
                    let _ = ws.close(None).await;
                    return Some((list.len(), clients, busy));
                }
                _ => {}
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(3), fut).await.ok().flatten()
}

pub async fn run(as_json: bool) -> i32 {
    let list = registry::live_instances();
    let mut out = Vec::new();
    for i in &list {
        let url = ws_url(i);
        let p = probe(&url).await;
        out.push((i.clone(), url, p));
    }
    if as_json {
        let v: Vec<Value> = out
            .iter()
            .map(|(i, url, p)| {
                let mut v = serde_json::to_value(i).unwrap_or(Value::Null);
                if let Value::Object(m) = &mut v {
                    m.insert("url".into(), json!(url));
                    m.insert("reachable".into(), json!(p.is_some()));
                    if let Some((sessions, clients, busy)) = p {
                        m.insert("open_sessions".into(), json!(sessions));
                        m.insert("busy_sessions".into(), json!(busy));
                        m.insert("frontends".into(), json!(clients));
                    }
                }
                v
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_else(|_| "[]".into()));
    } else if out.is_empty() {
        println!("No hernees-server instance is running.");
    } else {
        println!("{} hernees-server instance(s) running:", out.len());
        for (i, url, p) in &out {
            println!();
            println!("  pid           {}", i.pid);
            println!("  url           {url}");
            println!("  bind          {}:{}", i.bind, i.port);
            println!("  allowed hosts {}", i.allow_hosts.join(", "));
            println!("  idle timeout  {}", idle_human(i.idle_timeout));
            println!("  started       {} ({})", crate::util::fmt_time(i.started_at), crate::util::relative_age(i.started_at, now_ms()));
            println!("  version       {}", i.version);
            println!("  exe           {}", i.exe);
            match p {
                Some((s, c, b)) => println!("  sessions      {s} open ({b} busy), {c} frontend(s) connected"),
                None => println!("  sessions      (not reachable)"),
            }
        }
    }
    if out.is_empty() {
        1
    } else {
        0
    }
}
