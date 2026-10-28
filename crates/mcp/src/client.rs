use crate::*;

pub struct McpClient {
    pub name: String,
    pub tools: Vec<McpTool>,
    /// Server-provided `instructions` from initialize.
    pub instructions: Option<String>,
}

impl McpClient {
    pub async fn call(&self, _tool: &str, _args: Value, _cancel: &tokio_util::sync::CancellationToken) -> anyhow::Result<McpCallResult> { todo!() }
    pub async fn shutdown(&self) {}
}

pub(crate) async fn connect(_name: &str, _cfg: &McpConfig, _cwd: &std::path::Path) -> anyhow::Result<Arc<McpClient>> { todo!() }
