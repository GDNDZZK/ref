//! OpenAI Responses API (`POST {base}/responses`), stateless mode
//! (`store: false`) with encrypted reasoning replay.

use crate::error::stream_error;
use crate::sse::{SseEvent, data_values};
use crate::util::{
    Assembler, CLIENT_OPTION_KEYS, DEFAULT_OPENAI_BASE, IMAGE_OMITTED, StreamEnd, StreamState,
    arguments_string, build_headers, camel_to_snake, data_url, deep_merge, drive, endpoint,
    error_fields, merge_key, normalize_call_id, normalize_history, opt_str, prompt_cache_key,
    ptr_u64, same_origin, session_headers, snake_keys,
};
use crate::{
    AssistantBlock, BlockKind, ChatMessage, LlmError, Model, Provider, Request, Response,
    StreamEvent, UserContent,
};
use ref_proto::Usage;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use tokio_util::sync::CancellationToken;

/// OpenAI limits `call_id` / item ids to 64 characters.
const MAX_ID: usize = 64;
const ENCRYPTED_REASONING: &str = "reasoning.encrypted_content";

pub(crate) async fn stream(
    provider: &Provider,
    model: &Model,
    req: &Request,
    cancel: &CancellationToken,
    on_event: &mut (dyn FnMut(StreamEvent) + Send),
) -> Result<Response, LlmError> {
    let body = build_body(provider, model, req);
    let url = endpoint(&provider.base_url, DEFAULT_OPENAI_BASE, "responses");
    let headers = request_headers(provider, req);
    drive(
        ResponsesState::default(),
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

pub(crate) fn build_body(provider: &Provider, model: &Model, req: &Request) -> Value {
    let opts = &model.options;
    let store = opts.get("store").and_then(Value::as_bool).unwrap_or(false);

    let mut body = Map::new();
    body.insert("model".into(), json!(model.id));
    let system: Vec<&str> = req
        .system
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if !system.is_empty() {
        body.insert("instructions".into(), json!(system.join("\n\n")));
    }
    body.insert(
        "input".into(),
        Value::Array(convert_input(provider, model, req)),
    );
    if !req.tools.is_empty() {
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function", "name": t.name, "description": t.description,
                    "parameters": t.parameters, "strict": false,
                })
            })
            .collect();
        body.insert("tools".into(), Value::Array(tools));
        body.insert("tool_choice".into(), json!("auto"));
    }
    body.insert("stream".into(), json!(true));
    body.insert("store".into(), json!(store));
    if model.output > 0 {
        body.insert("max_output_tokens".into(), json!(model.output.max(16)));
    }

    // Reasoning configuration.
    let effort = opt_str(opts, &["reasoningEffort", "reasoning_effort"]);
    let summary = opt_str(opts, &["reasoningSummary", "reasoning_summary"]);
    let mut reasoning = Map::new();
    if let Some(e) = effort {
        reasoning.insert("effort".into(), json!(e));
    }
    if let Some(s) = summary {
        reasoning.insert("summary".into(), json!(s));
    }
    let reasoning_on = (model.reasoning || !reasoning.is_empty()) && effort != Some("none");
    if !reasoning.is_empty() {
        body.insert("reasoning".into(), Value::Object(reasoning));
    }
    match opts.get("include") {
        Some(include) if !include.is_null() => {
            body.insert("include".into(), include.clone());
        }
        _ if reasoning_on && !store => {
            body.insert("include".into(), json!([ENCRYPTED_REASONING]));
        }
        _ => {}
    }

    if let Some(t) = model.temperature {
        body.insert("temperature".into(), json!(t));
    }
    if let Some(p) = model.top_p {
        body.insert("top_p".into(), json!(p));
    }
    let cache_key = opt_str(opts, &["promptCacheKey"])
        .map(str::to_string)
        .or_else(|| prompt_cache_key(&req.session_id));
    if let Some(key) = cache_key {
        body.insert("prompt_cache_key".into(), json!(key));
    }

    // Remaining options.
    let mut overlays = Vec::new();
    for (key, value) in opts {
        match key.as_str() {
            "store" | "include" | "reasoningEffort" | "reasoning_effort" | "reasoningSummary"
            | "reasoning_summary" | "promptCacheKey" => {}
            "textVerbosity" => {
                let mut text = Map::new();
                text.insert("verbosity".into(), value.clone());
                merge_key(&mut body, "text", &Value::Object(text));
            }
            "extraBody" | "body" => {
                if value.is_object() {
                    overlays.push(value.clone());
                }
            }
            // Anthropic / chat-only options that the Responses API rejects.
            "thinking" | "effort" | "strictJsonSchema" => {}
            k if CLIENT_OPTION_KEYS.contains(&k) => {}
            k => merge_key(&mut body, &camel_to_snake(k), &snake_keys(value)),
        }
    }
    let mut body = Value::Object(body);
    for overlay in &overlays {
        deep_merge(&mut body, overlay);
    }
    body
}

fn input_image(model: &Model, img: &ref_proto::ImageData) -> Value {
    if model.images {
        json!({"type": "input_image", "detail": "auto", "image_url": data_url(img)})
    } else {
        json!({"type": "input_text", "text": IMAGE_OMITTED})
    }
}

fn convert_input(provider: &Provider, model: &Model, req: &Request) -> Vec<Value> {
    let mut out = Vec::new();
    for msg in normalize_history(&req.messages) {
        match &msg {
            ChatMessage::User(parts) => {
                let content: Vec<Value> = parts
                    .iter()
                    .filter_map(|p| match p {
                        UserContent::Text(t) if t.trim().is_empty() => None,
                        UserContent::Text(t) => Some(json!({"type": "input_text", "text": t})),
                        UserContent::Image(img) => Some(input_image(model, img)),
                    })
                    .collect();
                if !content.is_empty() {
                    out.push(json!({"role": "user", "content": content}));
                }
            }
            ChatMessage::Assistant { blocks, origin } => {
                let replay = same_origin(origin, provider, model);
                for block in blocks {
                    match block {
                        AssistantBlock::Reasoning { provider_data, .. } => {
                            // Encrypted reasoning only round-trips to the model that produced it.
                            let item = provider_data.as_ref().and_then(|d| d.get("item"));
                            if replay
                                && let Some(item) = item.filter(|i| {
                                    i.get("type").and_then(Value::as_str) == Some("reasoning")
                                })
                            {
                                out.push(item.clone());
                            }
                        }
                        AssistantBlock::Text(t) => {
                            if !t.trim().is_empty() {
                                out.push(json!({
                                    "type": "message", "role": "assistant", "status": "completed",
                                    "content": [{"type": "output_text", "text": t, "annotations": []}],
                                }));
                            }
                        }
                        AssistantBlock::ToolCall {
                            id,
                            name,
                            arguments,
                            provider_data,
                        } => {
                            let mut item = Map::new();
                            item.insert("type".into(), json!("function_call"));
                            let item_id = provider_data
                                .as_ref()
                                .and_then(|d| d.get("id"))
                                .and_then(Value::as_str)
                                .filter(|id| id.starts_with("fc_") && id.len() <= MAX_ID);
                            // The fc_ id pairs with reasoning items; only valid for the same model.
                            if replay && let Some(item_id) = item_id {
                                item.insert("id".into(), json!(item_id));
                            }
                            item.insert("call_id".into(), json!(normalize_call_id(id, MAX_ID)));
                            item.insert("name".into(), json!(name));
                            item.insert("arguments".into(), json!(arguments_string(arguments)));
                            out.push(Value::Object(item));
                        }
                    }
                }
            }
            ChatMessage::ToolResult {
                call_id,
                content,
                images,
                ..
            } => {
                let output = if images.is_empty() {
                    let text = if content.trim().is_empty() {
                        "(no tool output)"
                    } else {
                        content.as_str()
                    };
                    json!(text)
                } else {
                    let mut parts = Vec::new();
                    if !content.trim().is_empty() {
                        parts.push(json!({"type": "input_text", "text": content}));
                    }
                    parts.extend(images.iter().map(|img| input_image(model, img)));
                    Value::Array(parts)
                };
                out.push(json!({
                    "type": "function_call_output",
                    "call_id": normalize_call_id(call_id, MAX_ID),
                    "output": output,
                }));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Streaming
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotKind {
    Reasoning,
    Text,
    Tool,
}

#[derive(Debug)]
struct Slot {
    kind: SlotKind,
    block: usize,
    /// Last reasoning summary index seen (separates summary parts).
    summary_index: Option<u64>,
    done: bool,
}

#[derive(Debug, Default)]
pub(crate) struct ResponsesState {
    /// Keyed by `output_index` (or item id for servers that omit it).
    slots: HashMap<String, Slot>,
    /// Reasoning block index by item id (for backfilling encrypted content).
    reasoning_by_id: HashMap<String, usize>,
    status: Option<String>,
    incomplete_reason: Option<String>,
}

fn slot_key(ev: &Value) -> String {
    match ev.get("output_index").and_then(Value::as_u64) {
        Some(i) => format!("#{i}"),
        None => ev
            .get("item_id")
            .or_else(|| ev.pointer("/item/id"))
            .and_then(Value::as_str)
            .map(|s| format!("id:{s}"))
            .unwrap_or_else(|| "#?".into()),
    }
}

impl ResponsesState {
    fn open_slot(&mut self, asm: &mut Assembler<'_>, key: String, item: &Value) -> Option<usize> {
        if let Some(slot) = self.slots.get(&key) {
            return Some(slot.block);
        }
        let (kind, block_kind) = match item.get("type").and_then(Value::as_str)? {
            "reasoning" => (SlotKind::Reasoning, BlockKind::Reasoning),
            "message" => (SlotKind::Text, BlockKind::Text),
            "function_call" => {
                let id = item
                    .get("call_id")
                    .or_else(|| item.get("id"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let name = item
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                (SlotKind::Tool, BlockKind::ToolCall { id, name })
            }
            _ => return None,
        };
        let block = asm.start(block_kind);
        if kind == SlotKind::Tool
            && let Some(item_id) = item.get("id").and_then(Value::as_str)
        {
            asm.set_provider_data(block, json!({"id": item_id}));
        }
        self.slots.insert(
            key,
            Slot {
                kind,
                block,
                summary_index: None,
                done: false,
            },
        );
        Some(block)
    }

    fn slot_for(&mut self, ev: &Value, kind: SlotKind) -> Option<&mut Slot> {
        let key = slot_key(ev);
        self.slots
            .get_mut(&key)
            .filter(|s| s.kind == kind && !s.done)
    }

    fn on_event(&mut self, asm: &mut Assembler<'_>, ev: &Value) -> Result<bool, LlmError> {
        let ty = ev.get("type").and_then(Value::as_str).unwrap_or("");
        let delta = ev.get("delta").and_then(Value::as_str).unwrap_or("");
        match ty {
            "response.output_item.added" => {
                if let Some(item) = ev.get("item") {
                    self.open_slot(asm, slot_key(ev), item);
                }
            }
            "response.reasoning_summary_text.delta" => {
                let idx = ev.get("summary_index").and_then(Value::as_u64);
                if let Some(slot) = self.slot_for(ev, SlotKind::Reasoning) {
                    let block = slot.block;
                    let new_part =
                        idx.is_some() && slot.summary_index.is_some() && slot.summary_index != idx;
                    if idx.is_some() {
                        slot.summary_index = idx;
                    }
                    if new_part && !asm.text_of(block).is_empty() {
                        asm.delta(block, "\n\n");
                    }
                    asm.delta(block, delta);
                }
            }
            "response.reasoning_text.delta" => {
                if let Some(slot) = self.slot_for(ev, SlotKind::Reasoning) {
                    let block = slot.block;
                    asm.delta(block, delta);
                }
            }
            "response.output_text.delta" | "response.refusal.delta" => {
                if let Some(slot) = self.slot_for(ev, SlotKind::Text) {
                    let block = slot.block;
                    asm.delta(block, delta);
                }
            }
            "response.function_call_arguments.delta" => {
                if let Some(slot) = self.slot_for(ev, SlotKind::Tool) {
                    let block = slot.block;
                    asm.delta(block, delta);
                }
            }
            "response.function_call_arguments.done" => {
                if let (Some(slot), Some(args)) = (
                    self.slot_for(ev, SlotKind::Tool),
                    ev.get("arguments").and_then(Value::as_str),
                ) {
                    let block = slot.block;
                    sync_text(asm, block, args);
                }
            }
            "response.output_item.done" => {
                if let Some(item) = ev.get("item") {
                    self.on_item_done(asm, slot_key(ev), item);
                }
            }
            "response.completed" | "response.incomplete" => {
                if let Some(resp) = ev.get("response") {
                    self.on_terminal(asm, resp);
                }
                if self.status.is_none() {
                    self.status = Some(
                        if ty == "response.completed" {
                            "completed"
                        } else {
                            "incomplete"
                        }
                        .into(),
                    );
                }
                return Ok(true);
            }
            "response.failed" => {
                let resp = ev.get("response").cloned().unwrap_or(Value::Null);
                if let Some(err) = resp.get("error").filter(|e| !e.is_null()) {
                    let (code, message) = error_fields(err);
                    return Err(stream_error(code.as_deref(), &message));
                }
                let reason = resp
                    .pointer("/incomplete_details/reason")
                    .and_then(Value::as_str);
                return Err(stream_error(
                    None,
                    &format!(
                        "response failed{}",
                        reason.map(|r| format!(": {r}")).unwrap_or_default()
                    ),
                ));
            }
            "error" => {
                let err = ev.get("error").filter(|e| e.is_object()).unwrap_or(ev);
                let (code, message) = error_fields(err);
                let code = code.filter(|c| c != "error");
                return Err(stream_error(code.as_deref(), &message));
            }
            _ => {}
        }
        Ok(false)
    }

    fn on_item_done(&mut self, asm: &mut Assembler<'_>, key: String, item: &Value) {
        let Some(block) = self.open_slot(asm, key.clone(), item) else {
            return;
        };
        let kind = self.slots.get(&key).map(|s| s.kind);
        match kind {
            Some(SlotKind::Reasoning) => {
                if asm.text_of(block).is_empty() {
                    // No deltas streamed: use the final summary / content text.
                    let text = joined_texts(item.get("summary"))
                        .or_else(|| joined_texts(item.get("content")));
                    if let Some(text) = text {
                        asm.delta(block, &text);
                    }
                }
                asm.set_provider_data(block, json!({"item": item}));
                if let Some(id) = item.get("id").and_then(Value::as_str) {
                    self.reasoning_by_id.insert(id.to_string(), block);
                }
            }
            Some(SlotKind::Text) => {
                if let Some(content) = item.get("content").and_then(Value::as_array) {
                    let text: String = content
                        .iter()
                        .filter_map(|c| {
                            c.get("text")
                                .or_else(|| c.get("refusal"))
                                .and_then(Value::as_str)
                        })
                        .collect();
                    sync_text(asm, block, &text);
                }
            }
            Some(SlotKind::Tool) => {
                if let Some(args) = item.get("arguments").and_then(Value::as_str) {
                    sync_text(asm, block, args);
                }
                if let Some(AssistantBlock::ToolCall { id, name, .. }) = asm.block_mut(block) {
                    if id.is_empty()
                        && let Some(call_id) = item.get("call_id").and_then(Value::as_str)
                    {
                        *id = call_id.to_string();
                    }
                    if name.is_empty()
                        && let Some(n) = item.get("name").and_then(Value::as_str)
                    {
                        *name = n.to_string();
                    }
                }
                if let Some(item_id) = item.get("id").and_then(Value::as_str) {
                    asm.set_provider_data(block, json!({"id": item_id}));
                }
            }
            None => {}
        }
        if let Some(slot) = self.slots.get_mut(&key) {
            slot.done = true;
        }
        asm.end(block);
    }

    fn on_terminal(&mut self, asm: &mut Assembler<'_>, resp: &Value) {
        self.status = resp
            .get("status")
            .and_then(Value::as_str)
            .map(str::to_string);
        self.incomplete_reason = resp
            .pointer("/incomplete_details/reason")
            .and_then(Value::as_str)
            .map(str::to_string);
        // Some deployments only include encrypted_content in the final output.
        if let Some(output) = resp.get("output").and_then(Value::as_array) {
            for item in output {
                let (Some("reasoning"), Some(id)) = (
                    item.get("type").and_then(Value::as_str),
                    item.get("id").and_then(Value::as_str),
                ) else {
                    continue;
                };
                let Some(&block) = self.reasoning_by_id.get(id) else {
                    continue;
                };
                let missing = match asm.block(block) {
                    Some(AssistantBlock::Reasoning { provider_data, .. }) => provider_data
                        .as_ref()
                        .and_then(|d| d.pointer("/item/encrypted_content"))
                        .is_none_or(|v| v.is_null()),
                    _ => false,
                };
                if missing && item.get("encrypted_content").is_some_and(|v| !v.is_null()) {
                    asm.set_provider_data(block, json!({"item": item}));
                }
            }
        }
        if let Some(u) = resp.get("usage").filter(|u| u.is_object()) {
            asm.usage(parse_usage(u));
        }
    }
}

/// Bring a block's text up to `full` when the final item carries text that was
/// not (fully) streamed. Emits only the missing suffix.
fn sync_text(asm: &mut Assembler<'_>, block: usize, full: &str) {
    let current = asm.text_of(block);
    if current == full || full.is_empty() {
        return;
    }
    if let Some(rest) = full.strip_prefix(current) {
        let rest = rest.to_string();
        asm.delta(block, &rest);
    } else if current.is_empty() {
        asm.delta(block, full);
    } else if let Some(AssistantBlock::ToolCall { arguments, .. }) = asm.block_mut(block) {
        // Diverging argument stream: the final value is authoritative.
        *arguments = full.to_string();
    }
}

fn joined_texts(parts: Option<&Value>) -> Option<String> {
    let texts: Vec<&str> = parts?
        .as_array()?
        .iter()
        .filter_map(|p| p.get("text").and_then(Value::as_str))
        .filter(|t| !t.is_empty())
        .collect();
    (!texts.is_empty()).then(|| texts.join("\n\n"))
}

impl StreamState for ResponsesState {
    fn handle(&mut self, asm: &mut Assembler<'_>, ev: SseEvent) -> Result<bool, LlmError> {
        for mut value in data_values(&ev.data) {
            // The type normally lives in the JSON; fall back to the SSE event name.
            if value.get("type").is_none()
                && !ev.event.is_empty()
                && let Some(obj) = value.as_object_mut()
            {
                obj.insert("type".into(), json!(ev.event));
            }
            if self.on_event(asm, &value)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn finish(self, asm: Assembler<'_>, end: StreamEnd) -> Result<Response, LlmError> {
        if end != StreamEnd::Terminal {
            return Err(LlmError::transient(
                "stream ended unexpectedly (no response.completed)",
            ));
        }
        let unfinished_tool = self
            .slots
            .values()
            .any(|s| s.kind == SlotKind::Tool && !s.done);
        let stop = match (self.status.as_deref(), self.incomplete_reason.as_deref()) {
            (Some("incomplete"), Some("max_output_tokens")) => "length".to_string(),
            (Some("incomplete"), Some("content_filter")) => "content_filter".into(),
            (Some("incomplete"), Some(other)) => other.to_string(),
            (Some("completed") | None, _) => "stop".into(),
            (Some(other), _) => other.to_string(),
        };
        if stop == "stop" && unfinished_tool {
            return Err(LlmError::transient(
                "stream completed with an unfinished tool call",
            ));
        }
        Ok(asm.finish(&stop))
    }
}

/// Responses usage: `input_tokens` includes cached tokens.
fn parse_usage(u: &Value) -> Usage {
    let input = ptr_u64(u, "/input_tokens").unwrap_or(0);
    let cache_read = ptr_u64(u, "/input_tokens_details/cached_tokens").unwrap_or(0);
    let cache_write = ptr_u64(u, "/input_tokens_details/cache_write_tokens").unwrap_or(0);
    Usage {
        input: input.saturating_sub(cache_read).saturating_sub(cache_write),
        output: ptr_u64(u, "/output_tokens").unwrap_or(0),
        cache_read,
        cache_write,
        reasoning: ptr_u64(u, "/output_tokens_details/reasoning_tokens").unwrap_or(0),
    }
}

#[cfg(test)]
#[path = "responses_tests.rs"]
mod tests;
