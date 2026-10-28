use super::*;
use crate::test_support::{conversation, model, provider};
use crate::util::replay;
use crate::{ApiKind, StreamEvent};

fn anthropic() -> Provider {
    provider(
        "anthropic",
        ApiKind::Anthropic,
        "https://api.anthropic.com/v1/",
    )
}

fn has_cache(v: &Value) -> bool {
    v.get("cache_control").is_some()
}

#[test]
fn versions() {
    assert_eq!(claude_version("claude-opus-4-6"), Some((4, 6)));
    assert_eq!(claude_version("claude-sonnet-4.6"), Some((4, 6)));
    assert_eq!(claude_version("claude-opus-5-5"), Some((5, 5)));
    assert_eq!(claude_version("anthropic/claude-haiku-5.5"), Some((5, 5)));
    assert_eq!(claude_version("claude-sonnet-4-5-20250929"), Some((4, 5)));
    assert_eq!(claude_version("claude-opus-4-20250514"), Some((4, 0)));
    assert_eq!(claude_version("claude-3-7-sonnet-20250219"), Some((3, 7)));
    assert_eq!(
        claude_version("us.anthropic.claude-sonnet-4-5-v1:0"),
        Some((4, 5))
    );
    assert_eq!(claude_version("claude-opus-5"), Some((5, 0)));
    assert_eq!(claude_version("kimi-k2"), None);
}

#[test]
fn body_conversion_and_cache_breakpoints() {
    let p = anthropic();
    let m = model("claude-sonnet-4-5", json!({}));
    let req = conversation(
        ("anthropic", "claude-sonnet-4-5"),
        Some(json!({"signature": "SIG"})),
        None,
    );
    let body = build_body(&p, &m, &req);

    assert_eq!(body["max_tokens"], 8192);
    assert_eq!(body["stream"], true);
    assert!(body.get("thinking").is_none());
    let system = body["system"].as_array().unwrap();
    assert_eq!(system.len(), 2);
    assert!(!has_cache(&system[0]) && has_cache(&system[1]));
    assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
    assert!(has_cache(&body["tools"][0]));
    assert_eq!(body["tool_choice"], json!({"type": "auto"}));

    let msgs = body["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 3, "{msgs:#?}");
    assert_eq!(msgs[0]["role"], "user");
    assert_eq!(
        msgs[0]["content"][1],
        json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "iVBORw0KGgo="},
               "cache_control": {"type": "ephemeral"}})
    );
    let asst = msgs[1]["content"].as_array().unwrap();
    assert_eq!(
        asst[0],
        json!({"type": "thinking", "thinking": "Let me read the file.", "signature": "SIG"})
    );
    assert_eq!(asst[1], json!({"type": "text", "text": "Reading it."}));
    assert_eq!(
        asst[2],
        json!({"type": "tool_use", "id": "call_1", "name": "read", "input": {"path": "a.rs"}})
    );

    // Tool result and the following user text merged into one user message.
    let user = msgs[2]["content"].as_array().unwrap();
    assert_eq!(user[0]["type"], "tool_result");
    assert_eq!(user[0]["tool_use_id"], "call_1");
    assert_eq!(
        user[0]["content"][0],
        json!({"type": "text", "text": "fn main() {}"})
    );
    assert_eq!(user[0]["content"][1]["type"], "image");
    assert!(!has_cache(&user[0]));
    assert_eq!(user[1]["text"], "thanks");
    assert!(has_cache(&user[1]));

    // At most 4 breakpoints in total.
    let count = body.to_string().matches("cache_control").count();
    assert_eq!(count, 4);
}

#[test]
fn foreign_thinking_dropped_and_ids_sanitized() {
    let p = anthropic();
    let m = model("claude-sonnet-4-5", json!({}));
    let mut req = conversation(("openai", "gpt-5"), Some(json!({"signature": "SIG"})), None);
    if let ChatMessage::Assistant { blocks, .. } = &mut req.messages[1]
        && let AssistantBlock::ToolCall { id, arguments, .. } = &mut blocks[2]
    {
        *id = "call_x|fc_y".into();
        *arguments = "not json".into();
    }
    if let ChatMessage::ToolResult {
        call_id, is_error, ..
    } = &mut req.messages[2]
    {
        *call_id = "call_x|fc_y".into();
        *is_error = true;
    }
    let body = build_body(&p, &m, &req);
    let msgs = body["messages"].as_array().unwrap();
    let asst = msgs[1]["content"].as_array().unwrap();
    assert!(asst.iter().all(|b| b["type"] != "thinking"));
    assert_eq!(asst[1]["id"], "call_x_fc_y");
    assert_eq!(asst[1]["input"], json!({}));
    let result = &msgs[2]["content"][0];
    assert_eq!(result["tool_use_id"], "call_x_fc_y");
    assert_eq!(result["is_error"], true);
    assert_eq!(
        result["content"].as_array().unwrap().len(),
        1,
        "no images in error results"
    );
}

#[test]
fn redacted_thinking_replay() {
    let p = anthropic();
    let m = model("claude-sonnet-4-5", json!({}));
    let req = conversation(
        ("anthropic", "claude-sonnet-4-5"),
        Some(json!({"redacted": "OPAQUE"})),
        None,
    );
    let body = build_body(&p, &m, &req);
    assert_eq!(
        body["messages"][1]["content"][0],
        json!({"type": "redacted_thinking", "data": "OPAQUE"})
    );
}

#[test]
fn budget_thinking() {
    let p = anthropic();
    let mut m = model(
        "claude-sonnet-4-5",
        json!({"thinking": {"type": "enabled", "budgetTokens": 16000}}),
    );
    m.output = 64_000;
    m.temperature = Some(0.5);
    let body = build_body(&p, &m, &conversation(("a", "b"), None, None));
    assert_eq!(
        body["thinking"],
        json!({"type": "enabled", "budget_tokens": 16000})
    );
    assert_eq!(body["max_tokens"], 64_000);
    assert!(body.get("temperature").is_none());

    // Unknown output: default 32000, bumped above the budget.
    m.output = 0;
    m.options = json!({"thinking": {"type": "enabled", "budgetTokens": 40000}})
        .as_object()
        .cloned()
        .unwrap();
    let body = build_body(&p, &m, &conversation(("a", "b"), None, None));
    assert_eq!(body["thinking"]["budget_tokens"], 40000);
    assert_eq!(body["max_tokens"], 44096);

    // Tiny budgets are raised to the minimum.
    m.options = json!({"thinking": {"type": "enabled", "budgetTokens": 10}})
        .as_object()
        .cloned()
        .unwrap();
    let body = build_body(&p, &m, &conversation(("a", "b"), None, None));
    assert_eq!(body["thinking"]["budget_tokens"], 1024);

    // Effort on an older model maps to a budget.
    let mut m = model("claude-sonnet-4-5", json!({"reasoningEffort": "medium"}));
    m.output = 0;
    let body = build_body(&p, &m, &conversation(("a", "b"), None, None));
    assert_eq!(
        body["thinking"],
        json!({"type": "enabled", "budget_tokens": 8192})
    );
    assert_eq!(body["max_tokens"], 32000);
    assert!(body.get("output_config").is_none());

    // Budget >= the model's output limit: split the limit instead of exceeding it.
    m.output = 8192;
    let body = build_body(&p, &m, &conversation(("a", "b"), None, None));
    assert_eq!(body["thinking"]["budget_tokens"], 4096);
    assert_eq!(body["max_tokens"], 8192);
}

#[test]
fn adaptive_thinking() {
    let p = anthropic();
    let mut m = model("claude-opus-4-6", json!({"reasoningEffort": "xhigh"}));
    m.temperature = Some(0.3);
    let body = build_body(&p, &m, &conversation(("a", "b"), None, None));
    assert_eq!(body["thinking"], json!({"type": "adaptive"}));
    assert_eq!(body["output_config"], json!({"effort": "high"}));
    assert!(body.get("temperature").is_none());

    let m = model("claude-opus-5.5", json!({"reasoningEffort": "xhigh"}));
    let body = build_body(&p, &m, &conversation(("a", "b"), None, None));
    assert_eq!(
        body["thinking"],
        json!({"type": "adaptive", "display": "summarized"})
    );
    assert_eq!(body["output_config"], json!({"effort": "xhigh"}));

    let m = model("claude-sonnet-4.7", json!({"effort": "minimal"}));
    let body = build_body(&p, &m, &conversation(("a", "b"), None, None));
    assert_eq!(body["output_config"]["effort"], "low");

    // opencode variant style.
    let m = model(
        "claude-opus-4-7",
        json!({"thinking": {"type": "adaptive", "display": "summarized"}, "effort": "max"}),
    );
    let body = build_body(&p, &m, &conversation(("a", "b"), None, None));
    assert_eq!(
        body["thinking"],
        json!({"type": "adaptive", "display": "summarized"})
    );
    assert_eq!(body["output_config"], json!({"effort": "max"}));

    // Unknown ids (e.g. Anthropic-compatible hosts) default to adaptive.
    let m = model("some-model", json!({"reasoningEffort": "high"}));
    let body = build_body(&p, &m, &conversation(("a", "b"), None, None));
    assert_eq!(body["thinking"]["type"], "adaptive");

    // No options: provider default (thinking off), temperature allowed.
    let mut m = model("claude-sonnet-4-5", json!({}));
    m.temperature = Some(0.3);
    let body = build_body(&p, &m, &conversation(("a", "b"), None, None));
    assert!(body.get("thinking").is_none());
    assert_eq!(body["temperature"], 0.3);
}

#[test]
fn headers() {
    let mut p = anthropic();
    p.headers.insert(
        "anthropic-beta".into(),
        "context-1m-2025-08-07, interleaved-thinking-2025-05-14".into(),
    );
    let req = conversation(("a", "b"), None, None);
    let h = request_headers(&p, &req);
    assert_eq!(h["x-api-key"], "sk-test");
    assert!(h.get("authorization").is_none());
    assert_eq!(h["anthropic-version"], "2023-06-01");
    assert_eq!(
        h["anthropic-beta"],
        "interleaved-thinking-2025-05-14,fine-grained-tool-streaming-2025-05-14,context-1m-2025-08-07"
    );
    assert_eq!(h["x-session-affinity"], "ses_123");
    assert!(h.get("session_id").is_none());

    p.api_key = Some("sk-ant-oat01-abc".into());
    let h = request_headers(&p, &req);
    assert_eq!(h["authorization"], "Bearer sk-ant-oat01-abc");
    assert!(h.get("x-api-key").is_none());
}

const STREAM: &str = concat!(
    "event: message_start\n",
    "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude-sonnet-4-5\",\"usage\":{\"input_tokens\":12,\"cache_creation_input_tokens\":100,\"cache_read_input_tokens\":5000,\"output_tokens\":1}}}\n\n",
    "event: content_block_start\n",
    "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\",\"signature\":\"\"}}\n\n",
    "event: ping\ndata: {\"type\":\"ping\"}\n\n",
    "event: content_block_delta\n",
    "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"Need to \"}}\n\n",
    "event: content_block_delta\n",
    "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"read.\"}}\n\n",
    "event: content_block_delta\n",
    "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"EqQB\"}}\n\n",
    "event: content_block_delta\n",
    "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"CkYI\"}}\n\n",
    "event: content_block_stop\n",
    "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
    "event: content_block_start\n",
    "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"redacted_thinking\",\"data\":\"OPAQUE\"}}\n\n",
    "event: content_block_stop\n",
    "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
    "event: content_block_start\n",
    "data: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
    "event: content_block_delta\n",
    "data: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"text_delta\",\"text\":\"Reading.\"}}\n\n",
    "event: content_block_stop\n",
    "data: {\"type\":\"content_block_stop\",\"index\":2}\n\n",
    "event: content_block_start\n",
    "data: {\"type\":\"content_block_start\",\"index\":3,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_01\",\"name\":\"read\",\"input\":{}}}\n\n",
    "event: content_block_delta\n",
    "data: {\"type\":\"content_block_delta\",\"index\":3,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\"}}\n\n",
    "event: content_block_delta\n",
    "data: {\"type\":\"content_block_delta\",\"index\":3,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"path\\\": \\\"a.rs\\\"}\"}}\n\n",
    "event: content_block_stop\n",
    "data: {\"type\":\"content_block_stop\",\"index\":3}\n\n",
    "event: content_block_start\n",
    "data: {\"type\":\"content_block_start\",\"index\":4,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_02\",\"name\":\"ls\",\"input\":{}}}\n\n",
    "event: content_block_stop\n",
    "data: {\"type\":\"content_block_stop\",\"index\":4}\n\n",
    "event: message_delta\n",
    "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":42}}\n\n",
    "event: message_stop\n",
    "data: {\"type\":\"message_stop\"}\n\n",
);

#[test]
fn parse_stream() {
    let (res, events) = replay(AnthropicState::default(), STREAM.as_bytes());
    let res = res.unwrap();
    assert_eq!(res.stop_reason, "tool_calls");
    assert_eq!(
        res.usage,
        Usage {
            input: 12,
            output: 42,
            cache_read: 5000,
            cache_write: 100,
            reasoning: 0
        }
    );
    assert_eq!(res.blocks.len(), 5);
    assert!(
        matches!(&res.blocks[0], AssistantBlock::Reasoning { text, provider_data: Some(d) }
        if text == "Need to read." && d["signature"] == "EqQBCkYI")
    );
    assert!(
        matches!(&res.blocks[1], AssistantBlock::Reasoning { provider_data: Some(d), .. } if d["redacted"] == "OPAQUE")
    );
    assert!(matches!(&res.blocks[2], AssistantBlock::Text(t) if t == "Reading."));
    assert!(
        matches!(&res.blocks[3], AssistantBlock::ToolCall { id, arguments, .. }
        if id == "toolu_01" && arguments == r#"{"path": "a.rs"}"#)
    );
    assert!(
        matches!(&res.blocks[4], AssistantBlock::ToolCall { arguments, .. } if arguments == "{}")
    );

    assert!(matches!(&events[0], StreamEvent::Usage(u) if u.cache_read == 5000));
    assert!(matches!(
        &events[1],
        StreamEvent::BlockStart {
            index: 0,
            kind: BlockKind::Reasoning
        }
    ));
    assert!(matches!(events.last(), Some(StreamEvent::Usage(u)) if u.output == 42));
}

#[test]
fn stream_errors() {
    let s = concat!(
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":1}}}\n\n",
        "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n",
    );
    let err = replay(AnthropicState::default(), s.as_bytes())
        .0
        .unwrap_err();
    assert!(err.retryable);
    assert!(err.message.contains("Overloaded"));

    let s = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":1}}}\n\n";
    let err = replay(AnthropicState::default(), s.as_bytes())
        .0
        .unwrap_err();
    assert!(err.retryable);
    assert!(err.message.contains("stream ended unexpectedly"));
}
