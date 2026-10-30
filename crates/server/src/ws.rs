//! WebSocket accept (IP allow-list + token check before upgrade), per-connection reader/writer.

use crate::hub::{Conn, Hub};
use futures_util::{SinkExt, StreamExt};
use ref_proto::{ClientFrame, Request, ServerFrame};
use serde_json::{Value, json};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message as WsMsg;
use tokio_tungstenite::tungstenite::handshake::server::{
    ErrorResponse, Request as HsRequest, Response as HsResponse,
};
use tokio_tungstenite::tungstenite::http::StatusCode;

/// Client name used by `ref-server --status/--stop`; such connections do not count as frontends.
pub const CONTROL_CLIENT: &str = "ref-server-cli";

/// Largest frame/message accepted from a client.
const MAX_CLIENT_MESSAGE: usize = 64 << 20;

#[derive(Debug, Clone, Default)]
pub struct Auth {
    pub any: bool,
    pub allowed: Vec<IpAddr>,
    /// Empty = no token required.
    pub token: String,
}

impl Auth {
    pub fn ip_allowed(&self, ip: IpAddr) -> bool {
        let ip = ip.to_canonical();
        self.any || self.allowed.iter().any(|a| a.to_canonical() == ip)
    }
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
        let h = h.trim().trim_start_matches('[').trim_end_matches(']');
        if h.is_empty() {
            continue;
        }
        if h == "*" {
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
    if hosts.iter().any(|h| h.trim() == "localhost") {
        auth.allowed.push(IpAddr::from([127, 0, 0, 1]));
        auth.allowed.push("::1".parse().expect("ip"));
    }
    (auth, warnings)
}

fn query_token(uri: &str) -> Option<String> {
    let q = uri.split_once('?')?.1;
    for kv in q.split('&') {
        if let Some(v) = kv.strip_prefix("token=") {
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
pub async fn serve(hub: Arc<Hub>, listener: TcpListener, auth: Arc<Auth>) {
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
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

async fn reject_raw(mut stream: TcpStream) {
    use tokio::io::AsyncWriteExt;
    let body = "Forbidden: host not allowed\n";
    let resp = format!(
        "HTTP/1.1 403 Forbidden\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = tokio::time::timeout(Duration::from_secs(2), stream.write_all(resp.as_bytes())).await;
    let _ = stream.shutdown().await;
}

async fn handle_conn(hub: Arc<Hub>, stream: TcpStream, peer: SocketAddr, auth: Arc<Auth>) {
    if !auth.ip_allowed(peer.ip()) {
        tracing::warn!(%peer, "rejected connection: host not allowed");
        reject_raw(stream).await;
        return;
    }
    let _ = stream.set_nodelay(true);
    let mut query_ok = false;
    let auth2 = auth.clone();
    // The handshake callback signature (and its large `ErrorResponse`) is fixed by tungstenite.
    #[allow(clippy::result_large_err)]
    let cb = |req: &HsRequest, resp: HsResponse| -> Result<HsResponse, ErrorResponse> {
        match query_token(&req.uri().to_string()) {
            Some(t) if !auth2.token.is_empty() && t != auth2.token => {
                Err(forbidden("invalid token"))
            }
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
    let ws = match tokio::time::timeout(
        Duration::from_secs(10),
        tokio_tungstenite::accept_hdr_async_with_config(stream, cb, Some(cfg)),
    )
    .await
    {
        Ok(Ok(ws)) => ws,
        Ok(Err(e)) => {
            tracing::debug!(%peer, "websocket handshake failed: {e}");
            return;
        }
        Err(_) => return,
    };
    let authed = query_ok || auth.token.is_empty();
    let conn = hub.add_conn();
    conn.lock().authed = authed;
    tracing::info!(%peer, conn = conn.id, "connection opened");
    let (mut sink, mut source) = ws.split();

    // Writer.
    let wconn = conn.clone();
    let writer = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = wconn.out.notify.notified() => {}
                _ = wconn.kill.cancelled() => {}
            }
            if wconn.out.only_deltas() && !wconn.kill.is_cancelled() {
                tokio::time::sleep(Duration::from_millis(30)).await;
            }
            if wconn.out.overflow.load(std::sync::atomic::Ordering::SeqCst) {
                tracing::warn!(conn = wconn.id, "client too slow; dropping connection");
                let _ = sink.close().await;
                wconn.kill.cancel();
                return;
            }
            for f in wconn.out.drain() {
                let Ok(txt) = serde_json::to_string(&f) else {
                    continue;
                };
                if sink.feed(WsMsg::text(txt)).await.is_err() {
                    wconn.kill.cancel();
                    return;
                }
            }
            if sink.flush().await.is_err() {
                wconn.kill.cancel();
                return;
            }
            if wconn.kill.is_cancelled() {
                let _ = sink.close().await;
                return;
            }
        }
    });

    // Reader.
    loop {
        let msg = tokio::select! {
            m = source.next() => m,
            _ = conn.kill.cancelled() => break,
            _ = hub.shutdown.cancelled() => break,
        };
        let text = match msg {
            Some(Ok(WsMsg::Text(t))) => t.to_string(),
            Some(Ok(WsMsg::Binary(b))) => String::from_utf8_lossy(&b).to_string(),
            Some(Ok(WsMsg::Close(_))) | None | Some(Err(_)) => break,
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
        let hello_done = conn.lock().hello;
        if !hello_done {
            match frame.req {
                Request::Hello {
                    client,
                    version,
                    cwd,
                    token,
                } => {
                    let ok = conn.lock().authed
                        || (!auth.token.is_empty()
                            && token.as_deref() == Some(auth.token.as_str()));
                    if !ok {
                        if let Some(id) = frame.id {
                            conn.send(err_frame(id, "invalid or missing token".into()));
                        }
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        break;
                    }
                    {
                        let mut st = conn.lock();
                        st.authed = true;
                        st.hello = true;
                        st.frontend = client != CONTROL_CLIENT;
                        st.cwd = cwd.clone();
                    }
                    if client != CONTROL_CLIENT {
                        hub.frontend_joined();
                    }
                    tracing::info!(conn = conn.id, %client, version, "hello");
                    if let Some(id) = frame.id {
                        let mut inst = serde_json::to_value(
                            hub.instance
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .clone(),
                        )
                        .unwrap_or(Value::Null);
                        if let Some(o) = inst.as_object_mut() {
                            o.remove("token");
                        }
                        conn.send(ServerFrame::Response {
                            id,
                            ok: true,
                            result: json!({
                                "server_version": env!("CARGO_PKG_VERSION"),
                                "protocol": ref_proto::PROTOCOL_VERSION,
                                "instance": inst,
                                "cwd": cwd,
                            }),
                            error: None,
                        });
                    }
                    conn.send(ServerFrame::Sessions {
                        sessions: hub.infos(),
                    });
                }
                _ => {
                    if let Some(id) = frame.id {
                        conn.send(err_frame(id, "first frame must be hello".into()));
                    }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    break;
                }
            }
            continue;
        }
        let res = crate::api::handle(&hub, &conn, frame.req).await;
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
    tracing::info!(conn = conn.id, "connection closed");
    finish(&hub, &conn);
    let _ = tokio::time::timeout(Duration::from_secs(2), writer).await;
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
