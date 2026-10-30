//! Test support: scripted OpenAI-chat SSE mock server, config and hub builders.
#![allow(dead_code)]

use ref_config::{AgentConfig, Config, ModelConfig, PermissionRules, ProviderConfig};
use ref_proto::{Message, ModelRef, Part, Role};
use ref_server::session::Session;
use ref_server::{Hub, HubOptions};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Debug, Clone)]
pub enum Reply {
    Text(String),
    /// (call id, tool name, raw arguments JSON)
    Tools(Vec<(String, String, String)>),
    Status(u16, String),
    /// Send one text delta then stall (for abort tests).
    Hang(String),
}

pub fn tool(id: &str, name: &str, args: Value) -> (String, String, String) {
    (id.into(), name.into(), args.to_string())
}

pub type Handler = Arc<dyn Fn(usize, &Value) -> Reply + Send + Sync>;

pub struct Mock {
    pub url: String,
    /// Bodies of all non-title requests, in arrival order.
    pub bodies: Arc<Mutex<Vec<Value>>>,
}

impl Mock {
    pub fn bodies(&self) -> Vec<Value> {
        self.bodies.lock().unwrap().clone()
    }
    pub fn count(&self) -> usize {
        self.bodies.lock().unwrap().len()
    }
}

fn chunk(v: Value) -> String {
    format!("data: {v}\n\n")
}

fn sse(reply: &Reply) -> String {
    let mut s = String::new();
    match reply {
        Reply::Text(t) => {
            s.push_str(&chunk(json!({"choices":[{"index":0,"delta":{"role":"assistant","content":t},"finish_reason":null}]})));
            s.push_str(&chunk(
                json!({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
            ));
        }
        Reply::Tools(calls) => {
            for (i, (id, name, args)) in calls.iter().enumerate() {
                s.push_str(&chunk(json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":i,"id":id,"type":"function","function":{"name":name,"arguments":""}}]},"finish_reason":null}]})));
                // Split the arguments in two deltas to exercise streaming.
                let mid = args
                    .char_indices()
                    .nth(args.chars().count() / 2)
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                for part in [&args[..mid], &args[mid..]] {
                    s.push_str(&chunk(json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":i,"function":{"arguments":part}}]},"finish_reason":null}]})));
                }
            }
            s.push_str(&chunk(
                json!({"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
            ));
        }
        Reply::Status(..) | Reply::Hang(_) => {}
    }
    s.push_str(&chunk(
        json!({"choices":[],"usage":{"prompt_tokens":1000,"completion_tokens":20}}),
    ));
    s.push_str("data: [DONE]\n\n");
    s
}

fn is_title(body: &Value) -> bool {
    body.to_string().contains("Generate a short title")
}

pub async fn mock(handler: Handler) -> Mock {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let bodies: Arc<Mutex<Vec<Value>>> = Arc::default();
    let b2 = bodies.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let handler = handler.clone();
            let bodies = b2.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut tmp = [0u8; 8192];
                let header_end = loop {
                    let Ok(n) = sock.read(&mut tmp).await else {
                        return;
                    };
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                    if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break p + 4;
                    }
                };
                let head = String::from_utf8_lossy(&buf[..header_end]).to_lowercase();
                let len: usize = head
                    .lines()
                    .find_map(|l| {
                        l.strip_prefix("content-length:")
                            .map(|v| v.trim().parse().unwrap_or(0))
                    })
                    .unwrap_or(0);
                while buf.len() < header_end + len {
                    let Ok(n) = sock.read(&mut tmp).await else {
                        return;
                    };
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                }
                let body: Value = serde_json::from_slice(&buf[header_end..]).unwrap_or(Value::Null);
                let reply = if is_title(&body) {
                    Reply::Text("Mock title".into())
                } else {
                    let n = {
                        let mut b = bodies.lock().unwrap();
                        b.push(body.clone());
                        b.len() - 1
                    };
                    handler(n, &body)
                };
                match reply {
                    Reply::Status(code, text) => {
                        let resp = format!(
                            "HTTP/1.1 {code} Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                            text.len()
                        );
                        let _ = sock.write_all(resp.as_bytes()).await;
                    }
                    Reply::Hang(t) => {
                        let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";
                        let _ = sock.write_all(head.as_bytes()).await;
                        let c = chunk(
                            json!({"choices":[{"index":0,"delta":{"role":"assistant","content":t},"finish_reason":null}]}),
                        );
                        let _ = sock.write_all(c.as_bytes()).await;
                        let _ = sock.flush().await;
                        tokio::time::sleep(Duration::from_secs(60)).await;
                    }
                    r => {
                        let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n";
                        let _ = sock.write_all(head.as_bytes()).await;
                        let _ = sock.write_all(sse(&r).as_bytes()).await;
                        let _ = sock.shutdown().await;
                    }
                }
            });
        }
    });
    Mock { url, bodies }
}

pub fn config(base_url: &str) -> Config {
    let mut cfg = Config::default();
    let mut models = std::collections::BTreeMap::new();
    models.insert(
        "m".to_string(),
        ModelConfig {
            key: "m".into(),
            id: "m".into(),
            name: "Mock".into(),
            context: 10_000,
            ..Default::default()
        },
    );
    cfg.providers.insert(
        "mock".into(),
        ProviderConfig {
            id: "mock".into(),
            name: "Mock".into(),
            api: Some("chat".into()),
            base_url: Some(base_url.to_string()),
            api_key: Some("test".into()),
            models,
            ..Default::default()
        },
    );
    cfg.model = Some(ModelRef {
        provider: "mock".into(),
        model: "m".into(),
        variant: None,
    });
    cfg.default_agent = "build".into();
    cfg.agents.insert(
        "build".into(),
        AgentConfig {
            name: "build".into(),
            description: "Builder".into(),
            creatable: true,
            can_spawn: true,
            ..Default::default()
        },
    );
    cfg.agents.insert(
        "worker".into(),
        AgentConfig {
            name: "worker".into(),
            description: "Worker".into(),
            creatable: true,
            ..Default::default()
        },
    );
    cfg.retry_max_delay = 600;
    cfg
}

pub fn perms(v: Value) -> PermissionRules {
    PermissionRules::from_value(&v).0
}

pub fn temp_dir(tag: &str) -> PathBuf {
    let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let d = base.join(format!(
        "ref-test-{tag}-{}",
        ref_server::util::random_hex(8)
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub fn hub(cfg: Config, tag: &str) -> (Arc<Hub>, PathBuf) {
    let dir = temp_dir(tag);
    let mut opts = HubOptions::new(dir.join("data"));
    opts.home = None;
    opts.retry_base = Duration::from_millis(10);
    opts.mcp_wait = Duration::from_secs(1);
    opts.config_loader = Arc::new(move |_| cfg.clone());
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    (Hub::new(opts), work)
}

pub async fn session(
    hub: &Arc<Hub>,
    cwd: &std::path::Path,
    name: &str,
    agent: &str,
) -> Arc<Session> {
    ref_server::ops::create_session(
        hub,
        ref_server::ops::CreateOpts {
            cwd: cwd.to_string_lossy().to_string(),
            agent: Some(agent.into()),
            model: None,
            name: Some(name.into()),
            unique_suffix: false,
            parent: None,
            must_report: false,
        },
    )
    .await
    .unwrap()
}

pub fn prompt(hub: &Arc<Hub>, s: &Arc<Session>, text: &str) {
    let msg =
        ref_server::session::user_message(ref_proto::MessageSource::Human, text.into(), vec![]);
    ref_server::engine::submit(hub, s, msg);
}

pub async fn wait_until(what: &str, timeout: Duration, mut f: impl FnMut() -> bool) {
    let start = Instant::now();
    while !f() {
        if start.elapsed() > timeout {
            panic!("timed out waiting for: {what}");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

pub async fn wait_idle(s: &Arc<Session>) {
    let s2 = s.clone();
    wait_until("session idle", Duration::from_secs(20), move || {
        s2.lock().run.is_none()
    })
    .await;
}

pub fn messages(s: &Arc<Session>) -> Vec<Message> {
    s.lock().messages.clone()
}

pub fn last_text(s: &Arc<Session>) -> String {
    messages(s)
        .iter()
        .rev()
        .find(|m| m.role == Role::Assistant)
        .map(|m| m.raw_text())
        .unwrap_or_default()
}

pub fn tool_parts(s: &Arc<Session>) -> Vec<Part> {
    messages(s)
        .into_iter()
        .flat_map(|m| m.parts)
        .filter(|p| matches!(p, Part::Tool { .. }))
        .collect()
}

/// Tool-role messages in a chat request body.
pub fn tool_results(body: &Value) -> Vec<Value> {
    body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["role"] == "tool")
        .cloned()
        .collect()
}
