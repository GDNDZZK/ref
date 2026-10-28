//! LLM wire protocols: OpenAI Chat Completions, OpenAI Responses, Anthropic Messages.
//!
//! The server builds a [`ModelSpec`] (from config) and a [`ChatRequest`] (from session history),
//! then calls [`stream`]. Streaming events are delivered through a callback in order.
//! Retry policy lives in the caller; [`LlmError::kind`] tells it what to do.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub mod sse;
mod chat;
mod responses;
mod anthropic;
pub mod error;

pub use error::{classify, ErrorKind, LlmError};
pub use hn_proto::{ImageData, Usage};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    /// OpenAI Chat Completions (`@ai-sdk/openai-compatible`, most third-party hosts).
    Chat,
    /// OpenAI Responses API (`@ai-sdk/openai`).
    Responses,
    /// Anthropic Messages (`@ai-sdk/anthropic`).
    Anthropic,
}

impl Protocol {
    /// Map an opencode `npm` package name to a protocol.
    pub fn from_npm(npm: &str) -> Protocol {
        match npm {
            "@ai-sdk/anthropic" | "@ai-sdk/google-vertex/anthropic" => Protocol::Anthropic,
            "@ai-sdk/openai" | "@ai-sdk/azure" => Protocol::Responses,
            _ => Protocol::Chat,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Protocol::Chat => "chat",
            Protocol::Responses => "responses",
            Protocol::Anthropic => "anthropic",
        }
    }
}

/// Fully resolved model endpoint + options (variant already merged in).
#[derive(Debug, Clone, Default)]
pub struct ModelSpec {
    pub provider_id: String,
    /// Model id sent on the wire (opencode `models.<key>.id` or the key).
    pub api_model: String,
    pub protocol: Option<Protocol>,
    /// Base URL without trailing slash, e.g. "https://api.openai.com/v1".
    pub base_url: String,
    pub api_key: Option<String>,
    /// Extra HTTP headers (provider.options.headers + model.headers).
    pub headers: BTreeMap<String, String>,
    /// Merged `provider.options` (minus apiKey/baseURL/headers) + `model.options` + selected variant.
    /// camelCase keys as in opencode config; the protocol maps known keys to wire params and
    /// passes the rest through as described in each protocol module.
    pub options: Map<String, Value>,
    /// Extra JSON merged verbatim into the request body (options.body / extraBody).
    pub body: Map<String, Value>,
    /// Context window (tokens).
    pub context: u64,
    /// Max output tokens.
    pub output: u64,
    pub reasoning: bool,
    pub image_input: bool,
    /// Chat: assistant field used to replay reasoning ("reasoning_content"), from opencode `interleaved`.
    pub interleaved_field: Option<String>,
    /// Request timeout for the first byte / between chunks.
    pub chunk_timeout: Option<Duration>,
}

impl ModelSpec {
    pub fn protocol(&self) -> Protocol {
        self.protocol.unwrap_or(Protocol::Chat)
    }
    /// Key identifying which replay data (signatures, encrypted reasoning) is valid for this model.
    pub fn replay_key(&self) -> String {
        format!("{}:{}:{}", self.protocol().as_str(), self.provider_id, self.api_model)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    /// JSON schema (type: object).
    pub parameters: Value,
}

#[derive(Debug, Clone)]
pub enum Content {
    Text(String),
    Image(ImageData),
}

#[derive(Debug, Clone)]
pub enum AssistantPart {
    Text(String),
    /// `replay` is the opaque value from [`StreamEvent::ReasoningEnd`]; only replayed when
    /// [`LlmMessage::Assistant::replay`] is true.
    Reasoning { text: String, replay: Value },
    ToolCall { id: String, name: String, arguments: String },
}

#[derive(Debug, Clone)]
pub enum LlmMessage {
    User(Vec<Content>),
    Assistant {
        parts: Vec<AssistantPart>,
        /// True when this message was produced by the same model (replay_key matches);
        /// reasoning signatures / encrypted items are sent back only then.
        replay: bool,
    },
    ToolResult {
        call_id: String,
        name: String,
        content: String,
        images: Vec<ImageData>,
        is_error: bool,
    },
}

#[derive(Debug, Clone, Default)]
pub struct ChatRequest {
    /// System prompt parts: put stable parts first, volatile parts last (cache friendly).
    pub system: Vec<String>,
    pub messages: Vec<LlmMessage>,
    pub tools: Vec<ToolDef>,
    /// Stable per-session id; used for prompt_cache_key / session affinity headers.
    pub session_id: String,
    /// Overrides spec.output if set.
    pub max_tokens: Option<u64>,
}

/// Streaming events, delivered in order.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Append to the current text part (start a new one if the previous part isn't text).
    TextDelta(String),
    /// Append to the current reasoning part (start a new one if previous part isn't reasoning).
    ReasoningDelta(String),
    /// Reasoning block finished; attach opaque replay data to the most recent reasoning part
    /// (create an empty reasoning part if none is open, e.g. redacted/encrypted-only).
    ReasoningEnd { replay: Value },
    ToolCallStart { id: String, name: String },
    ToolCallDelta { id: String, delta: String },
    /// Final complete arguments JSON text.
    ToolCallEnd { id: String, arguments: String },
    Usage(Usage),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinishReason {
    Stop,
    ToolUse,
    Length,
    ContentFilter,
    Other(String),
}

impl FinishReason {
    pub fn as_str(&self) -> &str {
        match self {
            FinishReason::Stop => "stop",
            FinishReason::ToolUse => "tool_use",
            FinishReason::Length => "length",
            FinishReason::ContentFilter => "content_filter",
            FinishReason::Other(s) => s,
        }
    }
}

/// Send one streaming request. Returns when the stream finishes, errors, or `cancel` fires
/// (then returns `LlmError` with kind `Aborted`).
pub async fn stream(
    client: &reqwest::Client,
    spec: &ModelSpec,
    req: &ChatRequest,
    cancel: &CancellationToken,
    on_event: &mut (dyn FnMut(StreamEvent) + Send),
) -> Result<FinishReason, LlmError> {
    match spec.protocol() {
        Protocol::Chat => chat::stream(client, spec, req, cancel, on_event).await,
        Protocol::Responses => responses::stream(client, spec, req, cancel, on_event).await,
        Protocol::Anthropic => anthropic::stream(client, spec, req, cancel, on_event).await,
    }
}

/// Shared HTTP client builder (rustls, http2, sane timeouts).
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .pool_idle_timeout(Duration::from_secs(90))
        .user_agent(concat!("hernees/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("http client")
}

/// Rough token estimate (chars/4, CJK-aware) for context accounting before usage is known.
pub fn estimate_tokens(text: &str) -> u64 {
    let mut ascii = 0u64;
    let mut other = 0u64;
    for c in text.chars() {
        if c.is_ascii() {
            ascii += 1
        } else {
            other += 1
        }
    }
    ascii / 4 + other * 2 / 3 + 1
}
