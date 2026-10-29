//! Permissions: allow / deny / ask, per-agent overrides, bash patterns, Always remembered per
//! session, re-delivery to late clients, delays with run_at and cancellation.

mod common;

use std::time::{Duration, Instant};

use common::*;
use ref_proto::{ClientMsg, Part, PermissionDecision, ServerMsg, SessionStatus, ToolState};
use serde_json::json;

async fn ask(c: &mut Client) -> ref_proto::PermissionRequest {
    match c.until(|m| matches!(m, ServerMsg::PermissionAsk { .. })).await {
        ServerMsg::PermissionAsk { request } => request,
        _ => unreachable!(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rules_patterns_agent_override_and_always() {
    let env = setup(
        cmd_script(),
        json!({
            "permission": {"bash": {"*": "ask", "echo safe*": "allow", "rm *": "deny"}, "write": "deny"},
            "agent": {"trusted": {"permission": {"write": "allow", "bash": "allow"}}}
        }),
    )
    .await;
    let mut c = Client::connect(&env).await;
    let sid = c.create(&env.cwd).await;
    // Pattern allow / deny without asking; global deny for write.
    c.turn(
        &sid,
        &call(json!([["bash", {"command": "echo safe one"}], ["bash", {"command": "rm -rf x"}], ["write", {"path": "w.txt", "content": "x"}]])),
    )
    .await;
    let t = env.tool_parts(&sid);
    assert_eq!(t[0].state, ToolState::Done, "{}", t[0].output);
    assert_eq!(t[1].state, ToolState::Denied);
    assert_eq!(t[2].state, ToolState::Denied);
    assert!(!std::path::Path::new(&format!("{}/w.txt", env.cwd)).exists());
    assert!(!c.log.iter().any(|m| matches!(m, ServerMsg::PermissionAsk { .. })));

    // Ask → Deny → error result returned to the model; loop continues.
    c.prompt(&sid, &call(json!([["bash", {"command": "echo asked", "purpose": "需要确认"}]]))).await;
    let r = ask(&mut c).await;
    assert_eq!(r.purpose, "需要确认");
    assert_eq!(r.input["command"], "echo asked");
    c.send(ClientMsg::Permission { session_id: sid.clone(), call_id: r.call_id.clone(), decision: PermissionDecision::Deny }).await;
    c.until_idle_with(&sid, "done").await;
    let t = env.tool_parts(&sid).pop().unwrap();
    assert_eq!(t.state, ToolState::Denied);
    assert_eq!(t.output, "The user denied this tool call");

    // Always: remembered for the rest of this session (the second call is not asked).
    c.prompt(&sid, &call(json!([["bash", {"command": "echo a1"}]]))).await;
    let r = ask(&mut c).await;
    c.send(ClientMsg::Permission { session_id: sid.clone(), call_id: r.call_id, decision: PermissionDecision::Always }).await;
    c.until_idle_with(&sid, "done").await;
    let asks = c.log.iter().filter(|m| matches!(m, ServerMsg::PermissionAsk { .. })).count();
    c.turn(&sid, &call(json!([["bash", {"command": "echo a2"}]]))).await;
    assert_eq!(c.log.iter().filter(|m| matches!(m, ServerMsg::PermissionAsk { .. })).count(), asks);
    assert_eq!(env.tool_parts(&sid).pop().unwrap().state, ToolState::Done);
    // ... but deny rules still apply, and other sessions still ask.
    c.turn(&sid, &call(json!([["bash", {"command": "rm y"}]]))).await;
    assert_eq!(env.tool_parts(&sid).pop().unwrap().state, ToolState::Denied);
    let other = c.create(&env.cwd).await;
    c.prompt(&other, &call(json!([["bash", {"command": "echo other"}]]))).await;
    let r = ask(&mut c).await;
    assert_eq!(r.session_id, other);
    c.send(ClientMsg::Permission { session_id: other.clone(), call_id: r.call_id, decision: PermissionDecision::Once }).await;
    env.wait_done(&other).await;

    // Per-agent override of global rules.
    let tr = c.create_with(&env.cwd, Some("trusted"), None, None).await.id;
    c.turn(&tr, &call(json!([["write", {"path": "w.txt", "content": "x"}], ["bash", {"command": "echo t"}]]))).await;
    let t = env.tool_parts(&tr);
    assert_eq!(t[0].state, ToolState::Done, "{}", t[0].output);
    assert_eq!(t[1].state, ToolState::Done, "{}", t[1].output);
    env.server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pending_permission_resent_to_late_clients_and_resolved_everywhere() {
    let env = setup(cmd_script(), json!({"permission": {"bash": "ask"}})).await;
    let mut a = Client::connect(&env).await;
    let sid = a.create(&env.cwd).await;
    a.prompt(&sid, &call(json!([["bash", {"command": "echo late"}]]))).await;
    let r = ask(&mut a).await;
    wait_until(Duration::from_secs(5), || env.core(&sid).info().pending_permissions == 1).await;
    // A client that attaches later gets the request in the snapshot; so does a watcher.
    let mut b = Client::connect(&env).await;
    let (info, _, perms) = b.snapshot(&sid).await;
    assert_eq!(info.pending_permissions, 1);
    assert_eq!(perms.len(), 1);
    assert_eq!(perms[0].call_id, r.call_id);
    let mut w = Client::connect(&env).await;
    w.send(ClientMsg::Watch { session_id: sid.clone() }).await;
    let ServerMsg::SessionSnapshot { permissions, .. } = w.until(|m| matches!(m, ServerMsg::SessionSnapshot { .. })).await else {
        unreachable!()
    };
    assert_eq!(permissions.len(), 1);
    // The late client answers; everyone sees it resolved.
    b.send(ClientMsg::Permission { session_id: sid.clone(), call_id: r.call_id.clone(), decision: PermissionDecision::Once }).await;
    let cid = r.call_id.clone();
    a.until(move |m| matches!(m, ServerMsg::PermissionResolved { call_id, .. } if *call_id == cid)).await;
    let cid = r.call_id.clone();
    w.until(move |m| matches!(m, ServerMsg::PermissionResolved { call_id, .. } if *call_id == cid)).await;
    // A second answer for the same call is acknowledged with PermissionResolved, no error.
    a.send(ClientMsg::Permission { session_id: sid.clone(), call_id: r.call_id.clone(), decision: PermissionDecision::Deny }).await;
    let cid = r.call_id.clone();
    a.until(move |m| matches!(m, ServerMsg::PermissionResolved { call_id, .. } if *call_id == cid)).await;
    env.wait_done(&sid).await;
    assert_eq!(env.tool_parts(&sid)[0].state, ToolState::Done);
    env.server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn abort_while_asking_cancels_request() {
    let env = setup(cmd_script(), json!({"permission": {"bash": "ask"}})).await;
    let mut c = Client::connect(&env).await;
    let sid = c.create(&env.cwd).await;
    c.prompt(&sid, &call(json!([["bash", {"command": "echo x"}], ["bash", {"command": "echo y"}]]))).await;
    let r = ask(&mut c).await;
    c.send(ClientMsg::Abort { session_id: sid.clone() }).await;
    let cid = r.call_id.clone();
    c.until(move |m| matches!(m, ServerMsg::PermissionResolved { call_id, .. } if *call_id == cid)).await;
    c.wait_status(&sid, |s| *s == SessionStatus::Idle).await;
    let core = env.core(&sid);
    assert_eq!(core.info().pending_permissions, 0);
    let t = env.tool_parts(&sid);
    assert!(t.iter().all(|t| t.state == ToolState::Cancelled), "{t:?}");
    env.server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn delay_with_run_at_and_cancellation() {
    let env = setup(cmd_script(), json!({"delay": {"bash": 1.0}, "agent": {"fast": {"delay": {"bash": 0}}}})).await;
    let mut c = Client::connect(&env).await;
    let sid = c.create(&env.cwd).await;
    let t0 = Instant::now();
    c.prompt(&sid, &call(json!([["bash", {"command": "echo delayed"}]]))).await;
    let ServerMsg::PartUpsert { part: Part::Tool(t), .. } =
        c.until(|m| matches!(m, ServerMsg::PartUpsert { part: Part::Tool(t), .. } if t.state == ToolState::Delayed)).await
    else {
        unreachable!()
    };
    let run_at = t.run_at.expect("run_at");
    let d = run_at - ref_proto::now_ms();
    assert!((700..=1100).contains(&d), "{d}");
    c.until_idle_with(&sid, "done").await;
    assert!(t0.elapsed() >= Duration::from_millis(1000));
    let t = env.tool_parts(&sid).pop().unwrap();
    assert_eq!(t.state, ToolState::Done);
    assert!(t.run_at.is_none());
    // Abort during the delay: the command never runs.
    c.prompt(&sid, &call(json!([["bash", {"command": "touch never.txt"}]]))).await;
    c.until(|m| matches!(m, ServerMsg::PartUpsert { part: Part::Tool(t), .. } if t.state == ToolState::Delayed && t.input["command"] == "touch never.txt")).await;
    c.send(ClientMsg::Abort { session_id: sid.clone() }).await;
    c.wait_status(&sid, |s| *s == SessionStatus::Idle).await;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert!(!std::path::Path::new(&format!("{}/never.txt", env.cwd)).exists());
    assert_eq!(env.tool_parts(&sid).pop().unwrap().state, ToolState::Cancelled);
    // Agent override: no delay.
    let f = c.create_with(&env.cwd, Some("fast"), None, None).await.id;
    let t1 = Instant::now();
    c.turn(&f, &call(json!([["bash", {"command": "echo fast"}]]))).await;
    assert!(t1.elapsed() < Duration::from_millis(900), "{:?}", t1.elapsed());
    env.server.shutdown();
}
