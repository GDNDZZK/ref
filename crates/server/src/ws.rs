//! WebSocket accept (IP allow-list + token check before upgrade), per-connection reader/writer.

use crate::hub::{Conn, Hub};
use crate::util::lock;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use ref_proto::{ClientFrame, InstanceInfo, Request, ServerFrame};
use serde::Serialize;
use serde_json::{Value, json};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message as WsMsg;
use tokio_tungstenite::tungstenite::handshake::server::{
    ErrorResponse, Request as HsRequest, Response as HsResponse,
};
use tokio_tungstenite::tungstenite::http::StatusCode;

/// Client name used by `ref-server --status/--stop`; such connections do not count as frontends.
pub(crate) const CONTROL_CLIENT: &str = "ref-server-cli";

/// Largest frame/message accepted from a client.
const MAX_CLIENT_MESSAGE: usize = 64 << 20;

/// Pause after a failed `accept` before trying again.
const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(100);

/// Time allowed to write the 403 answer to a rejected host.
const REJECT_WRITE_TIMEOUT: Duration = Duration::from_secs(2);

/// Time a client has to finish the WebSocket handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Wait before sending a queue that holds only deltas, so that they go out in one batch.
const DELTA_COALESCE_WAIT: Duration = Duration::from_millis(30);

/// Pause that lets an error response reach the client before the connection is closed.
const CLOSE_GRACE: Duration = Duration::from_millis(50);

/// Time the writer task gets to finish after the connection ended.
const WRITER_JOIN_TIMEOUT: Duration = Duration::from_secs(2);

/// `--allow-host` entry that admits every host.
const WILDCARD_HOST: &str = "*";

/// Query parameter that carries the token in the handshake URL.
const TOKEN_PARAM: &str = "token=";

#[derive(Debug, Clone, Default)]
pub struct Auth {
    pub any: bool,
    pub allowed: Vec<IpAddr>,
    /// Empty = no token required.
    pub token: String,
}

impl Auth {
    fn ip_allowed(&self, ip: IpAddr) -> bool {
        let ip = ip.to_canonical();
        self.any || self.allowed.iter().any(|a| a.to_canonical() == ip)
    }
}

/// A host name or IP without whitespace and the brackets of an IPv6 literal.
pub fn bare_host(h: &str) -> &str {
    h.trim().trim_start_matches('[').trim_end_matches(']')
}

/// Resolve `--allow-host` entries (IPs, hostnames, `*`) into an [`Auth`].
pub fn resolve_hosts(hosts: &[String], token: String) -> (Auth, Vec<String>) {
    use std::net::ToSocketAddrs;
    let mut auth = Auth {
        token,
        ..Default::default()
    };
    let mut warnings = Vec::new();
    for h in hosts {
        let h = bare_host(h);
        if h.is_empty() {
            continue;
        }
        if h == WILDCARD_HOST {
            auth.any = true;
            continue;
        }
        if let Ok(ip) = h.parse::<IpAddr>() {
            auth.allowed.push(ip);
            continue;
        }
        match (h, 0u16).to_socket_addrs() {
            Ok(addrs) => {
                let mut n = 0;
                for a in addrs {
                    auth.allowed.push(a.ip());
                    n += 1;
                }
                if n == 0 {
                    warnings.push(format!("allow-host {h}: resolved to no addresses"));
                }
            }
            Err(e) => warnings.push(format!("allow-host {h}: cannot resolve: {e}")),
        }
    }
    if hosts.iter().any(|h| h.trim() == ref_proto::LOCALHOST) {
        auth.allowed.push(Ipv4Addr::LOCALHOST.into());
        auth.allowed.push(Ipv6Addr::LOCALHOST.into());
    }
    (auth, warnings)
}

fn query_token(uri: &str) -> Option<String> {
    let q = uri.split_once('?')?.1;
    for kv in q.split('&') {
        if let Some(v) = kv.strip_prefix(TOKEN_PARAM) {
            return Some(v.to_string());
        }
    }
    None
}

fn forbidden(text: &str) -> ErrorResponse {
    let mut r = ErrorResponse::new(Some(text.to_string()));
    *r.status_mut() = StatusCode::FORBIDDEN;
    r
}

/// Accept loop; returns when the hub shuts down.
pub(crate) async fn serve(hub: Arc<Hub>, listener: TcpListener, auth: Arc<Auth>) {
    loop {
        let accepted = tokio::select! {
            r = listener.accept() => r,
            _ = hub.shutdown.cancelled() => return,
        };
        match accepted {
            Ok((stream, peer)) => {
                let (hub, auth) = (hub.clone(), auth.clone());
                tokio::spawn(async move { handle_conn(hub, stream, peer, auth).await });
            }
            Err(e) => {
                tracing::warn!("accept failed: {e}");
                tokio::time::sleep(ACCEPT_RETRY_DELAY).await;
            }
        }
    }
}

async fn reject_raw(mut stream: TcpStream) {
    use tokio::io::AsyncWriteExt;
    let body = "Forbidden: host not allowed\n";
    let resp = format!(
        "HTTP/1.1 {}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        StatusCode::FORBIDDEN,
        body.len()
    );
    // Best effort: the peer is being turned away, a failed write or shutdown changes nothing.
    let _ = tokio::time::timeout(REJECT_WRITE_TIMEOUT, stream.write_all(resp.as_bytes())).await;
    let _ = stream.shutdown().await;
}

async fn handle_conn(hub: Arc<Hub>, stream: TcpStream, peer: SocketAddr, auth: Arc<Auth>) {
    if !auth.ip_allowed(peer.ip()) {
        tracing::warn!(%peer, "rejected connection: host not allowed");
        reject_raw(stream).await;
        return;
    }
    // Only a latency tweak; the connection works without it.
    let _ = stream.set_nodelay(true);
    let Some((ws, query_ok)) = accept_ws(stream, peer, &auth).await else {
        return;
    };
    let authed = query_ok || auth.token.is_empty();
    let conn = hub.add_conn();
    conn.lock().authed = authed;
    tracing::info!(%peer, conn = conn.id, "connection opened");
    let (sink, source) = ws.split();
    let writer = spawn_writer(conn.clone(), sink);
    read_loop(&hub, &conn, source, &auth).await;
    tracing::info!(conn = conn.id, "connection closed");
    finish(&hub, &conn);
    // Best effort: a writer that does not finish in time is abandoned with the connection.
    let _ = tokio::time::timeout(WRITER_JOIN_TIMEOUT, writer).await;
}

/// WebSocket handshake with the token check of the URL. Returns the stream and whether the
/// URL carried a valid token.
async fn accept_ws(
    stream: TcpStream,
    peer: SocketAddr,
    auth: &Auth,
) -> Option<(WebSocketStream<TcpStream>, bool)> {
    let mut query_ok = false;
    // The handshake callback signature (and its large `ErrorResponse`) is fixed by tungstenite.
    #[allow(clippy::result_large_err)]
    let cb = |req: &HsRequest, resp: HsResponse| -> Result<HsResponse, ErrorResponse> {
        match query_token(&req.uri().to_string()) {
            Some(t) if !auth.token.is_empty() && t != auth.token => Err(forbidden("invalid token")),
            Some(_) => {
                query_ok = true;
                Ok(resp)
            }
            None => Ok(resp),
        }
    };
    // Prompts carry pasted images: allow one frame up to the whole message limit (the 16 MiB
    // default frame limit is lower than the 64 MiB message limit).
    let cfg = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(MAX_CLIENT_MESSAGE))
        .max_frame_size(Some(MAX_CLIENT_MESSAGE));
    let accepted = tokio::time::timeout(
        HANDSHAKE_TIMEOUT,
        tokio_tungstenite::accept_hdr_async_with_config(stream, cb, Some(cfg)),
    )
    .await;
    match accepted {
        Ok(Ok(ws)) => Some((ws, query_ok)),
        Ok(Err(e)) => {
            tracing::debug!(%peer, "websocket handshake failed: {e}");
            None
        }
        Err(_) => {
            tracing::debug!(%peer, "websocket handshake timed out");
            None
        }
    }
}

/// Sends the queued frames of the connection until it is killed or the client is too slow.
fn spawn_writer(
    conn: Arc<Conn>,
    mut sink: SplitSink<WebSocketStream<TcpStream>, WsMsg>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = conn.out.notify.notified() => {}
                _ = conn.kill.cancelled() => {}
            }
            if conn.out.only_deltas() && !conn.kill.is_cancelled() {
                tokio::time::sleep(DELTA_COALESCE_WAIT).await;
            }
            if conn.out.overflow.load(std::sync::atomic::Ordering::SeqCst) {
                tracing::warn!(conn = conn.id, "client too slow; dropping connection");
                // The connection is dropped anyway; a failed close handshake changes nothing.
                let _ = sink.close().await;
                conn.kill.cancel();
                return;
            }
            for f in conn.out.drain() {
                let txt = serde_json::to_string(&f).expect("ServerFrame serializes");
                if sink.feed(WsMsg::text(txt)).await.is_err() {
                    conn.kill.cancel();
                    return;
                }
            }
            if sink.flush().await.is_err() {
                conn.kill.cancel();
                return;
            }
            if conn.kill.is_cancelled() {
                // The connection is going away; a failed close handshake changes nothing.
                let _ = sink.close().await;
                return;
            }
        }
    })
}

/// Reads client frames until the connection ends: the hello first, then requests.
async fn read_loop(
    hub: &Arc<Hub>,
    conn: &Arc<Conn>,
    mut source: SplitStream<WebSocketStream<TcpStream>>,
    auth: &Auth,
) {
    loop {
        let msg = tokio::select! {
            m = source.next() => m,
            _ = conn.kill.cancelled() => break,
            _ = hub.shutdown.cancelled() => break,
        };
        let text = match msg {
            Some(Ok(WsMsg::Text(t))) => t.to_string(),
            Some(Ok(WsMsg::Binary(b))) => String::from_utf8_lossy(&b).to_string(),
            Some(Ok(WsMsg::Close(_))) | None => break,
            Some(Err(e)) => {
                tracing::debug!(conn = conn.id, "websocket read failed: {e}");
                break;
            }
            Some(Ok(_)) => continue,
        };
        let frame: ClientFrame = match serde_json::from_str(&text) {
            Ok(f) => f,
            Err(e) => {
                let id = serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|v| v.get("id").and_then(Value::as_u64));
                if let Some(id) = id {
                    conn.send(err_frame(id, format!("Invalid request: {e}")));
                } else {
                    conn.send(ServerFrame::Toast {
                        level: ref_proto::ToastLevel::Error,
                        text: format!("Invalid frame: {e}"),
                    });
                }
                if !conn.lock().hello {
                    break;
                }
                continue;
            }
        };
        if !conn.lock().hello {
            if !handle_hello(hub, conn, auth, frame).await {
                break;
            }
            continue;
        }
        let res = crate::api::handle(hub, conn, frame.req).await;
        if let Some(id) = frame.id {
            conn.send(match res {
                Ok(result) => ServerFrame::Response {
                    id,
                    ok: true,
                    result,
                    error: None,
                },
                Err(e) => err_frame(id, e),
            });
        } else if let Err(e) = res {
            conn.send(ServerFrame::Toast {
                level: ref_proto::ToastLevel::Error,
                text: e,
            });
        }
    }
}

/// Handles the first frame, which must be an authorized hello. Returns false to close the
/// connection.
async fn handle_hello(hub: &Arc<Hub>, conn: &Arc<Conn>, auth: &Auth, frame: ClientFrame) -> bool {
    let Request::Hello {
        client,
        version,
        cwd,
        token,
        notify,
    } = frame.req
    else {
        reject_frame(conn, frame.id, "first frame must be hello").await;
        return false;
    };
    let cwd = hub.effective_cwd(&cwd);
    let ok = conn.lock().authed
        || (!auth.token.is_empty() && token.as_deref() == Some(auth.token.as_str()));
    if !ok {
        reject_frame(conn, frame.id, "invalid or missing token").await;
        return false;
    }
    {
        let mut st = conn.lock();
        st.authed = true;
        st.hello = true;
        st.frontend = client != CONTROL_CLIENT;
        st.cwd = cwd.clone();
        st.notify = notify.into_iter().collect();
    }
    if client != CONTROL_CLIENT {
        hub.frontend_joined();
    }
    tracing::info!(conn = conn.id, %client, version, "hello");
    if let Some(id) = frame.id {
        conn.send(ServerFrame::Response {
            id,
            ok: true,
            result: hello_result(hub, &cwd),
            error: None,
        });
    }
    conn.send(ServerFrame::Sessions {
        sessions: hub.infos(),
    });
    true
}

/// Answers a refused first frame and gives the answer time to arrive before the close.
async fn reject_frame(conn: &Conn, id: Option<u64>, reason: &str) {
    if let Some(id) = id {
        conn.send(err_frame(id, reason.into()));
    }
    tokio::time::sleep(CLOSE_GRACE).await;
}

/// The instance as clients see it: everything but the token.
#[derive(Serialize)]
struct PublicInstance {
    pid: u32,
    port: u16,
    bind: String,
    allow_hosts: Vec<String>,
    idle_timeout: i64,
    started_at: i64,
    version: String,
}

impl From<&InstanceInfo> for PublicInstance {
    fn from(i: &InstanceInfo) -> Self {
        PublicInstance {
            pid: i.pid,
            port: i.port,
            bind: i.bind.clone(),
            allow_hosts: i.allow_hosts.clone(),
            idle_timeout: i.idle_timeout,
            started_at: i.started_at,
            version: i.version.clone(),
        }
    }
}

/// Result of the hello request.
fn hello_result(hub: &Hub, cwd: &str) -> Value {
    let instance = lock(&hub.instance).as_ref().map(PublicInstance::from);
    json!({
        "server_version": env!("CARGO_PKG_VERSION"),
        "protocol": ref_proto::PROTOCOL_VERSION,
        "instance": instance,
        "cwd": cwd,
    })
}

fn finish(hub: &Arc<Hub>, conn: &Arc<Conn>) {
    conn.out.notify.notify_one();
    hub.remove_conn(conn.id);
}

fn err_frame(id: u64, e: String) -> ServerFrame {
    ServerFrame::Response {
        id,
        ok: false,
        result: Value::Null,
        error: Some(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts() {
        let (a, _) = resolve_hosts(
            &["127.0.0.1".into(), "::1".into(), "localhost".into()],
            "t".into(),
        );
        assert!(a.ip_allowed("127.0.0.1".parse().unwrap()));
        assert!(a.ip_allowed("::ffff:127.0.0.1".parse().unwrap()));
        assert!(!a.ip_allowed("10.0.0.1".parse().unwrap()));
        let (b, _) = resolve_hosts(&["*".into()], String::new());
        assert!(b.ip_allowed("10.0.0.1".parse().unwrap()));
        assert_eq!(query_token("/?a=1&token=abc").as_deref(), Some("abc"));
        assert_eq!(query_token("/"), None);
    }
}
