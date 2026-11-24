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
mod rpc;
mod spawn;

use std::time::Duration;

use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

pub use mock::MockBackend;
pub use ref_proto::PROTOCOL_VERSION;
pub use rpc::{ClientError, ClientEvent, MessagesPage, RefClient};
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Discover a running backend or spawn one.
    Auto,
    /// Explicit ws URL (+ optional token); never spawns.
    Explicit { url: String, token: Option<String> },
}

/// High-level backend target, including a private `ref-server --mock` child.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendTarget {
    /// Discover a running local backend or spawn one on demand.
    Auto,
    /// Explicit `ws://` / `wss://` URL (+ optional token); never scans instances or spawns.
    Explicit { url: String, token: Option<String> },
    /// Spawn a private `ref-server --mock` child process.
    Mock,
}

impl BackendTarget {
    pub fn is_explicit(&self) -> bool {
        matches!(self, BackendTarget::Explicit { .. })
    }

    pub fn is_local(&self) -> bool {
        !self.is_explicit()
    }
}

impl From<Target> for BackendTarget {
    fn from(target: Target) -> Self {
        match target {
            Target::Auto => BackendTarget::Auto,
            Target::Explicit { url, token } => BackendTarget::Explicit { url, token },
        }
    }
}

/// A resolved backend address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub url: String,
    pub token: Option<String>,
}

/// Result of [`discover_or_connect`], carrying an optional private mock backend handle.
pub struct ConnectedBackend {
    pub ws: Ws,
    pub endpoint: Endpoint,
    pub mock: Option<MockBackend>,
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
    #[error("{0}")]
    StateDir(String),
    #[error("invalid backend URL {url:?}: {reason}")]
    InvalidUrl { url: String, reason: String },
}
impl Endpoint {
    /// The URL with the token as query parameter (how `ref-server` authenticates the upgrade).
    fn url_with_token(&self) -> String {
        let url = with_root_path(&self.url);
        match &self.token {
            Some(t) if !t.is_empty() => {
                let sep = if url.contains('?') { '&' } else { '?' };
                format!("{url}{sep}token={t}")
            }
            _ => url,
        }
    }
}

/// `ws://host:port` and `ws://host:port?x=1` become `ws://host:port/` and `ws://host:port/?x=1`:
/// without a path the handshake request line would start with `?`, which the server rejects.
fn with_root_path(url: &str) -> String {
    let authority_start = url.find("://").map_or(0, |i| i + "://".len());
    let rest = &url[authority_start..];
    match rest.find(['/', '?']) {
        Some(i) if rest.as_bytes()[i] == b'/' => url.to_string(),
        Some(i) => format!("{}/{}", &url[..authority_start + i], &rest[i..]),
        None => format!("{url}/"),
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

/// Validates that `url` is a well-formed `ws://` or `wss://` WebSocket URL with a host.
pub fn validate_explicit_url(url: &str) -> Result<(), ConnectError> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(ConnectError::InvalidUrl {
            url: url.to_string(),
            reason: "URL is empty".into(),
        });
    }
    let rest = if trimmed.len() >= 5 && trimmed[..5].eq_ignore_ascii_case("ws://") {
        &trimmed[5..]
    } else if trimmed.len() >= 6 && trimmed[..6].eq_ignore_ascii_case("wss://") {
        &trimmed[6..]
    } else {
        return Err(ConnectError::InvalidUrl {
            url: url.to_string(),
            reason: "URL must start with ws:// or wss://".into(),
        });
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host_part = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let has_host = if let Some(after_open) = host_part.strip_prefix('[') {
        after_open
            .split_once(']')
            .is_some_and(|(inside, _)| !inside.is_empty())
    } else {
        let host = host_part.split(':').next().unwrap_or("");
        !host.is_empty()
    };
    if !has_host {
        return Err(ConnectError::InvalidUrl {
            url: url.to_string(),
            reason: "URL is missing a host".into(),
        });
    }
    Ok(())
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
            let trimmed = url.trim().to_string();
            validate_explicit_url(&trimmed)?;
            let endpoint = Endpoint {
                url: trimmed,
                token: token.clone(),
            };
            let ws = connect(&endpoint, EXPLICIT_CONNECT_TIMEOUT).await?;
            Ok((ws, endpoint))
        }
    }
}

/// Connects to `target`, skipping local `instances` discovery and spawning whenever
/// `target` is [`BackendTarget::Explicit`].
pub async fn discover_or_connect(
    target: &BackendTarget,
    on_starting_backend: impl FnMut(),
) -> Result<ConnectedBackend, ConnectError> {
    match target {
        BackendTarget::Auto => {
            let (ws, endpoint) = discover(on_starting_backend).await?;
            Ok(ConnectedBackend {
                ws,
                endpoint,
                mock: None,
            })
        }
        BackendTarget::Explicit { url, token } => {
            let trimmed = url.trim().to_string();
            validate_explicit_url(&trimmed)?;
            let endpoint = Endpoint {
                url: trimmed,
                token: token.clone(),
            };
            let ws = connect(&endpoint, EXPLICIT_CONNECT_TIMEOUT).await?;
            Ok(ConnectedBackend {
                ws,
                endpoint,
                mock: None,
            })
        }
        BackendTarget::Mock => {
            let mock = MockBackend::spawn().await?;
            let endpoint = mock.endpoint().clone();
            match connect(&endpoint, EXPLICIT_CONNECT_TIMEOUT).await {
                Ok(ws) => Ok(ConnectedBackend {
                    ws,
                    endpoint,
                    mock: Some(mock),
                }),
                Err(e) => {
                    let _ = mock.shutdown().await;
                    Err(e)
                }
            }
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
    let dir = ref_proto::instances_dir().map_err(ConnectError::StateDir)?;
    if let Some(found) = connect_existing(&dir).await {
        return Ok(found);
    }
    // Only one frontend spawns; the others wait for the lock and then find its instance.
    // Without the lock (timeout or IO error) we proceed unlocked rather than hang: the worst
    // case is a second backend.
    let _lock = match spawn::SpawnLock::acquire(&dir, SPAWN_LOCK_TIMEOUT).await {
        Ok(lock) => Some(lock),
        Err(spawn::SpawnLockError::Timeout) => None,
        Err(spawn::SpawnLockError::Io(e)) => {
            tracing::debug!("cannot take the spawn lock: {e}");
            None
        }
    };
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
    fn a_url_without_a_path_gets_the_root_path_before_the_token() {
        let url = |url: &str, token: Option<&str>| {
            Endpoint {
                url: url.into(),
                token: token.map(Into::into),
            }
            .url_with_token()
        };
        assert_eq!(url("ws://h:1", None), "ws://h:1/");
        assert_eq!(url("ws://h:1", Some("t")), "ws://h:1/?token=t");
        assert_eq!(url("ws://h:1/", Some("t")), "ws://h:1/?token=t");
        assert_eq!(url("ws://h:1?x=1", Some("t")), "ws://h:1/?x=1&token=t");
        assert_eq!(url("ws://h:1/p?x=1", Some("t")), "ws://h:1/p?x=1&token=t");
        assert_eq!(url("ws://[::1]:1", None), "ws://[::1]:1/");
    }

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

    #[test]
    fn explicit_url_validation_accepts_valid_and_rejects_invalid() {
        assert!(validate_explicit_url("ws://127.0.0.1:8080").is_ok());
        assert!(validate_explicit_url("wss://example.com/ws").is_ok());
        assert!(validate_explicit_url("ws://[::1]:9000/").is_ok());
        assert!(validate_explicit_url("").is_err());
        assert!(validate_explicit_url("http://127.0.0.1:8080").is_err());
        assert!(validate_explicit_url("ws://").is_err());
        assert!(validate_explicit_url("ws:///path").is_err());
        assert!(validate_explicit_url("ws://:8080").is_err());
        assert!(validate_explicit_url("ws://[]").is_err());
    }

    #[tokio::test]
    async fn discover_or_connect_explicit_skips_instances_and_never_spawns() {
        let free_port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let target = BackendTarget::Explicit {
            url: format!("ws://127.0.0.1:{free_port}/"),
            token: None,
        };
        let mut starting_called = false;
        let err = discover_or_connect(&target, || {
            starting_called = true;
        })
        .await
        .err()
        .unwrap();
        assert!(
            !starting_called,
            "explicit target must never spawn local backend"
        );
        assert!(matches!(err, ConnectError::CannotConnect { .. }));
    }
}
