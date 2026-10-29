//! OpenAI Responses API (`POST {base_url}/responses`), stateless (`store: false`).
//!
//! Design notes:
//! - Tool call ids seen by the caller are `"{call_id}|{item_id}"` (as in pi), e.g.
//!   `call_abc|fc_123`. The caller echoes that id back in [`LlmMessage::ToolResult::call_id`];
//!   on the wire `call_id` is the part before `|`, and the `fc_` item id is only replayed when
//!   the assistant message has `replay == true`. Ids without `|` are used as `call_id` as-is.
//! - Reasoning replay data ([`StreamEvent::ReasoningEnd`]) is the complete `reasoning` output
//!   item JSON (`{type, id, summary, encrypted_content, ...}`) and is replayed verbatim.
//! - Options: known camelCase keys map to wire params (see [`apply_options`]); transport keys
//!   are dropped; every other key passes through unchanged.

use crate::common::*;
use crate::sse::SseEvent;
use crate::*;
use reqwest::header::{HeaderMap, ACCEPT, AUTHORIZATION};
use std::collections::{HashMap, HashSet};

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
    "setCacheKey",
    "strictJsonSchema",
];

pub(crate) async fn stream(
    client: &reqwest::Client,
    spec: &ModelSpec,
    req: &ChatRequest,
    cancel: &CancellationToken,
    on_event: &mut (dyn FnMut(StreamEvent) + Send),
) -> Result<FinishReason, LlmError> {
    let url = format!("{}/responses", spec.base_url.trim_end_matches('/'));
    let body = build_body(spec, req);
    let mut parser = ResponsesParser::default();
    run(client, spec, &url, headers(spec, req), &body, &mut parser, cancel, on_event).await
}

fn is_codex(spec: &ModelSpec) -> bool {
    host_has(spec, "chatgpt.com/backend-api")
}

fn headers(spec: &ModelSpec, req: &ChatRequest) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(ACCEPT, reqwest::header::HeaderValue::from_static("text/event-stream"));
    if let Some(key) = spec.api_key.as_deref().filter(|k| !k.is_empty()) {
        set_header(&mut h, AUTHORIZATION.as_str(), &format!("Bearer {key}"));
    }
    if !req.session_id.is_empty() {
        set_header(&mut h, "session_id", &req.session_id);
        if is_codex(spec) {
            set_header(&mut h, "session-id", &req.session_id);
        }
    }
    apply_headers(&mut h, &spec.headers);
    h
}

/// Split a caller-visible tool call id into `(call_id, item_id)`.
pub(crate) fn split_call_id(id: &str) -> (&str, Option<&str>) {
    match id.split_once('|') {
        Some((call, item)) if !call.is_empty() => (call, Some(item).filter(|s| !s.is_empty())),
        _ => (id, None),
    }
}

pub(crate) fn build_body(spec: &ModelSpec, req: &ChatRequest) -> Value {
    let mut body = Map::new();
    body.insert("model".into(), Value::String(spec.api_model.clone()));
    let instructions = join_system(&req.system);
    if !instructions.is_empty() || is_codex(spec) {
        let text = if instructions.is_empty() { "You are a helpful assistant.".to_string() } else { instructions };
        body.insert("instructions".into(), Value::String(text));
    }
    let store = spec.options.get("store").and_then(Value::as_bool).unwrap_or(false);
    body.insert("input".into(), Value::Array(build_input(req, store)));
    if !req.tools.is_empty() {
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": "function", "name": t.name, "description": t.description,
                    "parameters": t.parameters, "strict": false,
                })
            })
            .collect();
        body.insert("tools".into(), Value::Array(tools));
        body.insert("tool_choice".into(), Value::String("auto".into()));
    }
    body.insert("stream".into(), Value::Bool(true));
    body.insert("store".into(), Value::Bool(store));
    if !req.session_id.is_empty() {
        body.insert("prompt_cache_key".into(), Value::String(truncate_chars(&req.session_id, 64)));
    }
    if let Some(max) = max_tokens(spec, req).filter(|_| !is_codex(spec)) {
        body.insert("max_output_tokens".into(), Value::from(max));
    }
    apply_options(&mut body, spec);
    if spec.reasoning && !store && !body.contains_key("include") {
        body.insert("include".into(), serde_json::json!(["reasoning.encrypted_content"]));
    }
    merge_extra_body(&mut body, spec);
    Value::Object(body)
}

/// Map opencode/`@ai-sdk/openai` provider options onto the request body.
fn apply_options(body: &mut Map<String, Value>, spec: &ModelSpec) {
    for (k, v) in &spec.options {
        if IGNORED_OPTIONS.contains(&k.as_str()) || v.is_null() {
            continue;
        }
        match k.as_str() {
            "reasoningEffort" => set_nested(body, "reasoning", "effort", v.clone()),
            "reasoningSummary" => set_nested(body, "reasoning", "summary", v.clone()),
            "textVerbosity" => set_nested(body, "text", "verbosity", v.clone()),
            "reasoning" | "text" => {
                if let Value::Object(src) = v {
                    let entry = body.entry(k.clone()).or_insert_with(|| Value::Object(Map::new()));
                    if let Value::Object(dst) = entry {
                        merge_json(dst, src);
                    }
                }
            }
            "serviceTier" | "parallelToolCalls" | "promptCacheKey" | "promptCacheRetention" | "safetyIdentifier"
            | "maxToolCalls" | "previousResponseId" | "topLogprobs" => {
                body.insert(snake_case(k), v.clone());
            }
            _ => {
                body.insert(k.clone(), v.clone());
            }
        }
    }
}

fn input_image(img: &ImageData) -> Value {
    serde_json::json!({"type": "input_image", "image_url": data_url(img), "detail": "auto"})
}

fn build_input(req: &ChatRequest, store: bool) -> Vec<Value> {
    let mut out = Vec::new();
    for msg in normalize(&req.messages) {
        match msg {
            LlmMessage::User(content) => {
                let parts: Vec<Value> = content
                    .iter()
                    .filter_map(|c| match c {
                        Content::Text(t) if t.is_empty() => None,
                        Content::Text(t) => Some(serde_json::json!({"type": "input_text", "text": t})),
                        Content::Image(img) => Some(input_image(img)),
                    })
                    .collect();
                if !parts.is_empty() {
                    out.push(serde_json::json!({"role": "user", "content": parts}));
                }
            }
            LlmMessage::Assistant { parts, replay } => {
                let mut text = String::new();
                let flush = |out: &mut Vec<Value>, text: &mut String| {
                    if !text.trim().is_empty() {
                        out.push(serde_json::json!({
                            "type": "message", "role": "assistant", "status": "completed",
                            "content": [{"type": "output_text", "text": text, "annotations": []}],
                        }));
                    }
                    text.clear();
                };
                for p in &parts {
                    match p {
                        AssistantPart::Text(t) => text.push_str(t),
                        AssistantPart::Reasoning { replay: data, .. } => {
                            if !replay || data.get("type").and_then(Value::as_str) != Some("reasoning") {
                                continue;
                            }
                            // Without encrypted content a stateless request cannot resolve the item.
                            let has_enc = data.get("encrypted_content").and_then(Value::as_str).is_some_and(|s| !s.is_empty());
                            if !has_enc && !store {
                                continue;
                            }
                            flush(&mut out, &mut text);
                            out.push(data.clone());
                        }
                        AssistantPart::ToolCall { id, name, arguments } => {
                            flush(&mut out, &mut text);
                            let (call_id, item_id) = split_call_id(id);
                            let mut item = Map::new();
                            item.insert("type".into(), "function_call".into());
                            if let Some(item_id) = item_id.filter(|_| replay) {
                                item.insert("id".into(), Value::String(item_id.to_string()));
                            }
                            item.insert("call_id".into(), Value::String(call_id.to_string()));
                            item.insert("name".into(), Value::String(name.clone()));
                            let args = if arguments.trim().is_empty() { "{}" } else { arguments.as_str() };
                            item.insert("arguments".into(), Value::String(args.to_string()));
                            out.push(Value::Object(item));
                        }
                    }
                }
                flush(&mut out, &mut text);
            }
            LlmMessage::ToolResult { call_id, content, images, .. } => {
                let (call_id, _) = split_call_id(&call_id);
                let output = if images.is_empty() {
                    Value::String(tool_output_text(&content))
                } else {
                    let mut parts = vec![serde_json::json!({"type": "input_text", "text": tool_output_text(&content)})];
                    parts.extend(images.iter().map(input_image));
                    Value::Array(parts)
                };
                out.push(serde_json::json!({"type": "function_call_output", "call_id": call_id, "output": output}));
            }
        }
    }
    out
}

#[derive(Debug)]
enum Slot {
    Reasoning { streamed: bool, summary_index: Option<u64> },
    Message { streamed: String },
    Tool { id: String, args: String },
}

/// Incremental parser for Responses SSE events, keyed by `output_index`.
#[derive(Debug, Default)]
pub(crate) struct ResponsesParser {
    slots: HashMap<u64, Slot>,
    /// item id → output_index, for hosts that omit output_index on deltas.
    item_index: HashMap<String, u64>,
    /// output_index values whose `output_item.done` was handled.
    done_items: HashSet<u64>,
    terminal: bool,
    reason: Option<FinishReason>,
    usage: Option<Usage>,
    saw_tool_call: bool,
    unfinished_tools: usize,
}

fn usage_of(u: &Value) -> Usage {
    let input = u64_at(u, "/input_tokens").unwrap_or(0);
    let cache_read = u64_at(u, "/input_tokens_details/cached_tokens").unwrap_or(0);
    let cache_write = u64_at(u, "/input_tokens_details/cache_write_tokens").unwrap_or(0);
    Usage {
        input: input.saturating_sub(cache_read + cache_write),
        output: u64_at(u, "/output_tokens").unwrap_or(0),
        cache_read,
        cache_write,
        reasoning: u64_at(u, "/output_tokens_details/reasoning_tokens").unwrap_or(0),
    }
}

impl ResponsesParser {
    fn index_of(&self, ev: &Value) -> Option<u64> {
        ev.get("output_index")
            .and_then(Value::as_u64)
            .or_else(|| ev.get("item_id").and_then(Value::as_str).and_then(|id| self.item_index.get(id).copied()))
    }

    fn item_added(&mut self, idx: u64, item: &Value, out: &mut Vec<StreamEvent>) {
        if self.slots.contains_key(&idx) || self.done_items.contains(&idx) {
            return;
        }
        if let Some(id) = item.get("id").and_then(Value::as_str) {
            self.item_index.insert(id.to_string(), idx);
        }
        let slot = match item.get("type").and_then(Value::as_str) {
            Some("reasoning") => Slot::Reasoning { streamed: false, summary_index: None },
            Some("message") => Slot::Message { streamed: String::new() },
            Some("function_call") => {
                let call_id = item.get("call_id").and_then(Value::as_str).filter(|s| !s.is_empty());
                let call_id = call_id.map_or_else(generate_call_id, str::to_string);
                let id = match item.get("id").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                    Some(item_id) => format!("{call_id}|{item_id}"),
                    None => call_id,
                };
                let name = item.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                self.saw_tool_call = true;
                self.unfinished_tools += 1;
                out.push(StreamEvent::ToolCallStart { id: id.clone(), name });
                let args = item.get("arguments").and_then(Value::as_str).unwrap_or("").to_string();
                if !args.is_empty() {
                    out.push(StreamEvent::ToolCallDelta { id: id.clone(), delta: args.clone() });
                }
                Slot::Tool { id, args }
            }
            _ => return,
        };
        self.slots.insert(idx, slot);
    }

    fn item_done(&mut self, idx: u64, item: &Value, out: &mut Vec<StreamEvent>) {
        // Hosts that skip `output_item.added` still deliver the full item here.
        self.item_added(idx, item, out);
        let Some(slot) = self.slots.remove(&idx) else { return };
        self.done_items.insert(idx);
        match slot {
            Slot::Reasoning { streamed, .. } => {
                if !streamed {
                    let join = |key: &str| -> String {
                        item.get(key)
                            .and_then(Value::as_array)
                            .map(|a| a.iter().filter_map(|p| p.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n\n"))
                            .unwrap_or_default()
                    };
                    let text = Some(join("summary")).filter(|s| !s.is_empty()).unwrap_or_else(|| join("content"));
                    if !text.is_empty() {
                        out.push(StreamEvent::ReasoningDelta(text));
                    }
                }
                out.push(StreamEvent::ReasoningEnd { replay: item.clone() });
            }
            Slot::Message { streamed } => {
                let full: String = item
                    .get("content")
                    .and_then(Value::as_array)
                    .map(|parts| {
                        parts
                            .iter()
                            .filter_map(|p| p.get("text").or_else(|| p.get("refusal")).and_then(Value::as_str))
                            .collect()
                    })
                    .unwrap_or_default();
                // The final item is authoritative; emit whatever the deltas missed.
                if let Some(rest) = full.strip_prefix(streamed.as_str()) {
                    if !rest.is_empty() {
                        out.push(StreamEvent::TextDelta(rest.to_string()));
                    }
                } else if streamed.is_empty() {
                    out.push(StreamEvent::TextDelta(full));
                }
            }
            Slot::Tool { id, args } => {
                let final_args = item.get("arguments").and_then(Value::as_str).filter(|s| !s.is_empty());
                let arguments = final_args.map_or(args, str::to_string);
                let arguments = if arguments.trim().is_empty() { "{}".to_string() } else { arguments };
                self.unfinished_tools = self.unfinished_tools.saturating_sub(1);
                out.push(StreamEvent::ToolCallEnd { id, arguments });
            }
        }
    }

    fn terminal_response(&mut self, resp: &Value, out: &mut Vec<StreamEvent>) {
        self.terminal = true;
        // Non-streaming bodies (and hosts that skip item events) carry the items only here.
        if let Some(items) = resp.get("output").and_then(Value::as_array) {
            for (i, item) in items.iter().enumerate() {
                let idx = i as u64;
                let id = item.get("id").and_then(Value::as_str);
                let seen = id.is_some_and(|id| self.item_index.contains_key(id));
                if !seen && !self.slots.contains_key(&idx) && !self.done_items.contains(&idx) {
                    self.item_done(idx, item, out);
                }
            }
        }
        if let Some(u) = resp.get("usage").filter(|u| u.is_object()) {
            self.usage = Some(usage_of(u));
        }
    }
}

impl Parser for ResponsesParser {
    fn feed(&mut self, ev: &SseEvent) -> Result<Vec<StreamEvent>, LlmError> {
        let mut out = Vec::new();
        let data = ev.data.trim();
        if data.is_empty() || data == "[DONE]" {
            return Ok(out);
        }
        let Some(v) = parse_json(data) else { return Ok(out) };
        let ty = v.get("type").and_then(Value::as_str).or(ev.event.as_deref()).unwrap_or("");
        match ty {
            "response.output_item.added" => {
                if let (Some(idx), Some(item)) = (self.index_of(&v), v.get("item")) {
                    self.item_added(idx, item, &mut out);
                }
            }
            "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                let delta = v.get("delta").and_then(Value::as_str).unwrap_or("");
                let part = v.get("summary_index").or_else(|| v.get("content_index")).and_then(Value::as_u64);
                let idx = self.index_of(&v);
                if let Some(Slot::Reasoning { streamed, summary_index }) = idx.and_then(|i| self.slots.get_mut(&i)) {
                    if delta.is_empty() {
                        return Ok(out);
                    }
                    // Separate summary parts with a blank line.
                    if *streamed && part.is_some() && part != *summary_index {
                        out.push(StreamEvent::ReasoningDelta("\n\n".into()));
                    }
                    *summary_index = part;
                    *streamed = true;
                    out.push(StreamEvent::ReasoningDelta(delta.to_string()));
                }
            }
            "response.output_text.delta" | "response.refusal.delta" => {
                let delta = v.get("delta").and_then(Value::as_str).unwrap_or("");
                if let Some(Slot::Message { streamed }) = self.index_of(&v).and_then(|i| self.slots.get_mut(&i)) {
                    if !delta.is_empty() {
                        streamed.push_str(delta);
                        out.push(StreamEvent::TextDelta(delta.to_string()));
                    }
                }
            }
            "response.function_call_arguments.delta" => {
                let delta = v.get("delta").and_then(Value::as_str).unwrap_or("");
                if let Some(Slot::Tool { id, args }) = self.index_of(&v).and_then(|i| self.slots.get_mut(&i)) {
                    if !delta.is_empty() {
                        args.push_str(delta);
                        out.push(StreamEvent::ToolCallDelta { id: id.clone(), delta: delta.to_string() });
                    }
                }
            }
            "response.function_call_arguments.done" => {
                if let Some(Slot::Tool { args, .. }) = self.index_of(&v).and_then(|i| self.slots.get_mut(&i)) {
                    if let Some(a) = v.get("arguments").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                        *args = a.to_string();
                    }
                }
            }
            "response.output_item.done" => {
                if let (Some(idx), Some(item)) = (self.index_of(&v), v.get("item")) {
                    self.item_done(idx, item, &mut out);
                }
            }
            "response.completed" => {
                let resp = v.get("response").cloned().unwrap_or(Value::Null);
                self.terminal_response(&resp, &mut out);
                self.reason = Some(FinishReason::Stop);
            }
            "response.incomplete" => {
                let resp = v.get("response").cloned().unwrap_or(Value::Null);
                self.terminal_response(&resp, &mut out);
                let why = str_at(&resp, "/incomplete_details/reason").unwrap_or("unknown");
                self.reason = Some(match why {
                    "max_output_tokens" => FinishReason::Length,
                    "content_filter" => FinishReason::ContentFilter,
                    other => FinishReason::Other(other.to_string()),
                });
            }
            "response.failed" => {
                self.terminal = true;
                let err = v.pointer("/response/error").cloned().unwrap_or(Value::Null);
                let payload = serde_json::json!({"error": err}).to_string();
                let mut e = LlmError::from_stream_payload(&payload);
                if err.is_null() {
                    e.message = "provider error: response.failed without details".into();
                }
                return Err(e);
            }
            "error" => {
                let payload = if v.get("error").is_some() { data.to_string() } else { serde_json::json!({"error": v}).to_string() };
                return Err(LlmError::from_stream_payload(&payload));
            }
            "" if v.get("output").is_some() => {
                // Non-streaming response object.
                self.terminal_response(&v, &mut out);
                self.reason = Some(match str_at(&v, "/incomplete_details/reason") {
                    Some("max_output_tokens") => FinishReason::Length,
                    _ => FinishReason::Stop,
                });
            }
            _ => {} // created, in_progress, content_part.*, *.done, ...
        }
        Ok(out)
    }

    fn done(&self) -> bool {
        self.terminal
    }

    fn finish(&mut self) -> Result<(Vec<StreamEvent>, FinishReason), LlmError> {
        if !self.terminal {
            return Err(LlmError::incomplete("no response.completed event"));
        }
        let mut reason = self.reason.take().unwrap_or(FinishReason::Stop);
        if self.saw_tool_call && reason == FinishReason::Stop {
            if self.unfinished_tools > 0 {
                // Arguments may be truncated or mixed up: refuse to hand them over.
                return Err(LlmError::incomplete("tool call without output_item.done"));
            }
            reason = FinishReason::ToolUse;
        }
        let mut out = Vec::new();
        if let Some(u) = self.usage.take() {
            out.push(StreamEvent::Usage(u));
        }
        Ok((out, reason))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sse(events: &[Value]) -> String {
        events
            .iter()
            .map(|e| format!("event: {}\ndata: {}\n\n", e["type"].as_str().unwrap_or(""), e))
            .collect()
    }

    #[test]
    fn parses_full_stream() {
        let reasoning_item = json!({"type":"reasoning","id":"rs_1","summary":[{"type":"summary_text","text":"A"},{"type":"summary_text","text":"B"}],"encrypted_content":"gAAA"});
        let raw = sse(&[
            json!({"type":"response.created","response":{"id":"resp_1"}}),
            json!({"type":"response.output_item.added","output_index":0,"item":{"type":"reasoning","id":"rs_1","summary":[]}}),
            json!({"type":"response.reasoning_summary_text.delta","output_index":0,"summary_index":0,"delta":"A"}),
            json!({"type":"response.reasoning_summary_part.done","output_index":0,"summary_index":0}),
            json!({"type":"response.reasoning_summary_text.delta","output_index":0,"summary_index":1,"delta":"B"}),
            json!({"type":"response.output_item.done","output_index":0,"item":reasoning_item}),
            json!({"type":"response.output_item.added","output_index":1,"item":{"type":"message","id":"msg_1","role":"assistant","content":[]}}),
            json!({"type":"response.output_text.delta","output_index":1,"content_index":0,"delta":"Hel"}),
            json!({"type":"response.output_item.done","output_index":1,"item":{"type":"message","id":"msg_1","content":[{"type":"output_text","text":"Hello"}]}}),
            json!({"type":"response.output_item.added","output_index":2,"item":{"type":"function_call","id":"fc_1","call_id":"call_1","name":"read","arguments":""}}),
            json!({"type":"response.function_call_arguments.delta","output_index":2,"item_id":"fc_1","delta":"{\"path\":"}),
            json!({"type":"response.function_call_arguments.done","output_index":2,"arguments":"{\"path\":\"a.rs\"}"}),
            json!({"type":"response.output_item.done","output_index":2,"item":{"type":"function_call","id":"fc_1","call_id":"call_1","name":"read","arguments":"{\"path\":\"a.rs\"}"}}),
            json!({"type":"response.completed","response":{"id":"resp_1","status":"completed","usage":{"input_tokens":5000,"input_tokens_details":{"cached_tokens":4096},"output_tokens":300,"output_tokens_details":{"reasoning_tokens":200}}}}),
        ]);
        let (events, reason) = run_parser(&mut ResponsesParser::default(), &raw).unwrap();
        assert_eq!(reason, FinishReason::ToolUse);
        let reasoning: String =
            events.iter().filter_map(|e| if let StreamEvent::ReasoningDelta(d) = e { Some(d.as_str()) } else { None }).collect();
        assert_eq!(reasoning, "A\n\nB");
        let text: String =
            events.iter().filter_map(|e| if let StreamEvent::TextDelta(d) = e { Some(d.as_str()) } else { None }).collect();
        assert_eq!(text, "Hello");
        assert!(events.iter().any(|e| matches!(e, StreamEvent::ReasoningEnd { replay } if *replay == reasoning_item)));
        assert!(events.iter().any(|e| matches!(e, StreamEvent::ToolCallStart { id, name } if id == "call_1|fc_1" && name == "read")));
        assert!(events.iter().any(|e| matches!(e, StreamEvent::ToolCallEnd { id, arguments } if id == "call_1|fc_1" && arguments == "{\"path\":\"a.rs\"}")));
        let u = events.iter().find_map(|e| if let StreamEvent::Usage(u) = e { Some(u.clone()) } else { None }).unwrap();
        assert_eq!((u.input, u.cache_read, u.output, u.reasoning), (904, 4096, 300, 200));
    }

    #[test]
    fn incomplete_failed_and_unfinished() {
        let raw = sse(&[json!({"type":"response.created","response":{}})]);
        assert_eq!(run_parser(&mut ResponsesParser::default(), &raw).unwrap_err().kind, ErrorKind::Retryable);

        let raw = sse(&[json!({"type":"response.failed","response":{"error":{"code":"server_error","message":"boom"}}})]);
        assert_eq!(run_parser(&mut ResponsesParser::default(), &raw).unwrap_err().kind, ErrorKind::Retryable);

        let raw = sse(&[json!({"type":"error","code":"context_length_exceeded","message":"Your input exceeds the context window of this model."})]);
        assert_eq!(run_parser(&mut ResponsesParser::default(), &raw).unwrap_err().kind, ErrorKind::ContextOverflow);

        let raw = sse(&[
            json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"fc","call_id":"c","name":"x"}}),
            json!({"type":"response.completed","response":{}}),
        ]);
        assert!(run_parser(&mut ResponsesParser::default(), &raw).is_err());

        let raw = sse(&[json!({"type":"response.incomplete","response":{"incomplete_details":{"reason":"max_output_tokens"}}})]);
        assert_eq!(run_parser(&mut ResponsesParser::default(), &raw).unwrap().1, FinishReason::Length);
    }

    fn spec() -> ModelSpec {
        ModelSpec {
            provider_id: "openai".into(),
            api_model: "gpt-5".into(),
            protocol: Some(Protocol::Responses),
            base_url: "https://api.openai.com/v1".into(),
            output: 128000,
            reasoning: true,
            ..Default::default()
        }
    }

    #[test]
    fn variant_options_mapping() {
        let mut s = spec();
        s.options = serde_json::from_value(json!({
            "reasoningEffort": "high", "textVerbosity": "medium", "reasoningSummary": "auto",
            "include": ["reasoning.encrypted_content"], "store": false, "serviceTier": "flex",
            "apiKey": "secret", "custom_thing": 1
        }))
        .unwrap();
        let req = ChatRequest { system: vec!["sys".into()], session_id: "s".repeat(80), max_tokens: Some(1000), ..Default::default() };
        let b = build_body(&s, &req);
        assert_eq!(b["reasoning"], json!({"effort": "high", "summary": "auto"}));
        assert_eq!(b["text"], json!({"verbosity": "medium"}));
        assert_eq!(b["include"], json!(["reasoning.encrypted_content"]));
        assert_eq!(b["store"], false);
        assert_eq!(b["service_tier"], "flex");
        assert_eq!(b["custom_thing"], 1);
        assert_eq!(b["instructions"], "sys");
        assert_eq!(b["max_output_tokens"], 1000);
        assert_eq!(b["prompt_cache_key"].as_str().unwrap().len(), 64);
        assert!(b.get("apiKey").is_none());

        // Defaults: store false + encrypted reasoning for reasoning models.
        let b = build_body(&spec(), &ChatRequest::default());
        assert_eq!(b["store"], false);
        assert_eq!(b["include"], json!(["reasoning.encrypted_content"]));
    }

    #[test]
    fn input_items_and_replay() {
        let item = json!({"type":"reasoning","id":"rs_1","summary":[],"encrypted_content":"gAAA"});
        let history = |replay: bool| {
            vec![
                LlmMessage::User(vec![Content::Text("fix".into()), Content::Image(ImageData { mime: "image/png".into(), data: "AA".into() })]),
                LlmMessage::Assistant {
                    parts: vec![
                        AssistantPart::Reasoning { text: String::new(), replay: item.clone() },
                        AssistantPart::Text("I'll read.".into()),
                        AssistantPart::ToolCall { id: "call_1|fc_1".into(), name: "read".into(), arguments: "{}".into() },
                    ],
                    replay,
                },
                LlmMessage::ToolResult { call_id: "call_1|fc_1".into(), name: "read".into(), content: "x".into(), images: vec![], is_error: false },
            ]
        };
        let req = ChatRequest { messages: history(true), ..Default::default() };
        let b = build_body(&spec(), &req);
        let input = b["input"].as_array().unwrap();
        assert_eq!(input[0]["content"][1], json!({"type":"input_image","image_url":"data:image/png;base64,AA","detail":"auto"}));
        assert_eq!(input[1], item);
        assert_eq!(input[2]["content"][0]["text"], "I'll read.");
        assert_eq!(input[3], json!({"type":"function_call","id":"fc_1","call_id":"call_1","name":"read","arguments":"{}"}));
        assert_eq!(input[4], json!({"type":"function_call_output","call_id":"call_1","output":"x"}));

        let req = ChatRequest { messages: history(false), ..Default::default() };
        let b = build_body(&spec(), &req);
        let input = b["input"].as_array().unwrap();
        assert_eq!(input.len(), 4);
        assert!(input[2].get("id").is_none());
        assert_eq!(input[2]["call_id"], "call_1");
    }

    /// Captured from OpenRouter's Responses endpoint (nemotron, trimmed): raw `reasoning_text`
    /// deltas (no summary, no encrypted_content), `rs_tmp_`/`fc_tmp_` ids, `call-<uuid>` call ids.
    #[test]
    fn openrouter_responses_capture_and_replay() {
        let raw = sse(&[
            json!({"type":"response.created","response":{"id":"gen-1","status":"in_progress","output":[]}}),
            json!({"type":"response.output_item.added","output_index":0,"item":{"id":"rs_tmp_8vzb","type":"reasoning","status":"in_progress","summary":[]},"sequence_number":2}),
            json!({"type":"response.content_part.added","output_index":0,"item_id":"rs_tmp_8vzb","content_index":0,"part":{"type":"reasoning_text","text":""}}),
            json!({"type":"response.reasoning_text.delta","output_index":0,"item_id":"rs_tmp_8vzb","content_index":0,"delta":"Call the"}),
            json!({"type":"response.reasoning_text.delta","output_index":0,"item_id":"rs_tmp_8vzb","content_index":0,"delta":" tool."}),
            json!({"type":"response.reasoning_text.done","output_index":0,"item_id":"rs_tmp_8vzb","content_index":0,"text":"Call the tool."}),
            json!({"type":"response.output_item.done","output_index":0,"item":{"id":"rs_tmp_8vzb","type":"reasoning","status":"completed","content":[{"type":"reasoning_text","text":"Call the tool."}],"summary":[],"format":"unknown"}}),
            json!({"type":"response.output_item.added","output_index":1,"item":{"id":"fc_tmp_uz0e","type":"function_call","status":"in_progress","call_id":"call-697e7d10-f945-41c6-8e09-0c62f233b101","name":"get_weather","arguments":""}}),
            json!({"type":"response.function_call_arguments.delta","output_index":1,"item_id":"fc_tmp_uz0e","delta":"{\"city\":\"Paris\"}"}),
            json!({"type":"response.function_call_arguments.done","output_index":1,"item_id":"fc_tmp_uz0e","name":"get_weather","arguments":"{\"city\":\"Paris\"}"}),
            json!({"type":"response.output_item.done","output_index":1,"item":{"id":"fc_tmp_uz0e","type":"function_call","status":"completed","call_id":"call-697e7d10-f945-41c6-8e09-0c62f233b101","name":"get_weather","arguments":"{\"city\":\"Paris\"}"}}),
            json!({"type":"response.completed","response":{"id":"gen-1","status":"completed","output":[],"usage":{"input_tokens":311,"input_tokens_details":{"cached_tokens":0},"output_tokens":49,"output_tokens_details":{"reasoning_tokens":23},"total_tokens":360,"cost":0}}}),
        ]);
        let (events, reason) = run_parser(&mut ResponsesParser::default(), &raw).unwrap();
        assert_eq!(reason, FinishReason::ToolUse);
        let reasoning: String = events.iter().filter_map(|e| if let StreamEvent::ReasoningDelta(d) = e { Some(d.as_str()) } else { None }).collect();
        assert_eq!(reasoning, "Call the tool.");
        let id = "call-697e7d10-f945-41c6-8e09-0c62f233b101|fc_tmp_uz0e";
        assert!(events.iter().any(|e| matches!(e, StreamEvent::ToolCallEnd { id: i, arguments } if i == id && arguments == "{\"city\":\"Paris\"}")));
        let item = events.iter().find_map(|e| if let StreamEvent::ReasoningEnd { replay } = e { Some(replay.clone()) } else { None }).unwrap();
        // store:false + no encrypted_content: the reasoning item is not replayable and is skipped;
        // the call/result pair is replayed with the original ids (this round trip passed live).
        let req = ChatRequest {
            messages: vec![
                LlmMessage::User(vec![Content::Text("q".into())]),
                LlmMessage::Assistant {
                    parts: vec![
                        AssistantPart::Reasoning { text: reasoning.clone(), replay: item },
                        AssistantPart::ToolCall { id: id.into(), name: "get_weather".into(), arguments: "{\"city\":\"Paris\"}".into() },
                    ],
                    replay: true,
                },
                LlmMessage::ToolResult { call_id: id.into(), name: "get_weather".into(), content: "sunny".into(), images: vec![], is_error: false },
            ],
            ..Default::default()
        };
        let mut s = spec();
        s.base_url = "https://openrouter.ai/api/v1".into();
        let b = build_body(&s, &req);
        let input = b["input"].as_array().unwrap();
        assert_eq!(input.len(), 3);
        assert_eq!(input[1]["id"], "fc_tmp_uz0e");
        assert_eq!(input[1]["call_id"], "call-697e7d10-f945-41c6-8e09-0c62f233b101");
        assert_eq!(input[2]["call_id"], "call-697e7d10-f945-41c6-8e09-0c62f233b101");
    }
}
