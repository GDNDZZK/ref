use super::*;
use crate::test_support::{conversation, model, provider};
use crate::util::replay;
use crate::{ApiKind, StreamEvent};

fn deepseek() -> Provider {
    provider(
        "deepseek",
        ApiKind::OpenAIChat,
        "https://api.deepseek.com/v1/",
    )
}

#[test]
fn body_with_tools_images_and_reasoning_replay() {
    let p = deepseek();
    let m = model(
        "deepseek-v4-pro",
        json!({"reasoningEffort": "high", "thinking": {"type": "enabled"}}),
    );
    let req = conversation(
        ("deepseek", "deepseek-v4-pro"),
        Some(json!({"field": "reasoning_content"})),
        None,
    );
    let body = build_body(&p, &m, &req);

    assert_eq!(body["model"], "deepseek-v4-pro");
    assert_eq!(body["stream"], true);
    assert_eq!(body["stream_options"]["include_usage"], true);
    assert_eq!(body["max_tokens"], 8192);
    assert_eq!(body["reasoning_effort"], "high");
    assert_eq!(body["thinking"], json!({"type": "enabled"}));
    assert!(body.get("prompt_cache_key").is_none());
    assert_eq!(body["tools"][0]["type"], "function");
    assert_eq!(body["tools"][0]["function"]["name"], "read");

    let msgs = body["messages"].as_array().unwrap();
    assert_eq!(
        msgs[0],
        json!({"role": "system", "content": "You are a coding agent.\n\ncwd: /tmp"})
    );
    assert_eq!(
        msgs[1]["content"][1]["image_url"]["url"],
        "data:image/png;base64,iVBORw0KGgo="
    );
    let asst = &msgs[2];
    assert_eq!(asst["content"], "Reading it.");
    assert_eq!(asst["reasoning_content"], "Let me read the file.");
    assert_eq!(asst["tool_calls"][0]["id"], "call_1");
    assert_eq!(
        asst["tool_calls"][0]["function"]["arguments"],
        r#"{"path":"a.rs"}"#
    );
    assert_eq!(
        msgs[3],
        json!({"role": "tool", "tool_call_id": "call_1", "content": "fn main() {}"})
    );
    // Tool image as a synthetic user message, then the next user message merged into it.
    assert_eq!(msgs[4]["role"], "user");
    let parts = msgs[4]["content"].as_array().unwrap();
    assert_eq!(parts[0]["text"], "Attached image(s) from tool result:");
    assert_eq!(parts[1]["type"], "image_url");
    assert_eq!(parts[2], json!({"type": "text", "text": "thanks"}));
    assert_eq!(msgs.len(), 5);
}

#[test]
fn reasoning_dropped_for_foreign_model_but_deepseek_field_kept_empty() {
    let p = deepseek();
    let m = model("deepseek-v4-pro", json!({}));
    let req = conversation(
        ("anthropic", "claude-opus-4-6"),
        Some(json!({"signature": "x"})),
        None,
    );
    let body = build_body(&p, &m, &req);
    assert_eq!(body["messages"][2]["reasoning_content"], "");

    let p = provider(
        "glm",
        ApiKind::OpenAIChat,
        "https://open.bigmodel.cn/api/paas/v4",
    );
    let m = model("glm-5", json!({}));
    let body = build_body(&p, &m, &req);
    assert!(body["messages"][2].get("reasoning_content").is_none());
}

#[test]
fn recorded_reasoning_field_is_reused() {
    let p = provider(
        "openrouter",
        ApiKind::OpenAIChat,
        "https://openrouter.ai/api/v1",
    );
    let m = model("qwen/qwen3", json!({}));
    let req = conversation(
        ("openrouter", "qwen/qwen3"),
        Some(json!({"field": "reasoning"})),
        None,
    );
    let body = build_body(&p, &m, &req);
    assert_eq!(body["messages"][2]["reasoning"], "Let me read the file.");
    assert!(body["messages"][2].get("reasoning_content").is_none());
}

#[test]
fn options_mapping() {
    let mut p = provider("openai", ApiKind::OpenAIChat, "https://api.openai.com/v1");
    p.set_cache_key = true;
    let mut m = model(
        "gpt-4.1",
        json!({
            "textVerbosity": "low", "store": false, "include": ["x"], "reasoningSummary": "auto",
            "promptCacheKey": "nope", "setCacheKey": true,
            "thinking": {"type": "enabled", "budgetTokens": 1000},
            "chat_template_kwargs": {"enable_thinking": true},
            "extraBody": {"chat_template_kwargs": {"preserve_thinking": true}, "foo": 1},
        }),
    );
    m.temperature = Some(0.2);
    m.top_p = Some(0.9);
    let body = build_body(&p, &m, &conversation(("x", "y"), None, None));
    assert_eq!(body["verbosity"], "low");
    assert_eq!(body["store"], false);
    assert_eq!(body["prompt_cache_key"], "ses_123");
    assert_eq!(body["max_completion_tokens"], 8192);
    assert_eq!(
        body["thinking"],
        json!({"type": "enabled", "budget_tokens": 1000})
    );
    assert_eq!(
        body["chat_template_kwargs"],
        json!({"enable_thinking": true, "preserve_thinking": true})
    );
    assert_eq!(body["foo"], 1);
    assert_eq!(body["temperature"], 0.2);
    assert_eq!(body["top_p"], 0.9);
    for k in [
        "include",
        "reasoningSummary",
        "promptCacheKey",
        "setCacheKey",
        "extraBody",
        "textVerbosity",
    ] {
        assert!(body.get(k).is_none(), "{k}");
    }

    // `store` is only sent to the official OpenAI API.
    let p = deepseek();
    let body = build_body(&p, &m, &conversation(("x", "y"), None, None));
    assert!(body.get("store").is_none());
}

#[test]
fn headers_include_auth_and_session() {
    let mut p = deepseek();
    p.headers.insert("X-Session-Id".into(), "override".into());
    let req = conversation(("x", "y"), None, None);
    let h = request_headers(&p, &req);
    assert_eq!(h["authorization"], "Bearer sk-test");
    assert_eq!(h["accept"], "text/event-stream");
    assert_eq!(h["session_id"], "ses_123");
    assert_eq!(h["x-session-affinity"], "ses_123");
    assert_eq!(h["x-session-id"], "override");
}

const DEEPSEEK_STREAM: &str = concat!(
    ": keep-alive\r\n\r\n",
    "data: {\"id\":\"1\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"reasoning_content\":\"Think\"},\"finish_reason\":null}]}\r\n\r\n",
    "data: {\"id\":\"1\",\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"ing.\"},\"finish_reason\":null}]}\r\n\r\n",
    "data: {\"id\":\"1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"I'll read \"},\"finish_reason\":null}]}\r\n\r\n",
    "data: {\"id\":\"1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"it.\"},\"finish_reason\":null}]}\r\n\r\n",
    "data: {\"id\":\"1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"read\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\r\n\r\n",
    "data: {\"id\":\"1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"path\\\":\"}}]},\"finish_reason\":null}]}\r\n\r\n",
    "data: {\"id\":\"1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"call_b\",\"type\":\"function\",\"function\":{\"name\":\"ls\"}}]},\"finish_reason\":null}]}\r\n\r\n",
    "data: {\"id\":\"1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"a.rs\\\"}\"}}]},\"finish_reason\":null}]}\r\n\r\n",
    "data: {\"id\":\"1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\r\n\r\n",
    "data: {\"id\":\"1\",\"choices\":[],\"usage\":{\"prompt_tokens\":1000,\"completion_tokens\":50,\"prompt_cache_hit_tokens\":800,\"prompt_cache_miss_tokens\":200,\"completion_tokens_details\":{\"reasoning_tokens\":20}}}\r\n\r\n",
    "data: [DONE]\r\n\r\n",
);

#[test]
fn parse_deepseek_stream() {
    let (res, events) = replay(ChatState::default(), DEEPSEEK_STREAM.as_bytes());
    let res = res.unwrap();
    assert_eq!(res.stop_reason, "tool_calls");
    assert_eq!(
        res.usage,
        Usage {
            input: 200,
            output: 50,
            cache_read: 800,
            cache_write: 0,
            reasoning: 20
        }
    );
    assert_eq!(res.blocks.len(), 4);
    assert!(
        matches!(&res.blocks[0], AssistantBlock::Reasoning { text, provider_data: Some(d) }
        if text == "Thinking." && d["field"] == "reasoning_content")
    );
    assert!(matches!(&res.blocks[1], AssistantBlock::Text(t) if t == "I'll read it."));
    assert!(
        matches!(&res.blocks[2], AssistantBlock::ToolCall { id, name, arguments, .. }
        if id == "call_a" && name == "read" && arguments == r#"{"path":"a.rs"}"#)
    );
    assert!(
        matches!(&res.blocks[3], AssistantBlock::ToolCall { id, name, arguments, .. }
        if id == "call_b" && name == "ls" && arguments == "{}")
    );

    // Every block starts before its deltas and ends exactly once.
    let starts: Vec<usize> = events
        .iter()
        .filter_map(|e| {
            if let StreamEvent::BlockStart { index, .. } = e {
                Some(*index)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(starts, vec![0, 1, 2, 3]);
    let ends = events
        .iter()
        .filter(|e| matches!(e, StreamEvent::BlockEnd { .. }))
        .count();
    assert_eq!(ends, 4);
    assert!(matches!(
        events[0],
        StreamEvent::BlockStart {
            index: 0,
            kind: BlockKind::Reasoning
        }
    ));
    assert!(matches!(&events[1], StreamEvent::Delta { index: 0, text } if text == "Think"));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, StreamEvent::Usage(u) if u.cache_read == 800))
    );
}

#[test]
fn stream_without_finish_reason_is_retryable_error() {
    let s = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"}}]}\n\n";
    let (res, _) = replay(ChatState::default(), s.as_bytes());
    let err = res.unwrap_err();
    assert!(err.retryable);
}

#[test]
fn openai_usage_and_length() {
    let s = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":\"length\"}]}\n\n",
        "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":5,\"prompt_tokens_details\":{\"cached_tokens\":64}}}\n\n",
        "data: [DONE]\n\n",
    );
    let (res, _) = replay(ChatState::default(), s.as_bytes());
    let res = res.unwrap();
    assert_eq!(res.stop_reason, "length");
    assert_eq!(res.usage.input, 36);
    assert_eq!(res.usage.cache_read, 64);
}

#[test]
fn in_stream_error_chunk() {
    let s = "data: {\"error\":{\"message\":\"Server overloaded, try again\",\"code\":\"server_error\"}}\n\n";
    let (res, _) = replay(ChatState::default(), s.as_bytes());
    let err = res.unwrap_err();
    assert!(err.retryable);
    assert!(err.message.contains("overloaded"));
}

#[test]
fn reasoning_field_alias_and_tool_without_index() {
    let s = concat!(
        "data: {\"choices\":[{\"delta\":{\"reasoning\":\"r\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"id\":\"c1\",\"function\":{\"name\":\"x\",\"arguments\":\"{\\\"a\\\"\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"function\":{\"arguments\":\":1}\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n",
    );
    let (res, _) = replay(ChatState::default(), s.as_bytes());
    let res = res.unwrap();
    assert_eq!(res.stop_reason, "tool_calls");
    assert!(
        matches!(&res.blocks[0], AssistantBlock::Reasoning { provider_data: Some(d), .. } if d["field"] == "reasoning")
    );
    assert!(
        matches!(&res.blocks[1], AssistantBlock::ToolCall { arguments, .. } if arguments == r#"{"a":1}"#)
    );
}
