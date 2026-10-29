//! HTTP plumbing shared by all providers: request sending with timeouts, error mapping and the
//! SSE → [`StreamEvent`] driver.

use std::collections::VecDeque;
use std::pin::Pin;
use std::time::Duration;

use bytes::Bytes;
use futures::{Stream, StreamExt};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;

use crate::sse::{SseEvent, SseParser};
use crate::{EventStream, LlmError, ModelSpec, Request, StreamEvent};

pub(crate) const USER_AGENT: &str = concat!("ref/", env!("CARGO_PKG_VERSION"));

/// Max bytes of an error body kept in `LlmError::Http`.
const MAX_ERROR_BODY: usize = 16 * 1024;

/// Result of feeding one SSE event to a decoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Flow {
    Continue,
    /// A terminal event was seen; the stream is complete.
    Done,
}

/// Provider-specific translation of SSE events into [`StreamEvent`]s.
pub(crate) trait Decoder: Send + 'static {
    fn event(&mut self, ev: SseEvent, out: &mut Vec<StreamEvent>) -> Result<Flow, LlmError>;
    /// The byte stream ended before `Flow::Done`. Either finish gracefully or return an error.
    fn eof(&mut self, out: &mut Vec<StreamEvent>) -> Result<(), LlmError>;
}

/// Synchronous core of the stream driver: bytes in, events out. Shared by the async driver and
/// the unit tests.
pub(crate) struct Feeder<D> {
    parser: SseParser,
    decoder: D,
    pub(crate) queue: VecDeque<Result<StreamEvent, LlmError>>,
    pub(crate) done: bool,
}

impl<D: Decoder> Feeder<D> {
    pub(crate) fn new(decoder: D) -> Self {
        Feeder { parser: SseParser::new(), decoder, queue: VecDeque::new(), done: false }
    }

    pub(crate) fn push(&mut self, chunk: &[u8]) {
        if self.done {
            return;
        }
        let events = self.parser.push(chunk);
        self.feed(events);
    }

    pub(crate) fn finish(&mut self) {
        if self.done {
            return;
        }
        if let Some(ev) = self.parser.finish() {
            self.feed(vec![ev]);
        }
        if !self.done {
            self.done = true;
            let mut out = Vec::new();
            let r = self.decoder.eof(&mut out);
            self.queue.extend(out.into_iter().map(Ok));
            if let Err(e) = r {
                self.queue.push_back(Err(e));
            }
        }
    }

    pub(crate) fn fail(&mut self, err: LlmError) {
        if !self.done {
            self.done = true;
            self.queue.push_back(Err(err));
        }
    }

    fn feed(&mut self, events: Vec<SseEvent>) {
        for ev in events {
            if self.done {
                break;
            }
            let mut out = Vec::new();
            let r = self.decoder.event(ev, &mut out);
            self.queue.extend(out.into_iter().map(Ok));
            match r {
                Ok(Flow::Continue) => {}
                Ok(Flow::Done) => self.done = true,
                Err(e) => {
                    self.done = true;
                    self.queue.push_back(Err(e));
                }
            }
        }
    }
}

type ByteStream = Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>;

struct DriverState<D> {
    body: ByteStream,
    feeder: Feeder<D>,
    idle: Duration,
}

/// Turn a response body into an [`EventStream`] using `decoder`, enforcing `idle` as the maximum
/// gap between chunks (zero disables the check).
pub(crate) fn drive<D: Decoder>(body: ByteStream, decoder: D, idle: Duration) -> EventStream {
    let state = DriverState { body, feeder: Feeder::new(decoder), idle };
    Box::pin(futures::stream::unfold(state, |mut st| async move {
        loop {
            if let Some(item) = st.feeder.queue.pop_front() {
                return Some((item, st));
            }
            if st.feeder.done {
                return None;
            }
            let next = if st.idle.is_zero() {
                st.body.next().await
            } else {
                match tokio::time::timeout(st.idle, st.body.next()).await {
                    Ok(n) => n,
                    Err(_) => {
                        st.feeder.fail(LlmError::Timeout(format!(
                            "no data from provider for {}s",
                            st.idle.as_secs_f64()
                        )));
                        continue;
                    }
                }
            };
            match next {
                Some(Ok(bytes)) => st.feeder.push(&bytes),
                Some(Err(e)) => st.feeder.fail(LlmError::Network(format!("reading response stream: {e}"))),
                None => st.feeder.finish(),
            }
        }
    }))
}

/// Builder for request headers: later inserts override earlier ones (case-insensitive).
#[derive(Default)]
pub(crate) struct Headers {
    map: HeaderMap,
}

impl Headers {
    pub(crate) fn new() -> Self {
        let mut h = Headers::default();
        h.set("content-type", "application/json");
        h.set("accept", "text/event-stream");
        h.set("user-agent", USER_AGENT);
        h
    }

    /// Insert a header known to be valid (static names / sanitized values).
    pub(crate) fn set(&mut self, name: &str, value: &str) {
        // Values coming from user data (keys, session ids) could contain invalid bytes; skip them
        // rather than failing the whole request.
        if let (Ok(n), Ok(v)) = (HeaderName::from_bytes(name.as_bytes()), HeaderValue::from_str(value)) {
            self.map.insert(n, v);
        }
    }

    #[cfg(test)]
    pub(crate) fn get(&self, name: &str) -> Option<&str> {
        self.map.get(name).and_then(|v| v.to_str().ok())
    }

    /// Apply user-configured headers (override everything set before).
    pub(crate) fn apply_user(&mut self, model: &ModelSpec, skip: &[&str]) -> Result<(), LlmError> {
        for (k, v) in &model.headers {
            if skip.iter().any(|s| s.eq_ignore_ascii_case(k)) {
                continue;
            }
            let n = HeaderName::from_bytes(k.as_bytes())
                .map_err(|_| LlmError::Config(format!("invalid header name {k:?}")))?;
            let v = HeaderValue::from_str(v).map_err(|_| LlmError::Config(format!("invalid value for header {k:?}")))?;
            self.map.insert(n, v);
        }
        Ok(())
    }

    /// Look up a user-configured header case-insensitively.
    pub(crate) fn user_header<'a>(model: &'a ModelSpec, name: &str) -> Option<&'a str> {
        model.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    /// All headers as (name, value) with credential values replaced by `<redacted>`.
    pub(crate) fn redacted(&self) -> Vec<(String, String)> {
        const SECRET: &[&str] = &["authorization", "x-api-key", "api-key", "proxy-authorization", "cookie"];
        self.map
            .iter()
            .map(|(k, v)| {
                let name = k.as_str().to_string();
                let value = if SECRET.contains(&name.as_str()) || name.contains("token") || name.contains("secret") {
                    "<redacted>".to_string()
                } else {
                    v.to_str().unwrap_or("<binary>").to_string()
                };
                (name, value)
            })
            .collect()
    }

    pub(crate) fn into_map(self) -> HeaderMap {
        self.map
    }
}

/// POST `body` to `url` and return the event stream produced by `decoder`.
pub(crate) async fn post_sse<D: Decoder>(
    http: &reqwest::Client,
    model: &ModelSpec,
    url: &str,
    headers: Headers,
    body: &Value,
    decoder: D,
) -> Result<EventStream, LlmError> {
    let payload = serde_json::to_vec(body).map_err(|e| LlmError::Config(format!("serializing request: {e}")))?;
    tracing::debug!(target: "ref_llm", url, model = %model.api_model, bytes = payload.len(), "llm request");
    let send = http.post(url).headers(headers.into_map()).body(payload).send();
    let resp = if model.header_timeout.is_zero() {
        send.await
    } else {
        tokio::time::timeout(model.header_timeout, send).await.map_err(|_| {
            LlmError::Timeout(format!("no response headers after {}s", model.header_timeout.as_secs_f64()))
        })?
    }
    .map_err(|e| LlmError::Network(describe_reqwest_error(&e)))?;

    let status = resp.status();
    if !status.is_success() {
        let retry_after = crate::error::parse_retry_after(resp.headers());
        let body = read_error_body(resp, model.chunk_timeout).await;
        return Err(LlmError::Http { status: status.as_u16(), body, retry_after });
    }

    // Some gateways answer 200 with a JSON error instead of a stream.
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if content_type.starts_with("application/json") {
        let body = read_error_body(resp, model.chunk_timeout).await;
        return Err(LlmError::Stream(format!(
            "expected an event stream, got JSON: {}",
            crate::error::readable_body(&body)
        )));
    }

    Ok(drive(Box::pin(resp.bytes_stream()), decoder, model.chunk_timeout))
}

async fn read_error_body(resp: reqwest::Response, idle: Duration) -> String {
    let limit = if idle.is_zero() { Duration::from_secs(60) } else { idle };
    let text = match tokio::time::timeout(limit, resp.text()).await {
        Ok(Ok(t)) => t,
        Ok(Err(e)) => format!("(failed to read body: {e})"),
        Err(_) => "(timed out reading body)".to_string(),
    };
    truncate_str(text.trim(), MAX_ERROR_BODY).to_string()
}

pub(crate) fn truncate_str(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn describe_reqwest_error(e: &reqwest::Error) -> String {
    // reqwest's Display omits the root cause (e.g. "connection refused"); include the chain.
    let mut msg = e.to_string();
    let mut src = std::error::Error::source(e);
    while let Some(s) = src {
        let s_msg = s.to_string();
        if !msg.contains(&s_msg) {
            msg.push_str(": ");
            msg.push_str(&s_msg);
        }
        src = s.source();
    }
    msg
}

/// Deep-merge `src` into `dst` (objects merge recursively, everything else is replaced).
pub(crate) fn merge_json(dst: &mut serde_json::Map<String, Value>, src: &serde_json::Map<String, Value>) {
    for (k, v) in src {
        match (dst.get_mut(k), v) {
            (Some(Value::Object(d)), Value::Object(s)) => merge_json(d, s),
            _ => {
                dst.insert(k.clone(), v.clone());
            }
        }
    }
}

/// Upper bound for requested output tokens (no reply needs more; opencode uses the same cap).
const OUTPUT_TOKEN_MAX: u64 = 256_000;
/// Never request fewer output tokens than this, even when the prompt nearly fills the window.
const OUTPUT_TOKEN_MIN: u64 = 1_024;
/// Rough token cost of one image.
const IMAGE_TOKENS: u64 = 1_600;

/// Conservative token estimate of a request (ASCII ≈ 4 chars/token, other chars ≈ 1 token each,
/// +15% error margin). Used only to keep `max_tokens` inside the context window.
pub(crate) fn estimate_tokens(req: &Request) -> u64 {
    fn text(s: &str, acc: &mut (u64, u64)) {
        for c in s.chars() {
            if c.is_ascii() {
                acc.0 += 1;
            } else {
                acc.1 += 1;
            }
        }
    }
    let mut acc = (0u64, 0u64);
    let mut images = 0u64;
    for s in &req.system {
        text(s, &mut acc);
    }
    for t in &req.tools {
        text(&t.name, &mut acc);
        text(&t.description, &mut acc);
        text(&t.parameters.to_string(), &mut acc);
    }
    for m in &req.messages {
        for p in &m.parts {
            match p {
                ref_proto::Part::Text(t) => text(&t.text, &mut acc),
                ref_proto::Part::Reasoning(r) => text(&r.text, &mut acc),
                ref_proto::Part::Image(_) => images += 1,
                ref_proto::Part::Tool(t) => {
                    text(&t.input.to_string(), &mut acc);
                    text(&t.input_raw, &mut acc);
                    text(&t.output, &mut acc);
                    images += t.images.len() as u64;
                }
            }
        }
    }
    let base = acc.0.div_ceil(4) + acc.1 + images * IMAGE_TOKENS;
    base + base * 15 / 100
}

/// Output token limit to request: the model's limit (capped at 256k), reduced so that prompt +
/// output fit in the context window (providers such as vLLM / Kimi reject `max_tokens` larger
/// than the remaining room). Returns 0 when the model has no known output limit.
pub(crate) fn fit_max_tokens(model: &ModelSpec, req: &Request) -> u64 {
    if model.max_output == 0 {
        return 0;
    }
    let requested = model.max_output.min(OUTPUT_TOKEN_MAX);
    if model.context == 0 {
        return requested;
    }
    let room = model.context.saturating_sub(estimate_tokens(req));
    requested.min(room.max(OUTPUT_TOKEN_MIN))
}

/// Host part of a base URL, lowercased (`https://api.openai.com/v1` → `api.openai.com`).
pub(crate) fn host_of(base_url: &str) -> String {
    let rest = base_url.split_once("://").map(|(_, r)| r).unwrap_or(base_url);
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit_once('@').map(|(_, h)| h).unwrap_or(host);
    let host = host.split(':').next().unwrap_or("");
    host.to_ascii_lowercase()
}

pub(crate) fn endpoint(model: &ModelSpec, path: &str) -> Result<String, LlmError> {
    let base = model.base_url.trim_end_matches('/');
    if base.is_empty() {
        return Err(LlmError::Config(format!("model {} has no base URL", model.full_id())));
    }
    Ok(format!("{base}/{path}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts() {
        assert_eq!(host_of("https://api.openai.com/v1"), "api.openai.com");
        assert_eq!(host_of("http://user:pw@LocalHost:8080/x"), "localhost");
        assert_eq!(host_of("openrouter.ai/api/v1"), "openrouter.ai");
    }

    #[test]
    fn merge() {
        let mut a = serde_json::json!({"a": {"x": 1, "y": 2}, "b": 1}).as_object().unwrap().clone();
        let b = serde_json::json!({"a": {"y": 3}, "c": [1]}).as_object().unwrap().clone();
        merge_json(&mut a, &b);
        assert_eq!(Value::Object(a), serde_json::json!({"a": {"x": 1, "y": 3}, "b": 1, "c": [1]}));
    }
}
