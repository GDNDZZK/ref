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

/// Classify an HTTP status + body text.
pub fn classify(_status: Option<u16>, _body: &str) -> ErrorKind {
    todo!()
}
