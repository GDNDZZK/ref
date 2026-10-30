//! Where tabs get their backend connection from.

use std::time::Duration;

use ref_client::{ConnectError, MockBackend, Target, Ws};
use tokio::sync::Mutex;

const MOCK_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

pub enum Upstream {
    /// The shared backend (found or started) or an explicit `--server`.
    Remote(Target),
    /// A private `ref-server --mock`, started on demand.
    Mock(Box<Mutex<Option<MockBackend>>>),
}

impl Upstream {
    pub fn remote(target: Target) -> Self {
        Upstream::Remote(target)
    }

    pub fn mock() -> Self {
        Upstream::Mock(Box::new(Mutex::new(None)))
    }

    pub fn is_mock(&self) -> bool {
        matches!(self, Upstream::Mock(_))
    }

    /// Stops the mock backend, if there is one, so that it cleans up after itself.
    pub async fn shutdown(&self) {
        let Upstream::Mock(slot) = self else { return };
        if let Some(backend) = slot.lock().await.take() {
            backend.shutdown().await;
        }
    }

    /// Opens a new connection to the backend, starting it when needed.
    pub async fn connect(&self) -> Result<Ws, ConnectError> {
        match self {
            Upstream::Remote(target) => {
                let (ws, _) = ref_client::connect_target(target, || {}).await?;
                Ok(ws)
            }
            Upstream::Mock(slot) => connect_mock(slot).await,
        }
    }
}

/// The mock exits after its last client leaves, so a later tab may find it gone (or going):
/// start a new one, once, if connecting fails.
async fn connect_mock(slot: &Mutex<Option<MockBackend>>) -> Result<Ws, ConnectError> {
    let mut slot = slot.lock().await;
    let mut restarted = false;
    loop {
        let backend = running_mock(&mut slot).await?;
        match ref_client::connect(backend.endpoint(), MOCK_CONNECT_TIMEOUT).await {
            Ok(ws) => return Ok(ws),
            Err(e) if restarted => return Err(e),
            Err(_) => {
                *slot = None;
                restarted = true;
            }
        }
    }
}

async fn running_mock(slot: &mut Option<MockBackend>) -> Result<&MockBackend, ConnectError> {
    let running = match slot.as_mut() {
        Some(backend) => !backend
            .has_exited()
            .map_err(|e| ConnectError::MockStartFailed(e.to_string()))?,
        None => false,
    };
    if !running {
        return Ok(slot.insert(MockBackend::spawn().await?));
    }
    Ok(slot.as_ref().expect("checked above"))
}
