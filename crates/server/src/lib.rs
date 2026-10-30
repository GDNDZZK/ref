//! ref-server library: the backend of the ref coding agent.

pub mod agent_switch;
pub mod api;
pub mod compaction;
pub mod convert;
pub mod engine;
pub mod hub;
pub mod images;
pub mod instance;
pub mod mcp;
pub mod mock;
pub mod models;
mod notify;
pub mod ops;
pub mod peers;
pub mod prompt;
pub mod session;
pub mod store;
pub mod title;
pub mod toolset;
pub mod util;
pub mod ws;

use std::net::SocketAddr;
use std::sync::Arc;

pub use hub::{Hub, HubOptions};

/// A running server (hub + listener task).
pub struct Running {
    pub hub: Arc<Hub>,
    pub addr: SocketAddr,
    pub task: tokio::task::JoinHandle<()>,
}

/// Bind the listener and start serving.
pub async fn start(
    opts: HubOptions,
    bind: &str,
    port: u16,
    auth: ws::Auth,
) -> anyhow::Result<Running> {
    let listener = tokio::net::TcpListener::bind((bind, port)).await?;
    let addr = listener.local_addr()?;
    let hub = Hub::new(opts);
    let task = tokio::spawn(ws::serve(hub.clone(), listener, Arc::new(auth)));
    Ok(Running { hub, addr, task })
}
