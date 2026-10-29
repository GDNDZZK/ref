//! Context management: overflow → auto compaction → continue, threshold-based auto compaction,
//! manual compaction (idle / busy), the `compact` tool and `context_info`.

mod common;

use std::sync::atomic::Ordering;
use std::sync::Arc;

use common::*;
use ref_proto::{ClientMsg, MessageOrigin, SessionStatus};
use serde_json::json;

const SUMMARY_MARK: &str = "Summary of the conversation so far";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn overflow_compacts_and_continues_and_persists() {
    let script: Script = Arc::new(|body| {
        if is_summary_request(body) {
            return Reply::Text("SUMMARY: user said first".into());
        }
        let (_, content) = last_content(body);
        if request_contains(body, SUMMARY_MARK) {
            return Reply::Text(format!("after compaction: {content}"));
        }
        if content == "big" {
            return Reply::Status(400, "This model's maximum context length is 1000 tokens".into());
        }
        Reply::Text(format!("echo: {content}"))
    });
    let mut env = setup(script, json!({})).await;
    let mut c = Client::connect(&env).await;
    let sid = c.create(&env.cwd).await;
    c.turn(&sid, "first").await;
    c.prompt(&sid, "big").await;
    c.until_idle_with(&sid, "after compaction: big").await;
    let msgs = env.messages(&sid);
    let texts: Vec<String> = msgs.iter().map(|m| m.raw_text()).collect();
    // History kept; summary, then the prompt that overflowed (kept verbatim), then the answer.
    assert_eq!(texts[0], "first");
    assert_eq!(texts[1], "echo: first");
    assert_eq!(msgs[2].origin, MessageOrigin::Compaction);
    assert!(texts[2].contains("SUMMARY: user said first"));
    assert_eq!(texts[3], "big");
    assert_eq!(texts[4], "after compaction: big");
    assert_eq!(msgs.len(), 5);
    // The request after compaction: system + summary + prompt.
    let last = env.llm_requests().last().cloned().unwrap();
    assert_eq!(last["messages"].as_array().unwrap().len(), 3, "{}", last["messages"]);
    // Persistence keeps the re-ordered prompt (removed + re-inserted after the summary): the
    // append-only log as written so far (no graceful-shutdown rewrite yet, i.e. what a crash
    // would leave behind) already has the right content.
    let (live_disk, _) = refd::session::store::load_messages(&env.data_dir.join("sessions"), &sid);
    assert_eq!(live_disk.iter().map(|m| m.raw_text()).collect::<Vec<_>>(), texts);
    drop(c);
    env.restart().await;
    let mut c = Client::connect(&env).await;
    let (_, disk, _) = c.snapshot(&sid).await;
    let disk_texts: Vec<String> = disk.iter().map(|m| m.raw_text()).collect();
    assert_eq!(disk_texts, texts);
    env.server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn auto_compaction_near_limit_mid_turn() {
    let env = setup(cmd_script(), json!({})).await;
    let mut c = Client::connect(&env).await;
    let sid = c.create(&env.cwd).await;
    c.turn(&sid, "hello").await;
    // Next responses report a context close to the window (100000 - reserved 4000).
    env.mock.prompt_tokens.store(97_000, Ordering::Relaxed);
    c.prompt(&sid, &call(json!([["bash", {"command": "echo x"}]]))).await;
    // Step 1 runs the tool; before step 2 the context is compacted and the task continues.
    c.until_idle_with(&sid, "continued").await;
    let msgs = env.messages(&sid);
    let ci = msgs.iter().position(|m| m.origin == MessageOrigin::Compaction).expect("compaction");
    assert!(msgs[ci].raw_text().contains("SUMMARY of work"));
    assert!(msgs[ci].raw_text().ends_with("Continue the task from here."));
    assert_eq!(ci, msgs.len() - 2);
    let reqs = env.llm_requests();
    let last = reqs.last().unwrap();
    assert_eq!(last["messages"].as_array().unwrap().len(), 2, "{}", last["messages"]);
    env.server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn manual_compaction_idle_and_busy() {
    let env = setup(cmd_script(), json!({})).await;
    let mut c = Client::connect(&env).await;
    let sid = c.create(&env.cwd).await;
    // Nothing to compact yet.
    c.send(ClientMsg::Compact { session_id: sid.clone() }).await;
    c.until(|m| matches!(m, ref_proto::ServerMsg::Notice { text, .. } if text == "Nothing to compact")).await;
    c.wait_status(&sid, |s| *s == SessionStatus::Idle).await;
    c.turn(&sid, "one").await;
    // Idle: compacts right away, no continuation.
    c.send(ClientMsg::Compact { session_id: sid.clone() }).await;
    c.until(|m| matches!(m, ref_proto::ServerMsg::Notice { text, .. } if text == "Context compacted")).await;
    c.wait_status(&sid, |s| *s == SessionStatus::Idle).await;
    let msgs = env.messages(&sid);
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[2].origin, MessageOrigin::Compaction);
    assert!(!msgs[2].raw_text().contains("Continue the task"));
    let n_before = env.llm_requests().len();
    // Busy: requested during a running tool, applied at the next step boundary.
    c.prompt(&sid, &call(json!([["bash", {"command": "sleep 0.5; echo y"}]]))).await;
    c.until(|m| matches!(m, ref_proto::ServerMsg::PartUpsert { part: ref_proto::Part::Tool(t), .. } if t.state == ref_proto::ToolState::Running)).await;
    c.send(ClientMsg::Compact { session_id: sid.clone() }).await;
    c.until_idle_with(&sid, "continued").await;
    let msgs = env.messages(&sid);
    let comps: Vec<usize> =
        msgs.iter().enumerate().filter(|(_, m)| m.origin == MessageOrigin::Compaction).map(|(i, _)| i).collect();
    assert_eq!(comps.len(), 2, "{:?}", msgs.iter().map(|m| m.raw_text()).collect::<Vec<_>>());
    assert!(msgs[comps[1]].raw_text().ends_with("Continue the task from here."));
    let reqs = env.llm_requests();
    // tool step, summary, continuation.
    assert_eq!(reqs.len() - n_before, 3);
    assert!(is_summary_request(&reqs[n_before + 1]));
    env.server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn context_info_uses_configured_threshold() {
    let env = setup(cmd_script(), json!({"compaction": {"threshold": 0.5}})).await;
    let mut c = Client::connect(&env).await;
    let info = c.create_with(&env.cwd, None, None, None).await;
    assert_eq!(info.context.max, 100_000);
    assert_eq!(info.context.compact_at, 50_000);
    let sid = info.id;
    c.turn(&sid, &call(json!([["context_info", {}]]))).await;
    let t = env.tool_parts(&sid);
    // used = 110 (100 prompt + 10 completion reported by the mock).
    assert_eq!(t[0].output, "Context: used 110 tokens of 100000 (0%). Recommended compaction at 50000 tokens.");
    // Absolute threshold, and per-model `limit.compact` override.
    env.write_config(&json!({
        "model": "mock/m",
        "compaction": {"threshold": 30000},
        "provider": {"mock": {"npm": "@ai-sdk/openai-compatible", "name": "Mock",
            "options": {"baseURL": env.mock_base(), "apiKey": "sk-test"},
            "models": {"m": {"name": "M", "limit": {"context": 100000, "output": 4000}},
                       "small": {"name": "S", "limit": {"context": 50000, "output": 2000, "compact": 12345}}}}}
    }));
    c.send(ClientMsg::Reload).await;
    ref_test_wait(&env, &sid, 30_000).await;
    c.send(ClientMsg::SetModel { session_id: sid.clone(), model: "mock/small".into(), variant: None }).await;
    c.until(|m| matches!(m, ref_proto::ServerMsg::SessionUpdated { info } if info.context.compact_at == 12345)).await;
    env.server.shutdown();
}

async fn ref_test_wait(env: &Env, sid: &str, compact_at: u64) {
    let core = env.core(sid);
    wait_until(std::time::Duration::from_secs(5), || core.info().context.compact_at == compact_at).await;
}

trait MockBase {
    fn mock_base(&self) -> String;
}

impl MockBase for Env {
    fn mock_base(&self) -> String {
        let cfg: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(self.paths.global_dir.join("opencode.json")).unwrap()).unwrap();
        cfg["provider"]["mock"]["options"]["baseURL"].as_str().unwrap().to_string()
    }
}
