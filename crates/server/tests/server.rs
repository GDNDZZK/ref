mod common;

use common::*;
use hn_proto::*;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Model output: text, then (after the first) reply with a tool result echo.
#[tokio::test]
async fn simple_text_turn_and_persistence() {
    let mock = Mock::start(Arc::new(|_r: &Req| text("Hello from the mock"))).await;
    let env = Env::new(mock.port, json!({}));
    let srv = env.spawn(&["--idle-timeout", "-1"]);
    let mut c = Client::connect(&srv.url(), &env.work()).await;
    let info = c.create(&env.work(), None).await;
    assert!(info.name.starts_with("session-"));
    assert_eq!(info.attached, 1);
    c.prompt(&info.id, "say hi").await;
    c.wait_idle(&info.id, T).await;
    let (info2, msgs) = c.snapshot(&info.id).await;
    assert_eq!(msgs.len(), 2, "{msgs:#?}");
    assert_eq!(msgs[0].raw_text(), "say hi");
    assert_eq!(last_assistant_text(&msgs), "Hello from the mock");
    assert_eq!(msgs[1].finish.as_deref(), Some("stop"));
    assert_eq!(info2.context.used, 110);
    assert_eq!(info2.context.max, 100000);
    // System prompt: minimal base, env block, session id; tools include purpose.
    let reqs = mock.requests();
    let r = Req { body: reqs[0].clone(), n: 0 };
    let sys = r.system();
    assert!(sys.contains("running in Hernees"), "{sys}");
    assert!(sys.contains(&format!("Working directory: {}", env.work())), "{sys}");
    assert!(sys.contains(&info.id));
    let tools = r.body["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["function"]["name"].as_str().unwrap()).collect();
    for n in ["read", "bash", "list_sessions", "send_message", "context_info", "compact_context", "create_session"] {
        assert!(names.contains(&n), "{names:?}");
    }
    for t in tools {
        let p = &t["function"]["parameters"];
        assert_eq!(p["properties"].as_object().unwrap().keys().next().unwrap(), "purpose");
        assert_eq!(p["required"][0], "purpose");
    }
    // Title generated in the background (mock answers "Mock title").
    c.wait_for("title", T, |m| matches!(m, ServerMsg::Sessions { sessions } if sessions.iter().any(|s| s.name == "Mock title")))
        .await;
    // Persisted after debounce.
    tokio::time::sleep(Duration::from_millis(800)).await;
    let dir = env.path("data/sessions").join(&info.id);
    let saved: Vec<Message> = serde_json::from_str(&std::fs::read_to_string(dir.join("messages.json")).unwrap()).unwrap();
    assert_eq!(saved.len(), 2);
    let hist = c.req(ClientMsg::ListSessions { scope: ListScope::History }).await.unwrap();
    assert_eq!(hist.as_array().unwrap().len(), 1);
    // Export.
    let md = c.req(ClientMsg::Export { session: info.id.clone() }).await.unwrap();
    assert!(md.as_str().unwrap().contains("Hello from the mock"));
    // Undo removes the human message and the answer.
    let undone = c.req(ClientMsg::Undo { session: info.id.clone() }).await.unwrap();
    assert_eq!(undone, "say hi");
    let (_, msgs) = c.snapshot(&info.id).await;
    assert!(msgs.is_empty());
    assert_eq!(c.req(ClientMsg::Ping).await.unwrap(), "pong");
}

#[tokio::test]
async fn tool_round_trip_and_errors_do_not_stop_loop() {
    let script: Script = Arc::new(|r: &Req| match r.n {
        0 => tool("bash", json!({"purpose": "print greeting", "command": "echo hi-from-bash"})),
        // Unknown tool + invalid JSON + a tool error in one response.
        1 => Reply::Tools(vec![
            ("no_such_tool".into(), json!({"purpose": "x"}).to_string()),
            ("bash".into(), "{not json".into()),
            ("read".into(), json!({"purpose": "missing", "path": "does-not-exist.txt"}).to_string()),
        ]),
        _ => text("all done"),
    });
    let mock = Mock::start(script).await;
    let env = Env::new(mock.port, json!({}));
    let srv = env.spawn(&[]);
    let mut c = Client::connect(&srv.url(), &env.work()).await;
    let info = c.create(&env.work(), Some("tools")).await;
    c.prompt(&info.id, "run it").await;
    c.wait_idle(&info.id, T).await;
    let (_, msgs) = c.snapshot(&info.id).await;
    let tools = tool_parts(&msgs);
    assert_eq!(tools.len(), 4, "{tools:#?}");
    assert_eq!(tools[0].state, ToolState::Done);
    assert_eq!(tools[0].purpose, "print greeting");
    assert!(tools[0].output.contains("hi-from-bash"));
    assert_eq!(tools[1].state, ToolState::Error);
    assert!(tools[1].output.contains("Unknown tool"));
    assert_eq!(tools[2].state, ToolState::Error);
    assert!(tools[2].output.starts_with("Invalid JSON arguments"), "{}", tools[2].output);
    assert_eq!(tools[3].state, ToolState::Error);
    assert_eq!(last_assistant_text(&msgs), "all done");
    // The second request carried the bash result, without the purpose in the replayed args... (raw kept)
    let reqs = mock.requests();
    assert_eq!(reqs.len(), 3);
    let second = Req { body: reqs[1].clone(), n: 1 };
    assert_eq!(second.last_role(), "tool");
    assert!(second.last_text().contains("hi-from-bash"));
    let third = Req { body: reqs[2].clone(), n: 2 };
    let tool_msgs: Vec<&Value> = third.messages().iter().filter(|m| m["role"] == "tool").collect();
    assert_eq!(tool_msgs.len(), 4);
    // Live output delta was sent for bash.
    assert!(c.seen.iter().any(|m| matches!(m, ServerMsg::Event { event: Event::Delta { field: DeltaField::ToolOutput, .. }, .. })));
    // ToolInput deltas streamed.
    assert!(c.seen.iter().any(|m| matches!(m, ServerMsg::Event { event: Event::Delta { field: DeltaField::ToolInput, .. }, .. })));
}

#[tokio::test]
async fn retries_on_500_then_succeeds() {
    let script: Script = Arc::new(|r: &Req| if r.n < 2 { Reply::Status(500, "boom".into()) } else { text("recovered") });
    let mock = Mock::start(script).await;
    let env = Env::new(mock.port, json!({}));
    let srv = env.spawn(&[]);
    let mut c = Client::connect(&srv.url(), &env.work()).await;
    let info = c.create(&env.work(), Some("retry")).await;
    c.prompt(&info.id, "go").await;
    c.wait_for("retry status", T, |m| {
        matches!(m, ServerMsg::Event { event: Event::Info { info }, .. } if matches!(info.status, SessionStatus::Retry { attempt: 1, .. }))
    })
    .await;
    c.wait_idle(&info.id, T).await;
    let (_, msgs) = c.snapshot(&info.id).await;
    assert_eq!(msgs.len(), 2, "failed attempts must be discarded: {msgs:#?}");
    assert_eq!(last_assistant_text(&msgs), "recovered");
    assert!(c.seen.iter().any(|m| matches!(m, ServerMsg::Event { event: Event::Removed { .. }, .. })));
}

#[tokio::test]
async fn fatal_error_ends_turn_with_error() {
    let mock = Mock::start(Arc::new(|_r: &Req| Reply::Status(401, "bad key".into()))).await;
    let env = Env::new(mock.port, json!({}));
    let srv = env.spawn(&[]);
    let mut c = Client::connect(&srv.url(), &env.work()).await;
    let info = c.create(&env.work(), Some("fatal")).await;
    c.prompt(&info.id, "go").await;
    c.wait_idle(&info.id, T).await;
    let (_, msgs) = c.snapshot(&info.id).await;
    let last = msgs.last().unwrap();
    assert!(last.error.as_deref().unwrap_or("").contains("401"), "{last:#?}");
    assert!(c.seen.iter().any(|m| matches!(m, ServerMsg::Toast { level: ToastLevel::Error, .. })));
}

#[tokio::test]
async fn permission_ask_once_reject_and_deny() {
    let script: Script = Arc::new(|r: &Req| match r.n {
        0 => tool("bash", json!({"purpose": "touch", "command": "echo asked > asked.txt"})),
        1 => tool("bash", json!({"purpose": "again", "command": "echo nope > nope.txt"})),
        2 => tool("delete", json!({"purpose": "rm", "path": "x"})),
        _ => text("ok"),
    });
    let mock = Mock::start(script).await;
    let env = Env::new(mock.port, json!({"permission": {"bash": "ask", "delete": "deny"}}));
    let srv = env.spawn(&[]);
    let mut c = Client::connect(&srv.url(), &env.work()).await;
    let info = c.create(&env.work(), Some("perm")).await;
    c.prompt(&info.id, "go").await;
    for decision in [PermissionDecision::Once, PermissionDecision::Reject] {
        let m = c
            .wait_for("permission part", T, |m| {
                matches!(m, ServerMsg::Event { event: Event::Part { part: Part::Tool(tp), .. }, .. } if tp.state == ToolState::Permission)
            })
            .await;
        let ServerMsg::Event { event: Event::Part { part: Part::Tool(tp), .. }, .. } = m else { unreachable!() };
        assert!(tp.input["command"].as_str().is_some());
        c.wait_for("permission status", T, |m| {
            matches!(m, ServerMsg::Event { event: Event::Info { info }, .. } if info.status == SessionStatus::Permission)
        })
        .await;
        c.req(ClientMsg::Permission { session: info.id.clone(), call_id: tp.call_id.clone(), decision }).await.unwrap();
    }
    c.wait_idle(&info.id, T).await;
    let (_, msgs) = c.snapshot(&info.id).await;
    let tools = tool_parts(&msgs);
    assert_eq!(tools[0].state, ToolState::Done);
    assert!(std::path::Path::new(&env.work()).join("asked.txt").exists());
    assert_eq!(tools[1].state, ToolState::Denied);
    assert_eq!(tools[1].output, "The user rejected this tool call.");
    assert!(!std::path::Path::new(&env.work()).join("nope.txt").exists());
    assert_eq!(tools[2].state, ToolState::Denied);
    assert!(tools[2].output.starts_with("Permission denied"));
}

#[tokio::test]
async fn abort_cancels_delayed_tool() {
    let script: Script = Arc::new(|r: &Req| match r.n {
        0 => tool("bash", json!({"purpose": "slow", "command": "echo x > delayed.txt"})),
        _ => text("after"),
    });
    let mock = Mock::start(script).await;
    let env = Env::new(mock.port, json!({"delay": {"bash": 30}}));
    let srv = env.spawn(&[]);
    let mut c = Client::connect(&srv.url(), &env.work()).await;
    let info = c.create(&env.work(), Some("abort")).await;
    c.prompt(&info.id, "go").await;
    c.wait_for("delayed", T, |m| {
        matches!(m, ServerMsg::Event { event: Event::Part { part: Part::Tool(tp), .. }, .. } if tp.state == ToolState::Delayed && tp.run_at.is_some())
    })
    .await;
    c.req(ClientMsg::Abort { session: info.id.clone() }).await.unwrap();
    c.wait_idle(&info.id, T).await;
    let (i, msgs) = c.snapshot(&info.id).await;
    assert_eq!(i.status, SessionStatus::Idle);
    let tools = tool_parts(&msgs);
    assert_eq!(tools[0].state, ToolState::Cancelled);
    assert!(!std::path::Path::new(&env.work()).join("delayed.txt").exists());
    assert_eq!(mock.requests().len(), 1);
}

#[tokio::test]
async fn compact_context_drops_old_history() {
    let script: Script = Arc::new(|r: &Req| match r.n {
        0 => text("first answer SECRET-OLD"),
        1 => tool("compact_context", json!({"purpose": "save room", "summary": "User wants X; done Y."})),
        _ => text("continuing"),
    });
    let mock = Mock::start(script).await;
    let env = Env::new(mock.port, json!({}));
    let srv = env.spawn(&[]);
    let mut c = Client::connect(&srv.url(), &env.work()).await;
    let info = c.create(&env.work(), Some("compact")).await;
    c.prompt(&info.id, "first request OLD-PROMPT").await;
    c.wait_idle(&info.id, T).await;
    c.prompt(&info.id, "second").await;
    c.wait_idle(&info.id, T).await;
    let reqs = mock.requests();
    assert_eq!(reqs.len(), 3);
    let last = Req { body: reqs[2].clone(), n: 2 };
    let all = last.all_text();
    assert!(!all.contains("SECRET-OLD"), "{all}");
    assert!(!all.contains("OLD-PROMPT"), "{all}");
    assert!(all.contains("User wants X; done Y."));
    assert!(last.last_user().contains("<conversation-checkpoint>"));
    let (_, msgs) = c.snapshot(&info.id).await;
    assert!(msgs.iter().any(|m| m.origin == Origin::Compaction));
    assert!(msgs.iter().any(|m| m.raw_text().contains("SECRET-OLD")), "UI keeps full history");
}

#[tokio::test]
async fn manual_compact_uses_llm_summary() {
    let script: Script = Arc::new(|r: &Req| {
        if r.last_user().contains("You MUST summarize the conversation") {
            text("## Objective\n- summarized")
        } else {
            text("plain answer")
        }
    });
    let mock = Mock::start(script).await;
    let env = Env::new(mock.port, json!({}));
    let srv = env.spawn(&[]);
    let mut c = Client::connect(&srv.url(), &env.work()).await;
    let info = c.create(&env.work(), Some("manual")).await;
    c.prompt(&info.id, "hello").await;
    c.wait_idle(&info.id, T).await;
    c.req(ClientMsg::Compact { session: info.id.clone() }).await.unwrap();
    c.wait_for("checkpoint", T, |m| {
        matches!(m, ServerMsg::Event { event: Event::Message { message }, .. } if message.origin == Origin::Compaction)
    })
    .await;
    let (_, msgs) = c.snapshot(&info.id).await;
    let cp = msgs.iter().find(|m| m.origin == Origin::Compaction).unwrap();
    assert!(cp.raw_text().contains("## Objective\n- summarized"));
    // The summary request extends the previous request (same system + tools) so it is
    // served from the prompt cache.
    let reqs = mock.requests();
    let i = reqs.iter().position(|b| b.to_string().contains("You MUST summarize")).unwrap();
    let (prev, sum) = (&reqs[i - 1], &reqs[i]);
    assert_eq!(prev["tools"], sum["tools"]);
    let (pm, sm) = (prev["messages"].as_array().unwrap(), sum["messages"].as_array().unwrap());
    assert_eq!(&sm[..pm.len()], &pm[..]);
}

#[tokio::test]
async fn peers_send_message_wakes_idle_session() {
    // Session A sends to B by name; B answers with text.
    let script: Script = Arc::new(|r: &Req| {
        let last = r.last_user();
        if last.contains("[message from session") && r.last_role() == "user" {
            return text("B got it");
        }
        if r.last_role() == "user" && last.contains("ping B") {
            return tool("send_message", json!({"purpose": "notify", "to": "beta", "message": "hello beta"}));
        }
        text("done")
    });
    let mock = Mock::start(script).await;
    let env = Env::new(mock.port, json!({}));
    let srv = env.spawn(&[]);
    let mut c = Client::connect(&srv.url(), &env.work()).await;
    let b = c.create(&env.work(), Some("beta")).await;
    let a = c.create(&env.work(), Some("alpha")).await;
    c.prompt(&a.id, "ping B").await;
    c.wait_idle(&a.id, T).await;
    let (_, amsgs) = c.snapshot(&a.id).await;
    let t = &tool_parts(&amsgs)[0];
    assert!(t.output.starts_with("Delivered to \"beta\""), "{}", t.output);
    assert!(t.output.contains("it was idle and is now processing it"));
    // B processes the message.
    let mut ok = false;
    for _ in 0..100 {
        let (bi, bmsgs) = c.snapshot(&b.id).await;
        if bi.status == SessionStatus::Idle && last_assistant_text(&bmsgs) == "B got it" {
            let peer = &bmsgs[0];
            assert!(matches!(&peer.origin, Origin::Peer { from_name, .. } if from_name == "alpha"));
            assert!(peer.raw_text().contains("hello beta"));
            ok = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(ok, "beta never answered");
}

#[tokio::test]
async fn create_session_must_report_and_close_rules() {
    let reminded = Arc::new(AtomicUsize::new(0));
    let r2 = reminded.clone();
    let script: Script = Arc::new(move |r: &Req| {
        let last = r.last_user();
        let lr = r.last_role();
        if lr == "user" && last.contains("spawn a worker") {
            return tool(
                "create_session",
                json!({"purpose": "delegate", "name": "worker", "agent": "build", "prompt": "compute 2+2", "must_report": true}),
            );
        }
        if lr == "user" && last.contains("[task from session") {
            return text("I think it is 4"); // forgets to report
        }
        if lr == "user" && last.contains("have not sent it a message yet") {
            r2.fetch_add(1, Ordering::SeqCst);
            let parent = regex::Regex::new(r"\(id (ses_[a-z0-9]+)\)").unwrap().captures(&last).unwrap()[1].to_string();
            return tool("send_message", json!({"purpose": "report", "to": parent, "message": "result: 4"}));
        }
        if lr == "user" && last.contains("close the worker") {
            return tool("close_session", json!({"purpose": "cleanup", "session": "worker"}));
        }
        if lr == "user" && last.contains("close the fg") {
            return tool("close_session", json!({"purpose": "cleanup", "session": "fg"}));
        }
        text("ok")
    });
    let mock = Mock::start(script).await;
    let env = Env::new(mock.port, json!({}));
    let srv = env.spawn(&[]);
    let mut c = Client::connect(&srv.url(), &env.work()).await;
    let parent = c.create(&env.work(), Some("boss")).await;
    let _fg = c.create(&env.work(), Some("fg")).await;
    c.prompt(&parent.id, "spawn a worker").await;
    // The parent receives the report as a peer message.
    let mut got = false;
    for _ in 0..150 {
        let (_, msgs) = c.snapshot(&parent.id).await;
        if msgs.iter().any(|m| matches!(&m.origin, Origin::Peer { from_name, .. } if from_name == "worker") && m.raw_text().contains("result: 4")) {
            got = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(got, "parent never got the report");
    assert_eq!(reminded.load(Ordering::SeqCst), 1);
    let open: Vec<SessionInfo> = serde_json::from_value(c.req(ClientMsg::ListSessions { scope: ListScope::Open }).await.unwrap()).unwrap();
    let worker = open.iter().find(|s| s.name == "worker").expect("worker open");
    assert_eq!(worker.parent.as_deref(), Some(parent.id.as_str()));
    assert!(worker.must_report);
    assert_eq!(worker.attached, 0);
    // Wait for the parent to go idle, then try closing the fg session (refused) and the worker.
    for _ in 0..100 {
        if c.snapshot(&parent.id).await.0.status == SessionStatus::Idle {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    c.prompt(&parent.id, "close the fg").await;
    c.wait_idle(&parent.id, T).await;
    let (_, msgs) = c.snapshot(&parent.id).await;
    let t = tool_parts(&msgs).into_iter().find(|t| t.name == "close_session").unwrap();
    assert!(t.output.contains("is a foreground session"), "{}", t.output);
    c.prompt(&parent.id, "close the worker").await;
    c.wait_idle(&parent.id, T).await;
    let open: Vec<SessionInfo> = serde_json::from_value(c.req(ClientMsg::ListSessions { scope: ListScope::Open }).await.unwrap()).unwrap();
    assert!(!open.iter().any(|s| s.name == "worker"), "{open:#?}");
}

#[tokio::test]
async fn mid_turn_prompt_is_injected_at_step_boundary() {
    let script: Script = Arc::new(|r: &Req| {
        if r.n == 0 {
            return tool("bash", json!({"purpose": "wait", "command": "sleep 1; echo slept"}));
        }
        if r.all_text().contains("FOLLOWUP") && r.last_role() == "user" {
            return text("saw followup");
        }
        text("no followup")
    });
    let mock = Mock::start(script).await;
    let env = Env::new(mock.port, json!({}));
    let srv = env.spawn(&[]);
    let mut c = Client::connect(&srv.url(), &env.work()).await;
    let info = c.create(&env.work(), Some("queue")).await;
    c.prompt(&info.id, "start").await;
    c.wait_for("running", T, |m| {
        matches!(m, ServerMsg::Event { event: Event::Part { part: Part::Tool(tp), .. }, .. } if tp.state == ToolState::Running)
    })
    .await;
    let r = c.req(ClientMsg::Prompt { session: info.id.clone(), text: "FOLLOWUP".into(), images: vec![] }).await.unwrap();
    assert_eq!(r["queued"], true);
    c.wait_idle(&info.id, T).await;
    let (_, msgs) = c.snapshot(&info.id).await;
    assert_eq!(last_assistant_text(&msgs), "saw followup");
    assert_eq!(mock.requests().len(), 2, "follow-up joins the running turn");
}
