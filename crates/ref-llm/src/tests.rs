//! Cross-module tests: conversation conversion to each wire format and decoding of recorded
//! sample streams.

use std::collections::BTreeMap;
use std::time::Duration;

use ref_proto::{ImagePart, Message, MessageOrigin, Part, ReasoningPart, Role, TextPart, ToolPart, ToolState};
use serde_json::{json, Value};

use crate::transport::{Decoder, Feeder};
use crate::*;

pub(crate) fn model(api: ApiKind, api_model: &str) -> ModelSpec {
    ModelSpec {
        provider_id: "prov".into(),
        model_key: api_model.into(),
        api_model: api_model.into(),
        api,
        base_url: "https://example.com/v1".into(),
        api_key: Some("sk-test".into()),
        headers: BTreeMap::new(),
        options: Default::default(),
        extra_body: Default::default(),
        context: 200_000,
        max_output: 32_000,
        image_input: true,
        reasoning: true,
        temperature: None,
        top_p: None,
        cost: Cost { input: 3.0, output: 15.0, cache_read: 0.3, cache_write: 3.75 },
        header_timeout: Duration::from_secs(5),
        chunk_timeout: Duration::from_secs(5),
    }
}

fn text(t: &str) -> Part {
    Part::Text(TextPart { id: "t".into(), text: t.into(), time: 0, meta: None })
}

fn image(data: &str) -> ImagePart {
    ImagePart { id: "i".into(), mime: "image/png".into(), data: data.into(), name: None }
}

fn tool(call_id: &str, state: ToolState, output: &str, images: Vec<ImagePart>, meta: Option<Value>) -> Part {
    Part::Tool(ToolPart {
        id: format!("p_{call_id}"),
        call_id: call_id.into(),
        name: "read".into(),
        purpose: "look".into(),
        input: json!({"path": "a.rs", "purpose": "look"}),
        input_raw: String::new(),
        state,
        output: output.into(),
        images,
        time: 0,
        time_start: None,
        time_end: None,
        run_at: None,
        meta,
    })
}

fn user(parts: Vec<Part>) -> Message {
    let mut m = Message::new(Role::User, MessageOrigin::User);
    m.parts = parts;
    m
}

fn assistant(model: &ModelSpec, parts: Vec<Part>) -> Message {
    let mut m = Message::new(Role::Assistant, MessageOrigin::User);
    m.model = Some(model.full_id());
    m.parts = parts;
    m.finish = Some("tool_use".into());
    m
}

fn meta(model: &ModelSpec, fields: Value) -> Option<Value> {
    Some(convert::make_meta(model, fields))
}

/// user(text+image) → assistant(reasoning, text, 3 tools: done+image, error, running) →
/// user(text) → assistant(text) → user(text)
fn conversation(m: &ModelSpec, reasoning_meta: Option<Value>, call_meta: Option<Value>) -> Request {
    let msgs = vec![
        user(vec![text("look at this"), Part::Image(image("AAAA"))]),
        assistant(
            m,
            vec![
                Part::Reasoning(ReasoningPart {
                    id: "r".into(),
                    text: "thinking...".into(),
                    time: 0,
                    time_end: None,
                    meta: reasoning_meta,
                }),
                text("Let me look."),
                tool("toolu|01", ToolState::Done, "file text", vec![image("BBBB")], call_meta),
                tool("call_2", ToolState::Error, "boom", vec![], None),
                tool("call_3", ToolState::Running, "", vec![], None),
            ],
        ),
        user(vec![text("next")]),
        {
            let mut a = assistant(m, vec![text("ok")]);
            a.finish = Some("stop".into());
            a
        },
        // An errored, empty assistant message is skipped entirely.
        {
            let mut a = assistant(m, vec![Part::Reasoning(ReasoningPart {
                id: "x".into(),
                text: String::new(),
                time: 0,
                time_end: None,
                meta: None,
            })]);
            a.finish = Some("error".into());
            a
        },
        user(vec![text("continue")]),
    ];
    Request {
        system: vec!["You are ref.".into(), "Project rules.".into()],
        messages: msgs,
        tools: vec![
            ToolDef { name: "read".into(), description: "Read a file".into(), parameters: json!({"type": "object"}) },
            ToolDef { name: "bash".into(), description: "Run".into(), parameters: json!({"type": "object"}) },
        ],
        session_id: "ses_123".into(),
        tool_choice_none: false,
    }
}

fn run_fixture<D: Decoder>(decoder: D, sse: &str) -> Vec<Result<StreamEvent, LlmError>> {
    let mut f = Feeder::new(decoder);
    // Feed in small uneven chunks to exercise the incremental parser.
    let bytes = sse.as_bytes();
    let mut i = 0;
    let mut step = 1;
    while i < bytes.len() {
        let end = (i + step).min(bytes.len());
        f.push(&bytes[i..end]);
        i = end;
        step = step % 37 + 3;
    }
    f.finish();
    f.queue.into_iter().collect()
}

fn ok_events(evs: Vec<Result<StreamEvent, LlmError>>) -> Vec<StreamEvent> {
    evs.into_iter().map(|e| e.unwrap_or_else(|e| panic!("unexpected error: {e}"))).collect()
}

fn collect_text(evs: &[StreamEvent]) -> (String, String) {
    let mut t = String::new();
    let mut r = String::new();
    for e in evs {
        match e {
            StreamEvent::TextDelta { text, .. } => t.push_str(text),
            StreamEvent::ReasoningDelta { text, .. } => r.push_str(text),
            _ => {}
        }
    }
    (t, r)
}

fn last_usage(evs: &[StreamEvent]) -> Usage {
    evs.iter()
        .rev()
        .find_map(|e| match e {
            StreamEvent::Usage(u) => Some(u.clone()),
            _ => None,
        })
        .expect("usage event")
}

fn finish(evs: &[StreamEvent]) -> FinishReason {
    match evs.last() {
        Some(StreamEvent::Finish { reason }) => *reason,
        other => panic!("last event is not Finish: {other:?}"),
    }
}

fn tool_ends(evs: &[StreamEvent]) -> Vec<(String, String, Value, Option<Value>)> {
    evs.iter()
        .filter_map(|e| match e {
            StreamEvent::ToolCallEnd { call_id, name, input, meta, .. } => {
                Some((call_id.clone(), name.clone(), input.clone(), meta.clone()))
            }
            _ => None,
        })
        .collect()
}

fn count_key(v: &Value, key: &str) -> usize {
    match v {
        Value::Object(o) => o.iter().map(|(k, v)| usize::from(k == key) + count_key(v, key)).sum(),
        Value::Array(a) => a.iter().map(|v| count_key(v, key)).sum(),
        _ => 0,
    }
}

// ───────────────────────────── anthropic ─────────────────────────────

#[test]
fn anthropic_conversion() {
    let m = model(ApiKind::Anthropic, "claude-sonnet-4-5");
    let req = conversation(&m, meta(&m, json!({"signature": "SIG"})), None);
    let body = anthropic::build_body(&m, &req).body;

    assert_eq!(body["model"], "claude-sonnet-4-5");
    assert_eq!(body["max_tokens"], 32_000);
    assert_eq!(body["stream"], true);
    assert_eq!(body["system"].as_array().unwrap().len(), 2);
    assert!(body["system"][0].get("cache_control").is_none());
    assert_eq!(body["system"][1]["cache_control"], json!({"type": "ephemeral"}));
    assert_eq!(body["tools"][0]["input_schema"], json!({"type": "object"}));
    assert_eq!(body["tools"][1]["cache_control"], json!({"type": "ephemeral"}));
    assert!(body.get("thinking").is_none());

    let msgs = body["messages"].as_array().unwrap();
    let roles: Vec<&str> = msgs.iter().map(|m| m["role"].as_str().unwrap()).collect();
    assert_eq!(roles, ["user", "assistant", "user", "assistant", "user"]);

    assert_eq!(msgs[0]["content"][0], json!({"type": "text", "text": "look at this"}));
    assert_eq!(msgs[0]["content"][1]["source"]["data"], "AAAA");

    let a = msgs[1]["content"].as_array().unwrap();
    assert_eq!(a[0], json!({"type": "thinking", "thinking": "thinking...", "signature": "SIG"}));
    assert_eq!(a[1], json!({"type": "text", "text": "Let me look."}));
    assert_eq!(a[2]["type"], "tool_use");
    assert_eq!(a[2]["id"], "toolu_01", "ids are sanitized");
    assert_eq!(a[2]["input"]["path"], "a.rs");
    assert_eq!(a.len(), 5);

    // Tool results first, then the following user text, all in one user message.
    let u = msgs[2]["content"].as_array().unwrap();
    assert_eq!(u[0]["type"], "tool_result");
    assert_eq!(u[0]["tool_use_id"], "toolu_01");
    assert_eq!(u[0]["content"][0], json!({"type": "text", "text": "file text"}));
    assert_eq!(u[0]["content"][1]["type"], "image");
    assert!(u[0].get("is_error").is_none());
    assert_eq!(u[1], json!({"type": "tool_result", "tool_use_id": "call_2", "content": "boom", "is_error": true}));
    assert_eq!(u[2]["content"], convert::TOOL_INTERRUPTED);
    assert_eq!(u[2]["is_error"], true);
    assert_eq!(u[3]["text"], "next");
    assert_eq!(u[3]["cache_control"], json!({"type": "ephemeral"}));

    assert_eq!(msgs[3]["content"], json!([{"type": "text", "text": "ok"}]));
    assert_eq!(msgs[4]["content"][0]["cache_control"], json!({"type": "ephemeral"}));
    assert!(count_key(&body, "cache_control") <= 4);
    assert_eq!(count_key(&body, "cache_control"), 4);
}

#[test]
fn anthropic_drops_foreign_reasoning_and_omits_images() {
    let m = model(ApiKind::Anthropic, "claude-sonnet-4-5");
    let other = model(ApiKind::Anthropic, "claude-opus-4-1");
    let mut no_img = m.clone();
    no_img.image_input = false;
    let req = conversation(&no_img, meta(&other, json!({"signature": "SIG"})), None);
    let body = anthropic::build_body(&no_img, &req).body;
    let msgs = body["messages"].as_array().unwrap();
    assert_eq!(msgs[0]["content"][1]["text"], convert::IMAGE_OMITTED);
    assert_eq!(msgs[1]["content"][0]["type"], "text", "foreign thinking dropped");
    assert!(msgs[2]["content"][0]["content"].as_str().unwrap().contains("tool image omitted"));
    assert_eq!(count_key(&body, "signature"), 0);
}

#[test]
fn anthropic_cache_breakpoints_never_exceed_four() {
    let m = model(ApiKind::Anthropic, "claude-sonnet-4-5");
    let mut req = conversation(&m, None, None);
    req.messages.extend((0..5).map(|i| user(vec![text(&format!("u{i}"))])));
    let body = anthropic::build_body(&m, &req).body;
    assert_eq!(count_key(&body, "cache_control"), 4);
    // Without tools / system the two user tail breakpoints remain.
    req.tools.clear();
    req.system.clear();
    let body = anthropic::build_body(&m, &req).body;
    assert_eq!(count_key(&body, "cache_control"), 2);
}

#[test]
fn anthropic_error_tool_images_go_after_results() {
    let m = model(ApiKind::Anthropic, "claude-sonnet-4-5");
    let req = Request {
        system: vec![],
        messages: vec![
            user(vec![text("go")]),
            assistant(&m, vec![tool("a", ToolState::Error, "bad", vec![image("CC")], None), tool("b", ToolState::Done, "", vec![], None)]),
        ],
        tools: vec![],
        session_id: String::new(),
        tool_choice_none: false,
    };
    let body = anthropic::build_body(&m, &req).body;
    let u = body["messages"][2]["content"].as_array().unwrap();
    assert_eq!(u[0]["type"], "tool_result");
    assert!(u[0]["content"].is_string());
    assert_eq!(u[1]["type"], "tool_result");
    assert_eq!(u[1]["content"], "(no output)");
    assert_eq!(u[2]["type"], "text");
    assert_eq!(u[3]["type"], "image");
}

#[test]
fn anthropic_thinking_options() {
    let mut m = model(ApiKind::Anthropic, "claude-sonnet-4-5");
    m.temperature = Some(0.5);
    let req = Request { system: vec![], messages: vec![user(vec![text("hi")])], tools: vec![], session_id: String::new(), tool_choice_none: false };

    let body = anthropic::build_body(&m, &req).body;
    assert_eq!(body["temperature"], 0.5);

    m.options.insert("thinking".into(), json!({"type": "enabled", "budgetTokens": 50_000}));
    m.max_output = 16_000;
    let built = anthropic::build_body(&m, &req);
    assert_eq!(built.body["thinking"], json!({"type": "enabled", "budget_tokens": 14_976}));
    assert!(built.body["max_tokens"].as_u64().unwrap() > 14_976);
    assert!(built.body.get("temperature").is_none());
    let h = anthropic::build_headers(&m, &built, "").unwrap();
    assert_eq!(h.get("anthropic-beta"), Some("interleaved-thinking-2025-05-14"));
    assert_eq!(h.get("x-api-key"), Some("sk-test"));
    assert_eq!(h.get("anthropic-version"), Some("2023-06-01"));

    m.options.clear();
    m.options.insert("reasoningEffort".into(), json!("high"));
    let body = anthropic::build_body(&m, &req).body;
    assert_eq!(body["thinking"], json!({"type": "enabled", "budget_tokens": 14_976}), "clamped below max_tokens");

    let mut a = model(ApiKind::Anthropic, "claude-opus-4-6");
    a.options.insert("effort".into(), json!("xhigh"));
    let built = anthropic::build_body(&a, &req);
    assert_eq!(built.body["thinking"]["type"], "adaptive");
    assert_eq!(built.body["output_config"], json!({"effort": "high"}), "xhigh unsupported on 4.6");
    let h = anthropic::build_headers(&a, &built, "").unwrap();
    assert_eq!(h.get("anthropic-beta"), None);

    for id in ["claude-opus-4-7", "claude-sonnet-5", "claude-haiku-5-5"] {
        let mut a = model(ApiKind::Anthropic, id);
        a.options.insert("thinking".into(), json!({"type": "adaptive"}));
        a.options.insert("reasoningEffort".into(), json!("xhigh"));
        let body = anthropic::build_body(&a, &req).body;
        assert_eq!(body["output_config"]["effort"], "xhigh", "{id}");
    }
    let mut a = model(ApiKind::Anthropic, "claude-opus-4-6");
    a.options.insert("effort".into(), json!("minimal"));
    assert_eq!(anthropic::build_body(&a, &req).body["output_config"]["effort"], "low");

    let mut d = model(ApiKind::Anthropic, "claude-opus-4-6");
    d.options.insert("thinking".into(), json!({"type": "disabled"}));
    d.options.insert("effort".into(), json!("high"));
    d.temperature = Some(1.0);
    let body = anthropic::build_body(&d, &req).body;
    assert_eq!(body["thinking"], json!({"type": "disabled"}));
    assert_eq!(body["temperature"], 1.0);
    // Opus 4.7+ / *-5.5 reject temperature even with thinking disabled.
    for id in ["claude-opus-4-7", "claude-opus-5.5", "claude-sonnet-5-5", "claude-fable-5"] {
        d.api_model = id.into();
        assert!(anthropic::build_body(&d, &req).body.get("temperature").is_none(), "{id}");
    }
    assert!(anthropic::supports_temperature("claude-sonnet-5"));
    assert!(anthropic::supports_temperature("claude-3-7-sonnet-20250219"));
    assert!(anthropic::supports_temperature("glm-5.3"));

    assert!(anthropic::supports_adaptive("claude-sonnet-4-6"));
    assert!(anthropic::supports_adaptive("anthropic/claude-opus-4.6"));
    assert!(anthropic::supports_adaptive("claude-opus-5"));
    assert!(!anthropic::supports_adaptive("claude-sonnet-4-5-20250929"));
    assert!(!anthropic::supports_adaptive("claude-3-7-sonnet-20250219"));
    assert!(!anthropic::supports_adaptive("claude-opus-4-20250514"));
}

#[test]
fn anthropic_headers_oauth_and_user_beta() {
    let mut m = model(ApiKind::Anthropic, "claude-sonnet-4-5");
    m.api_key = Some("sk-ant-oat01-xyz".into());
    m.headers.insert("Anthropic-Beta".into(), "context-1m-2025-08-07, fine-grained-tool-streaming-2025-05-14".into());
    m.headers.insert("x-extra".into(), "1".into());
    let req = conversation(&m, None, None);
    let built = anthropic::build_body(&m, &req);
    let h = anthropic::build_headers(&m, &built, "").unwrap();
    assert_eq!(h.get("authorization"), Some("Bearer sk-ant-oat01-xyz"));
    assert_eq!(h.get("x-api-key"), None);
    assert_eq!(
        h.get("anthropic-beta"),
        Some("oauth-2025-04-20,fine-grained-tool-streaming-2025-05-14,context-1m-2025-08-07")
    );
    assert_eq!(h.get("x-extra"), Some("1"));
    assert_eq!(h.get("user-agent"), Some(transport::USER_AGENT));
}

const ANTHROPIC_STREAM: &str = "event: message_start\r\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"usage\":{\"input_tokens\":12,\"cache_creation_input_tokens\":100,\"cache_read_input_tokens\":5000,\"output_tokens\":1}}}\r\n\r\n\
event: ping\ndata: {\"type\":\"ping\"}\n\n\
event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\",\"signature\":\"\"}}\n\n\
event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"Let me \"}}\n\n\
event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"think\"}}\n\n\
event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"EqQB\"}}\n\n\
event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"redacted_thinking\",\"data\":\"REDACTED\"}}\n\n\
event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":1}\n\n\
event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello 世界\"}}\n\n\
event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":2}\n\n\
event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":3,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_01\",\"name\":\"read\",\"input\":{}}}\n\n\
event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":3,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"path\\\":\"}}\n\n\
event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":3,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"a.rs\\\"}\"}}\n\n\
event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":3}\n\n\
event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":4,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_02\",\"name\":\"ls\",\"input\":{}}}\n\n\
event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":4}\n\n\
event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":512,\"input_tokens\":null}}\n\n\
event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";

#[test]
fn anthropic_stream_decoding() {
    let m = model(ApiKind::Anthropic, "claude-sonnet-4-5");
    let evs = ok_events(run_fixture(anthropic::AnthropicDecoder::new(&m), ANTHROPIC_STREAM));
    let (t, r) = collect_text(&evs);
    assert_eq!(t, "Hello 世界");
    assert_eq!(r, "Let me think");
    let reasoning_metas: Vec<Value> = evs
        .iter()
        .filter_map(|e| match e {
            StreamEvent::ReasoningEnd { meta, .. } => meta.clone(),
            _ => None,
        })
        .collect();
    assert_eq!(reasoning_metas[0], json!({"api": "anthropic", "model": "claude-sonnet-4-5", "signature": "EqQB"}));
    assert_eq!(reasoning_metas[1]["redacted"], "REDACTED");
    let tools = tool_ends(&evs);
    assert_eq!(tools[0].0, "toolu_01");
    assert_eq!(tools[0].2, json!({"path": "a.rs"}));
    assert_eq!(tools[1].2, json!({}));
    let u = last_usage(&evs);
    assert_eq!((u.input, u.output, u.cache_read, u.cache_write), (12, 512, 5000, 100));
    assert!(u.cost > 0.0);
    assert_eq!(finish(&evs), FinishReason::ToolUse);

    // Every started block index has exactly one end.
    for idx in 0..5 {
        let ends = evs
            .iter()
            .filter(|e| {
                matches!(e, StreamEvent::TextEnd { index, .. } | StreamEvent::ReasoningEnd { index, .. }
                    | StreamEvent::ToolCallEnd { index, .. } if *index == idx)
            })
            .count();
        assert_eq!(ends, 1, "index {idx}");
    }
}

#[test]
fn anthropic_stream_errors() {
    let m = model(ApiKind::Anthropic, "claude-sonnet-4-5");
    let evs = run_fixture(
        anthropic::AnthropicDecoder::new(&m),
        "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n",
    );
    match evs.last() {
        Some(Err(e @ LlmError::Stream(msg))) => {
            assert_eq!(msg, "overloaded_error: Overloaded");
            assert!(e.is_retryable());
        }
        other => panic!("{other:?}"),
    }
    let truncated = &ANTHROPIC_STREAM[..ANTHROPIC_STREAM.find("event: message_delta").unwrap()];
    let evs = run_fixture(anthropic::AnthropicDecoder::new(&m), truncated);
    assert!(matches!(evs.last(), Some(Err(LlmError::Stream(m))) if m.contains("message_stop")));
}

// ───────────────────────────── openai responses ─────────────────────────────

#[test]
fn responses_conversion() {
    let m = model(ApiKind::OpenAIResponses, "gpt-5.1");
    let item = json!({"type": "reasoning", "id": "rs_1", "summary": [{"type": "summary_text", "text": "thinking..."}], "encrypted_content": "ENC"});
    let mut req = conversation(&m, meta(&m, json!({"item": item})), meta(&m, json!({"id": "fc_9"})));
    if let Part::Text(t) = &mut req.messages[1].parts[1] {
        t.meta = meta(&m, json!({"id": "msg_7"}));
    }
    let mut m2 = m.clone();
    m2.options.insert("reasoningEffort".into(), json!("high"));
    m2.options.insert("textVerbosity".into(), json!("low"));
    m2.options.insert("truncation".into(), json!("auto"));
    let body = openai_responses::build_body(&m2, &req);

    assert_eq!(body["store"], false);
    assert_eq!(body["stream"], true);
    assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
    assert_eq!(body["prompt_cache_key"], "ses_123");
    assert_eq!(body["reasoning"], json!({"effort": "high", "summary": "auto"}));
    assert_eq!(body["text"], json!({"verbosity": "low"}));
    assert_eq!(body["max_output_tokens"], 32_000);
    assert_eq!(body["truncation"], "auto", "unknown options pass through");
    assert_eq!(body["tools"][0], json!({"type": "function", "name": "read", "description": "Read a file", "parameters": {"type": "object"}, "strict": false}));
    assert_eq!(body["tool_choice"], "auto");
    assert_eq!(body["parallel_tool_calls"], true);

    let input = body["input"].as_array().unwrap();
    assert_eq!(input[0], json!({"role": "developer", "content": "You are ref.\n\nProject rules."}));
    assert_eq!(input[1]["role"], "user");
    assert_eq!(input[1]["content"][1], json!({"type": "input_image", "detail": "auto", "image_url": "data:image/png;base64,AAAA"}));
    assert_eq!(input[2], item);
    assert_eq!(input[3]["type"], "message");
    assert_eq!(input[3]["id"], "msg_7");
    assert_eq!(input[3]["content"][0], json!({"type": "output_text", "text": "Let me look.", "annotations": []}));
    assert_eq!(input[4]["type"], "function_call");
    assert_eq!(input[4]["id"], "fc_9");
    assert_eq!(input[4]["call_id"], "toolu_01");
    assert_eq!(serde_json::from_str::<Value>(input[4]["arguments"].as_str().unwrap()).unwrap()["path"], "a.rs");
    assert!(input[5].get("id").is_none());
    assert_eq!(input[7]["type"], "function_call_output");
    assert_eq!(input[7]["call_id"], "toolu_01");
    assert_eq!(input[7]["output"][0], json!({"type": "input_text", "text": "file text"}));
    assert_eq!(input[7]["output"][1]["type"], "input_image");
    assert_eq!(input[8]["output"], "boom");
    assert_eq!(input[9]["output"], convert::TOOL_INTERRUPTED);
    assert_eq!(input[10]["content"][0]["text"], "next");
    assert_eq!(input[11], json!({"role": "assistant", "content": "ok"}));
    assert_eq!(input.len(), 13);

    // Different model: reasoning item and item ids are not replayed.
    let other = model(ApiKind::OpenAIResponses, "gpt-5-mini");
    let body = openai_responses::build_body(&other, &req);
    assert_eq!(count_key(&body, "encrypted_content"), 0);
    assert!(body["input"][2].get("id").is_none());
    assert_eq!(body["input"][2]["role"], "assistant");
}

const RESPONSES_STREAM: &str = r#"event: response.created
data: {"type":"response.created","response":{"id":"resp_1","status":"in_progress"}}

event: response.output_item.added
data: {"type":"response.output_item.added","output_index":0,"item":{"type":"reasoning","id":"rs_1","summary":[]}}

data: {"type":"response.reasoning_summary_part.added","output_index":0,"summary_index":0}

data: {"type":"response.reasoning_summary_text.delta","output_index":0,"summary_index":0,"delta":"Part one."}

data: {"type":"response.reasoning_summary_part.added","output_index":0,"summary_index":1}

data: {"type":"response.reasoning_summary_text.delta","output_index":0,"summary_index":1,"delta":"Part two."}

data: {"type":"response.output_item.done","output_index":0,"item":{"type":"reasoning","id":"rs_1","summary":[{"type":"summary_text","text":"Part one."},{"type":"summary_text","text":"Part two."}],"encrypted_content":"ENC"}}

data: {"type":"response.output_item.added","output_index":1,"item":{"type":"message","id":"msg_1","role":"assistant","content":[]}}

data: {"type":"response.output_text.delta","output_index":1,"content_index":0,"delta":"Hi "}

data: {"type":"response.output_text.delta","output_index":1,"content_index":0,"delta":"there"}

data: {"type":"response.output_item.done","output_index":1,"item":{"type":"message","id":"msg_1","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Hi there"}]}}

data: {"type":"response.output_item.added","output_index":2,"item":{"type":"function_call","id":"fc_1","call_id":"call_A","name":"read","arguments":""}}

data: {"type":"response.function_call_arguments.delta","output_index":2,"item_id":"fc_1","delta":"{\"path\":"}

data: {"type":"response.function_call_arguments.delta","output_index":2,"item_id":"fc_1","delta":"\"b.rs\"}"}

data: {"type":"response.output_item.done","output_index":2,"item":{"type":"function_call","id":"fc_1","call_id":"call_A","name":"read","arguments":"{\"path\":\"b.rs\"}"}}

event: response.completed
data: {"type":"response.completed","response":{"id":"resp_1","status":"completed","usage":{"input_tokens":12000,"input_tokens_details":{"cached_tokens":10000},"output_tokens":800,"output_tokens_details":{"reasoning_tokens":512},"total_tokens":12800}}}

"#;

#[test]
fn responses_stream_decoding() {
    let m = model(ApiKind::OpenAIResponses, "gpt-5.1");
    let evs = ok_events(run_fixture(openai_responses::ResponsesDecoder::new(&m), RESPONSES_STREAM));
    let (t, r) = collect_text(&evs);
    assert_eq!(t, "Hi there");
    assert_eq!(r, "Part one.\n\nPart two.");
    let rmeta = evs.iter().find_map(|e| match e {
        StreamEvent::ReasoningEnd { meta, .. } => meta.clone(),
        _ => None,
    });
    let rmeta = rmeta.unwrap();
    assert_eq!(rmeta["api"], "openai_responses");
    assert_eq!(rmeta["item"]["encrypted_content"], "ENC");
    let tmeta = evs.iter().find_map(|e| match e {
        StreamEvent::TextEnd { meta, .. } => meta.clone(),
        _ => None,
    });
    assert_eq!(tmeta.unwrap()["id"], "msg_1");
    let tools = tool_ends(&evs);
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].0, "call_A");
    assert_eq!(tools[0].2, json!({"path": "b.rs"}));
    assert_eq!(tools[0].3.as_ref().unwrap()["id"], "fc_1");
    let u = last_usage(&evs);
    assert_eq!((u.input, u.output, u.cache_read, u.reasoning), (2000, 800, 10000, 512));
    assert_eq!(finish(&evs), FinishReason::ToolUse);
}

#[test]
fn responses_backfills_encrypted_content_and_terminal_variants() {
    let m = model(ApiKind::OpenAIResponses, "gpt-5.1");
    let sse = r#"data: {"type":"response.output_item.added","output_index":0,"item":{"type":"reasoning","id":"rs_1"}}

data: {"type":"response.output_item.done","output_index":0,"item":{"type":"reasoning","id":"rs_1","summary":[]}}

data: {"type":"response.incomplete","response":{"status":"incomplete","incomplete_details":{"reason":"max_output_tokens"},"output":[{"type":"reasoning","id":"rs_1","encrypted_content":"LATE"}]}}

"#;
    let evs = ok_events(run_fixture(openai_responses::ResponsesDecoder::new(&m), sse));
    let rmeta = evs.iter().find_map(|e| match e {
        StreamEvent::ReasoningEnd { meta, .. } => meta.clone(),
        _ => None,
    });
    assert_eq!(rmeta.unwrap()["item"]["encrypted_content"], "LATE");
    assert_eq!(finish(&evs), FinishReason::Length);

    let failed = r#"data: {"type":"response.failed","response":{"status":"failed","error":{"code":"context_length_exceeded","message":"Your input exceeds the context window of this model."}}}

"#;
    let evs = run_fixture(openai_responses::ResponsesDecoder::new(&m), failed);
    match evs.last() {
        Some(Err(e)) => assert!(e.is_context_overflow(), "{e}"),
        other => panic!("{other:?}"),
    }
    let cut = &RESPONSES_STREAM[..RESPONSES_STREAM.find("event: response.completed").unwrap()];
    let evs = run_fixture(openai_responses::ResponsesDecoder::new(&m), cut);
    assert!(matches!(evs.last(), Some(Err(LlmError::Stream(m))) if m.contains("terminal")));
}

// ───────────────────────────── openai chat ─────────────────────────────

#[test]
fn chat_conversion() {
    let mut m = model(ApiKind::OpenAIChat, "glm-5");
    m.options.insert("reasoningEffort".into(), json!("high"));
    m.options.insert("thinking".into(), json!({"type": "enabled", "budgetTokens": 4096}));
    m.options.insert("chat_template_kwargs".into(), json!({"enable_thinking": true}));
    m.options.insert("textVerbosity".into(), json!("low"));
    m.extra_body.insert("tool_stream".into(), json!(true));
    m.temperature = Some(0.7);
    let req = conversation(&m, meta(&m, json!({"field": "reasoning_content"})), None);
    let body = openai_chat::build_body(&m, &req);

    assert_eq!(body["stream_options"], json!({"include_usage": true}));
    assert_eq!(body["max_tokens"], 32_000);
    assert!(body.get("max_completion_tokens").is_none());
    assert_eq!(body["reasoning_effort"], "high");
    assert_eq!(body["thinking"], json!({"type": "enabled", "budget_tokens": 4096}));
    assert_eq!(body["chat_template_kwargs"], json!({"enable_thinking": true}));
    assert_eq!(body["verbosity"], "low");
    assert_eq!(body["tool_stream"], true);
    assert_eq!(body["temperature"], 0.7);
    assert_eq!(body["tools"][0], json!({"type": "function", "function": {"name": "read", "description": "Read a file", "parameters": {"type": "object"}}}));

    let msgs = body["messages"].as_array().unwrap();
    assert_eq!(msgs[0], json!({"role": "system", "content": "You are ref.\n\nProject rules."}));
    assert_eq!(msgs[1]["content"][1], json!({"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}}));
    let a = &msgs[2];
    assert_eq!(a["role"], "assistant");
    assert_eq!(a["content"], "Let me look.");
    assert_eq!(a["reasoning_content"], "thinking...");
    assert_eq!(a["tool_calls"].as_array().unwrap().len(), 3);
    assert_eq!(a["tool_calls"][0]["id"], "toolu_01");
    assert_eq!(a["tool_calls"][0]["type"], "function");
    assert_eq!(a["tool_calls"][0]["function"]["name"], "read");
    assert!(a["tool_calls"][0]["function"]["arguments"].is_string());
    assert_eq!(msgs[3], json!({"role": "tool", "tool_call_id": "toolu_01", "content": "file text"}));
    assert_eq!(msgs[4]["content"], "boom");
    assert_eq!(msgs[5]["content"], convert::TOOL_INTERRUPTED);
    assert_eq!(msgs[6]["role"], "user");
    assert_eq!(msgs[6]["content"][0]["text"], "Attached image(s) from tool result:");
    assert_eq!(msgs[6]["content"][1]["image_url"]["url"], "data:image/png;base64,BBBB");
    assert_eq!(msgs[7], json!({"role": "user", "content": "next"}));
    assert_eq!(msgs[8], json!({"role": "assistant", "content": "ok"}));
    assert_eq!(msgs[9], json!({"role": "user", "content": "continue"}));
    assert_eq!(msgs.len(), 10);

    // Other model: reasoning not replayed. OpenAI host: max_completion_tokens.
    let mut other = model(ApiKind::OpenAIChat, "gpt-4.1");
    other.base_url = "https://api.openai.com/v1".into();
    let body = openai_chat::build_body(&other, &req);
    assert_eq!(count_key(&body, "reasoning_content"), 0);
    assert_eq!(body["max_completion_tokens"], 32_000);

    let mut or = model(ApiKind::OpenAIChat, "anthropic/claude-sonnet-4.5");
    or.base_url = "https://openrouter.ai/api/v1".into();
    or.options.insert("reasoningEffort".into(), json!("low"));
    let body = openai_chat::build_body(&or, &req);
    assert_eq!(body["reasoning"], json!({"effort": "low"}));
    assert!(body.get("reasoning_effort").is_none());
}

const CHAT_STREAM: &str = r#"data: {"id":"c1","choices":[{"index":0,"delta":{"role":"assistant","reasoning_content":"Hmm, ","reasoning":"Hmm, "}}]}

data: {"id":"c1","choices":[{"index":0,"delta":{"reasoning_content":"ok."}}]}

: keep-alive

data: {"id":"c1","choices":[{"index":0,"delta":{"content":"Sure"}}]}

data: {"id":"c1","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"read","arguments":""}}]}}]}

data: {"id":"c1","choices":[{"index":0,"delta":{"tool_calls":[{"index":1,"id":"call_2","type":"function","function":{"name":"ls","arguments":"{}"}}]}}]}

data: {"id":"c1","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"path\": \"c.rs\""}}]}}]}

data: {"id":"c1","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"}"}}]}}]}

data: {"id":"c1","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}

data: {"id":"c1","choices":[],"usage":{"prompt_tokens":1000,"completion_tokens":50,"prompt_tokens_details":{"cached_tokens":800},"completion_tokens_details":{"reasoning_tokens":20}}}

data: [DONE]

"#;

#[test]
fn chat_stream_decoding() {
    let m = model(ApiKind::OpenAIChat, "glm-5");
    let evs = ok_events(run_fixture(openai_chat::ChatDecoder::new(&m), CHAT_STREAM));
    let (t, r) = collect_text(&evs);
    assert_eq!(t, "Sure");
    assert_eq!(r, "Hmm, ok.");
    let rmeta = evs.iter().find_map(|e| match e {
        StreamEvent::ReasoningEnd { meta, .. } => meta.clone(),
        _ => None,
    });
    assert_eq!(rmeta.unwrap(), json!({"api": "openai_chat", "model": "glm-5", "field": "reasoning_content"}));
    let tools = tool_ends(&evs);
    assert_eq!(tools.len(), 2);
    assert_eq!((tools[0].0.as_str(), tools[0].1.as_str()), ("call_1", "read"));
    assert_eq!(tools[0].2, json!({"path": "c.rs"}));
    assert_eq!(tools[1].2, json!({}));
    let u = last_usage(&evs);
    assert_eq!((u.input, u.output, u.cache_read, u.reasoning), (200, 50, 800, 20));
    assert_eq!(finish(&evs), FinishReason::ToolUse);
}

#[test]
fn chat_stream_without_finish_or_done() {
    let m = model(ApiKind::OpenAIChat, "local");
    let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":1,\"prompt_cache_hit_tokens\":2}}\n\n";
    let evs = ok_events(run_fixture(openai_chat::ChatDecoder::new(&m), sse));
    assert_eq!(finish(&evs), FinishReason::Stop);
    assert_eq!(last_usage(&evs).cache_read, 2);
    assert_eq!(last_usage(&evs).input, 3);

    let evs = run_fixture(openai_chat::ChatDecoder::new(&m), "");
    assert!(matches!(evs.last(), Some(Err(LlmError::Stream(_)))));

    let err = "data: {\"error\":{\"message\":\"Rate limit exceeded\",\"code\":429}}\n\n";
    let evs = run_fixture(openai_chat::ChatDecoder::new(&m), err);
    assert!(matches!(evs.last(), Some(Err(LlmError::Stream(m))) if m.contains("Rate limit")));
}

#[test]
fn chat_reasoning_details_roundtrip() {
    let m = model(ApiKind::OpenAIChat, "openai/gpt-5");
    let sse = r#"data: {"choices":[{"delta":{"reasoning":"a","reasoning_details":[{"type":"reasoning.text","text":"a","index":0}]}}]}

data: {"choices":[{"delta":{"reasoning":"b","reasoning_details":[{"type":"reasoning.text","text":"b","signature":"S","index":0}]}}]}

data: {"choices":[{"delta":{"reasoning_details":[{"type":"reasoning.encrypted","data":"E","index":1}]}}]}

data: {"choices":[{"delta":{"content":"x"},"finish_reason":"stop"}]}

data: [DONE]

"#;
    let evs = ok_events(run_fixture(openai_chat::ChatDecoder::new(&m), sse));
    let rmeta = evs
        .iter()
        .find_map(|e| match e {
            StreamEvent::ReasoningEnd { meta, .. } => meta.clone(),
            _ => None,
        })
        .unwrap();
    assert_eq!(rmeta["field"], "reasoning");
    assert_eq!(
        rmeta["details"],
        json!([{"type": "reasoning.text", "text": "ab", "signature": "S"}, {"type": "reasoning.encrypted", "data": "E"}])
    );
    // Replay on the same model.
    let req = Request {
        system: vec![],
        messages: vec![
            user(vec![text("q")]),
            assistant(&m, vec![
                Part::Reasoning(ReasoningPart { id: "r".into(), text: "ab".into(), time: 0, time_end: None, meta: Some(rmeta) }),
                text("x"),
            ]),
        ],
        tools: vec![],
        session_id: String::new(),
        tool_choice_none: false,
    };
    let body = openai_chat::build_body(&m, &req);
    assert_eq!(body["messages"][1]["reasoning"], "ab");
    assert_eq!(body["messages"][1]["reasoning_details"][1]["data"], "E");
}

// ───────────────────────────── end-to-end over HTTP ─────────────────────────────

/// Serve one canned HTTP response on a local port; returns the base URL.
async fn serve_once(response: Vec<Vec<u8>>, delay_between: Duration) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        // Read the request headers + body (Content-Length).
        let mut buf = Vec::new();
        let mut tmp = [0u8; 8192];
        loop {
            let n = sock.read(&mut tmp).await.unwrap();
            if n == 0 {
                return;
            }
            buf.extend_from_slice(&tmp[..n]);
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&buf[..pos]).to_ascii_lowercase();
                let len = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap()))
                    .unwrap_or(0);
                if buf.len() >= pos + 4 + len {
                    break;
                }
            }
        }
        for (i, chunk) in response.iter().enumerate() {
            if i > 0 {
                tokio::time::sleep(delay_between).await;
            }
            if sock.write_all(chunk).await.is_err() {
                return;
            }
            let _ = sock.flush().await;
        }
        let _ = sock.shutdown().await;
    });
    format!("http://{addr}/v1")
}

fn sse_response(body: &str) -> Vec<u8> {
    format!("HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n{body}").into_bytes()
}

#[tokio::test]
async fn http_end_to_end_chat_and_complete_text() {
    let mut m = model(ApiKind::OpenAIChat, "glm-5");
    m.base_url = serve_once(vec![sse_response(CHAT_STREAM)], Duration::ZERO).await;
    let http = reqwest::Client::new();
    let req = conversation(&m, None, None);
    let mut s = stream(&http, &m, &req).await.unwrap();
    let mut evs = Vec::new();
    while let Some(e) = futures::StreamExt::next(&mut s).await {
        evs.push(e.unwrap());
    }
    assert_eq!(finish(&evs), FinishReason::ToolUse);

    let mut m = model(ApiKind::Anthropic, "claude-sonnet-4-5");
    m.base_url = serve_once(vec![sse_response(ANTHROPIC_STREAM)], Duration::ZERO).await;
    assert_eq!(complete_text(&http, &m, "sys", "title?").await.unwrap(), "Hello 世界");
}

#[tokio::test]
async fn http_error_status_and_retry_after() {
    let mut m = model(ApiKind::OpenAIResponses, "gpt-5.1");
    let resp = b"HTTP/1.1 429 Too Many Requests\r\ncontent-type: application/json\r\nretry-after: 3\r\ncontent-length: 27\r\nconnection: close\r\n\r\n{\"error\":\"rate limit hit\"}\n"
        .to_vec();
    m.base_url = serve_once(vec![resp], Duration::ZERO).await;
    let http = reqwest::Client::new();
    let err = match stream(&http, &m, &conversation(&m, None, None)).await {
        Err(e) => e,
        Ok(_) => panic!("expected error"),
    };
    assert!(matches!(&err, LlmError::Http { status: 429, body, .. } if body.contains("rate limit")));
    assert_eq!(err.retry_after(), Some(Duration::from_secs(3)));
    assert!(err.is_retryable());
    assert!(!err.is_context_overflow());
}

#[tokio::test]
async fn http_idle_timeout() {
    let mut m = model(ApiKind::Anthropic, "claude-sonnet-4-5");
    m.chunk_timeout = Duration::from_millis(200);
    let head = sse_response("event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":1}}}\n\n");
    m.base_url = serve_once(vec![head, b"event: ping\n".to_vec()], Duration::from_secs(5)).await;
    let http = reqwest::Client::new();
    let mut s = stream(&http, &m, &conversation(&m, None, None)).await.unwrap();
    let mut last = None;
    while let Some(e) = futures::StreamExt::next(&mut s).await {
        last = Some(e);
    }
    assert!(matches!(last, Some(Err(LlmError::Timeout(_)))), "{last:?}");
}

// ───────────────────────────── hardening (real-endpoint QA) ─────────────────────────────

fn simple_req(messages: Vec<Message>) -> Request {
    Request { system: vec!["sys".into()], messages, tools: vec![], session_id: "ses_1".into(), tool_choice_none: false }
}

#[test]
fn chat_reasoning_content_required_for_deepseek_and_kimi() {
    for id in ["deepseek-v4-pro-202606", "kimi-k2.5", "DeepSeek-V4-Flash"] {
        let m = model(ApiKind::OpenAIChat, id);
        // Assistant turn without reasoning (e.g. produced by another model) + tool call.
        let mut a = assistant(&m, vec![text("Let me look."), tool("call_1", ToolState::Done, "x", vec![], None)]);
        a.model = Some("other/model".into());
        let req = simple_req(vec![user(vec![text("q")]), a, user(vec![text("next")])]);
        let body = openai_chat::build_body(&m, &req);
        let asst = body["messages"].as_array().unwrap().iter().find(|m| m["role"] == "assistant").unwrap().clone();
        assert_eq!(asst["reasoning_content"], "", "{id}");
        // Streamed reasoning of the same model is replayed verbatim.
        let r = Part::Reasoning(ReasoningPart {
            id: "r".into(),
            text: "deep thought".into(),
            time: 0,
            time_end: None,
            meta: meta(&m, json!({"field": "reasoning_content"})),
        });
        let req = simple_req(vec![
            user(vec![text("q")]),
            assistant(&m, vec![r, tool("call_1", ToolState::Done, "x", vec![], None)]),
        ]);
        let body = openai_chat::build_body(&m, &req);
        assert_eq!(body["messages"][2]["reasoning_content"], "deep thought", "{id}");
    }
    // Other models: no empty reasoning_content is invented.
    let m = model(ApiKind::OpenAIChat, "glm-5.3");
    let req = simple_req(vec![user(vec![text("q")]), assistant(&m, vec![text("a")]), user(vec![text("n")])]);
    assert_eq!(count_key(&openai_chat::build_body(&m, &req), "reasoning_content"), 0);
}

#[test]
fn chat_compat_fields_per_host() {
    let req = Request { tools: vec![ToolDef { name: "t".into(), description: "d".into(), parameters: json!({"type": "object"}) }], ..simple_req(vec![user(vec![text("q")])]) };
    // Generic compatible provider: no store / prompt_cache_key / tool_stream.
    let mut m = model(ApiKind::OpenAIChat, "glm-5.1");
    m.options.insert("store".into(), json!(false));
    m.options.insert("include".into(), json!(["reasoning.encrypted_content"]));
    let body = openai_chat::build_body(&m, &req);
    for k in ["store", "prompt_cache_key", "tool_stream", "include"] {
        assert!(body.get(k).is_none(), "{k}: {body}");
    }
    // OpenAI: store, prompt_cache_key, max_completion_tokens.
    m.base_url = "https://api.openai.com/v1".into();
    let body = openai_chat::build_body(&m, &req);
    assert_eq!(body["store"], false);
    assert_eq!(body["prompt_cache_key"], "ses_1");
    assert!(body.get("max_completion_tokens").is_some());
    // OpenRouter: prompt_cache_key + affinity headers.
    m.base_url = "https://openrouter.ai/api/v1".into();
    let body = openai_chat::build_body(&m, &req);
    assert_eq!(body["prompt_cache_key"], "ses_1");
    let h = openai_chat::build_headers(&m, &req).unwrap();
    assert_eq!(h.get("x-session-id"), Some("ses_1"));
    assert_eq!(h.get("x-session-affinity"), Some("ses_1"));
    // setCacheKey opt-in for other hosts.
    m.base_url = "https://example.com/v1".into();
    m.options.insert("setCacheKey".into(), json!(true));
    assert_eq!(openai_chat::build_body(&m, &req)["prompt_cache_key"], "ses_1");
    // Zhipu / Z.ai GLM: tool_stream when tools are present (not for glm-4.5).
    let mut z = model(ApiKind::OpenAIChat, "glm-5.3");
    z.base_url = "https://open.bigmodel.cn/api/coding/paas/v4".into();
    assert_eq!(openai_chat::build_body(&z, &req)["tool_stream"], true);
    assert!(openai_chat::build_body(&z, &simple_req(vec![user(vec![text("q")])])).get("tool_stream").is_none());
    // GLM effort variant implies preserved thinking; explicit `thinking` / effort none win.
    assert!(openai_chat::build_body(&z, &req).get("thinking").is_none());
    z.options.insert("reasoningEffort".into(), json!("high"));
    let body = openai_chat::build_body(&z, &req);
    assert_eq!(body["thinking"], json!({"type": "enabled", "clear_thinking": false}));
    assert_eq!(body["reasoning_effort"], "high");
    z.options.insert("reasoningEffort".into(), json!("none"));
    assert!(openai_chat::build_body(&z, &req).get("thinking").is_none());
    z.options.insert("thinking".into(), json!({"type": "disabled"}));
    assert_eq!(openai_chat::build_body(&z, &req)["thinking"], json!({"type": "disabled"}));
    z.options.clear();
    z.api_model = "glm-4.5-air".into();
    assert!(openai_chat::build_body(&z, &req).get("tool_stream").is_none());
    // Tool history without active tools → `tools: []`.
    let hist = simple_req(vec![user(vec![text("q")]), assistant(&z, vec![tool("c", ToolState::Done, "x", vec![], None)])]);
    assert_eq!(openai_chat::build_body(&z, &hist)["tools"], json!([]));
}

#[test]
fn chat_tool_call_ids_fit_openai_limit() {
    let m = model(ApiKind::OpenAIChat, "gpt-5");
    let long = format!("call_{}|fc_{}", "a".repeat(30), "b".repeat(40));
    let req = simple_req(vec![user(vec![text("q")]), assistant(&m, vec![tool(&long, ToolState::Done, "x", vec![], None)])]);
    let body = openai_chat::build_body(&m, &req);
    let id = body["messages"][2]["tool_calls"][0]["id"].as_str().unwrap().to_string();
    assert!(id.len() <= 40, "{id}");
    assert_eq!(body["messages"][3]["tool_call_id"], id.as_str());
}

#[test]
fn max_tokens_fit_the_context_window() {
    let mut m = model(ApiKind::OpenAIChat, "kimi-k2.5");
    m.context = 262_144;
    m.max_output = 262_144;
    let small = simple_req(vec![user(vec![text("hi")])]);
    let max = transport::fit_max_tokens(&m, &small);
    assert!(max < 262_144 && max > 250_000, "{max}");
    // A prompt filling most of the window leaves only the remaining room (min 1024).
    let big = simple_req(vec![user(vec![text(&"word ".repeat(200_000))])]);
    let est = transport::estimate_tokens(&big);
    assert!(est > 250_000, "{est}");
    let max = transport::fit_max_tokens(&m, &big);
    assert!(max <= m.context.saturating_sub(250_000).max(1024), "{max}");
    assert!(max >= 1024);
    // Huge output limits are capped at 256k.
    m.context = 2_000_000;
    m.max_output = 393_216;
    assert_eq!(transport::fit_max_tokens(&m, &small), 256_000);
    // CJK text counts ~1 token per char.
    let cjk = simple_req(vec![user(vec![text(&"上下文".repeat(1000))])]);
    assert!(transport::estimate_tokens(&cjk) >= 3000);
    // Applied to all three APIs.
    let mut a = model(ApiKind::Anthropic, "claude-opus-5.5");
    a.context = 100_000;
    a.max_output = 128_000;
    assert!(anthropic::build_body(&a, &small).body["max_tokens"].as_u64().unwrap() < 100_000);
    let mut r = model(ApiKind::OpenAIResponses, "gpt-5.5");
    r.context = 100_000;
    r.max_output = 128_000;
    assert!(openai_responses::build_body(&r, &small)["max_output_tokens"].as_u64().unwrap() < 100_000);
}

#[test]
fn responses_sampling_and_orphaned_item_ids() {
    let mut m = model(ApiKind::OpenAIResponses, "gpt-5.5");
    m.temperature = Some(0.3);
    m.top_p = Some(0.9);
    let req = simple_req(vec![user(vec![text("q")])]);
    let body = openai_responses::build_body(&m, &req);
    assert!(body.get("temperature").is_none() && body.get("top_p").is_none());
    m.options.insert("reasoningEffort".into(), json!("none"));
    assert_eq!(openai_responses::build_body(&m, &req)["temperature"], 0.3);
    m.reasoning = false;
    m.options.clear();
    assert_eq!(openai_responses::build_body(&m, &req)["temperature"], 0.3);

    // Reasoning item without encrypted content (dropped with store=false): the paired
    // function_call / message must lose their ids.
    let m = model(ApiKind::OpenAIResponses, "gpt-5.5");
    let item = json!({"type": "reasoning", "id": "rs_1", "summary": []});
    let mut t = TextPart { id: "t".into(), text: "Looking.".into(), time: 0, meta: meta(&m, json!({"id": "msg_1"})) };
    t.time = 0;
    let req = simple_req(vec![
        user(vec![text("q")]),
        assistant(&m, vec![
            Part::Reasoning(ReasoningPart { id: "r".into(), text: String::new(), time: 0, time_end: None, meta: meta(&m, json!({"item": item})) }),
            Part::Text(t),
            tool("call_1", ToolState::Done, "x", vec![], meta(&m, json!({"id": "fc_1"}))),
        ]),
    ]);
    let body = openai_responses::build_body(&m, &req);
    let input = body["input"].as_array().unwrap();
    assert_eq!(count_key(&body, "encrypted_content"), 0);
    assert!(input.iter().all(|i| i.get("type").and_then(Value::as_str) != Some("reasoning")));
    assert_eq!(input[2], json!({"role": "assistant", "content": "Looking."}));
    assert_eq!(input[3]["type"], "function_call");
    assert!(input[3].get("id").is_none());
    let h = openai_responses::build_headers(&m, &req).unwrap();
    assert_eq!(h.get("session_id"), Some("ses_1"));
}

#[test]
fn anthropic_skips_blank_text_and_sends_affinity() {
    let m = model(ApiKind::Anthropic, "claude-opus-5.5");
    let req = simple_req(vec![user(vec![text("  \n"), text("real")])]);
    let built = anthropic::build_body(&m, &req);
    let content = built.body["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content.len(), 1);
    assert_eq!(content[0]["text"], "real");
    let h = anthropic::build_headers(&m, &built, "ses_1").unwrap();
    assert_eq!(h.get("x-session-affinity"), Some("ses_1"));
    // Whitespace-only tool output becomes a placeholder (empty tool_result text is rejected).
    let req = simple_req(vec![user(vec![text("q")]), assistant(&m, vec![tool("c1", ToolState::Done, " \n", vec![], None)])]);
    let body = anthropic::build_body(&m, &req).body;
    assert_eq!(body["messages"][2]["content"][0]["content"], "(no output)");
}

#[test]
fn request_headers_are_redacted() {
    let mut m = model(ApiKind::Anthropic, "claude-opus-5.5");
    m.headers.insert("x-custom-token".into(), "secret".into());
    let h = request_headers(&m, &simple_req(vec![user(vec![text("q")])])).unwrap();
    let get = |k: &str| h.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    assert_eq!(get("x-api-key"), Some("<redacted>"));
    assert_eq!(get("x-custom-token"), Some("<redacted>"));
    assert_eq!(get("anthropic-version"), Some("2023-06-01"));
    let m = model(ApiKind::OpenAIChat, "glm-5.3");
    let h = request_headers(&m, &simple_req(vec![user(vec![text("q")])])).unwrap();
    assert!(h.iter().any(|(n, v)| n == "authorization" && v == "<redacted>"));
    assert!(!h.iter().any(|(_, v)| v.contains("sk-test")));
}
