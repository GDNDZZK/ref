//! Built-in file/shell tools: read, edit (hashline), write, delete, grep, find, bash.
//!
//! The `purpose` argument is injected/stripped by the session layer; tools here never see it.

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
pub use ref_proto::spec::ToolDef;
use ref_proto::ImagePart;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Per-session snapshot store for hashline tags (see hashline notes).
#[derive(Default)]
pub struct SnapshotStore {
    // implementation-defined
}

pub struct ToolCtx {
    /// Session working directory; relative paths resolve against it.
    pub cwd: PathBuf,
    pub abort: CancellationToken,
    pub store: Arc<Mutex<SnapshotStore>>,
    /// Whether the current model accepts images (read returns images only if true).
    pub image_input: bool,
    /// Live output chunks (bash stdout/stderr) for UI display. May be None.
    pub progress: Option<mpsc::UnboundedSender<String>>,
}

#[derive(Debug, Clone, Default)]
pub struct ToolOutput {
    pub text: String,
    pub images: Vec<ImagePart>,
    pub is_error: bool,
}

impl ToolOutput {
    pub fn ok(text: impl Into<String>) -> Self {
        ToolOutput { text: text.into(), images: vec![], is_error: false }
    }
    pub fn err(text: impl Into<String>) -> Self {
        ToolOutput { text: text.into(), images: vec![], is_error: true }
    }
}

/// Definitions of all built-in tools (without the `purpose` parameter).
pub fn builtin_defs() -> Vec<ToolDef> {
    todo!()
}

/// Names of the built-in tools.
pub const BUILTIN_TOOLS: &[&str] = &["read", "edit", "write", "delete", "grep", "find", "bash"];

/// Execute a built-in tool. Never panics; all failures become `ToolOutput::err`.
/// Must return promptly when `ctx.abort` is cancelled (bash kills its process group).
pub async fn execute(name: &str, input: Value, ctx: &ToolCtx) -> ToolOutput {
    let _ = (name, input, ctx);
    todo!()
}
