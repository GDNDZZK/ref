//! OpenAI Responses API (`POST {base_url}/responses`).

use std::collections::{BTreeMap, HashMap};

use serde_json::{json, Map, Value};

use crate::convert::{self, AsstBlock, IdMap, ToolResult, Turn, UserBlock};
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

pub(crate) async fn stream(http: &reqwest::Client, model: &ModelSpec, req: &Request) -> Result<EventStream, LlmError> {
    let body = build_body(model, req);
    let h = build_headers(model, req)?;
    let url = transport::endpoint(model, "responses")?;
    transport::post_sse(http, model, &url, h, &body, ResponsesDecoder::new(model)).await
}

pub(crate) fn build_headers(model: &ModelSpec, req: &Request) -> Result<Headers, LlmError> {
    let mut h = Headers::new();
    if let Some(key) = model.api_key.as_deref().filter(|k| !k.is_empty()) {
        h.set("authorization", &format!("Bearer {key}"));
    }
    if !req.session_id.is_empty() {
        h.set("session_id", &req.session_id);
        h.set("x-session-affinity", &req.session_id);
    }
    h.apply_user(model, &[])?;
    Ok(h)
}

fn store_enabled(model: &ModelSpec) -> bool {
    model.options.get("store").and_then(Value::as_bool).unwrap_or(false)
}

fn opt_str<'a>(model: &'a ModelSpec, key: &str) -> Option<&'a str> {
    model.options.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// Clamp to at most `max` characters (not bytes).
fn clamp_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

pub(crate) fn build_body(model: &ModelSpec, req: &Request) -> Value {
    let store = store_enabled(model);
    let mut body = Map::new();
    body.insert("model".into(), json!(model.api_model));
    body.insert("input".into(), Value::Array(build_input(model, req, store)));
    body.insert("stream".into(), json!(true));
    body.insert("store".into(), json!(store));

    match model.options.get("include") {
        Some(v) => {
            body.insert("include".into(), v.clone());
        }
        None if model.reasoning && !store => {
            body.insert("include".into(), json!(["reasoning.encrypted_content"]));
        }
        None => {}
    }

    let cache_key = opt_str(model, "promptCacheKey").map(str::to_string).or_else(|| {
        let enabled = model.options.get("setCacheKey").and_then(Value::as_bool).unwrap_or(true);
        (enabled && !req.session_id.is_empty()).then(|| req.session_id.clone())
    });
    if let Some(k) = cache_key {
        body.insert("prompt_cache_key".into(), json!(clamp_chars(&k, 64)));
    }

    let effort = opt_str(model, "reasoningEffort").or_else(|| opt_str(model, "effort"));
    let summary = opt_str(model, "reasoningSummary");
    if effort.is_some() || summary.is_some() || model.reasoning {
        let mut r = Map::new();
        if let Some(e) = effort {
            r.insert("effort".into(), json!(e));
        }
        match summary {
            Some(s) => {
                r.insert("summary".into(), json!(s));
            }
            None if model.reasoning && effort != Some("none") => {
                r.insert("summary".into(), json!("auto"));
            }
            None => {}
        }
        if !r.is_empty() {
            body.insert("reasoning".into(), Value::Object(r));
        }
    }
    if let Some(v) = opt_str(model, "textVerbosity") {
        body.insert("text".into(), json!({"verbosity": v}));
    }
    let max = transport::fit_max_tokens(model, req);
    if max > 0 {
        body.insert("max_output_tokens".into(), json!(max.max(16)));
    }
    // Reasoning models (gpt-5.x, o-series) reject sampling parameters unless reasoning is off.
    let sampling_ok = !model.reasoning || effort == Some("none");
    if let Some(t) = model.temperature.filter(|_| sampling_ok) {
        body.insert("temperature".into(), json!(t));
    }
    if let Some(p) = model.top_p.filter(|_| sampling_ok) {
        body.insert("top_p".into(), json!(p));
    }
    if let Some(t) = model.options.get("serviceTier") {
        body.insert("service_tier".into(), t.clone());
    }
    if !req.tools.is_empty() {
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                json!({"type": "function", "name": t.name, "description": t.description,
                       "parameters": t.parameters, "strict": false})
            })
            .collect();
        body.insert("tools".into(), Value::Array(tools));
        body.insert("tool_choice".into(), json!(if req.tool_choice_none { "none" } else { "auto" }));
        let parallel = model.options.get("parallelToolCalls").cloned().unwrap_or(json!(true));
        body.insert("parallel_tool_calls".into(), parallel);
    }
    for (k, v) in &model.options {
        if !KNOWN_OPTIONS.contains(&k.as_str()) {
            body.insert(k.clone(), v.clone());
        }
    }
    transport::merge_json(&mut body, &model.extra_body);
    Value::Object(body)
}

fn input_image(img: &convert::Image) -> Value {
    json!({"type": "input_image", "detail": "auto", "image_url": img.data_url()})
}

fn build_input(model: &ModelSpec, req: &Request, store: bool) -> Vec<Value> {
    let mut items = Vec::new();
    let system = req.system.iter().filter(|s| !s.trim().is_empty()).cloned().collect::<Vec<_>>().join("\n\n");
    if !system.is_empty() {
        let role = if model.reasoning { "developer" } else { "system" };
        items.push(json!({"role": role, "content": system}));
    }
    let mut ids = IdMap::new(64);
    for turn in convert::flatten(&req.messages, model) {
        match turn {
            Turn::User(blocks) => {
                let content: Vec<Value> = blocks
                    .iter()
                    .map(|b| match b {
                        UserBlock::Text(t) => json!({"type": "input_text", "text": t}),
                        UserBlock::Image(img) => input_image(img),
                    })
                    .collect();
                items.push(json!({"role": "user", "content": content}));
            }
            Turn::Assistant(blocks) => {
                let had_reasoning = blocks.iter().any(|b| matches!(b, AsstBlock::Reasoning { meta: Some(_), .. }));
                let mut out: Vec<Value> = blocks.into_iter().filter_map(|b| assistant_item(b, store, &mut ids)).collect();
                // Output items that were produced together with a reasoning item must not be
                // replayed with their ids when that reasoning item is gone (the API rejects a
                // `fc_`/`msg_` item "provided without its required reasoning item").
                let has_reasoning = out.iter().any(|i| i.get("type").and_then(Value::as_str) == Some("reasoning"));
                if had_reasoning && !has_reasoning {
                    for item in &mut out {
                        match item.get("type").and_then(Value::as_str) {
                            Some("message") => {
                                let text = item.pointer("/content/0/text").cloned().unwrap_or(json!(""));
                                *item = json!({"role": "assistant", "content": text});
                            }
                            _ => {
                                if let Some(o) = item.as_object_mut() {
                                    o.remove("id");
                                }
                            }
                        }
                    }
                }
                // A reasoning item must be followed by the item it produced (e.g. aborted after
                // thinking); a dangling one is rejected by the API.
                while out.last().is_some_and(|i| i.get("type").and_then(Value::as_str) == Some("reasoning")) {
                    out.pop();
                }
                items.extend(out);
            }
            Turn::ToolResults(results) => {
                for r in &results {
                    items.push(tool_output(r, &mut ids));
                }
            }
        }
    }
    items
}

fn meta_id<'a>(meta: &'a Option<Value>, prefix: &str) -> Option<&'a str> {
    meta.as_ref()?.get("id")?.as_str().filter(|id| id.starts_with(prefix) && id.len() <= 64)
}

fn assistant_item(b: AsstBlock, store: bool, ids: &mut IdMap) -> Option<Value> {
    match b {
        AsstBlock::Reasoning { meta, .. } => {
            let item = meta?.get("item")?.clone();
            if item.get("type").and_then(Value::as_str) != Some("reasoning") {
                return None;
            }
            // Without server-side storage the item is only resolvable through its encrypted content.
            if !store && item.get("encrypted_content").and_then(Value::as_str).is_none_or(str::is_empty) {
                return None;
            }
            Some(item)
        }
        AsstBlock::Text { text, meta } => {
            if text.trim().is_empty() {
                return None;
            }
            match meta_id(&meta, "msg_") {
                Some(id) => {
                    let mut item = json!({
                        "type": "message", "role": "assistant", "status": "completed", "id": id,
                        "content": [{"type": "output_text", "text": text, "annotations": []}],
                    });
                    if let Some(phase) = meta.as_ref().and_then(|m| m.get("phase")).filter(|p| p.is_string()) {
                        item["phase"] = phase.clone();
                    }
                    Some(item)
                }
                None => Some(json!({"role": "assistant", "content": text})),
            }
        }
        AsstBlock::ToolCall { call_id, name, input, meta } => {
            let arguments = match &input {
                Value::String(raw) => raw.clone(),
                v => v.to_string(),
            };
            let mut item = json!({"type": "function_call", "call_id": ids.get(&call_id), "name": name, "arguments": arguments});
            if let Some(id) = meta_id(&meta, "fc_") {
                item["id"] = json!(id);
            }
            Some(item)
        }
    }
}

fn tool_output(r: &ToolResult, ids: &mut IdMap) -> Value {
    let output = if r.images.is_empty() {
        json!(r.text)
    } else {
        let mut parts = vec![json!({"type": "input_text", "text": r.text})];
        parts.extend(r.images.iter().map(input_image));
        Value::Array(parts)
    };
    json!({"type": "function_call_output", "call_id": ids.get(&r.call_id), "output": output})
}

// ───────────────────────────── stream decoding ─────────────────────────────

#[derive(Debug)]
enum Slot {
    Text,
    Reasoning { parts_seen: Option<u64> },
    Tool { call_id: String, name: String, args: String },
}

pub(crate) struct ResponsesDecoder {
    model: ModelSpec,
    slots: HashMap<usize, Slot>,
    /// Reasoning items finished without `encrypted_content` (Azure); completed from the terminal
    /// response before their `ReasoningEnd` is emitted.
    pending_reasoning: BTreeMap<usize, Value>,
    tool_calls: usize,
    started: bool,
}

impl ResponsesDecoder {
    pub(crate) fn new(model: &ModelSpec) -> Self {
        ResponsesDecoder {
            model: model.clone(),
            slots: HashMap::new(),
            pending_reasoning: BTreeMap::new(),
            tool_calls: 0,
            started: false,
        }
    }

    fn open(&mut self, index: usize, item: &Value, out: &mut Vec<StreamEvent>) {
        if self.slots.contains_key(&index) {
            return;
        }
        let slot = match item.get("type").and_then(Value::as_str).unwrap_or("") {
            "message" => Slot::Text,
            "reasoning" => Slot::Reasoning { parts_seen: None },
            "function_call" => {
                let call_id = item.get("call_id").and_then(Value::as_str).unwrap_or("").to_string();
                let call_id = if call_id.is_empty() { format!("call_ref_{index}") } else { call_id };
                let name = item.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                let args = item.get("arguments").and_then(Value::as_str).unwrap_or("").to_string();
                self.tool_calls += 1;
                out.push(StreamEvent::ToolCallStart { index, call_id: call_id.clone(), name: name.clone() });
                if !args.is_empty() {
                    out.push(StreamEvent::ToolCallDelta { index, json: args.clone() });
                }
                Slot::Tool { call_id, name, args }
            }
            _ => return,
        };
        self.slots.insert(index, slot);
    }

    fn slot_for_delta(&mut self, index: usize, kind: &str) -> bool {
        match self.slots.get(&index) {
            Some(Slot::Text) => kind == "text",
            Some(Slot::Reasoning { .. }) => kind == "reasoning",
            Some(Slot::Tool { .. }) => kind == "tool",
            None => {
                // Delta without a preceding output_item.added: open implicitly.
                let slot = match kind {
                    "text" => Slot::Text,
                    "reasoning" => Slot::Reasoning { parts_seen: None },
                    _ => return false,
                };
                self.slots.insert(index, slot);
                true
            }
        }
    }

    fn done_item(&mut self, index: usize, item: &Value, out: &mut Vec<StreamEvent>) {
        self.open(index, item, out);
        let Some(slot) = self.slots.remove(&index) else { return };
        match slot {
            Slot::Text => {
                let id = item.get("id").and_then(Value::as_str).unwrap_or("");
                let mut fields = json!({"id": id});
                if let Some(phase) = item.get("phase").filter(|p| p.is_string()) {
                    fields["phase"] = phase.clone();
                }
                out.push(StreamEvent::TextEnd { index, meta: Some(convert::make_meta(&self.model, fields)) });
            }
            Slot::Reasoning { .. } => {
                let has_encrypted = item.get("encrypted_content").and_then(Value::as_str).is_some_and(|s| !s.is_empty());
                if has_encrypted || store_enabled(&self.model) {
                    out.push(self.reasoning_end(index, item.clone()));
                } else {
                    self.pending_reasoning.insert(index, item.clone());
                }
            }
            Slot::Tool { call_id, name, args } => {
                let final_args = item.get("arguments").and_then(Value::as_str).filter(|a| !a.is_empty());
                let args = final_args.map(str::to_string).unwrap_or(args);
                let meta = item
                    .get("id")
                    .and_then(Value::as_str)
                    .map(|id| convert::make_meta(&self.model, json!({"id": id})));
                out.push(StreamEvent::ToolCallEnd { index, call_id, name, input: convert::parse_tool_input(&args), meta });
            }
        }
    }

    fn reasoning_end(&self, index: usize, item: Value) -> StreamEvent {
        StreamEvent::ReasoningEnd { index, meta: Some(convert::make_meta(&self.model, json!({"item": item}))) }
    }

    /// Close everything still open at a terminal event.
    fn close_all(&mut self, response: &Value, out: &mut Vec<StreamEvent>) {
        let output = response.get("output").and_then(Value::as_array).cloned().unwrap_or_default();
        let mut open: Vec<usize> = self.slots.keys().copied().collect();
        open.sort_unstable();
        for index in open {
            let item = output.get(index).cloned().unwrap_or(Value::Null);
            match self.slots.get(&index) {
                Some(Slot::Text) => {
                    self.slots.remove(&index);
                    out.push(StreamEvent::TextEnd { index, meta: None });
                }
                Some(Slot::Reasoning { .. }) if item.get("type").and_then(Value::as_str) == Some("reasoning") => {
                    self.done_item(index, &item, out)
                }
                Some(Slot::Reasoning { .. }) => {
                    self.slots.remove(&index);
                    out.push(StreamEvent::ReasoningEnd { index, meta: None });
                }
                Some(Slot::Tool { .. }) => self.done_item(index, &item, out),
                None => {}
            }
        }
        for (index, mut item) in std::mem::take(&mut self.pending_reasoning) {
            let id = item.get("id").and_then(Value::as_str).unwrap_or("").to_string();
            let found = output.iter().find(|o| {
                o.get("type").and_then(Value::as_str) == Some("reasoning")
                    && o.get("id").and_then(Value::as_str) == Some(id.as_str())
            });
            if let Some(enc) = found.and_then(|o| o.get("encrypted_content")).filter(|e| e.is_string()) {
                item["encrypted_content"] = enc.clone();
            }
            out.push(self.reasoning_end(index, item));
        }
    }

    fn usage(&self, response: &Value) -> Option<StreamEvent> {
        let u = response.get("usage").filter(|u| u.is_object())?;
        let n = |p: &str| u.pointer(p).and_then(Value::as_u64).unwrap_or(0);
        let cached = n("/input_tokens_details/cached_tokens");
        let cache_write = n("/input_tokens_details/cache_write_tokens");
        let usage = ref_proto::Usage {
            input: n("/input_tokens").saturating_sub(cached + cache_write),
            output: n("/output_tokens"),
            cache_read: cached,
            cache_write,
            reasoning: n("/output_tokens_details/reasoning_tokens"),
            cost: 0.0,
        };
        Some(StreamEvent::Usage(crate::with_cost(&self.model, usage)))
    }
}

fn failure_message(v: &Value) -> String {
    let err = v.pointer("/response/error").or_else(|| v.get("error")).filter(|e| e.is_object()).unwrap_or(v);
    let code = err.get("code").and_then(Value::as_str).or_else(|| err.get("type").and_then(Value::as_str));
    let msg = err.get("message").and_then(Value::as_str);
    match (code, msg) {
        (Some(c), Some(m)) => format!("{c}: {m}"),
        (None, Some(m)) => m.to_string(),
        (Some(c), None) => c.to_string(),
        (None, None) => match v.pointer("/response/incomplete_details/reason").and_then(Value::as_str) {
            Some(r) => format!("incomplete: {r}"),
            None => format!("response failed: {}", transport::truncate_str(&v.to_string(), 500)),
        },
    }
}

impl Decoder for ResponsesDecoder {
    fn event(&mut self, ev: SseEvent, out: &mut Vec<StreamEvent>) -> Result<Flow, LlmError> {
        let data = ev.data.trim();
        if data.is_empty() || data == "[DONE]" {
            return Ok(Flow::Continue);
        }
        let v: Value = serde_json::from_str(data)
            .map_err(|e| LlmError::Stream(format!("invalid JSON in stream event: {e}: {}", transport::truncate_str(data, 500))))?;
        let ty = v.get("type").and_then(Value::as_str).or(ev.event.as_deref()).unwrap_or("");
        let index = v.get("output_index").and_then(Value::as_u64).unwrap_or(0) as usize;
        let delta = || v.get("delta").and_then(Value::as_str).unwrap_or("").to_string();
        self.started = true;
        match ty {
            "response.output_item.added" => {
                if let Some(item) = v.get("item") {
                    self.open(index, item, out);
                }
            }
            "response.output_text.delta" | "response.refusal.delta" => {
                let text = delta();
                if !text.is_empty() && self.slot_for_delta(index, "text") {
                    out.push(StreamEvent::TextDelta { index, text });
                }
            }
            "response.reasoning_summary_part.added" => {
                if self.slot_for_delta(index, "reasoning") {
                    if let Some(Slot::Reasoning { parts_seen }) = self.slots.get_mut(&index) {
                        let summary_index = v.get("summary_index").and_then(Value::as_u64).unwrap_or(0);
                        if parts_seen.is_some_and(|p| p != summary_index) {
                            out.push(StreamEvent::ReasoningDelta { index, text: "\n\n".into() });
                        }
                        *parts_seen = Some(summary_index);
                    }
                }
            }
            "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                let text = delta();
                if !text.is_empty() && self.slot_for_delta(index, "reasoning") {
                    out.push(StreamEvent::ReasoningDelta { index, text });
                }
            }
            "response.function_call_arguments.delta" => {
                let d = delta();
                if let Some(Slot::Tool { args, .. }) = self.slots.get_mut(&index) {
                    if !d.is_empty() {
                        args.push_str(&d);
                        out.push(StreamEvent::ToolCallDelta { index, json: d });
                    }
                }
            }
            "response.function_call_arguments.done" => {
                if let (Some(Slot::Tool { args, .. }), Some(full)) =
                    (self.slots.get_mut(&index), v.get("arguments").and_then(Value::as_str))
                {
                    if !full.is_empty() {
                        *args = full.to_string();
                    }
                }
            }
            "response.output_item.done" => {
                if let Some(item) = v.get("item") {
                    self.done_item(index, item, out);
                }
            }
            "response.completed" | "response.done" | "response.incomplete" => {
                let response = v.get("response").cloned().unwrap_or(Value::Null);
                self.close_all(&response, out);
                out.extend(self.usage(&response));
                let status = response.get("status").and_then(Value::as_str).unwrap_or("completed");
                let reason = if ty == "response.incomplete" || status == "incomplete" {
                    match response.pointer("/incomplete_details/reason").and_then(Value::as_str) {
                        Some("max_output_tokens") => FinishReason::Length,
                        _ => FinishReason::Error,
                    }
                } else if matches!(status, "failed" | "cancelled") {
                    return Err(LlmError::Stream(failure_message(&v)));
                } else if self.tool_calls > 0 {
                    FinishReason::ToolUse
                } else {
                    FinishReason::Stop
                };
                out.push(StreamEvent::Finish { reason });
                return Ok(Flow::Done);
            }
            "response.failed" | "error" => return Err(LlmError::Stream(failure_message(&v))),
            _ => {}
        }
        Ok(Flow::Continue)
    }

    fn eof(&mut self, _out: &mut Vec<StreamEvent>) -> Result<(), LlmError> {
        Err(LlmError::Stream(if self.started {
            "stream ended before a terminal response event".into()
        } else {
            "stream ended without any events".into()
        }))
    }
}
