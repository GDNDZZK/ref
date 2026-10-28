//! Provider-neutral LLM streaming client.
//!
//! Supports OpenAI Chat Completions (and compatibles), OpenAI Responses and
//! Anthropic Messages APIs with SSE streaming.
//!
//! The public API in this file is a contract used by `ref-server`; keep it stable.

use ref_proto::{ImageData, Usage};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

mod anthropic;
mod chat;
mod error;
mod responses;
mod sse;
#[cfg(test)]
mod test_support;
mod util;

pub use error::{classify_http_error, is_context_overflow};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiKind {
    /// `POST {base}/chat/completions`
    OpenAIChat,
    /// `POST {base}/responses`
    OpenAIResponses,
    /// `POST {base}/messages`
    Anthropic,
}

impl ApiKind {
    /// Map an opencode `npm` package name to an API kind.
    pub fn from_npm(npm: Option<&str>) -> ApiKind {
        match npm.unwrap_or("") {
            "@ai-sdk/anthropic" | "@ai-sdk/google-vertex/anthropic" => ApiKind::Anthropic,
            "@ai-sdk/openai" | "@ai-sdk/azure" => ApiKind::OpenAIResponses,
            _ => ApiKind::OpenAIChat,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Provider {
    pub id: String,
    pub api: ApiKind,
    /// e.g. `https://api.openai.com/v1`, `https://api.anthropic.com/v1`
    pub base_url: String,
    pub api_key: Option<String>,
    /// Extra headers from provider options + model headers.
    pub headers: BTreeMap<String, String>,
    /// Whether to send `prompt_cache_key` (OpenAI-ish). Default true for official OpenAI.
    pub set_cache_key: bool,
    /// Per-chunk idle timeout for the SSE stream.
    pub chunk_timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct Model {
    pub id: String,
    /// Max context tokens (0 = unknown).
    pub context: u64,
    /// Max output tokens (0 = unknown -> provider default/32000).
    pub output: u64,
    pub images: bool,
    pub reasoning: bool,
    /// Merged opencode-style options: model.options deep-merged with the selected variant
    /// (camelCase keys like `reasoningEffort`, `reasoningSummary`, `textVerbosity`,
    /// `thinking: {type, budgetTokens}`, `include`, `store`, plus arbitrary passthrough keys).
    pub options: serde_json::Map<String, Value>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    /// JSON schema (object).
    pub parameters: Value,
}

#[derive(Debug, Clone)]
pub enum UserContent {
    Text(String),
    Image(ImageData),
}

#[derive(Debug, Clone)]
pub enum AssistantBlock {
    Text(String),
    /// Reasoning text + opaque replay data captured from the provider
    /// (anthropic `{signature}` / `{redacted: data}`, responses `{item: <full reasoning item>}`,
    /// chat `{field: "reasoning_content"}`).
    Reasoning {
        text: String,
        provider_data: Option<Value>,
    },
    ToolCall {
        id: String,
        name: String,
        arguments: String,
        provider_data: Option<Value>,
    },
}

#[derive(Debug, Clone)]
pub enum ChatMessage {
    User(Vec<UserContent>),
    Assistant {
        blocks: Vec<AssistantBlock>,
        /// Provider + model that produced it. Reasoning provider_data is only replayed
        /// when this matches the current request's provider/model (otherwise reasoning
        /// is dropped or converted to plain text).
        origin: Option<(String, String)>,
    },
    ToolResult {
        call_id: String,
        name: String,
        content: String,
        images: Vec<ImageData>,
        is_error: bool,
    },
}

#[derive(Debug, Clone)]
pub struct Request {
    /// System prompt parts. Order them from most stable to least stable; the
    /// implementation places cache breakpoints accordingly.
    pub system: Vec<String>,
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolDef>,
    /// Stable conversation id used for prompt cache affinity headers / `prompt_cache_key`.
    pub session_id: String,
}

#[derive(Debug, Clone)]
pub enum BlockKind {
    Text,
    Reasoning,
    ToolCall { id: String, name: String },
}

/// Streaming events. `index` identifies the block inside the assistant response and
/// matches the index in [`Response::blocks`].
#[derive(Debug, Clone)]
pub enum StreamEvent {
    BlockStart { index: usize, kind: BlockKind },
    Delta { index: usize, text: String },
    BlockEnd { index: usize },
    Usage(Usage),
}

#[derive(Debug, Clone)]
pub struct Response {
    pub blocks: Vec<AssistantBlock>,
    pub usage: Usage,
    /// Normalised: "stop" | "tool_calls" | "length" | "content_filter" | other raw value.
    pub stop_reason: String,
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct LlmError {
    pub message: String,
    pub status: Option<u16>,
    /// Retrying might help (network, 429, 5xx, overloaded, stream cut).
    pub retryable: bool,
    /// Server-provided retry hint.
    pub retry_after: Option<Duration>,
    /// Prompt exceeded the context window.
    pub context_overflow: bool,
    /// Aborted by the cancellation token.
    pub cancelled: bool,
}

impl LlmError {
    pub fn cancelled() -> Self {
        LlmError {
            message: "aborted".into(),
            status: None,
            retryable: false,
            retry_after: None,
            context_overflow: false,
            cancelled: true,
        }
    }
}

/// Shared HTTP client (connection pooling).
pub fn http_client() -> reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(30))
                .pool_idle_timeout(Duration::from_secs(90))
                .user_agent(concat!("ref/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("http client")
        })
        .clone()
}

/// Stream one assistant turn. Calls `on_event` for every streaming event and
/// returns the assembled response. Never retries; the caller owns retry policy.
pub async fn stream(
    provider: &Provider,
    model: &Model,
    req: &Request,
    cancel: &CancellationToken,
    on_event: &mut (dyn FnMut(StreamEvent) + Send),
) -> Result<Response, LlmError> {
    match provider.api {
        ApiKind::OpenAIChat => chat::stream(provider, model, req, cancel, on_event).await,
        ApiKind::OpenAIResponses => responses::stream(provider, model, req, cancel, on_event).await,
        ApiKind::Anthropic => anthropic::stream(provider, model, req, cancel, on_event).await,
    }
}

/// Build the JSON request body without sending (for tests / debugging).
pub fn build_body(provider: &Provider, model: &Model, req: &Request) -> Value {
    match provider.api {
        ApiKind::OpenAIChat => chat::build_body(provider, model, req),
        ApiKind::OpenAIResponses => responses::build_body(provider, model, req),
        ApiKind::Anthropic => anthropic::build_body(provider, model, req),
    }
}
