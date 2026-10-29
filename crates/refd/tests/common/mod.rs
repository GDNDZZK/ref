//! Shared harness for the in-process integration tests: refd server + mock OpenAI
//! chat-completions SSE server + WebSocket client.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use futures::{SinkExt, StreamExt};
use parking_lot::Mutex;
use ref_proto::{ClientMsg, Message, Part, ServerMsg, SessionStatus, ToolPart, PROTOCOL_VERSION};
use refd::cli::ServeOpts;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message as WsMsg;

// ───────────────────────────── mock LLM ─────────────────────────────

pub enum Reply {
    Text(String),
    /// (reasoning, text)
    Reasoning(String, String),
    /// (tool name, arguments JSON)
    Tools(Vec<(String, Value)>),
    /// (tool name, raw argument string) — may be invalid JSON.
    RawTools(Vec<(String, String)>),
    Status(u16, String),
    /// Status with extra headers (e.g. retry-after).
    StatusWith(u16, Vec<(&'static str, String)>, String),
    /// Raw SSE events (complete `data: ...\n\n` strings) sent with `delay` between them;
    /// with `fail` the connection breaks after the last one.
    Stream { events: Vec<String>, delay: Duration, fail: bool },
    /// Never answers.
    Hang,
}

pub type Script = Arc<dyn Fn(&Value) -> Reply + Send + Sync>;

#[derive(Clone)]
pub struct Mock {
    pub script: Script,
    pub title: Script,
    pub requests: Arc<Mutex<Vec<Value>>>,
    /// prompt_tokens reported in usage.
    pub prompt_tokens: Arc<AtomicU64>,
}

pub fn chunk(delta: Value, finish: Option<&str>) -> String {
    let c = json!({"id": "c1", "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]});
    format!("data: {c}\n\n")
}

pub fn usage_event(prompt: u64) -> String {
    format!("data: {{\"id\":\"c1\",\"choices\":[],\"usage\":{{\"prompt_tokens\":{prompt},\"completion_tokens\":10}}}}\n\n")
}

pub fn text_events(t: &str) -> Vec<String> {
    let chars: Vec<char> = t.chars().collect();
    chars.chunks(3).map(|piece| chunk(json!({"content": piece.iter().collect::<String>()}), None)).collect()
}

fn rand_id() -> u32 {
    use std::sync::atomic::AtomicU32;
    static N: AtomicU32 = AtomicU32::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

fn tool_events(calls: &[(String, String)]) -> Vec<String> {
    let mut out = Vec::new();
    for (i, (name, args)) in calls.iter().enumerate() {
        let mid = args.char_indices().nth(args.chars().count() / 2).map(|x| x.0).unwrap_or(0);
        let (a, b) = args.split_at(mid);
        out.push(chunk(
            json!({"tool_calls": [{"index": i, "id": format!("call_{i}_{}", rand_id()), "type": "function", "function": {"name": name, "arguments": a}}]}),
            None,
        ));
        out.push(chunk(json!({"tool_calls": [{"index": i, "function": {"arguments": b}}]}), None));
    }
    out
}

/// Full SSE body for a complete reply.
pub fn sse(reply: &Reply, prompt_tokens: u64) -> String {
    let mut out: Vec<String> = Vec::new();
    match reply {
        Reply::Text(t) => {
            out.extend(text_events(t));
            out.push(chunk(json!({}), Some("stop")));
        }
        Reply::Reasoning(r, t) => {
            out.push(chunk(json!({"reasoning_content": r}), None));
            out.extend(text_events(t));
            out.push(chunk(json!({}), Some("stop")));
        }
        Reply::Tools(calls) => {
            let raw: Vec<(String, String)> = calls.iter().map(|(n, a)| (n.clone(), a.to_string())).collect();
            out.extend(tool_events(&raw));
            out.push(chunk(json!({}), Some("tool_calls")));
        }
        Reply::RawTools(calls) => {
            out.extend(tool_events(calls));
            out.push(chunk(json!({}), Some("tool_calls")));
        }
        _ => unreachable!(),
    }
    out.push(usage_event(prompt_tokens));
    out.push("data: [DONE]\n\n".into());
    out.concat()
}

async fn completions(State(m): State<Mock>, Json(body): Json<Value>) -> Response {
    m.requests.lock().push(body.clone());
    let reply = if body.get("tools").is_none() { (m.title)(&body) } else { (m.script)(&body) };
    let tokens = m.prompt_tokens.load(Ordering::Relaxed);
    match reply {
        Reply::Status(code, text) => {
            (axum::http::StatusCode::from_u16(code).unwrap(), Json(json!({"error": {"message": text}}))).into_response()
        }
        Reply::StatusWith(code, headers, text) => {
            let mut r =
                (axum::http::StatusCode::from_u16(code).unwrap(), Json(json!({"error": {"message": text}}))).into_response();
            for (k, v) in headers {
                r.headers_mut().insert(k, v.parse().unwrap());
            }
            r
        }
        Reply::Hang => {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            ().into_response()
        }
        Reply::Stream { events, delay, fail } => {
            let s = futures::stream::unfold((events.into_iter(), fail, false), move |(mut it, fail, started)| async move {
                if started {
                    tokio::time::sleep(delay).await;
                }
                match it.next() {
                    Some(c) => Some((Ok::<_, std::io::Error>(Bytes::from(c)), (it, fail, true))),
                    None if fail => Some((Err(std::io::Error::other("connection reset")), (it, false, true))),
                    None => None,
                }
            });
            ([("content-type", "text/event-stream")], Body::from_stream(s)).into_response()
        }
        r => ([("content-type", "text/event-stream")], sse(&r, tokens)).into_response(),
    }
}

pub async fn start_mock(mock: Mock) -> String {
    let app = Router::new().route("/v1/chat/completions", post(completions)).with_state(mock);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(l, app).await.unwrap();
    });
    format!("http://{addr}/v1")
}

/// (role, text) of the last message in a chat-completions request body.
pub fn last_content(body: &Value) -> (String, String) {
    let msgs = body["messages"].as_array().cloned().unwrap_or_default();
    let last = msgs.last().cloned().unwrap_or_default();
    let role = last["role"].as_str().unwrap_or("").to_string();
    (role, content_text(&last["content"]))
}

pub fn content_text(c: &Value) -> String {
    match c {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

/// Compaction summary requests are sent with `tool_choice: none`.
pub fn is_summary_request(body: &Value) -> bool {
    body["tool_choice"] == "none"
}

/// Whether any message of the request contains `needle`.
pub fn request_contains(body: &Value, needle: &str) -> bool {
    body["messages"].as_array().into_iter().flatten().any(|m| content_text(&m["content"]).contains(needle))
}

/// Parse `CALL [["tool", {args}], ...]` from any line of `text`. `purpose` is added when missing.
pub fn parse_calls(text: &str) -> Option<Vec<(String, Value)>> {
    let line = text.lines().find_map(|l| l.trim().strip_prefix("CALL "))?;
    let v: Value = serde_json::from_str(line).ok()?;
    let mut out = Vec::new();
    for c in v.as_array()? {
        let name = c[0].as_str()?.to_string();
        let mut args = c.get(1).cloned().unwrap_or(json!({}));
        if let Some(o) = args.as_object_mut() {
            o.entry("purpose").or_insert(json!("test purpose"));
        }
        out.push((name, args));
    }
    Some(out)
}

/// Generic script: summary requests → "SUMMARY of work"; last message is a tool result →
/// "done"; continuation after a compaction → "continued"; a `CALL [...]` line → those tool
/// calls; else `echo: <first line>`.
pub fn cmd_script() -> Script {
    Arc::new(|body| {
        if is_summary_request(body) {
            return Reply::Text("SUMMARY of work".into());
        }
        let (role, content) = last_content(body);
        if role == "tool" {
            return Reply::Text("done".into());
        }
        if content.starts_with("Summary of the conversation so far") {
            return Reply::Text("continued".into());
        }
        if let Some(calls) = parse_calls(&content) {
            return Reply::Tools(calls);
        }
        Reply::Text(format!("echo: {}", content.lines().next().unwrap_or("")))
    })
}

pub fn call(calls: Value) -> String {
    format!("CALL {calls}")
}

// ───────────────────────────── harness ─────────────────────────────

pub struct Env {
    pub _tmp: Arc<tempfile::TempDir>,
    pub root: PathBuf,
    pub cwd: String,
    pub data_dir: PathBuf,
    pub paths: ref_config::ConfigPaths,
    pub server: refd::server::Server,
    pub mock: Mock,
    pub requests: Arc<Mutex<Vec<Value>>>,
    pub idle_timeout: i64,
    pub registry: bool,
}

pub struct Opts {
    pub script: Script,
    pub title: Script,
    pub extra: Value,
    pub idle_timeout: i64,
    pub registry: bool,
}

impl Opts {
    pub fn new(script: Script) -> Self {
        Opts {
            script,
            title: Arc::new(|_| Reply::Text("Generated title".into())),
            extra: json!({}),
            idle_timeout: -1,
            registry: false,
        }
    }
    pub fn extra(mut self, v: Value) -> Self {
        self.extra = v;
        self
    }
}

pub fn deep_merge(a: &mut Value, b: &Value) {
    match (a, b) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in b {
                deep_merge(a.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        (a, b) => *a = b.clone(),
    }
}

pub async fn setup(script: Script, extra: Value) -> Env {
    setup_opts(Opts::new(script).extra(extra)).await
}

pub async fn setup_opts(o: Opts) -> Env {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mock = Mock { script: o.script, title: o.title, requests: requests.clone(), prompt_tokens: Arc::new(AtomicU64::new(100)) };
    let base = start_mock(mock.clone()).await;
    let tmp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap();
    let home = root.join("home");
    let global = home.join(".config/opencode");
    std::fs::create_dir_all(&global).unwrap();
    let work = root.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let mut cfg = json!({
        "model": "mock/m",
        "provider": {"mock": {"npm": "@ai-sdk/openai-compatible", "name": "Mock",
            "options": {"baseURL": base, "apiKey": "sk-test"},
            "models": {
                "m": {"name": "Mock Model", "limit": {"context": 100000, "output": 4000}},
                "small": {"name": "Small Model", "limit": {"context": 50000, "output": 2000}}
            }}}
    });
    deep_merge(&mut cfg, &o.extra);
    std::fs::write(global.join("opencode.json"), cfg.to_string()).unwrap();
    let paths = ref_config::ConfigPaths {
        global_dir: global,
        ref_global_dir: home.join(".config/ref"),
        catalog_path: home.join(".cache/opencode/models.json"),
        auth_path: home.join(".local/share/opencode/auth.json"),
        home,
        ..Default::default()
    };
    let data_dir = root.join("data");
    let server = start_server(&data_dir, &paths, o.idle_timeout, o.registry).await;
    Env {
        cwd: work.to_string_lossy().into_owned(),
        root,
        _tmp: Arc::new(tmp),
        data_dir,
        paths,
        server,
        mock,
        requests,
        idle_timeout: o.idle_timeout,
        registry: o.registry,
    }
}

pub async fn start_server(
    data_dir: &std::path::Path,
    paths: &ref_config::ConfigPaths,
    idle_timeout: i64,
    registry: bool,
) -> refd::server::Server {
    let opts = ServeOpts {
        port: 0,
        bind: None,
        allow_hosts: vec![],
        idle_timeout,
        data_dir: data_dir.to_path_buf(),
        registry,
        signals: false,
    };
    refd::server::start(opts, Some(paths.clone())).await.unwrap()
}

impl Env {
    /// Gracefully stop the server and start a new one on the same data dir.
    pub async fn restart(&mut self) {
        let fresh = start_server(&self.data_dir, &self.paths, self.idle_timeout, self.registry).await;
        let old = std::mem::replace(&mut self.server, fresh);
        old.shutdown();
        tokio::time::timeout(Duration::from_secs(20), old.wait()).await.expect("old server did not stop");
    }

    /// Agent-loop requests (title requests have no tools).
    pub fn llm_requests(&self) -> Vec<Value> {
        self.requests.lock().iter().filter(|r| r.get("tools").is_some()).cloned().collect()
    }

    pub fn title_requests(&self) -> Vec<Value> {
        self.requests.lock().iter().filter(|r| r.get("tools").is_none()).cloned().collect()
    }

    pub fn core(&self, sid: &str) -> Arc<refd::session::SessionCore> {
        self.server.hub.session(sid).unwrap_or_else(|| panic!("session {sid} not open"))
    }

    pub fn messages(&self, sid: &str) -> Vec<Message> {
        self.core(sid).st.lock().messages.clone()
    }

    pub fn tool_parts(&self, sid: &str) -> Vec<ToolPart> {
        tool_parts(&self.messages(sid))
    }

    /// Wait until the session is idle and its last message is a finished assistant message.
    pub async fn wait_done(&self, sid: &str) {
        let core = self.core(sid);
        wait_until(Duration::from_secs(20), || {
            let st = core.st.lock();
            st.turn.is_none()
                && st.info.status == SessionStatus::Idle
                && st.messages.last().is_some_and(|m| m.role == ref_proto::Role::Assistant && m.finish.is_some())
        })
        .await;
    }

    pub fn write_config(&self, cfg: &Value) {
        std::fs::write(self.paths.global_dir.join("opencode.json"), cfg.to_string()).unwrap();
        self.server.hub.clear_config_cache();
    }
}

pub fn tool_parts(messages: &[Message]) -> Vec<ToolPart> {
    messages
        .iter()
        .flat_map(|m| m.parts.iter())
        .filter_map(|p| match p {
            Part::Tool(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

/// Poll `cond` every 20ms; panics after `timeout`.
pub async fn wait_until(timeout: Duration, mut cond: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if cond() {
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("condition not reached within {timeout:?}");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

pub type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

pub struct Client {
    pub ws: Ws,
    pub log: Vec<ServerMsg>,
}

impl Client {
    pub async fn connect(env: &Env) -> Client {
        Self::connect_to(&env.server, &env.cwd).await
    }

    pub async fn connect_to(server: &refd::server::Server, cwd: &str) -> Client {
        let url = format!("ws://{}/ws?token={}", server.addr, server.token);
        let cfg = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
            .max_message_size(Some(512 << 20))
            .max_frame_size(Some(512 << 20));
        let (ws, _) = tokio_tungstenite::connect_async_with_config(url, Some(cfg), false).await.unwrap();
        let mut c = Client { ws, log: vec![] };
        c.send(ClientMsg::Hello { client: "test".into(), cwd: cwd.to_string(), protocol: PROTOCOL_VERSION }).await;
        let w = c.until(|m| matches!(m, ServerMsg::Welcome { .. })).await;
        let ServerMsg::Welcome { catalog, .. } = w else { unreachable!() };
        assert!(catalog.models.iter().any(|m| m.id == "mock/m"), "{:?}", catalog.models);
        c
    }

    pub async fn send(&mut self, m: ClientMsg) {
        self.ws.send(WsMsg::text(serde_json::to_string(&m).unwrap())).await.unwrap();
    }

    pub async fn send_raw(&mut self, text: &str) {
        self.ws.send(WsMsg::text(text.to_string())).await.unwrap();
    }

    /// Read messages until `pred` matches (20s timeout).
    pub async fn until(&mut self, pred: impl Fn(&ServerMsg) -> bool) -> ServerMsg {
        self.until_for(Duration::from_secs(20), pred).await.expect("timed out waiting for server message")
    }

    pub async fn until_for(&mut self, timeout: Duration, pred: impl Fn(&ServerMsg) -> bool) -> Option<ServerMsg> {
        let fut = async {
            loop {
                let m = self.ws.next().await.expect("socket closed").expect("ws error");
                let Ok(t) = m.to_text() else { continue };
                if t.is_empty() {
                    continue;
                }
                let msg: ServerMsg = serde_json::from_str(t).unwrap_or_else(|e| panic!("bad server msg {t}: {e}"));
                self.log.push(msg.clone());
                if pred(&msg) {
                    return msg;
                }
            }
        };
        tokio::time::timeout(timeout, fut).await.ok()
    }

    pub async fn create(&mut self, cwd: &str) -> String {
        self.create_with(cwd, None, None, None).await.id
    }

    pub async fn create_with(
        &mut self,
        cwd: &str,
        agent: Option<&str>,
        model: Option<&str>,
        name: Option<&str>,
    ) -> ref_proto::SessionInfo {
        let req = 1000 + rand_id() as u64;
        self.send(ClientMsg::CreateSession {
            req,
            cwd: cwd.into(),
            agent: agent.map(String::from),
            model: model.map(String::from),
            variant: None,
            name: name.map(String::from),
        })
        .await;
        match self.until(move |m| matches!(m, ServerMsg::SessionSnapshot { req: r, .. } | ServerMsg::Error { req: r, .. } if *r == req)).await
        {
            ServerMsg::SessionSnapshot { info, .. } => {
                assert_eq!(info.attached, 1);
                info
            }
            other => panic!("create failed: {other:?}"),
        }
    }

    pub async fn snapshot(&mut self, sid: &str) -> (ref_proto::SessionInfo, Vec<Message>, Vec<ref_proto::PermissionRequest>) {
        let req = 2000 + rand_id() as u64;
        self.send(ClientMsg::Attach { req, session_id: sid.into() }).await;
        match self.until(move |m| matches!(m, ServerMsg::SessionSnapshot { req: r, .. } | ServerMsg::Error { req: r, .. } if *r == req)).await
        {
            ServerMsg::SessionSnapshot { info, messages, permissions, .. } => (info, messages, permissions),
            other => panic!("attach failed: {other:?}"),
        }
    }

    pub async fn wait_status(&mut self, sid: &str, pred: impl Fn(&SessionStatus) -> bool) -> ref_proto::SessionInfo {
        let sid = sid.to_string();
        match self.until(move |m| matches!(m, ServerMsg::SessionUpdated { info } if info.id == sid && pred(&info.status))).await {
            ServerMsg::SessionUpdated { info } => info,
            _ => unreachable!(),
        }
    }

    /// Prompt an idle session (attached / watched by this client) and wait for the turn to end.
    pub async fn turn(&mut self, sid: &str, text: &str) {
        self.send(ClientMsg::Prompt { session_id: sid.into(), text: text.into(), images: vec![] }).await;
        self.wait_status(sid, |s| *s == SessionStatus::Busy).await;
        self.wait_status(sid, |s| *s == SessionStatus::Idle).await;
    }

    pub async fn prompt(&mut self, sid: &str, text: &str) {
        self.send(ClientMsg::Prompt { session_id: sid.into(), text: text.into(), images: vec![] }).await;
    }

    pub async fn until_idle_with(&mut self, sid: &str, text: &str) {
        let (sid, text) = (sid.to_string(), text.to_string());
        let sid2 = sid.clone();
        self.until(move |m| match m {
            ServerMsg::MessageUpsert { session_id, message } => {
                *session_id == sid && message.finish.as_deref() == Some("stop") && message.raw_text() == text
            }
            _ => false,
        })
        .await;
        self.until(move |m| matches!(m, ServerMsg::SessionUpdated { info } if info.id == sid2 && info.status == SessionStatus::Idle))
            .await;
    }
}

pub fn tool_result_content(body: &Value) -> Vec<String> {
    body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| m["content"].as_str().unwrap_or("").to_string())
        .collect()
}

pub fn tool_names(body: &Value) -> Vec<String> {
    body["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|t| t["function"]["name"].as_str().unwrap_or("").to_string())
        .collect()
}

pub fn system_text(body: &Value) -> String {
    content_text(&body["messages"][0]["content"])
}
