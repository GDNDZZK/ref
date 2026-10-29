mod common;

use common::*;
use ref_proto::{MessageSource, Part, Role, SessionEvent, SessionStatus, ToolState};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

const T: Duration = Duration::from_secs(10);

#[tokio::test]
async fn basic_turn_and_persistence() {
    let m = mock(Arc::new(|_, _| Reply::Text("Hello there".into()))).await;
    let (hub, work) = hub(config(&m.url), "basic");
    let s = session(&hub, &work, "main", "build").await;
    prompt(&hub, &s, "hi");
    wait_idle(&s).await;
    assert_eq!(last_text(&s), "Hello there");
    let msgs = messages(&s);
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[1].usage.unwrap().input, 1000);
    assert!(s.info().context.used > 0);
    // Tools sent with purpose first, in fixed order.
    let body = &m.bodies()[0];
    let tools = body["tools"].as_array().unwrap();
    assert_eq!(tools[0]["function"]["name"], "read");
    let props = tools[0]["function"]["parameters"]["properties"].as_object().unwrap();
    assert_eq!(props.keys().next().unwrap(), "purpose");
    assert!(tools.iter().any(|t| t["function"]["name"] == "create_session"));
    // Persisted to disk.
    let p = hub.store.load(&s.id).unwrap();
    assert_eq!(p.messages.len(), 2);
    // Title generated asynchronously.
    let s2 = s.clone();
    wait_until("title", T, move || s2.info().title == "Mock title").await;
}

#[tokio::test]
async fn tool_loop_errors_do_not_terminate_and_replay_is_stable() {
    let m = mock(Arc::new(|n, _| match n {
        0 => Reply::Tools(vec![
            tool("c1", "read", json!({"purpose": "Read missing file", "path": "nope.txt"})),
            tool("c2", "no_such_tool", json!({"purpose": "x"})),
            ("c3".into(), "bash".into(), "{not json".into()),
            tool("c4", "bash", json!({"purpose": "Say hi", "command": "echo hi"})),
        ]),
        _ => Reply::Text("done".into()),
    }))
    .await;
    let (hub, work) = hub(config(&m.url), "toolloop");
    let s = session(&hub, &work, "main", "build").await;
    prompt(&hub, &s, "go");
    wait_idle(&s).await;
    assert_eq!(last_text(&s), "done");
    let tools = tool_parts(&s);
    assert_eq!(tools.len(), 4);
    let states: Vec<ToolState> = tools.iter().map(|p| if let Part::Tool { state, .. } = p { *state } else { unreachable!() }).collect();
    assert_eq!(states, [ToolState::Error, ToolState::Error, ToolState::Error, ToolState::Done]);
    if let Part::Tool { purpose, input, .. } = &tools[0] {
        assert_eq!(purpose, "Read missing file");
        assert_eq!(input, &json!({"path": "nope.txt"}));
    }
    let bodies = m.bodies();
    assert_eq!(bodies.len(), 2);
    let results = tool_results(&bodies[1]);
    assert_eq!(results.len(), 4);
    assert!(results[1]["content"].as_str().unwrap().contains("Unknown tool"));
    assert!(results[2]["content"].as_str().unwrap().contains("Invalid JSON"));
    assert!(results[3]["content"].as_str().unwrap().contains("hi"));
    // Byte-identical replay: raw arguments incl. purpose.
    let am = bodies[1]["messages"].as_array().unwrap().iter().find(|x| x["role"] == "assistant").unwrap().clone();
    assert_eq!(
        am["tool_calls"][0]["function"]["arguments"],
        json!(json!({"purpose": "Read missing file", "path": "nope.txt"}).to_string())
    );
    // System prompt and tools identical between requests.
    assert_eq!(bodies[0]["messages"][0], bodies[1]["messages"][0]);
    assert_eq!(bodies[0]["tools"], bodies[1]["tools"]);
}

#[tokio::test]
async fn permission_ask_then_reply_and_deny() {
    let m = mock(Arc::new(|n, _| match n {
        0 => Reply::Tools(vec![
            tool("c1", "bash", json!({"purpose": "p1", "command": "echo one"})),
            tool("c2", "bash", json!({"purpose": "p2", "command": "echo two"})),
            tool("c3", "delete", json!({"purpose": "p3", "path": "x"})),
        ]),
        _ => Reply::Text("ok".into()),
    }))
    .await;
    let mut cfg = config(&m.url);
    cfg.permission = perms(json!({"bash": "ask", "delete": {"*": "deny", "keep": "allow"}}));
    let (hub, work) = hub(cfg, "perm");
    let s = session(&hub, &work, "main", "build").await;
    prompt(&hub, &s, "go");
    // First ask: allow once.
    let s2 = s.clone();
    wait_until("permission 1", T, move || s2.lock().perms.len() == 1).await;
    {
        let st = s.lock();
        assert_eq!(st.info.status, SessionStatus::WaitingPermission);
        assert_eq!(st.perms[0].request.input, json!({"command": "echo one"}));
        assert_eq!(st.perms[0].request.purpose, "p1");
    }
    let tx = s.lock().perms[0].tx.take().unwrap();
    tx.send((true, false)).unwrap();
    // Second ask: deny.
    let s2 = s.clone();
    wait_until("permission 2", T, move || s2.lock().perms.first().is_some_and(|p| p.request.call_id == "c2" && p.tx.is_some())).await;
    let tx = s.lock().perms[0].tx.take().unwrap();
    tx.send((false, false)).unwrap();
    wait_idle(&s).await;
    let tools = tool_parts(&s);
    let st: Vec<(ToolState, String)> = tools
        .iter()
        .map(|p| if let Part::Tool { state, output, .. } = p { (*state, output.clone().unwrap_or_default()) } else { unreachable!() })
        .collect();
    assert_eq!(st[0].0, ToolState::Done);
    assert_eq!(st[1], (ToolState::Denied, "The user denied this tool call.".into()));
    assert_eq!(st[2].0, ToolState::Denied);
    assert!(st[2].1.contains("Permission denied by configuration"));
    assert_eq!(last_text(&s), "ok");
}

#[tokio::test]
async fn permission_always_allows_rest_of_session() {
    let m = mock(Arc::new(|n, _| match n {
        0 => Reply::Tools(vec![
            tool("c1", "bash", json!({"purpose": "p", "command": "true"})),
            tool("c2", "bash", json!({"purpose": "p", "command": "true"})),
        ]),
        _ => Reply::Text("ok".into()),
    }))
    .await;
    let mut cfg = config(&m.url);
    cfg.permission = perms(json!({"bash": "ask"}));
    let (hub, work) = hub(cfg, "always");
    let s = session(&hub, &work, "main", "build").await;
    prompt(&hub, &s, "go");
    let s2 = s.clone();
    wait_until("permission", T, move || s2.lock().perms.len() == 1).await;
    let tx = s.lock().perms[0].tx.take().unwrap();
    tx.send((true, true)).unwrap();
    wait_idle(&s).await;
    assert!(tool_parts(&s).iter().all(|p| matches!(p, Part::Tool { state: ToolState::Done, .. })));
}

#[tokio::test]
async fn agent_permission_overrides_global_and_delay_applies() {
    let m = mock(Arc::new(|n, _| match n {
        0 => Reply::Tools(vec![tool("c1", "bash", json!({"purpose": "p", "command": "echo delayed"}))]),
        _ => Reply::Text("ok".into()),
    }))
    .await;
    let mut cfg = config(&m.url);
    cfg.permission = perms(json!({"bash": "deny"}));
    cfg.agents.get_mut("build").unwrap().permission = perms(json!({"bash": "allow"}));
    cfg.delay = vec![("bash".into(), 5.0)];
    cfg.agents.get_mut("build").unwrap().delay = vec![("bash".into(), 0.4)];
    let (hub, work) = hub(cfg, "delay");
    let s = session(&hub, &work, "main", "build").await;
    let start = std::time::Instant::now();
    prompt(&hub, &s, "go");
    let s2 = s.clone();
    wait_until("delayed state", T, move || {
        tool_parts(&s2).iter().any(|p| matches!(p, Part::Tool { state: ToolState::Delayed, run_at: Some(_), .. }))
    })
    .await;
    wait_idle(&s).await;
    let el = start.elapsed();
    assert!(el >= Duration::from_millis(400) && el < Duration::from_secs(4), "{el:?}");
    assert!(matches!(&tool_parts(&s)[0], Part::Tool { state: ToolState::Done, .. }));
}

#[tokio::test]
async fn abort_during_delay_and_stream() {
    let m = mock(Arc::new(|n, _| match n {
        0 => Reply::Tools(vec![tool("c1", "bash", json!({"purpose": "p", "command": "echo x"}))]),
        1 => Reply::Hang("partial".into()),
        _ => Reply::Text("after".into()),
    }))
    .await;
    let mut cfg = config(&m.url);
    cfg.delay = vec![("bash".into(), 30.0)];
    let (hub, work) = hub(cfg, "abort");
    let s = session(&hub, &work, "main", "build").await;
    prompt(&hub, &s, "go");
    let s2 = s.clone();
    wait_until("delayed", T, move || tool_parts(&s2).iter().any(|p| matches!(p, Part::Tool { state: ToolState::Delayed, .. }))).await;
    ref_server::engine::abort(&hub, &s, true);
    wait_idle(&s).await;
    let tools = tool_parts(&s);
    assert!(matches!(&tools[0], Part::Tool { state: ToolState::Cancelled, output: Some(o), .. } if o == "Cancelled by user."));
    let last = messages(&s).last().unwrap().clone();
    assert_eq!(last.error.as_deref(), Some("Aborted"));
    assert_eq!(s.info().status, SessionStatus::Idle);

    // Next prompt: the cancelled tool still has a result; then abort mid-stream.
    prompt(&hub, &s, "again");
    let s2 = s.clone();
    wait_until("partial text", T, move || last_text(&s2) == "partial").await;
    ref_server::engine::abort(&hub, &s, true);
    wait_idle(&s).await;
    let bodies = m.bodies();
    assert_eq!(tool_results(&bodies[1]).len(), 1);
    let last = messages(&s).last().unwrap().clone();
    assert_eq!(last.role, Role::Assistant);
    assert_eq!(last.error.as_deref(), Some("Aborted"));
    assert_eq!(last.raw_text(), "partial");
}

#[tokio::test]
async fn retry_on_500_then_success() {
    let m = mock(Arc::new(|n, _| match n {
        0 | 1 => Reply::Status(500, r#"{"error":{"message":"internal"}}"#.into()),
        _ => Reply::Text("recovered".into()),
    }))
    .await;
    let (hub, work) = hub(config(&m.url), "retry");
    let s = session(&hub, &work, "main", "build").await;
    prompt(&hub, &s, "go");
    wait_idle(&s).await;
    assert_eq!(last_text(&s), "recovered");
    assert_eq!(m.count(), 3);
    // Failed partial messages are removed: only user + final assistant.
    assert_eq!(messages(&s).len(), 2);
}

#[tokio::test]
async fn non_retryable_error_fails_after_three_attempts() {
    let m = mock(Arc::new(|_, _| Reply::Status(400, r#"{"error":{"message":"bad request"}}"#.into()))).await;
    let (hub, work) = hub(config(&m.url), "fail");
    let s = session(&hub, &work, "main", "build").await;
    prompt(&hub, &s, "go");
    wait_idle(&s).await;
    assert_eq!(m.count(), 3);
    let last = messages(&s).last().unwrap().clone();
    assert!(last.error.unwrap().contains("bad request"));
}

#[test]
fn backoff_math() {
    use ref_server::engine::backoff;
    let b = Duration::from_secs(2);
    let max = Duration::from_secs(600);
    assert_eq!(backoff(b, 1, max, None, 0.0), Duration::from_secs(2));
    assert_eq!(backoff(b, 3, max, None, 0.0), Duration::from_secs(8));
    assert_eq!(backoff(b, 20, max, None, 0.0), max);
    assert_eq!(backoff(b, 20, max, None, 0.2), max);
    assert_eq!(backoff(b, 1, max, Some(Duration::from_secs(30)), 0.0), Duration::from_secs(30));
    assert_eq!(backoff(b, 1, max, Some(Duration::from_secs(9999)), 0.0), max);
    let j = backoff(b, 2, max, None, -0.2);
    assert!((j.as_secs_f64() - 3.2).abs() < 1e-9);
}

fn non_system(body: &serde_json::Value) -> Vec<serde_json::Value> {
    body["messages"].as_array().unwrap().iter().filter(|x| x["role"] != "system").cloned().collect()
}

#[tokio::test]
async fn compact_context_tool_keeps_history_but_sends_summary_only() {
    let m = mock(Arc::new(|n, _| match n {
        0 => Reply::Tools(vec![tool("c1", "compact_context", json!({"purpose": "p", "summary": "The user wants X. Done Y."}))]),
        _ => Reply::Text("continuing".into()),
    }))
    .await;
    let (hub, work) = hub(config(&m.url), "compact");
    let s = session(&hub, &work, "main", "build").await;
    prompt(&hub, &s, "start");
    wait_idle(&s).await;
    let msgs = messages(&s);
    // History retained: [start, assistant(tool), summary, continuing].
    assert_eq!(msgs.len(), 4, "{msgs:#?}");
    assert_eq!(msgs[0].raw_text(), "start");
    assert_eq!(msgs[2].source, MessageSource::Compaction);
    assert!(msgs[2].raw_text().contains("The user wants X. Done Y."));
    assert_eq!(last_text(&s), "continuing");
    let bodies = m.bodies();
    let second = non_system(&bodies[1]);
    assert_eq!(second.len(), 1, "{second:#?}");
    assert!(second[0]["content"].as_str().unwrap().starts_with("[Context compacted."));
    // The next request extends the same effective prefix (prompt caching).
    prompt(&hub, &s, "more");
    wait_idle(&s).await;
    let third = non_system(&m.bodies()[2]);
    // [summary, continuing, more]
    assert_eq!(third.len(), 3, "{third:#?}");
    assert_eq!(third[0], second[0]);
    assert!(third.iter().all(|x| x["content"] != "start"));
    // Persisted with the full transcript.
    assert_eq!(hub.store.load(&s.id).unwrap().messages.len(), 6);
}

#[tokio::test]
async fn manual_compaction_uses_model_summary_and_undo_restores_context() {
    let m = mock(Arc::new(|n, _| match n {
        0 => Reply::Text("first answer".into()),
        1 => Reply::Text("## Goal\n- summary text".into()),
        _ => Reply::Text("later".into()),
    }))
    .await;
    let (hub, work) = hub(config(&m.url), "manualcompact");
    let s = session(&hub, &work, "main", "build").await;
    prompt(&hub, &s, "q");
    wait_idle(&s).await;
    ref_server::engine::start_custom_run(&hub, &s, ref_server::compaction::manual).unwrap();
    wait_idle(&s).await;
    let msgs = messages(&s);
    assert_eq!(msgs.len(), 3, "{msgs:#?}");
    assert_eq!(msgs[0].raw_text(), "q");
    assert_eq!(msgs[2].source, MessageSource::Compaction);
    assert!(msgs[2].raw_text().contains("summary text"));
    let b = m.bodies();
    assert!(b[1].get("tools").is_none());
    // Default compaction prompt (no compaction agent configured) as the system prompt.
    assert_eq!(b[1]["messages"][0]["content"], ref_config::COMPACTION_PROMPT);
    // Context usage estimated from the new effective context (system + tools + summary),
    // not the last reported usage.
    let used = s.info().context.used;
    assert!(used > 0 && used != 1020, "{used}");
    // Compacting again without new assistant output is refused.
    ref_server::engine::start_custom_run(&hub, &s, ref_server::compaction::manual).unwrap();
    wait_idle(&s).await;
    assert_eq!(m.count(), 2);

    prompt(&hub, &s, "next");
    wait_idle(&s).await;
    let after = non_system(&m.bodies()[2]);
    assert_eq!(after.len(), 1, "{after:#?}");
    let c = after[0]["content"].as_str().unwrap();
    assert!(c.contains("summary text") && c.ends_with("next") && !c.contains("first answer"), "{c}");

    // Undo from the compaction message: the old context becomes effective again.
    let conn = hub.add_conn();
    conn.lock().hello = true;
    let cid = msgs[2].id.clone();
    ref_server::api::handle(&hub, &conn, ref_proto::Request::Undo { session_id: s.id.clone(), message_id: Some(cid) }).await.unwrap();
    assert_eq!(messages(&s).len(), 2);
    prompt(&hub, &s, "again");
    wait_idle(&s).await;
    let restored = non_system(&m.bodies()[3]);
    assert_eq!(restored.len(), 3);
    assert_eq!(restored[0]["content"], "q");
}

#[tokio::test]
async fn compaction_agent_prompt_and_model_from_config() {
    let m = mock(Arc::new(|n, _| match n {
        0 => Reply::Text("first answer".into()),
        _ => Reply::Text("custom summary".into()),
    }))
    .await;
    let mut cfg = config(&m.url);
    cfg.providers.get_mut("mock").unwrap().models.insert(
        "small".into(),
        ref_config::ModelConfig { key: "small".into(), id: "small-wire".into(), name: "Small".into(), context: 10_000, ..Default::default() },
    );
    cfg.agents.insert(
        "compaction".into(),
        ref_config::AgentConfig {
            name: "compaction".into(),
            prompt: "CUSTOM COMPACTION PROMPT".into(),
            model: Some(ref_proto::ModelRef { provider: "mock".into(), model: "small".into(), variant: None }),
            hidden: true,
            ..Default::default()
        },
    );
    let (hub, work) = hub(cfg, "compactagent");
    let s = session(&hub, &work, "main", "build").await;
    prompt(&hub, &s, "q");
    wait_idle(&s).await;
    ref_server::engine::start_custom_run(&hub, &s, ref_server::compaction::manual).unwrap();
    wait_idle(&s).await;
    let b = m.bodies();
    assert_eq!(b[0]["model"], "m");
    assert_eq!(b[1]["model"], "small-wire");
    assert_eq!(b[1]["messages"][0]["content"], "CUSTOM COMPACTION PROMPT");
    let tail = non_system(&b[1]);
    assert_eq!(tail.len(), 3);
    assert_eq!(tail[2]["content"], ref_server::compaction::SUMMARY_REQUEST);
    assert!(messages(&s)[2].raw_text().contains("custom summary"));
    // Hidden agents are not offered to frontends and cannot run sessions.
    let conn = hub.add_conn();
    conn.lock().hello = true;
    let r = ref_server::api::handle(&hub, &conn, ref_proto::Request::GetConfig).await.unwrap();
    let names: Vec<&str> = r["config"]["agents"].as_array().unwrap().iter().map(|a| a["name"].as_str().unwrap()).collect();
    assert!(!names.contains(&"compaction"), "{names:?}");
    let e = ref_server::api::handle(&hub, &conn, ref_proto::Request::SetAgent { session_id: s.id.clone(), agent: "compaction".into() }).await;
    assert!(e.is_err());
}

#[tokio::test]
async fn recommend_compaction_notice_once_per_epoch() {
    let m = mock(Arc::new(|n, _| match n {
        0 => Reply::Tools(vec![tool("c1", "bash", json!({"purpose": "p", "command": "echo hi"}))]),
        1 => Reply::Text("done".into()),
        2 => Reply::Text("second".into()),
        3 => Reply::Tools(vec![tool("c2", "compact_context", json!({"purpose": "p", "summary": "S"}))]),
        _ => Reply::Text("fresh".into()),
    }))
    .await;
    let mut cfg = config(&m.url);
    cfg.compaction.recommend = 1000.0;
    let (hub, work) = hub(cfg, "notice");
    let s = session(&hub, &work, "main", "build").await;
    prompt(&hub, &s, "go");
    wait_idle(&s).await;
    // Injected after the tool batch, before the next request of the same run.
    let msgs = messages(&s);
    assert_eq!(msgs.len(), 4, "{msgs:#?}");
    assert_eq!(msgs[2].source, MessageSource::Reminder);
    let text = msgs[2].raw_text();
    assert!(text.starts_with("[Context notice] This conversation has used 1020 of 10000 tokens (10%), past the recommended compaction point (1000 tokens)."), "{text}");
    let b1 = non_system(&m.bodies()[1]);
    assert_eq!(b1[b1.len() - 2]["role"], "tool");
    assert_eq!(b1[b1.len() - 1]["role"], "user");
    assert!(b1[b1.len() - 1]["content"].as_str().unwrap().starts_with("[Context notice]"));
    assert!(s.lock().context_notice);
    assert!(hub.store.load(&s.id).unwrap().context_notice, "flag persisted");
    // Only once per context epoch.
    prompt(&hub, &s, "two");
    wait_idle(&s).await;
    let count = |s: &Arc<ref_server::session::Session>| messages(s).iter().filter(|m| m.source == MessageSource::Reminder).count();
    assert_eq!(count(&s), 1);
    // After a compaction it can fire again; with no further request in the run it is
    // appended at the end (and goes out with the next prompt).
    prompt(&hub, &s, "three");
    wait_idle(&s).await;
    assert_eq!(count(&s), 2);
    let last = messages(&s).last().unwrap().clone();
    assert_eq!(last.source, MessageSource::Reminder);
    assert_eq!(s.info().status, SessionStatus::Idle);
    assert_eq!(m.count(), 5, "no extra run for the notice");
}

#[tokio::test]
async fn mcp_status_request() {
    let m = mock(Arc::new(|_, _| Reply::Text("x".into()))).await;
    let mut cfg = config(&m.url);
    cfg.mcp.insert(
        "off".into(),
        ref_config::McpConfig::Local { command: vec!["true".into()], environment: Default::default(), cwd: None, enabled: false, timeout_ms: None },
    );
    let (hub, _work) = hub(cfg, "mcpstatus");
    let conn = hub.add_conn();
    conn.lock().hello = true;
    let r = ref_server::api::handle(&hub, &conn, ref_proto::Request::McpStatus { cwd: Some("/".into()) }).await.unwrap();
    assert_eq!(r["servers"], json!([{"name": "off", "status": "disabled", "error": null, "tools": []}]));
    let r = ref_server::api::handle(&hub, &conn, ref_proto::Request::GetConfig).await.unwrap();
    assert_eq!(r["config"]["mcp"], json!([["off", "disabled"]]));
}

#[tokio::test]
async fn context_window_tool() {
    let m = mock(Arc::new(|n, _| match n {
        0 => Reply::Text("a".into()),
        1 => Reply::Tools(vec![tool("c1", "context_window", json!({"purpose": "p"}))]),
        _ => Reply::Text("b".into()),
    }))
    .await;
    let (hub, work) = hub(config(&m.url), "ctx");
    let s = session(&hub, &work, "main", "build").await;
    prompt(&hub, &s, "1");
    wait_idle(&s).await;
    prompt(&hub, &s, "2");
    wait_idle(&s).await;
    let out = match &tool_parts(&s)[0] {
        Part::Tool { output: Some(o), .. } => o.clone(),
        _ => panic!(),
    };
    assert!(out.contains("Context window for \"main\": used 1020 tokens of 10000 (10%)"), "{out}");
    assert!(out.contains("Recommended compaction at 6000"), "{out}");
}

#[tokio::test]
async fn peer_send_message_between_sessions() {
    let m = mock(Arc::new(|_, body| {
        let all = body.to_string();
        let last_user = body["messages"].as_array().unwrap().iter().rev().find(|x| x["role"] == "user").cloned().unwrap();
        let lu = last_user["content"].to_string();
        if lu.contains("ping bob") && !all.contains("\"role\":\"tool\"") {
            Reply::Tools(vec![tool("c1", "send_message", json!({"purpose": "p", "to": "ボブ", "message": "hello bob"}))])
        } else if lu.contains("[Message from session") {
            Reply::Text("bob got it".into())
        } else {
            Reply::Text("ok".into())
        }
    }))
    .await;
    let (hub, work) = hub(config(&m.url), "peers");
    let alice = session(&hub, &work, "alice", "build").await;
    let bob = session(&hub, &work, "ボブ", "build").await;
    prompt(&hub, &alice, "ping bob");
    wait_idle(&alice).await;
    let b2 = bob.clone();
    wait_until("bob replied", T, move || last_text(&b2) == "bob got it").await;
    wait_idle(&bob).await;
    let bm = messages(&bob);
    assert!(matches!(&bm[0].source, MessageSource::Peer { from_name, .. } if from_name == "alice"));
    assert!(bm[0].raw_text().contains("hello bob"));
    assert!(bm[0].raw_text().contains(&format!("(id {})", alice.id)));
    let out = match &tool_parts(&alice)[0] {
        Part::Tool { output: Some(o), .. } => o.clone(),
        _ => panic!(),
    };
    assert!(out.starts_with("Delivered to \"ボブ\""), "{out}");
    // message_status: read.
    let msg_id = out.split("Message id ").nth(1).unwrap().split('.').next().unwrap().trim().to_string();
    let st = ref_server::peers::run(&hub, &alice, "message_status", &json!({"message_id": msg_id})).await;
    assert!(st.text.contains("read"), "{}", st.text);
    // list_sessions + read_session.
    let ls = ref_server::peers::run(&hub, &alice, "list_sessions", &json!({})).await;
    assert!(ls.text.starts_with("2 live session(s):"));
    assert!(ls.text.contains("\"alice\" (id") && ls.text.contains("(you)"));
    let rs = ref_server::peers::run(&hub, &alice, "read_session", &json!({"session": "ボブ", "page_size": 1})).await;
    assert!(rs.text.contains("messages 2–2 of 2 (page 1/2, newest first)"), "{}", rs.text);
    assert!(rs.text.contains("bob got it"));
    // Self target and unknown target errors.
    let e = ref_server::peers::run(&hub, &alice, "send_message", &json!({"to": "alice", "message": "x"})).await;
    assert!(e.is_error);
    let e = ref_server::peers::run(&hub, &alice, "send_message", &json!({"to": "carol", "message": "x"})).await;
    assert!(e.is_error && e.text.contains("no live session named"));
    // Case-insensitive resolution.
    assert_eq!(ref_server::peers::resolve(&hub, "ALICE", None).unwrap().id, alice.id);

    // User slash commands (no LLM turn).
    let cmd = |s, c: &str, a: &str| ref_server::peers::user_command(&hub, s, c, a);
    let p = cmd(&alice, "peers", "").unwrap();
    assert!(p.starts_with("You are \"alice\"") && p.contains("Other live sessions (1)") && p.contains("ボブ"), "{p}");
    let outbox = cmd(&alice, "peers-outbox", "").unwrap();
    assert!(outbox.contains("to \"ボブ\"") && outbox.contains("read") && outbox.contains("hello bob"), "{outbox}");
    let inbox = cmd(&bob, "peers-inbox", "").unwrap();
    assert!(inbox.contains("1 received") && inbox.contains("from \"alice\"") && inbox.contains("hello bob"), "{inbox}");
    assert!(!inbox.contains("[Message from session"), "{inbox}");
    assert!(cmd(&alice, "peers-inbox", "").unwrap().starts_with("Inbox is empty"));
    assert_eq!(cmd(&alice, "peers-name", "").unwrap(), "Current name: \"alice\"");
    assert!(cmd(&alice, "peers-name", "ボブ").is_err(), "duplicate name must be rejected");
    assert_eq!(cmd(&alice, "peers-name", "爱丽丝 🌸").unwrap(), "Renamed to \"爱丽丝 🌸\".");
    assert_eq!(alice.lock().info.name, "爱丽丝 🌸");
    assert!(cmd(&alice, "nope", "").is_err());
}

#[tokio::test]
async fn queued_message_delivered_after_current_step() {
    let m = mock(Arc::new(|n, _| match n {
        0 => Reply::Tools(vec![tool("c1", "bash", json!({"purpose": "p", "command": "sleep 0.5"}))]),
        _ => Reply::Text("ok".into()),
    }))
    .await;
    let (hub, work) = hub(config(&m.url), "queue");
    let s = session(&hub, &work, "main", "build").await;
    prompt(&hub, &s, "first");
    let s2 = s.clone();
    wait_until("running", T, move || tool_parts(&s2).iter().any(|p| matches!(p, Part::Tool { state: ToolState::Running, .. }))).await;
    prompt(&hub, &s, "second while busy");
    assert_eq!(s.lock().queue.len(), 1);
    wait_idle(&s).await;
    assert_eq!(m.count(), 2);
    let body = m.bodies()[1].to_string();
    assert!(body.contains("second while busy"));
    // Queued message appended after the tool results.
    let msgs = messages(&s);
    assert_eq!(msgs[2].raw_text(), "second while busy");
}

#[tokio::test]
async fn remove_queued_message_before_delivery() {
    let m = mock(Arc::new(|n, _| match n {
        0 => Reply::Tools(vec![tool("c1", "bash", json!({"purpose": "p", "command": "sleep 0.5"}))]),
        _ => Reply::Text("ok".into()),
    }))
    .await;
    let (hub, work) = hub(config(&m.url), "unqueue");
    let s = session(&hub, &work, "main", "build").await;
    let conn = hub.add_conn();
    conn.lock().hello = true;
    let req = |text: &str| ref_proto::Request::Prompt { session_id: s.id.clone(), text: text.into(), images: vec![] };
    let r = ref_server::api::handle(&hub, &conn, req("first")).await.unwrap();
    assert_eq!(r["queued"], false);
    assert!(r["message_id"].as_str().is_some_and(|id| !id.is_empty()));
    let s2 = s.clone();
    wait_until("running", T, move || tool_parts(&s2).iter().any(|p| matches!(p, Part::Tool { state: ToolState::Running, .. }))).await;
    let r = ref_server::api::handle(&hub, &conn, req("withdrawn while busy")).await.unwrap();
    assert_eq!(r["queued"], true);
    let id = r["message_id"].as_str().unwrap().to_string();
    assert_eq!(s.lock().queue.front().map(|m| m.id.clone()), Some(id.clone()));
    assert!(hub.store.load(&s.id).unwrap().messages.iter().any(|m| m.id == id), "queued prompt saved");
    // Attach snapshots list it (frontends rebuild their queued rows from this).
    let attach = ref_proto::Request::Attach { session_id: s.id.clone(), foreground: false, limit: None };
    let snap = ref_server::api::handle(&hub, &conn, attach.clone()).await.unwrap();
    assert_eq!(snap["snapshot"]["queued"][0]["id"], id.as_str());
    let remove = |id: &str| ref_proto::Request::RemoveQueued { session_id: s.id.clone(), message_id: id.into() };
    let r = ref_server::api::handle(&hub, &conn, remove(&id)).await.unwrap();
    assert_eq!(r["removed"], true);
    assert_eq!(r["text"], "withdrawn while busy");
    assert_eq!(r["images"], json!([]));
    assert!(s.lock().queue.is_empty());
    let snap = ref_server::api::handle(&hub, &conn, attach).await.unwrap();
    assert_eq!(snap["snapshot"]["queued"], json!([]));
    // Not persisted either (saves include the queue).
    assert!(!s.persisted().messages.iter().any(|m| m.id == id));
    assert!(!hub.store.load(&s.id).unwrap().messages.iter().any(|m| m.id == id));
    // Removing again: nothing left to remove.
    let r = ref_server::api::handle(&hub, &conn, remove(&id)).await.unwrap();
    assert_eq!(r["removed"], false);
    wait_idle(&s).await;
    // The run finishes its tool step and never sends the withdrawn prompt.
    assert_eq!(m.count(), 2);
    assert!(!m.bodies().iter().any(|b| b.to_string().contains("withdrawn while busy")));
    assert!(!messages(&s).iter().any(|m| m.id == id || m.raw_text().contains("withdrawn")));
    assert!(!hub.store.load(&s.id).unwrap().messages.iter().any(|m| m.id == id));
}

#[tokio::test]
async fn remove_queued_after_delivery_removes_nothing() {
    let m = mock(Arc::new(|n, _| match n {
        0 => Reply::Tools(vec![tool("c1", "bash", json!({"purpose": "p", "command": "sleep 0.3"}))]),
        _ => Reply::Text("ok".into()),
    }))
    .await;
    let (hub, work) = hub(config(&m.url), "unqueue-late");
    let s = session(&hub, &work, "main", "build").await;
    let conn = hub.add_conn();
    conn.lock().hello = true;
    prompt(&hub, &s, "first");
    let s2 = s.clone();
    wait_until("running", T, move || tool_parts(&s2).iter().any(|p| matches!(p, Part::Tool { state: ToolState::Running, .. }))).await;
    let r = ref_server::api::handle(&hub, &conn, ref_proto::Request::Prompt { session_id: s.id.clone(), text: "second".into(), images: vec![] })
        .await
        .unwrap();
    assert_eq!(r["queued"], true);
    let id = r["message_id"].as_str().unwrap().to_string();
    wait_idle(&s).await;
    // Delivered at the safe point after the tool step.
    assert!(messages(&s).iter().any(|m| m.id == id));
    let r = ref_server::api::handle(&hub, &conn, ref_proto::Request::RemoveQueued { session_id: s.id.clone(), message_id: id.clone() })
        .await
        .unwrap();
    assert_eq!(r["removed"], false);
    // The delivered message stays in the conversation.
    assert_eq!(messages(&s).iter().filter(|m| m.id == id).count(), 1);
    assert!(m.bodies()[1].to_string().contains("second"));
    // Unknown session: error.
    let e = ref_server::api::handle(&hub, &conn, ref_proto::Request::RemoveQueued { session_id: "nope".into(), message_id: id }).await;
    assert!(e.is_err());
}

#[tokio::test]
async fn busy_since_is_the_run_start_and_cleared_when_idle() {
    let m = mock(Arc::new(|n, _| match n {
        0 => Reply::Tools(vec![tool("c1", "bash", json!({"purpose": "p", "command": "sleep 0.3"}))]),
        _ => Reply::Text("ok".into()),
    }))
    .await;
    let (hub, work) = hub(config(&m.url), "busy-since");
    let s = session(&hub, &work, "main", "build").await;
    assert_eq!(s.info().busy_since, None);
    let before = ref_proto::now_ms();
    prompt(&hub, &s, "first");
    let started = s.info().busy_since.expect("set when the run starts");
    assert!(started >= before && started <= ref_proto::now_ms());
    let s2 = s.clone();
    wait_until("running", T, move || s2.info().status == SessionStatus::Running).await;
    // Kept across status changes within the run; visible in the live list; never persisted.
    assert_eq!(s.info().busy_since, Some(started));
    assert!(hub.infos().iter().any(|i| i.id == s.id && i.busy_since == Some(started)));
    assert_eq!(s.persisted().info.busy_since, None);
    wait_idle(&s).await;
    assert_eq!(s.info().busy_since, None);
}

#[tokio::test]
async fn create_session_must_report_and_close_rules() {
    let m = mock(Arc::new(|_, body| {
        let msgs = body["messages"].as_array().unwrap();
        let sys = msgs[0]["content"].as_str().unwrap_or("");
        let last_user = msgs.iter().rev().find(|x| x["role"] == "user").map(|x| x["content"].to_string()).unwrap_or_default();
        let has_tools = body["tools"].as_array().is_some_and(|t| t.iter().any(|t| t["function"]["name"] == "create_session"));
        if has_tools && last_user.contains("spawn a worker") && !body.to_string().contains("\"role\":\"tool\"") {
            assert!(sys.contains("Agents available for create_session:\n- build: Builder\n- worker: Worker"));
            return Reply::Tools(vec![tool(
                "c1",
                "create_session",
                json!({"purpose": "p", "name": "worker", "agent": "worker", "prompt": "do the work"}),
            )]);
        }
        if !has_tools && last_user.contains("[Reminder]") && !body.to_string().contains("\"role\":\"tool\"") {
            let parent = last_user.split("(to: \\\"").nth(1).unwrap().split("\\\"").next().unwrap().to_string();
            return Reply::Tools(vec![tool("r1", "send_message", json!({"purpose": "p", "to": parent, "message": "result: 42"}))]);
        }
        Reply::Text("ok".into())
    }))
    .await;
    let (hub, work) = hub(config(&m.url), "spawn");
    let parent = session(&hub, &work, "boss", "build").await;
    prompt(&hub, &parent, "spawn a worker");
    wait_idle(&parent).await;
    let h2 = hub.clone();
    wait_until("child exists", T, move || h2.sessions().len() == 2).await;
    let child = hub.sessions().into_iter().find(|x| x.id != parent.id).unwrap();
    // Child runs, stops without reporting, gets a reminder, then reports.
    let p2 = parent.clone();
    wait_until("parent got report", T, move || messages(&p2).iter().any(|m| m.raw_text().contains("result: 42"))).await;
    wait_idle(&child).await;
    let cm = messages(&child);
    assert!(matches!(&cm[0].source, MessageSource::Spawn { from_name, .. } if from_name == "boss"));
    assert!(cm[0].raw_text().contains("do the work"));
    assert!(cm[0].raw_text().contains("report the result to \"boss\""));
    assert!(cm.iter().any(|m| m.source == MessageSource::Reminder));
    {
        let info = child.info();
        assert_eq!(info.parent.as_deref(), Some(parent.id.as_str()));
        assert!(info.must_report);
        assert_eq!(info.agent, "worker");
        assert!(child.lock().reported);
    }
    // Worker agent can't spawn: no spawn tools sent for it.
    let worker_bodies: Vec<_> = m.bodies().into_iter().filter(|b| b["messages"][1]["content"].to_string().contains("[Session created by")).collect();
    assert!(!worker_bodies.is_empty());
    assert!(worker_bodies.iter().all(|b| !b["tools"].to_string().contains("create_session")));
    assert!(worker_bodies.iter().all(|b| !b["messages"][0]["content"].as_str().unwrap().contains("Agents available")));

    wait_idle(&parent).await;
    // close_session refuses foreground sessions.
    child.lock().info.attached = 1;
    let e = ref_server::peers::run(&hub, &parent, "close_session", &json!({"session": "worker"})).await;
    assert!(e.is_error);
    assert_eq!(e.text, "Cannot close \"worker\": it is a foreground session (a user is attached).");
    // Self close refused.
    let e = ref_server::peers::run(&hub, &parent, "close_session", &json!({"session": parent.id.clone()})).await;
    assert!(e.is_error);
    child.lock().info.attached = 0;
    let ok = ref_server::peers::run(&hub, &parent, "close_session", &json!({"session": "worker"})).await;
    assert!(!ok.is_error, "{}", ok.text);
    assert_eq!(hub.sessions().len(), 1);
    assert!(hub.store.exists(&child.id));
    // Name collision on create_session tool -> suffix.
    let r = ref_server::peers::run(&hub, &parent, "create_session", &json!({"name": "boss", "agent": "worker", "prompt": "x", "must_report": false})).await;
    assert!(r.text.contains("\"boss-2\""), "{}", r.text);
    let r = ref_server::peers::run(&hub, &parent, "create_session", &json!({"name": "x", "agent": "nope", "prompt": "x"})).await;
    assert!(r.is_error);
}

#[tokio::test]
async fn events_reach_attached_connection() {
    let m = mock(Arc::new(|_, _| Reply::Text("streamed".into()))).await;
    let (hub, work) = hub(config(&m.url), "events");
    let s = session(&hub, &work, "main", "build").await;
    let conn = hub.add_conn();
    {
        let mut c = conn.lock();
        c.hello = true;
        c.fg.insert(s.id.clone());
    }
    prompt(&hub, &s, "hi");
    wait_idle(&s).await;
    let frames = conn.out.drain();
    let kinds: Vec<String> = frames
        .iter()
        .filter_map(|f| match f {
            ref_proto::ServerFrame::Event { event, .. } => Some(match event {
                SessionEvent::Message { .. } => "message",
                SessionEvent::Part { .. } => "part",
                SessionEvent::Delta { .. } => "delta",
                SessionEvent::Info { .. } => "info",
                _ => "other",
            }),
            _ => None,
        })
        .map(String::from)
        .collect();
    assert!(kinds.contains(&"delta".to_string()), "{kinds:?}");
    assert!(kinds.iter().filter(|k| *k == "message").count() >= 2);
}

#[tokio::test]
async fn load_from_history_marks_unfinished_tools_cancelled() {
    let m = mock(Arc::new(|_, _| Reply::Text("x".into()))).await;
    let (hub, work) = hub(config(&m.url), "history");
    let s = session(&hub, &work, "main", "build").await;
    prompt(&hub, &s, "hi");
    wait_idle(&s).await;
    {
        let mut st = s.lock();
        let last = st.messages.last_mut().unwrap();
        last.parts.push(Part::Tool {
            call_id: "c9".into(),
            name: "bash".into(),
            purpose: String::new(),
            input: json!({}),
            raw_input: "{}".into(),
            state: ToolState::Running,
            output: None,
            meta: None,
            images: vec![],
            time: 0,
            time_end: None,
            run_at: None,
            provider_data: None,
        });
    }
    ref_server::ops::persist(&hub, &s);
    hub.remove_session(&s.id);
    let loaded = ref_server::ops::get_or_load(&hub, &s.id).await.unwrap();
    assert!(loaded.info().live);
    assert!(matches!(tool_parts(&loaded).last().unwrap(), Part::Tool { state: ToolState::Cancelled, .. }));
}

#[tokio::test]
async fn auto_compaction_when_threshold_reached() {
    let m = mock(Arc::new(|n, _| match n {
        0 => Reply::Text("first".into()),
        1 => Reply::Text("## Goal\n- auto summary".into()),
        _ => Reply::Text("second".into()),
    }))
    .await;
    let mut cfg = config(&m.url);
    cfg.compaction.threshold = 500.0;
    let (hub, work) = hub(cfg, "autocompact");
    let s = session(&hub, &work, "main", "build").await;
    prompt(&hub, &s, "one");
    wait_idle(&s).await;
    assert_eq!(s.info().context.auto, 500);
    prompt(&hub, &s, "two");
    wait_idle(&s).await;
    let msgs = messages(&s);
    // [one, first, summary, two, second]: the summary goes before the prompt being answered.
    assert_eq!(msgs.len(), 5, "{msgs:#?}");
    assert_eq!(msgs[2].source, MessageSource::Compaction);
    assert!(msgs[2].raw_text().contains("auto summary"));
    assert_eq!(msgs[3].raw_text(), "two");
    assert_eq!(last_text(&s), "second");
    let b = m.bodies();
    assert!(b[1].to_string().contains("Summarize the conversation above"));
    assert!(!b[1].to_string().contains("\"two\""));
    // Consecutive user messages are merged by the chat converter: summary + "two".
    let third = non_system(&b[2]);
    assert_eq!(third.len(), 1, "{third:#?}");
    let c = third[0]["content"].as_str().unwrap();
    assert!(c.starts_with("[Context compacted.") && c.contains("auto summary") && c.ends_with("two"), "{c}");
}

#[tokio::test]
async fn undo_last_prompt_or_from_given_message() {
    let m = mock(Arc::new(|_, _| Reply::Text("reply".into()))).await;
    let (hub, work) = hub(config(&m.url), "undo");
    let s = session(&hub, &work, "main", "build").await;
    for p in ["one", "two", "three"] {
        prompt(&hub, &s, p);
        wait_idle(&s).await;
    }
    assert_eq!(messages(&s).len(), 6);
    let conn = hub.add_conn();
    conn.lock().hello = true;
    // Default: the last human prompt and everything after it.
    let r = ref_server::api::handle(&hub, &conn, ref_proto::Request::Undo { session_id: s.id.clone(), message_id: None }).await.unwrap();
    assert_eq!(r["text"], "three");
    assert_eq!(messages(&s).len(), 4);
    // From a specific message: removes it and everything after.
    let first = messages(&s)[0].id.clone();
    let r = ref_server::api::handle(&hub, &conn, ref_proto::Request::Undo { session_id: s.id.clone(), message_id: Some(first.clone()) }).await.unwrap();
    assert_eq!(r["text"], "one");
    assert!(messages(&s).is_empty());
    assert!(hub.store.load(&s.id).unwrap().messages.is_empty());
    let e = ref_server::api::handle(&hub, &conn, ref_proto::Request::Undo { session_id: s.id.clone(), message_id: Some(first) }).await;
    assert!(e.is_err());
}

#[tokio::test]
async fn todo_tool_replaces_list_and_validates() {
    let m = mock(Arc::new(|n, _| match n {
        0 => Reply::Tools(vec![tool("t1", "todo", json!({"purpose": "p", "todos": [
            {"content": "Read code", "status": "completed"},
            {"content": "Fix bug", "status": "in_progress"},
            {"content": "Run tests", "status": "pending"}
        ]}))]),
        1 => Reply::Tools(vec![tool("t2", "todo", json!({"purpose": "p", "todos": [{"content": "x", "status": "bogus"}]}))]),
        _ => Reply::Text("done".into()),
    }))
    .await;
    let (hub, work) = hub(config(&m.url), "todo");
    let s = session(&hub, &work, "main", "build").await;
    prompt(&hub, &s, "go");
    wait_idle(&s).await;
    let todos = s.info().todos;
    assert_eq!(todos.len(), 3, "invalid update must not replace the list");
    assert_eq!(todos[1].content, "Fix bug");
    assert_eq!(todos[1].status, ref_proto::TodoStatus::InProgress);
    let parts = tool_parts(&s);
    match (&parts[0], &parts[1]) {
        (Part::Tool { output: Some(a), state: sa, .. }, Part::Tool { output: Some(b), state: sb, .. }) => {
            assert_eq!(*sa, ToolState::Done);
            assert!(a.starts_with("Todo list updated (1/3 completed):") && a.contains("[~] Fix bug"), "{a}");
            assert_eq!(*sb, ToolState::Error);
            assert!(b.contains("invalid"), "{b}");
        }
        _ => panic!(),
    }
    assert_eq!(last_text(&s), "done");
    // Persisted with the session info.
    assert_eq!(hub.store.load(&s.id).unwrap().info.todos.len(), 3);
}

#[tokio::test]
async fn set_cwd_switches_directory_and_tells_the_model() {
    let m = mock(Arc::new(|n, _| match n {
        0 => Reply::Hang("thinking".into()),
        _ => Reply::Text("ok".into()),
    }))
    .await;
    let (hub, work) = hub(config(&m.url), "cwd");
    std::fs::create_dir_all(work.join("sub dir")).unwrap();
    let s = session(&hub, &work, "main", "build").await;
    let conn = hub.add_conn();
    conn.lock().hello = true;
    let set = |path: &str| ref_proto::Request::SetCwd { session_id: s.id.clone(), path: path.into() };
    let old = s.info().cwd;

    // Busy: rejected, nothing changes.
    prompt(&hub, &s, "go");
    let s2 = s.clone();
    wait_until("partial text", T, move || last_text(&s2) == "thinking").await;
    let e = ref_server::api::handle(&hub, &conn, set("sub dir")).await.unwrap_err();
    assert!(e.contains("busy"), "{e}");
    assert_eq!(s.info().cwd, old);
    ref_server::engine::abort(&hub, &s, true);
    wait_idle(&s).await;
    let calls = m.count();

    // Relative to the current directory; a reminder is appended without an LLM turn.
    let r = ref_server::api::handle(&hub, &conn, set("sub dir")).await.unwrap();
    let new = std::fs::canonicalize(work.join("sub dir")).unwrap().to_string_lossy().to_string();
    assert_eq!(r["session"]["cwd"], json!(new));
    assert_eq!(s.info().cwd, new);
    let last = messages(&s).last().cloned().unwrap();
    assert_eq!(last.source, MessageSource::Reminder);
    assert!(last.raw_text().contains(&old) && last.raw_text().contains(&new), "{}", last.raw_text());
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(m.count(), calls, "no LLM turn");
    assert_eq!(s.info().status, SessionStatus::Idle);
    assert_eq!(hub.store.load(&s.id).unwrap().info.cwd, new, "persisted");

    // Same directory again: no second reminder. Missing directory: error.
    let n = messages(&s).len();
    ref_server::api::handle(&hub, &conn, set(".")).await.unwrap();
    assert_eq!(messages(&s).len(), n);
    let e = ref_server::api::handle(&hub, &conn, set("missing")).await.unwrap_err();
    assert!(e.contains("does not exist"), "{e}");
    assert!(ref_server::api::handle(&hub, &conn, set("  ")).await.is_err());

    // The next turn sees the new directory in the system prompt and the reminder.
    prompt(&hub, &s, "next");
    wait_idle(&s).await;
    let body = m.bodies().into_iter().rev().find(|b| b.get("tools").is_some()).unwrap();
    let all = body["messages"].to_string();
    assert!(body["messages"][0]["content"].to_string().contains(&new), "system prompt uses the new cwd");
    assert!(all.contains("changed the working directory"), "{all}");
}

#[tokio::test]
async fn queue_changes_reach_attached_connection() {
    let m = mock(Arc::new(|n, _| match n {
        0 => Reply::Hang("working".into()),
        _ => Reply::Text("done".into()),
    }))
    .await;
    let (hub, work) = hub(config(&m.url), "queue-ev");
    let s = session(&hub, &work, "main", "build").await;
    let peer = session(&hub, &work, "worker", "build").await;
    let conn = hub.add_conn();
    {
        let mut c = conn.lock();
        c.hello = true;
        c.fg.insert(s.id.clone());
    }
    prompt(&hub, &s, "go");
    let s2 = s.clone();
    wait_until("streaming", T, move || last_text(&s2) == "working").await;
    let queues = |conn: &Arc<ref_server::hub::Conn>| -> Vec<Vec<(String, MessageSource)>> {
        conn.out
            .drain()
            .into_iter()
            .filter_map(|f| match f {
                ref_proto::ServerFrame::Event { event: SessionEvent::Queue { messages }, .. } => Some(messages.iter().map(|m| (m.raw_text(), m.source.clone())).collect()),
                _ => None,
            })
            .collect()
    };
    let _ = queues(&conn);
    // A peer message and a human prompt while busy: each push is announced with the whole queue.
    let (mid, queued) = ref_server::peers::deliver(&hub, &peer, &s, "from the worker");
    assert!(queued);
    prompt(&hub, &s, "and me");
    let q = queues(&conn);
    assert_eq!(q.len(), 2, "{q:?}");
    assert_eq!(q[1].len(), 2);
    assert!(q[1][0].0.contains("from the worker") && matches!(&q[1][0].1, MessageSource::Peer { from_name, msg_id, .. } if from_name == "worker" && *msg_id == mid));
    assert_eq!(q[1][1], ("and me".to_string(), MessageSource::Human));
    // The snapshot lists every source too.
    let r = ref_server::api::handle(&hub, &conn, ref_proto::Request::Attach { session_id: s.id.clone(), foreground: true, limit: None }).await.unwrap();
    assert_eq!(r["snapshot"]["queued"].as_array().unwrap().len(), 2);
    // Abort: the queue is delivered at the end of the run and announced as empty.
    let _ = conn.out.drain();
    ref_server::engine::abort(&hub, &s, true);
    wait_idle(&s).await;
    let q = queues(&conn);
    assert!(q.last().is_some_and(|l| l.is_empty()), "{q:?}");
}

#[tokio::test]
async fn frontends_get_image_stubs_and_undo_returns_the_bytes() {
    let m = mock(Arc::new(|n, _| match n {
        0 => Reply::Tools(vec![tool("c1", "read", json!({"purpose": "p", "path": "shot.png"}))]),
        _ => Reply::Text("seen".into()),
    }))
    .await;
    let mut cfg = config(&m.url);
    cfg.providers.get_mut("mock").unwrap().models.get_mut("m").unwrap().images = true;
    let (hub, work) = hub(cfg, "image-stubs");
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend_from_slice(b"tool screenshot bytes");
    std::fs::write(work.join("shot.png"), &png).unwrap();
    let s = session(&hub, &work, "main", "build").await;
    let conn = hub.add_conn();
    {
        let mut c = conn.lock();
        c.hello = true;
        c.fg.insert(s.id.clone());
    }
    let pasted = ref_proto::ImageData { mime: "image/png".into(), data: "UGFzdGVkSW1hZ2VCeXRlcw==".into(), ..Default::default() };
    let req = ref_proto::Request::Prompt { session_id: s.id.clone(), text: "look [Image 1]".into(), images: vec![pasted.clone()] };
    ref_server::api::handle(&hub, &conn, req).await.unwrap();
    wait_idle(&s).await;
    // The session keeps the bytes (the model gets them) ...
    let tool_img = tool_parts(&s)
        .into_iter()
        .find_map(|p| match p {
            Part::Tool { images, .. } => images.into_iter().next(),
            _ => None,
        })
        .expect("read returned the image");
    assert!(!tool_img.data.is_empty());
    let bodies = m.bodies();
    assert!(bodies.iter().any(|b| b.to_string().contains(&pasted.data)));
    assert!(bodies.iter().any(|b| b.to_string().contains(&tool_img.data)));
    // ... but neither events nor the snapshot carry them to frontends.
    let events = serde_json::to_string(&conn.out.drain()).unwrap();
    assert!(events.contains("image/png"), "image stubs are still sent");
    assert!(!events.contains(&pasted.data) && !events.contains(&tool_img.data));
    let snap = ref_server::api::handle(&hub, &conn, ref_proto::Request::Attach { session_id: s.id.clone(), foreground: true, limit: None }).await.unwrap();
    // Stubs carry an id (and the decoded size); GetImage returns the bytes.
    let msgs: Vec<ref_proto::Message> = serde_json::from_value(snap["snapshot"]["messages"].clone()).unwrap();
    let stubs: Vec<ref_proto::ImageData> = msgs
        .iter()
        .flat_map(|m| m.parts.iter())
        .flat_map(|p| match p {
            Part::Image { image, .. } => vec![image.clone()],
            Part::Tool { images, .. } => images.clone(),
            _ => vec![],
        })
        .collect();
    assert_eq!(stubs.len(), 2);
    assert!(stubs.iter().all(|i| i.is_stub() && i.id.is_some()));
    assert_eq!(stubs[0].size, Some(16), "\"PastedImageBytes\"");
    assert_eq!(stubs[1].size, Some(png.len() as u64));
    let get = |id: &str| ref_proto::Request::GetImage { session_id: s.id.clone(), image_id: id.into() };
    for (stub, data) in stubs.iter().zip([&pasted.data, &tool_img.data]) {
        let r = ref_server::api::handle(&hub, &conn, get(stub.id.as_deref().unwrap())).await.unwrap();
        assert_eq!(r["image"]["data"], data.as_str());
        assert_eq!(r["image"]["mime"], "image/png");
    }
    assert!(ref_server::api::handle(&hub, &conn, get("nonsense")).await.is_err());
    assert!(ref_server::api::handle(&hub, &conn, get(&format!("{}:0:5", msgs[0].id))).await.is_err());
    let snap = snap.to_string();
    assert!(snap.contains("image/png") && !snap.contains(&pasted.data) && !snap.contains(&tool_img.data));
    assert!(hub.store.load(&s.id).unwrap().messages.iter().any(|m| serde_json::to_string(m).unwrap().contains(&pasted.data)), "saved with bytes");
    // Session closed: the image is read from the store.
    ref_server::ops::close_session(&hub, &s).await;
    assert!(hub.session(&s.id).is_none());
    let r = ref_server::api::handle(&hub, &conn, get(stubs[1].id.as_deref().unwrap())).await.unwrap();
    assert_eq!(r["image"]["data"], tool_img.data.as_str());
    let r = ref_server::api::handle(&hub, &conn, ref_proto::Request::Attach { session_id: s.id.clone(), foreground: true, limit: None }).await.unwrap();
    assert_eq!(r["snapshot"]["info"]["id"], s.id.as_str());
    let s = hub.session(&s.id).expect("live again");
    // A stub sent back as a prompt image is rejected.
    let stub = ref_proto::ImageData { mime: "image/png".into(), data: String::new(), ..Default::default() };
    let e = ref_server::api::handle(&hub, &conn, ref_proto::Request::Prompt { session_id: s.id.clone(), text: "x".into(), images: vec![stub] }).await;
    assert!(e.is_err());
    // Undo returns the prompt with its image bytes.
    let r = ref_server::api::handle(&hub, &conn, ref_proto::Request::Undo { session_id: s.id.clone(), message_id: None }).await.unwrap();
    assert_eq!(r["text"], "look [Image 1]");
    assert_eq!(r["images"], json!([{"mime": "image/png", "data": pasted.data}]));
}

#[tokio::test]
async fn history_is_served_in_pages() {
    let m = mock(Arc::new(|_, _| Reply::Text("ok".into()))).await;
    let (hub, work) = hub(config(&m.url), "pages");
    let s = session(&hub, &work, "main", "build").await;
    let conn = hub.add_conn();
    conn.lock().hello = true;
    {
        let mut st = s.lock();
        for i in 0..250 {
            st.messages.push(ref_server::session::user_message(MessageSource::Human, format!("p{i}"), vec![]));
        }
    }
    let texts = |v: &serde_json::Value| -> Vec<String> {
        let msgs: Vec<ref_proto::Message> = serde_json::from_value(v.clone()).unwrap();
        msgs.iter().map(|m| m.raw_text()).collect()
    };
    let attach = |limit| ref_proto::Request::Attach { session_id: s.id.clone(), foreground: true, limit };
    // Newest page first (default 100).
    let r = ref_server::api::handle(&hub, &conn, attach(None)).await.unwrap();
    let page = texts(&r["snapshot"]["messages"]);
    assert_eq!((page.len(), page[0].as_str(), page[99].as_str()), (100, "p150", "p249"));
    assert_eq!(r["snapshot"]["more_before"], true);
    // Older pages before a given message, down to the start.
    let first_id = |msgs: &serde_json::Value| msgs[0]["id"].as_str().unwrap().to_string();
    let older = |before: String, limit| ref_proto::Request::Messages { session_id: s.id.clone(), before, limit };
    let r2 = ref_server::api::handle(&hub, &conn, older(first_id(&r["snapshot"]["messages"]), None)).await.unwrap();
    let page = texts(&r2["messages"]);
    assert_eq!((page.len(), page[0].as_str(), page[99].as_str()), (100, "p50", "p149"));
    assert_eq!(r2["more_before"], true);
    let r3 = ref_server::api::handle(&hub, &conn, older(first_id(&r2["messages"]), Some(500))).await.unwrap();
    assert_eq!(texts(&r3["messages"]).len(), 50);
    assert_eq!(r3["more_before"], false);
    // Limits are clamped; unknown anchors are errors.
    let r = ref_server::api::handle(&hub, &conn, attach(Some(0))).await.unwrap();
    assert_eq!(texts(&r["snapshot"]["messages"]), ["p249"]);
    assert!(ref_server::api::handle(&hub, &conn, older("nope".into(), None)).await.is_err());
    // Large messages: the page stops at about 1 MiB.
    {
        let mut st = s.lock();
        for i in 0..20 {
            st.messages.push(ref_server::session::user_message(MessageSource::Human, format!("big{i} {}", "x".repeat(200_000)), vec![]));
        }
    }
    let r = ref_server::api::handle(&hub, &conn, attach(None)).await.unwrap();
    let n = r["snapshot"]["messages"].as_array().unwrap().len();
    assert!((5..=7).contains(&n), "{n}");
    // Reset events (undo) carry the newest page only.
    let _ = conn.out.drain();
    ref_server::api::handle(&hub, &conn, ref_proto::Request::Undo { session_id: s.id.clone(), message_id: None }).await.unwrap();
    let reset = conn.out.drain().into_iter().find_map(|f| match f {
        ref_proto::ServerFrame::Event { event: SessionEvent::Reset { messages, more_before }, .. } => Some((messages.len(), more_before)),
        _ => None,
    });
    let (len, more) = reset.expect("reset event");
    assert!(len < 269 && more, "{len}");
}
