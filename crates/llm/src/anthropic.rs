//! Anthropic Messages API (`POST {base_url}/messages`; opencode base URLs end in `/v1`).
//!
//! Design notes:
//! - Tool call ids pass through unchanged (`toolu_...`); on the wire they are sanitized to
//!   `^[A-Za-z0-9_-]{1,64}$` deterministically, so results always match their calls.
//! - Replay data ([`StreamEvent::ReasoningEnd`]): `{"signature": "..."}` for thinking blocks,
//!   `{"redacted": "<data>"}` for redacted thinking. Thinking is replayed only when the
//!   assistant message has `replay == true` and the signature is non-empty; otherwise it is
//!   dropped (never converted to text).
//! - Prompt caching: `cache_control: ephemeral` on the last tool, the last system block and
//!   the last content block of the last two user messages (≤ 4 breakpoints).
//! - Thinking options: `thinking` ({type: enabled, budgetTokens|budget_tokens} or adaptive),
//!   `effort` / `outputConfig.effort`, and opencode-style `reasoningEffort` variants (adaptive +
//!   `output_config.effort` on Claude ≥ 4.6, a token budget on older models).

use crate::common::*;
use crate::sse::SseEvent;
use crate::*;
use regex::Regex;
use reqwest::header::{HeaderMap, ACCEPT, AUTHORIZATION};
use std::collections::HashMap;
use std::sync::LazyLock;

const DEFAULT_MAX_TOKENS: u64 = 32_000;
const MIN_BUDGET: u64 = 1024;
const MAX_ID_LEN: usize = 64;
const OAUTH_PREFIX: &str = "sk-ant-oat";
const CLAUDE_CODE_IDENTITY: &str = "You are Claude Code, Anthropic's official CLI for Claude.";

/// Options consumed elsewhere or meaningful only for OpenAI protocols.
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
    "user",
    "include",
    "store",
    "reasoningSummary",
    "textVerbosity",
    "parallelToolCalls",
    "promptCacheKey",
    "sendReasoning",
    "toolStreaming",
    "thinking",
    "effort",
    "outputConfig",
    "output_config",
    "reasoningEffort",
];

pub(crate) async fn stream(
    client: &reqwest::Client,
    spec: &ModelSpec,
    req: &ChatRequest,
    cancel: &CancellationToken,
    on_event: &mut (dyn FnMut(StreamEvent) + Send),
) -> Result<FinishReason, LlmError> {
    let url = format!("{}/messages", spec.base_url.trim_end_matches('/'));
    let body = build_body(spec, req);
    let mut parser = AnthropicParser::default();
    run(client, spec, &url, headers(spec, req), &body, &mut parser, cancel, on_event).await
}

fn is_oauth(spec: &ModelSpec) -> bool {
    spec.api_key.as_deref().is_some_and(|k| k.starts_with(OAUTH_PREFIX))
}

fn headers(spec: &ModelSpec, req: &ChatRequest) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(ACCEPT, reqwest::header::HeaderValue::from_static("text/event-stream"));
    set_header(&mut h, "anthropic-version", "2023-06-01");
    let oauth = is_oauth(spec);
    if let Some(key) = spec.api_key.as_deref().filter(|k| !k.is_empty()) {
        if oauth {
            set_header(&mut h, AUTHORIZATION.as_str(), &format!("Bearer {key}"));
        } else {
            set_header(&mut h, "x-api-key", key);
        }
    }
    let mut betas: Vec<String> = Vec::new();
    if oauth {
        betas.extend(["claude-code-20250219".into(), "oauth-2025-04-20".into()]);
    }
    betas.push("interleaved-thinking-2025-05-14".into());
    if !req.tools.is_empty() {
        betas.push("fine-grained-tool-streaming-2025-05-14".into());
    }
    // Merge the user's anthropic-beta values instead of letting them replace ours.
    for (k, v) in &spec.headers {
        if k.eq_ignore_ascii_case("anthropic-beta") {
            betas.extend(v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
        }
    }
    let mut seen = std::collections::HashSet::new();
    betas.retain(|b| seen.insert(b.clone()));
    let extra: BTreeMap<String, String> =
        spec.headers.iter().filter(|(k, _)| !k.eq_ignore_ascii_case("anthropic-beta")).map(|(k, v)| (k.clone(), v.clone())).collect();
    apply_headers(&mut h, &extra);
    set_header(&mut h, "anthropic-beta", &betas.join(","));
    h
}

/// Parsed Claude version info from a model id.
#[derive(Debug, PartialEq, Eq)]
struct ClaudeVersion {
    major: u32,
    minor: u32,
}

static FAMILY_FIRST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:^|[^a-z])(?:opus|sonnet|haiku|fable|mythos)-(\d{1,2})(?:[.-](\d{1,2}))?(?:$|[^\d])").expect("regex")
});
static VERSION_FIRST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)claude-(\d{1,2})(?:[.-](\d{1,2}))?-(?:opus|sonnet|haiku|fable|mythos)").expect("regex")
});

fn claude_version(model: &str) -> Option<ClaudeVersion> {
    let caps = FAMILY_FIRST.captures(model).or_else(|| VERSION_FIRST.captures(model))?;
    let major = caps.get(1)?.as_str().parse().ok()?;
    let minor = caps.get(2).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
    Some(ClaudeVersion { major, minor })
}

/// Adaptive thinking (Claude ≥ 4.6, and non-Claude models behind Anthropic-compatible APIs)
/// versus manual token budgets (Claude 3.x, 4.0–4.5).
pub(crate) fn supports_adaptive(model: &str) -> bool {
    match claude_version(model) {
        Some(v) => v.major > 4 || (v.major == 4 && v.minor >= 6),
        None => !model.to_ascii_lowercase().contains("claude-3"),
    }
}

fn effort_budget(effort: &str) -> u64 {
    match effort {
        "minimal" => MIN_BUDGET,
        "low" => 4096,
        "medium" => 10_000,
        "high" => 16_000,
        _ => 32_000, // xhigh, max
    }
}

/// Resolve the `thinking` / `output_config` request fields from options.
fn thinking_config(spec: &ModelSpec) -> (Option<Value>, Option<String>) {
    let opts = &spec.options;
    let mut effort: Option<String> = opts
        .get("effort")
        .or_else(|| opts.get("outputConfig").and_then(|o| o.get("effort")))
        .or_else(|| opts.get("output_config").and_then(|o| o.get("effort")))
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut thinking = opts.get("thinking").filter(|t| t.is_object()).cloned();

    if thinking.is_none() {
        if let Some(re) = opts.get("reasoningEffort").and_then(Value::as_str) {
            if re == "none" {
                thinking = Some(serde_json::json!({"type": "disabled"}));
            } else if supports_adaptive(&spec.api_model) {
                thinking = Some(serde_json::json!({"type": "adaptive", "display": "summarized"}));
                if effort.is_none() {
                    effort = Some(if re == "minimal" { "low".to_string() } else { re.to_string() });
                }
            } else {
                thinking = Some(serde_json::json!({"type": "enabled", "budget_tokens": effort_budget(re)}));
            }
        } else if spec.reasoning && supports_adaptive(&spec.api_model) {
            // Newer models default to `display: omitted` (empty thinking text); ask for summaries.
            thinking = Some(serde_json::json!({"type": "adaptive", "display": "summarized"}));
        }
    }
    // Normalize budgetTokens → budget_tokens.
    if let Some(Value::Object(t)) = thinking.as_mut() {
        if let Some(b) = t.remove("budgetTokens") {
            t.entry("budget_tokens").or_insert(b);
        }
    }
    (thinking, effort)
}

pub(crate) fn build_body(spec: &ModelSpec, req: &ChatRequest) -> Value {
    let mut body = Map::new();
    body.insert("model".into(), Value::String(spec.api_model.clone()));
    let mut max = max_tokens(spec, req).unwrap_or(DEFAULT_MAX_TOKENS);

    let (mut thinking, effort) = thinking_config(spec);
    if let Some(Value::Object(t)) = thinking.as_mut() {
        if t.get("type").and_then(Value::as_str) == Some("enabled") {
            let mut budget = t.get("budget_tokens").and_then(Value::as_u64).unwrap_or(16_000).max(MIN_BUDGET);
            if budget >= max {
                budget = max.saturating_sub(MIN_BUDGET).max(MIN_BUDGET);
                if budget >= max {
                    max = budget + MIN_BUDGET;
                }
            }
            t.insert("budget_tokens".into(), Value::from(budget));
        }
    }
    let thinking_on = thinking
        .as_ref()
        .and_then(|t| t.get("type"))
        .and_then(Value::as_str)
        .is_some_and(|t| t != "disabled");
    body.insert("max_tokens".into(), Value::from(max));
    body.insert("stream".into(), Value::Bool(true));

    // System blocks.
    let mut system: Vec<Value> = Vec::new();
    if is_oauth(spec) {
        system.push(serde_json::json!({"type": "text", "text": CLAUDE_CODE_IDENTITY}));
    }
    for s in req.system.iter().filter(|s| !s.trim().is_empty()) {
        system.push(serde_json::json!({"type": "text", "text": s}));
    }
    let mut messages = build_messages(req);
    let mut tools: Vec<Value> = req
        .tools
        .iter()
        .map(|t| serde_json::json!({"name": t.name, "description": t.description, "input_schema": t.parameters}))
        .collect();
    apply_cache_control(&mut tools, &mut system, &mut messages);
    if !system.is_empty() {
        body.insert("system".into(), Value::Array(system));
    }
    body.insert("messages".into(), Value::Array(messages));
    if !tools.is_empty() {
        body.insert("tools".into(), Value::Array(tools));
        body.insert("tool_choice".into(), serde_json::json!({"type": "auto"}));
    }
    if let Some(t) = thinking {
        body.insert("thinking".into(), t);
    }
    if let Some(e) = effort {
        set_nested(&mut body, "output_config", "effort", Value::String(e));
    }

    for (k, v) in &spec.options {
        if IGNORED_OPTIONS.contains(&k.as_str()) || v.is_null() {
            continue;
        }
        let key = match k.as_str() {
            "serviceTier" | "cacheControl" | "inferenceGeo" | "topP" | "topK" | "stopSequences" => snake_case(k),
            _ => k.clone(),
        };
        body.insert(key, v.clone());
    }
    merge_extra_body(&mut body, spec);
    if thinking_on {
        // Sampling parameters are rejected while thinking is enabled.
        for k in ["temperature", "top_p", "top_k"] {
            body.remove(k);
        }
    }
    Value::Object(body)
}

fn image_block(img: &ImageData) -> Value {
    serde_json::json!({"type": "image", "source": {"type": "base64", "media_type": img.mime, "data": img.data}})
}

/// Convert history into alternating user/assistant messages.
fn build_messages(req: &ChatRequest) -> Vec<Value> {
    let mut out: Vec<(String, Vec<Value>)> = Vec::new();
    let mut push = |role: &str, blocks: Vec<Value>| {
        if blocks.is_empty() {
            return;
        }
        match out.last_mut() {
            Some((r, b)) if r == role => b.extend(blocks),
            _ => out.push((role.to_string(), blocks)),
        }
    };
    for msg in normalize(&req.messages) {
        match msg {
            LlmMessage::User(content) => {
                let blocks = content
                    .iter()
                    .filter_map(|c| match c {
                        Content::Text(t) if t.trim().is_empty() => None,
                        Content::Text(t) => Some(serde_json::json!({"type": "text", "text": t})),
                        Content::Image(img) => Some(image_block(img)),
                    })
                    .collect();
                push("user", blocks);
            }
            LlmMessage::Assistant { parts, replay } => {
                let mut blocks = Vec::new();
                for p in &parts {
                    match p {
                        AssistantPart::Text(t) if t.trim().is_empty() => {}
                        AssistantPart::Text(t) => blocks.push(serde_json::json!({"type": "text", "text": t})),
                        AssistantPart::Reasoning { text, replay: data } => {
                            if !replay {
                                continue;
                            }
                            if let Some(sig) = data.get("signature").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                                blocks.push(serde_json::json!({"type": "thinking", "thinking": text, "signature": sig}));
                            } else if let Some(d) = data.get("redacted").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                                blocks.push(serde_json::json!({"type": "redacted_thinking", "data": d}));
                            }
                        }
                        AssistantPart::ToolCall { id, name, arguments } => blocks.push(serde_json::json!({
                            "type": "tool_use", "id": sanitize_id(id, MAX_ID_LEN), "name": name,
                            "input": args_object(arguments),
                        })),
                    }
                }
                // Thinking-only content is not a valid assistant turn.
                if blocks.iter().any(|b| matches!(b["type"].as_str(), Some("text" | "tool_use"))) {
                    push("assistant", blocks);
                }
            }
            LlmMessage::ToolResult { call_id, content, images, is_error, .. } => {
                let text = tool_output_text(&content);
                let mut extra = Vec::new();
                let content = if images.is_empty() {
                    Value::String(text)
                } else if is_error {
                    // Error results may only contain text; images follow as sibling blocks.
                    extra.extend(images.iter().map(image_block));
                    Value::String(text)
                } else {
                    let mut parts = vec![serde_json::json!({"type": "text", "text": text})];
                    parts.extend(images.iter().map(image_block));
                    Value::Array(parts)
                };
                let mut blocks = vec![serde_json::json!({
                    "type": "tool_result", "tool_use_id": sanitize_id(&call_id, MAX_ID_LEN),
                    "content": content, "is_error": is_error,
                })];
                blocks.extend(extra);
                push("user", blocks);
            }
        }
    }
    if out.first().is_some_and(|(r, _)| r == "assistant") {
        out.insert(0, ("user".into(), vec![serde_json::json!({"type": "text", "text": "(continued)"})]));
    }
    // Tool results must lead the user message that follows a tool_use turn.
    for (role, blocks) in out.iter_mut() {
        if role == "user" {
            blocks.sort_by_key(|b| b["type"] != "tool_result");
        }
    }
    out.into_iter().map(|(role, content)| serde_json::json!({"role": role, "content": content})).collect()
}

/// Place up to 4 ephemeral cache breakpoints (cache order is tools → system → messages):
/// - the second-to-last system block: callers put per-conversation data (session id, env)
///   in the last block, so this prefix (tools + shared instructions) is reusable by other
///   conversations; with a single system block, the last tool instead;
/// - the last system block;
/// - the last block of the last two user messages.
fn apply_cache_control(tools: &mut [Value], system: &mut [Value], messages: &mut [Value]) {
    let mark = |v: &mut Value| {
        if let Value::Object(o) = v {
            o.insert("cache_control".into(), serde_json::json!({"type": "ephemeral"}));
        }
    };
    let n = system.len();
    if n >= 2 {
        mark(&mut system[n - 2]);
    } else if let Some(t) = tools.last_mut() {
        mark(t);
    }
    if let Some(s) = system.last_mut() {
        mark(s);
    }
    let mut marked = 0;
    for m in messages.iter_mut().rev() {
        if marked == 2 {
            break;
        }
        if m["role"] != "user" {
            continue;
        }
        if let Some(b) = m.get_mut("content").and_then(Value::as_array_mut).and_then(|c| c.last_mut()) {
            mark(b);
            marked += 1;
        }
    }
}

#[derive(Debug)]
enum Block {
    Text,
    Thinking { signature: String },
    Redacted { data: String },
    Tool { id: String, args: String },
    Other,
}

/// Incremental parser for Anthropic SSE events, keyed by content block index.
#[derive(Debug, Default)]
pub(crate) struct AnthropicParser {
    blocks: HashMap<u64, Block>,
    usage: Usage,
    saw_usage: bool,
    stop_reason: Option<String>,
    stopped: bool,
    saw_tool: bool,
}

impl AnthropicParser {
    fn apply_usage(&mut self, u: &Value) {
        self.saw_usage = true;
        if let Some(v) = u64_at(u, "/input_tokens") {
            self.usage.input = v;
        }
        if let Some(v) = u64_at(u, "/cache_read_input_tokens") {
            self.usage.cache_read = v;
        }
        if let Some(v) = u64_at(u, "/cache_creation_input_tokens") {
            self.usage.cache_write = v;
        }
        if let Some(v) = u64_at(u, "/output_tokens") {
            self.usage.output = v;
        }
        if let Some(v) = u64_at(u, "/output_tokens_details/thinking_tokens") {
            self.usage.reasoning = v;
        }
    }

    fn block_start(&mut self, index: u64, cb: &Value, out: &mut Vec<StreamEvent>) {
        let block = match cb.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(t) = cb.get("text").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                    out.push(StreamEvent::TextDelta(t.to_string()));
                }
                Block::Text
            }
            Some("thinking") => {
                if let Some(t) = cb.get("thinking").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                    out.push(StreamEvent::ReasoningDelta(t.to_string()));
                }
                Block::Thinking { signature: cb.get("signature").and_then(Value::as_str).unwrap_or("").to_string() }
            }
            Some("redacted_thinking") => {
                Block::Redacted { data: cb.get("data").and_then(Value::as_str).unwrap_or("").to_string() }
            }
            Some("tool_use") => {
                let id = cb.get("id").and_then(Value::as_str).filter(|s| !s.is_empty());
                let id = id.map_or_else(generate_call_id, str::to_string);
                let name = cb.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                self.saw_tool = true;
                out.push(StreamEvent::ToolCallStart { id: id.clone(), name });
                // Non-streaming bodies carry the full input here; streams send `{}` + deltas.
                let args = match cb.get("input") {
                    Some(Value::Object(o)) if !o.is_empty() => {
                        let s = Value::Object(o.clone()).to_string();
                        out.push(StreamEvent::ToolCallDelta { id: id.clone(), delta: s.clone() });
                        s
                    }
                    _ => String::new(),
                };
                Block::Tool { id, args }
            }
            _ => Block::Other,
        };
        self.blocks.insert(index, block);
    }

    fn block_delta(&mut self, index: u64, d: &Value, out: &mut Vec<StreamEvent>) {
        let Some(block) = self.blocks.get_mut(&index) else { return };
        let ty = d.get("type").and_then(Value::as_str).unwrap_or("");
        match (ty, block) {
            ("text_delta", Block::Text) => {
                if let Some(t) = d.get("text").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                    out.push(StreamEvent::TextDelta(t.to_string()));
                }
            }
            ("thinking_delta", Block::Thinking { .. }) => {
                if let Some(t) = d.get("thinking").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                    out.push(StreamEvent::ReasoningDelta(t.to_string()));
                }
            }
            ("signature_delta", Block::Thinking { signature }) => {
                signature.push_str(d.get("signature").and_then(Value::as_str).unwrap_or(""));
            }
            ("input_json_delta", Block::Tool { id, args }) => {
                if let Some(p) = d.get("partial_json").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                    args.push_str(p);
                    out.push(StreamEvent::ToolCallDelta { id: id.clone(), delta: p.to_string() });
                }
            }
            _ => {}
        }
    }

    fn block_stop(&mut self, index: u64, out: &mut Vec<StreamEvent>) {
        match self.blocks.remove(&index) {
            Some(Block::Thinking { signature }) => {
                out.push(StreamEvent::ReasoningEnd { replay: serde_json::json!({"signature": signature}) })
            }
            Some(Block::Redacted { data }) => {
                out.push(StreamEvent::ReasoningEnd { replay: serde_json::json!({"redacted": data}) })
            }
            Some(Block::Tool { id, args }) => {
                let arguments = if args.trim().is_empty() { "{}".to_string() } else { args };
                out.push(StreamEvent::ToolCallEnd { id, arguments });
            }
            _ => {}
        }
    }

    /// Non-streaming `{"type":"message", ...}` body.
    fn whole_message(&mut self, v: &Value, out: &mut Vec<StreamEvent>) {
        if let Some(u) = v.get("usage") {
            self.apply_usage(u);
        }
        for (i, b) in v.get("content").and_then(Value::as_array).into_iter().flatten().enumerate() {
            let i = i as u64;
            self.block_start(i, b, out);
            if let (Some(Block::Thinking { signature }), Some(sig)) =
                (self.blocks.get_mut(&i), b.get("signature").and_then(Value::as_str))
            {
                *signature = sig.to_string();
            }
            self.block_stop(i, out);
        }
        self.stop_reason = v.get("stop_reason").and_then(Value::as_str).map(str::to_string);
        self.stopped = true;
    }
}

impl Parser for AnthropicParser {
    fn feed(&mut self, ev: &SseEvent) -> Result<Vec<StreamEvent>, LlmError> {
        let mut out = Vec::new();
        let data = ev.data.trim();
        if data.is_empty() {
            return Ok(out);
        }
        let Some(v) = parse_json(data) else { return Ok(out) };
        let ty = v.get("type").and_then(Value::as_str).or(ev.event.as_deref()).unwrap_or("");
        let index = v.get("index").and_then(Value::as_u64).unwrap_or(0);
        match ty {
            "message_start" => {
                if let Some(u) = v.pointer("/message/usage") {
                    self.apply_usage(u);
                }
            }
            "content_block_start" => {
                if let Some(cb) = v.get("content_block") {
                    self.block_start(index, cb, &mut out);
                }
            }
            "content_block_delta" => {
                if let Some(d) = v.get("delta") {
                    self.block_delta(index, d, &mut out);
                }
            }
            "content_block_stop" => self.block_stop(index, &mut out),
            "message_delta" => {
                if let Some(r) = str_at(&v, "/delta/stop_reason") {
                    self.stop_reason = Some(r.to_string());
                }
                if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
                    self.apply_usage(u);
                }
            }
            "message_stop" => self.stopped = true,
            "message" => self.whole_message(&v, &mut out),
            "error" => return Err(LlmError::from_stream_payload(data)),
            _ => {} // ping and unknown events
        }
        Ok(out)
    }

    fn done(&self) -> bool {
        self.stopped
    }

    fn finish(&mut self) -> Result<(Vec<StreamEvent>, FinishReason), LlmError> {
        // Some proxies drop `message_stop`; a stop_reason is enough evidence of completion.
        if !self.stopped && self.stop_reason.is_none() {
            return Err(LlmError::incomplete("no message_stop event"));
        }
        let mut out = Vec::new();
        let mut open: Vec<u64> = self.blocks.keys().copied().collect();
        open.sort_unstable();
        for i in open {
            self.block_stop(i, &mut out);
        }
        if self.saw_usage {
            out.push(StreamEvent::Usage(self.usage.clone()));
        }
        let reason = match self.stop_reason.as_deref() {
            Some("tool_use") => FinishReason::ToolUse,
            Some("max_tokens" | "model_context_window_exceeded") => FinishReason::Length,
            Some("refusal") => FinishReason::ContentFilter,
            None | Some("end_turn" | "stop_sequence" | "pause_turn") => {
                if self.saw_tool { FinishReason::ToolUse } else { FinishReason::Stop }
            }
            Some(other) => FinishReason::Other(other.to_string()),
        };
        Ok((out, reason))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const STREAM: &str = r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_1","model":"claude-sonnet-4-5","usage":{"input_tokens":12,"cache_read_input_tokens":9000,"cache_creation_input_tokens":300,"output_tokens":1}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Let me"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"EqoB"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"redacted_thinking","data":"EmwK"}}

event: content_block_stop
data: {"type":"content_block_stop","index":1}

event: content_block_start
data: {"type":"content_block_start","index":2,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":2,"delta":{"type":"text_delta","text":"Reading"}}

event: content_block_stop
data: {"type":"content_block_stop","index":2}

event: ping
data: {"type":"ping"}

event: content_block_start
data: {"type":"content_block_start","index":3,"content_block":{"type":"tool_use","id":"toolu_01","name":"read","input":{}}}

event: content_block_delta
data: {"type":"content_block_delta","index":3,"delta":{"type":"input_json_delta","partial_json":"{\"path\":"}}

event: content_block_delta
data: {"type":"content_block_delta","index":3,"delta":{"type":"input_json_delta","partial_json":"\"a.rs\"}"}}

event: content_block_stop
data: {"type":"content_block_stop","index":3}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":120}}

event: message_stop
data: {"type":"message_stop"}

"#;

    #[test]
    fn parses_stream() {
        let (events, reason) = run_parser(&mut AnthropicParser::default(), STREAM).unwrap();
        assert_eq!(reason, FinishReason::ToolUse);
        let dbg: Vec<String> = events.iter().map(|e| format!("{e:?}")).collect();
        assert!(matches!(&events[0], StreamEvent::ReasoningDelta(s) if s == "Let me"), "{dbg:?}");
        assert!(matches!(&events[1], StreamEvent::ReasoningEnd { replay } if *replay == json!({"signature":"EqoB"})));
        assert!(matches!(&events[2], StreamEvent::ReasoningEnd { replay } if *replay == json!({"redacted":"EmwK"})));
        assert!(matches!(&events[3], StreamEvent::TextDelta(s) if s == "Reading"));
        assert!(matches!(&events[4], StreamEvent::ToolCallStart { id, name } if id == "toolu_01" && name == "read"));
        assert!(matches!(&events[7], StreamEvent::ToolCallEnd { id, arguments } if id == "toolu_01" && arguments == "{\"path\":\"a.rs\"}"));
        let StreamEvent::Usage(u) = &events[8] else { panic!("{dbg:?}") };
        assert_eq!((u.input, u.cache_read, u.cache_write, u.output), (12, 9000, 300, 120));
    }

    #[test]
    fn errors_and_incomplete() {
        let truncated = STREAM.split("event: message_delta").next().unwrap();
        let err = run_parser(&mut AnthropicParser::default(), truncated).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Retryable);
        let raw = "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n";
        assert_eq!(run_parser(&mut AnthropicParser::default(), raw).unwrap_err().kind, ErrorKind::Retryable);
    }

    #[test]
    fn version_detection() {
        assert!(supports_adaptive("claude-opus-5-5"));
        assert!(supports_adaptive("claude-opus-4-6"));
        assert!(supports_adaptive("claude-sonnet-4.7"));
        assert!(!supports_adaptive("claude-sonnet-4-5-20250929"));
        assert!(!supports_adaptive("claude-sonnet-4-20250514"));
        assert!(!supports_adaptive("claude-3-7-sonnet-latest"));
        assert!(!supports_adaptive("anthropic/claude-opus-4.1"));
        assert!(supports_adaptive("glm-4.6"));
    }

    fn spec(model: &str) -> ModelSpec {
        ModelSpec {
            provider_id: "zero".into(),
            api_model: model.into(),
            protocol: Some(Protocol::Anthropic),
            base_url: "https://zero.cat/v1".into(),
            api_key: Some("sk-x".into()),
            output: 64000,
            ..Default::default()
        }
    }

    fn history() -> Vec<LlmMessage> {
        let call = |id: &str| AssistantPart::ToolCall { id: id.into(), name: "read".into(), arguments: "{\"p\":1}".into() };
        let result = |id: &str| LlmMessage::ToolResult {
            call_id: id.into(),
            name: "read".into(),
            content: "ok".into(),
            images: vec![],
            is_error: false,
        };
        vec![
            LlmMessage::User(vec![Content::Text("hi".into()), Content::Text("".into())]),
            LlmMessage::Assistant {
                parts: vec![
                    AssistantPart::Reasoning { text: "think".into(), replay: json!({"signature": "sig"}) },
                    AssistantPart::Reasoning { text: "lost".into(), replay: json!({"signature": ""}) },
                    AssistantPart::Text("".into()),
                    call("toolu_1"),
                    call("toolu_2"),
                ],
                replay: true,
            },
            result("toolu_1"),
            result("toolu_2"),
            LlmMessage::User(vec![Content::Text("more".into())]),
            LlmMessage::Assistant { parts: vec![AssistantPart::Reasoning { text: "x".into(), replay: json!({"signature": "s2"}) }, AssistantPart::Text("done".into())], replay: false },
            LlmMessage::User(vec![Content::Text("last".into())]),
        ]
    }

    #[test]
    fn body_structure_and_cache() {
        let req = ChatRequest {
            system: vec!["base".into(), "env".into()],
            messages: history(),
            tools: vec![
                ToolDef { name: "a".into(), description: "".into(), parameters: json!({"type":"object"}) },
                ToolDef { name: "b".into(), description: "".into(), parameters: json!({"type":"object"}) },
            ],
            ..Default::default()
        };
        let mut s = spec("claude-sonnet-4-5");
        s.options.insert("temperature".into(), json!(0.5));
        let b = build_body(&s, &req);
        let msgs = b["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 5);
        assert_eq!(msgs[0]["content"], json!([{"type":"text","text":"hi"}]));
        let a = msgs[1]["content"].as_array().unwrap();
        assert_eq!(a[0], json!({"type":"thinking","thinking":"think","signature":"sig"}));
        assert_eq!(a.len(), 3, "unsigned thinking and empty text dropped");
        assert_eq!(a[1]["input"], json!({"p":1}));
        // Both results + the next user text merged into one user message, results first.
        let u = msgs[2]["content"].as_array().unwrap();
        assert_eq!(u.len(), 3);
        assert_eq!(u[0]["type"], "tool_result");
        assert_eq!(u[1]["tool_use_id"], "toolu_2");
        assert_eq!(u[2]["text"], "more");
        // Non-replay assistant: reasoning dropped.
        assert_eq!(msgs[3]["content"], json!([{"type":"text","text":"done"}]));
        // Cache breakpoints.
        let count = b.to_string().matches("cache_control").count();
        assert_eq!(count, 4);
        // Two system blocks: both marked (shared prefix + per-conversation tail); the system
        // breakpoint already covers the tools.
        assert!(b["tools"][1].get("cache_control").is_none() && b["tools"][0].get("cache_control").is_none());
        assert!(b["system"][1].get("cache_control").is_some() && b["system"][0].get("cache_control").is_some());
        // A single system block: the last tool gets the extra breakpoint.
        let one = ChatRequest { system: vec!["only".into()], ..req.clone() };
        let b1 = build_body(&s, &one);
        assert!(b1["tools"][1].get("cache_control").is_some());
        assert_eq!(b1.to_string().matches("cache_control").count(), 4);
        assert!(u[2].get("cache_control").is_some());
        assert!(msgs[4]["content"][0].get("cache_control").is_some());
        assert!(msgs[0]["content"][0].get("cache_control").is_none());
        assert_eq!(b["max_tokens"], 64000);
        assert_eq!(b["temperature"], 0.5);
        assert!(b.get("thinking").is_none());
    }

    #[test]
    fn thinking_variants() {
        let req = ChatRequest { messages: vec![LlmMessage::User(vec![Content::Text("x".into())])], ..Default::default() };
        let mut s = spec("claude-opus-5-5");
        s.options.insert("reasoningEffort".into(), json!("high"));
        s.options.insert("temperature".into(), json!(1));
        let b = build_body(&s, &req);
        assert_eq!(b["thinking"], json!({"type":"adaptive","display":"summarized"}));
        assert_eq!(b["output_config"], json!({"effort":"high"}));
        assert!(b.get("temperature").is_none());
        assert!(b.get("reasoningEffort").is_none());

        let mut s = spec("claude-sonnet-4-5");
        s.options.insert("reasoningEffort".into(), json!("high"));
        let b = build_body(&s, &req);
        assert_eq!(b["thinking"], json!({"type":"enabled","budget_tokens":16000}));
        assert!(b.get("output_config").is_none());

        let mut s = spec("claude-sonnet-4-5");
        s.options.insert("thinking".into(), json!({"type":"enabled","budgetTokens":50000}));
        let r = ChatRequest { max_tokens: Some(20000), ..req.clone() };
        let b = build_body(&s, &r);
        assert_eq!(b["thinking"], json!({"type":"enabled","budget_tokens":18976}));
        assert_eq!(b["max_tokens"], 20000);

        let mut s = spec("claude-opus-4-6");
        s.options.insert("outputConfig".into(), json!({"effort":"max"}));
        s.options.insert("thinking".into(), json!({"type":"adaptive"}));
        let b = build_body(&s, &req);
        assert_eq!(b["thinking"], json!({"type":"adaptive"}));
        assert_eq!(b["output_config"], json!({"effort":"max"}));

        let mut s = spec("claude-opus-4-6");
        s.reasoning = true;
        assert_eq!(build_body(&s, &req)["thinking"], json!({"type":"adaptive","display":"summarized"}));
        s.api_model = "claude-sonnet-4-5".into();
        assert!(build_body(&s, &req).get("thinking").is_none());
    }

    #[test]
    fn headers_merge_beta_and_oauth() {
        let mut s = spec("claude-opus-5-5");
        s.headers.insert("anthropic-beta".into(), "context-1m-2025-08-07".into());
        s.headers.insert("x-custom".into(), "1".into());
        let req = ChatRequest::default();
        let h = headers(&s, &req);
        assert_eq!(h["x-api-key"], "sk-x");
        assert!(h.get("authorization").is_none());
        assert_eq!(h["anthropic-beta"], "interleaved-thinking-2025-05-14,context-1m-2025-08-07");
        assert_eq!(h["x-custom"], "1");
        s.api_key = Some("sk-ant-oat01-abc".into());
        let h = headers(&s, &req);
        assert!(h.get("x-api-key").is_none());
        assert_eq!(h["authorization"], "Bearer sk-ant-oat01-abc");
        assert_eq!(build_body(&s, &req)["system"][0]["text"], CLAUDE_CODE_IDENTITY);
    }

    /// Captured from claude-haiku-5.5 via coding.dgnai.cn (signature shortened): an adaptive
    /// thinking block with NO thinking text, only a signature, plus the gateway's extra
    /// `contextUsagePercentage` usage field and the cache usage split.
    #[test]
    fn dgn_signature_only_thinking_capture() {
        let raw = r#"event: message_start
data: {"message":{"content":[],"id":"msg_716a","model":"claude-haiku-5.5","role":"assistant","stop_reason":null,"stop_sequence":null,"type":"message","usage":{"cache_creation_input_tokens":0,"cache_read_input_tokens":67,"input_tokens":15,"output_tokens":1}},"type":"message_start"}

event: content_block_start
data: {"content_block":{"thinking":"","type":"thinking"},"index":0,"type":"content_block_start"}

event: content_block_delta
data: {"delta":{"signature":"CAQShAcKEggTEAEYAjgB","type":"signature_delta"},"index":0,"type":"content_block_delta"}

event: content_block_stop
data: {"index":0,"type":"content_block_stop"}

event: content_block_start
data: {"content_block":{"text":"","type":"text"},"index":1,"type":"content_block_start"}

event: content_block_delta
data: {"delta":{"text":"It is sunny.","type":"text_delta"},"index":1,"type":"content_block_delta"}

event: content_block_stop
data: {"index":1,"type":"content_block_stop"}

event: message_delta
data: {"delta":{"stop_reason":"end_turn","stop_sequence":null},"type":"message_delta","usage":{"cache_creation":{"ephemeral_1h_input_tokens":0,"ephemeral_5m_input_tokens":0},"cache_creation_input_tokens":0,"cache_read_input_tokens":67,"contextUsagePercentage":0.2556999921798706,"input_tokens":15,"output_tokens":16}}

event: message_stop
data: {"type":"message_stop"}

"#;
        let (events, reason) = run_parser(&mut AnthropicParser::default(), raw).unwrap();
        assert_eq!(reason, FinishReason::Stop);
        assert!(matches!(&events[0], StreamEvent::ReasoningEnd { replay } if replay["signature"] == "CAQShAcKEggTEAEYAjgB"));
        let StreamEvent::Usage(u) = events.last().unwrap() else { panic!() };
        assert_eq!((u.input, u.cache_read, u.cache_write, u.output), (15, 67, 0, 16));
        // The empty-text signed block must still be replayed before the tool_use.
        let req = ChatRequest {
            messages: vec![
                LlmMessage::User(vec![Content::Text("q".into())]),
                LlmMessage::Assistant {
                    parts: vec![
                        AssistantPart::Reasoning { text: String::new(), replay: json!({"signature": "CAQS"}) },
                        AssistantPart::ToolCall { id: "toolu_bdrk_01PzUrkrVxVbWocYTYb5tSAm".into(), name: "w".into(), arguments: "{}".into() },
                    ],
                    replay: true,
                },
            ],
            ..Default::default()
        };
        let b = build_body(&spec("claude-haiku-5.5"), &req);
        assert_eq!(b["messages"][1]["content"][0], json!({"type":"thinking","thinking":"","signature":"CAQS"}));
        assert_eq!(b["messages"][1]["content"][1]["id"], "toolu_bdrk_01PzUrkrVxVbWocYTYb5tSAm");
    }
}
