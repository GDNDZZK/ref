//! WebSocket transport. One task owns the socket (with discovery, auto-spawn and reconnect);
//! the app talks to it through `Client`. Server frames — including responses — are delivered
//! in socket order on a single channel, so a snapshot response is always processed before
//! the events that follow it.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use ref_proto::{ClientFrame, PROTOCOL_VERSION, Request, ServerFrame};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message as WsMsg;

use crate::launcher::{self, Endpoint, Target};

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum NetEvent {
    /// Connected and Hello acknowledged.
    Connected,
    Disconnected(String),
    /// Progress text ("starting backend…").
    Status(String),
    Frame(ServerFrame),
}

/// Handle used by the app to send frames.
#[derive(Clone)]
pub struct Client {
    tx: mpsc::UnboundedSender<ClientFrame>,
}

impl Client {
    pub fn new(tx: mpsc::UnboundedSender<ClientFrame>) -> Self {
        Client { tx }
    }

    /// Sends a frame. Returns false if the transport task is gone.
    pub fn send(&self, id: Option<u64>, req: Request) -> bool {
        self.tx.send(ClientFrame { id, req }).is_ok()
    }
}

const HELLO_ID: u64 = u64::MAX;

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

fn url_with_token(ep: &Endpoint) -> String {
    match &ep.token {
        Some(t) if !t.is_empty() => {
            let sep = if ep.url.contains('?') { '&' } else { '?' };
            format!("{}{sep}token={t}", ep.url)
        }
        _ => ep.url.clone(),
    }
}

/// No size limits on incoming frames: a session snapshot carries every image of the session
/// (tool screenshots included) and easily exceeds tungstenite's 16 MiB default frame limit.
fn ws_config() -> tokio_tungstenite::tungstenite::protocol::WebSocketConfig {
    tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(None)
        .max_frame_size(None)
}

async fn try_connect(ep: &Endpoint, timeout: Duration) -> Option<Ws> {
    let url = url_with_token(ep);
    match tokio::time::timeout(
        timeout,
        tokio_tungstenite::connect_async_with_config(url, Some(ws_config()), false),
    )
    .await
    {
        Ok(Ok((ws, _))) => Some(ws),
        _ => None,
    }
}

async fn connect_existing(dir: &std::path::Path) -> Option<(Ws, Endpoint)> {
    for inst in launcher::instances(dir) {
        let ep = launcher::endpoint_for(&inst);
        if let Some(ws) = try_connect(&ep, Duration::from_millis(1500)).await {
            return Some((ws, ep));
        }
    }
    None
}

/// Discovers a running backend, or launches one.
async fn discover(
    events: &mpsc::UnboundedSender<NetEvent>,
    allow_spawn: bool,
) -> Result<(Ws, Endpoint), String> {
    let dir = ref_proto::instances_dir();
    if let Some(c) = connect_existing(&dir).await {
        return Ok(c);
    }
    if !allow_spawn {
        return Err("no backend running".into());
    }
    // Only one frontend spawns; the others wait for the lock and then find its instance.
    let _lock = launcher::SpawnLock::acquire(&dir, Duration::from_secs(15)).await;
    if let Some(c) = connect_existing(&dir).await {
        return Ok(c);
    }
    let port = launcher::pick_port().ok_or("no free port")?;
    let started = std::time::Instant::now();
    launcher::spawn_server(port).map_err(|e| {
        format!(
            "failed to start {}: {e}",
            launcher::server_binary().display()
        )
    })?;
    let wait = async {
        loop {
            if let Some(i) =
                launcher::wait_for_instance(&dir, port, Duration::from_millis(300)).await
            {
                return Some(i);
            }
            let _ = events.send(NetEvent::Status("starting backend…".into()));
            if started.elapsed() > Duration::from_secs(10) {
                return None;
            }
        }
    };
    let inst = wait.await.ok_or("backend did not start within 10s")?;
    let ep = launcher::endpoint_for(&inst);
    for _ in 0..20 {
        if let Some(ws) = try_connect(&ep, Duration::from_millis(1500)).await {
            return Ok((ws, ep));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err("could not connect to the new backend".into())
}

/// Runs the transport until the app drops the client.
pub async fn run(
    target: Target,
    cwd: String,
    mut out: mpsc::UnboundedReceiver<ClientFrame>,
    events: mpsc::UnboundedSender<NetEvent>,
) {
    let mut first = true;
    loop {
        let conn = match &target {
            Target::Auto => discover(&events, true).await,
            Target::Explicit { url, token } => {
                let ep = Endpoint {
                    url: url.clone(),
                    token: token.clone(),
                };
                try_connect(&ep, Duration::from_secs(3))
                    .await
                    .map(|ws| (ws, ep))
                    .ok_or_else(|| format!("cannot connect to {url}"))
            }
        };
        let (mut ws, ep) = match conn {
            Ok(c) => c,
            Err(e) => {
                if events.send(NetEvent::Disconnected(e)).is_err() {
                    return;
                }
                // Drain (fail) frames sent while disconnected: the app fails their callbacks.
                while out.try_recv().is_ok() {}
                tokio::time::sleep(Duration::from_secs(1)).await;
                if events.is_closed() {
                    return;
                }
                continue;
            }
        };
        let hello = ClientFrame {
            id: Some(HELLO_ID),
            req: Request::Hello {
                client: "ref-tui".into(),
                version: PROTOCOL_VERSION,
                cwd: cwd.clone(),
                token: ep.token.clone(),
            },
        };
        let Ok(txt) = serde_json::to_string(&hello) else {
            return;
        };
        if ws.send(WsMsg::text(txt)).await.is_err() {
            tokio::time::sleep(Duration::from_secs(1)).await;
            continue;
        }
        let _ = first;
        first = false;
        let mut greeted = false;
        let reason: String = loop {
            tokio::select! {
                f = out.recv() => {
                    let Some(f) = f else {
                        let _ = ws.close(None).await;
                        return;
                    };
                    let Ok(txt) = serde_json::to_string(&f) else { continue };
                    if let Err(e) = ws.send(WsMsg::text(txt)).await {
                        break e.to_string();
                    }
                }
                m = ws.next() => {
                    match m {
                        Some(Ok(WsMsg::Text(t))) => {
                            let frame: ServerFrame = match serde_json::from_str(t.as_str()) {
                                Ok(f) => f,
                                Err(_) => continue, // unknown frame kind: ignore (forward compat)
                            };
                            if let ServerFrame::Response { id: HELLO_ID, ok, error, .. } = &frame {
                                if !*ok {
                                    break error.clone().unwrap_or_else(|| "hello rejected".into());
                                }
                                greeted = true;
                                if events.send(NetEvent::Connected).is_err() {
                                    return;
                                }
                                continue;
                            }
                            if events.send(NetEvent::Frame(frame)).is_err() {
                                return;
                            }
                        }
                        Some(Ok(WsMsg::Ping(p))) => {
                            let _ = ws.send(WsMsg::Pong(p)).await;
                        }
                        Some(Ok(WsMsg::Close(_))) | None => break "connection closed".into(),
                        Some(Ok(_)) => {}
                        Some(Err(e)) => break e.to_string(),
                    }
                }
            }
        };
        let _ = greeted;
        if events.send(NetEvent::Disconnected(reason)).is_err() {
            return;
        }
        while out.try_recv().is_ok() {}
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
