mod common;

use common::*;
use hn_proto::*;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn idle_timeout_zero_exits_after_last_client_and_cleans_registry() {
    let mock = Mock::start(Arc::new(|_r: &Req| text("x"))).await;
    let env = Env::new(mock.port, json!({}));
    let mut srv = env.spawn(&["--idle-timeout", "0"]);
    let c = Client::connect(&srv.url(), &env.work()).await;
    let inst = env.path("state/instances").join(format!("{}.json", srv.child.id()));
    assert!(inst.exists(), "instance file written");
    let info: InstanceInfo = serde_json::from_str(&std::fs::read_to_string(&inst).unwrap()).unwrap();
    assert_eq!(info.port, srv.port);
    assert_eq!(info.idle_timeout, 0);
    // Still alive while connected.
    assert!(srv.wait_exit(Duration::from_millis(500)).await.is_none());
    c.close();
    let st = srv.wait_exit(Duration::from_secs(10)).await.expect("server should exit");
    assert!(st.success(), "{st:?}\n{}", srv.log_text());
    assert!(!inst.exists(), "instance file removed");
}

#[tokio::test]
async fn sigterm_flushes_and_exits() {
    let mock = Mock::start(Arc::new(|_r: &Req| text("x"))).await;
    let env = Env::new(mock.port, json!({}));
    let mut srv = env.spawn(&[]);
    let mut c = Client::connect(&srv.url(), &env.work()).await;
    let s = c.create(&env.work(), Some("名字 unicode")).await;
    // SAFETY: plain kill on our own child.
    unsafe { libc::kill(srv.child.id() as i32, libc::SIGTERM) };
    let st = srv.wait_exit(Duration::from_secs(10)).await.expect("exit on SIGTERM");
    assert!(st.success());
    let info: SessionInfo =
        serde_json::from_str(&std::fs::read_to_string(env.path("data/sessions").join(&s.id).join("info.json")).unwrap()).unwrap();
    assert_eq!(info.name, "名字 unicode");
}

#[tokio::test]
async fn status_reports_instances() {
    let mock = Mock::start(Arc::new(|_r: &Req| text("x"))).await;
    let env = Env::new(mock.port, json!({}));
    // Nothing running.
    let out = env.command().arg("--status").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stdout).contains("No hernees-server instance is running"));
    // Stale file is cleaned up.
    let dir = env.path("state/instances");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("999999.json"), r#"{"pid":999999,"port":1,"bind":"127.0.0.1","allow_hosts":[],"idle_timeout":0,"started_at":0,"version":"x","exe":"x"}"#).unwrap();

    let srv = env.spawn(&["--idle-timeout", "-1"]);
    let mut c = Client::connect(&srv.url(), &env.work()).await;
    c.create(&env.work(), None).await;
    let out = env.command().arg("--status").output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains(&format!("pid           {}", srv.child.id())), "{text}");
    assert!(text.contains("-1 (never)"), "{text}");
    assert!(text.contains("1 open (0 busy), 1 frontend(s) connected"), "{text}");
    assert!(!dir.join("999999.json").exists());
    let out = env.command().args(["--list", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v[0]["port"], srv.port);
    assert_eq!(v[0]["open_sessions"], 1);
    // The --status probe must not count as a frontend for the idle timer or the count.
    assert_eq!(v[0]["frontends"], 1);
}

#[tokio::test]
async fn rejects_disallowed_hosts() {
    let mock = Mock::start(Arc::new(|_r: &Req| text("x"))).await;
    let env = Env::new(mock.port, json!({}));
    let srv = env.spawn(&["--allow-host", "10.9.9.9"]);
    let url = srv.url();
    let mut last = None;
    for _ in 0..50 {
        match tokio_tungstenite::connect_async(url.as_str()).await {
            Err(tokio_tungstenite::tungstenite::Error::Http(resp)) => {
                last = Some(resp.status().as_u16());
                break;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
            Ok(_) => panic!("connection should be rejected"),
        }
    }
    assert_eq!(last, Some(403));
}

#[tokio::test]
async fn background_watch_and_reattach_after_restart() {
    let mock = Mock::start(Arc::new(|_r: &Req| text("answer"))).await;
    let env = Env::new(mock.port, json!({}));
    let srv = env.spawn(&[]);
    let mut fg = Client::connect(&srv.url(), &env.work()).await;
    let mut mon = Client::connect(&srv.url(), &env.work()).await;
    mon.req(ClientMsg::WatchBackground { enabled: true }).await.unwrap();
    let s = fg.create(&env.work(), Some("bg-test")).await;
    // Detach: becomes background; monitor watches it.
    fg.req(ClientMsg::Detach { session: s.id.clone() }).await.unwrap();
    mon.wait_for("bg in sessions", T, |m| {
        matches!(m, ServerMsg::Sessions { sessions } if sessions.iter().any(|x| x.id == s.id && x.attached == 0))
    })
    .await;
    mon.req(ClientMsg::Attach { session: s.id.clone(), foreground: false }).await.unwrap();
    fg.prompt(&s.id, "work in background").await;
    mon.wait_for("bg delta", T, |m| matches!(m, ServerMsg::Event { session, event: Event::Delta { .. } } if *session == s.id)).await;
    mon.wait_idle(&s.id, T).await;
    // Close then reopen from disk.
    fg.req(ClientMsg::Close { session: s.id.clone() }).await.unwrap();
    mon.wait_for("closed", T, |m| matches!(m, ServerMsg::Event { session, event: Event::Closed } if *session == s.id)).await;
    let open: Vec<SessionInfo> = serde_json::from_value(fg.req(ClientMsg::ListSessions { scope: ListScope::Open }).await.unwrap()).unwrap();
    assert!(open.is_empty());
    let (info, msgs) = fg.snapshot(&s.id).await;
    assert_eq!(msgs.len(), 2);
    assert!(info.open);
    // Delete removes files.
    fg.req(ClientMsg::Delete { session: s.id.clone() }).await.unwrap();
    assert!(!env.path("data/sessions").join(&s.id).exists());
}

#[tokio::test]
async fn positive_idle_timeout_starts_after_first_frontend() {
    let mock = Mock::start(Arc::new(|_r: &Req| text("x"))).await;
    let env = Env::new(mock.port, json!({}));
    let mut srv = env.spawn(&["--idle-timeout", "1"]);
    // No frontend yet: the timer has not started.
    assert!(srv.wait_exit(Duration::from_millis(2500)).await.is_none(), "exited before any frontend connected");
    // A --status probe is not a frontend and does not start or stop the timer.
    let out = env.command().arg("--status").output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(srv.wait_exit(Duration::from_millis(1500)).await.is_none());
    let c = Client::connect(&srv.url(), &env.work()).await;
    assert!(srv.wait_exit(Duration::from_millis(1500)).await.is_none(), "exited while a frontend is connected");
    c.close();
    let st = srv.wait_exit(Duration::from_secs(10)).await.expect("exit ~1s after the last frontend left");
    assert!(st.success());
}

#[tokio::test]
async fn watch_only_monitor_keeps_server_alive_but_is_not_foreground() {
    // A watch-only monitor is still a frontend for the idle timer (it is connected), but
    // does not make the session foreground.
    let mock = Mock::start(Arc::new(|_r: &Req| text("x"))).await;
    let env = Env::new(mock.port, json!({}));
    let mut srv = env.spawn(&["--idle-timeout", "0"]);
    let mut a = Client::connect(&srv.url(), &env.work()).await;
    let s = a.create(&env.work(), None).await;
    let mut mon = Client::connect(&srv.url(), &env.work()).await;
    mon.req(ClientMsg::WatchBackground { enabled: true }).await.unwrap();
    let (i, _) = mon.snapshot(&s.id).await;
    assert_eq!(i.attached, 1);
    a.close();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let (i, _) = mon.snapshot(&s.id).await;
    assert_eq!(i.attached, 0, "watch-only attachment is not foreground");
    assert!(srv.wait_exit(Duration::from_millis(300)).await.is_none());
    mon.close();
    assert!(srv.wait_exit(Duration::from_secs(10)).await.is_some());
}

#[tokio::test]
async fn idle_timeout_zero_rapid_reconnect_never_drops_a_client() {
    // A frontend that reconnects right after the previous one left must either get a working
    // connection (the shutdown is cancelled) or a clean refusal (HTTP 503 / connection
    // refused, after which the server exits and another one is started). An accepted client
    // must never be dropped.
    let mock = Mock::start(Arc::new(|_r: &Req| text("x"))).await;
    let env = Env::new(mock.port, json!({}));
    let mut srv = env.spawn(&["--idle-timeout", "0"]);
    let mut c = Client::connect(&srv.url(), &env.work()).await;
    let (mut kept, mut refused) = (0, 0);
    for i in 0..20u64 {
        // Odd iterations: disconnect, then reconnect after a varying gap so the reconnect
        // lands at different points of the shutdown path. Even iterations: the new socket
        // opens just before the old frontend leaves (a restarting TUI), so it must be kept.
        let attempt = if i % 2 == 0 {
            let url = srv.url();
            let work = env.work();
            let pending = tokio::spawn(async move { Client::try_connect(&url, &work).await });
            tokio::time::sleep(Duration::from_millis(20)).await;
            c.close();
            pending.await.unwrap()
        } else {
            c.close();
            tokio::time::sleep(Duration::from_millis(i % 5 * 4)).await;
            Client::try_connect(&srv.url(), &env.work()).await
        };
        match attempt {
            Ok(mut n) => {
                kept += 1;
                let r = n.try_req(ClientMsg::Ping, Duration::from_secs(5)).await;
                assert!(matches!(r, Ok(Ok(_))), "iteration {i}: accepted client got {r:?}\n{}", srv.log_text());
                tokio::time::sleep(Duration::from_millis(150)).await;
                let r = n.try_req(ClientMsg::ListSessions { scope: ListScope::Open }, Duration::from_secs(5)).await;
                assert!(matches!(r, Ok(Ok(_))), "iteration {i}: client dropped after connecting: {r:?}\n{}", srv.log_text());
                assert!(srv.child.try_wait().unwrap().is_none(), "iteration {i}: server exited with a frontend connected");
                c = n;
            }
            Err(e) => {
                refused += 1;
                assert!(e == "http 503" || e.starts_with("connect:"), "iteration {i}: unclean refusal: {e}\n{}", srv.log_text());
                let st = srv.wait_exit(Duration::from_secs(10)).await.expect("refusing server exits");
                assert!(st.success(), "{st:?}");
                srv = env.spawn(&["--idle-timeout", "0"]);
                c = Client::connect(&srv.url(), &env.work()).await;
            }
        }
    }
    eprintln!("rapid reconnect: {kept} kept, {refused} refused");
    assert!(kept >= 10, "overlapping reconnects must be kept ({kept} kept)");
    c.close();
    assert!(srv.wait_exit(Duration::from_secs(10)).await.is_some(), "exits after the last frontend");
}

#[tokio::test]
async fn idle_timeout_zero_pending_connection_cancels_shutdown() {
    // A WebSocket that is open but has not sent Hello yet when the last frontend leaves must
    // not be dropped: the shutdown waits for it, and its Hello makes it a frontend.
    use futures::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message as WsMsg;
    let mock = Mock::start(Arc::new(|_r: &Req| text("x"))).await;
    let env = Env::new(mock.port, json!({}));
    let mut srv = env.spawn(&["--idle-timeout", "0"]);
    let c = Client::connect(&srv.url(), &env.work()).await;
    let (mut raw, _) = tokio_tungstenite::connect_async(srv.url().as_str()).await.unwrap();
    c.close();
    assert!(srv.wait_exit(Duration::from_millis(1500)).await.is_none(), "exited with a connection pending\n{}", srv.log_text());
    let hello = json!({"id": 1, "type": "hello", "client": "late", "cwd": env.work(), "protocol": PROTOCOL_VERSION});
    raw.send(WsMsg::text(hello.to_string())).await.unwrap();
    let ok = loop {
        match tokio::time::timeout(Duration::from_secs(5), raw.next()).await {
            Ok(Some(Ok(WsMsg::Text(t)))) => {
                let v: serde_json::Value = serde_json::from_str(t.as_str()).unwrap();
                if v["type"] == "response" && v["id"] == 1 {
                    break v["error"].is_null();
                }
            }
            other => panic!("late client dropped: {other:?}\n{}", srv.log_text()),
        }
    };
    assert!(ok);
    assert!(srv.wait_exit(Duration::from_millis(500)).await.is_none());
    let _ = raw.close(None).await;
    drop(raw);
    let st = srv.wait_exit(Duration::from_secs(10)).await.expect("exits after the late frontend left");
    assert!(st.success());
    assert!(srv.log_text().contains("last frontend disconnected"), "{}", srv.log_text());
}
