//! Error type and provider-failure classification.
//!
//! Classification follows opencode's `provider-error.ts` with one product-driven deviation:
//! long-running agent loops must not die on transient trouble, so anything that is not
//! affirmatively an auth / billing / invalid-request / content-policy failure is
//! [`ErrorKind::Retryable`] (the caller caps attempts). In particular a 429 is only fatal when
//! it carries an explicit billing signal (`insufficient_quota`, "insufficient balance", ...),
//! not for generic "quota exceeded" / "usage limit" wording, which usually resets.

use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// Retry with backoff (network, 429, 5xx, overloaded, incomplete stream, unknown).
    Retryable,
    /// Prompt exceeds the context window: compact then retry.
    ContextOverflow,
    /// Auth, quota, invalid request: surface to the user, stop the loop.
    Fatal,
    /// Cancelled by the user.
    Aborted,
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct LlmError {
    pub kind: ErrorKind,
    pub status: Option<u16>,
    pub message: String,
    /// Server-requested delay (retry-after / retry-after-ms).
    pub retry_after: Option<Duration>,
    /// True if any output was already streamed to the caller before the failure.
    pub partial: bool,
}

impl LlmError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        LlmError { kind, status: None, message: message.into(), retry_after: None, partial: false }
    }

    /// The user cancelled the request.
    pub fn aborted() -> Self {
        Self::new(ErrorKind::Aborted, "request aborted")
    }

    /// The stream ended without its terminal event (retryable).
    pub fn incomplete(what: &str) -> Self {
        Self::new(ErrorKind::Retryable, format!("stream ended before completion: {what}"))
    }

    /// Build an error from an HTTP status, response headers and (possibly JSON) body.
    pub fn from_http(status: u16, headers: &reqwest::header::HeaderMap, body: &str) -> Self {
        let mut kind = classify(Some(status), body);
        // `x-should-retry` overrides everything except overflow (compaction is the better fix).
        if kind != ErrorKind::ContextOverflow {
            match header_str(headers, "x-should-retry") {
                Some("true") => kind = ErrorKind::Retryable,
                Some("false") => kind = ErrorKind::Fatal,
                _ => {}
            }
        }
        let detail = error_message(body).unwrap_or_else(|| {
            let t = body.trim();
            if t.is_empty() { "(no body)".to_string() } else { truncate(t, 1000) }
        });
        LlmError {
            kind,
            status: Some(status),
            message: format!("HTTP {status}: {detail}"),
            retry_after: retry_after(headers),
            partial: false,
        }
    }

    /// Build an error from an error payload received inside the stream (no HTTP status).
    pub fn from_stream_payload(payload: &str) -> Self {
        let kind = classify(None, payload);
        let detail = error_message(payload).unwrap_or_else(|| truncate(payload.trim(), 1000));
        LlmError::new(kind, format!("provider error: {detail}"))
    }

    /// Map a transport error. Everything is retryable except malformed requests (bad URL etc.).
    pub fn from_reqwest(e: &reqwest::Error) -> Self {
        let kind = if e.is_builder() { ErrorKind::Fatal } else { ErrorKind::Retryable };
        let what = if e.is_timeout() {
            "timeout"
        } else if e.is_connect() {
            "connection error"
        } else if e.is_body() || e.is_decode() {
            "response body error"
        } else {
            "network error"
        };
        LlmError::new(kind, format!("{what}: {}", error_chain(e)))
    }

    pub(crate) fn with_partial(mut self, partial: bool) -> Self {
        self.partial |= partial;
        self
    }
}

fn error_chain(e: &dyn std::error::Error) -> String {
    let mut s = e.to_string();
    let mut src = e.source();
    while let Some(inner) = src {
        let t = inner.to_string();
        if !s.contains(&t) {
            s.push_str(": ");
            s.push_str(&t);
        }
        src = inner.source();
    }
    s
}

fn truncate(s: &str, max_chars: usize) -> String {
    match s.char_indices().nth(max_chars) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

fn header_str<'a>(headers: &'a reqwest::header::HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok()).map(str::trim)
}

/// Parse `retry-after-ms`, then `retry-after` (seconds or HTTP date).
pub fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    if let Some(ms) = header_str(headers, "retry-after-ms").and_then(|v| v.parse::<f64>().ok()) {
        if ms.is_finite() && ms >= 0.0 {
            return Some(Duration::from_millis(ms as u64));
        }
    }
    let v = header_str(headers, "retry-after")?;
    parse_retry_after(v, chrono::Utc::now())
}

fn parse_retry_after(v: &str, now: chrono::DateTime<chrono::Utc>) -> Option<Duration> {
    if let Ok(secs) = v.parse::<f64>() {
        return (secs.is_finite() && secs >= 0.0).then(|| Duration::from_secs_f64(secs));
    }
    let date = chrono::DateTime::parse_from_rfc2822(v).ok()?;
    let delta = date.with_timezone(&chrono::Utc) - now;
    Some(delta.to_std().unwrap_or(Duration::ZERO))
}

static OVERFLOW: LazyLock<Regex> = LazyLock::new(|| {
    let patterns = [
        r"prompt (?:is )?too long",
        r"prompt (?:exceeds max length|超长)",
        r"request_too_large",
        r"input is too long for (?:the )?(?:requested|selected) model",
        r"too large for the model'?s configured input limit",
        r"exceeded this model context window limit",
        r"exceeds the context window",
        r"exceeds (?:the )?(?:model'?s )?maximum context length(?: of [\d,]+ tokens?|\s*\([\d,]+\))",
        r"context window exceeded",
        r"input token count.*exceeds the maximum",
        r"number of input tokens exceeds maximum length",
        r"tokens in request more than max tokens allowed",
        r"maximum prompt length is \d+",
        r"reduce the length of the messages",
        r"requested input length \d+ exceeds maximum input length",
        r"maximum context length is \d+ tokens",
        r"exceeds (?:the )?maximum allowed input length of [\d,]+ tokens?",
        r"input(?: \(\d+ tokens\))? is longer than the model'?s context length",
        r"exceeds the limit of \d+",
        r"exceeds the available context size",
        r"greater than the context length",
        r"`inputs` tokens \+ `max_new_tokens` must be <= \d+",
        r"context window exceeds limit",
        r"exceeded model token limit",
        r"context[_ ]length[_ ]exceeded",
        r"context length is only \d+ tokens",
        r"input length.*exceeds.*context length",
        r"too large for model with \d+ maximum context length",
        r"prompt has [\d,]+ tokens?, but the configured context size is",
        r"prompt too long; exceeded (?:max )?context length",
        r"model_context_window_exceeded",
        r"range of input length should be",
        r"too many tokens",
        r"token limit exceeded",
        r"^4(?:00|13)\s*(?:status code)?\s*\(no body\)",
    ];
    Regex::new(&format!("(?im)(?:{})", patterns.join("|"))).expect("overflow regex")
});

static OVERFLOW_EXCLUDE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?im)^(?:throttling error|service unavailable):|rate limit|too many requests|max[_ ]tokens must be less than|too many tokens(?: per day)?, please wait",
    )
    .expect("exclude regex")
});

static RATE_LIMIT_TEXT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)rate increased too quickly|rate[-_\s]?limit|too[_\s]?many[_\s]?requests|throttl|too many tokens(?: per day)?, please wait")
        .expect("rate regex")
});

/// Explicit, non-transient billing / account signals (fatal even on 429).
static BILLING_TEXT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)insufficient[-_\s]?(?:quota|balance|credits?|funds)|billing[-_\s]?(?:error|details|hard limit)|out of (?:budget|credits)|package has expired|plan does not yet include|subscription_sharing_\w*usage_limit_exceeded|account (?:is |has been )?(?:suspended|deactivated|disabled)",
    )
    .expect("billing regex")
});

static AUTH_TEXT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)incorrect api key provided|invalid[-_\s]?api[-_\s]?key").expect("auth regex"));

static CONTENT_POLICY_TEXT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)violating our usage policy|blocked by content filtering policy|content[-_\s]?policy|rejected (?:as a result of our|by the) safety system|detected potentially unsafe or sensitive content",
    )
    .expect("policy regex")
});

/// Transient wording that some gateways attach even to 4xx statuses.
static TRANSIENT_TEXT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(?:overloaded|at capacity|temporarily unavailable|service[-_\s]?unavailable|server (?:is )?busy|(?:server|internal)[-_\s]?error|try again|retry (?:the |this |your )?request|provider returned (?:an )?error|resource[-_\s]?exhausted|upstream (?:connect|connection|request))\b",
    )
    .expect("transient regex")
});

const OVERFLOW_CODES: &[&str] = &["context_length_exceeded", "model_context_window_exceeded", "request_too_large"];
const QUOTA_CODES: &[&str] = &[
    "insufficient_quota",
    "usage_not_included",
    "billing_error",
    "billing_not_active",
    "gousagelimiterror",
    "budgetlimiterror",
    "freeusagelimiterror",
    "creditlimitexceeded",
];
const AUTH_CODES: &[&str] =
    &["authentication_error", "permission_error", "permission_denied", "api_key_invalid", "invalid_api_key"];
const POLICY_CODES: &[&str] = &[
    "content_filter",
    "content_moderation",
    "responsibleaipolicyviolation",
    "content_policy_violation",
    "image_content_policy_violation",
    "datainspectionfailed",
    "moderation_blocked",
];
const INVALID_CODES: &[&str] =
    &["model_not_found", "invalid_prompt", "invalid_request", "invalid_request_error", "validationexception"];
const SERVER_CODES: &[&str] = &[
    "api_error",
    "internal_error",
    "overloaded_error",
    "server_error",
    "server_is_overloaded",
    "slow_down",
    "serviceunavailableexception",
    "internalserverexception",
    "modelstreamerrorexception",
    "timeout_error",
];

/// Error codes found in common JSON error layouts (lowercased).
fn error_codes(json: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let mut push = |v: Option<&Value>| match v {
        Some(Value::String(s)) => {
            // AWS shapes: `ns#Name:uri` → `Name`.
            let s = s.split(':').next().unwrap_or(s);
            let s = s.rsplit('#').next().unwrap_or(s);
            out.push(s.to_lowercase());
        }
        Some(Value::Number(n)) => out.push(n.to_string()),
        _ => {}
    };
    push(json.get("code"));
    push(json.get("__type"));
    push(json.get("error_type"));
    if let Some(err) = json.get("error").filter(|e| e.is_object()) {
        push(err.get("code"));
        push(err.get("type"));
        push(err.get("status"));
        push(err.get("error_type"));
        push(err.pointer("/innererror/code"));
        push(err.pointer("/metadata/error_type"));
        if let Some(details) = err.get("details").and_then(Value::as_array) {
            for d in details {
                push(d.get("reason"));
            }
        }
    }
    push(json.pointer("/response/error/code"));
    push(json.pointer("/response/error_type"));
    out
}

/// Human-readable message from common error layouts.
pub fn error_message(body: &str) -> Option<String> {
    let json: Value = serde_json::from_str(body.trim()).ok()?;
    let candidates = [
        json.pointer("/error/message"),
        json.get("error").filter(|v| v.is_string()),
        json.get("message"),
        json.get("Message"),
        json.get("detail"),
        json.pointer("/errors/0/message"),
        json.pointer("/response/error/message"),
    ];
    let msg = candidates.into_iter().flatten().find_map(|v| v.as_str().filter(|s| !s.trim().is_empty()))?;
    let mut msg = truncate(msg.trim(), 1000);
    // OpenRouter forwards the upstream body in error.metadata.raw.
    if let Some(raw) = json.pointer("/error/metadata/raw").and_then(Value::as_str) {
        msg.push_str(" (upstream: ");
        msg.push_str(&truncate(raw.trim(), 500));
        msg.push(')');
    }
    Some(msg)
}

/// Classify an HTTP status + body text. `status == None` means a mid-stream error payload
/// or a transport failure.
pub fn classify(status: Option<u16>, body: &str) -> ErrorKind {
    let json: Value = serde_json::from_str(body.trim()).unwrap_or(Value::Null);
    let codes = error_codes(&json);
    let has = |list: &[&str]| codes.iter().any(|c| list.contains(&c.as_str()));
    let mut text = body.to_string();
    if let Some(m) = error_message(body) {
        text.push('\n');
        text.push_str(&m);
    }
    let client_scoped = status.is_none_or(|s| (400..500).contains(&s));

    // 1. Context overflow → compaction. 429 is always a throttle, never overflow.
    if client_scoped && status != Some(429) && !codes.iter().any(|c| c == "throttlingexception") {
        let overflow_text = OVERFLOW.is_match(&text) && !OVERFLOW_EXCLUDE.is_match(&text);
        if has(OVERFLOW_CODES) || overflow_text || status == Some(413) {
            return ErrorKind::ContextOverflow;
        }
    }
    // 2. Content policy.
    if has(POLICY_CODES) || (client_scoped && CONTENT_POLICY_TEXT.is_match(&text)) {
        return ErrorKind::Fatal;
    }
    // 3. Billing / quota (explicit signals only).
    if status == Some(402) || has(QUOTA_CODES) || BILLING_TEXT.is_match(&text) {
        return ErrorKind::Fatal;
    }
    // 4. Authentication.
    if matches!(status, Some(401 | 403)) || has(AUTH_CODES) || (status == Some(400) && AUTH_TEXT.is_match(&text)) {
        return ErrorKind::Fatal;
    }
    // 5. Rate limits (any status, including generic quota wording on 429).
    if status == Some(429)
        || codes.iter().any(|c| c.contains("rate_limit") || c == "too_many_requests" || c == "throttlingexception")
        || RATE_LIMIT_TEXT.is_match(&text)
    {
        return ErrorKind::Retryable;
    }
    // 6. Server-side / transient.
    match status {
        Some(408 | 409 | 425) => return ErrorKind::Retryable,
        Some(s) if s >= 500 => return ErrorKind::Retryable,
        _ => {}
    }
    if has(SERVER_CODES) || codes.iter().any(|c| c.contains("exhausted") || c.contains("unavailable")) {
        return ErrorKind::Retryable;
    }
    if TRANSIENT_TEXT.is_match(&text) && !has(INVALID_CODES) {
        return ErrorKind::Retryable;
    }
    // 7. Deterministic request rejections.
    if has(INVALID_CODES) || (client_scoped && status.is_some()) {
        return ErrorKind::Fatal;
    }
    // 8. Unknown → retry.
    ErrorKind::Retryable
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue};

    #[test]
    fn overflow() {
        let anthropic = r#"{"type":"error","error":{"type":"invalid_request_error","message":"prompt is too long: 213462 tokens > 200000 maximum"}}"#;
        assert_eq!(classify(Some(400), anthropic), ErrorKind::ContextOverflow);
        let openai = r#"{"error":{"message":"Your input exceeds the context window of this model.","code":"context_length_exceeded"}}"#;
        assert_eq!(classify(Some(400), openai), ErrorKind::ContextOverflow);
        assert_eq!(classify(None, "Prompt 超长"), ErrorKind::ContextOverflow);
        assert_eq!(classify(Some(413), ""), ErrorKind::ContextOverflow);
        // 429 wording that looks like overflow is a throttle.
        assert_eq!(classify(Some(429), "too many tokens processed"), ErrorKind::Retryable);
        assert_eq!(classify(Some(400), "Too many tokens, please wait before trying again."), ErrorKind::Retryable);
    }

    #[test]
    fn rate_limits_and_quota() {
        assert_eq!(classify(Some(429), r#"{"error":{"message":"Rate limit reached"}}"#), ErrorKind::Retryable);
        assert_eq!(classify(Some(429), "usage limit reached, resets in 5h"), ErrorKind::Retryable);
        assert_eq!(classify(Some(429), "quota exceeded for this minute"), ErrorKind::Retryable);
        let insufficient = r#"{"error":{"message":"You exceeded your current quota, please check your plan and billing details.","type":"insufficient_quota","code":"insufficient_quota"}}"#;
        assert_eq!(classify(Some(429), insufficient), ErrorKind::Fatal);
        assert_eq!(classify(Some(429), r#"{"error":{"code":"1113","message":"Insufficient balance"}}"#), ErrorKind::Fatal);
        assert_eq!(classify(Some(402), "payment required"), ErrorKind::Fatal);
    }

    #[test]
    fn auth_and_invalid() {
        assert_eq!(classify(Some(401), r#"{"error":{"message":"invalid x-api-key"}}"#), ErrorKind::Fatal);
        assert_eq!(classify(Some(403), ""), ErrorKind::Fatal);
        assert_eq!(classify(Some(400), r#"{"error":{"message":"unknown field foo"}}"#), ErrorKind::Fatal);
        assert_eq!(classify(Some(404), "model not found"), ErrorKind::Fatal);
    }

    #[test]
    fn server_and_unknown() {
        assert_eq!(classify(Some(500), "Internal Server Error"), ErrorKind::Retryable);
        assert_eq!(classify(Some(529), r#"{"type":"error","error":{"type":"overloaded_error"}}"#), ErrorKind::Retryable);
        assert_eq!(classify(None, r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#), ErrorKind::Retryable);
        assert_eq!(classify(None, "something weird"), ErrorKind::Retryable);
        assert_eq!(classify(Some(400), "model is overloaded, try again later"), ErrorKind::Retryable);
        assert_eq!(classify(Some(408), ""), ErrorKind::Retryable);
    }

    #[test]
    fn retry_after_parsing() {
        let mut h = HeaderMap::new();
        h.insert("retry-after", HeaderValue::from_static("7"));
        assert_eq!(retry_after(&h), Some(Duration::from_secs(7)));
        h.insert("retry-after-ms", HeaderValue::from_static("1500"));
        assert_eq!(retry_after(&h), Some(Duration::from_millis(1500)));
        let now = chrono::DateTime::parse_from_rfc2822("Wed, 21 Oct 2015 07:28:00 GMT").unwrap().to_utc();
        assert_eq!(parse_retry_after("Wed, 21 Oct 2015 07:28:30 GMT", now), Some(Duration::from_secs(30)));
        assert_eq!(parse_retry_after("garbage", now), None);
    }

    #[test]
    fn http_error_message_and_override() {
        let mut h = HeaderMap::new();
        h.insert("x-should-retry", HeaderValue::from_static("true"));
        let e = LlmError::from_http(400, &h, r#"{"error":{"message":"bad thing"}}"#);
        assert_eq!(e.kind, ErrorKind::Retryable);
        assert_eq!(e.message, "HTTP 400: bad thing");
        assert_eq!(e.status, Some(400));
    }
}
