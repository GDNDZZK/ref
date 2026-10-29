//! OpenAI Chat Completions (`POST {base}/chat/completions`) and the many
//! OpenAI-compatible hosts (DeepSeek, GLM, Qwen, Kimi, OpenRouter, vLLM, ...).

use crate::error::stream_error;
use crate::sse::{SseEvent, data_values, is_done};
use crate::util::{
    Assembler, CLIENT_OPTION_KEYS, DEFAULT_OPENAI_BASE, IMAGE_OMITTED, StreamEnd, StreamState,
    arguments_string, build_headers, data_url, deep_merge, drive, endpoint, error_fields,
    has_tool_history, is_official_openai, merge_key, normalize_call_id, normalize_history,
    prompt_cache_key, ptr_u64, same_origin, session_headers, snake_keys,
};
use crate::{
    AssistantBlock, BlockKind, ChatMessage, LlmError, Model, Provider, Request, Response,
    StreamEvent, UserContent,
};
use ref_proto::Usage;
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

/// OpenAI limits tool-call ids to 40 characters.
const MAX_CALL_ID: usize = 40;
/// Reasoning field used when echoing reasoning without a recorded field name.
const DEFAULT_REASONING_FIELD: &str = "reasoning_content";
/// Delta fields carrying reasoning text, in priority order.
const REASONING_FIELDS: &[&str] = &["reasoning_content", "reasoning", "reasoning_text"];

pub(crate) async fn stream(
    provider: &Provider,
    model: &Model,
    req: &Request,
    cancel: &CancellationToken,
    on_event: &mut (dyn FnMut(StreamEvent) + Send),
) -> Result<Response, LlmError> {
    let body = build_body(provider, model, req);
    let url = endpoint(&provider.base_url, DEFAULT_OPENAI_BASE, "chat/completions");
    let headers = request_headers(provider, req);
    drive(
        ChatState::default(),
        provider,
        &url,
        headers,
        &body,
        cancel,
        on_event,
    )
    .await
}

fn request_headers(provider: &Provider, req: &Request) -> reqwest::header::HeaderMap {
    let mut defaults = vec![
        ("content-type".to_string(), "application/json".to_string()),
        ("accept".to_string(), "text/event-stream".to_string()),
    ];
    if let Some(key) = provider
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty())
    {
        defaults.push(("authorization".into(), format!("Bearer {key}")));
    }
    defaults.extend(session_headers(
        &req.session_id,
        &["session_id", "x-session-id", "x-session-affinity"],
    ));
    build_headers(provider, defaults, &[])
}

/// Hosts that reject assistant messages without `reasoning_content`.
fn requires_reasoning_content(provider: &Provider, model: &Model) -> bool {
    let needle = "deepseek";
    provider.id.to_ascii_lowercase().contains(needle)
        || provider.base_url.to_ascii_lowercase().contains(needle)
        || model.id.to_ascii_lowercase().contains(needle)
}

pub(crate) fn build_body(provider: &Provider, model: &Model, req: &Request) -> Value {
    let mut body = Map::new();
    body.insert("model".into(), json!(model.id));
    body.insert(
        "messages".into(),
        Value::Array(convert_messages(provider, model, req)),
    );

    if !req.tools.is_empty() {
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                json!({"type": "function", "function": {
                    "name": t.name, "description": t.description, "parameters": t.parameters,
                }})
            })
            .collect();
        body.insert("tools".into(), Value::Array(tools));
    } else if has_tool_history(&req.messages) {
        // LiteLLM / Bedrock-Anthropic proxies require `tools` when history has tool calls.
        body.insert("tools".into(), json!([]));
    }

    body.insert("stream".into(), json!(true));
    body.insert("stream_options".into(), json!({"include_usage": true}));
    if model.output > 0 {
        // Official OpenAI reasoning models reject the legacy `max_tokens`.
        let field = if is_official_openai(provider) {
            "max_completion_tokens"
        } else {
            "max_tokens"
        };
        body.insert(field.into(), json!(model.output));
    }
    if let Some(t) = model.temperature {
        body.insert("temperature".into(), json!(t));
    }
    if let Some(p) = model.top_p {
        body.insert("top_p".into(), json!(p));
    }
    if provider.set_cache_key
        && let Some(key) = prompt_cache_key(&req.session_id)
    {
        body.insert("prompt_cache_key".into(), json!(key));
    }

    apply_options(provider, &model.options, &mut body);
    Value::Object(body)
}

/// Map opencode-style options onto the request body. Unknown keys are passed
/// through verbatim, like `@ai-sdk/openai-compatible` does.
fn apply_options(provider: &Provider, options: &Map<String, Value>, body: &mut Map<String, Value>) {
    let mut overlays = Vec::new();
    for (key, value) in options {
        match key.as_str() {
            "reasoningEffort" | "reasoning_effort" => {
                if !value.is_null() {
                    body.insert("reasoning_effort".into(), value.clone());
                }
            }
            "textVerbosity" => {
                body.insert("verbosity".into(), value.clone());
            }
            "parallelToolCalls" => {
                body.insert("parallel_tool_calls".into(), value.clone());
            }
            "serviceTier" => {
                body.insert("service_tier".into(), value.clone());
            }
            "safetyIdentifier" => {
                body.insert("safety_identifier".into(), value.clone());
            }
            "store" => {
                // Strict OpenAI-compatible servers reject unknown fields.
                if is_official_openai(provider) {
                    body.insert("store".into(), value.clone());
                }
            }
            "thinking" => {
                let converted = if value.is_object() {
                    snake_keys(value)
                } else {
                    value.clone()
                };
                merge_key(body, "thinking", &converted);
            }
            "extraBody" | "body" => {
                if value.is_object() {
                    overlays.push(value.clone());
                }
            }
            // opencode / Responses-only keys that chat endpoints do not accept.
            "include" | "reasoningSummary" | "promptCacheKey" | "setCacheKey"
            | "strictJsonSchema" | "effort" => {}
            k if CLIENT_OPTION_KEYS.contains(&k) => {}
            k => merge_key(body, k, value),
        }
    }
    let mut merged = Value::Object(std::mem::take(body));
    for overlay in &overlays {
        deep_merge(&mut merged, overlay);
    }
    if let Value::Object(map) = merged {
        *body = map;
    }
}

/// Lower the conversation to chat `messages`.
fn convert_messages(provider: &Provider, model: &Model, req: &Request) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let system: Vec<&str> = req
        .system
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if !system.is_empty() {
        out.push(json!({"role": "system", "content": system.join("\n\n")}));
    }

    let require_reasoning = requires_reasoning_content(provider, model);
    let history = normalize_history(&req.messages);
    // Images from the current run of tool results, sent as a user message afterwards.
    let mut tool_images: Vec<Value> = Vec::new();

    for (i, msg) in history.iter().enumerate() {
        match msg {
            ChatMessage::User(parts) => {
                if let Some(m) = user_message(model, parts) {
                    push_merged(&mut out, m);
                }
            }
            ChatMessage::Assistant { blocks, origin } => {
                let replay = same_origin(origin, provider, model);
                if let Some(m) = assistant_message(blocks, replay, require_reasoning) {
                    push_merged(&mut out, m);
                }
            }
            ChatMessage::ToolResult {
                call_id,
                content,
                images,
                ..
            } => {
                let mut text = content.clone();
                if !model.images && !images.is_empty() {
                    // No follow-up user message for models that cannot see images; just note it.
                    if !text.trim().is_empty() {
                        text.push_str("\n\n");
                    }
                    text.push_str(IMAGE_OMITTED);
                } else if text.trim().is_empty() {
                    text = if images.is_empty() {
                        "(no tool output)".into()
                    } else {
                        "(see attached image)".into()
                    };
                }
                out.push(json!({
                    "role": "tool",
                    "tool_call_id": normalize_call_id(call_id, MAX_CALL_ID),
                    "content": text,
                }));
                if model.images {
                    for img in images {
                        tool_images.push(json!({"type": "image_url", "image_url": {"url": data_url(img)}}));
                    }
                }
                let run_ends = !matches!(history.get(i + 1), Some(ChatMessage::ToolResult { .. }));
                if run_ends && !tool_images.is_empty() {
                    let mut content = vec![
                        json!({"type": "text", "text": "Attached image(s) from tool result:"}),
                    ];
                    content.append(&mut tool_images);
                    out.push(json!({"role": "user", "content": content}));
                }
            }
        }
    }
    out
}

fn user_message(model: &Model, parts: &[UserContent]) -> Option<Value> {
    let mut content = Vec::new();
    let mut has_image = false;
    for part in parts {
        match part {
            UserContent::Text(t) if !t.trim().is_empty() => {
                content.push(json!({"type": "text", "text": t}))
            }
            UserContent::Text(_) => {}
            UserContent::Image(img) if model.images => {
                has_image = true;
                content.push(json!({"type": "image_url", "image_url": {"url": data_url(img)}}));
            }
            UserContent::Image(_) => content.push(json!({"type": "text", "text": IMAGE_OMITTED})),
        }
    }
    if content.is_empty() {
        return None;
    }
    if !has_image {
        // Text-only messages use the plain string form (widest compatibility).
        let text: Vec<&str> = content.iter().filter_map(|c| c["text"].as_str()).collect();
        return Some(json!({"role": "user", "content": text.join("\n\n")}));
    }
    Some(json!({"role": "user", "content": content}))
}

fn assistant_message(
    blocks: &[AssistantBlock],
    replay: bool,
    require_reasoning: bool,
) -> Option<Value> {
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut field: Option<String> = None;
    let mut tool_calls = Vec::new();
    for block in blocks {
        match block {
            AssistantBlock::Text(t) => text.push_str(t),
            AssistantBlock::Reasoning {
                text: r,
                provider_data,
            } => {
                // Chat reasoning is plain text: echo it when it came from this
                // model, or (for hosts that require the field) when it was
                // recorded from a chat reasoning field by another model.
                let chat_origin = provider_data
                    .as_ref()
                    .is_some_and(|d| d.get("field").is_some());
                if replay || (chat_origin && require_reasoning) {
                    reasoning.push_str(r);
                    if field.is_none() {
                        field = provider_data
                            .as_ref()
                            .and_then(|d| d.get("field"))
                            .and_then(Value::as_str)
                            .filter(|f| REASONING_FIELDS.contains(f))
                            .map(str::to_string);
                    }
                }
            }
            AssistantBlock::ToolCall {
                id,
                name,
                arguments,
                ..
            } => tool_calls.push(json!({
                "id": normalize_call_id(id, MAX_CALL_ID),
                "type": "function",
                "function": {"name": name, "arguments": arguments_string(arguments)},
            })),
        }
    }
    if text.trim().is_empty() && tool_calls.is_empty() {
        return None;
    }
    let mut msg = Map::new();
    msg.insert("role".into(), json!("assistant"));
    msg.insert(
        "content".into(),
        if text.is_empty() {
            Value::Null
        } else {
            json!(text)
        },
    );
    if !reasoning.is_empty() {
        msg.insert(
            field.unwrap_or_else(|| DEFAULT_REASONING_FIELD.into()),
            json!(reasoning),
        );
    } else if require_reasoning {
        msg.insert(DEFAULT_REASONING_FIELD.into(), json!(""));
    }
    if !tool_calls.is_empty() {
        msg.insert("tool_calls".into(), Value::Array(tool_calls));
    }
    Some(Value::Object(msg))
}

/// Push a message, merging it into the previous one when both are user
/// messages (some hosts reject consecutive same-role messages) or both are
/// assistant messages.
fn push_merged(out: &mut Vec<Value>, msg: Value) {
    let role = msg["role"].as_str().unwrap_or("");
    let Some(prev) = out.last_mut() else {
        out.push(msg);
        return;
    };
    if prev["role"].as_str() != Some(role) || !matches!(role, "user" | "assistant") {
        out.push(msg);
        return;
    }
    if role == "user" {
        let a = std::mem::take(&mut prev["content"]);
        prev["content"] = merge_user_content(a, msg["content"].clone());
        return;
    }
    // Assistant: concatenate text / reasoning, append tool calls.
    let Value::Object(next) = msg else { return };
    for (k, v) in next {
        match (k.as_str(), prev.get_mut(&k)) {
            ("role", _) => {}
            ("tool_calls", Some(Value::Array(existing))) => {
                if let Value::Array(more) = v {
                    existing.extend(more);
                }
            }
            (_, Some(Value::String(existing))) => {
                if let Some(s) = v.as_str() {
                    if !existing.is_empty() && !s.is_empty() {
                        existing.push_str("\n\n");
                    }
                    existing.push_str(s);
                }
            }
            (_, Some(existing)) if existing.is_null() => *existing = v,
            (_, Some(_)) => {}
            (_, None) => {
                prev[k.as_str()] = v;
            }
        }
    }
}

fn merge_user_content(a: Value, b: Value) -> Value {
    match (a, b) {
        (Value::String(x), Value::String(y)) => Value::String(format!("{x}\n\n{y}")),
        (x, y) => {
            let to_parts = |v: Value| match v {
                Value::Array(items) => items,
                Value::String(s) => vec![json!({"type": "text", "text": s})],
                _ => Vec::new(),
            };
            let mut parts = to_parts(x);
            parts.extend(to_parts(y));
            Value::Array(parts)
        }
    }
}

// ---------------------------------------------------------------------------
// Streaming
// ---------------------------------------------------------------------------

/// A streamed tool call, keyed by the provider's `index`.
#[derive(Debug, Default)]
struct ToolSlot {
    key: Option<u64>,
    id: Option<String>,
    name: Option<String>,
    /// Arguments received before the block could be started.
    pending: String,
    block: Option<usize>,
}

#[derive(Debug, Default)]
pub(crate) struct ChatState {
    text: Option<usize>,
    reasoning: Option<usize>,
    reasoning_field: Option<String>,
    tools: Vec<ToolSlot>,
    finish_reason: Option<String>,
    saw_done: bool,
}

impl ChatState {
    fn on_chunk(&mut self, asm: &mut Assembler<'_>, chunk: &Value) -> Result<(), LlmError> {
        if let Some(err) = chunk.get("error").filter(|e| !e.is_null()) {
            let (code, message) = error_fields(err);
            return Err(stream_error(code.as_deref(), &message));
        }
        let choice = chunk
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|c| c.first());
        let usage = chunk.get("usage").filter(|u| u.is_object()).or_else(|| {
            choice
                .and_then(|c| c.get("usage"))
                .filter(|u| u.is_object())
        });
        if let Some(u) = usage {
            asm.usage(parse_usage(u));
        }
        let Some(choice) = choice else { return Ok(()) };

        if let Some(delta) = choice.get("delta").filter(|d| d.is_object()) {
            // Reasoning first: aggregators may send reasoning and content together.
            let reasoning = REASONING_FIELDS.iter().find_map(|f| {
                delta
                    .get(*f)
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(|s| (*f, s))
            });
            if let Some((field, text)) = reasoning {
                let idx = match self.reasoning.filter(|i| asm.is_open(*i)) {
                    Some(i) => i,
                    None => {
                        self.close_text(asm);
                        let i = asm.start(BlockKind::Reasoning);
                        self.reasoning = Some(i);
                        i
                    }
                };
                let field = self
                    .reasoning_field
                    .get_or_insert_with(|| field.to_string())
                    .clone();
                asm.set_provider_data(idx, json!({"field": field}));
                asm.delta(idx, text);
            }

            for key in ["content", "refusal"] {
                if let Some(text) = delta
                    .get(key)
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    let idx = match self.text.filter(|i| asm.is_open(*i)) {
                        Some(i) => i,
                        None => {
                            self.close_reasoning(asm);
                            let i = asm.start(BlockKind::Text);
                            self.text = Some(i);
                            i
                        }
                    };
                    asm.delta(idx, text);
                }
            }

            if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                for (pos, call) in calls.iter().enumerate() {
                    self.on_tool_delta(asm, call, pos);
                }
            }
        }

        if let Some(reason) = choice
            .get("finish_reason")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            self.finish_reason = Some(reason.to_string());
        }
        Ok(())
    }

    fn on_tool_delta(&mut self, asm: &mut Assembler<'_>, call: &Value, pos: usize) {
        let key = call.get("index").and_then(Value::as_u64);
        let id = call
            .get("id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty());
        let func = call.get("function");
        let name = func
            .and_then(|f| f.get("name"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty());
        let args = func
            .and_then(|f| f.get("arguments"))
            .and_then(Value::as_str)
            .unwrap_or("");

        // Locate the slot: by index, then by id, then (no index) the latest slot.
        let found = match key {
            Some(k) => self.tools.iter().position(|s| s.key == Some(k)),
            None => None,
        }
        .or_else(|| id.and_then(|id| self.tools.iter().position(|s| s.id.as_deref() == Some(id))))
        .or_else(|| {
            if key.is_none() && id.is_none() && !self.tools.is_empty() {
                Some(self.tools.len() - 1)
            } else {
                None
            }
        });
        let slot_idx = match found {
            Some(i) => i,
            None => {
                self.tools.push(ToolSlot {
                    key: key.or(Some(1_000_000 + pos as u64)),
                    ..Default::default()
                });
                self.tools.len() - 1
            }
        };

        let slot = &mut self.tools[slot_idx];
        if slot.id.is_none() {
            slot.id = id.map(str::to_string);
        }
        if slot.name.is_none() {
            slot.name = name.map(str::to_string);
        }
        if slot.block.is_none() {
            slot.pending.push_str(args);
            if slot.name.is_some() {
                self.start_tool(asm, slot_idx);
            }
        } else if let Some(b) = slot.block {
            asm.delta(b, args);
        }
    }

    fn start_tool(&mut self, asm: &mut Assembler<'_>, slot_idx: usize) {
        self.close_text(asm);
        self.close_reasoning(asm);
        let slot = &mut self.tools[slot_idx];
        let id = slot
            .id
            .clone()
            .unwrap_or_else(|| format!("call_{slot_idx}"));
        slot.id = Some(id.clone());
        let name = slot.name.clone().unwrap_or_default();
        let b = asm.start(BlockKind::ToolCall { id, name });
        slot.block = Some(b);
        let pending = std::mem::take(&mut slot.pending);
        asm.delta(b, &pending);
    }

    fn close_text(&mut self, asm: &mut Assembler<'_>) {
        if let Some(i) = self.text.take() {
            asm.end(i);
        }
    }

    fn close_reasoning(&mut self, asm: &mut Assembler<'_>) {
        if let Some(i) = self.reasoning.take() {
            asm.end(i);
        }
    }
}

impl StreamState for ChatState {
    fn handle(&mut self, asm: &mut Assembler<'_>, ev: SseEvent) -> Result<bool, LlmError> {
        for chunk in data_values(&ev.data) {
            self.on_chunk(asm, &chunk)?;
        }
        if is_done(&ev.data) {
            self.saw_done = true;
            return Ok(true);
        }
        Ok(false)
    }

    fn finish(self, asm: Assembler<'_>, end: StreamEnd) -> Result<Response, LlmError> {
        let reason = match self.finish_reason.as_deref() {
            Some(r) => r.to_string(),
            // `[DONE]` without finish_reason: accept if something was produced.
            None if end == StreamEnd::Terminal && !asm.is_empty() => "stop".into(),
            None => {
                return Err(LlmError::transient(
                    "stream ended unexpectedly (no finish_reason)",
                ));
            }
        };
        let stop = match reason.as_str() {
            "stop" | "end" | "eos" | "end_turn" | "stop_sequence" => "stop".to_string(),
            "length" | "max_tokens" => "length".into(),
            "tool_calls" | "function_call" | "tool_use" => "tool_calls".into(),
            "content_filter" | "sensitive" => "content_filter".into(),
            "network_error" | "error" => {
                return Err(LlmError::transient(format!(
                    "provider finish_reason: {reason}"
                )));
            }
            "model_context_window_exceeded" => {
                return Err(LlmError {
                    context_overflow: true,
                    ..LlmError::fatal("model context window exceeded")
                });
            }
            other => other.to_string(),
        };
        Ok(asm.finish(&stop))
    }
}

/// Chat usage: `prompt_tokens` includes cached tokens.
fn parse_usage(u: &Value) -> Usage {
    let prompt = ptr_u64(u, "/prompt_tokens").unwrap_or(0);
    let output = ptr_u64(u, "/completion_tokens").unwrap_or(0);
    let cache_read = ptr_u64(u, "/prompt_tokens_details/cached_tokens")
        .or_else(|| ptr_u64(u, "/prompt_cache_hit_tokens"))
        .or_else(|| ptr_u64(u, "/cached_tokens"))
        .or_else(|| ptr_u64(u, "/cache_read_input_tokens"))
        .unwrap_or(0);
    let cache_write = ptr_u64(u, "/prompt_tokens_details/cache_write_tokens")
        .or_else(|| ptr_u64(u, "/cache_creation_input_tokens"))
        .unwrap_or(0);
    Usage {
        input: prompt
            .saturating_sub(cache_read)
            .saturating_sub(cache_write),
        output,
        cache_read,
        cache_write,
        reasoning: ptr_u64(u, "/completion_tokens_details/reasoning_tokens").unwrap_or(0),
    }
}

#[cfg(test)]
#[path = "chat_tests.rs"]
mod tests;
