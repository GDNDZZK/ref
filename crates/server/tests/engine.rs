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

#[tokio::test]
async fn compact_context_tool_replaces_history() {
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
    assert_eq!(msgs.len(), 2, "{msgs:#?}");
    assert_eq!(msgs[0].source, MessageSource::Compaction);
    assert!(msgs[0].raw_text().contains("The user wants X. Done Y."));
    let bodies = m.bodies();
    let second: Vec<_> = bodies[1]["messages"].as_array().unwrap().iter().filter(|x| x["role"] != "system").cloned().collect();
    assert_eq!(second.len(), 1);
    assert!(second[0]["content"].as_str().unwrap().starts_with("[Context compacted."));
}

#[tokio::test]
async fn manual_compaction_uses_model_summary() {
    let m = mock(Arc::new(|n, _| match n {
        0 => Reply::Text("first answer".into()),
        _ => Reply::Text("## Goal\n- summary text".into()),
    }))
    .await;
    let (hub, work) = hub(config(&m.url), "manualcompact");
    let s = session(&hub, &work, "main", "build").await;
    prompt(&hub, &s, "q");
    wait_idle(&s).await;
    ref_server::engine::start_custom_run(&hub, &s, ref_server::compaction::manual).unwrap();
    wait_idle(&s).await;
    let msgs = messages(&s);
    assert_eq!(msgs.len(), 1);
    assert!(msgs[0].raw_text().contains("summary text"));
    assert!(m.bodies()[1].get("tools").is_none());
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
    assert_eq!(msgs.len(), 3, "{msgs:#?}");
    assert_eq!(msgs[0].source, MessageSource::Compaction);
    assert!(msgs[0].raw_text().contains("auto summary"));
    assert_eq!(msgs[1].raw_text(), "two");
    assert_eq!(last_text(&s), "second");
    let b = m.bodies();
    assert!(b[1].to_string().contains("Summarize the conversation above"));
    assert!(!b[1].to_string().contains("\"two\""));
}
