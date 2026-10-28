//! Built-in tools (hashline file tools + bash) and the [`Tool`] trait shared with the server
//! (which implements session/peer/MCP tools on top of it).

use async_trait::async_trait;
use hn_proto::ImageData;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub mod hashline;
mod read;
mod write;
mod edit;
mod delete;
mod grep;
mod find;
mod bash;
pub mod util;
#[cfg(test)]
mod test_support;

pub use hashline::EditStore;

/// Execution context for one tool call.
pub struct ToolCtx {
    /// Session working directory (relative paths resolve against it).
    pub cwd: PathBuf,
    pub session_id: String,
    pub call_id: String,
    /// Fires when the user aborts. Long-running tools must stop promptly.
    pub cancel: CancellationToken,
    /// Per-session hashline snapshot store.
    pub store: Arc<EditStore>,
    /// Stream partial output to the UI (e.g. bash stdout). Cheap to call often.
    pub on_output: Arc<dyn Fn(&str) + Send + Sync>,
    /// Model supports image input (read may return images).
    pub image_input: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ToolOutput {
    /// Text returned to the model.
    pub text: String,
    pub images: Vec<ImageData>,
    /// Short UI title (e.g. "src/main.rs:1-300", "$ cargo build").
    pub title: String,
    /// Tool reported a failure (still sent back to the model, loop continues).
    pub is_error: bool,
}

impl ToolOutput {
    pub fn text(t: impl Into<String>) -> Self {
        ToolOutput { text: t.into(), ..Default::default() }
    }
    pub fn error(t: impl Into<String>) -> Self {
        ToolOutput { text: t.into(), is_error: true, ..Default::default() }
    }
    pub fn with_title(mut self, t: impl Into<String>) -> Self {
        self.title = t.into();
        self
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> String;
    /// JSON schema for the input object, WITHOUT the `purpose` field (server injects it).
    fn parameters(&self) -> Value;
    /// String matched against permission patterns (bash: the command; file tools: the path).
    fn permission_subject(&self, _input: &Value) -> String {
        "*".into()
    }
    /// Short UI title derived from the input, shown before/while running.
    fn title(&self, _input: &Value) -> String {
        String::new()
    }
    /// Run the tool. `Err` is converted to an error result for the model (loop continues).
    async fn run(&self, input: Value, ctx: &ToolCtx) -> anyhow::Result<ToolOutput>;
}

/// All built-in tools: read, edit, write, delete, grep, find, bash.
pub fn builtin_tools() -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(read::ReadTool),
        Arc::new(edit::EditTool),
        Arc::new(write::WriteTool),
        Arc::new(delete::DeleteTool),
        Arc::new(grep::GrepTool),
        Arc::new(find::FindTool),
        Arc::new(bash::BashTool),
    ]
}
