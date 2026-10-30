//! One browser WebSocket ⇄ one upstream connection. Frames are copied, never parsed.

use std::sync::Arc;

use axum::extract::State;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::http::header::{HOST, ORIGIN};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use futures_util::{SinkExt as _, StreamExt as _};
use ref_client::Ws;
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
        .on_upgrade(move |browser| tunnel(state, browser))
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

/// Copies frames both ways until one side ends. Ping and pong are answered by each
/// connection itself and not forwarded.
async fn pump(state: &AppState, browser: &mut WebSocket, upstream: &mut Ws) -> Ended {
    loop {
        tokio::select! {
            _ = state.shutdown.cancelled() => return Ended::Shutdown,
            frame = browser.recv() => {
                let message = match frame {
                    Some(Ok(Message::Close(_))) | None => return Ended::BrowserClosed,
                    Some(Err(e)) => {
                        tracing::debug!("browser read failed: {e}");
                        return Ended::BrowserClosed;
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
