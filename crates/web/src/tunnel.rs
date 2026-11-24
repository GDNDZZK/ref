//! One browser WebSocket ⇄ one upstream connection. The browser proves it knows the key first
//! (`channel`); then every frame is sealed on the browser side and copied, never parsed, on
//! the upstream side.

use std::sync::Arc;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::http::header::{HOST, ORIGIN};
use axum::response::Response;
use futures_util::{SinkExt as _, StreamExt as _};
use ref_client::Ws;
use serde::{Deserialize, Serialize};
use tokio::sync::OwnedSemaphorePermit;
use tokio::time::{Instant, MissedTickBehavior};
use tokio_tungstenite::tungstenite::Error as WsError;
use tokio_tungstenite::tungstenite::Message as UpMessage;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame as UpCloseFrame, WebSocketConfig};

use crate::channel::{self, BadFrame, Kind, NONCE_LEN, Opener, Sealer};
use crate::guard::{origin_matches_host, text_response};
use crate::routes::AppState;
use crate::token;
use crate::upstream::UpstreamRequest;
use crate::ws::{self, BrowserSocket};
/// Same limit as ref-server applies to client messages (prompts carry pasted images).
const MAX_BROWSER_MESSAGE: usize = 64 << 20;
/// Pre-auth WebSocket buffer sizes; widened to `MAX_BROWSER_MESSAGE` after the key handshake.
const PRE_AUTH_BUFFER: usize = 4096;
/// "Internal error": the backend could not be reached or went away.
const CLOSE_BACKEND_UNAVAILABLE: u16 = 1011;
/// "Going away": ref-web is shutting down.
const CLOSE_GOING_AWAY: u16 = 1001;
const SHUTDOWN_REASON: &str = "ref-web is shutting down";
/// Application close codes; the page knows them (`web/src/net/channel.ts`).
const CLOSE_BAD_MESSAGE: u16 = 4400;
const CLOSE_KEY_REJECTED: u16 = 4401;
const CLOSE_HANDSHAKE_TIMEOUT: u16 = 4408;
/// Close reasons are fixed texts: details go to the log, not to a browser that may not know
/// the key.
const REASON_BACKEND_UNAVAILABLE: &str = "backend unavailable";
const REASON_KEY_REJECTED: &str = "key rejected";
const REASON_BAD_HANDSHAKE: &str = "bad handshake";
const REASON_BAD_FRAME: &str = "bad frame";
const REASON_HANDSHAKE_TIMEOUT: &str = "handshake timed out";
/// Unauthenticated tunnels at once; more are refused with 503.
pub const MAX_HANDSHAKES: usize = 16;
/// The handshake messages are tiny.
const MAX_HANDSHAKE_MESSAGE: usize = 1024;
/// Pause before a rejected key is answered: limits guessing on one connection.
const WRONG_KEY_PAUSE: Duration = Duration::from_secs(1);
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// A close reason must fit a control frame (125 bytes) next to the 2-byte code.
const MAX_CLOSE_REASON: usize = 123;

/// How often a tab is pinged, and how long it may stay silent (no frame at all) or refuse
/// data before its connection is dropped.
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

fn pre_auth_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .read_buffer_size(PRE_AUTH_BUFFER)
        .write_buffer_size(PRE_AUTH_BUFFER)
        .max_message_size(Some(MAX_HANDSHAKE_MESSAGE))
        .max_frame_size(Some(MAX_HANDSHAKE_MESSAGE))
}

fn post_auth_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(MAX_BROWSER_MESSAGE))
        .max_frame_size(Some(MAX_BROWSER_MESSAGE))
}

pub async fn ws_handler(State(state): State<Arc<AppState>>, req: Request) -> Response {
    let headers = req.headers();
    let same_origin = headers
        .get(ORIGIN)
        .zip(headers.get(HOST))
        .and_then(|(origin, host)| Some((origin.to_str().ok()?, host.to_str().ok()?)))
        .is_some_and(|(origin, host)| origin_matches_host(origin, host, state.hosts.tls()));
    if !same_origin {
        return text_response(StatusCode::FORBIDDEN, "Forbidden: origin not allowed\n");
    }
    let Ok(permit) = state.handshakes.clone().try_acquire_owned() else {
        return text_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "Too many connections are starting\n",
        );
    };
    let (response, pending) = match ws::prepare_upgrade(req, pre_auth_config()) {
        Ok(pair) => pair,
        Err((status, message)) => return text_response(status, message),
    };
    let tunnels = state.tunnels.clone();
    tunnels.spawn(async move {
        if let Some(browser) = pending.finish().await {
            tunnel(state, browser, permit).await;
        }
    });
    response
}

/// What sealed frame 0 tells the page (it replaces the former `GET /api/info`).
#[derive(Serialize)]
struct Info {
    version: &'static str,
    cwd: String,
    mock: bool,
    cli_server: bool,
    channel_control: bool,
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum ControlRequest {
    ConnectUpstream {
        mode: ConnectMode,
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        token: Option<String>,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ConnectMode {
    Auto,
    Explicit,
    CliDefault,
}

#[derive(Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum ControlResponse {
    UpstreamReady,
    UpstreamError { error: String },
}

enum ControlOutcome {
    Request(UpstreamRequest),
    BrowserClosed,
    BadFrame,
    Shutdown,
}

fn seal_control(sealer: &mut Sealer, response: &ControlResponse) -> UpMessage {
    let bytes = serde_json::to_vec(response).expect("plain data serialises");
    UpMessage::Binary(sealer.seal(Kind::Binary, &bytes).into())
}

fn parse_control_frame(opener: &mut Opener, wire: &[u8]) -> Result<UpstreamRequest, BadFrame> {
    let (kind, plain) = opener.open(wire)?;
    if kind != Kind::Binary {
        return Err(BadFrame);
    }
    let ControlRequest::ConnectUpstream { mode, url, token } =
        serde_json::from_slice(&plain).map_err(|_| BadFrame)?;
    Ok(match mode {
        ConnectMode::CliDefault => UpstreamRequest::CliDefault,
        ConnectMode::Auto => UpstreamRequest::Auto,
        ConnectMode::Explicit => UpstreamRequest::Explicit {
            url: url.unwrap_or_default(),
            token,
        },
    })
}

async fn await_upstream_request(
    state: &AppState,
    browser: &mut BrowserSocket,
    opener: &mut Opener,
) -> ControlOutcome {
    let heartbeat = state.heartbeat;
    let mut ticker = tokio::time::interval_at(
        Instant::now() + heartbeat.ping_interval,
        heartbeat.ping_interval,
    );
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut last_heard = Instant::now();
    loop {
        tokio::select! {
            _ = state.shutdown.cancelled() => return ControlOutcome::Shutdown,
            _ = ticker.tick() => {
                if last_heard.elapsed() >= heartbeat.pong_timeout {
                    tracing::debug!("browser went silent before connect_upstream, dropping it");
                    return ControlOutcome::BrowserClosed;
                }
                if !send_to_browser(browser, UpMessage::Ping(Bytes::new()), heartbeat).await {
                    return ControlOutcome::BrowserClosed;
                }
            }
            frame = browser.recv() => {
                let message = match frame {
                    Some(Ok(UpMessage::Close(_))) | None => return ControlOutcome::BrowserClosed,
                    Some(Err(e)) => {
                        tracing::debug!("browser read failed before connect_upstream: {e}");
                        return ControlOutcome::BrowserClosed;
                    }
                    Some(Ok(message)) => message,
                };
                last_heard = Instant::now();
                match message {
                    UpMessage::Ping(_) | UpMessage::Pong(_) | UpMessage::Frame(_) => continue,
                    UpMessage::Text(_) => return ControlOutcome::BadFrame,
                    UpMessage::Close(_) => return ControlOutcome::BrowserClosed,
                    UpMessage::Binary(wire) => match parse_control_frame(opener, &wire) {
                        Ok(req) => return ControlOutcome::Request(req),
                        Err(BadFrame) => return ControlOutcome::BadFrame,
                    },
                }
            }
        }
    }
}

async fn tunnel(
    state: Arc<AppState>,
    mut browser: BrowserSocket,
    handshake_slot: OwnedSemaphorePermit,
) {
    let Some((mut sealer, mut opener)) = authenticate(&state, &mut browser).await else {
        return;
    };
    drop(handshake_slot);
    browser.set_config(|config| *config = post_auth_config());

    let info = Info {
        version: env!("CARGO_PKG_VERSION"),
        cwd: state.cwd.clone(),
        mock: state.upstream.is_mock(),
        cli_server: state.upstream.is_cli_server(),
        channel_control: true,
    };
    let info = serde_json::to_vec(&info).expect("plain data serialises");
    let frame_zero = UpMessage::Binary(sealer.seal(Kind::Text, &info).into());
    if !send_to_browser(&mut browser, frame_zero, state.heartbeat).await {
        return;
    }

    let request = match await_upstream_request(&state, &mut browser, &mut opener).await {
        ControlOutcome::Request(req) => req,
        ControlOutcome::BrowserClosed => return,
        ControlOutcome::BadFrame => {
            close_browser(&mut browser, CLOSE_BAD_MESSAGE, REASON_BAD_FRAME).await;
            return;
        }
        ControlOutcome::Shutdown => {
            close_browser(&mut browser, CLOSE_GOING_AWAY, SHUTDOWN_REASON).await;
            return;
        }
    };

    let upstream = tokio::select! {
        r = state.upstream.connect_for(request) => r,
        _ = state.shutdown.cancelled() => {
            close_browser(&mut browser, CLOSE_GOING_AWAY, SHUTDOWN_REASON).await;
            return;
        }
    };
    let mut upstream = match upstream {
        Ok(ws) => ws,
        Err(e) => {
            tracing::warn!("backend connection failed: {e}");
            let err_frame = seal_control(
                &mut sealer,
                &ControlResponse::UpstreamError {
                    error: e.to_string(),
                },
            );
            let _ = send_to_browser(&mut browser, err_frame, state.heartbeat).await;
            close_browser(
                &mut browser,
                CLOSE_BACKEND_UNAVAILABLE,
                REASON_BACKEND_UNAVAILABLE,
            )
            .await;
            return;
        }
    };
    let ready = seal_control(&mut sealer, &ControlResponse::UpstreamReady);
    if !send_to_browser(&mut browser, ready, state.heartbeat).await {
        close_upstream(&mut upstream).await;
        return;
    }
    let ended = pump(
        &state,
        &mut browser,
        &mut upstream,
        &mut sealer,
        &mut opener,
    )
    .await;
    finish(&mut browser, &mut upstream, ended).await;
}
async fn finish(browser: &mut BrowserSocket, upstream: &mut Ws, ended: Ended) {
    match ended {
        Ended::BrowserClosed => {
            close_upstream(upstream).await;
        }
        Ended::BadFrame => {
            close_upstream(upstream).await;
            close_browser(browser, CLOSE_BAD_MESSAGE, REASON_BAD_FRAME).await;
        }
        Ended::UpstreamClosed(close) => {
            let (code, reason) = close.unwrap_or((
                CLOSE_BACKEND_UNAVAILABLE,
                "backend connection closed".to_string(),
            ));
            close_browser(browser, code, &reason).await;
        }
        Ended::Shutdown => {
            close_upstream(upstream).await;
            close_browser(browser, CLOSE_GOING_AWAY, SHUTDOWN_REASON).await;
        }
    }
}

/// Why a handshake did not succeed.
enum Reject {
    /// The browser is gone; nothing to answer.
    Gone,
    /// Not the messages of the handshake.
    Malformed,
    KeyRejected,
}

/// The key handshake. On success, returns the channel; otherwise the browser has been told
/// and the tunnel ends. The backend is not touched before this succeeds.
async fn authenticate(state: &AppState, browser: &mut BrowserSocket) -> Option<(Sealer, Opener)> {
    let outcome = tokio::select! {
        r = tokio::time::timeout(state.handshake_timeout, handshake(state, browser)) => r,
        _ = state.shutdown.cancelled() => {
            close_browser(browser, CLOSE_GOING_AWAY, SHUTDOWN_REASON).await;
            return None;
        }
    };
    match outcome {
        Ok(Ok(channel)) => return Some(channel),
        Ok(Err(Reject::Gone)) => {}
        Ok(Err(Reject::Malformed)) => {
            close_browser(browser, CLOSE_BAD_MESSAGE, REASON_BAD_HANDSHAKE).await;
        }
        Ok(Err(Reject::KeyRejected)) => {
            tokio::time::sleep(WRONG_KEY_PAUSE).await;
            close_browser(browser, CLOSE_KEY_REJECTED, REASON_KEY_REJECTED).await;
        }
        Err(_) => {
            close_browser(browser, CLOSE_HANDSHAKE_TIMEOUT, REASON_HANDSHAKE_TIMEOUT).await;
        }
    }
    None
}

async fn handshake(
    state: &AppState,
    browser: &mut BrowserSocket,
) -> Result<(Sealer, Opener), Reject> {
    let sn = token::random_bytes::<NONCE_LEN>();
    let hello = channel::hello_json(state.key.salt(), &sn);
    browser
        .send(UpMessage::text(hello))
        .await
        .map_err(|_| Reject::Gone)?;
    let auth = loop {
        match browser.recv().await {
            Some(Ok(UpMessage::Text(text))) if text.len() <= MAX_HANDSHAKE_MESSAGE => break text,
            Some(Ok(UpMessage::Ping(_) | UpMessage::Pong(_))) => {}
            Some(Ok(UpMessage::Close(_))) | None => return Err(Reject::Gone),
            Some(Err(WsError::Capacity(_))) | Some(Ok(_)) => return Err(Reject::Malformed),
            Some(Err(_)) => return Err(Reject::Gone),
        }
    };
    let (cn, mac) = channel::parse_auth(auth.as_str()).ok_or(Reject::Malformed)?;
    let root = state.key.root();
    if !root.verify_auth(&sn, &cn, &mac) {
        tracing::debug!("a browser presented the wrong key");
        return Err(Reject::KeyRejected);
    }
    Ok(root.server_channel(&sn, &cn))
}

enum Ended {
    BrowserClosed,
    /// The browser sent something that does not open.
    BadFrame,
    /// The close code and reason the backend sent, if any.
    UpstreamClosed(Option<(u16, String)>),
    Shutdown,
}

/// Copies frames both ways until one side ends. Pings from either side are answered by
/// the connection itself and not forwarded. The browser is pinged on its own schedule.
async fn pump(
    state: &AppState,
    browser: &mut BrowserSocket,
    upstream: &mut Ws,
    sealer: &mut Sealer,
    opener: &mut Opener,
) -> Ended {
    let heartbeat = state.heartbeat;
    let mut ticker = tokio::time::interval_at(
        Instant::now() + heartbeat.ping_interval,
        heartbeat.ping_interval,
    );
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut last_heard = Instant::now();
    loop {
        tokio::select! {
            _ = state.shutdown.cancelled() => return Ended::Shutdown,
            _ = ticker.tick() => {
                if last_heard.elapsed() >= heartbeat.pong_timeout {
                    tracing::debug!("browser went silent, dropping it");
                    return Ended::BrowserClosed;
                }
                if !send_to_browser(browser, UpMessage::Ping(Bytes::new()), heartbeat).await {
                    return Ended::BrowserClosed;
                }
            }
            frame = browser.recv() => {
                let message = match frame {
                    Some(Ok(UpMessage::Close(_))) | None => return Ended::BrowserClosed,
                    Some(Err(e)) => {
                        tracing::debug!("browser read failed: {e}");
                        return Ended::BrowserClosed;
                    }
                    Some(Ok(message)) => message,
                };
                // Any frame shows the tab is alive; a large upload can queue its pong.
                last_heard = Instant::now();
                if matches!(message, UpMessage::Pong(_)) {
                    continue;
                }
                let forward = match to_upstream(opener, message) {
                    Ok(Some(forward)) => forward,
                    Ok(None) => continue,
                    Err(BadFrame) => return Ended::BadFrame,
                };
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
                let Some(forward) = to_browser(sealer, message) else { continue };
                if !send_to_browser(browser, forward, heartbeat).await {
                    return Ended::BrowserClosed;
                }
            }
        }
    }
}

/// False when the tab is gone or does not take the frame within the pong timeout (its
/// receive buffer is full). Waiting longer would also stop the pings.
async fn send_to_browser(
    browser: &mut BrowserSocket,
    message: UpMessage,
    heartbeat: Heartbeat,
) -> bool {
    match tokio::time::timeout(heartbeat.pong_timeout, browser.send(message)).await {
        Ok(Ok(())) => true,
        Ok(Err(e)) => {
            tracing::debug!("browser is gone: {e}");
            false
        }
        Err(_) => {
            tracing::debug!("browser does not take data, dropping it");
            false
        }
    }
}

async fn close_upstream(upstream: &mut Ws) {
    if let Err(e) = upstream.close(None).await {
        tracing::debug!("cannot close the backend connection: {e}");
    }
}

/// Opens what the browser sent. After the handshake every data message is a sealed binary one.
fn to_upstream(opener: &mut Opener, message: UpMessage) -> Result<Option<UpMessage>, BadFrame> {
    match message {
        UpMessage::Binary(wire) => {
            let (kind, plain) = opener.open(&wire)?;
            Ok(Some(match kind {
                Kind::Text => UpMessage::text(String::from_utf8(plain).map_err(|_| BadFrame)?),
                Kind::Binary => UpMessage::Binary(plain.into()),
            }))
        }
        UpMessage::Text(_) => Err(BadFrame),
        UpMessage::Ping(_) | UpMessage::Pong(_) | UpMessage::Close(_) | UpMessage::Frame(_) => {
            Ok(None)
        }
    }
}

fn to_browser(sealer: &mut Sealer, message: UpMessage) -> Option<UpMessage> {
    let wire = match message {
        UpMessage::Text(text) => sealer.seal(Kind::Text, text.as_bytes()),
        UpMessage::Binary(bytes) => sealer.seal(Kind::Binary, &bytes),
        UpMessage::Ping(_) | UpMessage::Pong(_) | UpMessage::Close(_) | UpMessage::Frame(_) => {
            return None;
        }
    };
    Some(UpMessage::Binary(wire.into()))
}

fn close_code_and_reason(close: UpCloseFrame) -> (u16, String) {
    (u16::from(close.code), close.reason.to_string())
}

async fn close_browser(browser: &mut BrowserSocket, code: u16, reason: &str) {
    let reason: String = reason
        .chars()
        .scan(0, |len, c| {
            *len += c.len_utf8();
            (*len <= MAX_CLOSE_REASON).then_some(c)
        })
        .collect();
    let frame = UpCloseFrame {
        code: CloseCode::from(code),
        reason: reason.into(),
    };
    if let Err(e) = browser.send(UpMessage::Close(Some(frame))).await {
        tracing::debug!("cannot send the close frame to the browser: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::{SALT_LEN, auth_json, parse_hello};
    use crate::guard::HostPolicy;
    use crate::key::Key;
    use crate::upstream::Upstream;
    use axum::Router;
    use axum::routing::get;
    use ref_client::Target;
    use tokio::net::TcpListener;
    use tokio::sync::{Semaphore, oneshot};
    use tokio::time::timeout;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
    use tokio_util::sync::CancellationToken;
    use tokio_util::task::TaskTracker;

    const TEST_HEARTBEAT: Heartbeat = Heartbeat {
        ping_interval: Duration::from_millis(50),
        pong_timeout: Duration::from_millis(500),
    };
    const LOOPBACK: &str = "127.0.0.1";
    const WAIT: Duration = Duration::from_secs(10);
    /// Several pong timeouts: a healthy tab must stay connected for this long.
    const HEALTHY_FOR: Duration = Duration::from_millis(1500);

    /// Enough data to fill the socket buffers of a tab that does not read.
    const FLOOD_MESSAGES: usize = 256;
    const FLOOD_MESSAGE_BYTES: usize = 1 << 20;
    /// How often a busy tab sends something.
    const BUSY_TAB_SENDS_EVERY: Duration = Duration::from_millis(50);

    /// A backend that reports (once) when its connection ends.
    async fn upstream_that_reports_close() -> (String, oneshot::Receiver<()>) {
        let listener = TcpListener::bind(format!("{LOOPBACK}:0")).await.unwrap();
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
            closed_tx.send(()).unwrap();
        });
        (url, closed_rx)
    }

    /// Like `upstream_that_reports_close`, but the backend also sends `FLOOD_MESSAGES` large
    /// binary frames as soon as the tab connects.
    async fn flooding_upstream_that_reports_close() -> (String, oneshot::Receiver<()>) {
        let listener = TcpListener::bind(format!("{LOOPBACK}:0")).await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let (closed_tx, closed_rx) = oneshot::channel();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            let (mut sink, mut source) = ws.split();
            tokio::spawn(async move {
                for _ in 0..FLOOD_MESSAGES {
                    let chunk = vec![0u8; FLOOD_MESSAGE_BYTES];
                    if sink.send(UpMessage::Binary(chunk.into())).await.is_err() {
                        return;
                    }
                }
            });
            while let Some(Ok(message)) = source.next().await {
                if message.is_close() {
                    break;
                }
            }
            closed_tx.send(()).unwrap();
        });
        (url, closed_rx)
    }

    const TEST_KEY: &str = "a key for the tunnel tests";

    /// Serves the tunnel route alone; returns the page address.
    async fn serve_tunnel(upstream_url: String) -> String {
        let state = Arc::new(AppState {
            key: Key::new(TEST_KEY.to_string(), [3; SALT_LEN]),
            handshakes: Arc::new(Semaphore::new(MAX_HANDSHAKES)),
            handshake_timeout: HANDSHAKE_TIMEOUT,
            hosts: HostPolicy::new(LOOPBACK, 0, false),
            cwd: String::new(),
            upstream: Upstream::Remote(Target::Explicit {
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
        let listener = TcpListener::bind(format!("{LOOPBACK}:0")).await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        address.to_string()
    }

    type Tab = tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >;

    /// A tab that has signed in and read `frame_zero`, before sending `connect_upstream`.
    async fn sign_in_control(address: &str) -> (Tab, Sealer, Opener, serde_json::Value) {
        let mut request = format!("ws://{address}/ws").into_client_request().unwrap();
        request
            .headers_mut()
            .insert(ORIGIN, format!("http://{address}").parse().unwrap());
        let (mut tab, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        let hello = tab.next().await.unwrap().unwrap().into_text().unwrap();
        let (salt, sn) = parse_hello(hello.as_str()).unwrap();
        let root = crate::channel::Root::derive(TEST_KEY.as_bytes(), &salt);
        let cn = [9; NONCE_LEN];
        tab.send(UpMessage::text(auth_json(&cn, &root.auth_mac(&sn, &cn))))
            .await
            .unwrap();
        let (sealer, mut opener) = root.client_channel(&sn, &cn);
        let frame_zero = loop {
            match tab.next().await.unwrap().unwrap() {
                UpMessage::Binary(wire) => {
                    let (kind, plain) = opener.open(&wire).unwrap();
                    assert_eq!(kind, Kind::Text);
                    break serde_json::from_slice(&plain).unwrap();
                }
                UpMessage::Ping(_) | UpMessage::Pong(_) => continue,
                other => panic!("expected frame_zero, got {other:?}"),
            }
        };
        (tab, sealer, opener, frame_zero)
    }

    /// A tab that has signed in and completed the `connect_upstream` handshake;
    /// `Sealer` seals what it sends.
    async fn open_tab(address: &str) -> (Tab, Sealer) {
        let (mut tab, mut sealer, mut opener, _) = sign_in_control(address).await;
        let wire = sealer.seal(
            Kind::Binary,
            br#"{"op":"connect_upstream","mode":"cli_default"}"#,
        );
        tab.send(UpMessage::Binary(wire.into())).await.unwrap();
        loop {
            match tab.next().await.unwrap().unwrap() {
                UpMessage::Binary(wire) => {
                    let (kind, plain) = opener.open(&wire).unwrap();
                    assert_eq!(kind, Kind::Binary);
                    let reply: serde_json::Value = serde_json::from_slice(&plain).unwrap();
                    assert_eq!(reply["op"], "upstream_ready");
                    break;
                }
                UpMessage::Ping(_) | UpMessage::Pong(_) => continue,
                other => panic!("expected upstream_ready, got {other:?}"),
            }
        }
        (tab, sealer)
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
        let (mut tab, _) = open_tab(&address).await;
        // Reading makes the library answer each ping with a pong.
        let reader = tokio::spawn(async move { while tab.next().await.is_some() {} });
        assert!(timeout(HEALTHY_FOR, &mut closed).await.is_err());
        reader.abort();
    }

    #[tokio::test]
    async fn a_tab_that_stops_reading_is_dropped_even_when_data_is_waiting_for_it() {
        let (url, closed) = flooding_upstream_that_reports_close().await;
        let address = serve_tunnel(url).await;
        // Never read: the tunnel ends up blocked writing to this tab.
        let _stuck_tab = open_tab(&address).await;
        timeout(WAIT, closed)
            .await
            .expect("the tunnel stayed stuck on the tab")
            .unwrap();
    }

    #[tokio::test]
    async fn a_tab_that_keeps_sending_is_alive_without_answering_pings() {
        let (url, mut closed) = upstream_that_reports_close().await;
        let address = serve_tunnel(url).await;
        let (mut tab, mut sealer) = open_tab(&address).await;
        // Sending only: the library does not get to answer the pings.
        let deadline = Instant::now() + HEALTHY_FOR;
        while Instant::now() < deadline {
            let frame = sealer.seal(Kind::Text, b"busy");
            tab.send(UpMessage::Binary(frame.into())).await.unwrap();
            tokio::time::sleep(BUSY_TAB_SENDS_EVERY).await;
        }
        assert!(closed.try_recv().is_err(), "the tab was dropped while busy");
    }

    #[tokio::test]
    async fn control_handshake_waits_before_connecting_upstream() {
        let listener = TcpListener::bind(format!("{LOOPBACK}:0")).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (accepted_tx, mut accepted_rx) = oneshot::channel();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let _ = accepted_tx.send(());
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            while ws.next().await.is_some() {}
        });

        let address = serve_tunnel(format!("ws://{addr}/")).await;
        let (mut tab, mut sealer, mut opener, frame_zero) = sign_in_control(&address).await;
        assert_eq!(frame_zero["cli_server"], true);
        assert_eq!(frame_zero["channel_control"], true);
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(
            accepted_rx.try_recv().is_err(),
            "upstream must not be connected before connect_upstream"
        );

        let wire = sealer.seal(
            Kind::Binary,
            br#"{"op":"connect_upstream","mode":"cli_default"}"#,
        );
        tab.send(UpMessage::Binary(wire.into())).await.unwrap();
        timeout(WAIT, accepted_rx)
            .await
            .expect("upstream was not connected after connect_upstream")
            .unwrap();
        loop {
            match tab.next().await.unwrap().unwrap() {
                UpMessage::Binary(wire) => {
                    let (kind, plain) = opener.open(&wire).unwrap();
                    assert_eq!(kind, Kind::Binary);
                    let reply: serde_json::Value = serde_json::from_slice(&plain).unwrap();
                    assert_eq!(reply["op"], "upstream_ready");
                    break;
                }
                UpMessage::Ping(_) | UpMessage::Pong(_) => continue,
                other => panic!("expected upstream_ready, got {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn control_handshake_invalid_explicit_url_sends_error_and_closes() {
        let address = serve_tunnel(format!("ws://{LOOPBACK}:1/")).await;
        let (mut tab, mut sealer, mut opener, _) = sign_in_control(&address).await;
        let wire = sealer.seal(
            Kind::Binary,
            br#"{"op":"connect_upstream","mode":"explicit","url":"http://127.0.0.1:1234"}"#,
        );
        tab.send(UpMessage::Binary(wire.into())).await.unwrap();
        let err_msg = loop {
            match tab.next().await.unwrap().unwrap() {
                UpMessage::Binary(wire) => {
                    let (kind, plain) = opener.open(&wire).unwrap();
                    assert_eq!(kind, Kind::Binary);
                    let reply: serde_json::Value = serde_json::from_slice(&plain).unwrap();
                    assert_eq!(reply["op"], "upstream_error");
                    break reply["error"].as_str().unwrap().to_string();
                }
                UpMessage::Ping(_) | UpMessage::Pong(_) => continue,
                other => panic!("expected upstream_error, got {other:?}"),
            }
        };
        assert!(err_msg.contains("ws:// or wss://"), "{err_msg}");
        let close = loop {
            match tab.next().await.unwrap().unwrap() {
                UpMessage::Close(Some(frame)) => break frame,
                UpMessage::Ping(_) | UpMessage::Pong(_) => continue,
                other => panic!("expected close after upstream_error, got {other:?}"),
            }
        };
        assert_eq!(u16::from(close.code), CLOSE_BACKEND_UNAVAILABLE);
        assert_eq!(close.reason.as_str(), REASON_BACKEND_UNAVAILABLE);
    }

    #[tokio::test]
    async fn control_handshake_rejects_text_frame_before_connect_upstream() {
        let address = serve_tunnel(format!("ws://{LOOPBACK}:1/")).await;
        let (mut tab, mut sealer, _, _) = sign_in_control(&address).await;
        let wire = sealer.seal(Kind::Text, br#"{"id":1,"op":"hello"}"#);
        tab.send(UpMessage::Binary(wire.into())).await.unwrap();
        let close = loop {
            match tab.next().await.unwrap().unwrap() {
                UpMessage::Close(Some(frame)) => break frame,
                UpMessage::Ping(_) | UpMessage::Pong(_) => continue,
                other => panic!("expected close 4400, got {other:?}"),
            }
        };
        assert_eq!(u16::from(close.code), CLOSE_BAD_MESSAGE);
        assert_eq!(close.reason.as_str(), REASON_BAD_FRAME);
    }
}
