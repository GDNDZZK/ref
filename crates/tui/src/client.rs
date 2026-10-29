//! WebSocket connection task: request ids, response routing, auto-reconnect.
//!
//! The app sends [`ClientMsg`]s with an app-defined tag; replies come back on the
//! app channel as [`Incoming::Reply`] carrying that tag, server pushes as
//! [`Incoming::Server`]. Connection state changes are reported as [`Incoming::Conn`].

use futures::{SinkExt, StreamExt};
use hn_proto::{ClientMsg, Request, ServerMsg, PROTOCOL_VERSION};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message as WsMsg;

#[derive(Debug, Clone, PartialEq)]
pub enum ConnState {
    Connecting,
    Connected,
    /// Lost connection; retrying at the given unix ms.
    Reconnecting { attempt: u32, next_at: i64, error: String },
    /// Unrecoverable (e.g. protocol mismatch).
    Failed(String),
}

pub enum Incoming<T> {
    Server(ServerMsg),
    Reply(T, Result<Value, String>),
    /// Connection state change. `Connected` is sent after Hello succeeded.
    Conn(ConnState),
    /// Switched to another backend instance (the old one is gone). Sent before `Connected`.
    Backend { url: String, spawned: bool },
}

struct Outgoing<T> {
    msg: ClientMsg,
    tag: Option<T>,
}

#[derive(Clone)]
pub struct Client<T> {
    tx: mpsc::UnboundedSender<Outgoing<T>>,
}

impl<T: Send + 'static> Client<T> {
    /// Fire and forget (errors are reported with `on_error` tag by the app if desired).
    pub fn send(&self, msg: ClientMsg) {
        let _ = self.tx.send(Outgoing { msg, tag: None });
    }

    /// Send a request; the reply comes back as `Incoming::Reply(tag, ..)`.
    pub fn request(&self, msg: ClientMsg, tag: T) {
        let _ = self.tx.send(Outgoing { msg, tag: Some(tag) });
    }
}

/// A client not connected to anything (requests are dropped). For tests.
#[cfg(test)]
pub fn detached<T>() -> Client<T> {
    let (tx, _rx) = mpsc::unbounded_channel();
    Client { tx }
}

/// Candidate URLs for a ws endpoint (`/` then `/ws`) unless a path was given.
pub fn candidates(base: &str) -> Vec<String> {
    let b = base.trim_end_matches('/');
    let after_scheme = b.split_once("://").map(|x| x.1).unwrap_or(b);
    if after_scheme.contains('/') {
        vec![b.to_string()]
    } else {
        vec![format!("{b}/"), format!("{b}/ws")]
    }
}

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Connect and perform the Hello handshake. Returns the stream and the URL that worked.
pub async fn connect_hello(base: &str, cwd: &str, timeout: Duration) -> Result<(Ws, String), String> {
    let mut last_err = String::from("no endpoint");
    for url in candidates(base) {
        match tokio::time::timeout(timeout, tokio_tungstenite::connect_async(url.as_str())).await {
            Ok(Ok((mut ws, _))) => {
                let hello = Request {
                    id: 0,
                    msg: ClientMsg::Hello { client: "tui".into(), cwd: cwd.into(), protocol: PROTOCOL_VERSION },
                };
                let s = serde_json::to_string(&hello).map_err(|e| e.to_string())?;
                ws.send(WsMsg::text(s)).await.map_err(|e| e.to_string())?;
                // Wait for the Hello response (other pushes may arrive first; they are re-sent after attach).
                let deadline = tokio::time::Instant::now() + timeout;
                loop {
                    let next = tokio::time::timeout_at(deadline, ws.next()).await;
                    match next {
                        Ok(Some(Ok(WsMsg::Text(t)))) => {
                            if let Ok(ServerMsg::Response { id: 0, error, .. }) = serde_json::from_str(t.as_str()) {
                                if let Some(e) = error {
                                    return Err(format!("hello rejected: {e}"));
                                }
                                return Ok((ws, url));
                            }
                        }
                        Ok(Some(Ok(_))) => {}
                        Ok(Some(Err(e))) => {
                            last_err = e.to_string();
                            break;
                        }
                        Ok(None) => {
                            last_err = "closed during handshake".into();
                            break;
                        }
                        Err(_) => {
                            last_err = "handshake timed out".into();
                            break;
                        }
                    }
                }
            }
            Ok(Err(e)) => last_err = e.to_string(),
            Err(_) => last_err = "connect timed out".into(),
        }
    }
    Err(last_err)
}

/// Spawn the connection task. `first` is an already-established stream (optional).
/// `discover`: when the backend becomes unreachable, look for another live instance
/// (or spawn a new one) instead of retrying the same URL forever.
pub fn spawn<T: Send + 'static>(
    base: String,
    cwd: String,
    first: Option<Ws>,
    discover: bool,
    app: mpsc::UnboundedSender<Incoming<T>>,
) -> Client<T> {
    let (tx, rx) = mpsc::unbounded_channel::<Outgoing<T>>();
    tokio::spawn(run(base, cwd, first, discover, rx, app));
    Client { tx }
}

async fn run<T: Send + 'static>(
    mut base: String,
    cwd: String,
    mut first: Option<Ws>,
    discover: bool,
    mut rx: mpsc::UnboundedReceiver<Outgoing<T>>,
    app: mpsc::UnboundedSender<Incoming<T>>,
) {
    let mut next_id: u64 = 1;
    let mut attempt: u32 = 0;
    // The previous connection died right after it was established (e.g. the backend was
    // shutting down): look for / spawn another backend without retrying this one.
    let mut rediscover = false;
    loop {
        let ws = match first.take() {
            Some(ws) => Ok(ws),
            None if discover && std::mem::take(&mut rediscover) => match crate::backend::connect(None, &cwd).await {
                Ok(b) => {
                    base = b.url.clone();
                    let _ = app.send(Incoming::Backend { url: b.url, spawned: b.spawned });
                    Ok(b.ws)
                }
                Err(e) => Err(format!("{e:#}")),
            },
            None => match connect_hello(&base, &cwd, Duration::from_secs(5)).await {
                Ok((ws, _)) => Ok(ws),
                // The instance may have exited (crash, idle shutdown): after a couple of
                // failed attempts find/spawn another backend; sessions are persisted so
                // the app simply re-attaches.
                Err(e) if discover && attempt >= 1 && !e.starts_with("hello rejected") => {
                    match crate::backend::connect(None, &cwd).await {
                        Ok(b) => {
                            base = b.url.clone();
                            let _ = app.send(Incoming::Backend { url: b.url, spawned: b.spawned });
                            Ok(b.ws)
                        }
                        Err(e2) => Err(format!("{e}; {e2:#}")),
                    }
                }
                Err(e) => Err(e),
            },
        };
        let ws = match ws {
            Ok(ws) => ws,
            Err(e) => {
                if e.starts_with("hello rejected") {
                    let _ = app.send(Incoming::Conn(ConnState::Failed(e)));
                    return;
                }
                attempt += 1;
                let delay = backoff(attempt);
                let next_at = hn_proto::now_ms() + delay.as_millis() as i64;
                let _ = app.send(Incoming::Conn(ConnState::Reconnecting { attempt, next_at, error: e }));
                if !wait_dropping(&mut rx, &app, delay).await {
                    return;
                }
                continue;
            }
        };
        attempt = 0;
        let connected_at = std::time::Instant::now();
        let _ = app.send(Incoming::Conn(ConnState::Connected));
        let (mut sink, mut stream) = ws.split();
        let mut pending: HashMap<u64, T> = HashMap::new();
        let mut ping = tokio::time::interval(Duration::from_secs(20));
        ping.tick().await;
        let err: String = loop {
            tokio::select! {
                out = rx.recv() => {
                    let Some(out) = out else { let _ = sink.close().await; return; };
                    let id = next_id;
                    next_id += 1;
                    let req = Request { id, msg: out.msg };
                    let s = match serde_json::to_string(&req) { Ok(s) => s, Err(_) => continue };
                    if let Some(tag) = out.tag { pending.insert(id, tag); }
                    if let Err(e) = sink.send(WsMsg::text(s)).await {
                        if let Some(tag) = pending.remove(&id) {
                            let _ = app.send(Incoming::Reply(tag, Err("disconnected".into())));
                        }
                        break e.to_string();
                    }
                }
                msg = stream.next() => {
                    match msg {
                        Some(Ok(WsMsg::Text(t))) => {
                            match serde_json::from_str::<ServerMsg>(t.as_str()) {
                                Ok(ServerMsg::Response { id, ok, error }) => {
                                    if let Some(tag) = pending.remove(&id) {
                                        let r = match error { Some(e) => Err(e), None => Ok(ok.unwrap_or(Value::Null)) };
                                        let _ = app.send(Incoming::Reply(tag, r));
                                    } else if let Some(e) = error {
                                        let _ = app.send(Incoming::Server(ServerMsg::Toast {
                                            level: hn_proto::ToastLevel::Error, message: e,
                                        }));
                                    }
                                }
                                Ok(m) => { let _ = app.send(Incoming::Server(m)); }
                                Err(_) => {}
                            }
                        }
                        Some(Ok(WsMsg::Ping(p))) => { let _ = sink.send(WsMsg::Pong(p)).await; }
                        Some(Ok(WsMsg::Close(_))) | None => break "connection closed".to_string(),
                        Some(Ok(_)) => {}
                        Some(Err(e)) => break e.to_string(),
                    }
                }
                _ = ping.tick() => {
                    if sink.send(WsMsg::Ping(Vec::new().into())).await.is_err() {
                        break "ping failed".to_string();
                    }
                }
            }
        };
        for (_, tag) in pending.drain() {
            let _ = app.send(Incoming::Reply(tag, Err("disconnected".into())));
        }
        if discover && connected_at.elapsed() < SHORT_LIVED {
            rediscover = true;
        }
        attempt += 1;
        let delay = if rediscover { Duration::from_millis(50) } else { backoff(attempt) };
        let next_at = hn_proto::now_ms() + delay.as_millis() as i64;
        let _ = app.send(Incoming::Conn(ConnState::Reconnecting { attempt, next_at, error: err }));
        if !wait_dropping(&mut rx, &app, delay).await {
            return;
        }
    }
}

/// A connection that ends sooner than this counts as a failed connect.
const SHORT_LIVED: Duration = Duration::from_secs(1);

fn backoff(attempt: u32) -> Duration {
    let ms = (250u64 << attempt.min(5)).min(5000);
    Duration::from_millis(ms)
}

/// Sleep while failing any requests sent in the meantime. Returns false if the app is gone.
async fn wait_dropping<T>(
    rx: &mut mpsc::UnboundedReceiver<Outgoing<T>>,
    app: &mpsc::UnboundedSender<Incoming<T>>,
    d: Duration,
) -> bool {
    let sleep = tokio::time::sleep(d);
    tokio::pin!(sleep);
    loop {
        tokio::select! {
            _ = &mut sleep => return true,
            out = rx.recv() => match out {
                None => return false,
                Some(o) => if let Some(tag) = o.tag {
                    let _ = app.send(Incoming::Reply(tag, Err("not connected".into())));
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_candidates() {
        assert_eq!(candidates("ws://127.0.0.1:9000"), vec!["ws://127.0.0.1:9000/", "ws://127.0.0.1:9000/ws"]);
        assert_eq!(candidates("ws://h:1/socket"), vec!["ws://h:1/socket"]);
    }
}
