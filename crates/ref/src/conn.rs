//! WebSocket client task with automatic reconnect.

use std::time::Duration;

use futures::{SinkExt, StreamExt};
use ref_proto::{ClientMsg, ServerMsg, PROTOCOL_VERSION};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message as WsMsg;

#[derive(Debug)]
pub enum ConnEvent {
    /// Connected and Hello sent.
    Connected,
    Msg(Box<ServerMsg>),
    Disconnected(String),
    /// Fatal startup problem (keeps retrying, but tells the UI).
    Error(String),
}

#[derive(Clone)]
pub struct Conn {
    tx: mpsc::UnboundedSender<ClientMsg>,
}

impl Conn {
    pub fn send(&self, m: ClientMsg) {
        let _ = self.tx.send(m);
    }

    /// A connection whose outgoing messages are captured by the returned receiver (tests).
    #[cfg(test)]
    pub fn capture() -> (Conn, mpsc::UnboundedReceiver<ClientMsg>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Conn { tx }, rx)
    }
}

pub struct Target {
    /// Explicit URL (`--url`), else discovery/spawn.
    pub url: Option<String>,
    pub token: Option<String>,
    pub cwd: String,
}

pub fn start(target: Target) -> (Conn, mpsc::UnboundedReceiver<ConnEvent>) {
    let (tx, mut out_rx) = mpsc::unbounded_channel::<ClientMsg>();
    let (ev_tx, ev_rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut backoff = Duration::from_millis(300);
        loop {
            let url = match &target.url {
                Some(u) => crate::backend::with_token(u, target.token.as_deref()),
                None => match crate::backend::discover_or_spawn().await {
                    Ok(u) => u,
                    Err(e) => {
                        let _ = ev_tx.send(ConnEvent::Error(format!("{e:#}")));
                        tokio::time::sleep(backoff).await;
                        backoff = (backoff * 2).min(Duration::from_secs(5));
                        continue;
                    }
                },
            };
            match tokio_tungstenite::connect_async(url.as_str()).await {
                Ok((ws, _)) => {
                    backoff = Duration::from_millis(300);
                    let (mut sink, mut stream) = ws.split();
                    let hello = ClientMsg::Hello { client: "ref-tui".into(), cwd: target.cwd.clone(), protocol: PROTOCOL_VERSION };
                    let ok = match serde_json::to_string(&hello) {
                        Ok(s) => sink.send(WsMsg::Text(s.into())).await.is_ok(),
                        Err(_) => false,
                    };
                    if !ok {
                        let _ = ev_tx.send(ConnEvent::Disconnected("handshake failed".into()));
                        tokio::time::sleep(backoff).await;
                        continue;
                    }
                    // Drop messages queued while disconnected: they refer to stale state.
                    while out_rx.try_recv().is_ok() {}
                    if ev_tx.send(ConnEvent::Connected).is_err() {
                        return;
                    }
                    let mut ping = tokio::time::interval(Duration::from_secs(20));
                    ping.tick().await;
                    let reason = loop {
                        tokio::select! {
                            m = out_rx.recv() => {
                                let Some(m) = m else { return };
                                let s = match serde_json::to_string(&m) { Ok(s) => s, Err(_) => continue };
                                if let Err(e) = sink.send(WsMsg::Text(s.into())).await {
                                    break e.to_string();
                                }
                            }
                            r = stream.next() => match r {
                                Some(Ok(WsMsg::Text(t))) => {
                                    match serde_json::from_str::<ServerMsg>(&t) {
                                        Ok(m) => { if ev_tx.send(ConnEvent::Msg(Box::new(m))).is_err() { return; } }
                                        Err(_) => { /* unknown message type from a newer backend: ignore */ }
                                    }
                                }
                                Some(Ok(WsMsg::Ping(p))) => { let _ = sink.send(WsMsg::Pong(p)).await; }
                                Some(Ok(WsMsg::Close(_))) | None => break "connection closed".to_string(),
                                Some(Ok(_)) => {}
                                Some(Err(e)) => break e.to_string(),
                            },
                            _ = ping.tick() => {
                                if sink.send(WsMsg::Ping(Vec::new().into())).await.is_err() {
                                    break "ping failed".to_string();
                                }
                            }
                        }
                    };
                    if ev_tx.send(ConnEvent::Disconnected(reason)).is_err() {
                        return;
                    }
                }
                Err(e) => {
                    let _ = ev_tx.send(ConnEvent::Error(format!("connect {}: {e}", redact(&url))));
                }
            }
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(Duration::from_secs(5));
        }
    });
    (Conn { tx }, ev_rx)
}

fn redact(url: &str) -> String {
    match url.split_once("token=") {
        Some((a, _)) => format!("{a}token=…"),
        None => url.to_string(),
    }
}
