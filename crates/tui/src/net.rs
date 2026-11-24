//! WebSocket transport. One task owns the socket (with discovery, auto-spawn and reconnect);
//! the app talks to it through `Client`. Server frames — including responses — are delivered
//! in socket order on a single channel, so a snapshot response is always processed before
//! the events that follow it.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use ref_proto::{ClientFrame, PROTOCOL_VERSION, Request, ServerFrame};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message as WsMsg;

use ref_client::{BackendTarget, ConnectedBackend, Target, Ws};

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum NetEvent {
    /// Connected and Hello acknowledged.
    Connected,
    /// Backend speaks an older protocol than this client (`protocol < PROTOCOL_VERSION`).
    BackendTooOld {
        protocol: u32,
    },
    Disconnected(String),
    /// Server refused the handshake due to a protocol version mismatch; do not reconnect.
    Refused(String),
    /// Progress text ("starting backend…").
    Status(String),
    /// Non-fatal transport warning (e.g. malformed server frame).
    Warn(String),
    Frame(ServerFrame),
}
/// Handle used by the app to send frames and switch backend targets.
#[derive(Clone)]
pub struct Client {
    tx: mpsc::UnboundedSender<ClientFrame>,
    ctrl: Option<mpsc::UnboundedSender<BackendTarget>>,
}

impl Client {
    pub fn new(tx: mpsc::UnboundedSender<ClientFrame>) -> Self {
        Client { tx, ctrl: None }
    }

    pub fn with_control(
        tx: mpsc::UnboundedSender<ClientFrame>,
        ctrl: mpsc::UnboundedSender<BackendTarget>,
    ) -> Self {
        Client {
            tx,
            ctrl: Some(ctrl),
        }
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

    /// Asks the transport task to drop the current connection and connect to `target`.
    pub fn switch_target(&self, target: BackendTarget) -> bool {
        self.ctrl.as_ref().is_some_and(|tx| tx.send(target).is_ok())
    }
}
const HELLO_ID: u64 = u64::MAX;
const RECONNECT_DELAY: Duration = Duration::from_secs(1);

/// What a text frame from the server means to the transport.
enum Incoming {
    HelloAccepted { protocol: u32 },
    HelloRejected(String),
    HelloMismatch(String),
    Frame(Box<ServerFrame>),
}

/// Why `pump` returned.
enum PumpExit {
    /// The app is gone; the transport task ends.
    Stop,
    /// The connection broke; reconnect.
    Lost(String),
    /// Server refused this client's protocol version; stop retrying.
    Fatal(String),
    /// The user requested a switch to another backend target.
    Switch(BackendTarget),
}
/// Connects to the backend, reporting progress while one is being started.
async fn connect(
    target: &BackendTarget,
    events: &mpsc::UnboundedSender<NetEvent>,
) -> Result<ConnectedBackend, String> {
    ref_client::discover_or_connect(target, || {
        // Progress text only; a closed receiver is noticed by the next event that matters.
        let _ = events.send(NetEvent::Status("starting backend…".into()));
    })
    .await
    .map_err(|e| e.to_string())
}
/// Parses a server text frame; unknown frame kinds are ignored (forward compat).
fn classify_text_frame(text: &str) -> Result<Option<Incoming>, String> {
    let raw: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("invalid server frame: {e}"))?;
    let frame: ServerFrame = match serde_json::from_value(raw) {
        Ok(f) => f,
        Err(e) => {
            if e.to_string().contains("unknown variant") {
                return Ok(None);
            }
            return Err(format!("malformed server frame: {e}"));
        }
    };
    let ServerFrame::Response {
        id: HELLO_ID,
        ok,
        result,
        error,
        code,
    } = &frame
    else {
        return Ok(Some(Incoming::Frame(Box::new(frame))));
    };
    if *ok {
        let protocol = result
            .get("protocol")
            .and_then(|v| v.as_u64())
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or(1);
        return Ok(Some(Incoming::HelloAccepted { protocol }));
    }
    let reason = error.clone().unwrap_or_else(|| "hello rejected".into());
    if code.as_deref() == Some(ref_proto::ERR_PROTOCOL_MISMATCH) {
        return Ok(Some(Incoming::HelloMismatch(reason)));
    }
    Ok(Some(Incoming::HelloRejected(reason)))
}
async fn pump(
    ws: &mut Ws,
    out: &mut mpsc::UnboundedReceiver<ClientFrame>,
    switch_rx: &mut mpsc::UnboundedReceiver<BackendTarget>,
    events: &mpsc::UnboundedSender<NetEvent>,
) -> PumpExit {
    loop {
        tokio::select! {
            biased;
            sw = switch_rx.recv() => {
                let Some(next_target) = sw else {
                    let _ = ws.close(None).await;
                    return PumpExit::Stop;
                };
                let _ = ws.close(None).await;
                return PumpExit::Switch(next_target);
            }
            f = out.recv() => {
                let Some(f) = f else {
                    // Closing a socket we are about to drop; a failure changes nothing.
                    let _ = ws.close(None).await;
                    return PumpExit::Stop;
                };
                let txt = match serde_json::to_string(&f) {
                    Ok(t) => t,
                    Err(e) => {
                        if let Some(id) = f.id {
                            let frame = ServerFrame::Response {
                                id,
                                ok: false,
                                result: serde_json::Value::Null,
                                error: Some(msg),
                                code: None,
                            };
                            if events.send(NetEvent::Frame(frame)).is_err() {
                                return PumpExit::Stop;
                            }
                            continue;
                        }
                        return PumpExit::Lost(msg);
                    }
                };
                if let Err(e) = ws.send(WsMsg::text(txt)).await {
                    return PumpExit::Lost(e.to_string());
                }
            }
            m = ws.next() => {
                match m {
                    Some(Ok(WsMsg::Text(t))) => match classify_text_frame(t.as_str()) {
                        Ok(None) => {}
                        Err(msg) => {
                            if events.send(NetEvent::Warn(msg)).is_err() {
                                return PumpExit::Stop;
                            }
                        }
                        Ok(Some(Incoming::HelloMismatch(reason))) => return PumpExit::Fatal(reason),
                        Ok(Some(Incoming::HelloRejected(reason))) => return PumpExit::Lost(reason),
                        Ok(Some(Incoming::HelloAccepted { protocol })) => {
                            if events.send(NetEvent::Connected).is_err() {
                                return PumpExit::Stop;
                            }
                            if protocol < PROTOCOL_VERSION
                                && events.send(NetEvent::BackendTooOld { protocol }).is_err()
                            {
                                return PumpExit::Stop;
                            }
                        }
                        Ok(Some(Incoming::Frame(frame))) => {
                            if events.send(NetEvent::Frame(*frame)).is_err() {
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
    out: mpsc::UnboundedReceiver<ClientFrame>,
    events: mpsc::UnboundedSender<NetEvent>,
) {
    let (_switch_tx, switch_rx) = mpsc::unbounded_channel();
    run_with_control(target.into(), cwd, out, switch_rx, events).await;
}

async fn shutdown_mock(
    mock: Option<ref_client::MockBackend>,
    events: &mpsc::UnboundedSender<NetEvent>,
) {
    if let Some(m) = mock
        && let Err(e) = m.shutdown().await
    {
        let _ = events.send(NetEvent::Warn(format!(
            "failed to shut down mock backend: {e}"
        )));
    }
}

enum SwitchOrWait {
    Switched(BackendTarget),
    Proceed,
    Stop,
}

async fn await_switch_or_delay(
    switch_rx: &mut mpsc::UnboundedReceiver<BackendTarget>,
    out: &mut mpsc::UnboundedReceiver<ClientFrame>,
    delay: Duration,
) -> SwitchOrWait {
    tokio::select! {
        biased;
        sw = switch_rx.recv() => match sw {
            Some(next_target) => {
                while out.try_recv().is_ok() {}
                SwitchOrWait::Switched(next_target)
            }
            None => SwitchOrWait::Stop,
        },
        _ = tokio::time::sleep(delay) => SwitchOrWait::Proceed,
    }
}

async fn await_switch_while_refused(
    switch_rx: &mut mpsc::UnboundedReceiver<BackendTarget>,
    out: &mut mpsc::UnboundedReceiver<ClientFrame>,
) -> Option<BackendTarget> {
    loop {
        tokio::select! {
            biased;
            sw = switch_rx.recv() => {
                let next_target = sw?;
                while out.try_recv().is_ok() {}
                return Some(next_target);
            }
            f = out.recv() => {
                f?;
            }
        }
    }
}

/// Runs the transport with runtime backend hot-switching support.
pub async fn run_with_control(
    mut target: BackendTarget,
    cwd: String,
    mut out: mpsc::UnboundedReceiver<ClientFrame>,
    mut switch_rx: mpsc::UnboundedReceiver<BackendTarget>,
    events: mpsc::UnboundedSender<NetEvent>,
) {
    loop {
        let conn = tokio::select! {
            biased;
            sw = switch_rx.recv() => {
                let Some(next_target) = sw else {
                    return;
                };
                target = next_target;
                while out.try_recv().is_ok() {}
                continue;
            }
            res = connect(&target, &events) => res,
        };
        let mut backend = match conn {
            Ok(c) => c,
            Err(e) => {
                if events.send(NetEvent::Disconnected(e)).is_err() {
                    return;
                }
                while out.try_recv().is_ok() {}
                match await_switch_or_delay(&mut switch_rx, &mut out, RECONNECT_DELAY).await {
                    SwitchOrWait::Switched(next) => target = next,
                    SwitchOrWait::Proceed => {}
                    SwitchOrWait::Stop => return,
                }
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
                token: backend.endpoint.token.clone(),
                notify: Vec::new(),
            },
        };
        let txt = match serde_json::to_string(&hello) {
            Ok(t) => t,
            Err(e) => {
                shutdown_mock(backend.mock.take(), &events).await;
                let _ = events.send(NetEvent::Disconnected(format!("cannot encode hello: {e}")));
                return;
            }
        };
        if let Err(e) = backend.ws.send(WsMsg::text(txt)).await {
            shutdown_mock(backend.mock.take(), &events).await;
            if events.send(NetEvent::Disconnected(e.to_string())).is_err() {
                return;
            }
            match await_switch_or_delay(&mut switch_rx, &mut out, RECONNECT_DELAY).await {
                SwitchOrWait::Switched(next) => target = next,
                SwitchOrWait::Proceed => {}
                SwitchOrWait::Stop => return,
            }
            continue;
        }
        let exit = pump(&mut backend.ws, &mut out, &mut switch_rx, &events).await;
        shutdown_mock(backend.mock.take(), &events).await;
        let reason = match exit {
            PumpExit::Stop => return,
            PumpExit::Switch(next_target) => {
                target = next_target;
                while out.try_recv().is_ok() {}
                continue;
            }
            PumpExit::Fatal(reason) => {
                let _ = events.send(NetEvent::Refused(reason));
                while out.try_recv().is_ok() {}
                let Some(next_target) = await_switch_while_refused(&mut switch_rx, &mut out).await
                else {
                    return;
                };
                target = next_target;
                continue;
            }
            PumpExit::Lost(reason) => reason,
        };
        if events.send(NetEvent::Disconnected(reason)).is_err() {
            return;
        }
        while out.try_recv().is_ok() {}
        match await_switch_or_delay(&mut switch_rx, &mut out, RECONNECT_DELAY).await {
            SwitchOrWait::Switched(next) => target = next,
            SwitchOrWait::Proceed => {}
            SwitchOrWait::Stop => return,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_text_frame_surfaces_invalid_and_ignores_unknown_variant() {
        assert!(classify_text_frame("not json").is_err());
        assert!(classify_text_frame(r#"{"kind":"response","id":"bad"}"#).is_err());
        assert!(matches!(
            classify_text_frame(r#"{"kind":"future_frame","x":1}"#),
            Ok(None)
        ));
        assert!(matches!(
            classify_text_frame(r#"{"kind":"toast","level":"info","text":"hi"}"#),
            Ok(Some(Incoming::Frame(_)))
        ));
    }
}
