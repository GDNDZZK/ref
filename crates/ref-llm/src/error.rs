//! Error classification: context overflow, retryability, Retry-After parsing.

use std::sync::OnceLock;
use std::time::Duration;

use regex::{Regex, RegexSet};
use reqwest::header::HeaderMap;

use crate::LlmError;

/// Patterns (case-insensitive) that indicate the prompt does not fit the context window.
const OVERFLOW_PATTERNS: &[&str] = &[
    r"prompt (?:is )?too long",
    r"prompt exceeds max length",
    r"request_too_large",
    r"input is too long for (?:the )?(?:requested|selected) model",
    r"input is too long",
    r"exceeds the context window",
    r"context window exceeded",
    r"exceeds (?:the )?(?:model'?s )?maximum context length",
    r"input token count.*exceeds the maximum",
    r"maximum prompt length is \d+",
    r"reduce the length of the messages",
    r"maximum context length is \d+ tokens",
    r"exceeds (?:the )?maximum allowed input length of [\d,]+ tokens?",
    r"input(?: \(\d+ tokens\))? is longer than the model'?s context length",
    r"exceeds the limit of \d+",
    r"exceeds the available context size",
    r"greater than the context length",
    r"context window exceeds limit",
    r"exceeded model token limit",
    r"too large for model with \d+ maximum context length",
    r"prompt has [\d,]+ tokens?, but the configured context size is [\d,]+ tokens?",
    r"model_context_window_exceeded",
    r"prompt too long; exceeded (?:max )?context length",
    r"range of input length should be",
    r"context[_ ]length[_ ]exceeded",
    r"too many tokens",
    r"token limit exceeded",
    r"number of input tokens exceeds maximum length",
    r"tokens in request more than max tokens allowed",
    r"requested input length \d+ exceeds maximum input length",
    r"`inputs` tokens \+ `max_new_tokens` must be <= \d+",
    r"context length is only \d+ tokens",
    r"input length.*exceeds.*context length",
    r"exceeded this model context window limit",
    r"too large for the model'?s configured input limit",
    r"prompt (?:too long|exceeds max length|超长)",
    r"^4(?:00|13)\s*(?:status code)?\s*\(no body\)",
];

/// Messages that look like overflow but are rate limits / parameter errors.
const OVERFLOW_EXCLUSIONS: &[&str] = &[
    r"^(?:throttling error|service unavailable):",
    r"rate limit",
    r"too many requests",
    r"max[_ ]tokens must be less than",
    r"too many tokens(?: per day)?,? please wait",
    r"tokens per (?:min|minute|day)",
];

/// Error texts that indicate a transient condition worth retrying.
const RETRYABLE_PATTERN: &str = r"overloaded|server_busy|servers are currently busy|high demand|at capacity|rate.?limit|too many requests|service.?(?:is.?)?(?:temporarily.?)?unavailable|server.?error|internal.?error|try again|retry your request|temporarily|upstream|timed? ?out|timeout|connection";

/// Error texts that indicate exhausted quota / billing problems (never fixed by retrying soon).
const QUOTA_PATTERN: &str = r"insufficient_quota|quota exceeded|exceeded your current quota|billing|out of budget|available balance|usage limit reached|usage_not_included|credit";

fn overflow_set() -> &'static (RegexSet, RegexSet) {
    static SET: OnceLock<(RegexSet, RegexSet)> = OnceLock::new();
    SET.get_or_init(|| {
        let build = |p: &[&str]| {
            RegexSet::new(p.iter().map(|s| format!("(?i){s}"))).expect("valid overflow regexes")
        };
        (build(OVERFLOW_PATTERNS), build(OVERFLOW_EXCLUSIONS))
    })
}

fn regex(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(&format!("(?i){pattern}")).expect("valid regex"))
}

/// True if `text` describes a context-window overflow.
pub fn text_is_context_overflow(text: &str) -> bool {
    let (yes, no) = overflow_set();
    let text = text.trim();
    yes.is_match(text) && !no.is_match(text)
}

fn text_is_retryable(text: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    regex(&RE, RETRYABLE_PATTERN).is_match(text)
}

fn text_is_quota(text: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    regex(&RE, QUOTA_PATTERN).is_match(text)
}

impl LlmError {
    pub(crate) fn overflow(&self) -> bool {
        match self {
            LlmError::Http { status, body, .. } => {
                if *status == 429 {
                    return false;
                }
                if *status == 413 {
                    return true;
                }
                text_is_context_overflow(body)
                    || (body.is_empty() && *status == 400 && text_is_context_overflow("400 (no body)"))
            }
            LlmError::Stream(msg) => text_is_context_overflow(msg),
            _ => false,
        }
    }

    /// Whether retrying the same request later may succeed. Network problems, stream
    /// interruptions, timeouts, rate limits and server errors are retryable; authentication,
    /// invalid requests, quota exhaustion and configuration errors are not. Context overflow
    /// is not retryable (compact instead).
    pub fn is_retryable(&self) -> bool {
        match self {
            LlmError::Network(_) | LlmError::Timeout(_) => true,
            LlmError::Stream(msg) => !self.overflow() && !text_is_quota(msg),
            LlmError::Config(_) => false,
            LlmError::Http { status, body, .. } => match *status {
                408 | 409 | 425 | 529 => true,
                429 => !text_is_quota(body) || text_is_retryable(body),
                500..=599 => true,
                401 | 402 | 403 | 404 | 405 | 413 | 422 => false,
                400..=499 => !self.overflow() && !text_is_quota(body) && text_is_retryable(body),
                _ => false,
            },
        }
    }
}

/// Parse `retry-after-ms` (milliseconds) or `retry-after` (seconds or HTTP date).
pub(crate) fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
    let get = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).map(str::trim);
    if let Some(ms) = get("retry-after-ms").and_then(|v| v.parse::<f64>().ok()) {
        if ms.is_finite() && ms >= 0.0 {
            return Some(Duration::from_secs_f64(ms / 1000.0));
        }
    }
    let v = get("retry-after")?;
    if let Ok(secs) = v.parse::<f64>() {
        return (secs.is_finite() && secs >= 0.0).then(|| Duration::from_secs_f64(secs));
    }
    let date = chrono::DateTime::parse_from_rfc2822(v).ok()?;
    let delta = date.with_timezone(&chrono::Utc) - chrono::Utc::now();
    Some(delta.to_std().unwrap_or(Duration::ZERO))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn http(status: u16, body: &str) -> LlmError {
        LlmError::Http { status, body: body.into(), retry_after: None }
    }

    #[test]
    fn overflow_detection() {
        let yes = [
            http(400, r#"{"type":"error","error":{"type":"invalid_request_error","message":"prompt is too long: 213462 tokens > 200000 maximum"}}"#),
            http(400, r#"{"error":{"message":"This model's maximum context length is 128000 tokens. However, your messages resulted in 130000 tokens.","code":"context_length_exceeded"}}"#),
            http(400, "Your input exceeds the context window of this model."),
            http(413, ""),
            http(413, "request_too_large"),
            http(400, "model_context_window_exceeded"),
            http(400, "Input is too long for requested model."),
            http(400, ""),
            LlmError::Stream("context_length_exceeded: Your input exceeds the context window".into()),
        ];
        for e in &yes {
            assert!(e.is_context_overflow(), "{e}");
            assert!(!e.is_retryable(), "{e}");
        }
        let no = [
            http(429, "Rate limit reached: too many tokens per minute"),
            http(400, "rate limit exceeded: too many tokens, please wait"),
            http(400, "max_tokens must be less than or equal to 8192"),
            http(500, "internal server error"),
            http(401, "invalid api key"),
            LlmError::Network("connection refused".into()),
            LlmError::Stream("Overloaded".into()),
        ];
        for e in &no {
            assert!(!e.is_context_overflow(), "{e}");
        }
    }

    #[test]
    fn retryable() {
        for e in [
            http(429, "slow down"),
            http(529, "overloaded_error"),
            http(503, ""),
            http(408, ""),
            http(400, "The server is overloaded, please try again later"),
            LlmError::Stream("overloaded_error: Overloaded".into()),
            LlmError::Timeout("x".into()),
            LlmError::Network("x".into()),
        ] {
            assert!(e.is_retryable(), "{e}");
        }
        for e in [
            http(400, "invalid_request_error: messages: field required"),
            http(401, "unauthorized"),
            http(403, "forbidden"),
            http(404, "model not found"),
            http(429, "You exceeded your current quota, please check your plan and billing details. insufficient_quota"),
            LlmError::Config("x".into()),
        ] {
            assert!(!e.is_retryable(), "{e}");
        }
    }

    #[test]
    fn retry_after_headers() {
        let mut h = HeaderMap::new();
        h.insert("retry-after", "7".parse().unwrap());
        assert_eq!(parse_retry_after(&h), Some(Duration::from_secs(7)));
        h.insert("retry-after-ms", "1500".parse().unwrap());
        assert_eq!(parse_retry_after(&h), Some(Duration::from_millis(1500)));
        let mut h = HeaderMap::new();
        let future = (chrono::Utc::now() + chrono::Duration::seconds(120)).to_rfc2822();
        h.insert("retry-after", future.parse().unwrap());
        let d = parse_retry_after(&h).unwrap();
        assert!(d > Duration::from_secs(100) && d <= Duration::from_secs(120), "{d:?}");
        let mut h = HeaderMap::new();
        h.insert("retry-after", "garbage".parse().unwrap());
        assert_eq!(parse_retry_after(&h), None);
    }
}
