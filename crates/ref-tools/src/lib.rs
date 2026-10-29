//! Built-in file/shell tools: read, edit (hashline), write, delete, grep, find, bash.
//!
//! The `purpose` argument is injected/stripped by the session layer; tools here never see it.

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
pub use ref_proto::spec::ToolDef;
use ref_proto::ImagePart;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

mod bash;
mod edit;
mod find;
mod grep;
mod hash;
mod read;
mod store;
mod util;
mod write;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_edge;

pub use hash::file_hash;
pub use store::{Seen, Snapshot, SnapshotStore};

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

impl ToolCtx {
    /// Convenience constructor with a fresh store, no progress channel, no image input.
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        ToolCtx {
            cwd: cwd.into(),
            abort: CancellationToken::new(),
            store: Arc::new(Mutex::new(SnapshotStore::default())),
            image_input: false,
            progress: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ToolOutput {
    pub text: String,
    pub images: Vec<ImagePart>,
    pub is_error: bool,
}

impl ToolOutput {
    pub fn ok(text: impl Into<String>) -> Self {
        ToolOutput {
            text: text.into(),
            images: vec![],
            is_error: false,
        }
    }
    pub fn err(text: impl Into<String>) -> Self {
        ToolOutput {
            text: text.into(),
            images: vec![],
            is_error: true,
        }
    }
}

const READ_DESC: &str = "\
Read a file or list a directory. Text files come back in hashline format: a `[path#TAG]` header \
(4-hex snapshot tag of the whole file) then `N:text` rows (N = 1-based line number; the `N:` prefix is \
not part of the file). Use the header and line numbers with `edit`.
`offset` = first line (1-based), `limit` = number of lines (default 2000; output also stops at 50 KB, \
with a notice saying where to continue). Lines over 2000 chars are cut with `…` and cannot be edited by line number. Images (png/jpg/gif/webp) are returned as images when supported.";

const EDIT_DESC: &str = "\
Patch existing files (new files: `write`). Each file section starts with `[PATH#TAG]` copied from the latest \
`read`/`grep`/`edit` output, followed by hunks. Line numbers are the ORIGINAL numbers of that snapshot, never \
shifted by earlier hunks in the same call. Line endings (CRLF) and BOM are kept automatically.
Hunks:
- `PUT N:` REPLACES line N; `PUT N.=M:` replaces lines N..M (inclusive, M is a line number, not a count)
- `PUT <N:` / `PUT >N:` insert before / after line N without removing anything; `PUT >$:` append at end of file
- `CUT N` / `CUT N.=M` delete lines; `REM` delete the file
Body rows (only after `PUT …:`): `+TEXT` verbatim including indentation; a lone `+` is an empty line. \
The body is only the final content of the targeted lines: no `-old` rows, no unchanged neighbours. \
Touch only lines that `read`/`grep`/`edit` output showed under that tag (bash output does not count); use \
tight ranges and separate hunks for non-adjacent changes; several files may be patched in one call.
After an edit the tag and numbering change: use the new `[path#TAG]` and numbers from the response.
Example:
[src/app.py#1A2B]
PUT 3:
+    return total
PUT >10:
+
+def helper():
+    pass
CUT 14.=15
Fallback: {path, old_string, new_string, replace_all?} performs an exact string replacement.";

const WRITE_DESC: &str = "\
Create or overwrite a file with `content` (parent directories are created). Prefer `edit` for changes \
to existing files. Returns the new `[path#TAG]` header usable by `edit`.";

const DELETE_DESC: &str = "\
Delete a file. Deleting a directory requires `recursive: true`.";

const GREP_DESC: &str = "\
Search file contents with a regex (Rust syntax; `literal: true` for plain text). `path` is a file or \
directory (default: working directory); `glob` filters file names (e.g. `*.rs`). Respects .gitignore, \
skips hidden and binary files. Output per file: a `[path#TAG]` header, then `*N:text` match rows and \
` N:text` context rows; `edit` accepts that header and those line numbers directly.
Default 100 matches (`limit`, max 1000), 20 per file when several files match.";

const FIND_DESC: &str = "\
Find files by glob. A pattern without `/` matches file names at any depth (`*.rs`); with `/` it matches \
the path relative to `path` (`src/**/*.ts`). Respects .gitignore; hidden files only when the pattern starts \
with `.`. No pattern lists all files. Newest first; directories end with `/`. Default limit 200.";

const BASH_DESC: &str = "\
Run a shell command with `bash -c` (non-interactive, no stdin). Combined stdout+stderr is returned; \
long output keeps the head and tail and saves the full output to a file. `timeout` in seconds (default \
300, max 7200, 0 = none). Use `cwd` instead of `cd`. Use read/grep/find instead of cat/grep/find, and \
edit/write instead of sed, heredocs or scripts to change files.";

/// Definitions of all built-in tools (without the `purpose` parameter).
pub fn builtin_defs() -> Vec<ToolDef> {
    let def = |name: &str, desc: &str, params: Value| ToolDef {
        name: name.to_string(),
        description: desc.to_string(),
        parameters: params,
    };
    vec![
        def(
            "read",
            READ_DESC,
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File or directory path (absolute, ~/..., or relative to the working directory)"},
                    "offset": {"type": "integer", "description": "First line to show (1-based)"},
                    "limit": {"type": "integer", "description": "Number of lines to show (default 2000)"}
                },
                "required": ["path"]
            }),
        ),
        def(
            "edit",
            EDIT_DESC,
            json!({
                "type": "object",
                "properties": {
                    "input": {"type": "string", "description": "Hashline patch: one or more `[PATH#TAG]` sections with PUT/CUT/REM hunks"}
                },
                "required": ["input"]
            }),
        ),
        def(
            "write",
            WRITE_DESC,
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path"},
                    "content": {"type": "string", "description": "Full file content"}
                },
                "required": ["path", "content"]
            }),
        ),
        def(
            "delete",
            DELETE_DESC,
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File or directory path"},
                    "recursive": {"type": "boolean", "description": "Required to delete a directory and its contents"}
                },
                "required": ["path"]
            }),
        ),
        def(
            "grep",
            GREP_DESC,
            json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Regex (or literal text with literal=true)"},
                    "path": {"type": "string", "description": "File or directory to search (default: working directory)"},
                    "glob": {"type": "string", "description": "File name glob filter, e.g. `*.rs` or `src/**/*.ts`"},
                    "ignore_case": {"type": "boolean", "description": "Case-insensitive search"},
                    "literal": {"type": "boolean", "description": "Treat pattern as plain text"},
                    "context": {"type": "integer", "description": "Context lines before and after each match (default 0)"},
                    "limit": {"type": "integer", "description": "Maximum matches (default 100, max 1000)"}
                },
                "required": ["pattern"]
            }),
        ),
        def(
            "find",
            FIND_DESC,
            json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Glob, e.g. `*.rs`, `src/**/test_*.py`"},
                    "path": {"type": "string", "description": "Directory to search (default: working directory)"},
                    "type": {"type": "string", "enum": ["file", "dir"], "description": "Only files or only directories"},
                    "limit": {"type": "integer", "description": "Maximum results (default 200, max 2000)"}
                },
                "required": []
            }),
        ),
        def(
            "bash",
            BASH_DESC,
            json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "Command to run"},
                    "timeout": {"type": "integer", "description": "Timeout in seconds (default 300, max 7200, 0 = none)"},
                    "cwd": {"type": "string", "description": "Working directory (default: session working directory)"}
                },
                "required": ["command"]
            }),
        ),
    ]
}

/// Names of the built-in tools.
pub const BUILTIN_TOOLS: &[&str] = &["read", "edit", "write", "delete", "grep", "find", "bash"];

/// Execute a built-in tool. Never panics; all failures become `ToolOutput::err`.
/// Must return promptly when `ctx.abort` is cancelled (bash kills its process group).
pub async fn execute(name: &str, input: Value, ctx: &ToolCtx) -> ToolOutput {
    if ctx.abort.is_cancelled() {
        return ToolOutput::err("[aborted]");
    }
    let input = match input {
        Value::Null => json!({}),
        Value::String(s) => match serde_json::from_str::<Value>(&s) {
            Ok(v @ Value::Object(_)) => v,
            _ => return ToolOutput::err("tool input must be a JSON object"),
        },
        v @ Value::Object(_) => v,
        _ => return ToolOutput::err("tool input must be a JSON object"),
    };
    let fut = async {
        match name {
            "read" => read::run(&input, ctx),
            "edit" => edit::run(&input, ctx),
            "write" => write::run_write(&input, ctx),
            "delete" => write::run_delete(&input, ctx),
            "grep" => grep::run(&input, ctx).await,
            "find" => find::run(&input, ctx).await,
            "bash" => bash::run(&input, ctx).await,
            other => ToolOutput::err(format!("unknown tool `{other}`")),
        }
    };
    // Catch panics so a bug in a tool never takes down the session task.
    match futures_catch(fut).await {
        Ok(out) => out,
        Err(msg) => ToolOutput::err(format!("internal error in tool `{name}`: {msg}")),
    }
}

async fn futures_catch<F: std::future::Future<Output = ToolOutput>>(
    fut: F,
) -> Result<ToolOutput, String> {
    use std::panic::AssertUnwindSafe;
    use std::pin::pin;
    use std::task::Poll;
    let mut fut = pin!(fut);
    std::future::poll_fn(move |cx| {
        match std::panic::catch_unwind(AssertUnwindSafe(|| fut.as_mut().poll(cx))) {
            Ok(Poll::Ready(v)) => Poll::Ready(Ok(v)),
            Ok(Poll::Pending) => Poll::Pending,
            Err(e) => {
                let msg = e
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| e.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "panic".to_string());
                Poll::Ready(Err(msg))
            }
        }
    })
    .await
}
