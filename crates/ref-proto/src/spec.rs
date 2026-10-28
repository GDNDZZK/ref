//! Resolved configuration types shared between backend crates (not sent on the wire).

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiKind {
    Anthropic,
    OpenAIChat,
    OpenAIResponses,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Cost {
    /// USD per 1M tokens.
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

/// A fully resolved model ready to be called.
#[derive(Debug, Clone)]
pub struct ModelSpec {
    /// Provider key in config (e.g. `zai`).
    pub provider_id: String,
    /// Model key in config (e.g. `glm-5.3`); used in `provider/model` ids.
    pub model_key: String,
    /// Model id sent to the API (config `id` or the key).
    pub api_model: String,
    pub api: ApiKind,
    /// Base URL without trailing slash, e.g. `https://api.anthropic.com/v1`.
    pub base_url: String,
    pub api_key: Option<String>,
    /// Extra HTTP headers (provider options.headers + model headers).
    pub headers: BTreeMap<String, String>,
    /// Merged options: provider `options` minus (apiKey, baseURL, headers, timeout ...) +
    /// model `options` + selected variant options. opencode-style camelCase keys
    /// (`reasoningEffort`, `reasoningSummary`, `textVerbosity`, `include`, `store`,
    /// `thinking: {type, budgetTokens}`, `effort`, ...). Unknown keys are forwarded to the
    /// request body verbatim for chat/responses (e.g. `chat_template_kwargs`).
    pub options: Map<String, Value>,
    /// Extra fields merged into the JSON body last (provider/model `body`/`extraBody`).
    pub extra_body: Map<String, Value>,
    pub context: u64,
    pub max_output: u64,
    pub image_input: bool,
    pub reasoning: bool,
    /// Whether temperature may be sent.
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub cost: Cost,
    /// Request timeout for the initial response headers.
    pub header_timeout: Duration,
    /// Max idle time between stream chunks.
    pub chunk_timeout: Duration,
}

impl ModelSpec {
    /// `provider/model`
    pub fn full_id(&self) -> String {
        format!("{}/{}", self.provider_id, self.model_key)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    /// JSON schema (object).
    pub parameters: Value,
}

/// opencode-compatible MCP server config.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpServerConfig {
    Local {
        command: Vec<String>,
        #[serde(default)]
        environment: BTreeMap<String, String>,
        #[serde(default)]
        cwd: Option<String>,
        #[serde(default)]
        enabled: Option<bool>,
        /// ms
        #[serde(default)]
        timeout: Option<u64>,
    },
    Remote {
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
        #[serde(default)]
        enabled: Option<bool>,
        #[serde(default)]
        timeout: Option<u64>,
    },
}

