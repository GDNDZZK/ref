//! Session lifecycle: foreground/background accounting, session list pushes, close/delete,
//! history, restart + repair of interrupted turns, model/agent defaults, titles, idle
//! timeout, `--status`, and robustness against bad clients.

mod common;

use std::time::Duration;

use common::*;
use futures::{SinkExt, StreamExt};
use ref_proto::{ClientMsg, Message, MessageOrigin, Part, Role, ServerMsg, SessionStatus, ToolState};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message as WsMsg;

fn list_entry(m: &ServerMsg, sid: &str) -> Option<ref_proto::SessionInfo> {
    match m {
        ServerMsg::SessionList { sessions, .. } => sessions.iter().find(|s| s.id == sid).cloned(),
        _ => None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn foreground_background_and_session_list() {
    let env = setup(cmd_script(), json!({})).await;
    let mut a = Client::connect(&env).await;
    a.send(ClientMsg::ListSessions { req: 3 }).await;
    a.until(|m| matches!(m, ServerMsg::SessionList { req: 3, .. })).await;
    let sid = a.create(&env.cwd).await;
    let s = sid.clone();
    a.until(move |m| list_entry(m, &s).is_some_and(|e| e.attached == 1 && e.open)).await;
    // A second attached client → 2; a watcher does not count.
    let mut b = Client::connect(&env).await;
    let (info, _, _) = b.snapshot(&sid).await;
    assert_eq!(info.attached, 2);
    let mut w = Client::connect(&env).await;
    w.send(ClientMsg::Watch { session_id: sid.clone() }).await;
    w.until(|m| matches!(m, ServerMsg::SessionSnapshot { .. })).await;
    assert_eq!(env.core(&sid).info().attached, 2);
    // Disconnect detaches.
    drop(b);
    let s = sid.clone();
    a.until(move |m| matches!(m, ServerMsg::SessionUpdated { info } if info.id == s && info.attached == 1)).await;
    // Status changes are pushed in the list too.
    a.prompt(&sid, &call(json!([["bash", {"command": "sleep 0.4"}]]))).await;
    let s = sid.clone();
    a.until(move |m| list_entry(m, &s).is_some_and(|e| e.status == SessionStatus::Busy)).await;
    let s = sid.clone();
    a.until(move |m| list_entry(m, &s).is_some_and(|e| e.status == SessionStatus::Idle && e.message_count == 3)).await;
    // The watcher saw the events.
    w.until(|m| matches!(m, ServerMsg::MessageUpsert { message, .. } if message.raw_text() == "done")).await;
    // Explicit detach → background (0); still open.
    a.send(ClientMsg::Detach { session_id: sid.clone() }).await;
    let s = sid.clone();
    a.until(move |m| list_entry(m, &s).is_some_and(|e| e.attached == 0 && e.open)).await;
    // Close from a client: SessionClosed to everyone, listed as history.
    a.send(ClientMsg::Close { session_id: sid.clone() }).await;
    let s = sid.clone();
    w.until(move |m| matches!(m, ServerMsg::SessionClosed { session_id } if *session_id == s)).await;
    let s = sid.clone();
    let e = a.until(move |m| list_entry(m, &s).is_some_and(|e| !e.open)).await;
    let e = list_entry(&e, &sid).unwrap();
    assert_eq!(e.attached, 0);
    assert_eq!(e.status, SessionStatus::Idle);
    assert_eq!(e.message_count, 3);
    // Attach reopens with messages.
    let (info, msgs, _) = a.snapshot(&sid).await;
    assert!(info.open);
    assert_eq!(info.attached, 1);
    assert_eq!(msgs.len(), 3);
    // Delete (while open): gone from list and disk; attaching fails afterwards.
    a.send(ClientMsg::Delete { session_id: sid.clone() }).await;
    let s = sid.clone();
    a.until(move |m| matches!(m, ServerMsg::SessionList { sessions, .. } if !sessions.iter().any(|x| x.id == s))).await;
    assert!(!env.data_dir.join(format!("sessions/{sid}.jsonl")).exists());
    assert!(!env.data_dir.join(format!("sessions/{sid}.meta.json")).exists());
    a.send(ClientMsg::Attach { req: 77, session_id: sid.clone() }).await;
    a.until(|m| matches!(m, ServerMsg::Error { req: 77, .. })).await;
    a.send(ClientMsg::Delete { session_id: sid.clone() }).await;
    a.until(|m| matches!(m, ServerMsg::Error { text, .. } if text.contains("Unknown session"))).await;
    env.server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn restart_keeps_sessions_and_repairs_interrupted_turns() {
    let mut env = setup(cmd_script(), json!({})).await;
    let mut c = Client::connect(&env).await;
    let sid = c.create_with(&env.cwd, None, None, Some("持久")).await.id;
    c.turn(&sid, "before restart").await;
    drop(c);
    env.restart().await;
    // Simulate a crash mid-turn: append an unfinished assistant message with a running tool
    // and an empty assistant placeholder.
    let mut m = Message::new(Role::Assistant, MessageOrigin::User);
    m.parts.push(Part::Tool(ref_proto::ToolPart {
        id: "prt_x".into(),
        call_id: "call_x".into(),
        name: "bash".into(),
        purpose: "p".into(),
        input: json!({"purpose": "p", "command": "sleep 100"}),
        input_raw: String::new(),
        state: ToolState::Running,
        output: String::new(),
        images: vec![],
        time: 0,
        time_start: Some(0),
        time_end: None,
        run_at: None,
        meta: None,
    }));
    let empty = Message::new(Role::Assistant, MessageOrigin::User);
    let log = env.data_dir.join(format!("sessions/{sid}.jsonl"));
    let mut text = std::fs::read_to_string(&log).unwrap();
    text.push_str(&serde_json::to_string(&m).unwrap());
    text.push('\n');
    text.push_str(&serde_json::to_string(&empty).unwrap());
    text.push_str("\n{corrupt line\n");
    std::fs::write(&log, text).unwrap();

    let mut c = Client::connect(&env).await;
    c.send(ClientMsg::ListSessions { req: 1 }).await;
    let ServerMsg::SessionList { sessions, .. } = c.until(|m| matches!(m, ServerMsg::SessionList { req: 1, .. })).await else {
        unreachable!()
    };
    let e = sessions.iter().find(|s| s.id == sid).expect("history entry after restart");
    assert!(!e.open);
    assert_eq!(e.name, "持久");
    let (info, msgs, _) = c.snapshot(&sid).await;
    assert!(info.open);
    assert_eq!(info.status, SessionStatus::Idle);
    assert_eq!(msgs.len(), 3, "{msgs:?}");
    let ref_proto::Part::Tool(t) = &msgs[2].parts[0] else { panic!() };
    assert_eq!(t.state, ToolState::Cancelled);
    assert_eq!(msgs[2].finish.as_deref(), Some("aborted"));
    // Continuing works and the model gets a result for the interrupted call.
    c.turn(&sid, "after restart").await;
    let last = env.llm_requests().pop().unwrap();
    assert!(tool_result_content(&last).iter().any(|r| r.contains("Interrupted")), "{}", last["messages"]);
    assert_eq!(env.messages(&sid).last().unwrap().raw_text(), "echo: after restart");
    env.server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn default_model_follows_agent() {
    let env = setup(cmd_script(), json!({"agent": {"alt": {"model": "mock/small", "description": "alt"}}})).await;
    let mut c = Client::connect(&env).await;
    let a = c.create(&env.cwd).await;
    assert_eq!(env.core(&a).info().model, "mock/m");
    c.send(ClientMsg::SetModel { session_id: a.clone(), model: "mock/small".into(), variant: None }).await;
    let s = a.clone();
    c.until(move |m| matches!(m, ServerMsg::SessionUpdated { info } if info.id == s && info.model == "mock/small")).await;
    // Not remembered for new sessions.
    let b = c.create_with(&env.cwd, None, None, None).await;
    assert_eq!(b.model, "mock/m");
    // New session with an agent uses the agent's model; SetAgent resets the model.
    let d = c.create_with(&env.cwd, Some("alt"), None, None).await;
    assert_eq!(d.model, "mock/small");
    assert_eq!(d.context.max, 50_000);
    c.send(ClientMsg::SetAgent { session_id: a.clone(), agent: "alt".into() }).await;
    let s = a.clone();
    c.until(move |m| matches!(m, ServerMsg::SessionUpdated { info } if info.id == s && info.agent == "alt" && info.model == "mock/small")).await;
    c.send(ClientMsg::SetModel { session_id: a.clone(), model: "mock/m".into(), variant: None }).await;
    let s = a.clone();
    c.until(move |m| matches!(m, ServerMsg::SessionUpdated { info } if info.id == s && info.model == "mock/m")).await;
    c.send(ClientMsg::SetAgent { session_id: a.clone(), agent: "alt".into() }).await;
    let s = a.clone();
    c.until(move |m| matches!(m, ServerMsg::SessionUpdated { info } if info.id == s && info.model == "mock/small")).await;
    c.send(ClientMsg::SetAgent { session_id: a.clone(), agent: "build".into() }).await;
    let s = a.clone();
    c.until(move |m| matches!(m, ServerMsg::SessionUpdated { info } if info.id == s && info.agent == "build" && info.model == "mock/m" && info.context.max == 100_000)).await;
    // Errors.
    c.send(ClientMsg::SetAgent { session_id: a.clone(), agent: "nope".into() }).await;
    c.until(|m| matches!(m, ServerMsg::Error { text, .. } if text.contains("Unknown agent"))).await;
    c.send(ClientMsg::SetModel { session_id: a.clone(), model: "mock/none".into(), variant: None }).await;
    c.until(|m| matches!(m, ServerMsg::Error { .. })).await;
    // The request uses the session's model.
    c.turn(&d.id, "hi").await;
    assert_eq!(env.llm_requests().pop().unwrap()["model"], "small");
    env.server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn title_generation() {
    let mut o = Opts::new(cmd_script());
    o.title = std::sync::Arc::new(|body| {
        let (_, content) = last_content(body);
        if content.contains("失败") {
            Reply::Status(500, "title backend down".into())
        } else if content.contains("修复") {
            Reply::Text("<think>user wants a fix</think>\n\"修复登录问题。\"".into())
        } else {
            Reply::Text("Some Title".into())
        }
    });
    let env = setup_opts(o).await;
    let mut c = Client::connect(&env).await;
    let a = c.create(&env.cwd).await;
    let auto = env.core(&a).name();
    c.turn(&a, "请修复登录页面的问题").await;
    wait_until(Duration::from_secs(5), || env.core(&a).name() == "修复登录问题").await;
    let t = env.title_requests();
    assert_eq!(t.len(), 1);
    assert_eq!(t[0]["model"], "m", "session model used without small_model");
    let req = serde_json::to_string(&t[0]).unwrap();
    assert!(req.contains("Generate a short title (max 6 words) for this conversation in the same language as the user's message. Reply with the title only."));
    assert!(req.contains("请修复登录页面的问题"));
    assert_ne!(auto, "修复登录问题");
    // Only the first prompt.
    c.turn(&a, "another prompt").await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(env.title_requests().len(), 1);
    // Failure is silent: name unchanged, no error/notice.
    let b = c.create(&env.cwd).await;
    let name_b = env.core(&b).name();
    c.turn(&b, "这次会失败").await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(env.core(&b).name(), name_b);
    assert!(!c.log.iter().any(|m| matches!(m, ServerMsg::Error { .. })));
    assert!(!c.log.iter().any(|m| matches!(m, ServerMsg::Notice { text, .. } if text.contains("title"))));
    // Explicitly named sessions keep their name.
    let n = c.create_with(&env.cwd, None, None, Some("mine")).await.id;
    c.turn(&n, "hello").await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(env.core(&n).name(), "mine");
    assert_eq!(env.title_requests().len(), 2);
    env.server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bad_client_messages_never_break_the_connection() {
    let env = setup(cmd_script(), json!({})).await;
    let mut c = Client::connect(&env).await;
    for bad in ["{not json", "{}", "{\"type\":\"nope\"}", "[1,2,3]", "{\"type\":\"prompt\"}", "null", ""] {
        c.send_raw(bad).await;
        c.until(|m| matches!(m, ServerMsg::Error { text, .. } if text.starts_with("invalid message"))).await;
    }
    c.ws.send(WsMsg::Binary(b"\xff\xfe".to_vec().into())).await.unwrap();
    c.until(|m| matches!(m, ServerMsg::Error { .. })).await;
    // Unknown / malicious ids.
    c.send(ClientMsg::Attach { req: 4, session_id: "../../etc/passwd".into() }).await;
    c.until(|m| matches!(m, ServerMsg::Error { req: 4, .. })).await;
    c.send(ClientMsg::Prompt { session_id: "ses_missing".into(), text: "x".into(), images: vec![] }).await;
    c.until(|m| matches!(m, ServerMsg::Error { .. })).await;
    for m in [
        ClientMsg::Abort { session_id: "x".into() },
        ClientMsg::Detach { session_id: "x".into() },
        ClientMsg::Unwatch { session_id: "x".into() },
        ClientMsg::Watch { session_id: "x".into() },
        ClientMsg::Compact { session_id: "x".into() },
        ClientMsg::Undo { session_id: "x".into() },
        ClientMsg::Close { session_id: "x".into() },
        ClientMsg::Rename { session_id: "x".into(), name: "y".into() },
        ClientMsg::Command { session_id: "x".into(), name: "y".into(), arguments: String::new() },
        ClientMsg::CreateSession { req: 9, cwd: "relative/path".into(), agent: None, model: None, variant: None, name: None },
        ClientMsg::CreateSession { req: 9, cwd: env.cwd.clone(), agent: Some("ghost".into()), model: None, variant: None, name: None },
    ] {
        c.send(m).await;
    }
    c.send(ClientMsg::Ping { req: 42 }).await;
    c.until(|m| matches!(m, ServerMsg::Pong { req: 42 })).await;
    // A big prompt (> 16 MiB default frame limit) in one frame.
    let sid = c.create(&env.cwd).await;
    let big = format!("BIG{}", "x".repeat(20 * 1024 * 1024));
    c.prompt(&sid, &big).await;
    let n = big.len();
    c.until(move |m| matches!(m, ServerMsg::MessageUpsert { message, .. } if message.raw_text().len() == n)).await;
    // (The mock rejects the 20 MiB body; the session keeps retrying until aborted.)
    c.send(ClientMsg::Abort { session_id: sid.clone() }).await;
    c.wait_status(&sid, |s| *s == SessionStatus::Idle).await;
    assert_eq!(env.messages(&sid)[0].raw_text().len(), big.len());
    let before = env.messages(&sid).len();
    // Empty prompt is ignored.
    c.prompt(&sid, "   ").await;
    c.send(ClientMsg::Ping { req: 43 }).await;
    c.until(|m| matches!(m, ServerMsg::Pong { req: 43 })).await;
    assert_eq!(env.messages(&sid).len(), before);
    // Rename collisions.
    let other = c.create_with(&env.cwd, None, None, Some("taken")).await.id;
    let _ = other;
    c.send(ClientMsg::Rename { session_id: sid.clone(), name: "taken".into() }).await;
    c.until(|m| matches!(m, ServerMsg::Error { text, .. } if text.contains("already named"))).await;
    c.send(ClientMsg::Rename { session_id: sid.clone(), name: " \n ".into() }).await;
    c.until(|m| matches!(m, ServerMsg::Error { text, .. } if text.contains("must not be empty"))).await;
    // A client that never sends Hello is dropped (here: sends garbage first).
    let url = format!("ws://{}/ws?token={}", env.server.addr, env.server.token);
    let (mut raw, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    raw.send(WsMsg::text("{\"type\":\"ping\",\"req\":1}")).await.unwrap();
    let first = raw.next().await.unwrap().unwrap();
    assert!(first.to_text().unwrap().contains("expected hello"));
    env.server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn slow_client_does_not_block_others() {
    let chunk = "y".repeat(256 * 1024);
    let script: Script = std::sync::Arc::new(move |_| Reply::Text(chunk.clone()));
    let env = setup(script, json!({})).await;
    let mut fast = Client::connect(&env).await;
    let sid = fast.create(&env.cwd).await;
    // A client that attaches and then never reads.
    let mut slow = Client::connect(&env).await;
    let _ = slow.snapshot(&sid).await;
    for i in 0..8 {
        fast.turn(&sid, &format!("turn {i}")).await;
    }
    assert_eq!(env.messages(&sid).len(), 16);
    drop(slow);
    env.server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn idle_timeout_zero_exits_after_last_client() {
    let mut o = Opts::new(cmd_script());
    o.idle_timeout = 0;
    let env = setup_opts(o).await;
    // Without any connection it stays up (30s grace).
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(!env.server.hub.shutdown.is_cancelled());
    let a = Client::connect(&env).await;
    let b = Client::connect(&env).await;
    drop(a);
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(!env.server.hub.shutdown.is_cancelled(), "one client still connected");
    drop(b);
    tokio::time::timeout(Duration::from_secs(3), env.server.hub.shutdown.cancelled()).await.expect("did not exit");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn idle_timeout_seconds() {
    let mut o = Opts::new(cmd_script());
    o.idle_timeout = 2;
    let env = setup_opts(o).await;
    let a = Client::connect(&env).await;
    drop(a);
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert!(!env.server.hub.shutdown.is_cancelled());
    // Reconnecting resets the countdown.
    let b = Client::connect(&env).await;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert!(!env.server.hub.shutdown.is_cancelled());
    drop(b);
    tokio::time::timeout(Duration::from_secs(4), env.server.hub.shutdown.cancelled()).await.expect("did not exit");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn status_report_text_and_json() {
    let mut o = Opts::new(cmd_script());
    o.registry = true;
    o.idle_timeout = 0;
    let env = setup_opts(o).await;
    // A stale entry of a dead process is removed.
    let stale = env.data_dir.join("instances/999999999.json");
    let mut entry = refd::registry::RegistryEntry { instance: env.server.hub.instance.clone(), token: "t".into() };
    entry.instance.pid = 999_999_999;
    std::fs::write(&stale, serde_json::to_vec(&entry).unwrap()).unwrap();
    let reg = env.data_dir.join(format!("instances/{}.json", std::process::id()));
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&reg).unwrap().permissions().mode() & 0o777, 0o600);
    }
    let (text, code) = refd::registry::status_report(&env.data_dir, false).await;
    assert_eq!(code, 0);
    assert!(!stale.exists());
    let inst = &env.server.hub.instance;
    for needle in [
        format!("refd pid {}", inst.pid),
        "bind:          127.0.0.1".to_string(),
        format!("port:          {}", inst.port),
        format!("token:         {}", env.server.token),
        "allow-hosts:   (loopback only)".into(),
        "idle-timeout:  0 (exit when the last frontend disconnects)".into(),
        "started:".into(),
        format!("data dir:      {}", env.data_dir.display()),
    ] {
        assert!(text.contains(&needle), "missing {needle:?} in:\n{text}");
    }
    let (json_text, code) = refd::registry::status_report(&env.data_dir, true).await;
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(&json_text).unwrap();
    let e = &v[0];
    assert_eq!(e["pid"], inst.pid);
    assert_eq!(e["port"], inst.port);
    assert_eq!(e["bind"], "127.0.0.1");
    assert_eq!(e["idle_timeout"], 0);
    assert_eq!(e["token"], env.server.token.as_str());
    assert!(e["url"].as_str().unwrap().starts_with("ws://127.0.0.1:"));
    assert!(e["allow_hosts"].as_array().unwrap().is_empty());
    assert!(e["started"].as_i64().unwrap() > 0);
    // After shutdown: no instances, exit code 1, registry file removed.
    env.server.shutdown();
    let Env { server, .. } = env;
    let dd = server.hub.data_dir.clone();
    server.wait().await;
    assert!(!reg.exists());
    let (text, code) = refd::registry::status_report(&dd, false).await;
    assert_eq!(code, 1);
    assert_eq!(text, "No running refd instance.");
}
