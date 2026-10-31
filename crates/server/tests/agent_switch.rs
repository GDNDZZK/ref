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

impl Rig {
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
async fn an_idle_switch_adds_one_reminder_and_no_model_turn() {
    let rig = rig(text_handler()).await;
    rig.switch("worker").await;
    assert_eq!(rig.notes(), [from_to("build", "worker")]);
    assert!(rig.session.lock().run.is_none());
    assert_eq!(rig.mock.count(), 0, "no LLM request for a reminder");
    let events = rig.events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SessionEvent::Message { message } if message.role == Role::User)),
        "{events:?}"
    );

    prompt(&rig.hub, &rig.session, "go");
    wait_idle(&rig.session).await;
    let request = rig.mock.bodies()[0].to_string();
    assert!(request.contains(&from_to("build", "worker")), "{request}");
}

#[tokio::test]
async fn switching_to_the_current_agent_adds_nothing() {
    let rig = rig(text_handler()).await;
    rig.switch("build").await;
    assert!(rig.notes().is_empty());
}

#[tokio::test]
async fn quick_switches_share_one_reminder() {
    let rig = rig(text_handler()).await;
    rig.switch("worker").await;
    let id = messages(&rig.session)[0].id.clone();
    rig.switch("review").await;
    assert_eq!(rig.notes(), [from_to("build", "review")]);
    assert_eq!(messages(&rig.session)[0].id, id, "updated in place");
    let replaced = rig.events().into_iter().any(|e| {
        matches!(e, SessionEvent::Message { message } if message.id == id && message.raw_text() == from_to("build", "review"))
    });
    assert!(replaced, "clients get the new text under the same id");
}

#[tokio::test]
async fn switching_back_removes_the_reminder() {
    let rig = rig(text_handler()).await;
    rig.switch("worker").await;
    let id = messages(&rig.session)[0].id.clone();
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
    let rig = rig(text_handler()).await;
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
