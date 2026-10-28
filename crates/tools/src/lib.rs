//! Built-in coding tools (hashline-style read/edit, write, delete, grep, find, bash).
//!
//! Public API in this file is a contract used by `ref-server`; keep it stable.

use ref_proto::ImageData;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

mod bash;
mod delete;
mod edit;
mod find;
mod grep;
pub mod hashline;
mod read;
mod util;
mod write;

pub use hashline::HashStore;

/// Static description of a tool exposed to the model.
#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: String,
    /// JSON schema object (`{"type":"object","properties":{..},"required":[..]}`)
    /// WITHOUT the `purpose` field — the server adds `purpose` to every tool.
    pub parameters: Value,
}

/// Execution context for one tool call.
#[derive(Clone)]
pub struct ToolCtx {
    /// Session working directory; relative paths resolve against it.
    pub cwd: PathBuf,
    /// Per-session snapshot store for hashline tags.
    pub store: Arc<HashStore>,
    pub cancel: CancellationToken,
    /// Optional live-output sink (bash streams combined stdout/stderr chunks here).
    pub on_output: Option<Arc<dyn Fn(&str) + Send + Sync>>,
}

#[derive(Debug, Clone, Default)]
pub struct ToolOutput {
    /// Text returned to the model.
    pub text: String,
    pub images: Vec<ImageData>,
    /// Display metadata for the frontend (e.g. `{"diff": "...", "exit_code": 0, "path": ".."}`).
    pub meta: Option<Value>,
    /// True if the tool failed. The text then contains the error message for the model.
    pub is_error: bool,
}

impl ToolOutput {
    pub fn ok(text: impl Into<String>) -> Self {
        ToolOutput { text: text.into(), ..Default::default() }
    }
    pub fn err(text: impl Into<String>) -> Self {
        ToolOutput { text: text.into(), is_error: true, ..Default::default() }
    }
}

/// All built-in tool specs, in a stable order (stable order keeps prompt cache hits).
pub fn specs() -> Vec<ToolSpec> {
    vec![
        read::spec(),
        edit::spec(),
        write::spec(),
        delete::spec(),
        grep::spec(),
        find::spec(),
        bash::spec(),
    ]
}

pub fn is_builtin(name: &str) -> bool {
    matches!(name, "read" | "edit" | "write" | "delete" | "grep" | "find" | "bash")
}

/// Execute a built-in tool. Never panics; all failures become `ToolOutput { is_error: true }`.
/// `input` has already had `purpose` removed.
pub async fn execute(name: &str, input: Value, ctx: &ToolCtx) -> ToolOutput {
    let res = match name {
        "read" => read::run(input, ctx).await,
        "edit" => edit::run(input, ctx).await,
        "write" => write::run(input, ctx).await,
        "delete" => delete::run(input, ctx).await,
        "grep" => grep::run(input, ctx).await,
        "find" => find::run(input, ctx).await,
        "bash" => bash::run(input, ctx).await,
        _ => Err(anyhow::anyhow!("unknown tool: {name}")),
    };
    match res {
        Ok(o) => o,
        Err(e) => ToolOutput::err(format!("Error: {e:#}")),
    }
}
