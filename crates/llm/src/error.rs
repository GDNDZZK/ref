//! Error classification: HTTP failures, transport problems and in-stream
//! provider errors are mapped onto [`LlmError`] with `retryable`,
//! `retry_after` and `context_overflow` filled in.

use crate::LlmError;
use regex::Regex;
use reqwest::StatusCode;
use reqwest::header::HeaderMap;
use serde_json::Value;
use std::sync::LazyLock;
use std::time::Duration;

/// HTTP statuses the classification depends on.
const PAYLOAD_TOO_LARGE: u16 = StatusCode::PAYLOAD_TOO_LARGE.as_u16();
const TOO_MANY_REQUESTS: u16 = StatusCode::TOO_MANY_REQUESTS.as_u16();

/// Maximum number of body characters included in error messages.
const MAX_BODY_CHARS: usize = 2000;

impl LlmError {
    /// Plain, non-retryable error.
    pub(crate) fn fatal(message: impl Into<String>) -> Self {
        LlmError {
            message: message.into(),
            status: None,
            retryable: false,
            retry_after: None,
            context_overflow: false,
            cancelled: false,
        }
    }

    /// Transient error (network, stream cut, timeout); retrying may help.
    pub(crate) fn transient(message: impl Into<String>) -> Self {
        LlmError {
            retryable: true,
            ..LlmError::fatal(message)
        }
    }
}

/// Context-window overflow patterns collected from many providers.
static OVERFLOW_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)prompt (is )?too long",
        r"|prompt exceeds max length|prompt 超长",
        r"|request_too_large",
        r"|input is too long",
        r"|exceeds the context window",
        r"|exceeds (the )?(model'?s )?maximum context length",
        r"|maximum context length",
        r"|context window exceeded|context window exceeds limit",
        r"|input token count.*exceeds the maximum",
        r"|maximum prompt length is \d+",
        r"|reduce the length of the messages",
        r"|exceeds (the )?maximum allowed input length",
        r"|is longer than the model'?s context length",
        r"|exceeds the limit of \d+",
        r"|exceeds the available context size",
        r"|greater than the context length",
        r"|exceeded model token limit",
        r"|too large for model with \d+ maximum context length",
        r"|but the configured context size is",
        r"|model_context_window_exceeded",
        r"|exceeded (max )?context length",
        r"|range of input length should be",
        r"|requested input length \d+ exceeds maximum input length",
        r"|`inputs` tokens \+ `max_new_tokens` must be",
        r"|context[_ ]length[_ ]exceeded|too many tokens|token limit exceeded",
    ))
    .expect("overflow regex")
});

/// Messages that look like overflow but are actually throttling / parameter errors.
static OVERFLOW_EXCLUSION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)^(throttling error|service unavailable):",
        r"|rate.?limit|too many requests",
        r"|too many tokens(,)?( per day)?(,)? please wait",
        r"|max[_ ]tokens must be less than",
    ))
    .expect("overflow exclusion regex")
});

/// Whether an error text indicates that the prompt exceeded the context window.
pub fn is_context_overflow(text: &str) -> bool {
    OVERFLOW_RE.is_match(text) && !OVERFLOW_EXCLUSION_RE.is_match(text)
}

/// Truncate to [`MAX_BODY_CHARS`] characters (on a char boundary).
pub(crate) fn trim_body(body: &str) -> String {
    let body = body.trim();
    match body.char_indices().nth(MAX_BODY_CHARS) {
        Some((i, _)) => format!("{}…", &body[..i]),
        None => body.to_string(),
    }
}

/// Classify a non-2xx HTTP response.
pub fn classify_http_error(status: u16, headers: &HeaderMap, body: &str) -> LlmError {
    let trimmed = trim_body(body);
    let message = if trimmed.is_empty() {
        format!("HTTP {status}")
    } else {
        format!("HTTP {status}: {trimmed}")
    };

    let context_overflow =
        status == PAYLOAD_TOO_LARGE || (status != TOO_MANY_REQUESTS && is_context_overflow(body));
    let retryable = !context_overflow;

    LlmError {
        message,
        status: Some(status),
        retryable,
        retry_after: if retryable {
            parse_retry_after(headers).max(parse_retry_after_body(body))
        } else {
            None
        },
        context_overflow,
        cancelled: false,
    }
}

/// Classify an error reported inside the SSE stream (Anthropic `error` event,
/// Responses `response.failed` / `error`, Chat `{"error": {...}}` chunks).
/// `code` is the provider error code/type when available.
#[cfg(test)]
fn stream_error(code: Option<&str>, message: &str) -> LlmError {
    build_stream_error(code, message, None, &[])
}

/// Like `stream_error`, inspecting a scoped JSON error payload `err` and optional
/// top-level retry metadata values (ignoring echoed prompts, tools, and outputs).
pub(crate) fn stream_error_json(
    code: Option<&str>,
    message: &str,
    err: &Value,
    meta: &[&Value],
) -> LlmError {
    build_stream_error(code, message, Some(err), meta)
}

fn build_stream_error(
    code: Option<&str>,
    message: &str,
    err: Option<&Value>,
    meta: &[&Value],
) -> LlmError {
    let code = code.unwrap_or("").trim();
    let text = if code.is_empty() || message.contains(code) {
        message.trim().to_string()
    } else if message.trim().is_empty() {
        code.to_string()
    } else {
        format!("{} ({code})", message.trim())
    };
    let text = if text.is_empty() {
        "provider returned an error".to_string()
    } else {
        trim_body(&text)
    };

    let overflow = matches!(
        code,
        "context_length_exceeded" | "model_context_window_exceeded" | "request_too_large"
    ) || is_context_overflow(message);
    let retryable = !overflow;
    let retry_after = if retryable {
        let from_err = err.and_then(parse_retry_after_json);
        let from_meta = meta
            .iter()
            .filter_map(|m| parse_retry_after_top_level(m))
            .max();
        let from_message = parse_retry_after_body(message);
        from_err.max(from_meta).max(from_message)
    } else {
        None
    };
    LlmError {
        context_overflow: overflow,
        retryable,
        retry_after,
        ..LlmError::fatal(text)
    }
}

/// Error from the HTTP client while connecting / reading.
pub(crate) fn network_error(err: &reqwest::Error) -> LlmError {
    use std::error::Error as _;
    let mut msg = format!("network error: {err}");
    let mut source = err.source();
    while let Some(s) = source {
        let s_text = s.to_string();
        if !msg.contains(&s_text) {
            msg.push_str(": ");
            msg.push_str(&s_text);
        }
        source = s.source();
    }
    LlmError {
        status: err.status().map(|s| s.as_u16()),
        ..LlmError::transient(msg)
    }
}

/// Server-provided retry delay: `retry-after-ms`, `retry-after` (seconds or
/// HTTP-date), then OpenAI-style `x-ratelimit-reset-*` durations.
pub(crate) fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
    let get = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
    };

    if let Some(d) = get("retry-after-ms")
        .and_then(|v| v.parse::<f64>().ok())
        .and_then(|ms| secs_to_duration(ms / 1000.0))
    {
        return Some(d);
    }
    if let Some(v) = get("retry-after") {
        if let Ok(secs) = v.parse::<f64>() {
            if let Some(d) = secs_to_duration(secs) {
                return Some(d);
            }
        } else if let Some(d) = parse_timestamp_retry_after(v) {
            return Some(d);
        }
    }
    [
        "x-ratelimit-reset-requests",
        "x-ratelimit-reset-tokens",
        "x-ratelimit-reset",
    ]
    .iter()
    .filter_map(|h| get(h).and_then(parse_go_duration))
    .max()
}

/// A duration from seconds; `None` for negative or non-finite values. Values too large for a
/// `Duration` saturate to `Duration::MAX`; the retry loop caps the wait at its maximum delay.
fn secs_to_duration(secs: f64) -> Option<Duration> {
    if !secs.is_finite() || secs < 0.0 {
        return None;
    }
    Some(Duration::try_from_secs_f64(secs).unwrap_or(Duration::MAX))
}

/// Parse Go-style durations as used by OpenAI rate-limit headers
/// (`"1s"`, `"6m0s"`, `"20ms"`, `"1h2m3.5s"`) or plain seconds.
fn parse_go_duration(s: &str) -> Option<Duration> {
    if let Ok(secs) = s.parse::<f64>() {
        return secs_to_duration(secs);
    }
    let mut total = 0f64;
    let mut num = String::new();
    let mut chars = s.chars().peekable();
    let mut any = false;
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() || c == '.' {
            num.push(c);
            continue;
        }
        let value: f64 = num.parse().ok()?;
        num.clear();
        let factor = match c {
            'h' => 3600.0,
            'm' if chars.peek() == Some(&'s') => {
                chars.next();
                0.001
            }
            'm' => 60.0,
            's' => 1.0,
            _ => return None,
        };
        total += value * factor;
        any = true;
    }
    if any && num.is_empty() {
        secs_to_duration(total)
    } else {
        None
    }
}

fn parse_timestamp_retry_after(s: &str) -> Option<Duration> {
    let dt = chrono::DateTime::parse_from_rfc3339(s)
        .or_else(|_| chrono::DateTime::parse_from_rfc2822(s))
        .ok()?;
    let delta = dt.timestamp_millis() - chrono::Utc::now().timestamp_millis();
    Some(Duration::from_millis(delta.max(0) as u64))
}

/// Parse a `retry_after` / `retryDelay` string value: numeric seconds, duration with unit, or timestamp.
fn parse_retry_after_str(s: &str) -> Option<Duration> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(secs) = s.parse::<f64>() {
        return secs_to_duration(secs);
    }
    if let Some(d) = parse_duration_token(s) {
        return Some(d);
    }
    parse_timestamp_retry_after(s)
}

fn parse_json_ms(v: &Value) -> Option<Duration> {
    let ms = match v {
        Value::Number(n) => n.as_f64()?,
        Value::String(s) => s.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    secs_to_duration(ms / 1000.0)
}

fn parse_json_retry_after(v: &Value) -> Option<Duration> {
    match v {
        Value::Number(n) => n.as_f64().and_then(secs_to_duration),
        Value::String(s) => parse_retry_after_str(s),
        _ => None,
    }
}

const SKIP_JSON_KEYS: &[&str] = &[
    "instructions",
    "tools",
    "output",
    "input",
    "messages",
    "prompt",
    "choices",
    "candidates",
    "delta",
    "usage",
];

const ERROR_TEXT_KEYS: &[&str] = &[
    "message",
    "msg",
    "detail",
    "details",
    "error",
    "reason",
    "error_description",
    "title",
    "description",
    "text",
];

fn parse_structured_retry_field(key: &str, value: &Value) -> Option<Option<Duration>> {
    match key.to_ascii_lowercase().as_str() {
        "retry_after_ms" | "retry-after-ms" | "retryafterms" => Some(parse_json_ms(value)),
        "retry_after" | "retry-after" | "retryafter" | "retrydelay" | "retry_delay" => {
            Some(parse_json_retry_after(value))
        }
        _ => None,
    }
}

fn parse_retry_after_top_level(value: &Value) -> Option<Duration> {
    let Value::Object(map) = value else {
        return None;
    };
    map.iter()
        .filter_map(|(k, v)| parse_structured_retry_field(k, v).flatten())
        .max()
}

/// Extract the largest retry delay hint from a parsed JSON payload, checking structured retry
/// fields recursively and error-message text fields while skipping echoed request/output subtrees.
pub(crate) fn parse_retry_after_json(value: &Value) -> Option<Duration> {
    if let Value::String(s) = value {
        return parse_retry_after_text(s);
    }
    let mut best: Option<Duration> = None;
    collect_retry_after_json(value, false, &mut best);
    best
}

fn collect_json_field(key: &str, value: &Value, best: &mut Option<Duration>) {
    if let Some(hint) = parse_structured_retry_field(key, value) {
        *best = (*best).max(hint);
        return;
    }
    let lower = key.to_ascii_lowercase();
    if SKIP_JSON_KEYS.contains(&lower.as_str()) {
        return;
    }
    let allow_text = ERROR_TEXT_KEYS.contains(&lower.as_str());
    collect_retry_after_json(value, allow_text, best);
}

fn collect_retry_after_json(value: &Value, allow_text: bool, best: &mut Option<Duration>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                collect_json_field(k, v, best);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_retry_after_json(item, allow_text, best);
            }
        }
        Value::String(s) if allow_text => {
            *best = (*best).max(parse_retry_after_text(s));
        }
        _ => {}
    }
}

/// Extract a retry delay hint from an HTTP/error body (JSON fields or message text).
fn parse_retry_after_body(body: &str) -> Option<Duration> {
    if body.trim().is_empty() {
        return None;
    }
    if let Ok(val) = serde_json::from_str::<Value>(body) {
        return parse_retry_after_json(&val);
    }
    parse_retry_after_text(body)
}

const DURATION_RE_FRAG: &str = concat!(
    r"(?:",
    r"\d+(?:\.\d+)?(?:ms|h|m|s)(?:\s*\d+(?:\.\d+)?(?:ms|h|m|s))*(?-u:\b)",
    r"|",
    r"\d+(?:\.\d+)?\s*(?:milliseconds?|msecs?|ms|seconds?|secs?|s|minutes?|mins?|m|hours?|hrs?|h)(?-u:\b)",
    r"|",
    r"\d+(?:\.\d+)?\s*(?:毫秒|秒钟?|分钟?|小时)",
    r")",
);

static RETRY_PHRASE_RE: LazyLock<Regex> = LazyLock::new(|| {
    let en = format!(
        r"(?i)(?-u:\b)(?:(?:re)?try\s+again\s+(?:in|after)|retry\s+(?:in|after)|please\s+wait|available\s+in|resets?\s+in)\s+(?P<en>{DURATION_RE_FRAG})"
    );
    let wait_retry = format!(
        r"(?i)(?-u:\b)wait\s+(?P<wr>{DURATION_RE_FRAG})\s+(?:before\s+(?:re)?trying(?:\s+again)?|to\s+retry|and\s+(?:re)?try(?:\s+again)?)(?-u:\b)"
    );
    let zh_after = format!(r"(?:请在\s*)?(?P<zha>{DURATION_RE_FRAG})\s*(?:后重试|后再试)");
    let zh_wait = format!(r"请等待\s*(?P<zhw>{DURATION_RE_FRAG})");
    Regex::new(&format!("{en}|{wait_retry}|{zh_after}|{zh_wait}")).expect("retry phrase regex")
});

static RETRY_KV_PREFIX_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(?-u:\b)(?P<key>retry[-_ ]?after[-_ ]?ms|retry[-_ ]?after|retry[-_ ]?delay)(?P<sep>["']?\s*[:=]\s*|\s+)"#)
        .expect("retry kv prefix regex")
});

static LEADING_DURATION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"^(?i){DURATION_RE_FRAG}")).expect("leading duration regex")
});

static LEADING_FLOAT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([+-]?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)(?-u:\b)").expect("leading float regex")
});

static LEADING_RFC3339_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})")
        .expect("leading rfc3339 regex")
});

static LEADING_RFC2822_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?i)(?:Mon|Tue|Wed|Thu|Fri|Sat|Sun),\s+\d{1,2}\s+(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)\s+\d{4}\s+\d{2}:\d{2}:\d{2}\s+(?:GMT|UT|[+-]\d{4})")
        .expect("leading rfc2822 regex")
});

fn parse_duration_token(raw: &str) -> Option<Duration> {
    let trimmed = raw.trim();
    let compact: String = trimmed
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if compact.bytes().any(|b| b.is_ascii_alphabetic())
        && let Some(d) = parse_go_duration(&compact)
    {
        return Some(d);
    }
    let num_end = trimmed
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(trimmed.len());
    let num: f64 = trimmed[..num_end].parse().ok()?;
    let unit = trimmed[num_end..].trim().to_ascii_lowercase();
    let factor = match unit.as_str() {
        "ms" | "msec" | "msecs" | "millisecond" | "milliseconds" | "毫秒" => 0.001,
        "s" | "sec" | "secs" | "second" | "seconds" | "秒" | "秒钟" => 1.0,
        "m" | "min" | "mins" | "minute" | "minutes" | "分" | "分钟" => 60.0,
        "h" | "hr" | "hrs" | "hour" | "hours" | "小时" => 3600.0,
        _ => return None,
    };
    secs_to_duration(num * factor)
}

fn unwrap_leading_quote(s: &str) -> &str {
    for q in ['"', '\''] {
        if let Some(rest) = s.strip_prefix(q) {
            return rest.split_once(q).map_or(rest, |(inner, _)| inner);
        }
    }
    s
}

fn parse_leading_unitless_float(rest: &str) -> Option<f64> {
    let m = LEADING_FLOAT_RE.find(rest)?;
    let after = &rest[m.end()..];
    if after.starts_with(['-', '/', ':', '%'])
        || (after.starts_with('.')
            && after[1..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit()))
    {
        return None;
    }
    let after_ws = after.trim_start_matches([' ', '\t']);
    if after_ws.chars().next().is_some_and(char::is_alphabetic) {
        return None;
    }
    m.as_str().parse::<f64>().ok()
}

fn parse_kv_value(key: &str, rest: &str, allow_unitless: bool) -> Option<Duration> {
    let normalized = key.to_ascii_lowercase().replace(['-', '_', ' '], "");
    if normalized == "retryafter"
        && let Some(m) = LEADING_RFC3339_RE
            .find(rest)
            .or_else(|| LEADING_RFC2822_RE.find(rest))
    {
        return parse_timestamp_retry_after(m.as_str());
    }
    if let Some(m) = LEADING_DURATION_RE.find(rest) {
        return parse_duration_token(m.as_str());
    }
    if !allow_unitless {
        return None;
    }
    let val = parse_leading_unitless_float(rest)?;
    let secs = if normalized == "retryafterms" {
        val / 1000.0
    } else {
        val
    };
    secs_to_duration(secs)
}

pub(crate) fn parse_retry_after_text(text: &str) -> Option<Duration> {
    let mut best: Option<Duration> = None;
    for caps in RETRY_PHRASE_RE.captures_iter(text) {
        for name in ["en", "wr", "zha", "zhw"] {
            if let Some(m) = caps.name(name) {
                best = best.max(parse_duration_token(m.as_str()));
            }
        }
    }
    for caps in RETRY_KV_PREFIX_RE.captures_iter(text) {
        let key = caps.name("key").expect("key");
        let sep = caps.name("sep").expect("sep");
        let full = caps.get(0).expect("full match");
        let sep_str = sep.as_str();
        let has_punct = sep_str.contains(':') || sep_str.contains('=');
        let has_key_sep = key.as_str().contains('-') || key.as_str().contains('_');
        let rest = &text[full.end()..];
        let rest = if has_punct {
            unwrap_leading_quote(rest)
        } else {
            rest
        };
        best = best.max(parse_kv_value(key.as_str(), rest, has_punct || has_key_sep));
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderName, HeaderValue};

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        h
    }

    #[test]
    fn retryable_statuses() {
        for s in [
            400, 401, 402, 403, 404, 408, 409, 422, 425, 429, 500, 502, 503, 529,
        ] {
            let e = classify_http_error(s, &HeaderMap::new(), "oops");
            assert!(e.retryable, "{s}");
            assert_eq!(e.status, Some(s));
            assert!(!e.context_overflow);
        }
    }

    #[test]
    fn overflow_detection() {
        let body = r#"{"type":"error","error":{"type":"invalid_request_error","message":"prompt is too long: 213462 tokens > 200000 maximum"}}"#;
        let e = classify_http_error(400, &HeaderMap::new(), body);
        assert!(e.context_overflow && !e.retryable);
        assert!(e.message.starts_with("HTTP 400: "));

        let e = classify_http_error(
            400,
            &HeaderMap::new(),
            "This model's maximum context length is 128000 tokens. However, your messages resulted in 130000 tokens.",
        );
        assert!(e.context_overflow && !e.retryable);
        let e = classify_http_error(413, &headers(&[("retry-after", "10")]), "");
        assert!(e.context_overflow && !e.retryable);
        assert_eq!(e.retry_after, None);
        // Throttling that mentions tokens is not overflow.
        let e = classify_http_error(
            429,
            &HeaderMap::new(),
            "Too many tokens, please wait before trying again.",
        );
        assert!(!e.context_overflow && e.retryable);
        assert!(is_context_overflow(
            "Your input exceeds the context window of this model."
        ));
        assert!(!is_context_overflow("rate limit reached: too many tokens"));
    }

    #[test]
    fn quota_and_billing_are_retryable() {
        let e = classify_http_error(
            429,
            &headers(&[("retry-after", "15")]),
            r#"{"error":{"code":"insufficient_quota"}}"#,
        );
        assert!(e.retryable && !e.context_overflow);
        assert_eq!(e.retry_after, Some(Duration::from_secs(15)));
        let e = classify_http_error(402, &HeaderMap::new(), "billing limit reached");
        assert!(e.retryable && !e.context_overflow);
    }

    #[test]
    fn should_retry_false_header_is_ignored() {
        let e = classify_http_error(
            500,
            &headers(&[("x-should-retry", "false"), ("retry-after", "5")]),
            "",
        );
        assert!(e.retryable);
        assert_eq!(e.retry_after, Some(Duration::from_secs(5)));
        let e = classify_http_error(400, &headers(&[("x-should-retry", "false")]), "");
        assert!(e.retryable);
        let e = classify_http_error(400, &headers(&[("x-should-retry", "true")]), "");
        assert!(e.retryable);
    }

    #[test]
    fn huge_retry_after_saturates_instead_of_panicking() {
        // 10^300 hours in Go duration syntax.
        let go_hours = format!("1{}h", "0".repeat(300));
        for (name, value) in [
            ("retry-after-ms", "1e300"),
            ("retry-after", "1e300"),
            ("x-ratelimit-reset", "1e300"),
            ("x-ratelimit-reset-requests", go_hours.as_str()),
        ] {
            let e = classify_http_error(429, &headers(&[(name, value)]), "");
            assert_eq!(e.retry_after, Some(Duration::MAX), "{name}");
        }
    }

    #[test]
    fn invalid_retry_after_is_ignored() {
        for value in ["-5", "NaN", "inf"] {
            let e = classify_http_error(429, &headers(&[("retry-after", value)]), "");
            assert_eq!(e.retry_after, None, "{value}");
        }
    }

    #[test]
    fn retry_after_headers() {
        let e = classify_http_error(429, &headers(&[("retry-after-ms", "1500")]), "");
        assert_eq!(e.retry_after, Some(Duration::from_millis(1500)));
        let e = classify_http_error(429, &headers(&[("retry-after", "7")]), "");
        assert_eq!(e.retry_after, Some(Duration::from_secs(7)));
        let e = classify_http_error(
            429,
            &headers(&[("x-ratelimit-reset-requests", "1m30s")]),
            "",
        );
        assert_eq!(e.retry_after, Some(Duration::from_secs(90)));
        let e = classify_http_error(429, &headers(&[("x-ratelimit-reset-tokens", "20ms")]), "");
        assert_eq!(e.retry_after, Some(Duration::from_millis(20)));
        let past = headers(&[("retry-after", "Wed, 21 Oct 2015 07:28:00 GMT")]);
        assert_eq!(
            classify_http_error(503, &past, "").retry_after,
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn body_is_trimmed() {
        let body = "x".repeat(5000);
        let e = classify_http_error(500, &HeaderMap::new(), &body);
        assert!(e.message.chars().count() < 2100);
    }

    #[test]
    fn stream_errors() {
        let e = stream_error(Some("overloaded_error"), "Overloaded");
        assert!(e.retryable && !e.context_overflow);
        assert_eq!(e.message, "Overloaded (overloaded_error)");
        let e = stream_error(
            Some("context_length_exceeded"),
            "Your input exceeds the context window",
        );
        assert!(e.context_overflow && !e.retryable);
        assert!(
            stream_error(
                Some("insufficient_quota"),
                "You exceeded your current quota"
            )
            .retryable
        );
        assert!(stream_error(Some("billing_error"), "Billing hard limit").retryable);
        assert!(stream_error(Some("invalid_request_error"), "bad tool schema").retryable);
        assert!(stream_error(Some("authentication_error"), "invalid key").retryable);
        assert!(stream_error(None, "something broke").retryable);
    }

    #[test]
    fn retry_after_in_body_messages() {
        let cases = [
            (
                r#"{"error":{"message":"Rate limit reached for gpt-4. Please try again in 20s."}}"#,
                Some(Duration::from_secs(20)),
            ),
            (
                r#"{"error":{"message":"Please try again in 1m30s."}}"#,
                Some(Duration::from_secs(90)),
            ),
            (
                r#"{"error":{"message":"Please try again in 500ms."}}"#,
                Some(Duration::from_millis(500)),
            ),
            (
                r#"{"error":{"message":"Please try again in 1h2m3.5s."}}"#,
                Some(Duration::from_secs_f64(3723.5)),
            ),
            (
                "Requests have exceeded rate limit. Please retry after 12 seconds.",
                Some(Duration::from_secs(12)),
            ),
            (
                "Quota exceeded for metric. Please retry in 34.5s.",
                Some(Duration::from_secs_f64(34.5)),
            ),
            (
                "Please wait 45 secs before retrying",
                Some(Duration::from_secs(45)),
            ),
            (
                "Too busy: wait 2 minutes to retry",
                Some(Duration::from_secs(120)),
            ),
            (
                "Model will be available in 1.5 hours",
                Some(Duration::from_secs(5400)),
            ),
            (
                "Rate limit resets in 250 milliseconds",
                Some(Duration::from_millis(250)),
            ),
            (
                "请求过于频繁，请在 30 秒后重试",
                Some(Duration::from_secs(30)),
            ),
            ("1.5分钟后重试", Some(Duration::from_secs(90))),
            (
                "系统繁忙，请等待 500 毫秒",
                Some(Duration::from_millis(500)),
            ),
            ("上游错误: Retry-After: 8", Some(Duration::from_secs(8))),
            ("retry_after_ms=2500", Some(Duration::from_millis(2500))),
            ("retry-delay: 6m0s", Some(Duration::from_secs(360))),
            ("TRY AGAIN IN 15S", Some(Duration::from_secs(15))),
            ("Rate limit.\nTry again in 7s", Some(Duration::from_secs(7))),
            (
                r#"{"error":{"message":"Rate limit.\nTry again in 7s"}}"#,
                Some(Duration::from_secs(7)),
            ),
        ];
        for (body, expected) in cases {
            let e = classify_http_error(429, &HeaderMap::new(), body);
            assert!(e.retryable, "{body}");
            assert_eq!(e.retry_after, expected, "{body}");
        }
    }

    #[test]
    fn retry_after_in_json_fields_and_dates() {
        let e = classify_http_error(429, &HeaderMap::new(), r#"{"retry_after":15}"#);
        assert_eq!(e.retry_after, Some(Duration::from_secs(15)));

        let e = classify_http_error(
            429,
            &HeaderMap::new(),
            r#"{"error":{"retry_after_ms":2500,"message":"rate limit"}}"#,
        );
        assert_eq!(e.retry_after, Some(Duration::from_millis(2500)));

        let e = classify_http_error(
            429,
            &HeaderMap::new(),
            r#"{"error":{"retryAfterMs":"1750"}}"#,
        );
        assert_eq!(e.retry_after, Some(Duration::from_millis(1750)));

        let gemini = r#"{"error":{"code":429,"message":"Resource exhausted","details":[{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"34.567s"}]}}"#;
        let e = classify_http_error(429, &HeaderMap::new(), gemini);
        assert_eq!(e.retry_after, Some(Duration::from_secs_f64(34.567)));

        let gemini_arr = r#"[{"error":{"details":[{"retryDelay":"12s"}]}}]"#;
        let e = classify_http_error(429, &HeaderMap::new(), gemini_arr);
        assert_eq!(e.retry_after, Some(Duration::from_secs(12)));

        // Past RFC 2822 / RFC 3339 timestamps clamp to ZERO in both JSON and body text.
        for body in [
            r#"{"retry_after":"2015-10-21T07:28:00Z"}"#,
            r#"{"retry-after":"Wed, 21 Oct 2015 07:28:00 GMT"}"#,
            "Retry-After: 2015-10-21T07:28:00Z",
            "Retry-After: Wed, 21 Oct 2015 07:28:00 GMT",
        ] {
            let e = classify_http_error(503, &HeaderMap::new(), body);
            assert_eq!(e.retry_after, Some(Duration::ZERO), "{body}");
        }

        // Future RFC 3339 timestamp yields positive duration.
        let future = (chrono::Utc::now() + chrono::Duration::seconds(30)).to_rfc3339();
        let body = format!(r#"{{"retry_after":"{future}"}}"#);
        let d = classify_http_error(429, &HeaderMap::new(), &body)
            .retry_after
            .unwrap();
        assert!(
            d >= Duration::from_secs(20) && d <= Duration::from_secs(35),
            "{d:?}"
        );
    }

    #[test]
    fn retry_after_false_positives_and_overflow_are_ignored() {
        for body in [
            "exceeded 100 requests per 1 minute",
            "HTTP 429 Too Many Requests",
            "upstream timed out after 30s",
            "200000 tokens exceeded",
            "connection to port 8080 refused",
            "400ms latency spike",
            "try again in 5mb",
            "wait 10s for tool output",
            "cannot retry after 3 attempts",
            "try again in -5s",
            "Retry-After: -5",
            "Retry-After: NaN",
            "Retry-After: inf",
            "retry-after-ms: -100",
            r#"{"retry_after": -5}"#,
            r#"{"retry_after": "-5"}"#,
            r#"{"retry_after": "NaN"}"#,
            r#"{"retry_after": "inf"}"#,
            r#"{"retry_after_ms": -100}"#,
        ] {
            let e = classify_http_error(429, &HeaderMap::new(), body);
            assert_eq!(e.retry_after, None, "false positive for: {body}");
        }

        // Overflow must never return retry_after even when both header and body carry hints.
        let e = classify_http_error(
            400,
            &headers(&[("retry-after", "10")]),
            r#"{"error":{"message":"prompt is too long: 213462 tokens > 200000. Please try again in 20s","retry_after":30}}"#,
        );
        assert!(e.context_overflow && !e.retryable);
        assert_eq!(e.retry_after, None);
    }

    #[test]
    fn retry_after_body_saturates_huge_and_takes_max_with_header() {
        for body in [
            r#"{"retry_after": 1e300}"#,
            r#"{"retry_after_ms": "1e300"}"#,
            "Retry-After: 1e300",
        ] {
            let e = classify_http_error(429, &HeaderMap::new(), body);
            assert_eq!(e.retry_after, Some(Duration::MAX), "{body}");
        }

        let e = classify_http_error(
            429,
            &headers(&[("retry-after", "5")]),
            "Please try again in 20s",
        );
        assert_eq!(e.retry_after, Some(Duration::from_secs(20)));

        let e = classify_http_error(
            429,
            &headers(&[("retry-after", "30")]),
            "Please try again in 10s",
        );
        assert_eq!(e.retry_after, Some(Duration::from_secs(30)));
    }

    #[test]
    fn stream_error_extracts_retry_after_from_message_and_json() {
        let e = stream_error(Some("rate_limit_error"), "Please try again in 12s");
        assert!(e.retryable);
        assert_eq!(e.retry_after, Some(Duration::from_secs(12)));

        let payload = serde_json::json!({
            "error": {
                "type": "rate_limit_error",
                "message": "Rate limited, please wait 5s",
                "retry_after": 25,
                "details": [{"retryDelay": "10s"}]
            }
        });
        let e = stream_error_json(
            Some("rate_limit_error"),
            "Rate limited, please wait 5s",
            &payload,
            &[],
        );
        assert!(e.retryable);
        assert_eq!(e.retry_after, Some(Duration::from_secs(25)));

        let overflow_payload = serde_json::json!({
            "error": {
                "code": "context_length_exceeded",
                "message": "Your input exceeds the context window. Please try again in 20s",
                "retry_after": 20
            }
        });
        let e = stream_error_json(
            Some("context_length_exceeded"),
            "Your input exceeds the context window. Please try again in 20s",
            &overflow_payload,
            &[],
        );
        assert!(e.context_overflow && !e.retryable);
        assert_eq!(e.retry_after, None);
    }

    #[test]
    fn retry_after_ignores_echoed_payloads_and_unitless_false_positives() {
        let echoed = serde_json::json!({
            "id": "try again in 99s",
            "model": "retry in 99s",
            "name": "please wait 99s",
            "instructions": "Please try again in 99s",
            "prompt": "Please try again in 99s",
            "input": [{"content": "retry_after: 99"}],
            "messages": [{"role": "user", "content": "retry in 99s", "retry_after": 99}],
            "tools": [{"description": "Please try again in 99s"}],
            "output": [{"content": [{"text": "Please try again in 99s"}]}],
            "choices": [{"delta": {"content": "Please try again in 99s"}}],
            "error": {
                "code": "rate_limit",
                "message": "Rate limit exceeded"
            }
        });
        let e = classify_http_error(429, &HeaderMap::new(), &echoed.to_string());
        assert_eq!(e.retry_after, None);

        for body in [
            "Retry-After: 500 us",
            "Retry-After: 500μs",
            "Retry-After: 5 mb",
            "Retry-After: 3 attempts",
            "retry-after 3 attempts",
            "retry after 8",
            "Please retry after 2026-11-19 11:05:00 UTC",
            "Retry-After: 2026-11-19",
            "Please retry after 11:30 UTC",
            "Retry-After: 1.2.3",
            "cannot retry after 3/3 attempts",
            "retry after 50% jitter",
        ] {
            let e = classify_http_error(429, &HeaderMap::new(), body);
            assert_eq!(e.retry_after, None, "false positive for: {body}");
        }
    }

    #[test]
    fn retry_after_real_world_variants_and_punctuation_edges() {
        let cases = [
            ("retry_after_ms: 2 s", Duration::from_secs(2)),
            ("Retry-After: 30, please wait", Duration::from_secs(30)),
            ("Retry-After: 30. Please wait", Duration::from_secs(30)),
            ("retry-after 8", Duration::from_secs(8)),
            ("retry again in 20s", Duration::from_secs(20)),
            ("retry again after 30s", Duration::from_secs(30)),
            (r#"{"retry_after": "20 seconds"}"#, Duration::from_secs(20)),
            (r#"{"retryDelay": "500 ms"}"#, Duration::from_millis(500)),
            ("Please try again in 1m 30s", Duration::from_secs(90)),
            ("Wait 30s and try again", Duration::from_secs(30)),
            ("Wait 30s before trying again", Duration::from_secs(30)),
            ("请求过于频繁，30秒后再试", Duration::from_secs(30)),
        ];
        for (body, expected) in cases {
            let e = classify_http_error(429, &HeaderMap::new(), body);
            assert_eq!(e.retry_after, Some(expected), "{body}");
        }
    }
}
