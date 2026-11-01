//! The key handshake and the sealed frames on `/ws`, seen from a browser's side.

mod common;

use std::time::{Duration, Instant};

use common::channel::{Kind, MAC_LEN};
use common::*;
use futures_util::{SinkExt as _, StreamExt as _};
use tokio_tungstenite::tungstenite::Message;

const BACKEND_TOKEN: &str = "backend-secret";
const WRONG_KEY: &str = "this is not the key at all";
/// The pause before a wrong key is answered is one second.
const AT_LEAST_THE_PAUSE: Duration = Duration::from_millis(900);
/// Time for a wrongly connected backend to show up, if it was going to.
const SETTLE: Duration = Duration::from_millis(400);
/// More than a handshake message may hold (1 KiB).
const TOO_LONG: usize = 2000;
const MAX_HANDSHAKES: usize = 16;
/// The handshake timeout is 10 seconds.
const PAST_THE_TIMEOUT: Duration = Duration::from_secs(15);
const BAD_MESSAGE: u16 = 4400;
const KEY_REJECTED: u16 = 4401;
const HANDSHAKE_TIMEOUT: u16 = 4408;
const BAD_FRAME_REASON: &str = "bad frame";

async fn web_with_fake_upstream(env: &Env) -> (Web, FakeUpstream) {
    let upstream = fake_upstream().await;
    let server = format!("ws://{LOOPBACK}:{}/", upstream.port);
    let web = env
        .start(&["--server", &server, "--token", BACKEND_TOKEN])
        .await;
    (web, upstream)
}

fn backend_was_contacted(upstream: &FakeUpstream) -> bool {
    upstream.handshake_query.borrow().is_some()
}

#[tokio::test]
async fn a_wrong_key_is_refused_after_a_pause_and_never_reaches_the_backend() {
    let env = Env::new("wrong-key");
    let (web, upstream) = web_with_fake_upstream(&env).await;
    let started = Instant::now();
    let closed = Channel::sign_in(web.ws().await, WRONG_KEY).await.err();
    assert_eq!(closed, Some((KEY_REJECTED, "key rejected".to_string())));
    assert!(
        started.elapsed() >= AT_LEAST_THE_PAUSE,
        "{:?}",
        started.elapsed()
    );
    tokio::time::sleep(SETTLE).await;
    assert!(!backend_was_contacted(&upstream));
}

#[tokio::test]
async fn nothing_reaches_the_backend_before_the_handshake_ends() {
    let env = Env::new("no-auth");
    let (web, upstream) = web_with_fake_upstream(&env).await;
    let mut ws = web.ws().await;
    let hello = next_text(&mut ws).await;
    assert!(channel::parse_hello(&hello).is_some(), "{hello}");
    tokio::time::sleep(SETTLE).await;
    assert!(!backend_was_contacted(&upstream), "connected without a key");

    let (root, sn, cn) = (
        channel::Root::derive(web.key.as_bytes(), &channel::parse_hello(&hello).unwrap().0),
        channel::parse_hello(&hello).unwrap().1,
        [7; channel::NONCE_LEN],
    );
    let auth = channel::auth_json(&cn, &root.auth_mac(&sn, &cn));
    ws.send(Message::text(auth)).await.unwrap();
    // Frame 0 comes once the backend is connected.
    assert!(matches!(next_message(&mut ws).await, Message::Binary(_)));
    assert!(backend_was_contacted(&upstream));
}

#[tokio::test]
async fn a_recorded_auth_does_not_work_on_a_new_connection() {
    let env = Env::new("replay");
    let (web, _upstream) = web_with_fake_upstream(&env).await;

    // The first connection signs in; an eavesdropper records its auth message.
    let mut first = web.ws().await;
    let (root, sn, cn) = Channel::start_handshake(&mut first, &web.key).await;
    let recorded = channel::auth_json(&cn, &root.auth_mac(&sn, &cn));
    first.send(Message::text(recorded.clone())).await.unwrap();
    assert!(matches!(next_message(&mut first).await, Message::Binary(_)));

    // A new connection has a new server nonce, so the same message fails.
    let mut second = web.ws().await;
    let hello = next_text(&mut second).await;
    assert_ne!(channel::parse_hello(&hello).unwrap().1, sn);
    second.send(Message::text(recorded)).await.unwrap();
    match next_message(&mut second).await {
        Message::Close(Some(frame)) => assert_eq!(u16::from(frame.code), KEY_REJECTED),
        other => panic!("expected a close, got {other:?}"),
    }
}

#[tokio::test]
async fn frames_that_were_changed_repeated_or_reordered_close_the_tunnel() {
    let env = Env::new("tamper");
    let (web, _upstream) = web_with_fake_upstream(&env).await;
    let bad_frame = (BAD_MESSAGE, BAD_FRAME_REASON.to_string());

    let mut changed = web.channel().await;
    let mut wire = changed.seal(Kind::Text, b"changed");
    wire[0] ^= 1;
    changed.send_wire(wire).await;
    assert_eq!(changed.next_close().await, bad_frame);

    let mut repeated = web.channel().await;
    let wire = repeated.seal(Kind::Text, b"once");
    repeated.send_wire(wire.clone()).await;
    assert_eq!(repeated.next_text().await, "once");
    repeated.send_wire(wire).await;
    assert_eq!(repeated.next_close().await, bad_frame);

    let mut reordered = web.channel().await;
    let first = reordered.seal(Kind::Text, b"first");
    let second = reordered.seal(Kind::Text, b"second");
    reordered.send_wire(second).await;
    assert_eq!(reordered.next_close().await, bad_frame);
    drop(first);

    let mut truncated = web.channel().await;
    let wire = truncated.seal(Kind::Text, b"cut");
    truncated.send_wire(wire[..wire.len() - 1].to_vec()).await;
    assert_eq!(truncated.next_close().await, bad_frame);

    let mut plain = web.channel().await;
    plain.ws.send(Message::text("not sealed")).await.unwrap();
    assert_eq!(plain.next_close().await, bad_frame);

    // A frame sealed under another key does not open.
    let mut wrong_key = web.channel().await;
    let mut other = common::channel::Root::derive(WRONG_KEY.as_bytes(), &[0; 16])
        .client_channel(&[0; 16], &[0; 16])
        .0;
    wrong_key.send_wire(other.seal(Kind::Text, b"x")).await;
    assert_eq!(wrong_key.next_close().await, bad_frame);
}

#[tokio::test]
async fn text_and_binary_frames_keep_their_kind() {
    let env = Env::new("kinds");
    let (web, _upstream) = web_with_fake_upstream(&env).await;
    let mut tab = web.channel().await;
    tab.send_text("text").await;
    tab.send_binary(b"binary").await;
    tab.send_text("").await;
    assert_eq!(tab.next_event().await, Event::Text("text".into()));
    assert_eq!(tab.next_event().await, Event::Binary(b"binary".to_vec()));
    assert_eq!(tab.next_event().await, Event::Text(String::new()));
}

#[tokio::test]
async fn malformed_handshake_messages_close_with_4400() {
    let env = Env::new("bad-hs");
    let (web, upstream) = web_with_fake_upstream(&env).await;
    let valid_cn = "00".repeat(channel::NONCE_LEN);
    let short_mac = "00".repeat(MAC_LEN - 1);
    let attempts = [
        Message::text("not json"),
        Message::text(r#"{"t":"auth"}"#),
        Message::text(format!(r#"{{"t":"auth","cn":"00","mac":"{short_mac}"}}"#)),
        Message::text(format!(r#"{{"t":"hi","cn":"{valid_cn}","mac":"00"}}"#)),
        Message::text("x".repeat(TOO_LONG)),
        Message::binary(vec![1, 2, 3]),
    ];
    for attempt in attempts {
        let mut ws = web.ws().await;
        let _hello = next_text(&mut ws).await;
        ws.send(attempt.clone()).await.unwrap();
        match next_message(&mut ws).await {
            Message::Close(Some(frame)) => {
                assert_eq!(u16::from(frame.code), BAD_MESSAGE, "{attempt:?}");
            }
            other => panic!("expected a close for {attempt:?}, got {other:?}"),
        }
    }
    // A well-formed message with a wrong MAC is a rejected key, not a malformed one.
    let mut ws = web.ws().await;
    let _hello = next_text(&mut ws).await;
    let wrong_mac = "11".repeat(MAC_LEN);
    ws.send(Message::text(format!(
        r#"{{"t":"auth","cn":"{valid_cn}","mac":"{wrong_mac}"}}"#
    )))
    .await
    .unwrap();
    match next_message(&mut ws).await {
        Message::Close(Some(frame)) => assert_eq!(u16::from(frame.code), KEY_REJECTED),
        other => panic!("expected a close, got {other:?}"),
    }
    assert!(!backend_was_contacted(&upstream));
}

#[tokio::test]
async fn a_connection_that_never_answers_the_hello_times_out() {
    let env = Env::new("hs-timeout");
    let (web, upstream) = web_with_fake_upstream(&env).await;
    let mut ws = web.ws().await;
    let _hello = next_text(&mut ws).await;
    let closed = tokio::time::timeout(PAST_THE_TIMEOUT, ws.next()).await;
    match closed.expect("the server never gave up").unwrap().unwrap() {
        Message::Close(Some(frame)) => {
            assert_eq!(u16::from(frame.code), HANDSHAKE_TIMEOUT);
            assert_eq!(frame.reason.as_str(), "handshake timed out");
        }
        other => panic!("expected a close, got {other:?}"),
    }
    assert!(!backend_was_contacted(&upstream));
}

#[tokio::test]
async fn only_sixteen_handshakes_run_at_once() {
    let env = Env::new("hs-limit");
    let (web, _upstream) = web_with_fake_upstream(&env).await;
    let mut waiting = Vec::new();
    for _ in 0..MAX_HANDSHAKES {
        waiting.push(web.ws().await);
    }
    let refused = web.try_ws(Some(web.origin()), None).await.err().unwrap();
    match refused {
        tokio_tungstenite::tungstenite::Error::Http(response) => {
            assert_eq!(response.status().as_u16(), 503);
        }
        other => panic!("expected an HTTP rejection, got {other:?}"),
    }
    // Slots free up when handshakes end, and a signed-in tab does not hold one.
    drop(waiting);
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut tab = loop {
        match web.try_ws(Some(web.origin()), None).await {
            Ok(ws) => break ws,
            Err(_) if Instant::now() < deadline => tokio::time::sleep(POLL_FAST).await,
            Err(e) => panic!("no slot came free: {e}"),
        }
    };
    let _ = next_text(&mut tab).await;
    drop(tab);
    let mut signed_in = Vec::new();
    for _ in 0..MAX_HANDSHAKES {
        signed_in.push(web.channel().await);
    }
    assert_eq!(signed_in.len(), MAX_HANDSHAKES);
}
