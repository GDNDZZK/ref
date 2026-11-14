//! WebSocket upgrade and `tungstenite::WebSocket` async adapter.
//!
//! `axum::extract::ws::WebSocket` fixes `max_message_size` at upgrade time and offers no way to
//! change it later. Wrapping `tungstenite::WebSocket` directly lets `tunnel` keep a 1 KiB limit
//! until the key handshake finishes and then widen it to 64 MiB.

use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderMap, HeaderName, Method, StatusCode, header};
use axum::response::Response;
use futures_util::task::{ArcWake, AtomicWaker, noop_waker_ref, waker_ref};
use hyper::upgrade::OnUpgrade;
use hyper_util::rt::TokioIo;
use std::future::poll_fn;
use std::io::{Read, Write};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_tungstenite::tungstenite::Error as WsError;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;
use tokio_tungstenite::tungstenite::protocol::{Role, WebSocket, WebSocketConfig};

const SEC_WEBSOCKET_KEY: &str = "sec-websocket-key";
const SEC_WEBSOCKET_VERSION: &str = "sec-websocket-version";
const REQUIRED_VERSION: &str = "13";

pub struct PendingUpgrade {
    on_upgrade: OnUpgrade,
    config: WebSocketConfig,
}

impl PendingUpgrade {
    pub async fn finish(self) -> Option<BrowserSocket> {
        let upgraded = self.on_upgrade.await.ok()?;
        Some(BrowserSocket::new(TokioIo::new(upgraded), self.config))
    }
}

/// Validates an HTTP/1.1 WebSocket upgrade and builds the `101 Switching Protocols` response.
pub fn prepare_upgrade(
    mut req: Request,
    config: WebSocketConfig,
) -> Result<(Response, PendingUpgrade), (StatusCode, &'static str)> {
    if *req.method() != Method::GET {
        return Err((StatusCode::METHOD_NOT_ALLOWED, "Method not allowed\n"));
    }
    let headers = req.headers();
    if !header_contains(headers, header::CONNECTION, "upgrade") {
        return Err((StatusCode::BAD_REQUEST, "Invalid Connection header\n"));
    }
    if !header_eq(headers, header::UPGRADE, "websocket") {
        return Err((StatusCode::BAD_REQUEST, "Invalid Upgrade header\n"));
    }
    if !header_eq_str(headers, SEC_WEBSOCKET_VERSION, REQUIRED_VERSION) {
        return Err((StatusCode::BAD_REQUEST, "Unsupported WebSocket version\n"));
    }
    let Some(key) = headers.get(SEC_WEBSOCKET_KEY).cloned() else {
        return Err((StatusCode::BAD_REQUEST, "Missing Sec-WebSocket-Key\n"));
    };
    let Some(on_upgrade) = req.extensions_mut().remove::<OnUpgrade>() else {
        return Err((StatusCode::UPGRADE_REQUIRED, "Connection not upgradable\n"));
    };
    let accept = derive_accept_key(key.as_bytes());
    let response = Response::builder()
        .status(StatusCode::SWITCHING_PROTOCOLS)
        .header(header::CONNECTION, "upgrade")
        .header(header::UPGRADE, "websocket")
        .header("sec-websocket-accept", accept)
        .body(Body::empty())
        .expect("101 response builds");
    Ok((response, PendingUpgrade { on_upgrade, config }))
}

fn header_eq(headers: &HeaderMap, name: HeaderName, expected: &str) -> bool {
    headers
        .get(&name)
        .is_some_and(|v| v.as_bytes().eq_ignore_ascii_case(expected.as_bytes()))
}

fn header_eq_str(headers: &HeaderMap, name: &str, expected: &str) -> bool {
    headers
        .get(name)
        .is_some_and(|v| v.as_bytes().eq_ignore_ascii_case(expected.as_bytes()))
}

fn header_contains(headers: &HeaderMap, name: HeaderName, token: &str) -> bool {
    headers.get_all(&name).iter().any(|value| {
        value.to_str().ok().is_some_and(|text| {
            text.split(',')
                .any(|p| p.trim().eq_ignore_ascii_case(token))
        })
    })
}

pub struct BrowserSocket {
    inner: WebSocket<AllowStd<TokioIo<hyper::upgrade::Upgraded>>>,
}

impl BrowserSocket {
    fn new(stream: TokioIo<hyper::upgrade::Upgraded>, config: WebSocketConfig) -> Self {
        let allow_std = AllowStd::new(stream, noop_waker_ref());
        BrowserSocket {
            inner: WebSocket::from_raw_socket(allow_std, Role::Server, Some(config)),
        }
    }

    pub fn set_config(&mut self, f: impl FnOnce(&mut WebSocketConfig)) {
        self.inner.set_config(f);
    }

    pub async fn recv(&mut self) -> Option<Result<Message, WsError>> {
        poll_fn(|cx| {
            self.inner
                .get_mut()
                .set_waker(ContextWaker::Read, cx.waker());
            match self.inner.read() {
                Ok(msg) => Poll::Ready(Some(Ok(msg))),
                Err(WsError::Io(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    Poll::Pending
                }
                Err(WsError::ConnectionClosed | WsError::AlreadyClosed) => Poll::Ready(None),
                Err(e) => Poll::Ready(Some(Err(e))),
            }
        })
        .await
    }

    pub async fn send(&mut self, msg: Message) -> Result<(), WsError> {
        let mut pending = Some(msg);
        poll_fn(|cx| {
            self.inner
                .get_mut()
                .set_waker(ContextWaker::Write, cx.waker());
            if let Some(msg) = pending.take() {
                match self.inner.write(msg) {
                    Ok(()) => {}
                    Err(WsError::Io(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => return Poll::Ready(Err(e)),
                }
            }
            match self.inner.flush() {
                Ok(()) | Err(WsError::ConnectionClosed) => Poll::Ready(Ok(())),
                Err(WsError::Io(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    Poll::Pending
                }
                Err(e) => Poll::Ready(Err(e)),
            }
        })
        .await
    }
}

#[derive(Clone, Copy)]
enum ContextWaker {
    Read,
    Write,
}

#[derive(Default)]
struct WakerProxy {
    read_waker: AtomicWaker,
    write_waker: AtomicWaker,
}

impl ArcWake for WakerProxy {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        arc_self.read_waker.wake();
        arc_self.write_waker.wake();
    }
}

struct AllowStd<S> {
    inner: S,
    read_waker_proxy: Arc<WakerProxy>,
    write_waker_proxy: Arc<WakerProxy>,
}

impl<S> AllowStd<S> {
    fn new(inner: S, waker: &std::task::Waker) -> Self {
        let read_waker_proxy = Arc::new(WakerProxy::default());
        let write_waker_proxy = Arc::new(WakerProxy::default());
        read_waker_proxy.read_waker.register(waker);
        write_waker_proxy.read_waker.register(waker);
        AllowStd {
            inner,
            read_waker_proxy,
            write_waker_proxy,
        }
    }

    fn set_waker(&self, kind: ContextWaker, waker: &std::task::Waker) {
        match kind {
            ContextWaker::Read => {
                self.read_waker_proxy.read_waker.register(waker);
                self.write_waker_proxy.read_waker.register(waker);
            }
            ContextWaker::Write => {
                self.read_waker_proxy.write_waker.register(waker);
                self.write_waker_proxy.write_waker.register(waker);
            }
        }
    }
}

impl<S: Unpin> AllowStd<S> {
    fn with_context<F, R>(&mut self, kind: ContextWaker, f: F) -> Poll<std::io::Result<R>>
    where
        F: FnOnce(&mut Context<'_>, Pin<&mut S>) -> Poll<std::io::Result<R>>,
    {
        let waker = match kind {
            ContextWaker::Read => waker_ref(&self.read_waker_proxy),
            ContextWaker::Write => waker_ref(&self.write_waker_proxy),
        };
        let mut cx = Context::from_waker(&waker);
        f(&mut cx, Pin::new(&mut self.inner))
    }
}

impl<S: AsyncRead + Unpin> Read for AllowStd<S> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut read_buf = ReadBuf::new(buf);
        match self.with_context(ContextWaker::Read, |cx, s| s.poll_read(cx, &mut read_buf)) {
            Poll::Ready(Ok(())) => Ok(read_buf.filled().len()),
            Poll::Ready(Err(e)) => Err(e),
            Poll::Pending => Err(std::io::Error::from(std::io::ErrorKind::WouldBlock)),
        }
    }
}

impl<S: AsyncWrite + Unpin> Write for AllowStd<S> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self.with_context(ContextWaker::Write, |cx, s| s.poll_write(cx, buf)) {
            Poll::Ready(r) => r,
            Poll::Pending => Err(std::io::Error::from(std::io::ErrorKind::WouldBlock)),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self.with_context(ContextWaker::Write, |cx, s| s.poll_flush(cx)) {
            Poll::Ready(r) => r,
            Poll::Pending => Err(std::io::Error::from(std::io::ErrorKind::WouldBlock)),
        }
    }
}
