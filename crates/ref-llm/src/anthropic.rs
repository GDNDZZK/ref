//! Anthropic Messages API (`POST {base_url}/messages`).

use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;
use serde_json::{json, Map, Value};

use crate::convert::{self, AsstBlock, IdMap, ToolResult, Turn, UserBlock};
use crate::sse::SseEvent;
use crate::transport::{self, Decoder, Flow, Headers};
use crate::{EventStream, FinishReason, LlmError, ModelSpec, Request, StreamEvent};

const DEFAULT_MAX_TOKENS: u64 = 32_000;
const MIN_BUDGET: u64 = 1024;
const MAX_CACHE_BREAKPOINTS: usize = 4;
const BETA_INTERLEAVED: &str = "interleaved-thinking-2025-05-14";
const BETA_FINE_GRAINED_TOOLS: &str = "fine-grained-tool-streaming-2025-05-14";
const BETA_OAUTH: &str = "oauth-2025-04-20";

pub(crate) async fn stream(http: &reqwest::Client, model: &ModelSpec, req: &Request) -> Result<EventStream, LlmError> {
    let built = build_body(model, req);
    let headers = build_headers(model, &built)?;
    let url = transport::endpoint(model, "messages")?;
    transport::post_sse(http, model, &url, headers, &built.body, AnthropicDecoder::new(model)).await
}

pub(crate) struct Built {
    pub body: Value,
    pub thinking: Thinking,
}

// ───────────────────────────── thinking config ─────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Thinking {
    /// Nothing requested: no `thinking` field (provider default).
    Default,
    Disabled,
    Budget(u64),
    Adaptive(Option<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ClaudeVersion {
    family: &'static str,
    major: u32,
    minor: u32,
}

fn claude_version(model_id: &str) -> Option<ClaudeVersion> {
    static NEW: OnceLock<Regex> = OnceLock::new();
    static OLD: OnceLock<Regex> = OnceLock::new();
    let id = model_id.to_ascii_lowercase();
    let new = NEW.get_or_init(|| {
        Regex::new(r"(opus|sonnet|haiku|fable|mythos)[-_ ]?(\d{1,2})(?:[-.](\d{1,2}))?(?:\D|$)").expect("regex")
    });
    let family = |s: &str| -> &'static str {
        match s {
            "opus" => "opus",
            "sonnet" => "sonnet",
            "haiku" => "haiku",
            "fable" => "fable",
            _ => "mythos",
        }
    };
    if let Some(c) = new.captures(&id) {
        return Some(ClaudeVersion {
            family: family(&c[1]),
            major: c[2].parse().ok()?,
            minor: c.get(3).and_then(|m| m.as_str().parse().ok()).unwrap_or(0),
        });
    }
    // Legacy naming: claude-3-7-sonnet-20250219.
    let old = OLD.get_or_init(|| Regex::new(r"claude-(\d)(?:-(\d))?-(opus|sonnet|haiku)").expect("regex"));
    let c = old.captures(&id)?;
    Some(ClaudeVersion {
        family: family(&c[3]),
        major: c[1].parse().ok()?,
        minor: c.get(2).and_then(|m| m.as_str().parse().ok()).unwrap_or(0),
    })
}

/// Claude models with adaptive thinking (`thinking: {type: "adaptive"}` + `output_config.effort`).
pub(crate) fn supports_adaptive(model_id: &str) -> bool {
    claude_version(model_id).is_some_and(|v| v.major >= 5 || (v.major == 4 && v.minor >= 6))
}

fn supports_xhigh(model_id: &str) -> bool {
    claude_version(model_id).is_some_and(|v| v.major >= 5 || (v.family == "opus" && v.major == 4 && v.minor >= 7))
}

fn effort_budget(effort: &str) -> u64 {
    match effort {
        "minimal" => 1024,
        "low" => 2048,
        "medium" => 8192,
        "high" => 16_384,
        _ => 32_000,
    }
}

fn adaptive_effort(model_id: &str, effort: &str) -> String {
    match effort {
        "minimal" | "low" => "low",
        "medium" => "medium",
        "high" => "high",
        "xhigh" if supports_xhigh(model_id) => "xhigh",
        "xhigh" => "high",
        "max" => "max",
        _ => "high",
    }
    .to_string()
}

fn opt_str<'a>(opts: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    opts.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

pub(crate) fn thinking_config(model: &ModelSpec) -> Thinking {
    let opts = &model.options;
    let adaptive = supports_adaptive(&model.api_model);
    let effort = opt_str(opts, "effort").or_else(|| opt_str(opts, "reasoningEffort")).map(str::to_ascii_lowercase);
    if matches!(effort.as_deref(), Some("none") | Some("off") | Some("disabled")) {
        return Thinking::Disabled;
    }
    let from_effort = |effort: Option<&str>| -> Thinking {
        if adaptive {
            Thinking::Adaptive(effort.map(|e| adaptive_effort(&model.api_model, e)))
        } else {
            Thinking::Budget(effort_budget(effort.unwrap_or("medium")))
        }
    };
    match opts.get("thinking") {
        Some(Value::Object(t)) => match t.get("type").and_then(Value::as_str).unwrap_or("enabled") {
            "disabled" => Thinking::Disabled,
            "adaptive" => from_effort(effort.as_deref()),
            _ => {
                let budget = t.get("budgetTokens").or_else(|| t.get("budget_tokens")).and_then(Value::as_u64);
                match (budget, effort.as_deref()) {
                    (Some(b), _) => Thinking::Budget(b),
                    (None, e) => from_effort(e),
                }
            }
        },
        Some(Value::Bool(false)) => Thinking::Disabled,
        Some(Value::Bool(true)) => from_effort(effort.as_deref()),
        _ => match effort {
            Some(e) => from_effort(Some(&e)),
            None => Thinking::Default,
        },
    }
}

// ───────────────────────────── request body ─────────────────────────────

fn cache_control() -> Value {
    json!({"type": "ephemeral"})
}

pub(crate) fn build_body(model: &ModelSpec, req: &Request) -> Built {
    let mut body = Map::new();
    body.insert("model".into(), json!(model.api_model));

    let mut max_tokens = if model.max_output > 0 { model.max_output } else { DEFAULT_MAX_TOKENS };
    let mut thinking = thinking_config(model);
    if let Thinking::Budget(b) = &mut thinking {
        let budget = (*b).min(max_tokens.saturating_sub(MIN_BUDGET)).max(MIN_BUDGET);
        if max_tokens <= budget {
            max_tokens = budget + MIN_BUDGET;
        }
        *b = budget;
    }
    body.insert("max_tokens".into(), json!(max_tokens));
    body.insert("stream".into(), json!(true));

    let mut breakpoints = 0usize;
    let system: Vec<Value> = req
        .system
        .iter()
        .filter(|s| !s.trim().is_empty())
        .map(|s| json!({"type": "text", "text": s}))
        .collect();
    if !system.is_empty() {
        let mut system = system;
        if let Some(last) = system.last_mut() {
            last["cache_control"] = cache_control();
            breakpoints += 1;
        }
        body.insert("system".into(), Value::Array(system));
    }

    let mut ids = IdMap::new(64);
    let mut messages = build_messages(model, req, &mut ids);
    if !req.tools.is_empty() {
        let mut tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| json!({"name": t.name, "description": t.description, "input_schema": input_schema(&t.parameters)}))
            .collect();
        if let Some(last) = tools.last_mut() {
            last["cache_control"] = cache_control();
            breakpoints += 1;
        }
        body.insert("tools".into(), Value::Array(tools));
        if req.tool_choice_none {
            body.insert("tool_choice".into(), json!({"type": "none"}));
        }
    }
    add_message_cache_points(&mut messages, MAX_CACHE_BREAKPOINTS.saturating_sub(breakpoints).min(2));
    body.insert("messages".into(), Value::Array(messages));

    let thinking_on = matches!(thinking, Thinking::Budget(_) | Thinking::Adaptive(_));
    match &thinking {
        Thinking::Default => {}
        Thinking::Disabled => {
            body.insert("thinking".into(), json!({"type": "disabled"}));
        }
        Thinking::Budget(b) => {
            body.insert("thinking".into(), json!({"type": "enabled", "budget_tokens": b}));
        }
        Thinking::Adaptive(effort) => {
            body.insert("thinking".into(), json!({"type": "adaptive", "display": "summarized"}));
            if let Some(e) = effort {
                body.insert("output_config".into(), json!({"effort": e}));
            }
        }
    }
    if !thinking_on {
        if let Some(t) = model.temperature {
            body.insert("temperature".into(), json!(t));
        }
        if let Some(p) = model.top_p {
            body.insert("top_p".into(), json!(p));
        }
    }
    transport::merge_json(&mut body, &model.extra_body);
    Built { body: Value::Object(body), thinking }
}

fn input_schema(params: &Value) -> Value {
    match params {
        Value::Object(o) if !o.is_empty() => params.clone(),
        _ => json!({"type": "object", "properties": {}}),
    }
}

/// Put cache breakpoints on the last content block of the last `n` user messages.
fn add_message_cache_points(messages: &mut [Value], n: usize) {
    let mut left = n;
    for msg in messages.iter_mut().rev() {
        if left == 0 {
            break;
        }
        if msg["role"] != "user" {
            continue;
        }
        if let Some(last) = msg["content"].as_array_mut().and_then(|c| c.last_mut()) {
            last["cache_control"] = cache_control();
            left -= 1;
        }
    }
}

fn image_block(img: &convert::Image) -> Value {
    json!({"type": "image", "source": {"type": "base64", "media_type": img.mime, "data": img.data}})
}

struct MsgBuilder {
    messages: Vec<Value>,
}

impl MsgBuilder {
    /// Append blocks to the last message if it has `role`, else start a new message.
    fn push(&mut self, role: &str, blocks: Vec<Value>) {
        if blocks.is_empty() {
            return;
        }
        if let Some(last) = self.messages.last_mut() {
            if last["role"] == role {
                if let Some(arr) = last["content"].as_array_mut() {
                    arr.extend(blocks);
                    return;
                }
            }
        }
        self.messages.push(json!({"role": role, "content": blocks}));
    }
}

fn build_messages(model: &ModelSpec, req: &Request, ids: &mut IdMap) -> Vec<Value> {
    let mut b = MsgBuilder { messages: Vec::new() };
    for turn in convert::flatten(&req.messages, model) {
        match turn {
            Turn::User(blocks) => {
                let blocks = blocks
                    .iter()
                    .map(|u| match u {
                        UserBlock::Text(t) => json!({"type": "text", "text": t}),
                        UserBlock::Image(img) => image_block(img),
                    })
                    .collect();
                b.push("user", blocks);
            }
            Turn::Assistant(blocks) => {
                let blocks = blocks.into_iter().filter_map(|blk| assistant_block(blk, ids)).collect();
                b.push("assistant", blocks);
            }
            Turn::ToolResults(results) => {
                // Tool results must be the first blocks of the user message that follows the
                // assistant tool_use turn; images from error results go after all of them.
                let mut blocks = Vec::new();
                let mut trailing = Vec::new();
                for r in &results {
                    blocks.push(tool_result_block(r, ids, &mut trailing));
                }
                blocks.extend(trailing);
                b.messages.push(json!({"role": "user", "content": blocks}));
            }
        }
    }
    let mut messages = b.messages;
    if messages.first().is_some_and(|m| m["role"] == "assistant") {
        messages.insert(0, json!({"role": "user", "content": [{"type": "text", "text": "(conversation continues)"}]}));
    }
    messages
}

fn assistant_block(blk: AsstBlock, ids: &mut IdMap) -> Option<Value> {
    match blk {
        AsstBlock::Reasoning { text, meta } => {
            let meta = meta?;
            if let Some(data) = meta.get("redacted").and_then(Value::as_str) {
                return Some(json!({"type": "redacted_thinking", "data": data}));
            }
            let sig = meta.get("signature").and_then(Value::as_str).filter(|s| !s.is_empty())?;
            Some(json!({"type": "thinking", "thinking": text, "signature": sig}))
        }
        AsstBlock::Text { text, .. } => (!text.trim().is_empty()).then(|| json!({"type": "text", "text": text})),
        AsstBlock::ToolCall { call_id, name, input, .. } => {
            let input = if input.is_object() { input } else { json!({}) };
            Some(json!({"type": "tool_use", "id": ids.get(&call_id), "name": name, "input": input}))
        }
    }
}

fn tool_result_block(r: &ToolResult, ids: &mut IdMap, trailing: &mut Vec<Value>) -> Value {
    let id = ids.get(&r.call_id);
    let mut block = Map::new();
    block.insert("type".into(), json!("tool_result"));
    block.insert("tool_use_id".into(), json!(id));
    if r.images.is_empty() {
        block.insert("content".into(), json!(r.text));
    } else if r.is_error {
        // Error results must be text-only.
        block.insert("content".into(), json!(format!("{}\n(image attached after the tool results)", r.text)));
        trailing.push(json!({"type": "text", "text": format!("Images from the error result of tool call {id} ({}):", r.name)}));
        trailing.extend(r.images.iter().map(image_block));
    } else {
        let mut content = vec![json!({"type": "text", "text": r.text})];
        content.extend(r.images.iter().map(image_block));
        block.insert("content".into(), Value::Array(content));
    }
    if r.is_error {
        block.insert("is_error".into(), json!(true));
    }
    Value::Object(block)
}

// ───────────────────────────── headers ─────────────────────────────

pub(crate) fn build_headers(model: &ModelSpec, built: &Built) -> Result<Headers, LlmError> {
    let mut h = Headers::new();
    h.set("anthropic-version", "2023-06-01");
    let mut betas: Vec<String> = Vec::new();
    if let Some(key) = model.api_key.as_deref().filter(|k| !k.is_empty()) {
        if key.starts_with("sk-ant-oat") {
            h.set("authorization", &format!("Bearer {key}"));
            betas.push(BETA_OAUTH.into());
        } else {
            h.set("x-api-key", key);
        }
    }
    if matches!(built.thinking, Thinking::Budget(_)) {
        betas.push(BETA_INTERLEAVED.into());
    }
    if built.body.get("tools").is_some() {
        betas.push(BETA_FINE_GRAINED_TOOLS.into());
    }
    if let Some(user) = Headers::user_header(model, "anthropic-beta") {
        betas.extend(user.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
    }
    let mut seen = std::collections::HashSet::new();
    betas.retain(|b| seen.insert(b.clone()));
    h.apply_user(model, &["anthropic-beta"])?;
    if !betas.is_empty() {
        h.set("anthropic-beta", &betas.join(","));
    }
    Ok(h)
}

// ───────────────────────────── stream decoding ─────────────────────────────

#[derive(Debug)]
enum BlockKind {
    Text,
    Thinking { signature: String },
    Redacted { data: String },
    Tool { id: String, name: String, json: String, initial: Option<Value> },
    Ignored,
}

pub(crate) struct AnthropicDecoder {
    model: ModelSpec,
    blocks: HashMap<usize, BlockKind>,
    usage: ref_proto::Usage,
    stop_reason: Option<String>,
    started: bool,
    tool_calls: usize,
}

impl AnthropicDecoder {
    pub(crate) fn new(model: &ModelSpec) -> Self {
        AnthropicDecoder {
            model: model.clone(),
            blocks: HashMap::new(),
            usage: Default::default(),
            stop_reason: None,
            started: false,
            tool_calls: 0,
        }
    }

    fn apply_usage(&mut self, u: &Value) -> bool {
        let mut changed = false;
        let mut set = |field: &mut u64, key: &str| {
            if let Some(v) = u.get(key).and_then(Value::as_u64) {
                *field = v;
                changed = true;
            }
        };
        set(&mut self.usage.input, "input_tokens");
        set(&mut self.usage.output, "output_tokens");
        set(&mut self.usage.cache_read, "cache_read_input_tokens");
        set(&mut self.usage.cache_write, "cache_creation_input_tokens");
        if let Some(v) = u.pointer("/output_tokens_details/thinking_tokens").and_then(Value::as_u64) {
            self.usage.reasoning = v;
            changed = true;
        }
        changed
    }

    fn usage_event(&self) -> StreamEvent {
        StreamEvent::Usage(crate::with_cost(&self.model, self.usage.clone()))
    }

    fn end_block(&mut self, index: usize, out: &mut Vec<StreamEvent>) {
        let Some(kind) = self.blocks.remove(&index) else { return };
        match kind {
            BlockKind::Text => out.push(StreamEvent::TextEnd { index, meta: None }),
            BlockKind::Thinking { signature } => {
                let fields = if signature.is_empty() { json!({}) } else { json!({"signature": signature}) };
                out.push(StreamEvent::ReasoningEnd { index, meta: Some(convert::make_meta(&self.model, fields)) });
            }
            BlockKind::Redacted { data } => out.push(StreamEvent::ReasoningEnd {
                index,
                meta: Some(convert::make_meta(&self.model, json!({"redacted": data}))),
            }),
            BlockKind::Tool { id, name, json, initial } => {
                let input = if json.trim().is_empty() {
                    initial.unwrap_or_else(|| json!({}))
                } else {
                    convert::parse_tool_input(&json)
                };
                out.push(StreamEvent::ToolCallEnd { index, call_id: id, name, input, meta: None });
            }
            BlockKind::Ignored => {}
        }
    }

    fn close_all(&mut self, out: &mut Vec<StreamEvent>) {
        let mut open: Vec<usize> = self.blocks.keys().copied().collect();
        open.sort_unstable();
        for i in open {
            self.end_block(i, out);
        }
    }

    fn finish_reason(&self) -> FinishReason {
        let reason = match self.stop_reason.as_deref() {
            None | Some("end_turn") | Some("stop_sequence") | Some("pause_turn") => FinishReason::Stop,
            Some("tool_use") => FinishReason::ToolUse,
            Some("max_tokens") | Some("model_context_window_exceeded") => FinishReason::Length,
            Some(_) => FinishReason::Error,
        };
        if reason == FinishReason::Stop && self.tool_calls > 0 {
            FinishReason::ToolUse
        } else {
            reason
        }
    }
}

fn error_message(v: &Value) -> String {
    let err = v.get("error").unwrap_or(v);
    let kind = err.get("type").and_then(Value::as_str).unwrap_or("error");
    let msg = err.get("message").and_then(Value::as_str).unwrap_or("");
    if msg.is_empty() {
        format!("{kind}: {v}")
    } else {
        format!("{kind}: {msg}")
    }
}

impl Decoder for AnthropicDecoder {
    fn event(&mut self, ev: SseEvent, out: &mut Vec<StreamEvent>) -> Result<Flow, LlmError> {
        let data = ev.data.trim();
        if data.is_empty() || data == "[DONE]" {
            return Ok(Flow::Continue);
        }
        let v: Value = serde_json::from_str(data)
            .map_err(|e| LlmError::Stream(format!("invalid JSON in stream event: {e}: {}", transport::truncate_str(data, 500))))?;
        let ty = v.get("type").and_then(Value::as_str).or(ev.event.as_deref()).unwrap_or("");
        let index = v.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
        match ty {
            "message_start" => {
                self.started = true;
                if let Some(u) = v.pointer("/message/usage") {
                    self.apply_usage(u);
                    out.push(self.usage_event());
                }
            }
            "content_block_start" => {
                self.started = true;
                // A new block at an already-open index means the previous one never got a stop.
                self.end_block(index, out);
                let cb = v.get("content_block").cloned().unwrap_or(Value::Null);
                let kind = match cb.get("type").and_then(Value::as_str).unwrap_or("") {
                    "text" => {
                        if let Some(t) = cb.get("text").and_then(Value::as_str).filter(|t| !t.is_empty()) {
                            out.push(StreamEvent::TextDelta { index, text: t.to_string() });
                        }
                        BlockKind::Text
                    }
                    "thinking" => {
                        if let Some(t) = cb.get("thinking").and_then(Value::as_str).filter(|t| !t.is_empty()) {
                            out.push(StreamEvent::ReasoningDelta { index, text: t.to_string() });
                        }
                        let signature = cb.get("signature").and_then(Value::as_str).unwrap_or("").to_string();
                        BlockKind::Thinking { signature }
                    }
                    "redacted_thinking" => {
                        BlockKind::Redacted { data: cb.get("data").and_then(Value::as_str).unwrap_or("").to_string() }
                    }
                    "tool_use" => {
                        let id = cb.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                        let id = if id.is_empty() { format!("toolu_ref_{index}") } else { id };
                        let name = cb.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                        let initial = cb.get("input").filter(|i| i.as_object().is_some_and(|o| !o.is_empty())).cloned();
                        self.tool_calls += 1;
                        out.push(StreamEvent::ToolCallStart { index, call_id: id.clone(), name: name.clone() });
                        BlockKind::Tool { id, name, json: String::new(), initial }
                    }
                    _ => BlockKind::Ignored,
                };
                self.blocks.insert(index, kind);
            }
            "content_block_delta" => {
                let delta = v.get("delta").cloned().unwrap_or(Value::Null);
                let dtype = delta.get("type").and_then(Value::as_str).unwrap_or("");
                let s = |k: &str| delta.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                match (self.blocks.get_mut(&index), dtype) {
                    (Some(BlockKind::Text), "text_delta") => {
                        let text = s("text");
                        if !text.is_empty() {
                            out.push(StreamEvent::TextDelta { index, text });
                        }
                    }
                    (Some(BlockKind::Thinking { .. }), "thinking_delta") => {
                        let text = s("thinking");
                        if !text.is_empty() {
                            out.push(StreamEvent::ReasoningDelta { index, text });
                        }
                    }
                    (Some(BlockKind::Thinking { signature }), "signature_delta") => signature.push_str(&s("signature")),
                    (Some(BlockKind::Tool { json, .. }), "input_json_delta") => {
                        let part = s("partial_json");
                        if !part.is_empty() {
                            json.push_str(&part);
                            out.push(StreamEvent::ToolCallDelta { index, json: part });
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => self.end_block(index, out),
            "message_delta" => {
                if let Some(r) = v.pointer("/delta/stop_reason").and_then(Value::as_str) {
                    self.stop_reason = Some(r.to_string());
                }
                if let Some(u) = v.get("usage") {
                    if self.apply_usage(u) {
                        out.push(self.usage_event());
                    }
                }
            }
            "message_stop" => {
                self.close_all(out);
                if let Some(u) = v.pointer("/message/usage").or_else(|| v.get("usage")) {
                    if self.apply_usage(u) {
                        out.push(self.usage_event());
                    }
                }
                out.push(StreamEvent::Finish { reason: self.finish_reason() });
                return Ok(Flow::Done);
            }
            "error" => return Err(LlmError::Stream(error_message(&v))),
            _ => {} // ping, unknown
        }
        Ok(Flow::Continue)
    }

    fn eof(&mut self, _out: &mut Vec<StreamEvent>) -> Result<(), LlmError> {
        Err(LlmError::Stream(if self.started {
            "stream ended before message_stop".into()
        } else {
            "stream ended without any events".into()
        }))
    }
}
