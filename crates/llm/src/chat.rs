//! OpenAI Chat Completions (`POST {base_url}/chat/completions`), used for OpenAI chat and
//! every OpenAI-compatible host (DeepSeek, GLM, Kimi, Qwen, OpenRouter, vLLM, ...).
//!
//! Options follow `@ai-sdk/openai-compatible`: `reasoningEffort` → `reasoning_effort`,
//! `textVerbosity` → `verbosity`, transport keys are ignored, and every other key is copied
//! verbatim into the body (so `thinking`, `enable_thinking`, `chat_template_kwargs` reach the
//! wire as written). `spec.body` is merged last.
//!
//! Replay data ([`StreamEvent::ReasoningEnd`]): `{"field": "<delta field the reasoning came
//! in, or null>", "details": [<OpenRouter reasoning_details>]}` (`details` only when present).
//! Tool call ids are passed through (sanitized to the 40-char OpenAI limit on the wire).

use crate::common::*;
use crate::sse::SseEvent;
use crate::*;
use reqwest::header::{HeaderMap, ACCEPT, AUTHORIZATION};
use std::collections::HashMap;

/// Option keys consumed by the transport layer (never sent).
const IGNORED_OPTIONS: &[&str] = &[
    "apiKey",
    "baseURL",
    "headers",
    "body",
    "extraBody",
    "timeout",
    "chunkTimeout",
    "headerTimeout",
    "disabled",
    "user",
    "strictJsonSchema",
    "setCacheKey",
    "include",
];

/// Reasoning text fields in delta priority order.
const REASONING_FIELDS: &[&str] = &["reasoning_content", "reasoning", "reasoning_text"];

const MAX_ID_LEN: usize = 40;

pub(crate) async fn stream(
    client: &reqwest::Client,
    spec: &ModelSpec,
    req: &ChatRequest,
    cancel: &CancellationToken,
    on_event: &mut (dyn FnMut(StreamEvent) + Send),
) -> Result<FinishReason, LlmError> {
    let url = format!("{}/chat/completions", spec.base_url.trim_end_matches('/'));
    let body = build_body(spec, req);
    let mut parser = ChatParser::default();
    run(client, spec, &url, headers(spec, req), &body, &mut parser, cancel, on_event).await
}

fn is_openai(spec: &ModelSpec) -> bool {
    host_has(spec, "api.openai.com")
}

fn is_deepseek(spec: &ModelSpec) -> bool {
    host_has(spec, "deepseek") || spec.api_model.to_ascii_lowercase().contains("deepseek")
}

fn is_zai(spec: &ModelSpec) -> bool {
    host_has(spec, "bigmodel.cn") || host_has(spec, "z.ai")
}

fn is_kimi(spec: &ModelSpec) -> bool {
    host_has(spec, "moonshot") || host_has(spec, "kimi") || spec.api_model.to_ascii_lowercase().contains("kimi")
}

fn headers(spec: &ModelSpec, req: &ChatRequest) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(ACCEPT, reqwest::header::HeaderValue::from_static("text/event-stream"));
    if let Some(key) = spec.api_key.as_deref().filter(|k| !k.is_empty()) {
        set_header(&mut h, AUTHORIZATION.as_str(), &format!("Bearer {key}"));
    }
    if host_has(spec, "openrouter.ai") {
        if !req.session_id.is_empty() {
            set_header(&mut h, "x-session-id", &req.session_id);
        }
        set_header(&mut h, "x-openrouter-title", "hernees");
    }
    apply_headers(&mut h, &spec.headers);
    h
}

/// Build the JSON request body.
pub(crate) fn build_body(spec: &ModelSpec, req: &ChatRequest) -> Value {
    let mut body = Map::new();
    body.insert("model".into(), Value::String(spec.api_model.clone()));
    body.insert("messages".into(), Value::Array(build_messages(spec, req)));

    let has_tool_history = req.messages.iter().any(|m| {
        matches!(m, LlmMessage::ToolResult { .. })
            || matches!(m, LlmMessage::Assistant { parts, .. } if parts.iter().any(|p| matches!(p, AssistantPart::ToolCall { .. })))
    });
    if !req.tools.is_empty() {
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {"name": t.name, "description": t.description, "parameters": t.parameters},
                })
            })
            .collect();
        body.insert("tools".into(), Value::Array(tools));
        if is_zai(spec) && !spec.api_model.to_ascii_lowercase().contains("glm-4.5") {
            body.insert("tool_stream".into(), Value::Bool(true));
        }
    } else if has_tool_history && !is_openai(spec) {
        // LiteLLM / Bedrock-Anthropic proxies require `tools` when history has tool calls.
        // OpenAI itself rejects an empty array.
        body.insert("tools".into(), Value::Array(Vec::new()));
    }

    body.insert("stream".into(), Value::Bool(true));
    body.insert("stream_options".into(), serde_json::json!({"include_usage": true}));
    if let Some(max) = max_tokens(spec, req) {
        let field = if is_openai(spec) { "max_completion_tokens" } else { "max_tokens" };
        body.insert(field.into(), Value::from(max));
    }

    let set_cache_key = spec.options.get("setCacheKey").and_then(Value::as_bool) == Some(true);
    if (is_openai(spec) || set_cache_key) && !req.session_id.is_empty() {
        body.insert("prompt_cache_key".into(), Value::String(truncate_chars(&req.session_id, 64)));
    }

    for (k, v) in &spec.options {
        if IGNORED_OPTIONS.contains(&k.as_str()) || v.is_null() {
            continue;
        }
        match k.as_str() {
            "reasoningEffort" => {
                body.insert("reasoning_effort".into(), v.clone());
            }
            "textVerbosity" => {
                body.insert("verbosity".into(), v.clone());
            }
            // `store` is only accepted by OpenAI itself.
            "store" if !is_openai(spec) => {}
            _ => {
                body.insert(k.clone(), v.clone());
            }
        }
    }
    merge_extra_body(&mut body, spec);
    Value::Object(body)
}

fn build_messages(spec: &ModelSpec, req: &ChatRequest) -> Vec<Value> {
    let mut out = Vec::new();
    let system = join_system(&req.system);
    if !system.is_empty() {
        out.push(serde_json::json!({"role": "system", "content": system}));
    }
    let deepseek = is_deepseek(spec);
    let kimi = is_kimi(spec);
    // Images from the current run of tool results, sent as one follow-up user message.
    let mut tool_images: Vec<Value> = Vec::new();
    let flush_images = |out: &mut Vec<Value>, imgs: &mut Vec<Value>| {
        if imgs.is_empty() {
            return;
        }
        let mut content = vec![serde_json::json!({"type": "text", "text": "Attached image(s) from tool result:"})];
        content.append(imgs);
        out.push(serde_json::json!({"role": "user", "content": content}));
    };

    for msg in normalize(&req.messages) {
        if !matches!(msg, LlmMessage::ToolResult { .. }) {
            flush_images(&mut out, &mut tool_images);
        }
        match msg {
            LlmMessage::User(content) => {
                let only_text = content.iter().all(|c| matches!(c, Content::Text(_)));
                let value = if only_text {
                    let text: Vec<&str> = content
                        .iter()
                        .filter_map(|c| match c {
                            Content::Text(t) => Some(t.as_str()),
                            Content::Image(_) => None,
                        })
                        .collect();
                    Value::String(text.join("\n"))
                } else {
                    Value::Array(
                        content
                            .iter()
                            .filter_map(|c| match c {
                                Content::Text(t) if t.is_empty() => None,
                                Content::Text(t) => Some(serde_json::json!({"type": "text", "text": t})),
                                Content::Image(img) => {
                                    Some(serde_json::json!({"type": "image_url", "image_url": {"url": data_url(img)}}))
                                }
                            })
                            .collect(),
                    )
                };
                out.push(serde_json::json!({"role": "user", "content": value}));
            }
            LlmMessage::Assistant { parts, replay } => {
                let mut text = String::new();
                let mut reasoning = String::new();
                let mut field: Option<String> = None;
                let mut details: Vec<Value> = Vec::new();
                let mut calls = Vec::new();
                for p in &parts {
                    match p {
                        AssistantPart::Text(t) => text.push_str(t),
                        AssistantPart::Reasoning { text: r, replay: data } => {
                            if !replay {
                                continue;
                            }
                            if !r.is_empty() {
                                if !reasoning.is_empty() {
                                    reasoning.push('\n');
                                }
                                reasoning.push_str(r);
                            }
                            if field.is_none() {
                                field = data.get("field").and_then(Value::as_str).map(str::to_string);
                            }
                            if let Some(d) = data.get("details").and_then(Value::as_array) {
                                details.extend(d.iter().cloned());
                            }
                        }
                        AssistantPart::ToolCall { id, name, arguments } => calls.push(serde_json::json!({
                            "id": sanitize_id(id, MAX_ID_LEN),
                            "type": "function",
                            "function": {"name": name, "arguments": if arguments.trim().is_empty() { "{}" } else { arguments.as_str() }},
                        })),
                    }
                }
                let mut m = Map::new();
                m.insert("role".into(), "assistant".into());
                m.insert(
                    "content".into(),
                    if text.trim().is_empty() { Value::Null } else { Value::String(text) },
                );
                if !calls.is_empty() {
                    m.insert("tool_calls".into(), Value::Array(calls));
                }
                if replay {
                    let field = field.or_else(|| spec.interleaved_field.clone());
                    if let Some(f) = field.filter(|f| REASONING_FIELDS.contains(&f.as_str())) {
                        m.insert(f, Value::String(reasoning));
                    }
                    if !details.is_empty() {
                        if kimi {
                            for d in details.iter_mut() {
                                if let Value::Object(o) = d {
                                    o.remove("index");
                                }
                            }
                        }
                        m.insert("reasoning_details".into(), Value::Array(details));
                    }
                }
                if deepseek && !m.contains_key("reasoning_content") {
                    // DeepSeek thinking mode 400s if any assistant turn lacks it.
                    m.insert("reasoning_content".into(), Value::String(String::new()));
                }
                out.push(Value::Object(m));
            }
            LlmMessage::ToolResult { call_id, content, images, .. } => {
                out.push(serde_json::json!({
                    "role": "tool",
                    "tool_call_id": sanitize_id(&call_id, MAX_ID_LEN),
                    "content": tool_output_text(&content),
                }));
                for img in &images {
                    tool_images.push(serde_json::json!({"type": "image_url", "image_url": {"url": data_url(img)}}));
                }
            }
        }
    }
    flush_images(&mut out, &mut tool_images);
    out
}

#[derive(Debug, Default)]
struct ToolSlot {
    id: String,
    name: String,
    args: String,
    started: bool,
}

/// Incremental parser for Chat Completions chunks.
#[derive(Debug, Default)]
pub(crate) struct ChatParser {
    /// `[DONE]` seen.
    finished: bool,
    finish_reason: Option<String>,
    usage: Option<Usage>,
    reasoning_open: bool,
    reasoning_field: Option<String>,
    details: Vec<Value>,
    tools: Vec<ToolSlot>,
    by_index: HashMap<i64, usize>,
    saw_tool_calls: bool,
}

impl ChatParser {
    fn end_reasoning(&mut self, out: &mut Vec<StreamEvent>) {
        if !self.reasoning_open {
            return;
        }
        self.reasoning_open = false;
        let mut replay = Map::new();
        replay.insert("field".into(), self.reasoning_field.clone().map_or(Value::Null, Value::String));
        if !self.details.is_empty() {
            replay.insert("details".into(), Value::Array(std::mem::take(&mut self.details)));
        }
        out.push(StreamEvent::ReasoningEnd { replay: Value::Object(replay) });
    }

    fn slot_for(&mut self, tc: &Value, position: Option<usize>) -> usize {
        let index = tc.get("index").and_then(Value::as_i64).or(position.map(|p| p as i64));
        let id = tc.get("id").and_then(Value::as_str).filter(|s| !s.is_empty());
        let new_slot = |tools: &mut Vec<ToolSlot>| {
            tools.push(ToolSlot { id: id.map_or_else(generate_call_id, str::to_string), ..Default::default() });
            tools.len() - 1
        };
        if let Some(i) = index {
            if let Some(&slot) = self.by_index.get(&i) {
                // A different id at a known index is a new call (some hosts reuse index 0).
                if id.is_none_or(|id| id == self.tools[slot].id) {
                    return slot;
                }
            }
            let slot = match id.and_then(|id| self.tools.iter().position(|t| t.id == id)) {
                Some(s) => s,
                None => new_slot(&mut self.tools),
            };
            self.by_index.insert(i, slot);
            return slot;
        }
        if let Some(id) = id {
            if let Some(s) = self.tools.iter().position(|t| t.id == id) {
                return s;
            }
            return new_slot(&mut self.tools);
        }
        if self.tools.is_empty() {
            new_slot(&mut self.tools)
        } else {
            self.tools.len() - 1
        }
    }

    fn handle_delta(&mut self, delta: &Value, whole_message: bool, out: &mut Vec<StreamEvent>) {
        // Reasoning first (it precedes content within a delta).
        for f in REASONING_FIELDS {
            if let Some(r) = delta.get(*f).and_then(Value::as_str).filter(|s| !s.is_empty()) {
                if self.reasoning_field.is_none() {
                    self.reasoning_field = Some((*f).to_string());
                }
                self.reasoning_open = true;
                out.push(StreamEvent::ReasoningDelta(r.to_string()));
                break;
            }
        }
        if let Some(details) = delta.get("reasoning_details").and_then(Value::as_array) {
            for d in details.iter().filter(|d| d.is_object()) {
                self.reasoning_open = true;
                append_detail(&mut self.details, d);
            }
        }
        if let Some(text) = delta.get("content").and_then(Value::as_str).filter(|s| !s.is_empty()) {
            self.end_reasoning(out);
            out.push(StreamEvent::TextDelta(text.to_string()));
        }
        if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
            if !calls.is_empty() {
                self.end_reasoning(out);
            }
            for (pos, tc) in calls.iter().enumerate() {
                self.saw_tool_calls = true;
                let slot = self.slot_for(tc, whole_message.then_some(pos));
                let t = &mut self.tools[slot];
                if let Some(name) = str_at(tc, "/function/name").filter(|s| !s.is_empty()) {
                    if t.name.is_empty() {
                        t.name = name.to_string();
                    }
                }
                let args = str_at(tc, "/function/arguments").unwrap_or("");
                if !t.started && !t.name.is_empty() {
                    t.started = true;
                    out.push(StreamEvent::ToolCallStart { id: t.id.clone(), name: t.name.clone() });
                    if !t.args.is_empty() {
                        out.push(StreamEvent::ToolCallDelta { id: t.id.clone(), delta: t.args.clone() });
                    }
                }
                if !args.is_empty() {
                    t.args.push_str(args);
                    if t.started {
                        out.push(StreamEvent::ToolCallDelta { id: t.id.clone(), delta: args.to_string() });
                    }
                }
            }
        }
    }
}

/// Merge OpenRouter `reasoning_details` deltas: consecutive text/summary entries concatenate.
fn append_detail(details: &mut Vec<Value>, d: &Value) {
    let ty = d.get("type").and_then(Value::as_str);
    if let (Some(last), Some(ty @ ("reasoning.text" | "reasoning.summary"))) = (details.last_mut(), ty) {
        if last.get("type").and_then(Value::as_str) == Some(ty) {
            let key = if ty == "reasoning.text" { "text" } else { "summary" };
            let add = d.get(key).and_then(Value::as_str).unwrap_or("").to_string();
            if let Value::Object(l) = last {
                let cur = l.get(key).and_then(Value::as_str).unwrap_or("").to_string();
                l.insert(key.into(), Value::String(cur + &add));
                if let Value::Object(src) = d {
                    for (k, v) in src {
                        let missing = l.get(k).is_none_or(|x| x.is_null() || x.as_str() == Some(""));
                        if k != key && missing && !v.is_null() {
                            l.insert(k.clone(), v.clone());
                        }
                    }
                }
            }
            return;
        }
    }
    details.push(d.clone());
}

pub(crate) fn parse_usage(u: &Value) -> Usage {
    let prompt = u64_at(u, "/prompt_tokens").unwrap_or(0);
    let cache_read = u64_at(u, "/prompt_tokens_details/cached_tokens")
        .or_else(|| u64_at(u, "/prompt_cache_hit_tokens"))
        .or_else(|| u64_at(u, "/cached_tokens"))
        .unwrap_or(0);
    let cache_write = u64_at(u, "/prompt_tokens_details/cache_write_tokens").unwrap_or(0);
    Usage {
        input: prompt.saturating_sub(cache_read + cache_write),
        output: u64_at(u, "/completion_tokens").unwrap_or(0),
        cache_read,
        cache_write,
        reasoning: u64_at(u, "/completion_tokens_details/reasoning_tokens").unwrap_or(0),
    }
}

impl Parser for ChatParser {
    fn feed(&mut self, ev: &SseEvent) -> Result<Vec<StreamEvent>, LlmError> {
        let mut out = Vec::new();
        let data = ev.data.trim();
        if data == "[DONE]" {
            self.finished = true;
            return Ok(out);
        }
        if data.is_empty() {
            return Ok(out);
        }
        let Some(chunk) = parse_json(data) else { return Ok(out) };
        if chunk.get("error").is_some_and(|e| !e.is_null()) || ev.event.as_deref() == Some("error") {
            return Err(LlmError::from_stream_payload(data));
        }
        if let Some(u) = chunk.get("usage").filter(|u| u.is_object()) {
            self.usage = Some(parse_usage(u));
        }
        if let Some(choices) = chunk.get("choices").and_then(Value::as_array) {
            // Only the first choice is used (n = 1).
            if let Some(choice) = choices.iter().find(|c| c.get("index").and_then(Value::as_u64).unwrap_or(0) == 0) {
                if let Some(u) = choice.get("usage").filter(|u| u.is_object()) {
                    self.usage = Some(parse_usage(u)); // Moonshot
                }
                if let Some(delta) = choice.get("delta").filter(|d| d.is_object()) {
                    self.handle_delta(delta, false, &mut out);
                } else if let Some(msg) = choice.get("message").filter(|d| d.is_object()) {
                    // Non-streaming response.
                    self.handle_delta(msg, true, &mut out);
                    if self.finish_reason.is_none() {
                        self.finish_reason = Some("stop".into());
                    }
                }
                if let Some(r) = choice.get("finish_reason").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                    self.finish_reason = Some(r.to_string());
                }
            }
        }
        Ok(out)
    }

    fn done(&self) -> bool {
        self.finished
    }

    fn finish(&mut self) -> Result<(Vec<StreamEvent>, FinishReason), LlmError> {
        let mut out = Vec::new();
        if !self.finished && self.finish_reason.is_none() {
            return Err(LlmError::incomplete("no finish_reason or [DONE]"));
        }
        self.end_reasoning(&mut out);
        for t in self.tools.iter_mut() {
            if !t.started {
                t.started = true;
                out.push(StreamEvent::ToolCallStart { id: t.id.clone(), name: t.name.clone() });
                if !t.args.is_empty() {
                    out.push(StreamEvent::ToolCallDelta { id: t.id.clone(), delta: t.args.clone() });
                }
            }
            let arguments = if t.args.trim().is_empty() { "{}".to_string() } else { t.args.clone() };
            out.push(StreamEvent::ToolCallEnd { id: t.id.clone(), arguments });
        }
        if let Some(u) = self.usage.take() {
            out.push(StreamEvent::Usage(u));
        }
        let reason = match self.finish_reason.as_deref() {
            None | Some("stop" | "end" | "eos" | "end_turn" | "stop_sequence") => {
                if self.saw_tool_calls { FinishReason::ToolUse } else { FinishReason::Stop }
            }
            Some("length" | "max_tokens") => FinishReason::Length,
            Some("tool_calls" | "function_call" | "tool_use") => FinishReason::ToolUse,
            Some("content_filter" | "sensitive") => FinishReason::ContentFilter,
            Some("network_error") => {
                return Err(LlmError::new(ErrorKind::Retryable, "provider reported finish_reason network_error"))
            }
            Some("model_context_window_exceeded") => {
                return Err(LlmError::new(ErrorKind::ContextOverflow, "model_context_window_exceeded"))
            }
            Some(other) => {
                if self.saw_tool_calls { FinishReason::ToolUse } else { FinishReason::Other(other.to_string()) }
            }
        };
        Ok((out, reason))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::run_parser;
    use serde_json::json;

    fn spec(base: &str, model: &str) -> ModelSpec {
        ModelSpec {
            provider_id: "p".into(),
            api_model: model.into(),
            protocol: Some(Protocol::Chat),
            base_url: base.into(),
            api_key: Some("k".into()),
            output: 8000,
            ..Default::default()
        }
    }

    #[test]
    fn parses_reasoning_text_tools_usage() {
        let raw = r#"data: {"choices":[{"index":0,"delta":{"role":"assistant","reasoning_content":"Let me"}}]}

data: {"choices":[{"index":0,"delta":{"reasoning_content":" think"}}]}

data: {"choices":[{"index":0,"delta":{"content":"Hello"}}]}

data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"read","arguments":""}}]}}]}

data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"path\":"}}]}}]}

data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"a.rs\"}"}}]}}]}

data: {"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}

data: {"choices":[],"usage":{"prompt_tokens":1200,"completion_tokens":80,"prompt_tokens_details":{"cached_tokens":1024},"completion_tokens_details":{"reasoning_tokens":40}}}

data: [DONE]

"#;
        let (events, reason) = run_parser(&mut ChatParser::default(), raw).unwrap();
        assert_eq!(reason, FinishReason::ToolUse);
        let dbg: Vec<String> = events.iter().map(|e| format!("{e:?}")).collect();
        assert!(matches!(&events[0], StreamEvent::ReasoningDelta(s) if s == "Let me"));
        assert!(matches!(&events[2], StreamEvent::ReasoningEnd { replay } if replay["field"] == "reasoning_content"), "{dbg:?}");
        assert!(matches!(&events[3], StreamEvent::TextDelta(s) if s == "Hello"));
        assert!(matches!(&events[4], StreamEvent::ToolCallStart { id, name } if id == "call_1" && name == "read"));
        let end = events.iter().find_map(|e| match e {
            StreamEvent::ToolCallEnd { id, arguments } => Some((id.clone(), arguments.clone())),
            _ => None,
        });
        assert_eq!(end, Some(("call_1".into(), r#"{"path":"a.rs"}"#.into())));
        let usage = events.iter().find_map(|e| match e {
            StreamEvent::Usage(u) => Some(u.clone()),
            _ => None,
        });
        let u = usage.unwrap();
        assert_eq!((u.input, u.cache_read, u.output, u.reasoning), (176, 1024, 80, 40));
    }

    #[test]
    fn openrouter_details_and_deepseek_usage() {
        let raw = r#"data: {"choices":[{"delta":{"reasoning":"a","reasoning_details":[{"type":"reasoning.text","text":"a","index":0}]}}]}

data: {"choices":[{"delta":{"reasoning":"b","reasoning_details":[{"type":"reasoning.text","text":"b","signature":"sig","index":0}]}}]}

data: {"choices":[{"delta":{"content":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":100,"completion_tokens":5,"prompt_cache_hit_tokens":60}}

"#;
        let (events, reason) = run_parser(&mut ChatParser::default(), raw).unwrap();
        assert_eq!(reason, FinishReason::Stop);
        let replay = events
            .iter()
            .find_map(|e| match e {
                StreamEvent::ReasoningEnd { replay } => Some(replay.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(replay, json!({"field": "reasoning", "details": [{"type":"reasoning.text","text":"ab","index":0,"signature":"sig"}]}));
        let u = events.iter().find_map(|e| if let StreamEvent::Usage(u) = e { Some(u.clone()) } else { None }).unwrap();
        assert_eq!((u.input, u.cache_read), (40, 60));
    }

    #[test]
    fn incomplete_stream_and_error_chunk() {
        let raw = "data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n\n";
        let err = run_parser(&mut ChatParser::default(), raw).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Retryable);
        let raw = "data: {\"error\":{\"message\":\"Overloaded\",\"code\":503}}\n\n";
        let err = run_parser(&mut ChatParser::default(), raw).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Retryable);
    }

    #[test]
    fn tool_call_without_index_and_new_id() {
        let raw = r#"data: {"choices":[{"delta":{"tool_calls":[{"id":"a","function":{"name":"x","arguments":"{}"}}]}}]}

data: {"choices":[{"delta":{"tool_calls":[{"id":"b","function":{"name":"y","arguments":"{\"q\":1}"}}]}}]}

data: [DONE]

"#;
        let (events, reason) = run_parser(&mut ChatParser::default(), raw).unwrap();
        assert_eq!(reason, FinishReason::ToolUse);
        let ends: Vec<_> = events.iter().filter(|e| matches!(e, StreamEvent::ToolCallEnd { .. })).collect();
        assert_eq!(ends.len(), 2);
    }

    fn history() -> Vec<LlmMessage> {
        vec![
            LlmMessage::User(vec![Content::Text("hi".into())]),
            LlmMessage::Assistant {
                parts: vec![
                    AssistantPart::Reasoning { text: "thinking".into(), replay: json!({"field": "reasoning_content"}) },
                    AssistantPart::Text("Reading.".into()),
                    AssistantPart::ToolCall { id: "call_1".into(), name: "read".into(), arguments: "{\"p\":1}".into() },
                ],
                replay: true,
            },
            LlmMessage::ToolResult {
                call_id: "call_1".into(),
                name: "read".into(),
                content: "data".into(),
                images: vec![ImageData { mime: "image/png".into(), data: "AAAA".into() }],
                is_error: false,
            },
            LlmMessage::Assistant { parts: vec![AssistantPart::Text("done".into())], replay: false },
        ]
    }

    #[test]
    fn body_messages_and_options() {
        let mut s = spec("https://open.bigmodel.cn/api/paas/v4", "glm-4.6");
        s.options = serde_json::from_value(json!({
            "reasoningEffort": "high", "textVerbosity": "low", "thinking": {"type": "enabled"},
            "apiKey": "secret", "include": ["x"], "store": false, "chat_template_kwargs": {"a": 1}
        }))
        .unwrap();
        s.body = serde_json::from_value(json!({"thinking": {"clear_thinking": false}})).unwrap();
        let req = ChatRequest {
            system: vec!["A".into(), "B".into()],
            messages: history(),
            tools: vec![ToolDef { name: "read".into(), description: "r".into(), parameters: json!({"type":"object"}) }],
            session_id: "ses_1".into(),
            max_tokens: None,
        };
        let b = build_body(&s, &req);
        assert_eq!(b["messages"][0], json!({"role": "system", "content": "A\n\nB"}));
        assert_eq!(b["messages"][1], json!({"role": "user", "content": "hi"}));
        let a = &b["messages"][2];
        assert_eq!(a["content"], "Reading.");
        assert_eq!(a["reasoning_content"], "thinking");
        assert_eq!(a["tool_calls"][0]["function"]["arguments"], "{\"p\":1}");
        assert_eq!(b["messages"][3], json!({"role": "tool", "tool_call_id": "call_1", "content": "data"}));
        assert_eq!(b["messages"][4]["role"], "user");
        assert_eq!(b["messages"][4]["content"][1]["image_url"]["url"], "data:image/png;base64,AAAA");
        assert!(b["messages"][5].get("reasoning_content").is_none());
        assert_eq!(b["reasoning_effort"], "high");
        assert_eq!(b["verbosity"], "low");
        assert_eq!(b["thinking"], json!({"type": "enabled", "clear_thinking": false}));
        assert_eq!(b["chat_template_kwargs"], json!({"a": 1}));
        assert_eq!(b["tool_stream"], true);
        assert_eq!(b["max_tokens"], 8000);
        for k in ["apiKey", "include", "store", "prompt_cache_key", "max_completion_tokens"] {
            assert!(b.get(k).is_none(), "{k}");
        }
    }

    #[test]
    fn deepseek_and_openai_specifics() {
        let s = spec("https://api.deepseek.com", "deepseek-chat");
        let mut req = ChatRequest { messages: history(), session_id: "ses".into(), ..Default::default() };
        let b = build_body(&s, &req);
        assert_eq!(b["messages"][1]["reasoning_content"], "thinking");
        assert_eq!(b["messages"][4]["reasoning_content"], "");
        assert_eq!(b["tools"], json!([]));

        let mut s = spec("https://api.openai.com/v1", "gpt-5");
        s.options.insert("store".into(), json!(false));
        req.max_tokens = Some(100);
        let b = build_body(&s, &req);
        assert_eq!(b["max_completion_tokens"], 100);
        assert_eq!(b["prompt_cache_key"], "ses");
        assert_eq!(b["store"], false);
        assert!(b.get("tools").is_none());
        // Reasoning field not observed/configured: nothing replayed for OpenAI.
        let mut req2 = req.clone();
        req2.messages[1] = LlmMessage::Assistant {
            parts: vec![AssistantPart::Reasoning { text: "t".into(), replay: json!({"field": null}) }, AssistantPart::Text("x".into())],
            replay: true,
        };
        let b = build_body(&s, &req2);
        assert!(b["messages"][1].get("reasoning_content").is_none());
    }
}
