//! Aborting a turn at every stage: the step's assistant message must end with
//! `finish: "aborted"` (never `tool_use`), open tool calls are cancelled, the session is Idle.
mod common;

use common::*;
use hn_proto::*;
use serde_json::{json, Value};
use std::sync::Arc;

/// Prompt, wait until `ready` matches, abort, and return the final state.
async fn abort_when(
    script: Script,
    extra: Value,
    what: &str,
    ready: impl FnMut(&ServerMsg) -> bool,
) -> (Env, SessionInfo, Vec<Message>, Mock) {
    let mock = Mock::start(script).await;
    let env = Env::new(mock.port, extra);
    let srv = env.spawn(&[]);
    let mut c = Client::connect(&srv.url(), &env.work()).await;
    let info = c.create(&env.work(), Some("abort")).await;
    c.prompt(&info.id, "go").await;
    c.wait_for(what, T, ready).await;
    assert_eq!(c.req(ClientMsg::Abort { session: info.id.clone() }).await.unwrap()["aborted"], true);
    c.wait_idle(&info.id, T).await;
    let (i, msgs) = c.snapshot(&info.id).await;
    // Persisted state agrees (flushed on close).
    c.req(ClientMsg::Close { session: info.id.clone() }).await.unwrap();
    let saved: Vec<Message> =
        serde_json::from_str(&std::fs::read_to_string(env.path("data/sessions").join(&info.id).join("messages.json")).unwrap()).unwrap();
    assert_eq!(finishes(&saved), finishes(&msgs), "persisted == live");
    drop(srv);
    (env, i, msgs, mock)
}

fn finishes(msgs: &[Message]) -> Vec<Option<String>> {
    msgs.iter().filter(|m| m.role == Role::Assistant).map(|m| m.finish.clone()).collect()
}

fn assert_aborted(i: &SessionInfo, msgs: &[Message]) {
    assert_eq!(i.status, SessionStatus::Idle);
    let last = msgs.iter().rev().find(|m| m.role == Role::Assistant).expect("assistant message");
    assert_eq!(last.finish.as_deref(), Some("aborted"), "{msgs:#?}");
    assert!(last.completed.is_some());
    for m in msgs.iter().filter(|m| m.role == Role::Assistant) {
        assert_ne!(m.finish.as_deref(), Some("tool_use"), "no step may stay tool_use after an abort: {m:#?}");
    }
    for tp in tool_parts(msgs) {
        assert!(tp.state.is_final(), "{tp:?}");
    }
}

fn tool_state(want: ToolState) -> impl FnMut(&ServerMsg) -> bool {
    move |m| matches!(m, ServerMsg::Event { event: Event::Part { part: Part::Tool(tp), .. }, .. } if tp.state == want)
}

#[tokio::test]
async fn abort_during_tool_delay() {
    let script: Script = Arc::new(|r: &Req| match r.n {
        0 => tool("bash", json!({"purpose": "p", "command": "echo x > delayed.txt"})),
        _ => text("after"),
    });
    let (env, i, msgs, mock) = abort_when(script, json!({"delay": {"bash": 30}}), "delayed", tool_state(ToolState::Delayed)).await;
    assert_aborted(&i, &msgs);
    assert_eq!(tool_parts(&msgs)[0].state, ToolState::Cancelled);
    assert!(!std::path::Path::new(&env.work()).join("delayed.txt").exists());
    assert_eq!(mock.requests().len(), 1);
}

#[tokio::test]
async fn abort_during_permission_wait() {
    let script: Script = Arc::new(|r: &Req| match r.n {
        0 => tool("bash", json!({"purpose": "p", "command": "echo x > asked.txt"})),
        _ => text("after"),
    });
    let (env, i, msgs, mock) = abort_when(script, json!({"permission": {"bash": "ask"}}), "permission", tool_state(ToolState::Permission)).await;
    assert_aborted(&i, &msgs);
    assert_eq!(tool_parts(&msgs)[0].state, ToolState::Cancelled);
    assert!(!std::path::Path::new(&env.work()).join("asked.txt").exists());
    assert_eq!(mock.requests().len(), 1);
}

#[tokio::test]
async fn abort_during_tool_run() {
    let script: Script = Arc::new(|r: &Req| match r.n {
        0 => tool("bash", json!({"purpose": "p", "command": "echo started; sleep 30"})),
        _ => text("after"),
    });
    let (_env, i, msgs, mock) = abort_when(script, json!({}), "running", tool_state(ToolState::Running)).await;
    assert_aborted(&i, &msgs);
    assert_eq!(tool_parts(&msgs)[0].state, ToolState::Cancelled);
    assert_eq!(mock.requests().len(), 1);
}

#[tokio::test]
async fn abort_one_of_parallel_tools_marks_step_aborted() {
    // One call finishes, the other is still running when the user aborts.
    let script: Script = Arc::new(|r: &Req| match r.n {
        0 => Reply::Tools(vec![
            ("bash".into(), json!({"purpose": "fast", "command": "echo fast"}).to_string()),
            ("bash".into(), json!({"purpose": "slow", "command": "sleep 30"}).to_string()),
        ]),
        _ => text("after"),
    });
    let (_env, i, msgs, _mock) = abort_when(script, json!({}), "fast done", tool_state(ToolState::Done)).await;
    assert_aborted(&i, &msgs);
    let tools = tool_parts(&msgs);
    assert_eq!(tools[0].state, ToolState::Done);
    assert_eq!(tools[1].state, ToolState::Cancelled);
}

#[tokio::test]
async fn abort_during_streaming() {
    let script: Script = Arc::new(|_r: &Req| Reply::Stall("partial answer".into()));
    let (_env, i, msgs, _mock) = abort_when(script, json!({}), "text part", |m| {
        matches!(m, ServerMsg::Event { event: Event::Part { part: Part::Text { .. }, .. }, .. })
    })
    .await;
    assert_aborted(&i, &msgs);
    assert_eq!(last_assistant_text(&msgs), "partial answer");
}

#[tokio::test]
async fn abort_during_retry_wait() {
    let script: Script = Arc::new(|_r: &Req| Reply::Status(503, "overloaded".into()));
    let extra = json!({"retry": {"initial_delay": 30, "max_delay": 60}});
    let (_env, i, msgs, mock) = abort_when(script, extra, "retry status", |m| {
        matches!(m, ServerMsg::Event { event: Event::Info { info }, .. } if matches!(info.status, SessionStatus::Retry { .. }))
    })
    .await;
    assert_eq!(i.status, SessionStatus::Idle);
    // The failed attempt's partial message was discarded before waiting; nothing is left open.
    for m in msgs.iter().filter(|m| m.role == Role::Assistant) {
        assert!(m.completed.is_some());
        assert_ne!(m.finish.as_deref(), Some("tool_use"));
    }
    assert_eq!(mock.requests().len(), 1, "no retry after the abort");
}

#[tokio::test]
async fn abort_during_tool_step_retry_keeps_earlier_steps_intact() {
    // Step 1 runs a tool to completion; step 2 fails and waits to retry; abort then.
    let script: Script = Arc::new(|r: &Req| match r.n {
        0 => tool("bash", json!({"purpose": "p", "command": "echo ok"})),
        _ => Reply::Status(503, "overloaded".into()),
    });
    let extra = json!({"retry": {"initial_delay": 30, "max_delay": 60}});
    let (_env, i, msgs, _mock) = abort_when(script, extra, "retry status", |m| {
        matches!(m, ServerMsg::Event { event: Event::Info { info }, .. } if matches!(info.status, SessionStatus::Retry { .. }))
    })
    .await;
    assert_eq!(i.status, SessionStatus::Idle);
    // The completed tool step legitimately ended with tool_use: its tools all ran.
    assert_eq!(finishes(&msgs), vec![Some("tool_use".to_string())]);
    assert_eq!(tool_parts(&msgs)[0].state, ToolState::Done);
}

#[tokio::test]
async fn abort_during_manual_compaction() {
    let script: Script = Arc::new(|r: &Req| {
        if r.last_user().contains("You MUST summarize the conversation") {
            Reply::Stall("## Objective".into())
        } else {
            text("plain answer")
        }
    });
    let mock = Mock::start(script).await;
    let env = Env::new(mock.port, json!({}));
    let srv = env.spawn(&[]);
    let mut c = Client::connect(&srv.url(), &env.work()).await;
    let info = c.create(&env.work(), Some("compact-abort")).await;
    c.prompt(&info.id, "hello").await;
    c.wait_idle(&info.id, T).await;
    c.req(ClientMsg::Compact { session: info.id.clone() }).await.unwrap();
    c.wait_for("compaction toast", T, |m| matches!(m, ServerMsg::Toast { message, .. } if message.contains("Compacting")))
        .await;
    // Give the summary request time to reach the mock.
    for _ in 0..50 {
        if mock.requests().iter().any(|b| b.to_string().contains("You MUST summarize")) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(c.req(ClientMsg::Abort { session: info.id.clone() }).await.unwrap()["aborted"], true);
    c.wait_idle(&info.id, T).await;
    let (i, msgs) = c.snapshot(&info.id).await;
    assert_eq!(i.status, SessionStatus::Idle);
    assert!(!msgs.iter().any(|m| m.origin == Origin::Compaction), "no checkpoint from an aborted compaction");
    assert_eq!(finishes(&msgs), vec![Some("stop".to_string())]);
    // The session still works afterwards.
    c.prompt(&info.id, "again").await;
    c.wait_idle(&info.id, T).await;
    let (_, msgs) = c.snapshot(&info.id).await;
    assert_eq!(last_assistant_text(&msgs), "plain answer");
}

#[tokio::test]
async fn shutdown_during_tool_run_persists_aborted() {
    let script: Script = Arc::new(|r: &Req| match r.n {
        0 => tool("bash", json!({"purpose": "p", "command": "sleep 30"})),
        _ => text("after"),
    });
    let mock = Mock::start(script).await;
    let env = Env::new(mock.port, json!({}));
    let mut srv = env.spawn(&[]);
    let mut c = Client::connect(&srv.url(), &env.work()).await;
    let info = c.create(&env.work(), Some("sigterm")).await;
    c.prompt(&info.id, "go").await;
    c.wait_for("running", T, tool_state(ToolState::Running)).await;
    // SAFETY: plain kill on our own child.
    unsafe { libc::kill(srv.child.id() as i32, libc::SIGTERM) };
    srv.wait_exit(T).await.expect("exit on SIGTERM");
    let saved: Vec<Message> =
        serde_json::from_str(&std::fs::read_to_string(env.path("data/sessions").join(&info.id).join("messages.json")).unwrap()).unwrap();
    assert_eq!(finishes(&saved), vec![Some("aborted".to_string())]);
    assert_eq!(tool_parts(&saved)[0].state, ToolState::Cancelled);
}
