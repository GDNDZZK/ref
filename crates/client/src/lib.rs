//! Finding, starting and connecting to `ref-server`, shared by the frontends (TUI, ref-web).
//!
//! ```text
//! Target::Auto      instance files ──> live backend ──> connect
//!                        └ none ──> lock ──> spawn sibling ref-server ──> wait for its instance file ──> connect
//! Target::Explicit  url (+ token) ──> connect, never spawns
//! MockBackend       spawn `ref-server --mock`, read {"port","token"} from its stdout
//! ```

mod instance;
mod mock;
mod spawn;

use std::time::Duration;

use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

pub use mock::MockBackend;

/// Timeout for connecting to an instance found through its instance file.
const DISCOVERED_CONNECT_TIMEOUT: Duration = Duration::from_millis(1500);
/// Timeout for connecting to an explicit `--url`.
const EXPLICIT_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
/// How long a frontend waits for another frontend that is starting the backend.
const SPAWN_LOCK_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a spawned backend gets to publish its instance file.
const BACKEND_START_TIMEOUT: Duration = Duration::from_secs(10);
/// Interval of the "backend is starting" callback.
const STARTING_TICK: Duration = Duration::from_millis(300);
/// Connection attempts to a freshly started backend (it may not accept yet).
const NEW_BACKEND_CONNECT_ATTEMPTS: u32 = 20;
const NEW_BACKEND_CONNECT_PAUSE: Duration = Duration::from_millis(100);

pub type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Where to connect.
#[derive(Debug, Clone)]
pub enum Target {
    /// Discover a running backend or spawn one.
    Auto,
    /// Explicit ws URL (+ optional token); never spawns.
    Explicit { url: String, token: Option<String> },
}

/// A resolved backend address.
#[derive(Debug, Clone)]
pub struct Endpoint {
    pub url: String,
    pub token: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error("no free port")]
    NoFreePort,
    #[error("failed to start {binary}: {source}")]
    SpawnFailed {
        binary: String,
        source: std::io::Error,
    },
    #[error("backend did not start within {}s", BACKEND_START_TIMEOUT.as_secs())]
    StartTimeout,
    #[error("could not connect to the new backend")]
    CannotConnectNew,
    #[error("cannot connect to {url}: {reason}")]
    CannotConnect { url: String, reason: String },
    #[error("cannot start the mock backend: {0}")]
    MockStartFailed(String),
}

impl Endpoint {
    /// The URL with the token as query parameter (how `ref-server` authenticates the upgrade).
    fn url_with_token(&self) -> String {
        match &self.token {
            Some(t) if !t.is_empty() => {
                let sep = if self.url.contains('?') { '&' } else { '?' };
                format!("{}{sep}token={t}", self.url)
            }
            _ => self.url.clone(),
        }
    }
}

/// No size limits on incoming frames: a session snapshot carries every image of the session
/// (tool screenshots included) and easily exceeds tungstenite's 16 MiB default frame limit.
fn ws_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(None)
        .max_frame_size(None)
}

/// Opens a WebSocket to `endpoint`.
pub async fn connect(endpoint: &Endpoint, timeout: Duration) -> Result<Ws, ConnectError> {
    let attempt = tokio::time::timeout(
        timeout,
        tokio_tungstenite::connect_async_with_config(
            endpoint.url_with_token(),
            Some(ws_config()),
            false,
        ),
    )
    .await;
    let reason = match attempt {
        Ok(Ok((ws, _))) => return Ok(ws),
        Ok(Err(e)) => e.to_string(),
        Err(_) => "timed out".to_string(),
    };
    Err(ConnectError::CannotConnect {
        url: endpoint.url.clone(),
        reason,
    })
}

/// Connects to `target`. `on_starting_backend` is called every 300ms while a spawned backend
/// is starting.
pub async fn connect_target(
    target: &Target,
    on_starting_backend: impl FnMut(),
) -> Result<(Ws, Endpoint), ConnectError> {
    match target {
        Target::Auto => discover(on_starting_backend).await,
        Target::Explicit { url, token } => {
            let endpoint = Endpoint {
                url: url.clone(),
                token: token.clone(),
            };
            let ws = connect(&endpoint, EXPLICIT_CONNECT_TIMEOUT).await?;
            Ok((ws, endpoint))
        }
    }
}

/// Connects to the newest live instance that accepts us.
async fn connect_existing(dir: &std::path::Path) -> Option<(Ws, Endpoint)> {
    for instance in instance::instances(dir) {
        let endpoint = instance::endpoint_for(&instance);
        if let Ok(ws) = connect(&endpoint, DISCOVERED_CONNECT_TIMEOUT).await {
            return Some((ws, endpoint));
        }
    }
    None
}

/// Discovers a running backend, or launches one.
async fn discover(mut on_starting_backend: impl FnMut()) -> Result<(Ws, Endpoint), ConnectError> {
    let dir = ref_proto::instances_dir();
    if let Some(found) = connect_existing(&dir).await {
        return Ok(found);
    }
    // Only one frontend spawns; the others wait for the lock and then find its instance.
    let _lock = spawn::SpawnLock::acquire(&dir, SPAWN_LOCK_TIMEOUT).await;
    if let Some(found) = connect_existing(&dir).await {
        return Ok(found);
    }
    let port = spawn::pick_port().ok_or(ConnectError::NoFreePort)?;
    let started = std::time::Instant::now();
    spawn::spawn_server(port).map_err(|source| ConnectError::SpawnFailed {
        binary: spawn::server_binary().display().to_string(),
        source,
    })?;
    let instance = loop {
        if let Some(i) = spawn::wait_for_instance(&dir, port, STARTING_TICK).await {
            break i;
        }
        on_starting_backend();
        if started.elapsed() > BACKEND_START_TIMEOUT {
            return Err(ConnectError::StartTimeout);
        }
    };
    let endpoint = instance::endpoint_for(&instance);
    for _ in 0..NEW_BACKEND_CONNECT_ATTEMPTS {
        if let Ok(ws) = connect(&endpoint, DISCOVERED_CONNECT_TIMEOUT).await {
            return Ok((ws, endpoint));
        }
        tokio::time::sleep(NEW_BACKEND_CONNECT_PAUSE).await;
    }
    Err(ConnectError::CannotConnectNew)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_texts_name_their_cause_and_limits() {
        let refused = ConnectError::CannotConnect {
            url: "ws://127.0.0.1:1/".into(),
            reason: "connection refused".into(),
        };
        assert_eq!(
            refused.to_string(),
            "cannot connect to ws://127.0.0.1:1/: connection refused"
        );
        assert_eq!(
            ConnectError::StartTimeout.to_string(),
            "backend did not start within 10s"
        );
    }

    #[tokio::test]
    async fn connect_error_carries_the_reason() {
        let free_port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let endpoint = Endpoint {
            url: format!("ws://127.0.0.1:{free_port}/"),
            token: Some("secret".into()),
        };
        let error = connect(&endpoint, Duration::from_secs(3))
            .await
            .err()
            .unwrap();
        let text = error.to_string();
        assert!(
            text.starts_with(&format!("cannot connect to ws://127.0.0.1:{free_port}/: ")),
            "{text}"
        );
        assert!(
            !text.contains("secret"),
            "the token stays out of messages: {text}"
        );
    }
}
