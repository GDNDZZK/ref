//! LLM provider clients: Anthropic Messages, OpenAI Chat Completions, OpenAI Responses.
//!
//! The conversation is expressed with `ref_proto::Message` directly. Assistant messages hold
//! text / reasoning / tool parts; a `ToolPart` carries both the call (input) and its result
//! (output / images / state), so providers split it into call + result items.

use std::pin::Pin;
use std::time::Duration;

use futures::Stream;
pub use ref_proto::spec::{ApiKind, Cost, ModelSpec, ToolDef};
use ref_proto::{Message, Usage};
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    #[error("HTTP {status}: {body}")]
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
        todo!()
    }
}

pub type EventStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, LlmError>> + Send>>;

/// Start a streaming request. Errors before the stream starts (HTTP status) are returned
/// directly; errors mid-stream are yielded as `Err` items (after which the stream ends).
/// A stream that ends without a terminal event yields `Err(LlmError::Stream(..))`.
pub async fn stream(http: &reqwest::Client, model: &ModelSpec, req: &Request) -> Result<EventStream, LlmError> {
    let _ = (http, model, req);
    todo!()
}

/// Compute USD cost of a usage record.
pub fn cost_of(cost: &Cost, usage: &Usage) -> f64 {
    (usage.input as f64 * cost.input
        + usage.output as f64 * cost.output
        + usage.cache_read as f64 * cost.cache_read
        + usage.cache_write as f64 * cost.cache_write)
        / 1_000_000.0
}
