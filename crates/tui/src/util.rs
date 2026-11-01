//! Names and pure helpers shared by the client layers and the in-process mock backend.

/// Error text a message carries when its turn was aborted (matched case-insensitively).
pub const ABORTED_ERROR: &str = "Aborted";

/// Status strings of an MCP server as the backend reports them.
pub mod mcp_status {
    pub const CONNECTED: &str = "connected";
    pub const CONNECTING: &str = "connecting";
    pub const FAILED: &str = "failed";
    pub const DISABLED: &str = "disabled";
}
