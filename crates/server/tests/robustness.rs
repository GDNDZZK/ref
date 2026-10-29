//! Protocol robustness: malformed input, ordering, disconnects, multiple frontends, large
//! outputs, agent-level permission/delay precedence.
mod common;

use common::*;
use futures::{SinkExt, StreamExt};
use hn_proto::*;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message as WsMsg;

/// Raw socket helpers for malformed / burst traffic.
async fn raw_connect(url: &str) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    for _ in 0..100 {
        if let Ok((ws, _)) = tokio_tungstenite::connect_async(url).await {
            return ws;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("connect {url}");
}

async fn next_response<S>(ws: &mut S, id: u64) -> Value
where
    S: futures::Stream<Item = Result<WsMsg, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        let m = tokio::time::timeout(Duration::from_secs(10), ws.next()).await.expect("timeout").expect("eof").unwrap();
        if let WsMsg::Text(t) = m {
            let v: Value = serde_json::from_str(t.as_str()).unwrap();
            if v["type"] == "response" && v["id"] == id {
                return v;
            }
        }
    }
}

#[tokio::test]
async fn malformed_messages_get_errors_and_connection_survives() {
    let mock = Mock::start(Arc::new(|_r: &Req| text("x"))).await;
    let env = Env::new(mock.port, json!({}));
    let srv = env.spawn(&[]);
    let mut ws = raw_connect(&srv.url()).await;
    let cases = [
        ("not json at all", 0u64),
        (r#"{"id": 7, "type": "no_such_message"}"#, 7),
        (r#"{"id": 8, "type": "prompt"}"#, 8),
        (r#"{"id": 9, "type": "prompt", "session": 5, "text": []}"#, 9),
        (r#"{"id": 10, "type": "attach", "session": "../../etc", "foreground": true}"#, 10),
        (r#"{"id": 11, "type": "abort", "session": "ses_nope"}"#, 11),
        (r#"{"id": 12, "type": "permission", "session": "ses_nope", "call_id": "x", "decision": "once"}"#, 12),
        (r#"{"id": 13, "type": "hello", "client": "t", "cwd": "/", "protocol": 999}"#, 13),
        (r#"{"id": 14, "type": "create_session", "cwd": "relative/dir"}"#, 14),
        (r#"{"id": 15, "type": "undo", "session": ""}"#, 15),
    ];
    for (raw, id) in cases {
        ws.send(WsMsg::text(raw)).await.unwrap();
        let r = next_response(&mut ws, id).await;
        assert!(r["error"].as_str().is_some_and(|e| !e.is_empty()), "{raw} -> {r}");
    }
    ws.send(WsMsg::binary(br#"{"id": 20, "type": "ping"}"#.to_vec())).await.unwrap();
    assert_eq!(next_response(&mut ws, 20).await["ok"], "pong");
    ws.send(WsMsg::text(r#"{"id": 21, "type": "ping"}"#)).await.unwrap();
    assert_eq!(next_response(&mut ws, 21).await["ok"], "pong");
}

#[tokio::test]
async fn burst_requests_are_processed_in_order() {
    let mock = Mock::start(Arc::new(|_r: &Req| text("ok"))).await;
    let env = Env::new(mock.port, json!({}));
    let srv = env.spawn(&[]);
    let mut c = Client::connect(&srv.url(), &env.work()).await;
    let info = c.create(&env.work(), Some("order")).await;
    c.req(ClientMsg::Abort { session: info.id.clone() }).await.unwrap(); // pause: prompts only queue
    let mut ws = raw_connect(&srv.url()).await;
    ws.send(WsMsg::text(json!({"id": 1, "type": "hello", "client": "t", "cwd": env.work(), "protocol": PROTOCOL_VERSION}).to_string()))
        .await
        .unwrap();
    next_response(&mut ws, 1).await;
    // Many renames + prompts without waiting: the final state must reflect the last one.
    let mut frames = Vec::new();
    for i in 0..40u64 {
        frames.push(json!({"id": 100 + i, "type": "rename", "session": info.id, "name": format!("name-{i}")}));
    }
    for i in 0..20u64 {
        frames.push(json!({"id": 200 + i, "type": "prompt", "session": info.id, "text": format!("P{i}")}));
    }
    for f in &frames {
        ws.feed(WsMsg::text(f.to_string())).await.unwrap();
    }
    ws.flush().await.unwrap();
    next_response(&mut ws, 219).await;
    c.wait_idle(&info.id, T).await;
    let (i, msgs) = c.snapshot(&info.id).await;
    assert_eq!(i.name, "name-39");
    let prompts: Vec<String> = msgs.iter().filter(|m| m.role == Role::User).map(|m| m.raw_text()).collect();
    let expected: Vec<String> = (0..20).map(|i| format!("P{i}")).collect();
    assert_eq!(prompts, expected);
}

#[tokio::test]
async fn huge_tool_output_is_truncated_and_not_flooding() {
    let script: Script = Arc::new(|r: &Req| match r.n {
        0 => tool("bash", json!({"purpose": "spam", "command": "head -c 10000000 /dev/zero | tr '\\0' 'x' | fold -w 100"})),
        _ => text("done"),
    });
    let mock = Mock::start(script).await;
    let env = Env::new(mock.port, json!({}));
    let srv = env.spawn(&[]);
    let mut c = Client::connect(&srv.url(), &env.work()).await;
    let info = c.create(&env.work(), Some("big")).await;
    c.prompt(&info.id, "go").await;
    c.wait_idle(&info.id, Duration::from_secs(60)).await;
    let (_, msgs) = c.snapshot(&info.id).await;
    let t = &tool_parts(&msgs)[0];
    assert_eq!(t.state, ToolState::Done);
    assert!(t.output.starts_with("[output truncated: showing last ") && t.output.contains(" lines of 100000; full output: "), "{}", &t.output[..200]);
    let log = t.output.split("full output: ").nth(1).unwrap().split(']').next().unwrap().to_string();
    assert!(log.contains(&info.id), "spill log is per session: {log}");
    let _ = std::fs::remove_file(&log);
    assert!(t.output.len() < 60 * 1024, "{}", t.output.len());
    // Bounded event traffic: streamed deltas stop at the cap.
    let (mut deltas, mut bytes, mut parts) = (0usize, 0usize, 0usize);
    for m in &c.seen {
        match m {
            ServerMsg::Event { event: Event::Delta { field: DeltaField::ToolOutput, text, .. }, .. } => {
                deltas += 1;
                bytes += text.len();
            }
            ServerMsg::Event { event: Event::Part { part: Part::Tool(_), .. }, .. } => parts += 1,
            _ => {}
        }
    }
    assert!(bytes <= 256 * 1024, "streamed {bytes} bytes in {deltas} deltas");
    assert!(parts < 100, "{parts} tool part events");
    // The model saw the truncated text only.
    let reqs = mock.requests();
    assert!(reqs[1].to_string().len() < 200 * 1024);
}

#[tokio::test]
async fn two_frontends_share_events_and_either_can_abort() {
    let script: Script = Arc::new(|r: &Req| match r.n {
        0 | 1 => tool("bash", json!({"purpose": "wait", "command": "sleep 20"})),
        _ => text("never"),
    });
    let mock = Mock::start(script).await;
    let env = Env::new(mock.port, json!({}));
    let srv = env.spawn(&[]);
    let mut a = Client::connect(&srv.url(), &env.work()).await;
    let mut b = Client::connect(&srv.url(), &env.work()).await;
    let info = a.create(&env.work(), Some("shared")).await;
    b.req(ClientMsg::Attach { session: info.id.clone(), foreground: true }).await.unwrap();
    let running = |m: &ServerMsg| matches!(m, ServerMsg::Event { event: Event::Part { part: Part::Tool(tp), .. }, .. } if tp.state == ToolState::Running);
    for (round, aborter) in [(0, 1), (1, 0)] {
        a.prompt(&info.id, &format!("go {round}")).await;
        a.wait_for("a running", T, running).await;
        b.wait_for("b running", T, running).await;
        let (x, y) = if aborter == 0 { (&mut a, &mut b) } else { (&mut b, &mut a) };
        assert_eq!(x.req(ClientMsg::Abort { session: info.id.clone() }).await.unwrap()["aborted"], true);
        x.wait_idle(&info.id, T).await;
        y.wait_idle(&info.id, T).await;
    }
    let (i, msgs) = a.snapshot(&info.id).await;
    assert_eq!(i.attached, 2);
    let tools = tool_parts(&msgs);
    assert_eq!(tools.len(), 2);
    assert!(tools.iter().all(|t| t.state == ToolState::Cancelled), "{tools:#?}");
    // Watch-only attachment does not make the session foreground; closing b drops the count.
    b.close();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut w = Client::connect(&srv.url(), &env.work()).await;
    let (wi, _) = w.snapshot(&info.id).await;
    assert_eq!(wi.attached, 1, "only a is foreground now");
}

#[tokio::test]
async fn client_disconnect_mid_turn_keeps_session_running() {
    let script: Script = Arc::new(|r: &Req| match r.n {
        0 => tool("bash", json!({"purpose": "wait", "command": "sleep 1; echo finished-bg"})),
        _ => text("all done"),
    });
    let mock = Mock::start(script).await;
    let env = Env::new(mock.port, json!({}));
    let srv = env.spawn(&["--idle-timeout", "-1"]);
    let mut a = Client::connect(&srv.url(), &env.work()).await;
    let info = a.create(&env.work(), Some("dc")).await;
    a.prompt(&info.id, "go").await;
    a.wait_for("running", T, |m| matches!(m, ServerMsg::Event { event: Event::Part { part: Part::Tool(tp), .. }, .. } if tp.state == ToolState::Running)).await;
    a.close();
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let mut b = Client::connect(&srv.url(), &env.work()).await;
    let (i, msgs) = b.snapshot(&info.id).await;
    assert_eq!(i.status, SessionStatus::Idle);
    assert_eq!(i.attached, 0, "background after the frontend left");
    assert_eq!(last_assistant_text(&msgs), "all done");
    assert!(tool_parts(&msgs)[0].output.contains("finished-bg"));
}

#[tokio::test]
async fn agent_permission_and_delay_override_global() {
    let script: Script = Arc::new(|r: &Req| {
        if r.last_role() == "user" {
            tool("bash", json!({"purpose": "p", "command": "echo ran"}))
        } else {
            text("ok")
        }
    });
    let mock = Mock::start(script).await;
    let env = Env::new(
        mock.port,
        json!({
            "permission": {"bash": "deny"},
            "delay": {"bash": 30},
            "agent": {
                "trusted": {"description": "t", "permission": {"bash": "allow"}, "delay": {"bash": 0}},
                "careful": {"description": "c", "permission": {"bash": "ask"}, "delay": {"*": 0.4}},
            }
        }),
    );
    let srv = env.spawn(&[]);
    let mut c = Client::connect(&srv.url(), &env.work()).await;
    // Global: denied (no delay because denial happens first).
    let g = c.create(&env.work(), Some("global")).await;
    c.prompt(&g.id, "go").await;
    c.wait_idle(&g.id, T).await;
    assert_eq!(tool_parts(&c.snapshot(&g.id).await.1)[0].state, ToolState::Denied);
    // Agent override: allowed, no delay.
    let v = c.req(ClientMsg::CreateSession { cwd: env.work(), agent: Some("trusted".into()), model: None, name: None }).await.unwrap();
    let t: SessionInfo = serde_json::from_value(v).unwrap();
    let start = std::time::Instant::now();
    c.prompt(&t.id, "go").await;
    c.wait_idle(&t.id, T).await;
    let tp = &tool_parts(&c.snapshot(&t.id).await.1)[0];
    assert_eq!(tp.state, ToolState::Done, "{tp:#?}");
    assert!(start.elapsed() < Duration::from_secs(10));
    // Agent ask + agent delay (overrides global 30s): the permission part carries the input.
    let v = c.req(ClientMsg::CreateSession { cwd: env.work(), agent: Some("careful".into()), model: None, name: None }).await.unwrap();
    let k: SessionInfo = serde_json::from_value(v).unwrap();
    c.prompt(&k.id, "go").await;
    let m = c
        .wait_for("ask", T, |m| matches!(m, ServerMsg::Event { session, event: Event::Part { part: Part::Tool(tp), .. } } if session == &k.id && tp.state == ToolState::Permission))
        .await;
    let ServerMsg::Event { event: Event::Part { part: Part::Tool(tp), .. }, .. } = m else { unreachable!() };
    assert_eq!(tp.input["command"], "echo ran");
    assert_eq!(tp.purpose, "p");
    c.req(ClientMsg::Permission { session: k.id.clone(), call_id: tp.call_id.clone(), decision: PermissionDecision::Once }).await.unwrap();
    let m = c
        .wait_for("delayed", T, |m| matches!(m, ServerMsg::Event { session, event: Event::Part { part: Part::Tool(tp), .. } } if session == &k.id && tp.state == ToolState::Delayed))
        .await;
    let ServerMsg::Event { event: Event::Part { part: Part::Tool(tp), .. }, .. } = m else { unreachable!() };
    let wait = tp.run_at.unwrap() - now_ms();
    assert!((0..=600).contains(&wait), "agent delay 0.4s, got {wait}ms");
    c.wait_idle(&k.id, T).await;
    assert_eq!(tool_parts(&c.snapshot(&k.id).await.1)[0].state, ToolState::Done);
}

#[tokio::test]
async fn model_follows_agent_default_and_set_model_is_not_remembered() {
    let mock = Mock::start(Arc::new(|_r: &Req| text("ok"))).await;
    let env = Env::new(
        mock.port,
        json!({
            "provider": {"mock": {
                "npm": "@ai-sdk/openai-compatible",
                "options": {"baseURL": format!("http://127.0.0.1:{}/v1", mock.port), "apiKey": "k"},
                "models": {"m": {}, "m2": {}, "m3": {}}
            }},
            "agent": {"other": {"description": "o", "model": "mock/m3"}}
        }),
    );
    let srv = env.spawn(&[]);
    let mut c = Client::connect(&srv.url(), &env.work()).await;
    let s1 = c.create(&env.work(), None).await;
    assert_eq!(s1.model.key(), "mock/m");
    let r = c.req(ClientMsg::SetModel { session: s1.id.clone(), model: ModelRef::parse("mock/m2").unwrap() }).await.unwrap();
    assert_eq!(r["model"]["model"], "m2");
    c.prompt(&s1.id, "hi").await;
    c.wait_idle(&s1.id, T).await;
    assert_eq!(mock.requests().last().unwrap()["model"], "m2");
    let s2 = c.create(&env.work(), None).await;
    assert_eq!(s2.model.key(), "mock/m", "a new session uses the default again");
    let r = c.req(ClientMsg::SetAgent { session: s2.id.clone(), agent: "other".into() }).await.unwrap();
    assert_eq!(r["model"]["model"], "m3", "switching agent switches to its default model");
    let v = c.req(ClientMsg::CreateSession { cwd: env.work(), agent: Some("other".into()), model: None, name: None }).await.unwrap();
    assert_eq!(v["model"]["model"], "m3");
}

/// `[path#TAG]` header from the latest tool output in a request (chat format).
fn last_tag(r: &Req, path: &str) -> Option<String> {
    let re = regex::Regex::new(&format!(r"\[{}#([0-9A-Fa-f]{{4}})\]", regex::escape(path))).unwrap();
    r.messages().iter().rev().filter(|m| m["role"] == "tool").find_map(|m| re.captures(&content_text(m)).map(|c| c[1].to_string()))
}

#[tokio::test]
async fn concurrent_sessions_edit_same_crlf_file_and_binary_read() {
    // Each session: read shared.txt, then replace "its" line, then read the binary, then stop.
    let script: Script = Arc::new(|r: &Req| {
        let first = r.messages().iter().find(|m| m["role"] == "user").map(content_text).unwrap_or_default();
        let (line, word) = if first.contains("alpha") { (1, "ALPHA") } else { (3, "GAMMA") };
        let tools = r.messages().iter().filter(|m| m["role"] == "tool").count();
        match tools {
            0 => tool("read", json!({"purpose": "look", "path": "shared.txt"})),
            1 | 2 => {
                // Retry with the fresh tag if the other session changed the file meanwhile.
                let tag = last_tag(r, "shared.txt").unwrap_or_else(|| "0000".into());
                if tools == 2 && !r.last_text().contains("rejected") && !r.last_text().contains("changed") {
                    return tool("read", json!({"purpose": "bin", "path": "blob.bin"}));
                }
                tool("edit", json!({"purpose": "edit", "input": format!("[shared.txt#{tag}]\nPUT {line}.={line}:\n+{word}\n")}))
            }
            3 => tool("read", json!({"purpose": "bin", "path": "blob.bin"})),
            _ => text("done"),
        }
    });
    let mock = Mock::start(script).await;
    let env = Env::new(mock.port, json!({}));
    let work = env.work();
    std::fs::write(format!("{work}/shared.txt"), "alpha\r\nbeta\r\ngamma\r\n").unwrap();
    std::fs::write(format!("{work}/blob.bin"), [0u8, 159, 146, 150, 0, 1, 2, 255]).unwrap();
    let srv = env.spawn(&[]);
    let mut c = Client::connect(&srv.url(), &work).await;
    let a = c.create(&work, Some("ea")).await;
    let b = c.create(&work, Some("eb")).await;
    c.prompt(&a.id, "edit alpha").await;
    c.prompt(&b.id, "edit gamma").await;
    for id in [&a.id, &b.id] {
        for _ in 0..200 {
            let (i, m) = c.snapshot(id).await;
            if i.status == SessionStatus::Idle && last_assistant_text(&m) == "done" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    let content = std::fs::read_to_string(format!("{work}/shared.txt")).unwrap();
    assert_eq!(content, "ALPHA\r\nbeta\r\nGAMMA\r\n", "both edits applied, CRLF kept");
    for id in [&a.id, &b.id] {
        let (_, msgs) = c.snapshot(id).await;
        let tools = tool_parts(&msgs);
        let bin = tools.iter().find(|t| t.input["path"] == "blob.bin").expect("binary read");
        assert!(bin.output.contains("Cannot read binary file"), "{}", bin.output);
        assert!(tools.iter().any(|t| t.name == "edit" && t.state == ToolState::Done), "{tools:#?}");
    }
}

#[tokio::test]
async fn unicode_names_paging_and_manage_sessions_gating() {
    let script: Script = Arc::new(|r: &Req| {
        let last = r.last_user();
        if r.last_role() == "user" && last.contains("ping 中文") {
            return tool("send_message", json!({"purpose": "msg", "to": "  测试会话 🚀 ", "message": "你好"}));
        }
        if r.last_role() == "user" && last.contains("[message from session") {
            return text("收到");
        }
        if r.last_role() == "user" && last.contains("read page") {
            let page: u64 = last.rsplit(' ').next().unwrap().parse().unwrap();
            return tool("session_messages", json!({"purpose": "read", "session": "测试会话 🚀", "page": page, "page_size": 1}));
        }
        text("ok")
    });
    let mock = Mock::start(script).await;
    let env = Env::new(mock.port, json!({"agent": {"worker": {"description": "w", "spawnable": true}, "lead": {"description": "l", "manage_sessions": true}}}));
    let srv = env.spawn(&[]);
    let mut c = Client::connect(&srv.url(), &env.work()).await;
    let target = c.create(&env.work(), Some("测试会话 🚀")).await;
    let v = c.req(ClientMsg::CreateSession { cwd: env.work(), agent: Some("worker".into()), model: None, name: Some("w".into()) }).await.unwrap();
    let w: SessionInfo = serde_json::from_value(v).unwrap();
    c.prompt(&w.id, "ping 中文").await;
    c.wait_idle(&w.id, T).await;
    let out = &tool_parts(&c.snapshot(&w.id).await.1)[0].output;
    assert!(out.starts_with("Delivered to \"测试会话 🚀\""), "{out}");
    for _ in 0..100 {
        let (i, m) = c.snapshot(&target.id).await;
        if i.status == SessionStatus::Idle && last_assistant_text(&m) == "收到" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // Paging: page 1 = most recent message, page 2 the one before.
    for (page, expect) in [(1, "收到"), (2, "你好")] {
        c.prompt(&w.id, &format!("read page {page}")).await;
        c.wait_idle(&w.id, T).await;
        let tools = tool_parts(&c.snapshot(&w.id).await.1);
        let out = &tools.last().unwrap().output;
        assert!(out.contains(&format!("(page {page}/2)")) && out.contains(expect), "{out}");
    }
    // worker (manage_sessions unset) has no create/close tools; lead has them and sees spawnable agents.
    let reqs = mock.requests();
    let names = |b: &Value| -> Vec<String> { b["tools"].as_array().unwrap().iter().map(|t| t["function"]["name"].as_str().unwrap().to_string()).collect() };
    let wreq = reqs.iter().find(|b| b.to_string().contains("ping 中文")).unwrap();
    assert!(!names(wreq).contains(&"create_session".to_string()));
    assert!(!names(wreq).contains(&"close_session".to_string()));
    assert!(!wreq.to_string().contains("Agents you can start"));
    let v = c.req(ClientMsg::CreateSession { cwd: env.work(), agent: Some("lead".into()), model: None, name: None }).await.unwrap();
    let l: SessionInfo = serde_json::from_value(v).unwrap();
    c.prompt(&l.id, "hello").await;
    c.wait_idle(&l.id, T).await;
    let lreq = mock.requests().into_iter().last().unwrap();
    assert!(names(&lreq).contains(&"create_session".to_string()) && names(&lreq).contains(&"close_session".to_string()));
    let sys = Req { body: lreq.clone(), n: 0 }.system();
    assert!(sys.contains("Agents you can start with create_session:\n") && sys.contains("\n- worker: w"), "{sys}");
    assert!(!sys.contains("- lead"), "lead is not spawnable: {sys}");
}

#[tokio::test]
async fn retries_do_not_give_up_and_backoff_is_capped() {
    // 12 consecutive retryable failures (mixed kinds), then success. The cap (max_delay=1s
    // in the test config) bounds each wait; nothing ends the turn early.
    let script: Script = Arc::new(|r: &Req| match r.n {
        n if n < 12 => match n % 3 {
            0 => Reply::Status(529, "Overloaded".into()),
            1 => Reply::Status(429, "rate limited".into()),
            _ => Reply::Status(503, "upstream connect error".into()),
        },
        _ => text("finally"),
    });
    let mock = Mock::start(script).await;
    let env = Env::new(mock.port, json!({"provider": {
        "mock": {"npm": "@ai-sdk/openai-compatible",
            "options": {"baseURL": format!("http://127.0.0.1:{}/v1", mock.port), "apiKey": "k"},
            "models": {"m": {}}},
        "mocka": anthropic_provider(mock.port)}, "model": "mocka/claude-sonnet-5-5", "retry": {"initial_delay": 0.05, "max_delay": 1}}));
    let srv = env.spawn(&[]);
    let mut c = Client::connect(&srv.url(), &env.work()).await;
    let info = c.create(&env.work(), Some("retry-forever")).await;
    let start = std::time::Instant::now();
    c.prompt(&info.id, "go").await;
    c.wait_idle(&info.id, Duration::from_secs(60)).await;
    let (_, msgs) = c.snapshot(&info.id).await;
    assert_eq!(last_assistant_text(&msgs), "finally");
    assert_eq!(msgs.len(), 2, "failed attempts leave no messages");
    let attempts: Vec<u32> = c
        .seen
        .iter()
        .filter_map(|m| match m {
            ServerMsg::Event { event: Event::Info { info }, .. } => match &info.status {
                SessionStatus::Retry { attempt, .. } => Some(*attempt),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(attempts.iter().max(), Some(&12), "{attempts:?}");
    assert!(start.elapsed() < Duration::from_secs(25), "backoff not capped: {:?}", start.elapsed());
}
