//! `refd`: the backend daemon of the ref coding-agent harness.
//!
//! Architecture:
//! - [`server`]: axum HTTP/WebSocket server (access control, per-client reader/writer tasks).
//! - [`hub`]: global state shared by all clients: connected clients, open sessions, history
//!   index, config cache, MCP managers, session-list subscriptions and idle-timeout tracking.
//! - [`session`]: one [`session::SessionCore`] per open session (state behind a short-lived
//!   mutex, never held across `.await`), the agent loop (`session::engine`), session tools,
//!   system prompt and persistence.
//! - [`peers`]: target resolution and formatting for inter-session messaging.
//! - [`registry`]: `<data_dir>/instances/<pid>.json`, `--status` and `--stop`.

pub mod cli;
pub mod hub;
pub mod peers;
pub mod registry;
pub mod server;
pub mod session;
pub mod util;
