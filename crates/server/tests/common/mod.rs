//! Test harness: isolated env dirs, a scripted mock OpenAI Chat Completions server, the real
//! `hernees-server` binary, and a WebSocket client.
#![allow(dead_code)]

use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Router;
use futures::{SinkExt, StreamExt};
use hn_proto::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message as WsMsg;

// ---------------------------------------------------------------------------
// Mock LLM
// ---------------------------------------------------------------------------

pub enum Reply {
    Text(String),
    /// (tool name, arguments JSON text)
    Tools(Vec<(String, String)>),
    Status(u16, String),
}

pub fn text(t: &str) -> Reply {
    Reply::Text(t.to_string())
}

pub fn tool(name: &str, args: Value) -> Reply {
    Reply::Tools(vec![(name.to_string(), args.to_string())])
}

pub type Script = Arc<dyn Fn(&Req) -> Reply + Send + Sync>;

/// A parsed chat completions request.
pub struct Req {
    pub body: Value,
    /// How many requests the mock had seen before this one (0-based).
    pub n: usize,
}

impl Req {
    pub fn messages(&self) -> &[Value] {
        self.body["messages"].as_array().map(|v| v.as_slice()).unwrap_or(&[])
    }
    pub fn system(&self) -> String {
        let mut parts: Vec<String> = self.messages().iter().filter(|m| m["role"] == "system").map(content_text).collect();
        // Anthropic bodies: top-level `system` blocks.
        if let Some(a) = self.body["system"].as_array() {
            parts.extend(a.iter().filter_map(|b| b["text"].as_str().map(String::from)));
        }
        parts.join("\n")
    }
    pub fn last(&self) -> &Value {
        self.messages().last().unwrap_or(&Value::Null)
    }
    pub fn last_role(&self) -> String {
        self.last()["role"].as_str().unwrap_or("").to_string()
    }
    pub fn last_text(&self) -> String {
        content_text(self.last())
    }
    /// Text of the last message with role user.
    pub fn last_user(&self) -> String {
        self.messages().iter().rev().find(|m| m["role"] == "user").map(content_text).unwrap_or_default()
    }
    pub fn all_text(&self) -> String {
        self.body["messages"].to_string()
    }
    pub fn is_title(&self) -> bool {
        self.system().contains("You name conversations")
    }
    pub fn session_id(&self) -> Option<String> {
        let sys = self.system();
        let re = regex::Regex::new(r"You are session id (ses_[a-z0-9]+)").unwrap();
        re.captures(&sys).map(|c| c[1].to_string())
    }
}

pub fn content_text(m: &Value) -> String {
    match &m["content"] {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

#[derive(Clone)]
struct MockState {
    script: Script,
    log: Arc<Mutex<Vec<Value>>>,
}

pub struct Mock {
    pub port: u16,
    pub log: Arc<Mutex<Vec<Value>>>,
}

impl Mock {
    pub async fn start(script: Script) -> Mock {
        let log = Arc::new(Mutex::new(Vec::new()));
        let st = MockState { script, log: log.clone() };
        let app = Router::new()
            .route("/v1/chat/completions", post(chat))
            .route("/v1/messages", post(anthropic))
            .with_state(st);
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            let _ = axum::serve(l, app).await;
        });
        Mock { port, log }
    }

    /// Non-title requests received so far.
    pub fn requests(&self) -> Vec<Value> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter(|b| !Req { body: (*b).clone(), n: 0 }.is_title())
            .cloned()
            .collect()
    }
}

async fn chat(State(st): State<MockState>, body: String) -> Response {
    let body: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    let n = {
        let mut l = st.log.lock().unwrap();
        l.push(body.clone());
        l.len() - 1
    };
    let req = Req { body, n };
    let reply = if req.is_title() { text("Mock title") } else { (st.script)(&req) };
    let mut events: Vec<Value> = Vec::new();
    match reply {
        Reply::Status(code, msg) => {
            return (StatusCode::from_u16(code).unwrap(), json!({"error": {"message": msg}}).to_string()).into_response();
        }
        Reply::Text(t) => {
            // Stream in two chunks to exercise deltas.
            let mid = t.char_indices().nth(t.chars().count() / 2).map(|x| x.0).unwrap_or(0);
            for chunk in [&t[..mid], &t[mid..]] {
                if !chunk.is_empty() {
                    events.push(json!({"choices":[{"index":0,"delta":{"content":chunk}}]}));
                }
            }
            events.push(json!({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],
                "usage":{"prompt_tokens":100,"completion_tokens":10}}));
        }
        Reply::Tools(calls) => {
            for (i, (name, args)) in calls.iter().enumerate() {
                let id = format!("call_{}_{i}", req.n);
                events.push(json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":i,"id":id,"type":"function","function":{"name":name,"arguments":""}}]}}]}));
                let mid = args.len() / 2;
                let mid = (0..=mid).rev().find(|m| args.is_char_boundary(*m)).unwrap_or(0);
                for chunk in [&args[..mid], &args[mid..]] {
                    events.push(json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":i,"function":{"arguments":chunk}}]}}]}));
                }
            }
            events.push(json!({"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],
                "usage":{"prompt_tokens":120,"completion_tokens":20}}));
        }
    }
    let mut sse = String::new();
    for e in events {
        sse.push_str(&format!("data: {e}\n\n"));
    }
    sse.push_str("data: [DONE]\n\n");
    Response::builder().header("content-type", "text/event-stream").body(Body::from(sse)).unwrap()
}

/// Anthropic Messages mock: every reply starts with a signed thinking block
/// (`thinking-{n}` / `sig-{n}`), then text or tool_use blocks.
async fn anthropic(State(st): State<MockState>, body: String) -> Response {
    let body: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    let n = {
        let mut l = st.log.lock().unwrap();
        l.push(body.clone());
        l.len() - 1
    };
    // Adapt to the chat-shaped helpers: system text as a system message, user text as last user.
    let sys: String = body["system"].as_array().map(|a| a.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n")).unwrap_or_default();
    let mut msgs = vec![json!({"role": "system", "content": sys})];
    for m in body["messages"].as_array().cloned().unwrap_or_default() {
        let text: String = m["content"].as_array().map(|a| a.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n")).unwrap_or_default();
        let only_results = m["content"].as_array().is_some_and(|a| a.iter().all(|b| b["type"] == "tool_result"));
        let role = if only_results { "tool" } else { m["role"].as_str().unwrap_or("") };
        msgs.push(json!({"role": role, "content": text}));
    }
    let req = Req { body: json!({"messages": msgs}), n };
    let reply = if req.is_title() { text("Mock title") } else { (st.script)(&req) };
    let mut ev: Vec<(&str, Value)> = vec![(
        "message_start",
        json!({"type":"message_start","message":{"id":format!("msg_{n}"),"type":"message","role":"assistant","content":[],"usage":{"input_tokens":100,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"output_tokens":1}}}),
    )];
    let mut idx = 0;
    let mut block = |ev: &mut Vec<(&str, Value)>, start: Value, deltas: Vec<Value>| {
        ev.push(("content_block_start", json!({"type":"content_block_start","index":idx,"content_block":start})));
        for d in deltas {
            ev.push(("content_block_delta", json!({"type":"content_block_delta","index":idx,"delta":d})));
        }
        ev.push(("content_block_stop", json!({"type":"content_block_stop","index":idx})));
        idx += 1;
    };
    if let Reply::Status(code, msg) = &reply {
        return (StatusCode::from_u16(*code).unwrap(), json!({"type":"error","error":{"type":"api_error","message":msg}}).to_string()).into_response();
    }
    block(
        &mut ev,
        json!({"type":"thinking","thinking":"","signature":""}),
        vec![json!({"type":"thinking_delta","thinking":format!("thinking-{n}")}), json!({"type":"signature_delta","signature":format!("sig-{n}")})],
    );
    let stop = match reply {
        Reply::Text(t) => {
            block(&mut ev, json!({"type":"text","text":""}), vec![json!({"type":"text_delta","text":t})]);
            "end_turn"
        }
        Reply::Tools(calls) => {
            for (i, (name, args)) in calls.iter().enumerate() {
                let mid = (0..=args.len() / 2).rev().find(|m| args.is_char_boundary(*m)).unwrap_or(0);
                block(
                    &mut ev,
                    json!({"type":"tool_use","id":format!("toolu_{n}_{i}"),"name":name,"input":{}}),
                    vec![
                        json!({"type":"input_json_delta","partial_json":&args[..mid]}),
                        json!({"type":"input_json_delta","partial_json":&args[mid..]}),
                    ],
                );
            }
            "tool_use"
        }
        Reply::Status(..) => unreachable!(),
    };
    ev.push(("message_delta", json!({"type":"message_delta","delta":{"stop_reason":stop},"usage":{"output_tokens":20}})));
    ev.push(("message_stop", json!({"type":"message_stop"})));
    let mut sse = String::new();
    for (name, data) in ev {
        sse.push_str(&format!("event: {name}\ndata: {data}\n\n"));
    }
    Response::builder().header("content-type", "text/event-stream").body(Body::from(sse)).unwrap()
}

/// Provider config for the Anthropic endpoint of the mock (`mocka/claude-sonnet-5-5`).
pub fn anthropic_provider(mock_port: u16) -> Value {
    json!({
        "npm": "@ai-sdk/anthropic",
        "options": {"baseURL": format!("http://127.0.0.1:{mock_port}/v1"), "apiKey": "test-key"},
        "models": {"claude-sonnet-5-5": {"reasoning": true, "limit": {"context": 200000, "output": 8000},
            "variants": {"high": {"reasoningEffort": "high"}}}}
    })
}

// ---------------------------------------------------------------------------
// Environment + server process
// ---------------------------------------------------------------------------

pub struct Env {
    pub dir: tempfile::TempDir,
}

impl Env {
    /// `extra` is merged into the top-level config object.
    pub fn new(mock_port: u16, extra: Value) -> Env {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for d in ["home", "oc", "hn", "data", "state", "work"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        let mut cfg = json!({
            "provider": {"mock": {
                "npm": "@ai-sdk/openai-compatible",
                "options": {"baseURL": format!("http://127.0.0.1:{mock_port}/v1"), "apiKey": "test-key"},
                "models": {"m": {"limit": {"context": 100000, "output": 1000}}}
            }},
            "model": "mock/m",
            "retry": {"initial_delay": 0.05, "max_delay": 1},
        });
        if let (Value::Object(c), Value::Object(e)) = (&mut cfg, extra) {
            for (k, v) in e {
                c.insert(k, v);
            }
        }
        std::fs::write(root.join("oc/opencode.json"), serde_json::to_string_pretty(&cfg).unwrap()).unwrap();
        Env { dir }
    }

    pub fn path(&self, p: &str) -> PathBuf {
        self.dir.path().join(p)
    }

    pub fn work(&self) -> String {
        std::fs::canonicalize(self.path("work")).unwrap().to_string_lossy().into_owned()
    }

    pub fn command(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_hernees-server"));
        let r = self.dir.path();
        c.env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", r.join("home"))
            .env("XDG_CONFIG_HOME", r.join("home/.config"))
            .env("OPENCODE_CONFIG_DIR", r.join("oc"))
            .env("HERNEES_CONFIG_DIR", r.join("hn"))
            .env("HERNEES_DATA_DIR", r.join("data"))
            .env("HERNEES_STATE_DIR", r.join("state"))
            .env("HERNEES_MODELS_PATH", r.join("no-models.json"))
            .env("HERNEES_LOG", "debug,hyper=info,reqwest=info")
            .current_dir(r.join("work"));
        c
    }

    pub fn spawn(&self, args: &[&str]) -> Server {
        let port = free_port();
        let log = std::fs::File::create(self.path(&format!("server-{port}.log"))).unwrap();
        let child = self
            .command()
            .args(["--port", &port.to_string()])
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(log))
            .spawn()
            .unwrap();
        Server { child, port, log: self.path(&format!("server-{port}.log")) }
    }
}

pub fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

pub struct Server {
    pub child: Child,
    pub port: u16,
    pub log: PathBuf,
}

impl Server {
    pub fn url(&self) -> String {
        format!("ws://127.0.0.1:{}/", self.port)
    }
    pub fn log_text(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
    /// Wait for the process to exit (polling).
    pub async fn wait_exit(&mut self, t: Duration) -> Option<std::process::ExitStatus> {
        let deadline = tokio::time::Instant::now() + t;
        while tokio::time::Instant::now() < deadline {
            if let Ok(Some(s)) = self.child.try_wait() {
                return Some(s);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        None
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------------------
// WS client
// ---------------------------------------------------------------------------

pub struct Client {
    tx: mpsc::UnboundedSender<String>,
    rx: mpsc::UnboundedReceiver<ServerMsg>,
    next: u64,
    /// Every non-response message received, in order.
    pub seen: Vec<ServerMsg>,
    pending: Vec<ServerMsg>,
}

impl Client {
    pub async fn connect(url: &str, cwd: &str) -> Client {
        let mut last = String::new();
        for _ in 0..100 {
            match tokio_tungstenite::connect_async(url).await {
                Ok((ws, _)) => {
                    let (mut sink, mut stream) = ws.split();
                    let (tx, mut orx) = mpsc::unbounded_channel::<String>();
                    let (itx, rx) = mpsc::unbounded_channel::<ServerMsg>();
                    tokio::spawn(async move {
                        while let Some(s) = orx.recv().await {
                            if sink.send(WsMsg::text(s)).await.is_err() {
                                break;
                            }
                        }
                        let _ = sink.close().await;
                    });
                    tokio::spawn(async move {
                        while let Some(Ok(m)) = stream.next().await {
                            if let WsMsg::Text(t) = m {
                                if let Ok(sm) = serde_json::from_str::<ServerMsg>(t.as_str()) {
                                    let _ = itx.send(sm);
                                }
                            }
                        }
                    });
                    let mut c = Client { tx, rx, next: 1, seen: Vec::new(), pending: Vec::new() };
                    let r = c.req_id(0, ClientMsg::Hello { client: "test".into(), cwd: cwd.into(), protocol: PROTOCOL_VERSION }).await;
                    r.expect("hello");
                    return c;
                }
                Err(e) => last = e.to_string(),
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("cannot connect to {url}: {last}");
    }

    pub fn close(self) {
        drop(self.tx);
    }

    async fn recv(&mut self, t: Duration) -> Option<ServerMsg> {
        tokio::time::timeout(t, self.rx.recv()).await.ok().flatten()
    }

    async fn req_id(&mut self, id: u64, msg: ClientMsg) -> Result<Value, String> {
        let s = serde_json::to_string(&Request { id, msg }).unwrap();
        self.tx.send(s).unwrap();
        loop {
            let m = self.recv(Duration::from_secs(20)).await.expect("response timeout");
            match m {
                ServerMsg::Response { id: rid, ok, error } if rid == id => {
                    return match error {
                        Some(e) => Err(e),
                        None => Ok(ok.unwrap_or(Value::Null)),
                    }
                }
                other => {
                    self.seen.push(other.clone());
                    self.pending.push(other);
                }
            }
        }
    }

    pub async fn req(&mut self, msg: ClientMsg) -> Result<Value, String> {
        let id = self.next;
        self.next += 1;
        self.req_id(id, msg).await
    }

    /// Wait until a received message satisfies `f` (checks buffered ones first).
    pub async fn wait_for(&mut self, what: &str, t: Duration, mut f: impl FnMut(&ServerMsg) -> bool) -> ServerMsg {
        if let Some(i) = self.pending.iter().position(&mut f) {
            let m = self.pending.remove(i);
            self.pending.drain(..i);
            return m;
        }
        self.pending.clear();
        let deadline = tokio::time::Instant::now() + t;
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            let Some(m) = self.recv(left).await else { panic!("timed out waiting for {what}") };
            self.seen.push(m.clone());
            if f(&m) {
                return m;
            }
        }
    }

    pub async fn create(&mut self, cwd: &str, name: Option<&str>) -> SessionInfo {
        let v = self
            .req(ClientMsg::CreateSession { cwd: cwd.into(), agent: None, model: None, name: name.map(String::from) })
            .await
            .expect("create session");
        serde_json::from_value(v).unwrap()
    }

    pub async fn prompt(&mut self, sid: &str, text: &str) {
        self.req(ClientMsg::Prompt { session: sid.into(), text: text.into(), images: Vec::new() }).await.expect("prompt");
    }

    /// Wait until `sid` is idle after having been busy (via Info events).
    pub async fn wait_idle(&mut self, sid: &str, t: Duration) {
        // Already busy (per the last Info seen)? Then only wait for idle.
        let last = self.seen.iter().rev().find_map(|m| match m {
            ServerMsg::Event { session, event: Event::Info { info } } if session == sid => Some(info.status.is_active()),
            _ => None,
        });
        let mut busy = last == Some(true);
        if busy {
            self.pending.retain(|m| !matches!(m, ServerMsg::Event { session, event: Event::Info { .. } } if session == sid));
        }
        self.wait_for("session idle", t, |m| match m {
            ServerMsg::Event { session, event: Event::Info { info } } if session == sid => {
                if info.status.is_active() {
                    busy = true;
                    false
                } else {
                    busy
                }
            }
            _ => false,
        })
        .await;
    }

    /// Full current state via a fresh Attach.
    pub async fn snapshot(&mut self, sid: &str) -> (SessionInfo, Vec<Message>) {
        // Older buffered snapshots are stale; the fresh one arrives right after the response.
        self.pending.retain(|m| !matches!(m, ServerMsg::Event { session, event: Event::Snapshot { .. } } if session == sid));
        self.req(ClientMsg::Attach { session: sid.into(), foreground: false }).await.expect("attach");
        let m = self
            .wait_for("snapshot", Duration::from_secs(10), |m| {
                matches!(m, ServerMsg::Event { session, event: Event::Snapshot { .. } } if session == sid)
            })
            .await;
        match m {
            ServerMsg::Event { event: Event::Snapshot { info, messages }, .. } => (info, messages),
            _ => unreachable!(),
        }
    }
}

pub fn tool_parts(msgs: &[Message]) -> Vec<ToolPart> {
    msgs.iter()
        .flat_map(|m| m.parts.iter())
        .filter_map(|p| match p {
            Part::Tool(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

pub fn last_assistant_text(msgs: &[Message]) -> String {
    msgs.iter().rev().find(|m| m.role == Role::Assistant).map(|m| m.raw_text()).unwrap_or_default()
}

pub fn exists(p: &Path) -> bool {
    p.exists()
}

pub const T: Duration = Duration::from_secs(20);
