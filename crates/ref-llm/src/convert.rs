//! Provider-neutral view of the conversation: `ref_proto::Message`s flattened into alternating
//! user / assistant / tool-result turns that each provider renders into its own wire format.

use std::collections::HashMap;

use ref_proto::{ImagePart, Message, Part, Role, ToolPart, ToolState};
use serde_json::{json, Map, Value};

use crate::{ApiKind, ModelSpec};

pub(crate) const IMAGE_OMITTED: &str = "[image omitted: model does not support images]";
pub(crate) const TOOL_IMAGE_OMITTED: &str = "(tool image omitted: model does not support images)";
pub(crate) const TOOL_INTERRUPTED: &str = "Tool execution was interrupted";
pub(crate) const SEE_ATTACHED_IMAGE: &str = "(see attached image)";

#[derive(Debug, Clone)]
pub(crate) struct Image {
    pub mime: String,
    pub data: String,
}

impl Image {
    fn from_part(p: &ImagePart) -> Self {
        Image { mime: p.mime.clone(), data: p.data.clone() }
    }

    pub fn data_url(&self) -> String {
        format!("data:{};base64,{}", self.mime, self.data)
    }
}

#[derive(Debug, Clone)]
pub(crate) enum UserBlock {
    Text(String),
    Image(Image),
}

#[derive(Debug, Clone)]
pub(crate) enum AsstBlock {
    /// `meta` is only kept when it was produced by the same API kind and model (see
    /// [`replay_meta`]); otherwise it is `None` and providers drop / degrade the block.
    Reasoning { text: String, meta: Option<Value> },
    Text { text: String, meta: Option<Value> },
    ToolCall { call_id: String, name: String, input: Value, meta: Option<Value> },
}

#[derive(Debug, Clone)]
pub(crate) struct ToolResult {
    pub call_id: String,
    pub name: String,
    pub text: String,
    /// Empty when the model does not accept images (a note is appended to `text` instead).
    pub images: Vec<Image>,
    pub is_error: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum Turn {
    User(Vec<UserBlock>),
    Assistant(Vec<AsstBlock>),
    /// Results of the tool calls of the immediately preceding assistant turn, in call order.
    ToolResults(Vec<ToolResult>),
}

/// Tag stored in `meta.api`.
pub(crate) fn api_tag(api: ApiKind) -> &'static str {
    match api {
        ApiKind::Anthropic => "anthropic",
        ApiKind::OpenAIChat => "openai_chat",
        ApiKind::OpenAIResponses => "openai_responses",
    }
}

/// Build a replay meta object `{"api": .., "model": .., ...fields}`.
pub(crate) fn make_meta(model: &ModelSpec, fields: Value) -> Value {
    let mut m = Map::new();
    m.insert("api".into(), Value::String(api_tag(model.api).into()));
    m.insert("model".into(), Value::String(model.api_model.clone()));
    if let Value::Object(f) = fields {
        m.extend(f);
    }
    Value::Object(m)
}

/// Keep `meta` only if it was produced by the same API kind and model id.
fn replay_meta(meta: &Option<Value>, model: &ModelSpec, same_source: bool) -> Option<Value> {
    let m = meta.as_ref()?;
    let ok = same_source
        && m.get("api").and_then(Value::as_str) == Some(api_tag(model.api))
        && m.get("model").and_then(Value::as_str) == Some(model.api_model.as_str());
    ok.then(|| m.clone())
}

fn has_content(msg: &Message) -> bool {
    msg.parts.iter().any(|p| match p {
        Part::Text(t) => !t.text.trim().is_empty(),
        Part::Tool(_) => true,
        Part::Reasoning(_) | Part::Image(_) => false,
    })
}

fn tool_result(tp: &ToolPart, image_input: bool) -> ToolResult {
    let (mut text, is_error) = match tp.state {
        ToolState::Done => (tp.output.clone(), false),
        ToolState::Error => (non_empty(&tp.output, "Tool execution failed"), true),
        ToolState::Denied => (non_empty(&tp.output, "The user denied this tool call"), true),
        ToolState::Cancelled => (non_empty(&tp.output, "Tool execution was cancelled"), true),
        _ => (TOOL_INTERRUPTED.to_string(), true),
    };
    let mut images = Vec::new();
    if tp.state.is_final() && !tp.images.is_empty() {
        if image_input {
            images = tp.images.iter().map(Image::from_part).collect();
        } else {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(TOOL_IMAGE_OMITTED);
        }
    }
    if text.trim().is_empty() {
        text = if images.is_empty() { "(no output)".into() } else { SEE_ATTACHED_IMAGE.into() };
    }
    ToolResult { call_id: tp.call_id.clone(), name: tp.name.clone(), text, images, is_error }
}

fn non_empty(s: &str, fallback: &str) -> String {
    if s.trim().is_empty() {
        fallback.to_string()
    } else {
        s.to_string()
    }
}

/// Flatten messages into turns. Consecutive turns of the same kind are NOT merged here (each
/// provider decides); assistant messages are split so that no text / reasoning follows a tool
/// call within one assistant turn.
pub(crate) fn flatten(messages: &[Message], model: &ModelSpec) -> Vec<Turn> {
    let full_id = model.full_id();
    let mut turns = Vec::new();
    for msg in messages {
        match msg.role {
            Role::User => {
                let mut blocks = Vec::new();
                for p in &msg.parts {
                    match p {
                        Part::Text(t) if !t.text.is_empty() => blocks.push(UserBlock::Text(t.text.clone())),
                        Part::Image(img) => {
                            if model.image_input {
                                blocks.push(UserBlock::Image(Image::from_part(img)));
                            } else {
                                blocks.push(UserBlock::Text(IMAGE_OMITTED.into()));
                            }
                        }
                        _ => {}
                    }
                }
                if !blocks.is_empty() {
                    turns.push(Turn::User(blocks));
                }
            }
            Role::Assistant => {
                if msg.parts.is_empty() {
                    continue;
                }
                let failed = matches!(msg.finish.as_deref(), Some("error") | Some("aborted"));
                if failed && !has_content(msg) {
                    continue;
                }
                let same_source = msg.model.as_deref().is_none_or(|m| m == full_id);
                flatten_assistant(msg, model, same_source, &mut turns);
            }
        }
    }
    turns
}

fn flatten_assistant(msg: &Message, model: &ModelSpec, same_source: bool, turns: &mut Vec<Turn>) {
    let mut blocks: Vec<AsstBlock> = Vec::new();
    let mut results: Vec<ToolResult> = Vec::new();
    let flush = |blocks: &mut Vec<AsstBlock>, results: &mut Vec<ToolResult>, turns: &mut Vec<Turn>| {
        if !blocks.is_empty() {
            turns.push(Turn::Assistant(std::mem::take(blocks)));
        }
        if !results.is_empty() {
            turns.push(Turn::ToolResults(std::mem::take(results)));
        }
    };
    for p in &msg.parts {
        match p {
            Part::Reasoning(r) => {
                if !results.is_empty() {
                    flush(&mut blocks, &mut results, turns);
                }
                let meta = replay_meta(&r.meta, model, same_source);
                if r.text.is_empty() && meta.is_none() {
                    continue;
                }
                blocks.push(AsstBlock::Reasoning { text: r.text.clone(), meta });
            }
            Part::Text(t) => {
                if t.text.is_empty() {
                    continue;
                }
                if !results.is_empty() {
                    flush(&mut blocks, &mut results, turns);
                }
                blocks.push(AsstBlock::Text { text: t.text.clone(), meta: replay_meta(&t.meta, model, same_source) });
            }
            Part::Tool(tp) => {
                blocks.push(AsstBlock::ToolCall {
                    call_id: tp.call_id.clone(),
                    name: tp.name.clone(),
                    input: tool_call_input(tp),
                    meta: replay_meta(&tp.meta, model, same_source),
                });
                results.push(tool_result(tp, model.image_input));
            }
            Part::Image(_) => {}
        }
    }
    flush(&mut blocks, &mut results, turns);
}

/// The arguments to replay for a tool call.
fn tool_call_input(tp: &ToolPart) -> Value {
    match &tp.input {
        Value::Null => {
            if tp.input_raw.trim().is_empty() {
                json!({})
            } else {
                parse_tool_input(&tp.input_raw)
            }
        }
        v => v.clone(),
    }
}

// ───────────────────────────── tool call ids ─────────────────────────────

/// Deterministic mapping of tool call ids onto a provider's allowed alphabet / length. Distinct
/// input ids always map to distinct output ids within one request.
pub(crate) struct IdMap {
    max_len: usize,
    map: HashMap<String, String>,
    used: HashMap<String, String>,
}

impl IdMap {
    pub fn new(max_len: usize) -> Self {
        IdMap { max_len, map: HashMap::new(), used: HashMap::new() }
    }

    pub fn get(&mut self, id: &str) -> String {
        if let Some(v) = self.map.get(id) {
            return v.clone();
        }
        let mut out: String =
            id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
        if out.is_empty() {
            out = "call".into();
        }
        if out.len() > self.max_len || (out != id && self.used.contains_key(&out)) {
            out = shorten(&out, id, self.max_len);
        }
        let mut n = 1u32;
        let base = out.clone();
        while self.used.get(&out).is_some_and(|orig| orig != id) {
            let suffix = format!("_{n}");
            let keep = self.max_len.saturating_sub(suffix.len()).min(base.len());
            out = format!("{}{suffix}", &base[..keep]);
            n += 1;
        }
        self.used.insert(out.clone(), id.to_string());
        self.map.insert(id.to_string(), out.clone());
        out
    }
}

fn shorten(sanitized: &str, original: &str, max_len: usize) -> String {
    let hash = format!("{:016x}", fnv1a(original.as_bytes()));
    let keep = max_len.saturating_sub(hash.len() + 1).min(sanitized.len());
    if keep == 0 {
        return hash[..max_len.min(hash.len())].to_string();
    }
    format!("{}_{hash}", &sanitized[..keep])
}

pub(crate) fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

// ───────────────────────────── tool input parsing ─────────────────────────────

/// Parse streamed tool-call arguments. Empty → `{}`; invalid JSON is repaired leniently when
/// possible (trailing garbage, unterminated strings / objects), otherwise `Value::String(raw)`.
pub fn parse_tool_input(raw: &str) -> Value {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return json!({});
    }
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        return unwrap_double_encoded(v);
    }
    // Leading valid value followed by garbage (e.g. `{"a":1}}` or duplicated objects).
    if let Some(Ok(v)) = serde_json::Deserializer::from_str(trimmed).into_iter::<Value>().next() {
        if v.is_object() {
            return v;
        }
    }
    if let Some(v) = repair_truncated(trimmed) {
        return v;
    }
    Value::String(raw.to_string())
}

fn unwrap_double_encoded(v: Value) -> Value {
    if let Value::String(s) = &v {
        if let Ok(inner @ Value::Object(_)) = serde_json::from_str::<Value>(s.trim()) {
            return inner;
        }
    }
    v
}

/// Close an unterminated JSON object: finish an open string, drop a dangling key / separator,
/// then close all open brackets.
fn repair_truncated(s: &str) -> Option<Value> {
    if !s.starts_with('{') {
        return None;
    }
    let mut stack = Vec::new();
    let mut in_str = false;
    let mut escape = false;
    for c in s.chars() {
        if in_str {
            match (escape, c) {
                (true, _) => escape = false,
                (false, '\\') => escape = true,
                (false, '"') => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => stack.push('}'),
            '[' => stack.push(']'),
            '}' | ']' if stack.pop() != Some(c) => return None,
            _ => {}
        }
    }
    let mut fixed = s.to_string();
    if in_str {
        if escape {
            fixed.pop();
        }
        fixed.push('"');
    }
    // Try progressively: as-is, then with a dangling tail removed.
    let close = |body: &str| {
        let mut b = body.trim_end().to_string();
        while b.ends_with(',') || b.ends_with(':') {
            b.pop();
            b = b.trim_end().to_string();
        }
        for c in stack.iter().rev() {
            b.push(*c);
        }
        serde_json::from_str::<Value>(&b).ok().filter(Value::is_object)
    };
    if let Some(v) = close(&fixed) {
        return Some(v);
    }
    // Dangling key without value: `{"a":1,"b"` or `{"a":1,"b":`.
    let cut = fixed.rfind(',')?;
    close(&fixed[..cut])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_input_parsing() {
        assert_eq!(parse_tool_input(""), json!({}));
        assert_eq!(parse_tool_input("  "), json!({}));
        assert_eq!(parse_tool_input(r#"{"a":1}"#), json!({"a":1}));
        assert_eq!(parse_tool_input(r#"{"a":1}}"#), json!({"a":1}));
        assert_eq!(parse_tool_input(r#"{"a":1}{"a":1}"#), json!({"a":1}));
        assert_eq!(parse_tool_input(r#"{"a":"x\"y"#), json!({"a":"x\"y"}));
        assert_eq!(parse_tool_input(r#"{"a":[1,2"#), json!({"a":[1,2]}));
        assert_eq!(parse_tool_input(r#"{"a":1,"b""#), json!({"a":1}));
        assert_eq!(parse_tool_input(r#"{"a":1,"b":"#), json!({"a":1}));
        assert_eq!(parse_tool_input(r#""{\"a\":1}""#), json!({"a":1}));
        assert_eq!(parse_tool_input("not json"), Value::String("not json".into()));
    }

    #[test]
    fn id_map() {
        let mut m = IdMap::new(64);
        assert_eq!(m.get("toolu_01ABC"), "toolu_01ABC");
        assert_eq!(m.get("call|fc_1"), "call_fc_1");
        assert_eq!(m.get("call|fc_1"), "call_fc_1");
        let other = m.get("call:fc_1");
        assert_ne!(other, "call_fc_1");
        assert!(other.len() <= 64);
        let long = "x".repeat(100);
        let a = m.get(&long);
        assert_eq!(a.len(), 64);
        assert_eq!(IdMap::new(64).get(&long), a, "deterministic");
        let mut m40 = IdMap::new(40);
        assert!(m40.get(&long).len() <= 40);
    }
}
