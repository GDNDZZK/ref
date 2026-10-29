//! Prompt-cache friendliness: every request must be a byte-exact extension of the previous
//! one (system, tools, options and the message prefix), across steps and turns.
mod common;

use common::*;
use hn_proto::*;
use serde_json::{json, Value};
use std::sync::Arc;

/// Remove Anthropic `cache_control` markers (breakpoints move forward by design; they do not
/// change the cached content).
fn strip_cache_control(v: &Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(m.iter().filter(|(k, _)| *k != "cache_control").map(|(k, v)| (k.clone(), strip_cache_control(v))).collect()),
        Value::Array(a) => Value::Array(a.iter().map(strip_cache_control).collect()),
        other => other.clone(),
    }
}

/// Assert `next` extends `prev`: identical non-message fields and `prev.messages` is an exact
/// prefix of `next.messages` (compared as serialized JSON text, i.e. byte-for-byte).
fn assert_prefix(prev: &Value, next: &Value, what: &str) {
    let (p, n) = (strip_cache_control(prev), strip_cache_control(next));
    let (Value::Object(po), Value::Object(no)) = (&p, &n) else { panic!("bodies must be objects") };
    for (k, v) in po {
        if k == "messages" {
            continue;
        }
        assert_eq!(v.to_string(), no.get(k).map(|x| x.to_string()).unwrap_or_default(), "{what}: field {k} changed");
    }
    assert_eq!(po.len(), no.len(), "{what}: top-level fields differ");
    let pm = po["messages"].as_array().unwrap();
    let nm = no["messages"].as_array().unwrap();
    assert!(nm.len() > pm.len(), "{what}: history did not grow");
    for (i, (a, b)) in pm.iter().zip(nm).enumerate() {
        // For Anthropic, the last user message of `prev` may be merged with later tool results
        // only if it was a tool_result-only message; any other change breaks the cache.
        assert_eq!(a.to_string(), b.to_string(), "{what}: message {i} changed between requests");
    }
}

fn script() -> Script {
    Arc::new(|r: &Req| {
        if r.last_role() == "user" && r.last_user().contains("first") {
            // Non-alphabetical key order must survive replay.
            return Reply::Tools(vec![("bash".into(), r#"{"timeout":30,"purpose":"list","command":"echo hi"}"#.into())]);
        }
        if r.last_role() == "tool" {
            return text("step done");
        }
        text("ok")
    })
}

async fn run(model: Option<&str>, extra: Value) -> Vec<Value> {
    let mock = Mock::start(script()).await;
    let mut cfg = json!({"provider": {
        "mock": {"npm": "@ai-sdk/openai-compatible",
            "options": {"baseURL": format!("http://127.0.0.1:{}/v1", mock.port), "apiKey": "k"},
            "models": {"m": {"limit": {"context": 100000, "output": 1000}}}},
        "mocka": anthropic_provider(mock.port)}});
    if let (Value::Object(c), Value::Object(e)) = (&mut cfg, extra) {
        c.extend(e);
    }
    let env = Env::new(mock.port, cfg);
    let srv = env.spawn(&[]);
    let mut c = Client::connect(&srv.url(), &env.work()).await;
    let v = c
        .req(ClientMsg::CreateSession { cwd: env.work(), agent: None, model: model.and_then(ModelRef::parse), name: None })
        .await
        .unwrap();
    let info: SessionInfo = serde_json::from_value(v).unwrap();
    c.prompt(&info.id, "first request").await;
    c.wait_idle(&info.id, T).await;
    // Volatile state between turns must not leak into the prompt.
    let titled = |m: &ServerMsg| matches!(m, ServerMsg::Sessions { sessions } if sessions.iter().any(|s| s.name == "Mock title"));
    if !c.seen.iter().any(titled) {
        c.wait_for("title", T, titled).await;
    }
    c.req(ClientMsg::Rename { session: info.id.clone(), name: "重命名 session".into() }).await.unwrap();
    c.prompt(&info.id, "second request").await;
    c.wait_idle(&info.id, T).await;
    c.prompt(&info.id, "third request").await;
    c.wait_idle(&info.id, T).await;
    let reqs = mock.requests();
    assert_eq!(reqs.len(), 4, "{reqs:#?}");
    for i in 1..reqs.len() {
        assert_prefix(&reqs[i - 1], &reqs[i], &format!("request {i}"));
    }
    reqs
}

#[tokio::test]
async fn chat_requests_are_prefix_stable() {
    let reqs = run(None, json!({})).await;
    let replayed = reqs[1]["messages"].as_array().unwrap().iter().find(|m| m["tool_calls"].is_array()).unwrap();
    assert_eq!(replayed["tool_calls"][0]["function"]["arguments"], r#"{"timeout":30,"purpose":"list","command":"echo hi"}"#);
}

#[tokio::test]
async fn anthropic_requests_are_prefix_stable_with_thinking_replay() {
    let reqs = run(Some("mocka/claude-sonnet-5-5#high"), json!({})).await;
    let first = &reqs[0];
    assert_eq!(first["thinking"], json!({"type": "adaptive", "display": "summarized"}));
    assert_eq!(first["output_config"], json!({"effort": "high"}));
    // Breakpoints: the shared system prefix (before the per-session block) and the last
    // system block; the session id only appears in the last block.
    let sys = first["system"].as_array().unwrap();
    assert!(sys.len() >= 2);
    assert!(sys[sys.len() - 1].get("cache_control").is_some());
    assert!(sys[sys.len() - 2].get("cache_control").is_some());
    let sid = Req { body: first.clone(), n: 0 }.session_id().unwrap();
    assert!(sys[..sys.len() - 1].iter().all(|b| !b.to_string().contains(&sid)));
    // The signed thinking block is replayed in front of the tool_use, with input key order kept.
    let a = &reqs[1]["messages"][1]["content"];
    assert_eq!(a[0]["type"], "thinking");
    assert!(a[0]["signature"].as_str().unwrap().starts_with("sig-"), "{a}");
    assert!(a[0]["thinking"].as_str().unwrap().starts_with("thinking-"), "{a}");
    assert_eq!(a[1]["type"], "tool_use");
    let keys: Vec<&String> = a[1]["input"].as_object().unwrap().keys().collect();
    assert_eq!(keys, ["timeout", "purpose", "command"]);
}
