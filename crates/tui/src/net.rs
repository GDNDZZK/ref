//! WebSocket transport. One task owns the socket (with discovery, auto-spawn and reconnect);
//! the app talks to it through `Client`. Server frames — including responses — are delivered
//! in socket order on a single channel, so a snapshot response is always processed before
//! the events that follow it.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use ref_proto::{ClientFrame, PROTOCOL_VERSION, Request, ServerFrame};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message as WsMsg;

use ref_client::{Endpoint, Target, Ws};

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

/// Connects to the backend, reporting progress while one is being started.
async fn connect(
    target: &Target,
    events: &mpsc::UnboundedSender<NetEvent>,
) -> Result<(Ws, Endpoint), String> {
    ref_client::connect_target(target, || {
        let _ = events.send(NetEvent::Status("starting backend…".into()));
    })
    .await
    .map_err(|e| e.to_string())
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
        let conn = connect(&target, &events).await;
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
