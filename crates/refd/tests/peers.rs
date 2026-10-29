//! Inter-session tools (list_sessions, send_message, read_messages, session_context,
//! create_session, close_session), must_report reminders, can_spawn / spawnable, queued
//! prompts and peer messages while busy, abort semantics.

mod common;

use std::time::Duration;

use common::*;
use ref_proto::{ClientMsg, MessageOrigin, Part, ServerMsg, ToolState};
use serde_json::json;

fn agents_cfg() -> serde_json::Value {
    json!({"agent": {
        "worker": {"description": "Does focused work", "spawnable": true, "can_spawn": false},
        "lead": {"description": "Leads", "spawnable": false, "can_spawn": true}
    }})
}

/// Run one tool call in `sid` and return its output.
async fn tool(c: &mut Client, env: &Env, sid: &str, name: &str, args: serde_json::Value) -> (ToolState, String) {
    c.turn(sid, &call(json!([[name, args]]))).await;
    let t = env.tool_parts(sid).pop().expect("tool part");
    assert_eq!(t.name, name);
    (t.state, t.output)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn send_message_resolution() {
    let env = setup(cmd_script(), agents_cfg()).await;
    let mut c = Client::connect(&env).await;
    let a = c.create_with(&env.cwd, None, None, Some("alpha")).await.id;
    let weird = "名字 \"quoted\" 🚀 [x]";
    let b = c.create_with(&env.cwd, None, None, Some(weird)).await;
    assert_eq!(b.name, weird);
    let b1 = c.create_with(&env.cwd, None, None, Some("Beta")).await.id;
    let b2 = c.create_with(&env.cwd, None, None, Some("beta")).await.id;
    // Same name again → unique suffix.
    let dup = c.create_with(&env.cwd, None, None, Some("alpha")).await;
    assert_eq!(dup.name, "alpha (2)");

    let (st, out) = tool(&mut c, &env, &a, "send_message", json!({"to": weird, "message": "hi 你好"})).await;
    assert_eq!(st, ToolState::Done, "{out}");
    assert!(out.starts_with(&format!("Delivered to \"{weird}\"")), "{out}");
    // Target received it as a peer message and answered.
    let core_b = env.core(&b.id);
    wait_until(Duration::from_secs(10), || {
        core_b.st.lock().messages.iter().any(|m| m.raw_text() == "echo: [Message from session \"alpha\" (id: ".to_string() + &a + ", cwd: " + &env.cwd + ")]")
    })
    .await;
    let bm = env.messages(&b.id);
    assert!(matches!(&bm[0].origin, MessageOrigin::Peer { from_session, from_name } if *from_session == a && from_name == "alpha"));
    assert!(bm[0].raw_text().contains("hi 你好"));
    env.wait_done(&b.id).await;

    // By id.
    let (st, out) = tool(&mut c, &env, &a, "send_message", json!({"to": b1, "message": "x"})).await;
    assert_eq!(st, ToolState::Done, "{out}");
    env.wait_done(&b1).await;
    // Exact name wins over case-insensitive matches.
    let (st, out) = tool(&mut c, &env, &a, "send_message", json!({"to": "beta", "message": "x"})).await;
    assert_eq!(st, ToolState::Done, "{out}");
    assert!(out.contains(&b2), "{out}");
    env.wait_done(&b2).await;
    // Ambiguous.
    let (st, out) = tool(&mut c, &env, &a, "send_message", json!({"to": "BETA", "message": "x"})).await;
    assert_eq!(st, ToolState::Error);
    assert!(out.contains("ambiguous") && out.contains(&b1) && out.contains(&b2), "{out}");
    // Self (by name and id).
    let (st, out) = tool(&mut c, &env, &a, "send_message", json!({"to": "alpha", "message": "x"})).await;
    assert_eq!(st, ToolState::Error);
    assert!(out.contains("your own session"), "{out}");
    let (st, _) = tool(&mut c, &env, &a, "send_message", json!({"to": a, "message": "x"})).await;
    assert_eq!(st, ToolState::Error);
    // Unknown / empty.
    let (st, out) = tool(&mut c, &env, &a, "send_message", json!({"to": "nobody", "message": "x"})).await;
    assert_eq!(st, ToolState::Error);
    assert!(out.contains("no open session named \"nobody\""), "{out}");
    let (st, _) = tool(&mut c, &env, &a, "send_message", json!({"to": "alpha (2)", "message": "  "})).await;
    assert_eq!(st, ToolState::Error);
    // Closed target.
    c.send(ClientMsg::Close { session_id: dup.id.clone() }).await;
    let d = dup.id.clone();
    c.until(move |m| matches!(m, ServerMsg::SessionClosed { session_id } if *session_id == d)).await;
    let (st, out) = tool(&mut c, &env, &a, "send_message", json!({"to": dup.id, "message": "x"})).await;
    assert_eq!(st, ToolState::Error);
    assert!(out.contains("is closed"), "{out}");

    // list_sessions shows the others with status / fg-bg, not self.
    let (_, out) = tool(&mut c, &env, &a, "list_sessions", json!({})).await;
    assert!(out.starts_with("3 other open session(s):"), "{out}");
    assert!(out.contains(&format!("\"{weird}\" id={}", b.id)), "{out}");
    assert!(out.contains("foreground"), "{out}");
    assert!(out.ends_with(&format!("You are \"alpha\" (id {a}).")), "{out}");
    assert!(!out.contains(&format!("id={a}")), "{out}");
    env.server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn read_messages_pagination_and_session_context() {
    let env = setup(cmd_script(), json!({})).await;
    let mut c = Client::connect(&env).await;
    let a = c.create_with(&env.cwd, None, None, Some("reader")).await.id;
    let b = c.create_with(&env.cwd, None, None, Some("目标")).await.id;
    for i in 0..5 {
        c.turn(&b, &format!("msg {i}")).await;
    }
    // 10 messages: user/assistant pairs.
    let (_, out) = tool(&mut c, &env, &a, "read_messages", json!({"session": "目标"})).await;
    assert!(out.contains("showing messages 1-10 of 10"), "{out}");
    assert!(!out.contains("Older messages"), "{out}");
    let (_, out) = tool(&mut c, &env, &a, "read_messages", json!({"session": b, "limit": 3})).await;
    assert!(out.contains("showing messages 8-10 of 10"), "{out}");
    assert!(out.contains("Older messages: use offset=3."), "{out}");
    assert!(out.contains("[10] assistant:\necho: msg 4"), "{out}");
    assert!(out.contains("[8] assistant:\necho: msg 3") && !out.contains("[7]"), "{out}");
    let (_, out) = tool(&mut c, &env, &a, "read_messages", json!({"session": b, "limit": "3", "offset": 3})).await;
    assert!(out.contains("showing messages 5-7 of 10"), "{out}");
    assert!(out.contains("[5] user:\nmsg 2"), "{out}");
    let (_, out) = tool(&mut c, &env, &a, "read_messages", json!({"session": b, "offset": 50})).await;
    assert!(out.contains("no messages in that range (10 message(s) in total"), "{out}");
    // Tool calls are shown with purpose.
    c.turn(&b, &call(json!([["context_info", {"purpose": "检查上下文"}]]))).await;
    let (_, out) = tool(&mut c, &env, &a, "read_messages", json!({"session": b, "limit": 3})).await;
    assert!(out.contains("(tool context_info: 检查上下文 [done])"), "{out}");
    // Closed sessions can still be read (from disk).
    c.send(ClientMsg::Close { session_id: b.clone() }).await;
    let b2 = b.clone();
    c.until(move |m| matches!(m, ServerMsg::SessionClosed { session_id } if *session_id == b2)).await;
    let (st, out) = tool(&mut c, &env, &a, "read_messages", json!({"session": b, "limit": 2})).await;
    assert_eq!(st, ToolState::Done, "{out}");
    assert!(out.contains("of 13"), "{out}");
    let (st, out) = tool(&mut c, &env, &a, "session_context", json!({"session": b})).await;
    assert_eq!(st, ToolState::Done, "{out}");
    assert!(out.contains("Context: used") && out.contains("of 100000"), "{out}");
    env.server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn create_and_close_sessions() {
    let env = setup(cmd_script(), agents_cfg()).await;
    std::fs::create_dir_all(format!("{}/sub", env.cwd)).unwrap();
    let mut c = Client::connect(&env).await;
    c.send(ClientMsg::ListSessions { req: 1 }).await;
    let p = c.create_with(&env.cwd, None, None, Some("parent")).await.id;
    let fg = c.create_with(&env.cwd, None, None, Some("front")).await.id;

    // Not spawnable / unknown agents.
    let (st, out) = tool(&mut c, &env, &p, "create_session", json!({"name": "x", "agent": "lead", "prompt": "p"})).await;
    assert_eq!(st, ToolState::Error);
    assert!(out.contains("cannot be used") && out.contains("worker") && !out.contains("lead,"), "{out}");
    // Bad cwd.
    let (st, _) = tool(&mut c, &env, &p, "create_session", json!({"name": "x", "agent": "worker", "prompt": "p", "cwd": "nope"})).await;
    assert_eq!(st, ToolState::Error);

    let (st, out) = tool(
        &mut c,
        &env,
        &p,
        "create_session",
        json!({"name": "子任务", "agent": "worker", "prompt": "work on it", "cwd": "sub"}),
    )
    .await;
    assert_eq!(st, ToolState::Done, "{out}");
    assert!(out.starts_with("Created background session \"子任务\""), "{out}");
    let child = env.server.hub.open_sessions().into_iter().find(|s| s.name() == "子任务").unwrap();
    let ci = child.info();
    assert_eq!(ci.cwd, format!("{}/sub", env.cwd));
    assert_eq!(ci.agent, "worker");
    assert_eq!(ci.parent.as_deref(), Some(p.as_str()));
    assert_eq!(ci.attached, 0);
    assert!(!ci.must_report);
    // Starts immediately.
    env.wait_done(&child.id).await;
    let cm = env.messages(&child.id);
    assert!(matches!(cm[0].origin, MessageOrigin::Spawn { .. }));
    assert!(cm[0].raw_text().ends_with("work on it"));
    assert!(cm[1].raw_text().starts_with("echo: [Task from session \"parent\""));
    // Name taken → suffix + note.
    let (_, out) = tool(&mut c, &env, &p, "create_session", json!({"name": "子任务", "agent": "build", "prompt": "again"})).await;
    assert!(out.contains("\"子任务 (2)\"") && out.contains("was taken"), "{out}");

    // close_session: self / foreground → errors; background child → ok.
    let (st, out) = tool(&mut c, &env, &p, "close_session", json!({"session": "parent"})).await;
    assert_eq!(st, ToolState::Error);
    assert!(out.contains("your own session"), "{out}");
    let (st, out) = tool(&mut c, &env, &p, "close_session", json!({"session": fg})).await;
    assert_eq!(st, ToolState::Error);
    assert!(out.contains("foreground"), "{out}");
    // A watched (not attached) session is still background.
    c.send(ClientMsg::Watch { session_id: child.id.clone() }).await;
    let (st, out) = tool(&mut c, &env, &p, "close_session", json!({"session": "子任务"})).await;
    assert_eq!(st, ToolState::Done, "{out}");
    assert!(env.server.hub.session(&child.id).is_none());
    assert!(!env.server.hub.history_info(&child.id).unwrap().open);
    env.server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn can_spawn_false_hides_spawn_tools_and_agent_list() {
    let env = setup(cmd_script(), agents_cfg()).await;
    let mut c = Client::connect(&env).await;
    let w = c.create_with(&env.cwd, Some("worker"), None, None).await.id;
    c.turn(&w, "hi").await;
    let b = c.create_with(&env.cwd, Some("build"), None, None).await.id;
    c.turn(&b, "hi").await;
    let reqs = env.llm_requests();
    let (rw, rb) = (&reqs[0], &reqs[1]);
    let names = tool_names(rw);
    assert!(!names.contains(&"create_session".to_string()) && !names.contains(&"close_session".to_string()), "{names:?}");
    assert!(names.contains(&"send_message".to_string()));
    assert!(!system_text(rw).contains("Agents available"), "{}", system_text(rw));
    let names = tool_names(rb);
    assert!(names.contains(&"create_session".to_string()) && names.contains(&"close_session".to_string()));
    let sys = system_text(rb);
    assert!(sys.contains("Agents available for create_session:\n- build:"), "{sys}");
    assert!(sys.contains("- worker: Does focused work") && !sys.contains("- lead"), "{sys}");
    // A can_spawn=false session calling create_session anyway gets an unknown-tool error.
    c.turn(&w, &call(json!([["create_session", {"name": "x", "agent": "build", "prompt": "p"}]]))).await;
    let t = env.tool_parts(&w).pop().unwrap();
    assert_eq!(t.state, ToolState::Error);
    assert!(t.output.contains("Unknown tool"), "{}", t.output);
    env.server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn must_report_reminders_capped_at_three() {
    let env = setup(cmd_script(), json!({})).await;
    let mut c = Client::connect(&env).await;
    let p = c.create_with(&env.cwd, None, None, Some("boss")).await.id;
    let (st, out) =
        tool(&mut c, &env, &p, "create_session", json!({"name": "lazy", "agent": "build", "prompt": "never reports", "must_report": true}))
            .await;
    assert_eq!(st, ToolState::Done, "{out}");
    let child = env.server.hub.open_sessions().into_iter().find(|s| s.name() == "lazy").unwrap();
    wait_until(Duration::from_secs(10), || {
        let st = child.st.lock();
        st.turn.is_none() && st.messages.iter().filter(|m| m.origin == MessageOrigin::Reminder).count() == 3
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let msgs = env.messages(&child.id);
    assert_eq!(msgs.iter().filter(|m| m.origin == MessageOrigin::Reminder).count(), 3);
    assert!(msgs[0].raw_text().contains("report back with send_message"));
    // The parent got nothing.
    assert!(!env.messages(&p).iter().any(|m| matches!(m.origin, MessageOrigin::Peer { .. })));
    // A peer message to the child does not re-arm the reminders.
    let (st, _) = tool(&mut c, &env, &p, "send_message", json!({"to": "lazy", "message": "status?"})).await;
    assert_eq!(st, ToolState::Done);
    env.wait_done(&child.id).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(env.messages(&child.id).iter().filter(|m| m.origin == MessageOrigin::Reminder).count(), 3);
    env.server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn queued_inputs_at_step_boundary_and_abort() {
    let env = setup(cmd_script(), json!({})).await;
    let mut c = Client::connect(&env).await;
    let a = c.create_with(&env.cwd, None, None, Some("a")).await.id;
    let b = c.create_with(&env.cwd, None, None, Some("b")).await.id;
    // b is busy with a slow tool; a peer message and a user prompt arrive meanwhile.
    c.prompt(&b, &call(json!([["bash", {"command": "sleep 1; echo slow"}]]))).await;
    c.until(|m| matches!(m, ServerMsg::PartUpsert { part: Part::Tool(t), .. } if t.state == ToolState::Running)).await;
    let (st, out) = tool(&mut c, &env, &a, "send_message", json!({"to": "b", "message": "peer while busy"})).await;
    assert_eq!(st, ToolState::Done);
    assert!(out.starts_with("Queued for \"b\""), "{out}");
    c.prompt(&b, "user while busy").await;
    env.wait_done(&b).await;
    // Both were inserted after the tool step, before the next LLM request, in arrival order.
    let reqs = env.llm_requests();
    let next = reqs.iter().find(|r| request_contains(r, "user while busy")).unwrap();
    let msgs = next["messages"].as_array().unwrap();
    let n = msgs.len();
    assert_eq!(msgs[n - 3]["role"], "tool");
    assert!(content_text(&msgs[n - 2]["content"]).contains("peer while busy"));
    assert_eq!(content_text(&msgs[n - 1]["content"]), "user while busy");
    // One request answered both (no extra turn).
    assert_eq!(reqs.iter().filter(|r| request_contains(r, "user while busy")).count(), 1);

    // Abort with a queued prompt: it lands in the history, no new turn starts.
    c.prompt(&b, &call(json!([["bash", {"command": "sleep 5"}]]))).await;
    c.until(|m| matches!(m, ServerMsg::PartUpsert { part: Part::Tool(t), .. } if t.state == ToolState::Running && t.input["command"] == "sleep 5")).await;
    c.prompt(&b, "queued then aborted").await;
    c.until(|m| matches!(m, ServerMsg::Notice { text, .. } if text.starts_with("Queued"))).await;
    let before = env.llm_requests().len();
    c.send(ClientMsg::Abort { session_id: b.clone() }).await;
    c.wait_status(&b, |s| *s == ref_proto::SessionStatus::Idle).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(env.llm_requests().len(), before);
    let msgs = env.messages(&b);
    assert_eq!(msgs.last().unwrap().raw_text(), "queued then aborted");
    let t = env.tool_parts(&b).pop().unwrap();
    assert_eq!(t.state, ToolState::Cancelled);
    // The next prompt sees everything, and the aborted tool call has a result.
    c.turn(&b, "next").await;
    let last = env.llm_requests().pop().unwrap();
    assert!(request_contains(&last, "queued then aborted"));
    assert!(tool_result_content(&last).iter().any(|r| r == "Aborted by user"));
    env.server.shutdown();
}
