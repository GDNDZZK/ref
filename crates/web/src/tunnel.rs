//! One browser WebSocket ⇄ one upstream connection. Frames are copied, never parsed.

use std::sync::Arc;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::State;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::http::header::{HOST, ORIGIN};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use futures_util::{SinkExt as _, StreamExt as _};
use ref_client::Ws;
use tokio::time::{Instant, MissedTickBehavior};
use tokio_tungstenite::tungstenite::Message as UpMessage;
use tokio_tungstenite::tungstenite::protocol::CloseFrame as UpCloseFrame;

use crate::guard::{origin_matches_host, text_response};
use crate::routes::AppState;

/// Same limit as ref-server applies to client messages (prompts carry pasted images).
const MAX_BROWSER_MESSAGE: usize = 64 << 20;
/// "Internal error": the backend could not be reached or went away.
const CLOSE_BACKEND_UNAVAILABLE: u16 = 1011;
/// "Going away": ref-web is shutting down.
const CLOSE_GOING_AWAY: u16 = 1001;
/// A close reason must fit a control frame (125 bytes) next to the 2-byte code.
const MAX_CLOSE_REASON: usize = 123;

/// How often a tab is pinged, and how long it may stay silent before its connection is dropped.
/// A tab that went away without closing (sleeping laptop, dead network) would otherwise keep
/// its upstream connection, and with it a session subscription, until the TCP stack gives up.
#[derive(Clone, Copy)]
pub struct Heartbeat {
    pub ping_interval: Duration,
    pub pong_timeout: Duration,
}

impl Heartbeat {
    pub const DEFAULT: Heartbeat = Heartbeat {
        ping_interval: Duration::from_secs(30),
        pong_timeout: Duration::from_secs(60),
    };
}

pub async fn ws_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let same_origin = headers
        .get(ORIGIN)
        .zip(headers.get(HOST))
        .and_then(|(origin, host)| Some((origin.to_str().ok()?, host.to_str().ok()?)))
        .is_some_and(|(origin, host)| origin_matches_host(origin, host));
    if !same_origin {
        return text_response(StatusCode::FORBIDDEN, "Forbidden: origin not allowed\n");
    }
    upgrade
        .max_message_size(MAX_BROWSER_MESSAGE)
        .max_frame_size(MAX_BROWSER_MESSAGE)
        .on_upgrade(move |browser| {
            let tunnels = state.tunnels.clone();
            tunnels.track_future(tunnel(state, browser))
        })
}

async fn tunnel(state: Arc<AppState>, mut browser: WebSocket) {
    let upstream = tokio::select! {
        r = state.upstream.connect() => r,
        _ = state.shutdown.cancelled() => {
            close_browser(&mut browser, CLOSE_GOING_AWAY, "ref-web is shutting down").await;
            return;
        }
    };
    let mut upstream = match upstream {
        Ok(ws) => ws,
        Err(e) => {
            tracing::warn!("backend connection failed: {e}");
            close_browser(&mut browser, CLOSE_BACKEND_UNAVAILABLE, &e.to_string()).await;
            return;
        }
    };
    let ended = pump(&state, &mut browser, &mut upstream).await;
    match ended {
        Ended::BrowserClosed => {
            let _ = upstream.close(None).await;
        }
        Ended::UpstreamClosed(close) => {
            let (code, reason) = close.unwrap_or((
                CLOSE_BACKEND_UNAVAILABLE,
                "backend connection closed".to_string(),
            ));
            close_browser(&mut browser, code, &reason).await;
        }
        Ended::Shutdown => {
            let _ = upstream.close(None).await;
            close_browser(&mut browser, CLOSE_GOING_AWAY, "ref-web is shutting down").await;
        }
    }
}

enum Ended {
    BrowserClosed,
    /// The close code and reason the backend sent, if any.
    UpstreamClosed(Option<(u16, String)>),
    Shutdown,
}

/// Copies frames both ways until one side ends. Pings from either side are answered by
/// the connection itself and not forwarded. The browser is pinged on its own schedule.
async fn pump(state: &AppState, browser: &mut WebSocket, upstream: &mut Ws) -> Ended {
    let heartbeat = state.heartbeat;
    let mut ticker = tokio::time::interval_at(
        Instant::now() + heartbeat.ping_interval,
        heartbeat.ping_interval,
    );
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut last_pong = Instant::now();
    loop {
        tokio::select! {
            _ = state.shutdown.cancelled() => return Ended::Shutdown,
            _ = ticker.tick() => {
                if last_pong.elapsed() >= heartbeat.pong_timeout {
                    tracing::debug!("browser did not answer pings, dropping it");
                    return Ended::BrowserClosed;
                }
                if browser.send(Message::Ping(Bytes::new())).await.is_err() {
                    return Ended::BrowserClosed;
                }
            }
            frame = browser.recv() => {
                let message = match frame {
                    Some(Ok(Message::Close(_))) | None => return Ended::BrowserClosed,
                    Some(Err(e)) => {
                        tracing::debug!("browser read failed: {e}");
                        return Ended::BrowserClosed;
                    }
                    Some(Ok(Message::Pong(_))) => {
                        last_pong = Instant::now();
                        continue;
                    }
                    Some(Ok(message)) => message,
                };
                let Some(forward) = to_upstream(message) else { continue };
                if let Err(e) = upstream.send(forward).await {
                    tracing::debug!("backend write failed: {e}");
                    return Ended::UpstreamClosed(None);
                }
            }
            frame = upstream.next() => {
                let message = match frame {
                    Some(Ok(UpMessage::Close(close))) => {
                        return Ended::UpstreamClosed(close.map(close_code_and_reason));
                    }
                    Some(Ok(message)) => message,
                    Some(Err(e)) => {
                        tracing::debug!("backend read failed: {e}");
                        return Ended::UpstreamClosed(None);
                    }
                    None => return Ended::UpstreamClosed(None),
                };
                let Some(forward) = to_browser(message) else { continue };
                if browser.send(forward).await.is_err() {
                    return Ended::BrowserClosed;
                }
            }
        }
    }
}

fn to_upstream(message: Message) -> Option<UpMessage> {
    match message {
        Message::Text(text) => Some(UpMessage::text(text.as_str())),
        Message::Binary(bytes) => Some(UpMessage::Binary(bytes)),
        Message::Ping(_) | Message::Pong(_) | Message::Close(_) => None,
    }
}

fn to_browser(message: UpMessage) -> Option<Message> {
    match message {
        UpMessage::Text(text) => Some(Message::text(text.as_str())),
        UpMessage::Binary(bytes) => Some(Message::Binary(bytes)),
        UpMessage::Ping(_) | UpMessage::Pong(_) | UpMessage::Close(_) | UpMessage::Frame(_) => None,
    }
}

fn close_code_and_reason(close: UpCloseFrame) -> (u16, String) {
    (u16::from(close.code), close.reason.to_string())
}

async fn close_browser(browser: &mut WebSocket, code: u16, reason: &str) {
    let reason: String = reason
        .chars()
        .scan(0, |len, c| {
            *len += c.len_utf8();
            (*len <= MAX_CLOSE_REASON).then_some(c)
        })
        .collect();
    let frame = CloseFrame {
        code,
        reason: reason.into(),
    };
    let _ = browser.send(Message::Close(Some(frame))).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guard::{HostPolicy, SessionCookie};
    use crate::upstream::Upstream;
    use axum::Router;
    use axum::routing::get;
    use ref_client::Target;
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;
    use tokio::time::timeout;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
    use tokio_util::sync::CancellationToken;
    use tokio_util::task::TaskTracker;

    const TEST_HEARTBEAT: Heartbeat = Heartbeat {
        ping_interval: Duration::from_millis(50),
        pong_timeout: Duration::from_millis(500),
    };
    const WAIT: Duration = Duration::from_secs(10);
    /// Several pong timeouts: a healthy tab must stay connected for this long.
    const HEALTHY_FOR: Duration = Duration::from_millis(1500);

    /// A backend that reports (once) when its connection ends.
    async fn upstream_that_reports_close() -> (String, oneshot::Receiver<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let (closed_tx, closed_rx) = oneshot::channel();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            while let Some(Ok(message)) = ws.next().await {
                if message.is_close() {
                    break;
                }
            }
            let _ = closed_tx.send(());
        });
        (url, closed_rx)
    }

    /// Serves the tunnel route alone; returns the page address.
    async fn serve_tunnel(upstream_url: String) -> String {
        let state = Arc::new(AppState {
            login_token: String::new(),
            session: SessionCookie::new(0),
            hosts: HostPolicy::new("127.0.0.1", 0),
            cwd: String::new(),
            upstream: Upstream::remote(Target::Explicit {
                url: upstream_url,
                token: None,
            }),
            heartbeat: TEST_HEARTBEAT,
            shutdown: CancellationToken::new(),
            tunnels: TaskTracker::new(),
        });
        let app = Router::new()
            .route("/ws", get(ws_handler))
            .with_state(state);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        address.to_string()
    }

    async fn open_tab(
        address: &str,
    ) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>
    {
        let mut request = format!("ws://{address}/ws").into_client_request().unwrap();
        request
            .headers_mut()
            .insert(ORIGIN, format!("http://{address}").parse().unwrap());
        let (tab, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        tab
    }

    #[tokio::test]
    async fn a_tab_that_never_answers_pings_loses_its_upstream_connection() {
        let (url, closed) = upstream_that_reports_close().await;
        let address = serve_tunnel(url).await;
        // The client library only answers pings while it is being read, so this tab stays mute.
        let _silent_tab = open_tab(&address).await;
        timeout(WAIT, closed)
            .await
            .expect("upstream connection was not closed")
            .unwrap();
    }

    #[tokio::test]
    async fn a_tab_that_answers_pings_keeps_its_upstream_connection() {
        let (url, mut closed) = upstream_that_reports_close().await;
        let address = serve_tunnel(url).await;
        let mut tab = open_tab(&address).await;
        // Reading makes the library answer each ping with a pong.
        let reader = tokio::spawn(async move { while tab.next().await.is_some() {} });
        assert!(timeout(HEALTHY_FOR, &mut closed).await.is_err());
        reader.abort();
    }
}
