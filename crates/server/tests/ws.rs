mod common;

use common::*;
use futures_util::{SinkExt, StreamExt};
use ref_proto::{ClientFrame, NotifyKind, Request, ServerFrame};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message as WsMsg;

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn send(ws: &mut Ws, id: u64, req: Request) {
    let t = serde_json::to_string(&ClientFrame { id: Some(id), req }).unwrap();
    ws.send(WsMsg::text(t)).await.unwrap();
}

/// Wait for the response with `id`, collecting other frames.
async fn response(
    ws: &mut Ws,
    id: u64,
    other: &mut Vec<ServerFrame>,
) -> (bool, Value, Option<String>) {
    let fut = async {
        while let Some(m) = ws.next().await {
            let Ok(WsMsg::Text(t)) = m else { continue };
            let f: ServerFrame = serde_json::from_str(&t).unwrap();
            match f {
                ServerFrame::Response {
                    id: rid,
                    ok,
                    result,
                    error,
                } if rid == id => return (ok, result, error),
                f => other.push(f),
            }
        }
        panic!("socket closed")
    };
    tokio::time::timeout(Duration::from_secs(10), fut)
        .await
        .expect("response timeout")
}

async fn start(token: &str, idle: i64) -> (ref_server::Running, String, std::path::PathBuf) {
    let m = mock(Arc::new(|_, _| Reply::Text("pong".into()))).await;
    let cfg = config(&m.url);
    let dir = temp_dir("ws");
    let mut opts = ref_server::HubOptions::new(dir.join("data"));
    opts.home = None;
    opts.idle_timeout = idle;
    opts.config_loader = Arc::new(move |_| cfg.clone());
    let (auth, _) = ref_server::ws::resolve_hosts(&["127.0.0.1".into()], token.into());
    let r = ref_server::start(opts, "127.0.0.1", 0, auth).await.unwrap();
    let url = format!("ws://127.0.0.1:{}/", r.addr.port());
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    // Keep the mock alive for the duration of the test.
    std::mem::forget(m);
    (r, url, work)
}

#[tokio::test]
async fn ws_smoke() {
    let (r, url, work) = start("sekret", -1).await;
    // Wrong query token: rejected before upgrade.
    assert!(
        tokio_tungstenite::connect_async(format!("{url}?token=wrong"))
            .await
            .is_err()
    );

    // No query token but correct Hello token.
    let (mut ws, _) = tokio_tungstenite::connect_async(url.clone()).await.unwrap();
    let mut other = Vec::new();
    send(
        &mut ws,
        1,
        Request::Hello {
            client: "test".into(),
            version: 1,
            cwd: work.to_string_lossy().into(),
            token: Some("sekret".into()),
            notify: Vec::new(),
        },
    )
    .await;
    let (ok, res, _) = response(&mut ws, 1, &mut other).await;
    assert!(ok);
    assert!(res["server_version"].is_string());

    send(
        &mut ws,
        2,
        Request::CreateSession {
            cwd: work.to_string_lossy().into(),
            agent: None,
            model: None,
            name: Some("ワーク".into()),
        },
    )
    .await;
    let (ok, res, err) = response(&mut ws, 2, &mut other).await;
    assert!(ok, "{err:?}");
    let sid = res["session"]["id"].as_str().unwrap().to_string();
    assert_eq!(res["session"]["name"], "ワーク");
    assert_eq!(res["session"]["agent"], "build");

    // Duplicate name from a frontend -> error.
    send(
        &mut ws,
        3,
        Request::CreateSession {
            cwd: work.to_string_lossy().into(),
            agent: None,
            model: None,
            name: Some("ワーク".into()),
        },
    )
    .await;
    let (ok, _, err) = response(&mut ws, 3, &mut other).await;
    assert!(!ok && err.unwrap().contains("already exists"));

    send(
        &mut ws,
        4,
        Request::Attach {
            session_id: sid.clone(),
            foreground: true,
            limit: None,
        },
    )
    .await;
    let (ok, res, _) = response(&mut ws, 4, &mut other).await;
    assert!(ok);
    assert_eq!(res["snapshot"]["info"]["attached"], 1);

    send(&mut ws, 5, Request::ListSessions).await;
    let (ok, res, _) = response(&mut ws, 5, &mut other).await;
    assert!(ok);
    assert_eq!(res["sessions"].as_array().unwrap().len(), 1);

    send(
        &mut ws,
        6,
        Request::Prompt {
            session_id: sid.clone(),
            text: "ping".into(),
            images: vec![],
        },
    )
    .await;
    let (ok, _, _) = response(&mut ws, 6, &mut other).await;
    assert!(ok);
    // Wait for the assistant message to arrive as an event.
    let got = async {
        loop {
            let m = ws.next().await.unwrap().unwrap();
            let WsMsg::Text(t) = m else { continue };
            let f: ServerFrame = serde_json::from_str(&t).unwrap();
            if let ServerFrame::Event {
                event: ref_proto::SessionEvent::Message { message },
                ..
            } = f
                && message.role == ref_proto::Role::Assistant
                && message.time_end.is_some()
            {
                return message.raw_text();
            }
        }
    };
    let text = tokio::time::timeout(Duration::from_secs(10), got)
        .await
        .unwrap();
    assert_eq!(text, "pong");

    send(&mut ws, 7, Request::GetConfig).await;
    let (ok, res, _) = response(&mut ws, 7, &mut other).await;
    assert!(ok);
    assert_eq!(res["config"]["models"][0]["id"], "m");

    send(
        &mut ws,
        8,
        Request::ListHistory {
            cwd: None,
            limit: None,
        },
    )
    .await;
    let (ok, res, _) = response(&mut ws, 8, &mut other).await;
    assert!(ok);
    assert_eq!(res["sessions"][0]["id"], sid.as_str());

    // Closing a foreground session from a frontend is allowed.
    send(
        &mut ws,
        9,
        Request::Close {
            session_id: sid.clone(),
        },
    )
    .await;
    let (ok, _, _) = response(&mut ws, 9, &mut other).await;
    assert!(ok);
    assert!(r.hub.sessions().is_empty());

    // Attach reloads from history.
    send(
        &mut ws,
        10,
        Request::Attach {
            session_id: sid.clone(),
            foreground: false,
            limit: None,
        },
    )
    .await;
    let (ok, res, _) = response(&mut ws, 10, &mut other).await;
    assert!(ok);
    assert_eq!(res["snapshot"]["messages"].as_array().unwrap().len(), 2);
    assert_eq!(res["snapshot"]["info"]["attached"], 0);
}

#[tokio::test]
async fn ws_first_frame_must_be_hello_and_idle_zero_shuts_down() {
    let (r, url, work) = start("tok", 0).await;
    // Not hello first -> error + close.
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("{url}?token=tok"))
        .await
        .unwrap();
    send(&mut ws, 1, Request::ListSessions).await;
    let mut other = Vec::new();
    let (ok, _, _) = response(&mut ws, 1, &mut other).await;
    assert!(!ok);
    drop(ws);
    // Idle timer is not armed before the first frontend completed Hello.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!r.hub.shutdown.is_cancelled());

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("{url}?token=tok"))
        .await
        .unwrap();
    send(
        &mut ws,
        1,
        Request::Hello {
            client: "t".into(),
            version: 1,
            cwd: work.to_string_lossy().into(),
            token: None,
            notify: Vec::new(),
        },
    )
    .await;
    let (ok, _, _) = response(&mut ws, 1, &mut other).await;
    assert!(ok);
    ws.close(None).await.unwrap();
    drop(ws);
    let hub = r.hub.clone();
    wait_until("shutdown", Duration::from_secs(5), move || {
        hub.shutdown.is_cancelled()
    })
    .await;
}

async fn hello(ws: &mut Ws, cwd: &std::path::Path, notify: Vec<NotifyKind>) -> Value {
    let req = Request::Hello {
        client: "test".into(),
        version: ref_proto::PROTOCOL_VERSION,
        cwd: cwd.to_string_lossy().into(),
        token: None,
        notify,
    };
    send(ws, 1, req).await;
    let (ok, result, err) = response(ws, 1, &mut Vec::new()).await;
    assert!(ok, "{err:?}");
    result
}

#[tokio::test]
async fn hello_reports_protocol_and_notifications_follow_focus() {
    let (_r, url, work) = start("", -1).await;
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    let result = hello(&mut ws, &work, vec![NotifyKind::TurnDone]).await;
    assert_eq!(result["protocol"], ref_proto::PROTOCOL_VERSION);

    let mut other = Vec::new();
    let create = Request::CreateSession {
        cwd: work.to_string_lossy().into(),
        agent: None,
        model: None,
        name: None,
    };
    send(&mut ws, 2, create).await;
    let (_, res, _) = response(&mut ws, 2, &mut other).await;
    let sid = res["session"]["id"].as_str().unwrap().to_string();

    // In front and showing the session: no notification.
    let focus = Request::SetFocus {
        focused: true,
        sessions: vec![sid.clone()],
    };
    send(&mut ws, 3, focus).await;
    assert!(response(&mut ws, 3, &mut other).await.0);
    let prompt = || Request::Prompt {
        session_id: sid.clone(),
        text: "hi".into(),
        images: vec![],
    };
    send(&mut ws, 4, prompt()).await;
    assert!(response(&mut ws, 4, &mut other).await.0);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let notified = |frames: &[ServerFrame]| {
        frames
            .iter()
            .any(|f| matches!(f, ServerFrame::Notify { .. }))
    };
    send(&mut ws, 5, Request::Ping).await;
    response(&mut ws, 5, &mut other).await;
    assert!(!notified(&other), "{other:?}");

    // Window in the background: the next turn is announced.
    let blur = Request::SetFocus {
        focused: false,
        sessions: vec![sid.clone()],
    };
    send(&mut ws, 6, blur).await;
    assert!(response(&mut ws, 6, &mut other).await.0);
    send(&mut ws, 7, prompt()).await;
    assert!(response(&mut ws, 7, &mut other).await.0);
    let fut = async {
        while let Some(m) = ws.next().await {
            let Ok(WsMsg::Text(t)) = m else { continue };
            if let ServerFrame::Notify {
                session_id,
                notify_kind,
                detail,
                ..
            } = serde_json::from_str(&t).unwrap()
            {
                return (session_id, notify_kind, detail);
            }
        }
        panic!("socket closed")
    };
    let (session_id, kind, detail) = tokio::time::timeout(Duration::from_secs(10), fut)
        .await
        .expect("notification timeout");
    assert_eq!(
        (session_id, kind, detail),
        (sid, NotifyKind::TurnDone, None)
    );
}

#[tokio::test]
async fn list_dirs_answers_with_sorted_sub_directories() {
    let (_r, url, work) = start("", -1).await;
    for d in ["beta", "Alpha", "gamma/inner", ".hidden"] {
        std::fs::create_dir_all(work.join(d)).unwrap();
    }
    std::fs::write(work.join("file.txt"), "x").unwrap();
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    hello(&mut ws, &work, vec![]).await;
    let mut other = Vec::new();
    let canonical = dunce::canonicalize(&work).unwrap();

    // Relative to the connection's cwd.
    send(&mut ws, 2, Request::ListDirs { path: ".".into() }).await;
    let (ok, res, err) = response(&mut ws, 2, &mut other).await;
    assert!(ok, "{err:?}");
    assert_eq!(res["path"], canonical.to_string_lossy().as_ref());
    assert_eq!(
        res["dirs"],
        serde_json::json!([".hidden", "Alpha", "beta", "gamma"])
    );

    // Absolute and normalized.
    let messy = format!("{}/gamma/../gamma/inner", work.display());
    send(&mut ws, 3, Request::ListDirs { path: messy }).await;
    let (ok, res, _) = response(&mut ws, 3, &mut other).await;
    assert!(ok);
    assert_eq!(
        res["path"],
        canonical.join("gamma/inner").to_string_lossy().as_ref()
    );
    assert_eq!(res["dirs"], serde_json::json!([]));

    // Without a configured home directory `~` is just a missing name.
    send(&mut ws, 4, Request::ListDirs { path: "~".into() }).await;
    let (ok, _, err) = response(&mut ws, 4, &mut other).await;
    assert!(!ok && err.is_some());

    // Errors: missing path, a file, empty path.
    for (id, path) in [(5, "nowhere"), (6, "file.txt"), (7, "  ")] {
        send(&mut ws, id, Request::ListDirs { path: path.into() }).await;
        let (ok, _, err) = response(&mut ws, id, &mut other).await;
        assert!(!ok && err.is_some(), "{path}");
    }
}

#[tokio::test]
async fn list_dirs_expands_the_configured_home() {
    let m = mock(Arc::new(|_, _| Reply::Text("pong".into()))).await;
    let dir = temp_dir("ws-home");
    let home = dir.join("home");
    std::fs::create_dir_all(home.join("docs")).unwrap();
    let cfg = config(&m.url);
    let mut opts = ref_server::HubOptions::new(dir.join("data"));
    opts.home = Some(home.clone());
    opts.config_loader = Arc::new(move |_| cfg.clone());
    let hub = ref_server::Hub::new(opts);
    let conn = hub.add_conn();
    conn.lock().cwd = dir.to_string_lossy().into();
    for path in ["~", "~/"] {
        let req = Request::ListDirs { path: path.into() };
        let res = ref_server::api::handle(&hub, &conn, req).await.unwrap();
        let canonical = dunce::canonicalize(&home).unwrap();
        assert_eq!(res["path"], canonical.to_string_lossy().as_ref(), "{path}");
        assert_eq!(res["dirs"], serde_json::json!(["docs"]));
    }
}

#[tokio::test]
async fn frames_before_hello_are_refused() {
    let (_r, url, _work) = start("", -1).await;
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    let focus = Request::SetFocus {
        focused: true,
        sessions: vec![],
    };
    send(&mut ws, 1, focus).await;
    let (ok, _, err) = response(&mut ws, 1, &mut Vec::new()).await;
    assert!(!ok);
    assert!(err.unwrap().contains("hello"));
    // The server hangs up afterwards.
    let closed = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(m) = ws.next().await {
            if m.is_err() {
                return;
            }
        }
    })
    .await;
    assert!(closed.is_ok(), "the connection is closed");
}

#[cfg(unix)]
#[tokio::test]
async fn list_dirs_skips_names_that_are_not_utf8() {
    use std::os::unix::ffi::OsStrExt;
    let m = mock(Arc::new(|_, _| Reply::Text("pong".into()))).await;
    let dir = temp_dir("ws-utf8");
    std::fs::create_dir_all(dir.join("good")).unwrap();
    let bad = dir.join(std::ffi::OsStr::from_bytes(b"bad\xff"));
    if let Err(e) = std::fs::create_dir_all(&bad) {
        // APFS refuses names that are not valid UTF-8: nothing to test there.
        assert_eq!(e.raw_os_error(), Some(libc::EILSEQ), "{e}");
        eprintln!("skipped: this filesystem refuses non-UTF-8 names");
        return;
    }
    let cfg = config(&m.url);
    let mut opts = ref_server::HubOptions::new(dir.join("data"));
    opts.home = None;
    opts.config_loader = Arc::new(move |_| cfg.clone());
    let hub = ref_server::Hub::new(opts);
    let conn = hub.add_conn();
    conn.lock().cwd = dir.to_string_lossy().into();
    let req = Request::ListDirs { path: ".".into() };
    let res = ref_server::api::handle(&hub, &conn, req).await.unwrap();
    let names: Vec<&str> = res["dirs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|d| d.as_str())
        .filter(|d| *d == "good" || d.contains("bad") || d.contains('\u{fffd}'))
        .collect();
    assert_eq!(names, ["good"]);
}
