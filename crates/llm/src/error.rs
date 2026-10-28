//! Error classification: HTTP failures, transport problems and in-stream
//! provider errors are mapped onto [`LlmError`] with `retryable`,
//! `retry_after` and `context_overflow` filled in.

use crate::LlmError;
use regex::Regex;
use reqwest::header::HeaderMap;
use std::sync::OnceLock;
use std::time::Duration;

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
fn overflow_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
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
    })
}

/// Messages that look like overflow but are actually throttling / parameter errors.
fn overflow_exclusion_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(concat!(
            r"(?i)^(throttling error|service unavailable):",
            r"|rate.?limit|too many requests",
            r"|too many tokens(,)?( per day)?(,)? please wait",
            r"|max[_ ]tokens must be less than",
        ))
        .expect("overflow exclusion regex")
    })
}

/// Quota / billing problems: retrying will not help.
fn quota_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(concat!(
            r"(?i)insufficient_quota|insufficient balance|quota exceeded|exceeded your current quota",
            r"|billing|out of budget|usage_not_included|usagelimiterror|monthly usage limit",
            r"|subscription_sharing_usage_limit_exceeded|payment required",
        ))
        .expect("quota regex")
    })
}

/// Transient conditions detectable from the error text alone.
fn retryable_text_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(concat!(
            r"(?i)overloaded|server_busy|server is busy|rate.?limit|too many requests",
            r"|try again|retry your request|temporarily unavailable|service.?unavailable",
            r"|resource.?exhausted",
        ))
        .expect("retryable regex")
    })
}

/// Whether an error text indicates that the prompt exceeded the context window.
pub fn is_context_overflow(text: &str) -> bool {
    overflow_re().is_match(text) && !overflow_exclusion_re().is_match(text)
}

fn is_quota(text: &str) -> bool {
    quota_re().is_match(text)
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

    let context_overflow = status == 413 || (status != 429 && is_context_overflow(body));
    let quota = status == 402 || is_quota(body);
    let should_retry = headers
        .get("x-should-retry")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| match v.trim() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        });

    let retryable = if context_overflow || quota {
        false
    } else if let Some(hint) = should_retry {
        hint
    } else {
        match status {
            408 | 409 | 425 | 429 | 500..=599 => true,
            401 | 403 | 404 | 413 => false,
            _ => retryable_text_re().is_match(body),
        }
    };

    LlmError {
        message,
        status: Some(status),
        retryable,
        retry_after: if retryable {
            parse_retry_after(headers)
        } else {
            None
        },
        context_overflow,
        cancelled: false,
    }
}

/// Error codes / types reported inside a stream that are worth retrying.
const RETRYABLE_CODES: &[&str] = &[
    "server_error",
    "server_is_overloaded",
    "overloaded_error",
    "overloaded",
    "rate_limit_exceeded",
    "rate_limit_error",
    "api_error",
    "internal_error",
    "internal_server_error",
    "service_unavailable",
    "timeout",
    "slow_down",
];

/// Error codes that must never be retried.
const FATAL_CODES: &[&str] = &[
    "invalid_request_error",
    "invalid_prompt",
    "invalid_api_key",
    "authentication_error",
    "permission_error",
    "not_found_error",
    "model_not_found",
    "billing_error",
];

/// Classify an error reported inside the SSE stream (Anthropic `error` event,
/// Responses `response.failed` / `error`, Chat `{"error": {...}}` chunks).
/// `code` is the provider error code/type when available.
pub(crate) fn stream_error(code: Option<&str>, message: &str) -> LlmError {
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
    let quota = matches!(
        code,
        "insufficient_quota" | "usage_not_included" | "billing_error"
    ) || is_quota(message);
    let retryable = if overflow || quota {
        false
    } else if RETRYABLE_CODES.contains(&code) {
        true
    } else if FATAL_CODES.contains(&code) {
        retryable_text_re().is_match(message)
    } else {
        // Unknown in-stream failures are usually transient server-side problems.
        true
    };
    LlmError {
        context_overflow: overflow,
        retryable,
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

    if let Some(ms) = get("retry-after-ms").and_then(|v| v.parse::<f64>().ok())
        && ms.is_finite()
        && ms >= 0.0
    {
        return Some(Duration::from_secs_f64(ms / 1000.0));
    }
    if let Some(v) = get("retry-after") {
        if let Ok(secs) = v.parse::<f64>() {
            if secs.is_finite() && secs >= 0.0 {
                return Some(Duration::from_secs_f64(secs));
            }
        } else if let Ok(date) = chrono::DateTime::parse_from_rfc2822(v) {
            let delta = date.timestamp_millis() - chrono::Utc::now().timestamp_millis();
            return Some(Duration::from_millis(delta.max(0) as u64));
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

/// Parse Go-style durations as used by OpenAI rate-limit headers
/// (`"1s"`, `"6m0s"`, `"20ms"`, `"1h2m3.5s"`) or plain seconds.
fn parse_go_duration(s: &str) -> Option<Duration> {
    if let Ok(secs) = s.parse::<f64>() {
        return (secs.is_finite() && secs >= 0.0).then(|| Duration::from_secs_f64(secs));
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
    (any && num.is_empty() && total.is_finite()).then(|| Duration::from_secs_f64(total))
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
        for s in [408, 409, 425, 429, 500, 502, 503, 529] {
            let e = classify_http_error(s, &HeaderMap::new(), "oops");
            assert!(e.retryable, "{s}");
            assert_eq!(e.status, Some(s));
            assert!(!e.context_overflow);
        }
        for s in [400, 401, 403, 404, 422] {
            assert!(
                !classify_http_error(s, &HeaderMap::new(), "bad").retryable,
                "{s}"
            );
        }
    }

    #[test]
    fn overload_text_on_400_is_retryable() {
        let e = classify_http_error(
            400,
            &HeaderMap::new(),
            r#"{"error":{"message":"Model is overloaded"}}"#,
        );
        assert!(e.retryable);
        assert!(!classify_http_error(401, &HeaderMap::new(), "overloaded").retryable);
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
        assert!(e.context_overflow);
        assert!(classify_http_error(413, &HeaderMap::new(), "").context_overflow);
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
    fn quota_not_retryable() {
        let e = classify_http_error(
            429,
            &HeaderMap::new(),
            r#"{"error":{"code":"insufficient_quota"}}"#,
        );
        assert!(!e.retryable);
        assert!(!classify_http_error(402, &HeaderMap::new(), "").retryable);
    }

    #[test]
    fn should_retry_header_wins() {
        let e = classify_http_error(500, &headers(&[("x-should-retry", "false")]), "");
        assert!(!e.retryable);
        let e = classify_http_error(400, &headers(&[("x-should-retry", "true")]), "");
        assert!(e.retryable);
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
            !stream_error(
                Some("insufficient_quota"),
                "You exceeded your current quota"
            )
            .retryable
        );
        assert!(!stream_error(Some("invalid_request_error"), "bad tool schema").retryable);
        assert!(stream_error(None, "something broke").retryable);
    }
}
