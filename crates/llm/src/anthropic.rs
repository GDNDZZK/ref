//! Anthropic Messages API (`POST {base}/messages`).

use crate::error::stream_error;
use crate::sse::{SseEvent, data_values};
use crate::util::{
    Assembler, DEFAULT_ANTHROPIC_BASE, IMAGE_OMITTED, StreamEnd, StreamState, build_headers,
    deep_merge, drive, endpoint, error_fields, normalize_call_id, normalize_history, opt_str,
    opt_u64, parse_arguments, provider_header, ptr_u64, same_origin, session_headers, snake_keys,
};
use crate::{
    AssistantBlock, BlockKind, ChatMessage, LlmError, Model, Provider, Request, Response,
    StreamEvent, UserContent,
};
use ref_proto::{ImageData, Usage};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use tokio_util::sync::CancellationToken;

const API_VERSION: &str = "2023-06-01";
const DEFAULT_BETAS: &[&str] = &[
    "interleaved-thinking-2025-05-14",
    "fine-grained-tool-streaming-2025-05-14",
];
const OAUTH_BETA: &str = "oauth-2025-04-20";
/// Used when the model's output limit is unknown (the field is mandatory).
const DEFAULT_MAX_TOKENS: u64 = 32_000;
const MIN_THINKING_BUDGET: u64 = 1024;
/// Head-room for the visible answer when the budget is close to `max_tokens`.
const ANSWER_HEADROOM: u64 = 4096;
/// Anthropic tool ids: `^[a-zA-Z0-9_-]+$`, at most 64 characters.
const MAX_TOOL_ID: usize = 64;
/// Message-level cache breakpoints (plus one each for tools and system).
const MESSAGE_BREAKPOINTS: usize = 2;

pub(crate) async fn stream(
    provider: &Provider,
    model: &Model,
    req: &Request,
    cancel: &CancellationToken,
    on_event: &mut (dyn FnMut(StreamEvent) + Send),
) -> Result<Response, LlmError> {
    let body = build_body(provider, model, req);
    let url = endpoint(&provider.base_url, DEFAULT_ANTHROPIC_BASE, "messages");
    let headers = request_headers(provider, req);
    drive(
        AnthropicState::default(),
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
        ("anthropic-version".to_string(), API_VERSION.to_string()),
    ];
    let key = provider
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty());
    let oauth = key.is_some_and(|k| k.starts_with("sk-ant-oat"));
    if let Some(key) = key {
        if oauth {
            defaults.push(("authorization".into(), format!("Bearer {key}")));
        } else {
            defaults.push(("x-api-key".into(), key.to_string()));
        }
    }

    // anthropic-beta: defaults merged with user-provided values, deduplicated.
    let mut betas: Vec<String> = DEFAULT_BETAS.iter().map(|s| s.to_string()).collect();
    if oauth {
        betas.push(OAUTH_BETA.into());
    }
    if let Some(extra) = provider_header(provider, "anthropic-beta") {
        betas.extend(
            extra
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
        );
    }
    let mut seen = std::collections::HashSet::new();
    betas.retain(|b| seen.insert(b.clone()));
    defaults.push(("anthropic-beta".into(), betas.join(",")));
    defaults.extend(session_headers(&req.session_id, &["x-session-affinity"]));
    build_headers(provider, defaults, &["anthropic-beta"])
}

/// `(major, minor)` Claude generation parsed from a model id such as
/// `claude-opus-4-6`, `claude-sonnet-4.6`, `claude-3-7-sonnet-20250219`,
/// `anthropic/claude-opus-5.5` or `us.anthropic.claude-sonnet-4-5-v1:0`.
/// Date suffixes are not mistaken for minor versions.
pub(crate) fn claude_version(model_id: &str) -> Option<(u32, u32)> {
    let id = model_id.to_ascii_lowercase();
    let rest = &id[id.find("claude")? + "claude".len()..];
    let mut tokens = rest
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty());
    let is_small_number = |t: &str| t.len() <= 2 && t.chars().all(|c| c.is_ascii_digit());
    let major: u32 = tokens.find(|t| is_small_number(t))?.parse().ok()?;
    let minor = tokens
        .next()
        .filter(|t| is_small_number(t))
        .and_then(|t| t.parse().ok())
        .unwrap_or(0);
    Some((major, minor))
}

/// Adaptive thinking (+ `output_config.effort`) is available from Claude 4.6;
/// unknown model ids are assumed to be recent.
fn supports_adaptive(version: Option<(u32, u32)>) -> bool {
    version.is_none_or(|v| v >= (4, 6))
}

/// `xhigh` effort and the `display` thinking option exist from Claude 4.7.
fn is_4_7_plus(version: Option<(u32, u32)>) -> bool {
    version.is_some_and(|v| v >= (4, 7))
}

/// Map an effort level for adaptive thinking (`None` = thinking off).
fn adaptive_effort(effort: &str, version: Option<(u32, u32)>) -> Option<&'static str> {
    Some(match effort.to_ascii_lowercase().as_str() {
        "none" | "off" | "disabled" => return None,
        "minimal" | "low" => "low",
        "medium" => "medium",
        "xhigh" if is_4_7_plus(version) => "xhigh",
        "max" => "max",
        _ => "high",
    })
}

/// Thinking budget for pre-4.6 models (`None` = thinking off).
fn effort_budget(effort: &str) -> Option<u64> {
    Some(match effort.to_ascii_lowercase().as_str() {
        "none" | "off" | "disabled" => return None,
        "minimal" => 1024,
        "low" => 2048,
        "medium" => 8192,
        "xhigh" | "max" => 32_000,
        _ => 16_384,
    })
}

/// Resolved thinking settings.
#[derive(Debug, Default)]
struct ThinkingPlan {
    thinking: Option<Value>,
    effort: Option<String>,
    /// Thinking is active (temperature / top_p must not be sent).
    active: bool,
}

fn plan_thinking(model: &Model, max_tokens: &mut u64) -> ThinkingPlan {
    let opts = &model.options;
    let version = claude_version(&model.id);
    let effort = opt_str(opts, &["effort", "reasoningEffort", "reasoning_effort"]);
    let thinking = opts.get("thinking").filter(|t| t.is_object());

    match thinking.and_then(|t| t.get("type")).and_then(Value::as_str) {
        Some("enabled") => {
            let t = thinking.unwrap_or(&Value::Null);
            let budget = opt_u64(t.get("budgetTokens").or_else(|| t.get("budget_tokens")))
                .or_else(|| effort.and_then(effort_budget))
                .unwrap_or(16_384);
            let mut out = snake_keys(t);
            out["budget_tokens"] = json!(fit_budget(model, budget, max_tokens));
            ThinkingPlan {
                thinking: Some(out),
                effort: None,
                active: true,
            }
        }
        Some("adaptive") => {
            let mut out = snake_keys(thinking.unwrap_or(&Value::Null));
            if is_4_7_plus(version) && out.get("display").is_none() {
                out["display"] = json!("summarized");
            }
            let effort = effort
                .and_then(|e| adaptive_effort(e, version))
                .map(str::to_string);
            ThinkingPlan {
                thinking: Some(out),
                effort,
                active: true,
            }
        }
        Some("disabled") => ThinkingPlan {
            thinking: Some(json!({"type": "disabled"})),
            effort: None,
            active: false,
        },
        _ => {
            let Some(effort) = effort else {
                return ThinkingPlan::default();
            };
            if supports_adaptive(version) {
                let Some(level) = adaptive_effort(effort, version) else {
                    return ThinkingPlan::default();
                };
                let mut t = json!({"type": "adaptive"});
                // Opus 4.7+ omits thinking text unless asked for summaries.
                if version.is_none() || is_4_7_plus(version) {
                    t["display"] = json!("summarized");
                }
                ThinkingPlan {
                    thinking: Some(t),
                    effort: Some(level.into()),
                    active: true,
                }
            } else {
                let Some(budget) = effort_budget(effort) else {
                    return ThinkingPlan::default();
                };
                let budget = fit_budget(model, budget, max_tokens);
                ThinkingPlan {
                    thinking: Some(json!({"type": "enabled", "budget_tokens": budget})),
                    effort: None,
                    active: true,
                }
            }
        }
    }
}

/// Keep `1024 <= budget < max_tokens`, raising `max_tokens` when needed but
/// never above the model's known output limit.
fn fit_budget(model: &Model, budget: u64, max_tokens: &mut u64) -> u64 {
    let mut budget = budget.max(MIN_THINKING_BUDGET);
    if *max_tokens <= budget {
        *max_tokens = budget + ANSWER_HEADROOM;
    }
    if model.output > 0 && *max_tokens > model.output {
        // Respect the model limit; split it between thinking and the answer.
        *max_tokens = model.output;
        if budget >= *max_tokens {
            budget = (*max_tokens / 2).max(MIN_THINKING_BUDGET);
        }
    }
    budget
}

pub(crate) fn build_body(provider: &Provider, model: &Model, req: &Request) -> Value {
    let opts = &model.options;
    let mut max_tokens = if model.output > 0 {
        model.output
    } else {
        DEFAULT_MAX_TOKENS
    };
    let plan = plan_thinking(model, &mut max_tokens);

    let mut body = Map::new();
    body.insert("model".into(), json!(model.id));
    body.insert("max_tokens".into(), json!(max_tokens));
    body.insert("stream".into(), json!(true));

    let mut system: Vec<Value> = req
        .system
        .iter()
        .filter(|s| !s.trim().is_empty())
        .map(|s| json!({"type": "text", "text": s}))
        .collect();
    if let Some(last) = system.last_mut() {
        last["cache_control"] = json!({"type": "ephemeral"});
    }
    if !system.is_empty() {
        body.insert("system".into(), Value::Array(system));
    }

    body.insert(
        "messages".into(),
        Value::Array(convert_messages(provider, model, req)),
    );

    if !req.tools.is_empty() {
        let mut tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                let schema = if t.parameters.is_object() {
                    t.parameters.clone()
                } else {
                    json!({"type": "object", "properties": {}})
                };
                json!({"name": t.name, "description": t.description, "input_schema": schema})
            })
            .collect();
        if let Some(last) = tools.last_mut() {
            last["cache_control"] = json!({"type": "ephemeral"});
        }
        body.insert("tools".into(), Value::Array(tools));
        let mut choice = json!({"type": "auto"});
        if opts.get("disableParallelToolUse").and_then(Value::as_bool) == Some(true) {
            choice["disable_parallel_tool_use"] = json!(true);
        }
        body.insert("tool_choice".into(), choice);
    }

    if let Some(t) = plan.thinking {
        body.insert("thinking".into(), t);
    }
    let mut output_config = Map::new();
    if let Some(e) = plan.effort {
        output_config.insert("effort".into(), json!(e));
    }
    if let Some(Value::Object(extra)) = opts
        .get("outputConfig")
        .or_else(|| opts.get("output_config"))
    {
        for (k, v) in extra {
            output_config.insert(k.clone(), v.clone());
        }
    }
    if !output_config.is_empty() {
        body.insert("output_config".into(), Value::Object(output_config));
    }

    // Sampling parameters are rejected with thinking and on Claude 4.7+.
    if !plan.active && !is_4_7_plus(claude_version(&model.id)) {
        if let Some(t) = model.temperature {
            body.insert("temperature".into(), json!(t));
        }
        if let Some(p) = model.top_p {
            body.insert("top_p".into(), json!(p));
        }
    }

    for (key, value) in opts {
        match key.as_str() {
            "serviceTier" | "service_tier" => {
                body.insert("service_tier".into(), value.clone());
            }
            "metadata" | "contextManagement" | "context_management" | "container" | "topK"
            | "top_k" => {
                body.insert(crate::util::camel_to_snake(key), snake_keys(value));
            }
            _ => {}
        }
    }
    let mut body = Value::Object(body);
    for key in ["extraBody", "body"] {
        if let Some(overlay) = opts.get(key).filter(|v| v.is_object()) {
            deep_merge(&mut body, overlay);
        }
    }
    body
}

fn image_block(img: &ImageData) -> Value {
    json!({"type": "image", "source": {"type": "base64", "media_type": img.mime, "data": img.data}})
}

fn convert_messages(provider: &Provider, model: &Model, req: &Request) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for msg in normalize_history(&req.messages) {
        match &msg {
            ChatMessage::User(parts) => {
                let content: Vec<Value> = parts
                    .iter()
                    .filter_map(|p| match p {
                        UserContent::Text(t) if t.trim().is_empty() => None,
                        UserContent::Text(t) => Some(json!({"type": "text", "text": t})),
                        UserContent::Image(img) if model.images => Some(image_block(img)),
                        UserContent::Image(_) => {
                            Some(json!({"type": "text", "text": IMAGE_OMITTED}))
                        }
                    })
                    .collect();
                push_merged(&mut out, "user", content);
            }
            ChatMessage::Assistant { blocks, origin } => {
                let replay = same_origin(origin, provider, model);
                let mut content = Vec::new();
                for block in blocks {
                    match block {
                        AssistantBlock::Reasoning {
                            text,
                            provider_data,
                        } => {
                            if !replay {
                                continue;
                            }
                            let data = provider_data.as_ref();
                            let signature = data
                                .and_then(|d| d.get("signature"))
                                .and_then(Value::as_str);
                            let redacted =
                                data.and_then(|d| d.get("redacted")).and_then(Value::as_str);
                            if let Some(redacted) = redacted.filter(|r| !r.is_empty()) {
                                content
                                    .push(json!({"type": "redacted_thinking", "data": redacted}));
                            } else if let Some(sig) = signature.filter(|s| !s.is_empty()) {
                                content.push(
                                    json!({"type": "thinking", "thinking": text, "signature": sig}),
                                );
                            }
                        }
                        AssistantBlock::Text(t) => {
                            if !t.trim().is_empty() {
                                content.push(json!({"type": "text", "text": t}));
                            }
                        }
                        AssistantBlock::ToolCall {
                            id,
                            name,
                            arguments,
                            ..
                        } => content.push(json!({
                            "type": "tool_use",
                            "id": normalize_call_id(id, MAX_TOOL_ID),
                            "name": name,
                            "input": parse_arguments(arguments),
                        })),
                    }
                }
                push_merged(&mut out, "assistant", content);
            }
            ChatMessage::ToolResult {
                call_id,
                content,
                images,
                is_error,
                ..
            } => {
                let images: &[ImageData] = if *is_error || !model.images {
                    &[]
                } else {
                    images
                };
                let mut parts = Vec::new();
                if !content.trim().is_empty() {
                    parts.push(json!({"type": "text", "text": content}));
                } else if images.is_empty() {
                    parts.push(json!({"type": "text", "text": "(no tool output)"}));
                } else {
                    parts.push(json!({"type": "text", "text": "(see attached image)"}));
                }
                parts.extend(images.iter().map(image_block));
                let mut block = json!({
                    "type": "tool_result",
                    "tool_use_id": normalize_call_id(call_id, MAX_TOOL_ID),
                    "content": parts,
                });
                if *is_error {
                    block["is_error"] = json!(true);
                }
                push_merged(&mut out, "user", vec![block]);
            }
        }
    }

    // A trailing assistant message would be treated as a prefill, which is
    // rejected with thinking enabled; ask the model to continue instead.
    if out.last().is_some_and(|m| m["role"] == "assistant") {
        out.push(json!({"role": "user", "content": [{"type": "text", "text": "Continue."}]}));
    }

    // Cache breakpoints on the last block of the last user-role messages.
    let mut placed = 0;
    for msg in out.iter_mut().rev() {
        if placed == MESSAGE_BREAKPOINTS {
            break;
        }
        if msg["role"] != "user" {
            continue;
        }
        if let Some(last) = msg["content"].as_array_mut().and_then(|c| c.last_mut()) {
            last["cache_control"] = json!({"type": "ephemeral"});
            placed += 1;
        }
    }
    out
}

/// Append `content` as a `role` message, merging with the previous message
/// when it has the same role (the API requires alternating roles).
fn push_merged(out: &mut Vec<Value>, role: &str, content: Vec<Value>) {
    if content.is_empty() {
        return;
    }
    if let Some(prev) = out.last_mut()
        && prev["role"] == role
        && let Some(existing) = prev["content"].as_array_mut()
    {
        existing.extend(content);
        return;
    }
    out.push(json!({"role": role, "content": content}));
}

// ---------------------------------------------------------------------------
// Streaming
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub(crate) struct AnthropicState {
    /// Anthropic content index -> assembler block index.
    blocks: HashMap<u64, usize>,
    /// Accumulated thinking signatures by assembler block.
    signatures: HashMap<usize, String>,
    usage: Usage,
    stop_reason: Option<String>,
    started: bool,
}

impl AnthropicState {
    fn apply_usage(&mut self, u: &Value) {
        if let Some(v) = ptr_u64(u, "/input_tokens") {
            self.usage.input = v;
        }
        if let Some(v) = ptr_u64(u, "/output_tokens") {
            self.usage.output = v;
        }
        if let Some(v) = ptr_u64(u, "/cache_read_input_tokens") {
            self.usage.cache_read = v;
        }
        if let Some(v) = ptr_u64(u, "/cache_creation_input_tokens") {
            self.usage.cache_write = v;
        }
        if let Some(v) = ptr_u64(u, "/output_tokens_details/thinking_tokens") {
            self.usage.reasoning = v;
        }
    }

    fn on_event(&mut self, asm: &mut Assembler<'_>, ev: &Value) -> Result<bool, LlmError> {
        let ty = ev.get("type").and_then(Value::as_str).unwrap_or("");
        match ty {
            "message_start" => {
                self.started = true;
                if let Some(u) = ev.pointer("/message/usage").filter(|u| u.is_object()) {
                    self.apply_usage(u);
                    asm.usage(self.usage);
                }
            }
            "content_block_start" => {
                let index = ev.get("index").and_then(Value::as_u64).unwrap_or(0);
                let Some(cb) = ev.get("content_block") else {
                    return Ok(false);
                };
                let text_field =
                    |k: &str| cb.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                let block = match cb.get("type").and_then(Value::as_str).unwrap_or("") {
                    "text" => {
                        let b = asm.start(BlockKind::Text);
                        asm.delta(b, &text_field("text"));
                        b
                    }
                    "thinking" => {
                        let b = asm.start(BlockKind::Reasoning);
                        asm.delta(b, &text_field("thinking"));
                        let sig = text_field("signature");
                        if !sig.is_empty() {
                            asm.set_provider_data(b, json!({"signature": sig}));
                            self.signatures.insert(b, sig);
                        }
                        b
                    }
                    "redacted_thinking" => {
                        let b = asm.start(BlockKind::Reasoning);
                        asm.set_provider_data(b, json!({"redacted": text_field("data")}));
                        b
                    }
                    "tool_use" => {
                        let b = asm.start(BlockKind::ToolCall {
                            id: text_field("id"),
                            name: text_field("name"),
                        });
                        if let Some(input) = cb
                            .get("input")
                            .and_then(Value::as_object)
                            .filter(|m| !m.is_empty())
                        {
                            asm.delta(b, &Value::Object(input.clone()).to_string());
                        }
                        b
                    }
                    _ => return Ok(false), // server tools etc. are not surfaced
                };
                self.blocks.insert(index, block);
            }
            "content_block_delta" => {
                let index = ev.get("index").and_then(Value::as_u64).unwrap_or(0);
                let (Some(&block), Some(delta)) = (self.blocks.get(&index), ev.get("delta")) else {
                    return Ok(false);
                };
                let s = |k: &str| delta.get(k).and_then(Value::as_str).unwrap_or("");
                match delta.get("type").and_then(Value::as_str).unwrap_or("") {
                    "text_delta" => asm.delta(block, s("text")),
                    "thinking_delta" => asm.delta(block, s("thinking")),
                    "input_json_delta" => asm.delta(block, s("partial_json")),
                    "signature_delta" => {
                        let sig = self.signatures.entry(block).or_default();
                        sig.push_str(s("signature"));
                        let sig = sig.clone();
                        asm.set_provider_data(block, json!({"signature": sig}));
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                let index = ev.get("index").and_then(Value::as_u64).unwrap_or(0);
                if let Some(&block) = self.blocks.get(&index) {
                    asm.end(block);
                }
            }
            "message_delta" => {
                if let Some(r) = ev.pointer("/delta/stop_reason").and_then(Value::as_str) {
                    self.stop_reason = Some(r.to_string());
                }
                if let Some(u) = ev.get("usage").filter(|u| u.is_object()) {
                    self.apply_usage(u);
                    asm.usage(self.usage);
                }
            }
            "message_stop" => return Ok(true),
            "error" => {
                let err = ev.get("error").unwrap_or(ev);
                let (code, message) = error_fields(err);
                return Err(stream_error(code.as_deref(), &message));
            }
            _ => {} // ping and future events
        }
        Ok(false)
    }
}

impl StreamState for AnthropicState {
    fn handle(&mut self, asm: &mut Assembler<'_>, ev: SseEvent) -> Result<bool, LlmError> {
        for mut value in data_values(&ev.data) {
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
                "stream ended unexpectedly (no message_stop)",
            ));
        }
        let stop = match self.stop_reason.as_deref() {
            None | Some("end_turn" | "stop_sequence" | "pause_turn") => "stop".to_string(),
            Some("max_tokens" | "model_context_window_exceeded") => "length".into(),
            Some("tool_use") => "tool_calls".into(),
            Some("refusal") => "content_filter".into(),
            Some(other) => other.to_string(),
        };
        Ok(asm.finish(&stop))
    }
}

#[cfg(test)]
#[path = "anthropic_tests.rs"]
mod tests;
