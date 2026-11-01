//! Test support: scripted OpenAI-chat SSE mock server, config and hub builders.
#![allow(dead_code)]

use ref_config::{AgentConfig, Config, ModelConfig, PermissionRules, ProviderConfig};
use ref_proto::{Message, ModelRef, Part, Request, Role, ServerFrame, SessionEvent, ToolState};
use ref_server::hub::Conn;
use ref_server::session::Session;
use ref_server::{Hub, HubOptions};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Default limit for waiting on test conditions.
pub const TIMEOUT: Duration = Duration::from_secs(10);
/// Limit for a whole turn to finish (`wait_idle`).
const IDLE_TIMEOUT: Duration = Duration::from_secs(20);
const RETRY_BASE: Duration = Duration::from_millis(10);
const MCP_WAIT: Duration = Duration::from_secs(1);

/// Agent and session names that `config()` defines.
pub const BUILD: &str = "build";
pub const WORKER: &str = "worker";
pub const REVIEW: &str = "review";
pub const MAIN_SESSION: &str = "main";

/// Usage the mock server reports for every non-title reply.
pub const MOCK_PROMPT_TOKENS: u64 = 1000;
pub const MOCK_COMPLETION_TOKENS: u64 = 20;
/// Context window of the mock model.
pub const MOCK_CONTEXT: u64 = 10_000;

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
        json!({"choices":[],"usage":{"prompt_tokens":MOCK_PROMPT_TOKENS,"completion_tokens":MOCK_COMPLETION_TOKENS}}),
    ));
    s.push_str("data: [DONE]\n\n");
    s
}

fn is_title(body: &Value) -> bool {
    body.to_string().contains(ref_server::title::TITLE_PROMPT)
}

/// Write to the socket; false when the client has gone (expected on aborts).
async fn write_or_stop(sock: &mut TcpStream, data: &[u8]) -> bool {
    sock.write_all(data).await.is_ok()
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
                        write_or_stop(&mut sock, resp.as_bytes()).await;
                    }
                    Reply::Hang(t) => {
                        let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";
                        if !write_or_stop(&mut sock, head.as_bytes()).await {
                            return;
                        }
                        let c = chunk(
                            json!({"choices":[{"index":0,"delta":{"role":"assistant","content":t},"finish_reason":null}]}),
                        );
                        if !write_or_stop(&mut sock, c.as_bytes()).await
                            || sock.flush().await.is_err()
                        {
                            return;
                        }
                        tokio::time::sleep(Duration::from_secs(60)).await;
                    }
                    r => {
                        let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n";
                        if !write_or_stop(&mut sock, head.as_bytes()).await {
                            return;
                        }
                        if write_or_stop(&mut sock, sse(&r).as_bytes()).await {
                            // The client may already be gone; nothing to do then.
                            sock.shutdown().await.ok();
                        }
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
            context: MOCK_CONTEXT,
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
    cfg.default_agent = BUILD.into();
    cfg.agents.insert(
        BUILD.into(),
        AgentConfig {
            name: BUILD.into(),
            description: "Builder".into(),
            creatable: true,
            can_spawn: true,
            ..Default::default()
        },
    );
    cfg.agents.insert(
        WORKER.into(),
        AgentConfig {
            name: WORKER.into(),
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

/// Hub options with data in `dir/data`, no home directory and fast retries.
pub fn hub_options(cfg: Config, dir: &Path) -> HubOptions {
    let mut opts = HubOptions::new(dir.join("data"));
    opts.home = None;
    opts.retry_base = RETRY_BASE;
    opts.mcp_wait = MCP_WAIT;
    opts.config_loader = Arc::new(move |_| cfg.clone());
    opts
}

/// A hub in a fresh temp dir, with `adjust` applied to its options, and its work dir.
pub fn hub_with(
    cfg: Config,
    tag: &str,
    adjust: impl FnOnce(&mut HubOptions),
) -> (Arc<Hub>, PathBuf) {
    let dir = temp_dir(tag);
    let mut opts = hub_options(cfg, &dir);
    adjust(&mut opts);
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    (Hub::new(opts), work)
}

pub fn hub(cfg: Config, tag: &str) -> (Arc<Hub>, PathBuf) {
    hub_with(cfg, tag, |_| {})
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

/// Waits until `f` holds for `s`.
pub async fn wait_session(s: &Arc<Session>, what: &str, f: impl Fn(&Arc<Session>) -> bool) {
    wait_until(what, TIMEOUT, || f(s)).await;
}

/// Waits until some tool part of `s` is in `state`.
pub async fn wait_tool_state(s: &Arc<Session>, state: ToolState) {
    let what = format!("tool {state:?}");
    wait_session(s, &what, |s| {
        tool_parts(s)
            .iter()
            .any(|p| matches!(p, Part::Tool { state: now, .. } if *now == state))
    })
    .await;
}

pub async fn wait_idle(s: &Arc<Session>) {
    let s2 = s.clone();
    wait_until("session idle", IDLE_TIMEOUT, move || {
        s2.lock().run.is_none()
    })
    .await;
}

/// A connection that finished Hello.
pub fn hello_conn(hub: &Arc<Hub>) -> Arc<Conn> {
    let conn = hub.add_conn();
    conn.lock().hello = true;
    conn
}

/// A connection attached to `s` in the foreground, with the attach frames drained.
pub async fn attached_conn(hub: &Arc<Hub>, s: &Arc<Session>) -> Arc<Conn> {
    let conn = hello_conn(hub);
    let attach = Request::Attach {
        session_id: s.id.clone(),
        foreground: true,
        limit: None,
    };
    ref_server::api::handle(hub, &conn, attach).await.unwrap();
    conn.out.drain();
    conn
}

/// Drains the connection and keeps the session events.
pub fn events(conn: &Conn) -> Vec<SessionEvent> {
    conn.out
        .drain()
        .into_iter()
        .filter_map(|f| match f {
            ServerFrame::Event { event, .. } => Some(event),
            _ => None,
        })
        .collect()
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
