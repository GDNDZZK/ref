//! In-process integration tests: refd server + mock OpenAI chat-completions SSE server +
//! WebSocket client.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use futures::{SinkExt, StreamExt};
use parking_lot::Mutex;
use ref_proto::{ClientMsg, MessageOrigin, Part, PermissionDecision, ServerMsg, SessionStatus, ToolState, PROTOCOL_VERSION};
use refd::cli::ServeOpts;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message as WsMsg;

// ───────────────────────────── mock LLM ─────────────────────────────

pub enum Reply {
    Text(String),
    /// (tool name, arguments JSON)
    Tools(Vec<(String, Value)>),
    Status(u16, String),
}

type Script = Arc<dyn Fn(&Value) -> Reply + Send + Sync>;

#[derive(Clone)]
struct Mock {
    script: Script,
    requests: Arc<Mutex<Vec<Value>>>,
}

fn chunk(delta: Value, finish: Option<&str>) -> String {
    let c = json!({"id": "c1", "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]});
    format!("data: {c}\n\n")
}

fn sse(reply: &Reply) -> String {
    let mut out = String::new();
    match reply {
        Reply::Text(t) => {
            // Split into a few deltas to exercise delta batching.
            let chars: Vec<char> = t.chars().collect();
            for piece in chars.chunks(3) {
                out.push_str(&chunk(json!({"content": piece.iter().collect::<String>()}), None));
            }
            out.push_str(&chunk(json!({}), Some("stop")));
        }
        Reply::Tools(calls) => {
            for (i, (name, args)) in calls.iter().enumerate() {
                let args = args.to_string();
                let (a, b) = args.split_at(args.len() / 2);
                out.push_str(&chunk(
                    json!({"tool_calls": [{"index": i, "id": format!("call_{i}_{}", rand_id()), "type": "function", "function": {"name": name, "arguments": a}}]}),
                    None,
                ));
                out.push_str(&chunk(json!({"tool_calls": [{"index": i, "function": {"arguments": b}}]}), None));
            }
            out.push_str(&chunk(json!({}), Some("tool_calls")));
        }
        Reply::Status(..) => unreachable!(),
    }
    out.push_str("data: {\"id\":\"c1\",\"choices\":[],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":10}}\n\n");
    out.push_str("data: [DONE]\n\n");
    out
}

fn rand_id() -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

async fn completions(State(m): State<Mock>, Json(body): Json<Value>) -> Response {
    m.requests.lock().push(body.clone());
    let reply = if body.get("tools").is_none() { Reply::Text("Generated title".into()) } else { (m.script)(&body) };
    match reply {
        Reply::Status(code, text) => {
            (axum::http::StatusCode::from_u16(code).unwrap(), Json(json!({"error": {"message": text}}))).into_response()
        }
        r => ([("content-type", "text/event-stream")], sse(&r)).into_response(),
    }
}

async fn start_mock(script: Script) -> (String, Arc<Mutex<Vec<Value>>>) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mock = Mock { script, requests: requests.clone() };
    let app = Router::new().route("/v1/chat/completions", post(completions)).with_state(mock);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(l, app).await.unwrap();
    });
    (format!("http://{addr}/v1"), requests)
}

/// Text of the last message in a chat-completions request body.
fn last_content(body: &Value) -> (String, String) {
    let msgs = body["messages"].as_array().cloned().unwrap_or_default();
    let last = msgs.last().cloned().unwrap_or_default();
    let role = last["role"].as_str().unwrap_or("").to_string();
    let content = match &last["content"] {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    };
    (role, content)
}

// ───────────────────────────── harness ─────────────────────────────

struct Env {
    _tmp: tempfile::TempDir,
    cwd: String,
    server: refd::server::Server,
    requests: Arc<Mutex<Vec<Value>>>,
}

async fn setup(script: Script, extra_config: Value) -> Env {
    let (base, requests) = start_mock(script).await;
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
            "models": {"m": {"name": "Mock Model", "limit": {"context": 100000, "output": 4000}}}}}
    });
    if let (Some(c), Some(e)) = (cfg.as_object_mut(), extra_config.as_object()) {
        for (k, v) in e {
            c.insert(k.clone(), v.clone());
        }
    }
    std::fs::write(global.join("opencode.json"), cfg.to_string()).unwrap();
    let paths = ref_config::ConfigPaths {
        global_dir: global,
        ref_global_dir: home.join(".config/ref"),
        catalog_path: home.join(".cache/opencode/models.json"),
        auth_path: home.join(".local/share/opencode/auth.json"),
        home,
        ..Default::default()
    };
    let opts = ServeOpts {
        port: 0,
        bind: None,
        allow_hosts: vec![],
        idle_timeout: -1,
        data_dir: root.join("data"),
        registry: false,
        signals: false,
    };
    let server = refd::server::start(opts, Some(paths)).await.unwrap();
    Env { cwd: work.to_string_lossy().into_owned(), _tmp: tmp, server, requests }
}

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct Client {
    ws: Ws,
    log: Vec<ServerMsg>,
}

impl Client {
    async fn connect(env: &Env) -> Client {
        let url = format!("ws://{}/ws?token={}", env.server.addr, env.server.token);
        let (ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
        let mut c = Client { ws, log: vec![] };
        c.send(ClientMsg::Hello { client: "test".into(), cwd: env.cwd.clone(), protocol: PROTOCOL_VERSION }).await;
        let w = c.until(|m| matches!(m, ServerMsg::Welcome { .. })).await;
        let ServerMsg::Welcome { catalog, .. } = w else { unreachable!() };
        assert!(catalog.models.iter().any(|m| m.id == "mock/m"), "{:?}", catalog.models);
        c
    }

    async fn send(&mut self, m: ClientMsg) {
        self.ws.send(WsMsg::text(serde_json::to_string(&m).unwrap())).await.unwrap();
    }

    /// Read messages until `pred` matches (10s timeout).
    async fn until(&mut self, pred: impl Fn(&ServerMsg) -> bool) -> ServerMsg {
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
        tokio::time::timeout(Duration::from_secs(20), fut).await.expect("timed out waiting for server message")
    }

    async fn create(&mut self, cwd: &str) -> String {
        self.send(ClientMsg::CreateSession { req: 1, cwd: cwd.into(), agent: None, model: None, variant: None, name: None })
            .await;
        match self.until(|m| matches!(m, ServerMsg::SessionSnapshot { req: 1, .. } | ServerMsg::Error { .. })).await {
            ServerMsg::SessionSnapshot { info, .. } => {
                assert_eq!(info.attached, 1);
                info.id
            }
            other => panic!("create failed: {other:?}"),
        }
    }

    async fn until_idle_with(&mut self, sid: &str, text: &str) {
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

fn tool_result_content(body: &Value) -> Vec<String> {
    body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| m["content"].as_str().unwrap_or("").to_string())
        .collect()
}

// ───────────────────────────── tests ─────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn prompt_tool_loop_and_persistence() {
    let script: Script = Arc::new(|body| {
        let (role, _) = last_content(body);
        if role == "tool" {
            Reply::Text("All done.".into())
        } else {
            Reply::Tools(vec![
                ("bash".into(), json!({"purpose": "say hi", "command": "echo hi"})),
                ("context_info".into(), json!({"purpose": "check context"})),
            ])
        }
    });
    let env = setup(script, json!({})).await;
    let mut c = Client::connect(&env).await;
    let sid = c.create(&env.cwd).await;
    c.send(ClientMsg::Prompt { session_id: sid.clone(), text: "run it".into(), images: vec![] }).await;
    c.until_idle_with(&sid, "All done.").await;

    // Tool parts finished with outputs.
    let tools: Vec<_> = c
        .log
        .iter()
        .filter_map(|m| match m {
            ServerMsg::PartUpsert { part: Part::Tool(t), .. } if t.state.is_final() => Some(t.clone()),
            _ => None,
        })
        .collect();
    let bash = tools.iter().find(|t| t.name == "bash").expect("bash part");
    assert_eq!(bash.state, ToolState::Done, "{}", bash.output);
    assert!(bash.output.contains("hi"), "{}", bash.output);
    assert_eq!(bash.purpose, "say hi");
    let ci = tools.iter().find(|t| t.name == "context_info").expect("context_info part");
    assert!(ci.output.starts_with("Context: used"), "{}", ci.output);
    // Deltas were streamed.
    assert!(c.log.iter().any(|m| matches!(m, ServerMsg::PartDelta { .. })));

    // Request shape: purpose injected first + required; tool results sent back; stable prefix.
    let reqs: Vec<Value> = env.requests.lock().iter().filter(|r| r.get("tools").is_some()).cloned().collect();
    assert_eq!(reqs.len(), 2);
    let tool0 = &reqs[0]["tools"][0]["function"]["parameters"];
    assert_eq!(tool0["properties"].as_object().unwrap().keys().next().unwrap(), "purpose");
    assert_eq!(tool0["required"][0], "purpose");
    assert_eq!(reqs[0]["tools"], reqs[1]["tools"]);
    assert_eq!(reqs[0]["messages"][0], reqs[1]["messages"][0], "system prompt must be stable");
    let sys = reqs[0]["messages"][0]["content"].to_string();
    assert!(sys.contains("You are ref") && sys.contains(&sid), "{sys}");
    let results = tool_result_content(&reqs[1]);
    assert!(results.iter().any(|r| r.contains("hi")), "{results:?}");
    assert!(results.iter().any(|r| r.starts_with("Context: used")), "{results:?}");

    // Session list + persistence on close; re-attach loads it from history.
    c.send(ClientMsg::Close { session_id: sid.clone() }).await;
    let s2 = sid.clone();
    c.until(move |m| matches!(m, ServerMsg::SessionClosed { session_id } if *session_id == s2)).await;
    c.send(ClientMsg::ListSessions { req: 7 }).await;
    let ServerMsg::SessionList { sessions, .. } = c.until(|m| matches!(m, ServerMsg::SessionList { req: 7, .. })).await else {
        unreachable!()
    };
    let e = sessions.iter().find(|s| s.id == sid).expect("in history");
    assert!(!e.open);
    assert_eq!(e.message_count, 3);
    c.send(ClientMsg::Attach { req: 9, session_id: sid.clone() }).await;
    let ServerMsg::SessionSnapshot { info, messages, .. } = c.until(|m| matches!(m, ServerMsg::SessionSnapshot { req: 9, .. })).await
    else {
        unreachable!()
    };
    assert!(info.open);
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[2].raw_text(), "All done.");

    // Undo removes the prompt and everything after it.
    c.send(ClientMsg::Undo { session_id: sid.clone() }).await;
    let ServerMsg::Undone { text, .. } = c.until(|m| matches!(m, ServerMsg::Undone { .. })).await else { unreachable!() };
    assert_eq!(text, "run it");
    env.server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retries_after_server_error() {
    let calls = Arc::new(Mutex::new(0u32));
    let calls2 = calls.clone();
    let script: Script = Arc::new(move |_| {
        let mut n = calls2.lock();
        *n += 1;
        if *n == 1 {
            Reply::Status(500, "internal server error".into())
        } else {
            Reply::Text("Recovered".into())
        }
    });
    let env = setup(script, json!({})).await;
    let mut c = Client::connect(&env).await;
    let sid = c.create(&env.cwd).await;
    c.send(ClientMsg::Prompt { session_id: sid.clone(), text: "hello".into(), images: vec![] }).await;
    let s2 = sid.clone();
    let m = c
        .until(move |m| matches!(m, ServerMsg::SessionUpdated { info } if info.id == s2 && matches!(info.status, SessionStatus::Retrying { .. })))
        .await;
    let ServerMsg::SessionUpdated { info } = m else { unreachable!() };
    let SessionStatus::Retrying { attempt, error, .. } = info.status else { unreachable!() };
    assert_eq!(attempt, 1);
    assert!(error.contains("500"), "{error}");
    c.until_idle_with(&sid, "Recovered").await;
    // The failed partial assistant message is gone: user + one assistant.
    let s3 = sid.clone();
    c.send(ClientMsg::Watch { session_id: sid.clone() }).await;
    let ServerMsg::SessionSnapshot { messages, .. } =
        c.until(move |m| matches!(m, ServerMsg::SessionSnapshot { req: 0, info, .. } if info.id == s3)).await
    else {
        unreachable!()
    };
    assert_eq!(messages.len(), 2);
    assert_eq!(*calls.lock(), 2);
    env.server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn permission_ask_and_deny() {
    let script: Script = Arc::new(|body| {
        let (role, _) = last_content(body);
        if role == "tool" {
            Reply::Text("ok".into())
        } else {
            Reply::Tools(vec![
                ("bash".into(), json!({"purpose": "first", "command": "echo one"})),
                ("bash".into(), json!({"purpose": "second", "command": "echo two"})),
            ])
        }
    });
    let env = setup(script, json!({"permission": {"bash": {"*": "ask", "echo two": "deny"}}})).await;
    let mut c = Client::connect(&env).await;
    let sid = c.create(&env.cwd).await;
    c.send(ClientMsg::Prompt { session_id: sid.clone(), text: "go".into(), images: vec![] }).await;
    let ServerMsg::PermissionAsk { request } = c.until(|m| matches!(m, ServerMsg::PermissionAsk { .. })).await else {
        unreachable!()
    };
    assert_eq!(request.tool, "bash");
    assert_eq!(request.purpose, "first");
    assert_eq!(request.input["command"], "echo one");
    c.send(ClientMsg::Permission { session_id: sid.clone(), call_id: request.call_id.clone(), decision: PermissionDecision::Once })
        .await;
    c.until_idle_with(&sid, "ok").await;
    let reqs: Vec<Value> = env.requests.lock().iter().filter(|r| r.get("tools").is_some()).cloned().collect();
    let results = tool_result_content(reqs.last().unwrap());
    assert!(results.iter().any(|r| r.contains("one")), "{results:?}");
    assert!(results.iter().any(|r| r.contains("Permission denied by configuration")), "{results:?}");
    env.server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn abort_cancels_running_tool() {
    let script: Script = Arc::new(|body| {
        let (role, _) = last_content(body);
        if role == "tool" {
            Reply::Text("after".into())
        } else {
            Reply::Tools(vec![("bash".into(), json!({"purpose": "wait", "command": "sleep 30"}))])
        }
    });
    let env = setup(script, json!({})).await;
    let mut c = Client::connect(&env).await;
    let sid = c.create(&env.cwd).await;
    c.send(ClientMsg::Prompt { session_id: sid.clone(), text: "sleep".into(), images: vec![] }).await;
    c.until(|m| matches!(m, ServerMsg::PartUpsert { part: Part::Tool(t), .. } if t.state == ToolState::Running)).await;
    c.send(ClientMsg::Abort { session_id: sid.clone() }).await;
    let ServerMsg::PartUpsert { part: Part::Tool(t), .. } =
        c.until(|m| matches!(m, ServerMsg::PartUpsert { part: Part::Tool(t), .. } if t.state.is_final())).await
    else {
        unreachable!()
    };
    assert_eq!(t.state, ToolState::Cancelled);
    let s2 = sid.clone();
    c.until(move |m| matches!(m, ServerMsg::SessionUpdated { info } if info.id == s2 && info.status == SessionStatus::Idle))
        .await;
    env.server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn spawn_must_report_and_peer_messages() {
    let script: Script = Arc::new(|body| {
        let (role, content) = last_content(body);
        if role == "tool" {
            if content.starts_with("Created background session") {
                return Reply::Text("waiting for child".into());
            }
            return Reply::Text("child reported".into());
        }
        if content.starts_with("[Task from session") {
            return Reply::Text("did the work".into());
        }
        if content.starts_with("Reminder:") {
            let id = content.split("(id ").nth(1).and_then(|s| s.split(')').next()).unwrap_or("").to_string();
            return Reply::Tools(vec![("send_message".into(), json!({"purpose": "report", "to": id, "message": "REPORT: done"}))]);
        }
        if content.starts_with("[Message from session") {
            return Reply::Text("thanks".into());
        }
        Reply::Tools(vec![(
            "create_session".into(),
            json!({"purpose": "delegate", "name": "子任务", "agent": "build", "prompt": "do the work", "must_report": true}),
        )])
    });
    let env = setup(script, json!({})).await;
    let mut c = Client::connect(&env).await;
    let sid = c.create(&env.cwd).await;
    c.send(ClientMsg::Prompt { session_id: sid.clone(), text: "delegate".into(), images: vec![] }).await;
    // Parent eventually receives the peer report and answers "thanks".
    c.until_idle_with(&sid, "thanks").await;
    let hub = &env.server.hub;
    let child = hub.open_sessions().into_iter().find(|s| s.id != sid).expect("child session");
    let info = child.info();
    assert_eq!(info.name, "子任务");
    assert_eq!(info.parent.as_deref(), Some(sid.as_str()));
    assert!(info.must_report);
    assert_eq!(info.attached, 0);
    let msgs = child.st.lock().messages.clone();
    assert!(matches!(msgs[0].origin, MessageOrigin::Spawn { .. }));
    assert!(msgs.iter().any(|m| m.origin == MessageOrigin::Reminder));
    let parent = hub.session(&sid).unwrap();
    let pmsgs = parent.st.lock().messages.clone();
    let peer = pmsgs.iter().find(|m| matches!(m.origin, MessageOrigin::Peer { .. })).expect("peer message");
    assert!(peer.raw_text().contains("REPORT: done"));
    assert!(peer.raw_text().starts_with("[Message from session \"子任务\""));
    env.server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn status_endpoint_and_auth() {
    let env = setup(Arc::new(|_| Reply::Text("x".into())), json!({})).await;
    let entry = refd::registry::RegistryEntry { instance: env.server.hub.instance.clone(), token: env.server.token.clone() };
    let st = refd::registry::probe(&entry).await.expect("status reply");
    assert_eq!(st.instance.pid, std::process::id());
    assert_eq!(st.sessions, 0);
    let bad = refd::registry::RegistryEntry { token: "wrong".into(), ..entry };
    assert!(refd::registry::probe(&bad).await.is_none());
    let url = format!("ws://{}/ws?token=nope", env.server.addr);
    assert!(tokio_tungstenite::connect_async(url).await.is_err());
    env.server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn compact_tool_and_queued_prompt() {
    let script: Script = Arc::new(move |body| {
        let (role, content) = last_content(body);
        if content.contains("Summary of the conversation so far") {
            return Reply::Text("continued".into());
        }
        if role == "user" && content == "second prompt" {
            // The queued prompt arrives right after the tool results of the first step.
            let msgs = body["messages"].as_array().unwrap();
            assert_eq!(msgs[msgs.len() - 2]["role"], "tool");
            return Reply::Tools(vec![("compact".into(), json!({"purpose": "free context", "summary": "We did X."}))]);
        }
        if role == "tool" {
            return Reply::Text("unexpected".into());
        }
        Reply::Tools(vec![("bash".into(), json!({"purpose": "slow", "command": "sleep 1; echo slow"}))])
    });
    let env = setup(script, json!({})).await;
    let mut c = Client::connect(&env).await;
    let sid = c.create(&env.cwd).await;
    c.send(ClientMsg::Prompt { session_id: sid.clone(), text: "first".into(), images: vec![] }).await;
    c.until(|m| matches!(m, ServerMsg::PartUpsert { part: Part::Tool(t), .. } if t.state == ToolState::Running)).await;
    // Busy: this one is queued and delivered at the next step boundary.
    c.send(ClientMsg::Prompt { session_id: sid.clone(), text: "second prompt".into(), images: vec![] }).await;
    c.until(|m| matches!(m, ServerMsg::Notice { text, .. } if text.starts_with("Queued"))).await;
    c.until_idle_with(&sid, "continued").await;
    let core = env.server.hub.session(&sid).unwrap();
    let msgs = core.st.lock().messages.clone();
    // History is kept (old messages visible), context starts at the compaction message.
    assert_eq!(msgs[0].raw_text(), "first");
    let ci = msgs.iter().position(|m| m.origin == MessageOrigin::Compaction).expect("compaction msg");
    assert!(msgs[ci].raw_text().contains("We did X."));
    assert!(msgs[..ci].iter().any(|m| m.raw_text() == "second prompt"));
    let reqs: Vec<Value> = env.requests.lock().iter().filter(|r| r.get("tools").is_some()).cloned().collect();
    let last = reqs.last().unwrap();
    // system + summary only.
    assert_eq!(last["messages"].as_array().unwrap().len(), 2, "{}", last["messages"]);
    env.server.shutdown();
}
