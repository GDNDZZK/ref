//! Retry behavior: HTTP errors, Retry-After, mid-stream failures, network errors, backoff
//! cap, abort during backoff, persistence of failed partial messages.

mod common;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use ref_proto::{ClientMsg, ServerMsg, SessionStatus};
use serde_json::json;

fn counter() -> Arc<AtomicU32> {
    Arc::new(AtomicU32::new(0))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rate_limit_retry_after_respected_and_capped() {
    let n = counter();
    let n2 = n.clone();
    let script: Script = Arc::new(move |_| match n2.fetch_add(1, Ordering::SeqCst) {
        0 => Reply::StatusWith(429, vec![("retry-after", "2".into())], "slow down".into()),
        1 => Reply::StatusWith(429, vec![("retry-after", "30".into())], "slow down more".into()),
        _ => Reply::Text("finally".into()),
    });
    // Max delay 3s: the second Retry-After (30s) is capped.
    let env = setup(script, json!({"retry_max_delay": 3})).await;
    let mut c = Client::connect(&env).await;
    let sid = c.create(&env.cwd).await;
    c.prompt(&sid, "hi").await;
    let i1 = c.wait_status(&sid, |s| matches!(s, SessionStatus::Retrying { .. })).await;
    let now = ref_proto::now_ms();
    let SessionStatus::Retrying { attempt, at, error } = i1.status else { unreachable!() };
    assert_eq!(attempt, 1);
    assert!(error.contains("429") && error.contains("slow down"), "{error}");
    assert!(at - now >= 1800 && at - now <= 2200, "Retry-After 2s not respected: {}", at - now);
    let i2 = c.wait_status(&sid, |s| matches!(s, SessionStatus::Retrying { attempt: 2, .. })).await;
    let now = ref_proto::now_ms();
    let SessionStatus::Retrying { at, .. } = i2.status else { unreachable!() };
    assert!(at - now <= 3100, "delay not capped by retry_max_delay: {}", at - now);
    c.until_idle_with(&sid, "finally").await;
    assert_eq!(env.messages(&sid).len(), 2, "failed attempts must leave no messages");
    env.server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn backoff_attempts_increase_and_abort_is_immediate() {
    let script: Script = Arc::new(|_| Reply::Status(500, "boom".into()));
    let env = setup(script, json!({})).await;
    let mut c = Client::connect(&env).await;
    let sid = c.create(&env.cwd).await;
    c.prompt(&sid, "hi").await;
    let i = c.wait_status(&sid, |s| matches!(s, SessionStatus::Retrying { attempt: 2, .. })).await;
    let SessionStatus::Retrying { at, .. } = i.status else { unreachable!() };
    // 2nd attempt waits ~2s (exponential: 1s, 2s, ...).
    let wait = at - ref_proto::now_ms();
    assert!((1600..=2300).contains(&wait), "{wait}");
    let t = Instant::now();
    c.send(ClientMsg::Abort { session_id: sid.clone() }).await;
    c.wait_status(&sid, |s| *s == SessionStatus::Idle).await;
    assert!(t.elapsed() < Duration::from_millis(500), "abort took {:?}", t.elapsed());
    // No further request after the abort.
    let before = env.llm_requests().len();
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(env.llm_requests().len(), before);
    // History: the user prompt only.
    let msgs = env.messages(&sid);
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    env.server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mid_stream_failure_retries_and_cleans_up_partial() {
    let n = counter();
    let n2 = n.clone();
    let script: Script = Arc::new(move |_| {
        if n2.fetch_add(1, Ordering::SeqCst) == 0 {
            // Partial text over ~3.6s (so it is persisted mid-stream), then the connection breaks.
            let mut events = text_events("partial answer that will be lost");
            events.truncate(10);
            Reply::Stream { events, delay: Duration::from_millis(400), fail: true }
        } else {
            Reply::Text("complete answer".into())
        }
    });
    let mut env = setup(script, json!({})).await;
    let mut c = Client::connect(&env).await;
    let sid = c.create(&env.cwd).await;
    c.prompt(&sid, "hi").await;
    let i = c.wait_status(&sid, |s| matches!(s, SessionStatus::Retrying { .. })).await;
    let SessionStatus::Retrying { attempt, error, .. } = i.status else { unreachable!() };
    assert_eq!(attempt, 1);
    assert!(!error.is_empty());
    // The partial message was streamed to the client, then removed.
    assert!(c.log.iter().any(|m| matches!(m, ServerMsg::PartDelta { .. } | ServerMsg::PartUpsert { .. })));
    assert!(c.log.iter().any(|m| matches!(m, ServerMsg::MessageRemoved { .. })));
    // On disk (before the retry succeeds) only the user prompt remains.
    let (disk, _) = refd::session::store::load_messages(&env.data_dir.join("sessions"), &sid);
    assert_eq!(disk.len(), 1, "partial message still persisted: {disk:?}");
    c.until_idle_with(&sid, "complete answer").await;
    let msgs = env.messages(&sid);
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[1].raw_text(), "complete answer");
    // Survives a restart unchanged.
    drop(c);
    env.restart().await;
    let mut c = Client::connect(&env).await;
    let (_, msgs, _) = c.snapshot(&sid).await;
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[1].raw_text(), "complete answer");
    env.server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn network_error_retries_and_model_switch_wakes_retry() {
    let env = setup(
        Arc::new(|_| Reply::Text("via mock".into())),
        json!({"provider": {"dead": {"npm": "@ai-sdk/openai-compatible", "name": "Dead",
            "options": {"baseURL": "http://127.0.0.1:1/v1", "apiKey": "x"},
            "models": {"d": {"name": "Dead", "limit": {"context": 1000, "output": 100}}}}}}),
    )
    .await;
    let mut c = Client::connect(&env).await;
    let sid = c.create_with(&env.cwd, None, Some("dead/d"), None).await.id;
    c.prompt(&sid, "hi").await;
    let i = c.wait_status(&sid, |s| matches!(s, SessionStatus::Retrying { attempt: 2, .. })).await;
    let SessionStatus::Retrying { error, .. } = i.status else { unreachable!() };
    // Connection refused (or a 502 from an HTTP proxy configured in the environment).
    assert!(!error.is_empty());
    // Switching the model wakes the backoff immediately; the retry uses the new model.
    let t = Instant::now();
    c.send(ClientMsg::SetModel { session_id: sid.clone(), model: "mock/m".into(), variant: None }).await;
    c.until_idle_with(&sid, "via mock").await;
    assert!(t.elapsed() < Duration::from_millis(1500), "{:?}", t.elapsed());
    env.server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stalled_stream_times_out_and_retries() {
    let n = counter();
    let n2 = n.clone();
    let script: Script = Arc::new(move |_| {
        if n2.fetch_add(1, Ordering::SeqCst) == 0 {
            Reply::Hang
        } else {
            Reply::Text("after timeout".into())
        }
    });
    let env = setup(script, json!({"provider": {"mock": {"options": {"headerTimeout": 500}}}})).await;
    let mut c = Client::connect(&env).await;
    let sid = c.create(&env.cwd).await;
    c.prompt(&sid, "hi").await;
    let i = c.wait_status(&sid, |s| matches!(s, SessionStatus::Retrying { .. })).await;
    let SessionStatus::Retrying { error, .. } = i.status else { unreachable!() };
    assert!(error.contains("timeout"), "{error}");
    c.until_idle_with(&sid, "after timeout").await;
    env.server.shutdown();
}
