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
    // Chinese gateways: "超过模型最大长度/上下文" (exceeds max length / context), "上下文长度超" ...
    r"(?:输入|提示|上下文|请求).{0,8}(?:超长|超过|超出).{0,12}(?:长度|上下文|限制|token)",
    r"超过.{0,10}(?:最大|上下文).{0,6}(?:长度|窗口)",
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

/// Error texts that indicate a transient condition worth retrying (incl. Chinese gateway texts:
/// "访问量过大" = too much traffic, "稍后再试" = try again later, "繁忙" = busy, "限流" = throttled).
const RETRYABLE_PATTERN: &str = r"overloaded|server_busy|servers are currently busy|high demand|at capacity|rate.?limit|too many requests|service.?(?:is.?)?(?:temporarily.?)?unavailable|server.?error|internal.?error|try again|retry your request|temporarily|upstream|timed? ?out|timeout|connection|访问量过大|稍后再试|稍后重试|繁忙|过载|限流|请求过多|频率";

/// Error texts that indicate exhausted quota / billing problems (never fixed by retrying soon)
/// (incl. "余额不足" = insufficient balance, "欠费" = in arrears, "额度" = quota).
const QUOTA_PATTERN: &str = r"insufficient_quota|insufficient.{0,12}(?:quota|balance|credit)|quota (?:exceeded|exhausted)|remaining quota|exceeded your current quota|billing|out of budget|available balance|usage limit reached|usage_not_included|credit|余额不足|欠费|额度不足|额度已用完|充值";

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

/// Turn an HTTP error body into a short human-readable message: JSON bodies are reduced to
/// `type/code: message` (OpenAI / Anthropic / gateway shapes), HTML pages to their title and
/// text, anything else is returned trimmed.
pub(crate) fn readable_body(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        return String::new();
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
        if let Some(msg) = json_error_message(&v) {
            return msg;
        }
        return raw.to_string();
    }
    let lower = raw.get(..raw.len().min(512)).unwrap_or(raw).to_ascii_lowercase();
    if lower.starts_with("<!doctype html") || lower.starts_with("<html") || lower.contains("<body") {
        return html_text(raw);
    }
    raw.to_string()
}

/// Display text for an HTTP error: readable body, or the status reason when the body is empty.
pub(crate) fn describe_http(status: u16, body: &str) -> String {
    let msg = readable_body(body);
    if !msg.is_empty() {
        return msg;
    }
    reqwest::StatusCode::from_u16(status)
        .ok()
        .and_then(|s| s.canonical_reason())
        .map(|r| format!("{r} (no body)"))
        .unwrap_or_else(|| "(no body)".into())
}

fn json_error_message(v: &serde_json::Value) -> Option<String> {
    use serde_json::Value;
    let str_of = |v: Option<&Value>| -> Option<String> {
        match v? {
            Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
    };
    // `{"error": "text"}` / `{"detail": "text"}` / `{"message": "text"}`
    if let Some(s) = str_of(v.get("error")) {
        return Some(s);
    }
    let err = match v.get("error") {
        Some(e @ Value::Object(_)) => e,
        _ => match v.get("errors").and_then(Value::as_array).and_then(|a| a.first()) {
            Some(e) if e.is_object() => e,
            _ => v,
        },
    };
    let message = str_of(err.get("message"))
        .or_else(|| str_of(err.get("msg")))
        .or_else(|| str_of(err.get("detail")))
        .or_else(|| str_of(v.get("message")))
        .or_else(|| str_of(v.get("detail")))
        .or_else(|| err.get("detail").filter(|d| !d.is_null()).map(|d| d.to_string()))?;
    let mut labels: Vec<String> = Vec::new();
    for key in ["type", "code", "status"] {
        if let Some(l) = str_of(err.get(key)) {
            let useless = matches!(l.as_str(), "error" | "<nil>" | "null" | "None");
            if !useless && !message.contains(&l) && !labels.contains(&l) {
                labels.push(l);
            }
        }
    }
    Some(if labels.is_empty() { message } else { format!("{}: {message}", labels.join(" ")) })
}

fn html_text(raw: &str) -> String {
    static TITLE: OnceLock<Regex> = OnceLock::new();
    static TAGS: OnceLock<Regex> = OnceLock::new();
    static SPACE: OnceLock<Regex> = OnceLock::new();
    let title = regex(&TITLE, r"(?s)<title[^>]*>(.*?)</title>")
        .captures(raw)
        .map(|c| c[1].trim().to_string())
        .filter(|t| !t.is_empty());
    let body = regex(&TAGS, r"(?s)<(?:script|style)[^>]*>.*?</(?:script|style)>|<[^>]+>").replace_all(raw, " ");
    let body = regex(&SPACE, r"\s+").replace_all(&body, " ");
    let body = crate::transport::truncate_str(body.trim(), 300).to_string();
    match title {
        Some(t) if body.starts_with(&t) => body,
        Some(t) => format!("{t}: {body}"),
        None => body,
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
    fn readable_http_errors() {
        // Real bodies seen from the configured endpoints.
        let cases = [
            (403, r#"{"error":{"type":"\u003cnil\u003e","message":"This group does not allow /v1/messages dispatch (request id: 1)"},"type":"error"}"#,
             "HTTP 403: This group does not allow /v1/messages dispatch (request id: 1)"),
            (502, r#"{"error":{"message":"openai_error","type":"bad_response_status_code","param":"","code":"bad_response_status_code"}}"#,
             "HTTP 502: bad_response_status_code: openai_error"),
            (400, r#"{"error":{"code":"1261","message":"Prompt exceeds max length"}}"#, "HTTP 400: 1261: Prompt exceeds max length"),
            (400, r#"{"type":"error","error":{"type":"invalid_request_error","message":"prompt is too long: 213462 tokens > 200000 maximum"}}"#,
             "HTTP 400: invalid_request_error: prompt is too long: 213462 tokens > 200000 maximum"),
            (429, r#"{"error":"rate limit hit"}"#, "HTTP 429: rate limit hit"),
            (422, r#"{"detail":[{"loc":["body"],"msg":"field required"}]}"#, r#"HTTP 422: [{"loc":["body"],"msg":"field required"}]"#),
            (502, "<html><head><title>502 Bad Gateway</title></head><body><center><h1>502 Bad Gateway</h1></center><hr><center>nginx</center></body></html>",
             "HTTP 502: 502 Bad Gateway 502 Bad Gateway nginx"),
            (503, "", "HTTP 503: Service Unavailable (no body)"),
            (500, "plain text failure", "HTTP 500: plain text failure"),
        ];
        for (status, body, want) in cases {
            assert_eq!(http(status, body).to_string(), want);
        }
    }

    #[test]
    fn chinese_gateway_errors() {
        // Zhipu: 429 + code 1305 "model is overloaded, try again later" → retryable.
        assert!(http(429, r#"{"error":{"code":"1305","message":"该模型当前访问量过大，请您稍后再试"}}"#).is_retryable());
        assert!(http(400, r#"{"error":{"message":"服务繁忙，请稍后重试"}}"#).is_retryable());
        // Insufficient balance / quota → not retryable.
        for body in [
            r#"{"error":{"code":"1113","message":"余额不足或无可用资源包,请充值。"}}"#,
            r#"{"error":{"type":"new_api_error","message":"Insufficient user quota, remaining quota: ¥-0.05"}}"#,
        ] {
            assert!(!http(429, body).is_retryable(), "{body}");
            assert!(!http(400, body).is_retryable(), "{body}");
        }
        assert!(!http(402, r#"{"error":{"message":"Insufficient credits.","code":402}}"#).is_retryable());
        // Overflow in Chinese.
        assert!(http(400, r#"{"error":{"message":"输入长度超过模型最大上下文长度"}}"#).is_context_overflow());
        assert!(http(400, "Prompt exceeds max length").is_context_overflow());
        assert!(!http(400, r#"{"error":{"message":"请求参数错误"}}"#).is_context_overflow());
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
