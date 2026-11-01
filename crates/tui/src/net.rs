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

    /// Sends a fire-and-forget frame. A dead transport is already reported through
    /// `NetEvent::Disconnected`, so the failure is not surfaced here.
    pub fn notify(&self, req: Request) {
        self.send(None, req);
    }
}

const HELLO_ID: u64 = u64::MAX;
const RECONNECT_DELAY: Duration = Duration::from_secs(1);

/// What a text frame from the server means to the transport.
#[allow(clippy::large_enum_variant)]
enum Incoming {
    HelloAccepted,
    HelloRejected(String),
    Frame(ServerFrame),
}

/// Why `pump` returned.
enum PumpExit {
    /// The app is gone; the transport task ends.
    Stop,
    /// The connection broke; reconnect.
    Lost(String),
}

/// Connects to the backend, reporting progress while one is being started.
async fn connect(
    target: &Target,
    events: &mpsc::UnboundedSender<NetEvent>,
) -> Result<(Ws, Endpoint), String> {
    ref_client::connect_target(target, || {
        // Progress text only; a closed receiver is noticed by the next event that matters.
        let _ = events.send(NetEvent::Status("starting backend…".into()));
    })
    .await
    .map_err(|e| e.to_string())
}

/// Parses a server text frame; unknown frame kinds are ignored (forward compat).
fn classify_text_frame(text: &str) -> Option<Incoming> {
    let frame: ServerFrame = serde_json::from_str(text).ok()?;
    let ServerFrame::Response {
        id: HELLO_ID,
        ok,
        error,
        ..
    } = &frame
    else {
        return Some(Incoming::Frame(frame));
    };
    if *ok {
        return Some(Incoming::HelloAccepted);
    }
    Some(Incoming::HelloRejected(
        error.clone().unwrap_or_else(|| "hello rejected".into()),
    ))
}

/// Forwards frames between the app and the socket until either side ends.
async fn pump(
    ws: &mut Ws,
    out: &mut mpsc::UnboundedReceiver<ClientFrame>,
    events: &mpsc::UnboundedSender<NetEvent>,
) -> PumpExit {
    loop {
        tokio::select! {
            f = out.recv() => {
                let Some(f) = f else {
                    // Closing a socket we are about to drop; a failure changes nothing.
                    let _ = ws.close(None).await;
                    return PumpExit::Stop;
                };
                let Ok(txt) = serde_json::to_string(&f) else { continue };
                if let Err(e) = ws.send(WsMsg::text(txt)).await {
                    return PumpExit::Lost(e.to_string());
                }
            }
            m = ws.next() => {
                match m {
                    Some(Ok(WsMsg::Text(t))) => match classify_text_frame(t.as_str()) {
                        None => {}
                        Some(Incoming::HelloRejected(reason)) => return PumpExit::Lost(reason),
                        Some(Incoming::HelloAccepted) => {
                            if events.send(NetEvent::Connected).is_err() {
                                return PumpExit::Stop;
                            }
                        }
                        Some(Incoming::Frame(frame)) => {
                            if events.send(NetEvent::Frame(frame)).is_err() {
                                return PumpExit::Stop;
                            }
                        }
                    },
                    Some(Ok(WsMsg::Ping(p))) => {
                        // A failed pong surfaces as an error on the next `ws.next()`.
                        let _ = ws.send(WsMsg::Pong(p)).await;
                    }
                    Some(Ok(WsMsg::Close(_))) | None => {
                        return PumpExit::Lost("connection closed".into());
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => return PumpExit::Lost(e.to_string()),
                }
            }
        }
    }
}

/// Runs the transport until the app drops the client.
pub async fn run(
    target: Target,
    cwd: String,
    mut out: mpsc::UnboundedReceiver<ClientFrame>,
    events: mpsc::UnboundedSender<NetEvent>,
) {
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
                tokio::time::sleep(RECONNECT_DELAY).await;
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
                notify: Vec::new(),
            },
        };
        let Ok(txt) = serde_json::to_string(&hello) else {
            return;
        };
        if ws.send(WsMsg::text(txt)).await.is_err() {
            tokio::time::sleep(RECONNECT_DELAY).await;
            continue;
        }
        let reason = match pump(&mut ws, &mut out, &events).await {
            PumpExit::Stop => return,
            PumpExit::Lost(reason) => reason,
        };
        if events.send(NetEvent::Disconnected(reason)).is_err() {
            return;
        }
        while out.try_recv().is_ok() {}
        tokio::time::sleep(RECONNECT_DELAY).await;
    }
}
