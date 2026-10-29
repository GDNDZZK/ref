//! LLM provider clients: Anthropic Messages, OpenAI Chat Completions, OpenAI Responses.
//!
//! The conversation is expressed with `ref_proto::Message` directly. Assistant messages hold
//! text / reasoning / tool parts; a `ToolPart` carries both the call (input) and its result
//! (output / images / state), so providers split it into call + result items.

use std::pin::Pin;
use std::time::Duration;

use futures::{Stream, StreamExt};
pub use ref_proto::spec::{ApiKind, Cost, ModelSpec, ToolDef};
use ref_proto::{Message, MessageOrigin, Part, Role, TextPart, Usage};
use serde::{Deserialize, Serialize};
use serde_json::Value;

mod anthropic;
mod convert;
mod error;
mod openai_chat;
mod openai_responses;
pub mod sse;
mod transport;

#[cfg(test)]
mod tests;

pub use convert::parse_tool_input;
pub use error::text_is_context_overflow;

#[derive(Debug, Clone)]
pub struct Request {
    /// System prompt blocks, most stable first. Joined with "\n\n" where the API needs one string.
    pub system: Vec<String>,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDef>,
    /// Stable session id: used for prompt cache keys and session-affinity headers.
    pub session_id: String,
    /// Force the model not to call tools (tools are still sent to keep the cache prefix).
    pub tool_choice_none: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    ToolUse,
    Length,
    /// Provider reported an error / refusal / content filter as stop reason.
    Error,
}

/// Streaming events. `index` identifies a content block within one response; for each index
/// the sequence is `*Delta`* then exactly one `*End`.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    TextDelta { index: usize, text: String },
    /// `meta`: opaque replay data stored in `TextPart.meta` (e.g. responses message id).
    TextEnd { index: usize, meta: Option<Value> },
    ReasoningDelta { index: usize, text: String },
    /// `meta`: replay data stored in `ReasoningPart.meta` (anthropic signature / redacted data,
    /// responses reasoning item incl. encrypted_content, chat reasoning field/details).
    ReasoningEnd { index: usize, meta: Option<Value> },
    ToolCallStart { index: usize, call_id: String, name: String },
    ToolCallDelta { index: usize, json: String },
    /// `input` is the parsed arguments (`{}` when empty). If the JSON was invalid, `input` is
    /// `Value::String(raw)` so the caller can report the error to the model.
    ToolCallEnd { index: usize, call_id: String, name: String, input: Value, meta: Option<Value> },
    /// Usage so far (may be sent several times; last one wins).
    Usage(Usage),
    Finish { reason: FinishReason },
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum LlmError {
    /// `body` is the raw (truncated) response body; `Display` shows a readable digest of it.
    #[error("HTTP {status}: {}", error::describe_http(*.status, .body))]
    Http { status: u16, body: String, retry_after: Option<Duration> },
    #[error("network error: {0}")]
    Network(String),
    #[error("stream error: {0}")]
    Stream(String),
    #[error("timeout: {0}")]
    Timeout(String),
    #[error("invalid configuration: {0}")]
    Config(String),
}

impl LlmError {
    /// Server-requested retry delay (Retry-After / retry-after-ms headers), if any.
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            LlmError::Http { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    /// True when the error says the prompt exceeds the context window.
    pub fn is_context_overflow(&self) -> bool {
        self.overflow()
    }
}

pub type EventStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, LlmError>> + Send>>;

/// Start a streaming request. Errors before the stream starts (HTTP status) are returned
/// directly; errors mid-stream are yielded as `Err` items (after which the stream ends).
/// A stream that ends without a terminal event yields `Err(LlmError::Stream(..))`.
pub async fn stream(http: &reqwest::Client, model: &ModelSpec, req: &Request) -> Result<EventStream, LlmError> {
    match model.api {
        ApiKind::Anthropic => anthropic::stream(http, model, req).await,
        ApiKind::OpenAIChat => openai_chat::stream(http, model, req).await,
        ApiKind::OpenAIResponses => openai_responses::stream(http, model, req).await,
    }
}

/// The JSON request body that [`stream`] would send (for debugging / logging).
pub fn request_body(model: &ModelSpec, req: &Request) -> Value {
    match model.api {
        ApiKind::Anthropic => anthropic::build_body(model, req).body,
        ApiKind::OpenAIChat => openai_chat::build_body(model, req),
        ApiKind::OpenAIResponses => openai_responses::build_body(model, req),
    }
}

/// The HTTP headers [`stream`] would send, with credentials redacted (for debugging / logging).
pub fn request_headers(model: &ModelSpec, req: &Request) -> Result<Vec<(String, String)>, LlmError> {
    let h = match model.api {
        ApiKind::Anthropic => anthropic::build_headers(model, &anthropic::build_body(model, req), &req.session_id)?,
        ApiKind::OpenAIChat => openai_chat::build_headers(model, req)?,
        ApiKind::OpenAIResponses => openai_responses::build_headers(model, req)?,
    };
    Ok(h.redacted())
}

/// One-shot, tool-less completion that returns the concatenated response text (e.g. for
/// generating session titles). Reasoning is discarded.
pub async fn complete_text(
    http: &reqwest::Client,
    model: &ModelSpec,
    system: &str,
    prompt: &str,
) -> Result<String, LlmError> {
    let mut msg = Message::new(Role::User, MessageOrigin::User);
    msg.parts.push(Part::Text(TextPart { id: ref_proto::new_id("prt"), text: prompt.to_string(), time: msg.time, meta: None }));
    let req = Request {
        system: if system.is_empty() { vec![] } else { vec![system.to_string()] },
        messages: vec![msg],
        tools: vec![],
        session_id: String::new(),
        tool_choice_none: false,
    };
    let mut events = stream(http, model, &req).await?;
    let mut text = String::new();
    while let Some(ev) = events.next().await {
        match ev? {
            StreamEvent::TextDelta { text: t, .. } => text.push_str(&t),
            StreamEvent::Finish { reason: FinishReason::Error } => {
                return Err(LlmError::Stream("provider stopped with an error finish reason".into()))
            }
            _ => {}
        }
    }
    Ok(text.trim().to_string())
}

/// Fill in `usage.cost` from the model's pricing.
pub(crate) fn with_cost(model: &ModelSpec, mut usage: Usage) -> Usage {
    usage.cost = cost_of(&model.cost, &usage);
    usage
}

/// Compute USD cost of a usage record.
pub fn cost_of(cost: &Cost, usage: &Usage) -> f64 {
    (usage.input as f64 * cost.input
        + usage.output as f64 * cost.output
        + usage.cache_read as f64 * cost.cache_read
        + usage.cache_write as f64 * cost.cache_write)
        / 1_000_000.0
}
