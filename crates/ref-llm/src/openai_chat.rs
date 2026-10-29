//! OpenAI Chat Completions API (`POST {base_url}/chat/completions`), incl. compatible providers.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use crate::convert::{self, AsstBlock, IdMap, Turn, UserBlock};
use crate::sse::SseEvent;
use crate::transport::{self, Decoder, Flow, Headers};
use crate::{EventStream, FinishReason, LlmError, ModelSpec, Request, StreamEvent};

/// Option keys handled explicitly; everything else is forwarded to the body verbatim.
const KNOWN_OPTIONS: &[&str] = &[
    "reasoningEffort",
    "reasoningSummary",
    "textVerbosity",
    "include",
    "store",
    "thinking",
    "effort",
    "promptCacheKey",
    "setCacheKey",
    "serviceTier",
    "parallelToolCalls",
];

const REASONING_FIELDS: &[&str] = &["reasoning_content", "reasoning", "reasoning_text"];
const TOOL_IMAGES_TEXT: &str = "Attached image(s) from tool result:";

pub(crate) async fn stream(http: &reqwest::Client, model: &ModelSpec, req: &Request) -> Result<EventStream, LlmError> {
    let body = build_body(model, req);
    let h = build_headers(model, req)?;
    let url = transport::endpoint(model, "chat/completions")?;
    transport::post_sse(http, model, &url, h, &body, ChatDecoder::new(model)).await
}

pub(crate) fn build_headers(model: &ModelSpec, req: &Request) -> Result<Headers, LlmError> {
    let mut h = Headers::new();
    if let Some(key) = model.api_key.as_deref().filter(|k| !k.is_empty()) {
        h.set("authorization", &format!("Bearer {key}"));
    }
    if !req.session_id.is_empty() {
        // Session affinity for gateways / load balancers (keeps the prompt cache warm).
        h.set("x-session-affinity", &req.session_id);
        if is_openrouter(model) {
            h.set("x-session-id", &req.session_id);
        }
    }
    h.apply_user(model, &[])?;
    Ok(h)
}

fn is_openrouter(model: &ModelSpec) -> bool {
    let host = transport::host_of(&model.base_url);
    host == "openrouter.ai" || host.ends_with(".openrouter.ai")
}

fn is_openai(model: &ModelSpec) -> bool {
    let host = transport::host_of(&model.base_url);
    host == "api.openai.com" || host.ends_with(".openai.azure.com")
}

/// Z.ai / Zhipu GLM endpoints (`api.z.ai`, `open.bigmodel.cn`).
fn is_zai(model: &ModelSpec) -> bool {
    let host = transport::host_of(&model.base_url);
    host == "z.ai" || host.ends_with(".z.ai") || host == "bigmodel.cn" || host.ends_with(".bigmodel.cn")
}

/// Models whose API requires `reasoning_content` on every replayed assistant message while
/// thinking is on (DeepSeek: 400 "reasoning_content must be passed back"; Kimi K2.x thinking
/// models likewise). Matched by model id so it also works through gateways.
pub(crate) fn requires_reasoning_content(model: &ModelSpec) -> bool {
    let id = model.api_model.to_ascii_lowercase();
    let host = transport::host_of(&model.base_url);
    id.contains("deepseek")
        || id.contains("kimi")
        || host.ends_with("deepseek.com")
        || host.ends_with("moonshot.cn")
        || host.ends_with("moonshot.ai")
}

/// GLM model that supports `tool_stream` (GLM-4.6 and newer; not 4.5).
fn zai_tool_stream(model: &ModelSpec) -> bool {
    let id = model.api_model.to_ascii_lowercase();
    is_zai(model) && id.contains("glm") && !id.contains("glm-4.5") && !id.contains("glm-4-")
}

fn opt_str<'a>(model: &'a ModelSpec, key: &str) -> Option<&'a str> {
    model.options.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// `prompt_cache_key`: explicit `promptCacheKey` option, else the session id when the endpoint is
/// known to use it for cache routing (OpenAI, OpenRouter) or `setCacheKey: true` is configured.
/// Not sent to other compatible providers (strict ones reject unknown parameters).
fn prompt_cache_key(model: &ModelSpec, req: &Request) -> Option<String> {
    if let Some(k) = opt_str(model, "promptCacheKey") {
        return Some(k.to_string());
    }
    let set = model.options.get("setCacheKey").and_then(Value::as_bool);
    let wanted = set.unwrap_or_else(|| is_openai(model) || is_openrouter(model));
    (wanted && !req.session_id.is_empty()).then(|| req.session_id.clone())
}

pub(crate) fn build_body(model: &ModelSpec, req: &Request) -> Value {
    let mut body = Map::new();
    body.insert("model".into(), json!(model.api_model));
    body.insert("messages".into(), Value::Array(build_messages(model, req)));
    body.insert("stream".into(), json!(true));
    body.insert("stream_options".into(), json!({"include_usage": true}));
    if !req.tools.is_empty() {
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": t.parameters}}))
            .collect();
        body.insert("tools".into(), Value::Array(tools));
        if req.tool_choice_none {
            body.insert("tool_choice".into(), json!("none"));
        }
        if let Some(p) = model.options.get("parallelToolCalls") {
            body.insert("parallel_tool_calls".into(), p.clone());
        }
        if zai_tool_stream(model) {
            // Stream tool-call arguments incrementally instead of in one final chunk.
            body.insert("tool_stream".into(), json!(true));
        }
    } else if req.messages.iter().any(|m| m.parts.iter().any(|p| matches!(p, ref_proto::Part::Tool(_)))) {
        // Tool history without active tools: some proxies (LiteLLM → Anthropic, Bedrock) need
        // the field present.
        body.insert("tools".into(), json!([]));
    }
    let max = transport::fit_max_tokens(model, req);
    if max > 0 {
        let field = if is_openai(model) { "max_completion_tokens" } else { "max_tokens" };
        body.insert(field.into(), json!(max));
    }
    if let Some(t) = model.temperature {
        body.insert("temperature".into(), json!(t));
    }
    if let Some(p) = model.top_p {
        body.insert("top_p".into(), json!(p));
    }
    if let Some(effort) = opt_str(model, "reasoningEffort").or_else(|| opt_str(model, "effort")) {
        if is_zai(model) && !model.options.contains_key("thinking") && !matches!(effort, "none" | "minimal") {
            // GLM: an effort level implies thinking on; keep earlier reasoning in context
            // ("preserved thinking") so replayed reasoning_content is used (opencode spelling).
            body.insert("thinking".into(), json!({"type": "enabled", "clear_thinking": false}));
        }
        if is_openrouter(model) {
            body.insert("reasoning".into(), json!({"effort": effort}));
        } else {
            body.insert("reasoning_effort".into(), json!(effort));
        }
    }
    // `thinking` (DeepSeek / Kimi / GLM style `{"type": "enabled"}`): camelCase keys from
    // opencode-style configs are converted to the wire names.
    match model.options.get("thinking") {
        Some(Value::Object(t)) => {
            let mut out = Map::new();
            for (k, v) in t {
                let key = match k.as_str() {
                    "budgetTokens" => "budget_tokens",
                    "clearThinking" => "clear_thinking",
                    other => other,
                };
                out.insert(key.to_string(), v.clone());
            }
            body.insert("thinking".into(), Value::Object(out));
        }
        Some(v) if !v.is_null() => {
            body.insert("thinking".into(), v.clone());
        }
        _ => {}
    }
    if let Some(v) = opt_str(model, "textVerbosity") {
        body.insert("verbosity".into(), json!(v));
    }
    if let Some(v) = model.options.get("serviceTier") {
        body.insert("service_tier".into(), v.clone());
    }
    if let Some(k) = prompt_cache_key(model, req) {
        body.insert("prompt_cache_key".into(), json!(k.chars().take(64).collect::<String>()));
    }
    // `store` only exists on OpenAI / Azure; strict compatible providers reject it.
    if let Some(v) = model.options.get("store").filter(|_| is_openai(model)) {
        body.insert("store".into(), v.clone());
    }
    for (k, v) in &model.options {
        if !KNOWN_OPTIONS.contains(&k.as_str()) {
            body.insert(k.clone(), v.clone());
        }
    }
    transport::merge_json(&mut body, &model.extra_body);
    Value::Object(body)
}

fn image_url(img: &convert::Image) -> Value {
    json!({"type": "image_url", "image_url": {"url": img.data_url()}})
}

fn build_messages(model: &ModelSpec, req: &Request) -> Vec<Value> {
    let mut msgs = Vec::new();
    let system = req.system.iter().filter(|s| !s.trim().is_empty()).cloned().collect::<Vec<_>>().join("\n\n");
    if !system.is_empty() {
        msgs.push(json!({"role": "system", "content": system}));
    }
    // OpenAI rejects tool call ids longer than 40 characters.
    let mut ids = IdMap::new(40);
    let require_reasoning = requires_reasoning_content(model);
    for turn in convert::flatten(&req.messages, model) {
        match turn {
            Turn::User(blocks) => {
                let content: Value = if blocks.iter().all(|b| matches!(b, UserBlock::Text(_))) {
                    let text: Vec<&str> = blocks
                        .iter()
                        .filter_map(|b| match b {
                            UserBlock::Text(t) => Some(t.as_str()),
                            UserBlock::Image(_) => None,
                        })
                        .collect();
                    json!(text.join("\n\n"))
                } else {
                    Value::Array(
                        blocks
                            .iter()
                            .map(|b| match b {
                                UserBlock::Text(t) => json!({"type": "text", "text": t}),
                                UserBlock::Image(img) => image_url(img),
                            })
                            .collect(),
                    )
                };
                msgs.push(json!({"role": "user", "content": content}));
            }
            Turn::Assistant(blocks) => {
                if let Some(m) = assistant_message(blocks, &mut ids, require_reasoning) {
                    msgs.push(m);
                }
            }
            Turn::ToolResults(results) => {
                let mut images = Vec::new();
                for r in &results {
                    msgs.push(json!({"role": "tool", "tool_call_id": ids.get(&r.call_id), "content": r.text}));
                    images.extend(r.images.iter().map(image_url));
                }
                if !images.is_empty() {
                    let mut content = vec![json!({"type": "text", "text": TOOL_IMAGES_TEXT})];
                    content.extend(images);
                    msgs.push(json!({"role": "user", "content": content}));
                }
            }
        }
    }
    msgs
}

/// `require_reasoning`: always send `reasoning_content` (empty when there is none to replay).
fn assistant_message(blocks: Vec<AsstBlock>, ids: &mut IdMap, require_reasoning: bool) -> Option<Value> {
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    // field name → accumulated reasoning text (same-model reasoning only).
    let mut reasoning: BTreeMap<String, String> = BTreeMap::new();
    let mut details: Vec<Value> = Vec::new();
    for b in blocks {
        match b {
            AsstBlock::Reasoning { text: r, meta: Some(meta) } => {
                if let Some(field) = meta.get("field").and_then(Value::as_str) {
                    if REASONING_FIELDS.contains(&field) {
                        let acc = reasoning.entry(field.to_string()).or_default();
                        acc.push_str(&r);
                    }
                }
                if let Some(Value::Array(d)) = meta.get("details") {
                    details.extend(d.iter().cloned());
                }
            }
            AsstBlock::Reasoning { meta: None, .. } => {}
            AsstBlock::Text { text: t, .. } => {
                if !text.is_empty() {
                    text.push_str("\n\n");
                }
                text.push_str(&t);
            }
            AsstBlock::ToolCall { call_id, name, input, .. } => {
                let arguments = match &input {
                    Value::String(raw) => raw.clone(),
                    v => v.to_string(),
                };
                tool_calls.push(json!({"id": ids.get(&call_id), "type": "function",
                                       "function": {"name": name, "arguments": arguments}}));
            }
        }
    }
    if text.trim().is_empty() && tool_calls.is_empty() {
        return None;
    }
    let mut m = Map::new();
    m.insert("role".into(), json!("assistant"));
    m.insert("content".into(), if text.is_empty() { Value::Null } else { json!(text) });
    for (field, r) in reasoning {
        m.insert(field, json!(r));
    }
    if require_reasoning && !m.contains_key("reasoning_content") {
        m.insert("reasoning_content".into(), json!(""));
    }
    if !details.is_empty() {
        m.insert("reasoning_details".into(), Value::Array(details));
    }
    if !tool_calls.is_empty() {
        m.insert("tool_calls".into(), Value::Array(tool_calls));
    }
    Some(Value::Object(m))
}

// ───────────────────────────── stream decoding ─────────────────────────────

// Block indices within one response: text and reasoning are single slots, tool calls get
// `TOOL_BASE + provider index`.
const REASONING_INDEX: usize = 0;
const TEXT_INDEX: usize = 1;
const TOOL_BASE: usize = 2;

#[derive(Debug)]
struct ToolSlot {
    call_id: String,
    name: String,
    args: String,
    started: bool,
    extra: Option<Value>,
}

pub(crate) struct ChatDecoder {
    model: ModelSpec,
    reasoning_open: bool,
    reasoning_field: Option<String>,
    reasoning_details: Vec<Value>,
    text_open: bool,
    /// provider tool index → slot
    tools: BTreeMap<usize, ToolSlot>,
    finish: Option<String>,
    usage: Option<ref_proto::Usage>,
    got_content: bool,
    done: bool,
}

impl ChatDecoder {
    pub(crate) fn new(model: &ModelSpec) -> Self {
        ChatDecoder {
            model: model.clone(),
            reasoning_open: false,
            reasoning_field: None,
            reasoning_details: Vec::new(),
            text_open: false,
            tools: BTreeMap::new(),
            finish: None,
            usage: None,
            got_content: false,
            done: false,
        }
    }

    fn end_reasoning(&mut self, out: &mut Vec<StreamEvent>) {
        if !self.reasoning_open {
            return;
        }
        self.reasoning_open = false;
        let mut fields = Map::new();
        if let Some(f) = self.reasoning_field.take() {
            fields.insert("field".into(), json!(f));
        }
        if !self.reasoning_details.is_empty() {
            let details = merge_reasoning_details(std::mem::take(&mut self.reasoning_details));
            fields.insert("details".into(), Value::Array(details));
        }
        out.push(StreamEvent::ReasoningEnd {
            index: REASONING_INDEX,
            meta: Some(convert::make_meta(&self.model, Value::Object(fields))),
        });
    }

    fn end_all(&mut self, out: &mut Vec<StreamEvent>) {
        self.end_reasoning(out);
        if self.text_open {
            self.text_open = false;
            out.push(StreamEvent::TextEnd { index: TEXT_INDEX, meta: None });
        }
        for (i, slot) in std::mem::take(&mut self.tools) {
            let index = TOOL_BASE + i;
            if !slot.started {
                out.push(StreamEvent::ToolCallStart { index, call_id: slot.call_id.clone(), name: slot.name.clone() });
            }
            let meta = slot.extra.map(|e| convert::make_meta(&self.model, json!({"extra_content": e})));
            out.push(StreamEvent::ToolCallEnd {
                index,
                call_id: slot.call_id,
                name: slot.name,
                input: convert::parse_tool_input(&slot.args),
                meta,
            });
        }
    }

    fn finish(&mut self, out: &mut Vec<StreamEvent>) {
        let had_tools = !self.tools.is_empty();
        self.end_all(out);
        if let Some(u) = self.usage.take() {
            out.push(StreamEvent::Usage(crate::with_cost(&self.model, u)));
        }
        let reason = match self.finish.as_deref() {
            None | Some("stop") | Some("end") | Some("end_turn") | Some("eos") => {
                if had_tools {
                    FinishReason::ToolUse
                } else {
                    FinishReason::Stop
                }
            }
            Some("tool_calls") | Some("function_call") | Some("tool_use") => FinishReason::ToolUse,
            Some("length") | Some("max_tokens") => FinishReason::Length,
            Some(_) => FinishReason::Error,
        };
        out.push(StreamEvent::Finish { reason });
        self.done = true;
    }

    fn parse_usage(&mut self, u: &Value) {
        if !u.is_object() {
            return;
        }
        let n = |p: &str| u.pointer(p).and_then(Value::as_u64);
        let prompt = n("/prompt_tokens").unwrap_or(0);
        let cache_read = n("/prompt_tokens_details/cached_tokens")
            .or_else(|| n("/prompt_cache_hit_tokens"))
            .or_else(|| n("/cached_tokens"))
            .unwrap_or(0);
        let cache_write = n("/prompt_tokens_details/cache_write_tokens").unwrap_or(0);
        self.usage = Some(ref_proto::Usage {
            input: prompt.saturating_sub(cache_read + cache_write),
            output: n("/completion_tokens").unwrap_or(0),
            cache_read,
            cache_write,
            reasoning: n("/completion_tokens_details/reasoning_tokens").unwrap_or(0),
            cost: 0.0,
        });
    }

    fn handle_delta(&mut self, delta: &Value, out: &mut Vec<StreamEvent>) {
        // Reasoning first: first non-empty field wins (some providers duplicate it).
        let reasoning = REASONING_FIELDS
            .iter()
            .find_map(|f| delta.get(*f).and_then(Value::as_str).filter(|s| !s.is_empty()).map(|s| (*f, s)));
        let details = delta.get("reasoning_details").and_then(Value::as_array).filter(|d| !d.is_empty());
        if reasoning.is_some() || details.is_some() {
            if !self.reasoning_open {
                self.reasoning_open = true;
            }
            if let Some((field, text)) = reasoning {
                self.reasoning_field.get_or_insert_with(|| field.to_string());
                out.push(StreamEvent::ReasoningDelta { index: REASONING_INDEX, text: text.to_string() });
            } else if let Some(d) = details {
                // reasoning_details only (no plain field): surface its text for display.
                for entry in d {
                    let t = entry.get("text").or_else(|| entry.get("summary")).and_then(Value::as_str).unwrap_or("");
                    if !t.is_empty() {
                        out.push(StreamEvent::ReasoningDelta { index: REASONING_INDEX, text: t.to_string() });
                    }
                }
            }
            if let Some(d) = details {
                self.reasoning_details.extend(d.iter().cloned());
            }
        }

        if let Some(text) = delta.get("content").and_then(Value::as_str).filter(|s| !s.is_empty()) {
            self.end_reasoning(out);
            self.got_content = true;
            self.text_open = true;
            out.push(StreamEvent::TextDelta { index: TEXT_INDEX, text: text.to_string() });
        }

        if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
            self.end_reasoning(out);
            for (pos, tc) in calls.iter().enumerate() {
                self.handle_tool_delta(pos, tc, out);
            }
        }
    }

    fn handle_tool_delta(&mut self, pos: usize, tc: &Value, out: &mut Vec<StreamEvent>) {
        self.got_content = true;
        let id = tc.get("id").and_then(Value::as_str).filter(|s| !s.is_empty());
        let key = match tc.get("index").and_then(Value::as_u64) {
            Some(i) => i as usize,
            None => {
                // No index: match by id, else a new call when an id is present, else the last one.
                let by_id = id.and_then(|id| self.tools.iter().find(|(_, s)| s.call_id == id).map(|(k, _)| *k));
                match (by_id, id) {
                    (Some(k), _) => k,
                    (None, Some(_)) => self.tools.keys().next_back().map_or(pos, |k| k + 1),
                    (None, None) => self.tools.keys().next_back().copied().unwrap_or(pos),
                }
            }
        };
        let slot = self.tools.entry(key).or_insert_with(|| ToolSlot {
            call_id: String::new(),
            name: String::new(),
            args: String::new(),
            started: false,
            extra: None,
        });
        if let Some(id) = id {
            if slot.call_id.is_empty() {
                slot.call_id = id.to_string();
            }
        }
        if let Some(name) = tc.pointer("/function/name").and_then(Value::as_str).filter(|s| !s.is_empty()) {
            if slot.name.is_empty() {
                slot.name = name.to_string();
            }
        }
        if let Some(extra) = tc.get("extra_content").filter(|e| !e.is_null()) {
            slot.extra = Some(extra.clone());
        }
        let index = TOOL_BASE + key;
        if !slot.started && !slot.name.is_empty() {
            if slot.call_id.is_empty() {
                slot.call_id = format!("call_ref_{key}");
            }
            slot.started = true;
            out.push(StreamEvent::ToolCallStart { index, call_id: slot.call_id.clone(), name: slot.name.clone() });
            if !slot.args.is_empty() {
                out.push(StreamEvent::ToolCallDelta { index, json: slot.args.clone() });
            }
        }
        if let Some(args) = tc.pointer("/function/arguments").and_then(Value::as_str).filter(|s| !s.is_empty()) {
            slot.args.push_str(args);
            if slot.started {
                out.push(StreamEvent::ToolCallDelta { index, json: args.to_string() });
            }
        }
    }
}

/// Merge consecutive streamed `reasoning.text` / `reasoning.summary` fragments with the same id
/// into single entries (encrypted entries stay discrete) and drop the streaming `index`.
fn merge_reasoning_details(details: Vec<Value>) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for mut d in details {
        if let Some(o) = d.as_object_mut() {
            o.remove("index");
        }
        let ty = d.get("type").and_then(Value::as_str).unwrap_or("").to_string();
        let field = match ty.as_str() {
            "reasoning.text" => "text",
            "reasoning.summary" => "summary",
            _ => {
                out.push(d);
                continue;
            }
        };
        if let Some(prev) = out.last_mut() {
            let same = prev.get("type") == d.get("type")
                && prev.get("id").filter(|v| !v.is_null()) == d.get("id").filter(|v| !v.is_null());
            if same {
                let add = d.get(field).and_then(Value::as_str).unwrap_or("").to_string();
                let cur = prev.get(field).and_then(Value::as_str).unwrap_or("").to_string();
                prev[field] = json!(cur + &add);
                if let Some(o) = d.as_object() {
                    for (k, v) in o {
                        if k != field && !v.is_null() {
                            prev[k.as_str()] = v.clone();
                        }
                    }
                }
                continue;
            }
        }
        out.push(d);
    }
    out
}

fn chunk_error(v: &Value) -> Option<String> {
    let err = v.get("error").filter(|e| !e.is_null())?;
    let msg = err.get("message").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| err.to_string());
    let code = err.get("code").or_else(|| err.get("type")).filter(|c| !c.is_null());
    Some(match code {
        Some(Value::String(c)) => format!("{c}: {msg}"),
        Some(c) => format!("{c}: {msg}"),
        None => msg,
    })
}

impl Decoder for ChatDecoder {
    fn event(&mut self, ev: SseEvent, out: &mut Vec<StreamEvent>) -> Result<Flow, LlmError> {
        let data = ev.data.trim();
        if data.is_empty() {
            return Ok(Flow::Continue);
        }
        if data == "[DONE]" {
            self.finish(out);
            return Ok(Flow::Done);
        }
        let v: Value = serde_json::from_str(data)
            .map_err(|e| LlmError::Stream(format!("invalid JSON in stream chunk: {e}: {}", transport::truncate_str(data, 500))))?;
        if let Some(msg) = chunk_error(&v) {
            return Err(LlmError::Stream(msg));
        }
        if let Some(u) = v.get("usage") {
            self.parse_usage(u);
        }
        if let Some(choice) = v.get("choices").and_then(Value::as_array).and_then(|c| c.first()) {
            if let Some(u) = choice.get("usage") {
                self.parse_usage(u);
            }
            if let Some(delta) = choice.get("delta").or_else(|| choice.get("message")) {
                self.handle_delta(delta, out);
            }
            if let Some(r) = choice.get("finish_reason").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                self.finish = Some(r.to_string());
                if r == "error" || r == "network_error" {
                    return Err(LlmError::Stream(format!("provider finished with {r}")));
                }
            }
        }
        Ok(Flow::Continue)
    }

    fn eof(&mut self, out: &mut Vec<StreamEvent>) -> Result<(), LlmError> {
        if self.done {
            return Ok(());
        }
        // Some compatible providers close the stream without `[DONE]`; accept it if the response
        // clearly completed (finish_reason seen) or produced content.
        if self.finish.is_some() || self.got_content {
            self.finish(out);
            Ok(())
        } else {
            Err(LlmError::Stream("stream ended before any content".into()))
        }
    }
}
