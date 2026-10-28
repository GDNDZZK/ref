//! Test helpers: temp-dir contexts and a runner that maps `Err` to an error output (as the server does).

use std::sync::{Arc, Mutex};

use crate::{EditStore, Tool, ToolCtx, ToolOutput};

pub fn ctx() -> (tempfile::TempDir, ToolCtx) {
    let (d, c, _) = ctx_with_output();
    (d, c)
}

pub fn ctx_with_output() -> (tempfile::TempDir, ToolCtx, Arc<Mutex<String>>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let sink = Arc::new(Mutex::new(String::new()));
    let s2 = sink.clone();
    let ctx = ToolCtx {
        cwd: dir.path().to_path_buf(),
        session_id: "test".into(),
        call_id: format!("call-{}", std::process::id()),
        cancel: Default::default(),
        store: Arc::new(EditStore::new()),
        on_output: Arc::new(move |s: &str| s2.lock().unwrap().push_str(s)),
        image_input: true,
    };
    (dir, ctx, sink)
}

pub async fn run_tool(tool: &dyn Tool, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
    match tool.run(input, ctx).await {
        Ok(o) => o,
        Err(e) => ToolOutput::error(e.to_string()),
    }
}
