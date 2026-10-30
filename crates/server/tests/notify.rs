mod common;

use common::*;
use ref_proto::{NotifyKind, Request, ServerFrame, SessionStatus};
use ref_server::api;
use ref_server::hub::{Conn, Hub};
use ref_server::session::Session;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

const TITLE: &str = "Fix it";
const ALL: [NotifyKind; 3] = [
    NotifyKind::Permission,
    NotifyKind::TurnDone,
    NotifyKind::TurnError,
];

/// A connection that finished Hello and asked for `kinds`.
fn connect(hub: &Arc<Hub>, kinds: &[NotifyKind]) -> Arc<Conn> {
    let conn = hub.add_conn();
    {
        let mut c = conn.lock();
        c.hello = true;
        c.notify = kinds.iter().copied().collect();
        c.focus_stamp = hub.next_stamp();
    }
    conn
}

async fn set_focus(hub: &Arc<Hub>, conn: &Arc<Conn>, focused: bool, sessions: &[&Session]) {
    let sessions = sessions.iter().map(|s| s.id.clone()).collect();
    let req = Request::SetFocus { focused, sessions };
    api::handle(hub, conn, req).await.unwrap();
}

/// The notifications the connection received so far: kind and detail.
fn notes(conn: &Arc<Conn>) -> Vec<(NotifyKind, Option<String>)> {
    conn.out
        .drain()
        .into_iter()
        .filter_map(|f| match f {
            ServerFrame::Notify {
                notify_kind,
                session_title,
                detail,
                ..
            } => {
                assert_eq!(session_title, TITLE);
                Some((notify_kind, detail))
            }
            _ => None,
        })
        .collect()
}

async fn titled_session(hub: &Arc<Hub>, work: &std::path::Path) -> Arc<Session> {
    let s = session(hub, work, "main", "build").await;
    {
        let mut st = s.lock();
        st.info.title = TITLE.into();
        st.title_started = true;
    }
    s
}

#[tokio::test]
async fn turn_done_goes_to_the_most_recent_connection_only() {
    let m = mock(Arc::new(|_, _| Reply::Text("done".into()))).await;
    let (hub, work) = hub(config(&m.url), "done");
    let s = titled_session(&hub, &work).await;
    let older = connect(&hub, &ALL);
    let newer = connect(&hub, &ALL);

    prompt(&hub, &s, "go");
    wait_idle(&s).await;
    assert!(notes(&older).is_empty());
    assert_eq!(notes(&newer), [(NotifyKind::TurnDone, None)]);

    // The older window comes to the front (showing another session): it wins now.
    let other = session(&hub, &work, "other", "build").await;
    set_focus(&hub, &older, true, &[&other]).await;
    prompt(&hub, &s, "again");
    wait_idle(&s).await;
    assert_eq!(notes(&older), [(NotifyKind::TurnDone, None)]);
    assert!(notes(&newer).is_empty());
}

#[tokio::test]
async fn nothing_is_sent_while_a_window_in_front_shows_the_session() {
    let m = mock(Arc::new(|_, _| Reply::Text("done".into()))).await;
    let (hub, work) = hub(config(&m.url), "watching");
    let s = titled_session(&hub, &work).await;
    let viewer = connect(&hub, &[]);
    let other = connect(&hub, &ALL);

    // In front and showing the session (a tile of the split view is enough).
    set_focus(&hub, &viewer, true, &[&s]).await;
    prompt(&hub, &s, "go");
    wait_idle(&s).await;
    assert!(notes(&viewer).is_empty());
    assert!(notes(&other).is_empty());

    // Showing the session but not in front: the user is not looking.
    set_focus(&hub, &viewer, false, &[&s]).await;
    prompt(&hub, &s, "again");
    wait_idle(&s).await;
    assert_eq!(notes(&other), [(NotifyKind::TurnDone, None)]);
}

#[tokio::test]
async fn only_connections_that_asked_for_the_kind_are_considered() {
    let m = mock(Arc::new(|_, _| Reply::Text("done".into()))).await;
    let (hub, work) = hub(config(&m.url), "kinds");
    let s = titled_session(&hub, &work).await;
    let wants_done = connect(&hub, &[NotifyKind::TurnDone]);
    let newest_wants_errors = connect(&hub, &[NotifyKind::TurnError]);
    let none = connect(&hub, &[]);

    prompt(&hub, &s, "go");
    wait_idle(&s).await;
    assert_eq!(notes(&wants_done), [(NotifyKind::TurnDone, None)]);
    assert!(notes(&newest_wants_errors).is_empty());
    assert!(notes(&none).is_empty());

    // SetNotify replaces the kinds.
    let req = Request::SetNotify {
        kinds: vec![NotifyKind::TurnDone],
    };
    api::handle(&hub, &none, req).await.unwrap();
    prompt(&hub, &s, "again");
    wait_idle(&s).await;
    assert_eq!(notes(&none), [(NotifyKind::TurnDone, None)]);
    assert!(notes(&wants_done).is_empty());
}

#[tokio::test]
async fn permission_request_notifies_with_the_tool_name() {
    let m = mock(Arc::new(|n, _| match n {
        0 => Reply::Tools(vec![tool(
            "c1",
            "bash",
            json!({"purpose": "p", "command": "echo hi"}),
        )]),
        _ => Reply::Text("ok".into()),
    }))
    .await;
    let mut cfg = config(&m.url);
    cfg.permission = perms(json!({"bash": "ask"}));
    let (hub, work) = hub(cfg, "perm");
    let s = titled_session(&hub, &work).await;
    let conn = connect(&hub, &ALL);

    prompt(&hub, &s, "go");
    let s2 = s.clone();
    wait_until("permission", Duration::from_secs(10), move || {
        s2.lock().perms.len() == 1
    })
    .await;
    assert_eq!(
        notes(&conn),
        [(NotifyKind::Permission, Some("bash".into()))]
    );

    let tx = s.lock().perms[0].tx.take().unwrap();
    tx.send((true, false)).unwrap();
    wait_idle(&s).await;
    assert_eq!(notes(&conn), [(NotifyKind::TurnDone, None)]);
}

#[tokio::test]
async fn final_error_notifies_and_is_kept_until_the_next_run() {
    let fail = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let fail2 = fail.clone();
    let m = mock(Arc::new(move |_, _| {
        if fail2.load(std::sync::atomic::Ordering::SeqCst) {
            Reply::Status(400, r#"{"error":{"message":"bad request"}}"#.into())
        } else {
            Reply::Text("fine".into())
        }
    }))
    .await;
    let (hub, work) = hub(config(&m.url), "error");
    let s = titled_session(&hub, &work).await;
    let conn = connect(&hub, &ALL);

    prompt(&hub, &s, "go");
    wait_idle(&s).await;
    let found = notes(&conn);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, NotifyKind::TurnError);
    assert!(found[0].1.as_deref().unwrap().contains("bad request"));
    let error = s.info().last_error.expect("last_error is set");
    assert!(error.contains("bad request"));

    // The next run clears it and ends well.
    fail.store(false, std::sync::atomic::Ordering::SeqCst);
    prompt(&hub, &s, "again");
    wait_idle(&s).await;
    assert_eq!(s.info().last_error, None);
    assert_eq!(notes(&conn), [(NotifyKind::TurnDone, None)]);
}

#[tokio::test]
async fn retries_do_not_notify_until_the_final_outcome() {
    let m = mock(Arc::new(|n, _| match n {
        0 | 1 => Reply::Status(500, r#"{"error":{"message":"internal"}}"#.into()),
        _ => Reply::Text("recovered".into()),
    }))
    .await;
    let (hub, work) = hub(config(&m.url), "retry");
    let s = titled_session(&hub, &work).await;
    let conn = connect(&hub, &ALL);

    prompt(&hub, &s, "go");
    wait_idle(&s).await;
    assert_eq!(notes(&conn), [(NotifyKind::TurnDone, None)]);
    assert_eq!(s.info().last_error, None);
}

#[tokio::test]
async fn user_abort_sends_nothing() {
    let m = mock(Arc::new(|_, _| Reply::Hang("thinking".into()))).await;
    let (hub, work) = hub(config(&m.url), "abort");
    let s = titled_session(&hub, &work).await;
    let conn = connect(&hub, &ALL);

    prompt(&hub, &s, "go");
    let s2 = s.clone();
    wait_until("streaming", Duration::from_secs(10), move || {
        s2.lock().info.status == SessionStatus::Busy && s2.lock().messages.len() >= 2
    })
    .await;
    ref_server::engine::abort(&hub, &s, true);
    wait_idle(&s).await;
    assert!(notes(&conn).is_empty());
    assert_eq!(s.info().last_error, None);
}

#[tokio::test]
async fn manual_compaction_sends_nothing() {
    let m = mock(Arc::new(|_, _| Reply::Text("summary".into()))).await;
    let (hub, work) = hub(config(&m.url), "compact");
    let s = titled_session(&hub, &work).await;
    prompt(&hub, &s, "go");
    wait_idle(&s).await;
    let conn = connect(&hub, &ALL);

    ref_server::engine::start_custom_run(&hub, &s, ref_server::compaction::manual).unwrap();
    wait_idle(&s).await;
    assert!(notes(&conn).is_empty());
}

#[tokio::test]
async fn a_final_error_after_the_user_stopped_the_run_sends_nothing() {
    for closed in [false, true] {
        let m = mock(Arc::new(|_, _| {
            Reply::Status(400, r#"{"error":{"message":"bad request"}}"#.into())
        }))
        .await;
        let (hub, work) = hub(config(&m.url), "stopped-error");
        let s = titled_session(&hub, &work).await;
        let conn = connect(&hub, &ALL);

        prompt(&hub, &s, "go");
        // The run task has not started yet: mark the run as stopped before it fails.
        {
            let mut st = s.lock();
            st.user_aborted = !closed;
            st.closed = closed;
        }
        wait_idle(&s).await;
        assert!(notes(&conn).is_empty(), "closed: {closed}");
    }
}

#[tokio::test]
async fn closing_a_session_sends_nothing() {
    let m = mock(Arc::new(|_, _| Reply::Hang("thinking".into()))).await;
    let (hub, work) = hub(config(&m.url), "close");
    let s = titled_session(&hub, &work).await;
    let conn = connect(&hub, &ALL);

    prompt(&hub, &s, "go");
    let s2 = s.clone();
    wait_until("streaming", Duration::from_secs(10), move || {
        s2.lock().messages.len() >= 2
    })
    .await;
    let req = Request::Close {
        session_id: s.id.clone(),
    };
    api::handle(&hub, &conn, req).await.unwrap();
    wait_idle(&s).await;
    assert!(notes(&conn).is_empty());
}

#[tokio::test]
async fn a_queued_prompt_restarts_the_run_after_a_final_error() {
    // The first request fails for good (three non-retryable answers), then all is well.
    let m = mock(Arc::new(|n, _| {
        if n < 3 {
            Reply::Status(400, r#"{"error":{"message":"bad request"}}"#.into())
        } else {
            Reply::Text("fine".into())
        }
    }))
    .await;
    // Slow retries leave time to queue a prompt after the run took its queue for the last time.
    let cfg = config(&m.url);
    let dir = temp_dir("restart");
    let mut opts = ref_server::HubOptions::new(dir.join("data"));
    opts.home = None;
    opts.retry_base = Duration::from_millis(300);
    opts.config_loader = Arc::new(move |_| cfg.clone());
    let hub = Hub::new(opts);
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let s = titled_session(&hub, &work).await;
    let conn = connect(&hub, &ALL);

    prompt(&hub, &s, "go");
    let s2 = s.clone();
    wait_until("retrying", Duration::from_secs(10), move || {
        matches!(s2.lock().info.status, SessionStatus::Retrying { .. })
    })
    .await;
    prompt(&hub, &s, "and then this");
    wait_idle(&s).await;
    let kinds: Vec<NotifyKind> = notes(&conn).into_iter().map(|(k, _)| k).collect();
    assert_eq!(kinds, [NotifyKind::TurnError, NotifyKind::TurnDone]);
    assert_eq!(last_text(&s), "fine");
    assert_eq!(s.info().last_error, None, "the restarted run cleared it");
}
