use super::*;
use crate::test_support::{conversation, model, provider};
use crate::util::replay;
use crate::{ApiKind, StreamEvent};

fn openai() -> Provider {
    provider("openai", ApiKind::OpenAIResponses, "")
}

fn reasoning_item() -> Value {
    json!({"type": "reasoning", "id": "rs_1", "summary": [{"type": "summary_text", "text": "Let me read the file."}],
           "encrypted_content": "gAAAAB"})
}

#[test]
fn body_with_replay_tools_and_images() {
    let p = openai();
    let m = model(
        "gpt-5.5",
        json!({"reasoningEffort": "high", "reasoningSummary": "auto", "textVerbosity": "low",
               "parallelToolCalls": true, "serviceTier": "priority"}),
    );
    let req = conversation(
        ("openai", "gpt-5.5"),
        Some(json!({"item": reasoning_item()})),
        Some(json!({"id": "fc_9"})),
    );
    let body = build_body(&p, &m, &req);

    assert_eq!(body["instructions"], "You are a coding agent.\n\ncwd: /tmp");
    assert_eq!(body["store"], false);
    assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
    assert_eq!(
        body["reasoning"],
        json!({"effort": "high", "summary": "auto"})
    );
    assert_eq!(body["text"], json!({"verbosity": "low"}));
    assert_eq!(body["max_output_tokens"], 8192);
    assert_eq!(body["prompt_cache_key"], "ses_123");
    assert_eq!(body["parallel_tool_calls"], true);
    assert_eq!(body["service_tier"], "priority");
    assert!(body.get("reasoningEffort").is_none() && body.get("textVerbosity").is_none());
    assert_eq!(
        body["tools"][0],
        json!({"type": "function", "name": "read", "description": "Read a file",
               "parameters": crate::test_support::tool().parameters, "strict": false})
    );

    let input = body["input"].as_array().unwrap();
    assert_eq!(input[0]["role"], "user");
    assert_eq!(
        input[0]["content"][0],
        json!({"type": "input_text", "text": "look at this"})
    );
    assert_eq!(input[0]["content"][1]["type"], "input_image");
    assert_eq!(
        input[0]["content"][1]["image_url"],
        "data:image/png;base64,iVBORw0KGgo="
    );
    assert_eq!(input[1], reasoning_item());
    assert_eq!(input[2]["type"], "message");
    assert_eq!(
        input[2]["content"][0],
        json!({"type": "output_text", "text": "Reading it.", "annotations": []})
    );
    assert_eq!(
        input[3],
        json!({"type": "function_call", "id": "fc_9", "call_id": "call_1", "name": "read", "arguments": "{\"path\":\"a.rs\"}"})
    );
    assert_eq!(input[4]["type"], "function_call_output");
    assert_eq!(input[4]["call_id"], "call_1");
    assert_eq!(
        input[4]["output"][0],
        json!({"type": "input_text", "text": "fn main() {}"})
    );
    assert_eq!(input[4]["output"][1]["type"], "input_image");
    assert_eq!(
        input[5],
        json!({"role": "user", "content": [{"type": "input_text", "text": "thanks"}]})
    );
}

#[test]
fn foreign_reasoning_and_item_ids_are_dropped() {
    let p = openai();
    let m = model("gpt-5.5", json!({}));
    let req = conversation(
        ("openai", "gpt-5"),
        Some(json!({"item": reasoning_item()})),
        Some(json!({"id": "fc_9"})),
    );
    let body = build_body(&p, &m, &req);
    let input = body["input"].as_array().unwrap();
    assert!(input.iter().all(|i| i["type"] != "reasoning"));
    let call = input.iter().find(|i| i["type"] == "function_call").unwrap();
    assert!(call.get("id").is_none());
    // model.reasoning=true -> include encrypted content by default.
    assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
    assert!(body.get("reasoning").is_none());
}

#[test]
fn explicit_store_and_include_and_passthrough() {
    let p = openai();
    let mut m = model(
        "gpt-5.5",
        json!({"store": true, "include": ["x"], "maxToolCalls": 3, "metadata": {"fooBar": 1}}),
    );
    m.reasoning = false;
    let body = build_body(&p, &m, &conversation(("a", "b"), None, None));
    assert_eq!(body["store"], true);
    assert_eq!(body["include"], json!(["x"]));
    assert_eq!(body["max_tool_calls"], 3);
    assert_eq!(body["metadata"], json!({"foo_bar": 1}));

    let mut m = model("gpt-4.1", json!({}));
    m.reasoning = false;
    let body = build_body(&p, &m, &conversation(("a", "b"), None, None));
    assert!(body.get("include").is_none());
}

const STREAM: &str = concat!(
    "event: response.created\n",
    "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"status\":\"in_progress\"}}\n\n",
    "event: response.output_item.added\n",
    "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"reasoning\",\"id\":\"rs_1\",\"summary\":[]}}\n\n",
    "event: response.reasoning_summary_part.added\n",
    "data: {\"type\":\"response.reasoning_summary_part.added\",\"output_index\":0,\"summary_index\":0}\n\n",
    "event: response.reasoning_summary_text.delta\n",
    "data: {\"type\":\"response.reasoning_summary_text.delta\",\"item_id\":\"rs_1\",\"output_index\":0,\"summary_index\":0,\"delta\":\"Plan A\"}\n\n",
    "event: response.reasoning_summary_text.delta\n",
    "data: {\"type\":\"response.reasoning_summary_text.delta\",\"item_id\":\"rs_1\",\"output_index\":0,\"summary_index\":1,\"delta\":\"Plan B\"}\n\n",
    "event: response.output_item.done\n",
    "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"reasoning\",\"id\":\"rs_1\",\"summary\":[{\"type\":\"summary_text\",\"text\":\"Plan A\"},{\"type\":\"summary_text\",\"text\":\"Plan B\"}],\"encrypted_content\":\"ENC\"}}\n\n",
    "event: response.output_item.added\n",
    "data: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"type\":\"message\",\"id\":\"msg_1\",\"role\":\"assistant\",\"content\":[]}}\n\n",
    "event: response.output_text.delta\n",
    "data: {\"type\":\"response.output_text.delta\",\"output_index\":1,\"content_index\":0,\"delta\":\"Hello\"}\n\n",
    "event: response.output_text.delta\n",
    "data: {\"type\":\"response.output_text.delta\",\"output_index\":1,\"content_index\":0,\"delta\":\" world\"}\n\n",
    "event: response.output_item.done\n",
    "data: {\"type\":\"response.output_item.done\",\"output_index\":1,\"item\":{\"type\":\"message\",\"id\":\"msg_1\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Hello world\"}]}}\n\n",
    "event: response.output_item.added\n",
    "data: {\"type\":\"response.output_item.added\",\"output_index\":2,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"read\",\"arguments\":\"\"}}\n\n",
    "event: response.function_call_arguments.delta\n",
    "data: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":2,\"item_id\":\"fc_1\",\"delta\":\"{\\\"path\\\":\"}\n\n",
    "event: response.function_call_arguments.delta\n",
    "data: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":2,\"item_id\":\"fc_1\",\"delta\":\"\\\"a.rs\\\"}\"}\n\n",
    "event: response.function_call_arguments.done\n",
    "data: {\"type\":\"response.function_call_arguments.done\",\"output_index\":2,\"item_id\":\"fc_1\",\"arguments\":\"{\\\"path\\\":\\\"a.rs\\\"}\"}\n\n",
    "event: response.output_item.done\n",
    "data: {\"type\":\"response.output_item.done\",\"output_index\":2,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"read\",\"arguments\":\"{\\\"path\\\":\\\"a.rs\\\"}\",\"status\":\"completed\"}}\n\n",
    "event: response.completed\n",
    "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"status\":\"completed\",\"usage\":{\"input_tokens\":1200,\"input_tokens_details\":{\"cached_tokens\":1024},\"output_tokens\":80,\"output_tokens_details\":{\"reasoning_tokens\":40},\"total_tokens\":1280}}}\n\n",
);

#[test]
fn parse_stream() {
    let (res, events) = replay(ResponsesState::default(), STREAM.as_bytes());
    let res = res.unwrap();
    assert_eq!(res.stop_reason, "tool_calls");
    assert_eq!(
        res.usage,
        Usage {
            input: 176,
            output: 80,
            cache_read: 1024,
            cache_write: 0,
            reasoning: 40
        }
    );
    assert_eq!(res.blocks.len(), 3);
    match &res.blocks[0] {
        AssistantBlock::Reasoning {
            text,
            provider_data: Some(d),
        } => {
            assert_eq!(text, "Plan A\n\nPlan B");
            assert_eq!(d["item"]["encrypted_content"], "ENC");
            assert_eq!(d["item"]["id"], "rs_1");
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(matches!(&res.blocks[1], AssistantBlock::Text(t) if t == "Hello world"));
    match &res.blocks[2] {
        AssistantBlock::ToolCall {
            id,
            name,
            arguments,
            provider_data,
        } => {
            assert_eq!((id.as_str(), name.as_str()), ("call_1", "read"));
            assert_eq!(arguments, r#"{"path":"a.rs"}"#);
            assert_eq!(provider_data.as_ref().unwrap()["id"], "fc_1");
        }
        other => panic!("unexpected {other:?}"),
    }
    // Arguments were not duplicated by the `.done` event.
    let tool_deltas: String = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Delta { index: 2, text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(tool_deltas, r#"{"path":"a.rs"}"#);
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, StreamEvent::BlockEnd { .. }))
            .count(),
        3
    );
}

#[test]
fn incomplete_maps_to_length() {
    let s = concat!(
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"message\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"x\"}\n\n",
        "data: {\"type\":\"response.incomplete\",\"response\":{\"status\":\"incomplete\",\"incomplete_details\":{\"reason\":\"max_output_tokens\"}}}\n\n",
    );
    let (res, _) = replay(ResponsesState::default(), s.as_bytes());
    assert_eq!(res.unwrap().stop_reason, "length");
}

#[test]
fn failures() {
    let s = "event: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\",\"error\":{\"code\":\"server_error\",\"message\":\"boom\"}}}\n\n";
    let err = replay(ResponsesState::default(), s.as_bytes())
        .0
        .unwrap_err();
    assert!(err.retryable && err.message.contains("boom"));

    let s = "event: error\ndata: {\"type\":\"error\",\"code\":\"context_length_exceeded\",\"message\":\"Your input exceeds the context window of this model.\"}\n\n";
    let err = replay(ResponsesState::default(), s.as_bytes())
        .0
        .unwrap_err();
    assert!(err.context_overflow && !err.retryable);

    let s = "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"message\"}}\n\n";
    let err = replay(ResponsesState::default(), s.as_bytes())
        .0
        .unwrap_err();
    assert!(err.retryable);
}

#[test]
fn headers() {
    let p = openai();
    let h = request_headers(&p, &conversation(("a", "b"), None, None));
    assert_eq!(h["authorization"], "Bearer sk-test");
    assert_eq!(h["session_id"], "ses_123");
    assert_eq!(h["x-session-affinity"], "ses_123");
}
