//! Helpers shared by the three API implementations: URL/header construction,
//! option handling, history normalisation, the SSE transport loop and the
//! response/event assembler.

use crate::error::{classify_http_error, network_error, stream_error, trim_body};
use crate::sse::{SseEvent, SseParser};
use crate::{
    AssistantBlock, BlockKind, ChatMessage, LlmError, Model, Provider, Response, StreamEvent,
};
use futures_util::StreamExt;
use ref_proto::{ImageData, Usage};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub(crate) const DEFAULT_OPENAI_BASE: &str = "https://api.openai.com/v1";
pub(crate) const DEFAULT_ANTHROPIC_BASE: &str = "https://api.anthropic.com/v1";
/// Idle timeout used when the provider does not configure one.
const DEFAULT_CHUNK_TIMEOUT: Duration = Duration::from_secs(300);

/// `{base}/{path}` with trailing slashes trimmed; falls back to `default` when
/// the configured base URL is empty.
pub(crate) fn endpoint(base_url: &str, default: &str, path: &str) -> String {
    let base = base_url.trim();
    let base = if base.is_empty() { default } else { base };
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

/// Host part of a URL (lower-cased, without port / credentials).
pub(crate) fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if host.starts_with('[') {
        host.split_inclusive(']').next().unwrap_or(host)
    } else {
        host.split(':').next().unwrap_or(host)
    };
    host.to_ascii_lowercase()
}

/// Whether the provider talks to the official OpenAI API.
pub(crate) fn is_official_openai(provider: &Provider) -> bool {
    let base = if provider.base_url.trim().is_empty() {
        DEFAULT_OPENAI_BASE
    } else {
        provider.base_url.as_str()
    };
    host_of(base) == "api.openai.com"
}

/// `data:<mime>;base64,<data>`
pub(crate) fn data_url(img: &ImageData) -> String {
    format!("data:{};base64,{}", img.mime, img.data)
}

/// Whether an assistant message was produced by the provider + model of the
/// current request (only then is opaque reasoning data replayed).
pub(crate) fn same_origin(
    origin: &Option<(String, String)>,
    provider: &Provider,
    model: &Model,
) -> bool {
    matches!(origin, Some((p, m)) if *p == provider.id && *m == model.id)
}

/// Prompt cache key derived from the session id: OpenAI caps it at 64 chars,
/// so 68-char `ses_<hex>` ids lose their prefix and anything longer is cut.
pub(crate) fn prompt_cache_key(session_id: &str) -> Option<String> {
    let id = session_id.trim();
    if id.is_empty() {
        return None;
    }
    let id = if id.len() > 64 {
        id.strip_prefix("ses_").unwrap_or(id)
    } else {
        id
    };
    Some(truncate_chars(id, 64))
}

pub(crate) fn truncate_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// 32-bit FNV-1a, used to keep shortened ids unique and deterministic.
fn fnv1a(s: &str) -> u32 {
    s.bytes().fold(0x811c_9dc5u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

/// Make a tool-call id safe for APIs that restrict ids to `[a-zA-Z0-9_-]`
/// and `max` characters. The mapping is deterministic, so calls and results
/// stay paired; ids that are already valid are returned unchanged.
pub(crate) fn normalize_call_id(id: &str, max: usize) -> String {
    let sanitized: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() {
        return format!("call_{:08x}", fnv1a(id));
    }
    if sanitized.len() <= max {
        return sanitized;
    }
    let keep = max.saturating_sub(9);
    format!("{}_{:08x}", &sanitized[..keep], fnv1a(id))
}

/// Integer at a JSON pointer (missing / null / negative -> `None`).
pub(crate) fn ptr_u64(v: &Value, pointer: &str) -> Option<u64> {
    opt_u64(v.pointer(pointer))
}

/// Parse tool-call arguments into a JSON object (`{}` when invalid / not an object).
pub(crate) fn parse_arguments(arguments: &str) -> Value {
    match serde_json::from_str::<Value>(arguments) {
        Ok(v @ Value::Object(_)) => v,
        _ => Value::Object(Map::new()),
    }
}

/// Tool-call arguments as a JSON string (`{}` when empty).
pub(crate) fn arguments_string(arguments: &str) -> String {
    if arguments.trim().is_empty() {
        "{}".to_string()
    } else {
        arguments.to_string()
    }
}

/// Session-affinity headers so gateways route a conversation to the same
/// backend (improves prompt cache hits).
pub(crate) fn session_headers(session_id: &str, names: &[&str]) -> Vec<(String, String)> {
    let id = session_id.trim();
    if id.is_empty() {
        return Vec::new();
    }
    names
        .iter()
        .map(|n| (n.to_string(), id.to_string()))
        .collect()
}

/// Text used instead of an image for models without vision support.
pub(crate) const IMAGE_OMITTED: &str = "(image omitted: model does not support images)";

/// `camelCase` -> `snake_case` (already snake-case keys are unchanged).
pub(crate) fn camel_to_snake(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for (i, c) in s.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// Recursively convert object keys to snake_case.
pub(crate) fn snake_keys(v: &Value) -> Value {
    match v {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (camel_to_snake(k), snake_keys(v)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(snake_keys).collect()),
        other => other.clone(),
    }
}

/// Deep-merge `overlay` into `base`: objects merge recursively, everything
/// else (including arrays) is replaced.
pub(crate) fn deep_merge(base: &mut Value, overlay: &Value) {
    match (base, overlay) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                match b.get_mut(k) {
                    Some(existing) if existing.is_object() && v.is_object() => {
                        deep_merge(existing, v)
                    }
                    _ => {
                        b.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (b, o) => *b = o.clone(),
    }
}

/// Insert `key = value` into `body`, deep-merging when both are objects.
pub(crate) fn merge_key(body: &mut Map<String, Value>, key: &str, value: &Value) {
    match body.get_mut(key) {
        Some(existing) if existing.is_object() && value.is_object() => deep_merge(existing, value),
        _ => {
            body.insert(key.to_string(), value.clone());
        }
    }
}

/// Option keys that configure the HTTP client rather than the request body.
pub(crate) const CLIENT_OPTION_KEYS: &[&str] = &[
    "apiKey",
    "baseURL",
    "baseUrl",
    "headers",
    "timeout",
    "headerTimeout",
    "chunkTimeout",
    "includeUsage",
    "fetch",
    "setCacheKey",
];

/// Read a string option, accepting both `camelCase` and `snake_case` names.
pub(crate) fn opt_str<'a>(opts: &'a Map<String, Value>, names: &[&str]) -> Option<&'a str> {
    names
        .iter()
        .find_map(|n| opts.get(*n))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

pub(crate) fn opt_u64(v: Option<&Value>) -> Option<u64> {
    let v = v?;
    v.as_u64().or_else(|| {
        v.as_f64()
            .filter(|f| f.is_finite() && *f >= 0.0)
            .map(|f| f as u64)
    })
}

/// Normalise history before lowering it to a wire format:
/// - assistant messages without blocks are dropped,
/// - tool calls without a result get a synthetic error result (APIs reject
///   unanswered tool calls),
/// - tool results whose call is unknown are dropped (APIs reject them too),
/// - duplicate results for the same call are dropped.
pub(crate) fn normalize_history(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    let mut out: Vec<ChatMessage> = Vec::with_capacity(messages.len());
    let mut known_calls: HashSet<&str> = HashSet::new();
    let mut answered: HashSet<&str> = HashSet::new();
    // (id, name) of calls from the latest assistant message still awaiting results.
    let mut pending: Vec<(&str, &str)> = Vec::new();

    let flush =
        |pending: &mut Vec<(&str, &str)>, answered: &HashSet<&str>, out: &mut Vec<ChatMessage>| {
            for (id, name) in pending.drain(..) {
                if !answered.contains(id) {
                    out.push(ChatMessage::ToolResult {
                        call_id: id.to_string(),
                        name: name.to_string(),
                        content: "No result provided".into(),
                        images: Vec::new(),
                        is_error: true,
                    });
                }
            }
        };

    for msg in messages {
        match msg {
            ChatMessage::ToolResult { call_id, .. } => {
                if known_calls.contains(call_id.as_str()) && answered.insert(call_id.as_str()) {
                    out.push(msg.clone());
                }
            }
            ChatMessage::User(_) => {
                flush(&mut pending, &answered, &mut out);
                out.push(msg.clone());
            }
            ChatMessage::Assistant { blocks, .. } => {
                flush(&mut pending, &answered, &mut out);
                if blocks.is_empty() {
                    continue;
                }
                for b in blocks {
                    if let AssistantBlock::ToolCall { id, name, .. } = b {
                        known_calls.insert(id.as_str());
                        pending.push((id.as_str(), name.as_str()));
                    }
                }
                out.push(msg.clone());
            }
        }
    }
    flush(&mut pending, &answered, &mut out);
    out
}

/// Whether the history contains tool calls or results.
pub(crate) fn has_tool_history(messages: &[ChatMessage]) -> bool {
    messages.iter().any(|m| match m {
        ChatMessage::ToolResult { .. } => true,
        ChatMessage::Assistant { blocks, .. } => blocks
            .iter()
            .any(|b| matches!(b, AssistantBlock::ToolCall { .. })),
        ChatMessage::User(_) => false,
    })
}

/// Build a header map: `defaults` first, then provider headers (which
/// override defaults case-insensitively). Names in `skip` (lower-case) are
/// not taken from the provider headers (the caller merges them itself).
/// Invalid names/values are ignored with a warning.
pub(crate) fn build_headers(
    provider: &Provider,
    defaults: Vec<(String, String)>,
    skip: &[&str],
) -> HeaderMap {
    let mut map = HeaderMap::new();
    let mut put = |k: &str, v: &str| match (
        HeaderName::from_bytes(k.trim().as_bytes()),
        HeaderValue::from_str(v.trim()),
    ) {
        (Ok(name), Ok(value)) => {
            map.insert(name, value);
        }
        _ => tracing::warn!(header = k, "ignoring invalid HTTP header"),
    };
    for (k, v) in &defaults {
        put(k, v);
    }
    for (k, v) in &provider.headers {
        if !skip.contains(&k.trim().to_ascii_lowercase().as_str()) {
            put(k, v);
        }
    }
    map
}

/// Find a provider header value by case-insensitive name.
pub(crate) fn provider_header<'a>(provider: &'a Provider, name: &str) -> Option<&'a str> {
    provider
        .headers
        .iter()
        .find(|(k, _)| k.trim().eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn chunk_timeout(provider: &Provider) -> Duration {
    if provider.chunk_timeout.is_zero() {
        DEFAULT_CHUNK_TIMEOUT
    } else {
        provider.chunk_timeout
    }
}

/// Outcome of [`run_sse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StreamEnd {
    /// The handler saw a terminal event.
    Terminal,
    /// The server closed the stream before a terminal event.
    Eof,
}

/// POST `body` to `url` and feed every SSE event to `handle` until it
/// returns `Ok(true)` (terminal event) or the stream ends.
///
/// Honours the cancellation token while waiting for headers and for every
/// chunk, and applies the provider's idle timeout between chunks.
pub(crate) async fn run_sse(
    provider: &Provider,
    url: &str,
    headers: HeaderMap,
    body: &Value,
    cancel: &CancellationToken,
    mut handle: impl FnMut(SseEvent) -> Result<bool, LlmError>,
) -> Result<StreamEnd, LlmError> {
    if cancel.is_cancelled() {
        return Err(LlmError::cancelled());
    }
    let payload = serde_json::to_vec(body)
        .map_err(|e| LlmError::fatal(format!("failed to encode request: {e}")))?;
    let idle = chunk_timeout(provider);
    let request = crate::http_client()
        .post(url)
        .headers(headers)
        .body(payload)
        .send();

    let resp = tokio::select! {
        _ = cancel.cancelled() => return Err(LlmError::cancelled()),
        r = tokio::time::timeout(idle, request) => match r {
            Err(_) => return Err(LlmError::transient("timed out waiting for response headers")),
            Ok(Err(e)) => return Err(network_error(&e)),
            Ok(Ok(resp)) => resp,
        },
    };

    let status = resp.status();
    if !status.is_success() {
        let resp_headers = resp.headers().clone();
        let text = tokio::select! {
            _ = cancel.cancelled() => return Err(LlmError::cancelled()),
            r = tokio::time::timeout(idle, resp.text()) => r.ok().and_then(Result::ok).unwrap_or_default(),
        };
        return Err(classify_http_error(status.as_u16(), &resp_headers, &text));
    }

    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if content_type.contains("application/json") {
        // Some gateways answer streaming requests with a plain JSON error.
        let text = tokio::select! {
            _ = cancel.cancelled() => return Err(LlmError::cancelled()),
            r = tokio::time::timeout(idle, resp.text()) => r.ok().and_then(Result::ok).unwrap_or_default(),
        };
        return Err(json_body_error(&text));
    }

    let mut parser = SseParser::default();
    let mut stream = resp.bytes_stream();
    loop {
        let next = tokio::select! {
            _ = cancel.cancelled() => return Err(LlmError::cancelled()),
            r = tokio::time::timeout(idle, stream.next()) => match r {
                Err(_) => return Err(LlmError::transient(format!("stream idle timeout ({}s without data)", idle.as_secs()))),
                Ok(next) => next,
            },
        };
        let events = match next {
            Some(Ok(bytes)) => parser.push(&bytes),
            Some(Err(e)) => return Err(network_error(&e)),
            None => {
                for ev in parser.finish() {
                    if handle(ev)? {
                        return Ok(StreamEnd::Terminal);
                    }
                }
                return Ok(StreamEnd::Eof);
            }
        };
        for ev in events {
            if handle(ev)? {
                return Ok(StreamEnd::Terminal);
            }
        }
    }
}

/// Per-API stream decoder.
pub(crate) trait StreamState {
    /// Handle one SSE event; `Ok(true)` means a terminal event was seen.
    fn handle(&mut self, asm: &mut Assembler<'_>, ev: SseEvent) -> Result<bool, LlmError>;
    /// Build the final response once the stream is over.
    fn finish(self, asm: Assembler<'_>, end: StreamEnd) -> Result<Response, LlmError>;
}

/// Send the request and decode the stream with `state`.
pub(crate) async fn drive<S: StreamState>(
    mut state: S,
    provider: &Provider,
    url: &str,
    headers: HeaderMap,
    body: &Value,
    cancel: &CancellationToken,
    on_event: &mut (dyn FnMut(StreamEvent) + Send),
) -> Result<Response, LlmError> {
    let mut asm = Assembler::new(on_event);
    let end = run_sse(provider, url, headers, body, cancel, |ev| {
        state.handle(&mut asm, ev)
    })
    .await?;
    state.finish(asm, end)
}

/// Decode a recorded SSE byte stream (tests).
#[cfg(test)]
pub(crate) fn replay<S: StreamState>(
    mut state: S,
    bytes: &[u8],
) -> (Result<Response, LlmError>, Vec<StreamEvent>) {
    let mut events = Vec::new();
    let mut sink = |e: StreamEvent| events.push(e);
    let result = {
        let mut asm = Assembler::new(&mut sink);
        let mut parser = SseParser::default();
        let mut end = StreamEnd::Eof;
        let mut err = None;
        // Feed in small uneven chunks to exercise incremental parsing.
        'outer: for chunk in bytes
            .chunks(7)
            .map(|c| c.to_vec())
            .chain(std::iter::once(Vec::new()))
        {
            let evs = if chunk.is_empty() {
                parser.finish()
            } else {
                parser.push(&chunk)
            };
            for ev in evs {
                match state.handle(&mut asm, ev) {
                    Ok(true) => {
                        end = StreamEnd::Terminal;
                        break 'outer;
                    }
                    Ok(false) => {}
                    Err(e) => {
                        err = Some(e);
                        break 'outer;
                    }
                }
            }
        }
        match err {
            Some(e) => Err(e),
            None => state.finish(asm, end),
        }
    };
    (result, events)
}

/// Error for a 2xx response that carried JSON instead of an event stream.
fn json_body_error(text: &str) -> LlmError {
    let parsed: Option<Value> = serde_json::from_str(text).ok();
    if let Some(err) = parsed.as_ref().and_then(|v| v.get("error")) {
        let (code, message) = error_fields(err);
        return stream_error(code.as_deref(), &message);
    }
    LlmError::transient(format!(
        "expected an event stream, got JSON: {}",
        trim_body(text)
    ))
}

/// Extract `(code, message)` from a provider error object (or string).
pub(crate) fn error_fields(err: &Value) -> (Option<String>, String) {
    if let Some(s) = err.as_str() {
        return (None, s.to_string());
    }
    let code = ["code", "type", "status"]
        .iter()
        .find_map(|k| match err.get(*k) {
            Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
            Some(Value::Number(n)) => Some(n.to_string()),
            _ => None,
        });
    let message = err
        .get("message")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| {
            if err.is_null() {
                String::new()
            } else {
                err.to_string()
            }
        });
    (code, message)
}

/// Assembles the final [`Response`] while emitting [`StreamEvent`]s, keeping
/// event indices and `Response::blocks` positions in sync.
pub(crate) struct Assembler<'a> {
    on_event: &'a mut (dyn FnMut(StreamEvent) + Send),
    blocks: Vec<AssistantBlock>,
    open: Vec<bool>,
    usage: Usage,
}

impl<'a> Assembler<'a> {
    pub fn new(on_event: &'a mut (dyn FnMut(StreamEvent) + Send)) -> Self {
        Assembler {
            on_event,
            blocks: Vec::new(),
            open: Vec::new(),
            usage: Usage::default(),
        }
    }

    /// Open a new block and return its index.
    pub fn start(&mut self, kind: BlockKind) -> usize {
        let index = self.blocks.len();
        self.blocks.push(match &kind {
            BlockKind::Text => AssistantBlock::Text(String::new()),
            BlockKind::Reasoning => AssistantBlock::Reasoning {
                text: String::new(),
                provider_data: None,
            },
            BlockKind::ToolCall { id, name } => AssistantBlock::ToolCall {
                id: id.clone(),
                name: name.clone(),
                arguments: String::new(),
                provider_data: None,
            },
        });
        self.open.push(true);
        (self.on_event)(StreamEvent::BlockStart { index, kind });
        index
    }

    /// Append text (text, reasoning or tool-argument JSON) to a block.
    pub fn delta(&mut self, index: usize, text: &str) {
        if text.is_empty() {
            return;
        }
        let Some(block) = self.blocks.get_mut(index) else {
            return;
        };
        match block {
            AssistantBlock::Text(t) => t.push_str(text),
            AssistantBlock::Reasoning { text: t, .. } => t.push_str(text),
            AssistantBlock::ToolCall { arguments, .. } => arguments.push_str(text),
        }
        (self.on_event)(StreamEvent::Delta {
            index,
            text: text.to_string(),
        });
    }

    /// Close a block (idempotent).
    pub fn end(&mut self, index: usize) {
        if let Some(open) = self.open.get_mut(index)
            && *open
        {
            *open = false;
            (self.on_event)(StreamEvent::BlockEnd { index });
        }
    }

    pub fn end_all(&mut self) {
        for i in 0..self.blocks.len() {
            self.end(i);
        }
    }

    pub fn is_open(&self, index: usize) -> bool {
        self.open.get(index).copied().unwrap_or(false)
    }

    pub fn block(&self, index: usize) -> Option<&AssistantBlock> {
        self.blocks.get(index)
    }

    pub fn block_mut(&mut self, index: usize) -> Option<&mut AssistantBlock> {
        self.blocks.get_mut(index)
    }

    /// Text currently accumulated in a block (arguments for tool calls).
    pub fn text_of(&self, index: usize) -> &str {
        match self.blocks.get(index) {
            Some(AssistantBlock::Text(t)) => t,
            Some(AssistantBlock::Reasoning { text, .. }) => text,
            Some(AssistantBlock::ToolCall { arguments, .. }) => arguments,
            None => "",
        }
    }

    /// Attach provider replay data to a reasoning or tool-call block.
    pub fn set_provider_data(&mut self, index: usize, data: Value) {
        if let Some(
            AssistantBlock::Reasoning { provider_data, .. }
            | AssistantBlock::ToolCall { provider_data, .. },
        ) = self.blocks.get_mut(index)
        {
            *provider_data = Some(data);
        }
    }

    pub fn has_tool_calls(&self) -> bool {
        self.blocks
            .iter()
            .any(|b| matches!(b, AssistantBlock::ToolCall { .. }))
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// Record usage and emit a [`StreamEvent::Usage`].
    pub fn usage(&mut self, usage: Usage) {
        self.usage = usage;
        (self.on_event)(StreamEvent::Usage(usage));
    }

    /// Close remaining blocks and build the response. Empty tool arguments
    /// become `{}`; a plain "stop" with tool calls becomes "tool_calls".
    pub fn finish(mut self, stop_reason: &str) -> Response {
        self.end_all();
        for b in &mut self.blocks {
            if let AssistantBlock::ToolCall { arguments, .. } = b
                && arguments.trim().is_empty()
            {
                *arguments = "{}".into();
            }
        }
        let has_tools = self.has_tool_calls();
        let stop_reason = if stop_reason == "stop" && has_tools {
            "tool_calls".to_string()
        } else {
            stop_reason.to_string()
        };
        Response {
            blocks: self.blocks,
            usage: self.usage,
            stop_reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints() {
        assert_eq!(
            endpoint("", DEFAULT_OPENAI_BASE, "chat/completions"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            endpoint("https://x.ai/v1//", DEFAULT_OPENAI_BASE, "/responses"),
            "https://x.ai/v1/responses"
        );
        assert_eq!(
            host_of("https://user:pw@API.openai.com:443/v1"),
            "api.openai.com"
        );
        assert_eq!(host_of("http://[::1]:8080/v1"), "[::1]");
    }

    #[test]
    fn case_conversion_and_merge() {
        assert_eq!(camel_to_snake("budgetTokens"), "budget_tokens");
        assert_eq!(camel_to_snake("already_snake"), "already_snake");
        let mut a = serde_json::json!({"a": {"b": 1, "c": [1]}, "d": 1});
        deep_merge(
            &mut a,
            &serde_json::json!({"a": {"c": [2], "e": true}, "d": {"x": 1}}),
        );
        assert_eq!(
            a,
            serde_json::json!({"a": {"b": 1, "c": [2], "e": true}, "d": {"x": 1}})
        );
    }

    #[test]
    fn cache_key_clamp() {
        let long = format!("ses_{}", "a".repeat(64));
        assert_eq!(prompt_cache_key(&long).unwrap(), "a".repeat(64));
        assert_eq!(prompt_cache_key("abc").unwrap(), "abc");
        assert!(prompt_cache_key("  ").is_none());
    }

    #[test]
    fn history_normalisation() {
        let call = |id: &str| AssistantBlock::ToolCall {
            id: id.into(),
            name: "read".into(),
            arguments: "{}".into(),
            provider_data: None,
        };
        let result = |id: &str| ChatMessage::ToolResult {
            call_id: id.into(),
            name: "read".into(),
            content: "ok".into(),
            images: vec![],
            is_error: false,
        };
        let msgs = vec![
            result("ghost"),
            ChatMessage::Assistant {
                blocks: vec![call("a"), call("b")],
                origin: None,
            },
            result("a"),
            result("a"),
            ChatMessage::User(vec![crate::UserContent::Text("next".into())]),
            ChatMessage::Assistant {
                blocks: vec![],
                origin: None,
            },
        ];
        let out = normalize_history(&msgs);
        assert_eq!(out.len(), 4);
        assert!(
            matches!(&out[1], ChatMessage::ToolResult { call_id, is_error: false, .. } if call_id == "a")
        );
        assert!(
            matches!(&out[2], ChatMessage::ToolResult { call_id, is_error: true, .. } if call_id == "b")
        );
        assert!(matches!(&out[3], ChatMessage::User(_)));
    }
}
