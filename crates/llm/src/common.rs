//! Plumbing shared by the three protocols: history normalization, HTTP send with
//! cancellation/timeouts, and the SSE driver that feeds a protocol [`Parser`].

use crate::sse::{SseDecoder, SseEvent};
use crate::*;
use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, CONTENT_TYPE};
use std::collections::HashSet;

/// Default idle timeout (headers and between body chunks) when `spec.chunk_timeout` is unset.
pub(crate) const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(300);
/// Error bodies are truncated to this many bytes.
const MAX_ERROR_BODY: usize = 64 * 1024;
/// Non-streaming bodies (servers ignoring `stream: true`) are capped at this size.
const MAX_JSON_BODY: usize = 8 * 1024 * 1024;

pub(crate) const SYNTHETIC_TOOL_RESULT: &str = "No result provided";

/// Protocol-specific SSE parser. Kept free of I/O so it can be unit-tested with canned events.
/// `Send` so the future returned by [`crate::stream`] is `Send` (servers spawn turns on tokio).
pub(crate) trait Parser: Send {
    /// Handle one SSE event, returning the stream events it produces.
    fn feed(&mut self, ev: &SseEvent) -> Result<Vec<StreamEvent>, LlmError>;
    /// True once the terminal event was seen; the driver stops reading.
    fn done(&self) -> bool;
    /// End of input (terminal event or EOF): flush open blocks, emit usage, decide the finish
    /// reason, or fail if the stream was incomplete.
    fn finish(&mut self) -> Result<(Vec<StreamEvent>, FinishReason), LlmError>;
}

/// History hygiene applied before every protocol conversion:
/// - assistant messages without text or tool calls are dropped (providers reject them);
/// - every tool call gets exactly one result: missing ones get a synthetic error result
///   inserted right after the assistant turn's results, orphan/duplicate results are dropped;
/// - tool results are moved directly behind their assistant turn.
pub(crate) fn normalize(messages: &[LlmMessage]) -> Vec<LlmMessage> {
    let mut out: Vec<LlmMessage> = Vec::with_capacity(messages.len());
    // Tool calls of the latest assistant turn still waiting for a result, in call order.
    let mut pending: Vec<(String, String)> = Vec::new();
    let mut answered: HashSet<String> = HashSet::new();

    fn flush(out: &mut Vec<LlmMessage>, pending: &mut Vec<(String, String)>, answered: &mut HashSet<String>) {
        for (id, name) in pending.drain(..) {
            if !answered.contains(&id) {
                out.push(LlmMessage::ToolResult {
                    call_id: id,
                    name,
                    content: SYNTHETIC_TOOL_RESULT.to_string(),
                    images: Vec::new(),
                    is_error: true,
                });
            }
        }
        answered.clear();
    }

    for msg in messages {
        match msg {
            LlmMessage::ToolResult { call_id, .. } => {
                let known = pending.iter().any(|(id, _)| id == call_id);
                if known && answered.insert(call_id.clone()) {
                    out.push(msg.clone());
                } else {
                    tracing::debug!(call_id, "dropping orphan or duplicate tool result");
                }
            }
            LlmMessage::User(content) => {
                flush(&mut out, &mut pending, &mut answered);
                if !content.is_empty() {
                    out.push(msg.clone());
                }
            }
            LlmMessage::Assistant { parts, .. } => {
                flush(&mut out, &mut pending, &mut answered);
                let has_text = parts.iter().any(|p| matches!(p, AssistantPart::Text(t) if !t.trim().is_empty()));
                let calls: Vec<(String, String)> = parts
                    .iter()
                    .filter_map(|p| match p {
                        AssistantPart::ToolCall { id, name, .. } => Some((id.clone(), name.clone())),
                        _ => None,
                    })
                    .collect();
                if !has_text && calls.is_empty() {
                    continue;
                }
                pending = calls;
                out.push(msg.clone());
            }
        }
    }
    flush(&mut out, &mut pending, &mut answered);
    out
}

/// `data:` URL for an image.
pub(crate) fn data_url(img: &ImageData) -> String {
    format!("data:{};base64,{}", img.mime, img.data)
}

/// Deep-merge `src` into `dst`: objects merge recursively, everything else is replaced.
pub(crate) fn merge_json(dst: &mut Map<String, Value>, src: &Map<String, Value>) {
    for (k, v) in src {
        match (dst.get_mut(k), v) {
            (Some(Value::Object(d)), Value::Object(s)) => merge_json(d, s),
            _ => {
                dst.insert(k.clone(), v.clone());
            }
        }
    }
}

/// Merge `options.body` / `options.extraBody` (if the config left them in options) and then
/// `spec.body` into the request body.
pub(crate) fn merge_extra_body(body: &mut Map<String, Value>, spec: &ModelSpec) {
    for key in ["body", "extraBody"] {
        if let Some(Value::Object(extra)) = spec.options.get(key) {
            merge_json(body, extra);
        }
    }
    merge_json(body, &spec.body);
}

/// Insert into a nested object `body[outer][inner] = value`, creating `outer` as needed.
pub(crate) fn set_nested(body: &mut Map<String, Value>, outer: &str, inner: &str, value: Value) {
    let entry = body.entry(outer.to_string()).or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    if let Value::Object(m) = entry {
        m.insert(inner.to_string(), value);
    }
}

/// Text placeholder for empty tool output (some hosts reject empty content).
pub(crate) fn tool_output_text(content: &str) -> String {
    if content.is_empty() {
        "(no output)".to_string()
    } else {
        content.to_string()
    }
}

/// Truncate to at most `max` chars (unicode-safe).
pub(crate) fn truncate_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Make a tool-call id safe for providers with strict id rules (`^[A-Za-z0-9_-]{1,max}$`).
/// Deterministic, so a tool call and its result always map to the same wire id.
pub(crate) fn sanitize_id(id: &str, max: usize) -> String {
    let valid = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    if !id.is_empty() && id.len() <= max && id.chars().all(valid) {
        return id.to_string();
    }
    let mut s: String = id.chars().map(|c| if valid(c) { c } else { '_' }).collect();
    if s.is_empty() {
        s.push_str("call");
    }
    if s.len() > max {
        // Keep a prefix and append a hash of the full original id to stay unique.
        let hash = format!("{:016x}", fnv1a(id.as_bytes()));
        let keep = max.saturating_sub(hash.len() + 1);
        s = format!("{}_{}", &s[..keep], hash);
        s.truncate(max);
    }
    s
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

/// Random-ish id for tool calls that arrive without one.
pub(crate) fn generate_call_id() -> String {
    use rand::Rng;
    let n: u64 = rand::thread_rng().gen();
    format!("call_{n:016x}")
}

/// camelCase → snake_case (`serviceTier` → `service_tier`).
pub(crate) fn snake_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            if !out.is_empty() {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

pub(crate) fn host_has(spec: &ModelSpec, needle: &str) -> bool {
    spec.base_url.to_ascii_lowercase().contains(needle)
}

/// Max output tokens: request override, else the model limit; `None` when unknown.
pub(crate) fn max_tokens(spec: &ModelSpec, req: &ChatRequest) -> Option<u64> {
    let limit = (spec.output > 0).then_some(spec.output);
    match (req.max_tokens, limit) {
        (Some(m), Some(l)) => Some(m.min(l)),
        (m, l) => m.or(l),
    }
}

/// Join non-empty system parts.
pub(crate) fn join_system(system: &[String]) -> String {
    system.iter().map(|s| s.trim_end()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n\n")
}

/// Parse tool-call arguments into a JSON object (`{}` on empty/invalid input).
pub(crate) fn args_object(arguments: &str) -> Value {
    match serde_json::from_str::<Value>(arguments) {
        Ok(v @ Value::Object(_)) => v,
        _ => Value::Object(Map::new()),
    }
}

/// Insert `spec.headers` over the protocol defaults (case-insensitive). Invalid entries are skipped.
pub(crate) fn apply_headers(headers: &mut HeaderMap, extra: &BTreeMap<String, String>) {
    for (k, v) in extra {
        match (HeaderName::from_bytes(k.as_bytes()), HeaderValue::from_str(v)) {
            (Ok(name), Ok(value)) => {
                headers.insert(name, value);
            }
            _ => tracing::warn!(header = %k, "skipping invalid HTTP header"),
        }
    }
}

pub(crate) fn set_header(headers: &mut HeaderMap, name: &'static str, value: &str) {
    match HeaderValue::from_str(value) {
        Ok(v) => {
            headers.insert(name, v);
        }
        Err(_) => tracing::warn!(header = name, "skipping invalid header value"),
    }
}

pub(crate) fn idle_timeout(spec: &ModelSpec) -> Duration {
    spec.chunk_timeout.filter(|d| !d.is_zero()).unwrap_or(DEFAULT_IDLE_TIMEOUT)
}

/// POST `body` and stream the SSE response through `parser`, delivering events in order.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run(
    client: &reqwest::Client,
    spec: &ModelSpec,
    url: &str,
    headers: HeaderMap,
    body: &Value,
    parser: &mut dyn Parser,
    cancel: &CancellationToken,
    on_event: &mut (dyn FnMut(StreamEvent) + Send),
) -> Result<FinishReason, LlmError> {
    let idle = idle_timeout(spec);
    tracing::debug!(url, model = %spec.api_model, "llm request");
    // Body only (never headers: they carry the API key).
    tracing::trace!(target: "hn_llm::wire", body = %body, "request body");
    let send = client.post(url).headers(headers).json(body).send();
    let resp = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err(LlmError::aborted()),
        r = tokio::time::timeout(idle, send) => match r {
            Err(_) => return Err(LlmError::new(
                ErrorKind::Retryable,
                format!("timeout: no response headers within {}s", idle.as_secs()),
            )),
            Ok(Err(e)) => return Err(LlmError::from_reqwest(&e)),
            Ok(Ok(resp)) => resp,
        },
    };

    let status = resp.status();
    if !status.is_success() {
        let headers = resp.headers().clone();
        let body = read_capped(resp, MAX_ERROR_BODY, idle, cancel).await?;
        tracing::trace!(target: "hn_llm::wire", status = status.as_u16(), data = %body, "error body");
        return Err(LlmError::from_http(status.as_u16(), &headers, &body));
    }

    let mut emitted = false;
    let mut emit = |events: Vec<StreamEvent>, emitted: &mut bool| {
        for ev in events {
            *emitted = true;
            on_event(ev);
        }
    };

    // Servers that ignore `stream: true` (or report errors with 200) answer with plain JSON.
    let is_json = resp
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.contains("json") && !ct.contains("event-stream"));
    if is_json {
        let text = read_capped(resp, MAX_JSON_BODY, idle, cancel).await?;
        tracing::trace!(target: "hn_llm::wire", data = %text, "json body");
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            if v.get("error").is_some_and(|e| !e.is_null()) {
                return Err(LlmError::from_stream_payload(&text));
            }
        }
        let evs = parser.feed(&SseEvent { event: None, data: text })?;
        emit(evs, &mut emitted);
        let (evs, reason) = parser.finish().map_err(|e| e.with_partial(emitted))?;
        emit(evs, &mut emitted);
        return Ok(reason);
    }

    let mut stream = resp.bytes_stream();
    let mut decoder = SseDecoder::new();
    'read: loop {
        let next = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(LlmError::aborted().with_partial(emitted)),
            r = tokio::time::timeout(idle, stream.next()) => r,
        };
        let chunk = match next {
            Err(_) => {
                return Err(LlmError::new(
                    ErrorKind::Retryable,
                    format!("timeout: no data from the provider for {}s", idle.as_secs()),
                )
                .with_partial(emitted))
            }
            Ok(None) => break,
            Ok(Some(Err(e))) => return Err(LlmError::from_reqwest(&e).with_partial(emitted)),
            Ok(Some(Ok(chunk))) => chunk,
        };
        for ev in decoder.push(&chunk) {
            tracing::trace!(target: "hn_llm::wire", event = ?ev.event, data = %ev.data, "sse");
            let evs = parser.feed(&ev).map_err(|e| e.with_partial(emitted))?;
            emit(evs, &mut emitted);
            if parser.done() {
                break 'read;
            }
        }
    }
    if !parser.done() {
        if let Some(ev) = decoder.finish() {
            let evs = parser.feed(&ev).map_err(|e| e.with_partial(emitted))?;
            emit(evs, &mut emitted);
        }
    }
    let (evs, reason) = parser.finish().map_err(|e| e.with_partial(emitted))?;
    emit(evs, &mut emitted);
    Ok(reason)
}

/// Read up to `cap` bytes of a body as (lossy) UTF-8, honoring cancellation and the idle timeout.
async fn read_capped(
    mut resp: reqwest::Response,
    cap: usize,
    idle: Duration,
    cancel: &CancellationToken,
) -> Result<String, LlmError> {
    let mut buf: Vec<u8> = Vec::new();
    while buf.len() < cap {
        let next = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(LlmError::aborted()),
            r = tokio::time::timeout(idle, resp.chunk()) => r,
        };
        match next {
            Ok(Ok(Some(chunk))) => buf.extend_from_slice(&chunk),
            Ok(Ok(None)) => break,
            // A broken error body still lets us classify by status.
            Ok(Err(_)) | Err(_) => break,
        }
    }
    buf.truncate(cap);
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Parse an SSE `data:` payload as JSON; malformed payloads are skipped with a debug log.
pub(crate) fn parse_json(data: &str) -> Option<Value> {
    match serde_json::from_str::<Value>(data) {
        Ok(v) => Some(v),
        Err(e) => {
            tracing::debug!(error = %e, data = %truncate_chars(data, 200), "skipping malformed SSE payload");
            None
        }
    }
}

pub(crate) fn u64_at(v: &Value, ptr: &str) -> Option<u64> {
    v.pointer(ptr).and_then(|x| x.as_u64().or_else(|| x.as_f64().map(|f| f.max(0.0) as u64)))
}

pub(crate) fn str_at<'a>(v: &'a Value, ptr: &str) -> Option<&'a str> {
    v.pointer(ptr).and_then(Value::as_str)
}

/// Test helper: decode a raw SSE text and run it through a parser.
#[cfg(test)]
pub(crate) fn run_parser(parser: &mut dyn Parser, raw: &str) -> Result<(Vec<StreamEvent>, FinishReason), LlmError> {
    let mut dec = SseDecoder::new();
    let mut events = Vec::new();
    for ev in dec.push(raw.as_bytes()).into_iter().chain(dec.finish()) {
        events.extend(parser.feed(&ev)?);
        if parser.done() {
            break;
        }
    }
    let (rest, reason) = parser.finish()?;
    events.extend(rest);
    Ok((events, reason))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(id: &str) -> AssistantPart {
        AssistantPart::ToolCall { id: id.into(), name: "read".into(), arguments: "{}".into() }
    }
    fn result(id: &str) -> LlmMessage {
        LlmMessage::ToolResult { call_id: id.into(), name: "read".into(), content: "ok".into(), images: vec![], is_error: false }
    }

    #[test]
    fn normalize_inserts_synthetic_and_drops_orphans() {
        let msgs = vec![
            LlmMessage::User(vec![Content::Text("hi".into())]),
            LlmMessage::Assistant { parts: vec![AssistantPart::Text(" ".into())], replay: true },
            LlmMessage::Assistant { parts: vec![call("a"), call("b")], replay: true },
            result("b"),
            result("b"),
            result("zzz"),
            LlmMessage::User(vec![Content::Text("next".into())]),
            LlmMessage::Assistant { parts: vec![call("c")], replay: true },
        ];
        let out = normalize(&msgs);
        let kinds: Vec<String> = out
            .iter()
            .map(|m| match m {
                LlmMessage::User(_) => "user".to_string(),
                LlmMessage::Assistant { .. } => "assistant".to_string(),
                LlmMessage::ToolResult { call_id, is_error, .. } => format!("result:{call_id}:{is_error}"),
            })
            .collect();
        assert_eq!(
            kinds,
            ["user", "assistant", "result:b:false", "result:a:true", "user", "assistant", "result:c:true"]
        );
    }

    #[test]
    fn sanitize_ids() {
        assert_eq!(sanitize_id("toolu_01Ab-c", 64), "toolu_01Ab-c");
        assert_eq!(sanitize_id("call|fc_1", 64), "call_fc_1");
        let long = "x".repeat(100);
        let s = sanitize_id(&long, 40);
        assert_eq!(s.len(), 40);
        assert_eq!(s, sanitize_id(&long, 40));
        assert_ne!(s, sanitize_id(&"x".repeat(101), 40));
    }

    #[test]
    fn snake() {
        assert_eq!(snake_case("serviceTier"), "service_tier");
        assert_eq!(snake_case("already_snake"), "already_snake");
    }
}
