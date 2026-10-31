mod common;

use common::*;
use ref_proto::{
    NotifyKind, Part, QuestionRequest, Request, ServerFrame, SessionEvent, SessionStatus, ToolState,
};
use ref_server::api;
use ref_server::hub::{Conn, Hub};
use ref_server::session::Session;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

const CALL: &str = "q1";
const WAIT: Duration = Duration::from_secs(10);

fn one_question() -> Value {
    json!({"purpose": "ask", "questions": [{
        "question": "Which?",
        "header": "Pick",
        "options": [
            {"label": "A", "description": "first"},
            {"label": "B", "description": "second", "default": true},
        ],
    }]})
}

/// Round 0 asks `args`; the next round answers in text.
fn asking(args: Value) -> Handler {
    Arc::new(move |n, _| match n {
        0 => Reply::Tools(vec![tool(CALL, "question", args.clone())]),
        _ => Reply::Text("thanks".into()),
    })
}

struct Rig {
    hub: Arc<Hub>,
    conn: Arc<Conn>,
    session: Arc<Session>,
    mock: Mock,
}

async fn rig_with(handler: Handler, tweak: impl FnOnce(&mut ref_config::Config)) -> Rig {
    let mock = mock(handler).await;
    let mut cfg = config(&mock.url);
    tweak(&mut cfg);
    let (hub, work) = hub(cfg, "question");
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

async fn rig(handler: Handler) -> Rig {
    rig_with(handler, |_| {}).await
}

impl Rig {
    async fn request(&self, req: Request) -> Result<Value, String> {
        api::handle(&self.hub, &self.conn, req).await
    }

    async fn answer(&self, answers: Option<Vec<Vec<&str>>>) -> Result<Value, String> {
        let answers = answers.map(|a| {
            a.into_iter()
                .map(|e| e.into_iter().map(String::from).collect())
                .collect()
        });
        self.request(Request::QuestionReply {
            session_id: self.session.id.clone(),
            call_id: CALL.into(),
            answers,
        })
        .await
    }

    /// Waits until the model's question is pending.
    async fn asked(&self) -> QuestionRequest {
        let s = self.session.clone();
        wait_until("question pending", WAIT, move || {
            !s.lock().questions.is_empty()
        })
        .await;
        self.session.lock().questions[0].request.clone()
    }

    fn tool_state(&self) -> ToolState {
        match tool_parts(&self.session).remove(0) {
            Part::Tool { state, .. } => state,
            other => panic!("not a tool: {other:?}"),
        }
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

    /// The result of the question as the model saw it in its next request.
    fn result_seen_by_the_model(&self) -> String {
        let bodies = self.mock.bodies();
        let results = tool_results(&bodies[1]);
        results[0]["content"].as_str().unwrap().to_string()
    }
}

fn is_resolved(events: &[SessionEvent]) -> bool {
    events
        .iter()
        .any(|e| matches!(e, SessionEvent::QuestionResolved { call_id } if call_id == CALL))
}

#[tokio::test]
async fn the_session_waits_for_the_answer_and_the_model_gets_it() {
    let rig = rig(asking(one_question())).await;
    prompt(&rig.hub, &rig.session, "go");
    let request = rig.asked().await;
    assert_eq!(request.call_id, CALL);
    assert_eq!(request.questions[0].options[1].label, "B");
    assert!(request.questions[0].options[1].default);

    assert_eq!(rig.session.info().status, SessionStatus::WaitingQuestion);
    assert!(rig.session.lock().run.is_some(), "the run waits");
    assert_eq!(rig.tool_state(), ToolState::Running);
    let listed = rig.hub.infos();
    assert!(
        listed
            .iter()
            .any(|i| i.status == SessionStatus::WaitingQuestion)
    );
    assert!(
        rig.events()
            .iter()
            .any(|e| matches!(e, SessionEvent::Question { request: r } if r.call_id == CALL))
    );

    // A window that attaches now sees the question again.
    let late = rig.hub.add_conn();
    late.lock().hello = true;
    let attach = Request::Attach {
        session_id: rig.session.id.clone(),
        foreground: true,
        limit: None,
    };
    let res = api::handle(&rig.hub, &late, attach).await.unwrap();
    assert_eq!(res["snapshot"]["pending_questions"][0]["call_id"], CALL);

    rig.answer(Some(vec![vec!["A"]])).await.unwrap();
    wait_idle(&rig.session).await;
    assert_eq!(rig.tool_state(), ToolState::Done);
    assert_eq!(last_text(&rig.session), "thanks");
    assert_eq!(
        rig.result_seen_by_the_model(),
        "User has answered your questions: \"Which?\"=\"A\". You can now continue with the user's answers in mind."
    );
    assert!(is_resolved(&rig.events()));
    assert!(rig.session.lock().questions.is_empty());
    assert_eq!(rig.session.info().status, SessionStatus::Idle);
}

#[tokio::test]
async fn a_typed_answer_is_passed_on_as_text() {
    let rig = rig(asking(one_question())).await;
    prompt(&rig.hub, &rig.session, "go");
    rig.asked().await;
    rig.answer(Some(vec![vec![" my own idea "]])).await.unwrap();
    wait_idle(&rig.session).await;
    assert!(
        rig.result_seen_by_the_model()
            .contains("\"Which?\"=\"my own idea\"")
    );
}

#[tokio::test]
async fn a_dismissed_question_is_a_denied_call_and_the_turn_goes_on() {
    let rig = rig(asking(one_question())).await;
    prompt(&rig.hub, &rig.session, "go");
    rig.asked().await;
    rig.answer(None).await.unwrap();
    wait_idle(&rig.session).await;
    assert_eq!(rig.tool_state(), ToolState::Denied);
    assert!(rig.result_seen_by_the_model().contains("dismissed"));
    assert_eq!(last_text(&rig.session), "thanks");
}

#[tokio::test]
async fn the_first_valid_reply_wins_and_a_bad_one_changes_nothing() {
    let rig = rig(asking(one_question())).await;
    prompt(&rig.hub, &rig.session, "go");
    rig.asked().await;

    let two = rig.answer(Some(vec![vec!["A", "B"]])).await.unwrap_err();
    assert!(two.contains("one answer"), "{two}");
    let count = rig.answer(Some(vec![])).await.unwrap_err();
    assert!(count.contains("Expected 1"), "{count}");
    assert_eq!(rig.session.lock().questions.len(), 1, "still waiting");

    rig.answer(Some(vec![vec!["B"]])).await.unwrap();
    let late = rig.answer(Some(vec![vec!["A"]])).await.unwrap_err();
    assert!(late.contains("No pending question"), "{late}");
    wait_idle(&rig.session).await;
    assert!(rig.result_seen_by_the_model().contains("\"Which?\"=\"B\""));
}

#[tokio::test]
async fn an_interrupt_ends_the_wait_and_tells_the_clients() {
    let rig = rig(asking(one_question())).await;
    prompt(&rig.hub, &rig.session, "go");
    rig.asked().await;
    rig.events();
    let abort = Request::Abort {
        session_id: rig.session.id.clone(),
    };
    rig.request(abort).await.unwrap();
    wait_idle(&rig.session).await;
    assert_eq!(rig.tool_state(), ToolState::Cancelled);
    assert!(is_resolved(&rig.events()));
    assert!(rig.session.lock().questions.is_empty());
    let late = rig.answer(None).await.unwrap_err();
    assert!(late.contains("No pending question"), "{late}");
}

#[tokio::test]
async fn undo_ends_the_wait_too() {
    let rig = rig(asking(one_question())).await;
    prompt(&rig.hub, &rig.session, "go");
    rig.asked().await;
    rig.events();
    let undo = Request::Undo {
        session_id: rig.session.id.clone(),
        message_id: None,
    };
    rig.request(undo).await.unwrap();
    assert!(is_resolved(&rig.events()));
    assert!(rig.session.lock().questions.is_empty());
    assert!(messages(&rig.session).is_empty());
    assert_eq!(rig.session.info().status, SessionStatus::Idle);
}

#[tokio::test]
async fn a_malformed_call_goes_back_to_the_model_without_waiting() {
    let rig = rig(asking(json!({"purpose": "ask", "questions": []}))).await;
    prompt(&rig.hub, &rig.session, "go");
    wait_idle(&rig.session).await;
    assert_eq!(rig.tool_state(), ToolState::Error);
    assert!(rig.result_seen_by_the_model().contains("1 to 4"));
}

#[tokio::test]
async fn a_denying_permission_removes_the_tool() {
    let tool_names = |rig: &Rig| -> Vec<String> {
        rig.mock.bodies()[0]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["function"]["name"].as_str().unwrap().to_string())
            .collect()
    };
    let open = rig(Arc::new(|_, _| Reply::Text("ok".into()))).await;
    prompt(&open.hub, &open.session, "go");
    wait_idle(&open.session).await;
    assert!(tool_names(&open).contains(&"question".to_string()));

    let closed = rig_with(Arc::new(|_, _| Reply::Text("ok".into())), |cfg| {
        cfg.agents.get_mut("build").unwrap().permission = perms(json!({"question": "deny"}));
    })
    .await;
    prompt(&closed.hub, &closed.session, "go");
    wait_idle(&closed.session).await;
    assert!(!tool_names(&closed).contains(&"question".to_string()));
}

#[tokio::test]
async fn a_window_that_is_not_looking_gets_a_notification() {
    let rig = rig(asking(one_question())).await;
    let away = rig.hub.add_conn();
    {
        let mut c = away.lock();
        c.hello = true;
        c.notify = [NotifyKind::Question].into_iter().collect();
    }
    prompt(&rig.hub, &rig.session, "go");
    rig.asked().await;
    let notes: Vec<_> = away
        .out
        .drain()
        .into_iter()
        .filter_map(|f| match f {
            ServerFrame::Notify {
                notify_kind,
                detail,
                ..
            } => Some((notify_kind, detail)),
            _ => None,
        })
        .collect();
    assert_eq!(notes, [(NotifyKind::Question, Some("Which?".to_string()))]);

    // The same question shown in a window in front is not announced.
    rig.answer(Some(vec![vec!["A"]])).await.unwrap();
    wait_idle(&rig.session).await;
    let focus = Request::SetFocus {
        focused: true,
        sessions: vec![rig.session.id.clone()],
    };
    api::handle(&rig.hub, &away, focus).await.unwrap();
    away.out.drain();
    rig.mock.bodies.lock().unwrap().clear();
    prompt(&rig.hub, &rig.session, "again");
    wait_until("asked again", WAIT, {
        let s = rig.session.clone();
        move || s.lock().questions.len() == 1
    })
    .await;
    assert!(
        away.out
            .drain()
            .iter()
            .all(|f| !matches!(f, ServerFrame::Notify { .. }))
    );
}
