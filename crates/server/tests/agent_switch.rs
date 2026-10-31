mod common;

use common::*;
use ref_proto::{MessageSource, Request, Role, ServerFrame, SessionEvent, ToolState};
use ref_server::api;
use ref_server::hub::{Conn, Hub};
use ref_server::session::Session;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

const MARKER: &str = "The user switched the agent from";

struct Rig {
    hub: Arc<Hub>,
    conn: Arc<Conn>,
    session: Arc<Session>,
    mock: Mock,
}

async fn rig(handler: Handler) -> Rig {
    let mock = mock(handler).await;
    let mut cfg = config(&mock.url);
    cfg.agents.insert(
        "review".into(),
        ref_config::AgentConfig {
            name: "review".into(),
            description: "Reviewer".into(),
            ..Default::default()
        },
    );
    let (hub, work) = hub(cfg, "agent-switch");
    let session = session(&hub, &work, "main", "build").await;
    let conn = hub.add_conn();
    conn.lock().hello = true;
    let attach = Request::Attach {
        session_id: session.id.clone(),
        foreground: true,
        limit: None,
    };
    api::handle(&hub, &conn, attach).await.unwrap();
    conn.out.drain();
    Rig {
        hub,
        conn,
        session,
        mock,
    }
}

/// A rig whose session already has one answered prompt (a reminder is only written then).
async fn rig_with_history(handler: Handler) -> Rig {
    let rig = rig(handler).await;
    prompt(&rig.hub, &rig.session, "first");
    wait_idle(&rig.session).await;
    rig.events();
    rig
}

impl Rig {
    fn reminder_id(&self) -> String {
        messages(&self.session)
            .iter()
            .find(|m| m.source == MessageSource::Reminder)
            .map(|m| m.id.clone())
            .expect("a reminder")
    }

    async fn switch(&self, agent: &str) {
        let req = Request::SetAgent {
            session_id: self.session.id.clone(),
            agent: agent.into(),
        };
        api::handle(&self.hub, &self.conn, req).await.unwrap();
    }

    /// Texts of the reminders about agent switches, in conversation order.
    fn notes(&self) -> Vec<String> {
        messages(&self.session)
            .iter()
            .filter(|m| m.source == MessageSource::Reminder)
            .map(|m| m.raw_text())
            .collect()
    }

    fn events(&self) -> Vec<SessionEvent> {
        self.conn
            .out
            .drain()
            .into_iter()
            .filter_map(|f| match f {
                ServerFrame::Event { event, .. } => Some(event),
                _ => None,
            })
            .collect()
    }

    /// Waits until the bash tool of the first round is running.
    async fn wait_running(&self) {
        let s = self.session.clone();
        wait_until("tool running", Duration::from_secs(10), move || {
            tool_parts(&s).iter().any(|p| {
                matches!(
                    p,
                    ref_proto::Part::Tool {
                        state: ToolState::Running,
                        ..
                    }
                )
            })
        })
        .await;
    }
}

fn from_to(from: &str, to: &str) -> String {
    format!(
        "[Reminder] The user switched the agent from {from} to {to}. Your instructions and available tools now come from the {to} agent; where earlier messages conflict with them, follow the {to} agent."
    )
}

fn text_handler() -> Handler {
    Arc::new(|_, _| Reply::Text("ok".into()))
}

/// First round: a one second command. Second round: the answer.
fn slow_tool_handler() -> Handler {
    Arc::new(|n, _| match n {
        0 => Reply::Tools(vec![tool(
            "c1",
            "bash",
            json!({"purpose": "wait", "command": "sleep 1"}),
        )]),
        _ => Reply::Text("done".into()),
    })
}

#[tokio::test]
async fn an_idle_switch_after_history_adds_one_reminder_and_no_model_turn() {
    let rig = rig(text_handler()).await;
    prompt(&rig.hub, &rig.session, "first");
    wait_idle(&rig.session).await;
    rig.events();
    rig.switch("worker").await;
    assert_eq!(rig.notes(), [from_to("build", "worker")]);
    assert!(rig.session.lock().run.is_none());
    assert_eq!(rig.mock.count(), 1, "no LLM request for a reminder");
    let events = rig.events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SessionEvent::Message { message } if message.role == Role::User)),
        "{events:?}"
    );

    prompt(&rig.hub, &rig.session, "go");
    wait_idle(&rig.session).await;
    let request = rig.mock.bodies()[1].to_string();
    assert!(request.contains(&from_to("build", "worker")), "{request}");
}

#[tokio::test]
async fn switching_an_empty_session_only_changes_the_agent() {
    let rig = rig(text_handler()).await;
    rig.switch("worker").await;
    rig.switch("review").await;
    assert_eq!(rig.session.info().agent, "review");
    assert!(messages(&rig.session).is_empty());
    assert!(rig.session.lock().agent_switch.is_none());
    assert!(
        rig.events()
            .iter()
            .all(|e| !matches!(e, SessionEvent::Message { .. }))
    );

    prompt(&rig.hub, &rig.session, "go");
    wait_idle(&rig.session).await;
    assert!(!rig.mock.bodies()[0].to_string().contains(MARKER));
    assert!(rig.notes().is_empty());
}

#[tokio::test]
async fn switching_to_the_current_agent_adds_nothing() {
    let rig = rig(text_handler()).await;
    rig.switch("build").await;
    assert!(rig.notes().is_empty());
}

#[tokio::test]
async fn quick_switches_share_one_reminder() {
    let rig = rig_with_history(text_handler()).await;
    rig.switch("worker").await;
    let id = rig.reminder_id();
    rig.switch("review").await;
    assert_eq!(rig.notes(), [from_to("build", "review")]);
    assert_eq!(rig.reminder_id(), id, "updated in place");
    let replaced = rig.events().into_iter().any(|e| {
        matches!(e, SessionEvent::Message { message } if message.id == id && message.raw_text() == from_to("build", "review"))
    });
    assert!(replaced, "clients get the new text under the same id");
}

#[tokio::test]
async fn switching_back_removes_the_reminder() {
    let rig = rig_with_history(text_handler()).await;
    rig.switch("worker").await;
    let id = rig.reminder_id();
    rig.events();
    rig.switch("build").await;
    assert!(rig.notes().is_empty());
    let removed = rig
        .events()
        .into_iter()
        .any(|e| matches!(e, SessionEvent::Removed { message_id } if message_id == id));
    assert!(removed);
}

#[tokio::test]
async fn an_answered_reminder_is_not_rewritten() {
    let rig = rig_with_history(text_handler()).await;
    rig.switch("worker").await;
    prompt(&rig.hub, &rig.session, "go");
    wait_idle(&rig.session).await;
    rig.switch("review").await;
    assert_eq!(
        rig.notes(),
        [from_to("build", "worker"), from_to("worker", "review")]
    );
}

#[tokio::test]
async fn a_switch_during_a_run_is_told_before_the_next_request() {
    let rig = rig(slow_tool_handler()).await;
    prompt(&rig.hub, &rig.session, "go");
    rig.wait_running().await;
    rig.switch("worker").await;
    assert!(
        rig.notes().is_empty(),
        "not written while the run is active"
    );
    wait_idle(&rig.session).await;

    assert_eq!(rig.notes(), [from_to("build", "worker")]);
    let bodies = rig.mock.bodies();
    assert!(!bodies[0].to_string().contains(MARKER));
    assert!(bodies[1].to_string().contains(&from_to("build", "worker")));
    // The reminder and the new agent's tools reach the model in the same request.
    let has_tool = |body: &serde_json::Value, name: &str| {
        body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["function"]["name"] == name)
    };
    assert!(has_tool(&bodies[0], "create_session"), "build can spawn");
    assert!(!has_tool(&bodies[1], "create_session"), "worker cannot");
    // The reminder comes after the tool result and before the answer.
    let roles: Vec<(Role, MessageSource)> = messages(&rig.session)
        .iter()
        .map(|m| (m.role, m.source.clone()))
        .collect();
    assert_eq!(
        roles.iter().filter(|(r, _)| *r == Role::Assistant).count(),
        2
    );
    assert_eq!(roles.last().unwrap().0, Role::Assistant);
    assert_eq!(roles[roles.len() - 2].1, MessageSource::Reminder);
}

#[tokio::test]
async fn quick_switches_during_a_run_leave_one_reminder_or_none() {
    let rig = rig(slow_tool_handler()).await;
    prompt(&rig.hub, &rig.session, "go");
    rig.wait_running().await;
    rig.switch("worker").await;
    rig.switch("review").await;
    wait_idle(&rig.session).await;
    assert_eq!(rig.notes(), [from_to("build", "review")]);

    let back = self::rig(slow_tool_handler()).await;
    prompt(&back.hub, &back.session, "go");
    back.wait_running().await;
    back.switch("worker").await;
    back.switch("build").await;
    wait_idle(&back.session).await;
    assert!(back.notes().is_empty());
    assert!(!back.mock.bodies()[1].to_string().contains(MARKER));
}

#[tokio::test]
async fn a_switch_just_before_the_run_ends_is_recorded_without_a_new_turn() {
    let rig = rig(Arc::new(|_, _| Reply::Hang("thinking".into()))).await;
    prompt(&rig.hub, &rig.session, "go");
    let m = rig.mock.bodies.clone();
    wait_until("request sent", Duration::from_secs(10), move || {
        !m.lock().unwrap().is_empty()
    })
    .await;
    rig.switch("worker").await;
    let abort = Request::Abort {
        session_id: rig.session.id.clone(),
    };
    api::handle(&rig.hub, &rig.conn, abort).await.unwrap();
    wait_idle(&rig.session).await;
    assert_eq!(rig.notes(), [from_to("build", "worker")]);
    assert_eq!(
        rig.mock.count(),
        1,
        "the reminder did not start another turn"
    );
}

impl Rig {
    async fn undo(&self, message_id: Option<String>) {
        let req = Request::Undo {
            session_id: self.session.id.clone(),
            message_id,
        };
        api::handle(&self.hub, &self.conn, req).await.unwrap();
    }
}

#[tokio::test]
async fn undoing_the_reminder_makes_the_next_switch_start_from_the_agent_the_model_saw() {
    let rig = rig_with_history(text_handler()).await;
    rig.switch("worker").await;
    prompt(&rig.hub, &rig.session, "second");
    wait_idle(&rig.session).await;
    let id = rig.reminder_id();
    rig.undo(Some(id)).await;
    assert!(rig.notes().is_empty());

    // The model last saw the build agent, whatever the agent is now.
    rig.switch("review").await;
    assert_eq!(rig.notes(), [from_to("build", "review")]);
}

#[tokio::test]
async fn undoing_back_to_the_agent_the_model_saw_leaves_no_pending_switch() {
    let rig = rig_with_history(text_handler()).await;
    rig.switch("worker").await;
    let id = rig.reminder_id();
    rig.undo(Some(id)).await;
    rig.switch("build").await;
    assert!(rig.notes().is_empty());
    assert!(rig.session.lock().agent_switch.is_none());
}

#[tokio::test]
async fn undoing_the_whole_conversation_forgets_the_switch() {
    let rig = rig_with_history(text_handler()).await;
    rig.switch("worker").await;
    rig.undo(None).await;
    assert!(messages(&rig.session).is_empty());
    rig.switch("review").await;
    assert!(rig.notes().is_empty());
    assert_eq!(rig.session.info().agent, "review");
}

#[tokio::test]
async fn a_switch_during_manual_compaction_is_told_after_the_summary() {
    let rig = rig_with_history(Arc::new(|n, _| match n {
        0 => Reply::Text("answer".into()),
        1 => Reply::Text("## Goal\n- summary".into()),
        _ => Reply::Text("later".into()),
    }))
    .await;
    let gate = Arc::new(tokio::sync::Notify::new());
    let g = gate.clone();
    ref_server::engine::start_custom_run(
        &rig.hub,
        &rig.session,
        move |hub, s, cancel| async move {
            g.notified().await;
            ref_server::compaction::manual(hub, s, cancel).await;
        },
    )
    .unwrap();
    rig.switch("worker").await;
    assert!(
        rig.notes().is_empty(),
        "not written while the run is active"
    );
    gate.notify_one();
    wait_idle(&rig.session).await;

    let msgs = messages(&rig.session);
    let last = msgs.last().unwrap();
    assert_eq!(last.source, MessageSource::Reminder);
    assert_eq!(last.raw_text(), from_to("build", "worker"));
    assert_eq!(msgs[msgs.len() - 2].source, MessageSource::Compaction);
    assert_eq!(rig.notes().len(), 1);

    prompt(&rig.hub, &rig.session, "next");
    wait_idle(&rig.session).await;
    let request = rig.mock.bodies()[2].to_string();
    assert!(request.contains("summary") && request.contains(&from_to("build", "worker")));
}

#[tokio::test]
async fn a_second_switch_in_the_same_run_adds_a_second_reminder() {
    let rig = rig(Arc::new(|n, _| match n {
        0 | 1 => Reply::Tools(vec![tool(
            &format!("c{n}"),
            "bash",
            json!({"purpose": "wait", "command": "sleep 1"}),
        )]),
        _ => Reply::Text("done".into()),
    }))
    .await;
    prompt(&rig.hub, &rig.session, "go");
    rig.wait_running().await;
    rig.switch("worker").await;
    // The first reminder is written before the second request; its tool runs next.
    let s = rig.session.clone();
    wait_until("first reminder told", Duration::from_secs(10), move || {
        let msgs = s.lock().messages.clone();
        msgs.iter().any(|m| m.source == MessageSource::Reminder)
            && msgs.iter().any(|m| {
                m.parts.iter().any(|p| {
                    matches!(p, ref_proto::Part::Tool { call_id, state: ToolState::Running, .. } if call_id == "c1")
                })
            })
    })
    .await;
    rig.switch("review").await;
    wait_idle(&rig.session).await;
    assert_eq!(
        rig.notes(),
        [from_to("build", "worker"), from_to("worker", "review")]
    );
    assert!(
        rig.mock.bodies()[2]
            .to_string()
            .contains(&from_to("worker", "review"))
    );
}

#[tokio::test]
async fn a_pending_switch_is_not_persisted() {
    let rig = rig(slow_tool_handler()).await;
    prompt(&rig.hub, &rig.session, "go");
    rig.wait_running().await;
    rig.switch("worker").await;
    assert!(rig.session.lock().agent_switch.is_some());

    // What a reload would see: the new agent, but no reminder and no memory of the switch.
    let saved = rig.session.persisted();
    assert_eq!(saved.info.agent, "worker");
    assert!(
        saved
            .messages
            .iter()
            .all(|m| m.source != MessageSource::Reminder)
    );
    let cfg = rig.session.lock().cfg.clone();
    let reloaded = ref_server::session::Session::new(saved.info, saved.messages, cfg, false, false);
    assert!(reloaded.lock().agent_switch.is_none());
    wait_idle(&rig.session).await;
}
