//! `GET /api/ping`: lets a second ref-web recognise the first one without being fooled by
//! another program on the port. The caller sends a random nonce; only a process that knows
//! the key can answer with its HMAC. Without that check, the address (with the key) would be
//! opened on whatever listens there. Only loopback peers get an answer (`routes::ping`), so
//! remote hosts cannot collect verifiers; local processes on the same machine can still query
//! `/api/ping` and obtain a `(nonce, proof)` verifier (an offline guess oracle for weak keys).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use serde::Deserialize;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpSocket, TcpStream};

use crate::channel::{self, Root};
use crate::guard::constant_time_eq;
use crate::key::Key;
use crate::launch;
use crate::routes::PING_PATH;
use crate::token;

const NONCE_PARAM: &str = "nonce";
/// Longest nonce the server accepts.
const MAX_NONCE_LEN: usize = 128;
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const HEADERS_END: &str = "\r\n\r\n";

/// Hex proof for `nonce`, keyed with the key the channel is made from. Loopback callers can
/// request one for any nonce (an offline guess oracle for weak `--key` values).
pub fn proof(root: &Root, nonce: &str) -> String {
    channel::to_hex(&root.ping_proof(nonce))
}

pub fn is_valid_nonce(nonce: &str) -> bool {
    !nonce.is_empty()
        && nonce.len() <= MAX_NONCE_LEN
        && nonce.bytes().all(|b| b.is_ascii_hexdigit())
}

#[derive(Deserialize)]
struct PingBody {
    app: String,
    proof: String,
}

/// True when a ref-web that shares our key answers on `host:port`. Always probes from a loopback
/// source address so `routes::ping` accepts the probe even when the instance bound a LAN IP.
pub async fn is_our_ref_web(host: &str, port: u16, key: &Key) -> bool {
    let nonce = token::random_hex();
    let probe = async {
        let mut stream = connect_via_loopback(host, port).await?;
        let request = format!(
            "GET {PING_PATH}?{NONCE_PARAM}={nonce} HTTP/1.1\r\nHost: {host}:{port}\r\n\
             Connection: close\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).await?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await?;
        Ok::<_, std::io::Error>(response)
    };
    let response = match tokio::time::timeout(PROBE_TIMEOUT, probe).await {
        Ok(Ok(response)) => String::from_utf8_lossy(&response).into_owned(),
        Ok(Err(_)) | Err(_) => return false,
    };
    let Some((_, body)) = response.split_once(HEADERS_END) else {
        return false;
    };
    let Ok(body) = serde_json::from_str::<PingBody>(body) else {
        return false;
    };
    body.app == crate::routes::APP_NAME
        && constant_time_eq(body.proof.as_bytes(), proof(key.root(), &nonce).as_bytes())
}

async fn connect_via_loopback(host: &str, port: u16) -> std::io::Result<TcpStream> {
    let bare = launch::bare_host(host);
    let mut last_err = None;
    for addr in tokio::net::lookup_host((bare, port)).await? {
        let res = if addr.ip().is_loopback() {
            TcpStream::connect(addr).await
        } else {
            connect_bound_loopback(addr).await
        };
        match res {
            Ok(stream) => return Ok(stream),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::AddrNotAvailable,
            "no addresses resolved",
        )
    }))
}

async fn connect_bound_loopback(target: SocketAddr) -> std::io::Result<TcpStream> {
    let (socket, bind_addr) = match target {
        SocketAddr::V4(_) => (
            TcpSocket::new_v4()?,
            SocketAddr::from((IpAddr::V4(Ipv4Addr::LOCALHOST), 0)),
        ),
        SocketAddr::V6(_) => (
            TcpSocket::new_v6()?,
            SocketAddr::from((IpAddr::V6(Ipv6Addr::LOCALHOST), 0)),
        ),
    };
    socket.bind(bind_addr)?;
    socket.connect(target).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proof_depends_on_key_and_nonce() {
        let root = Root::derive(b"a key of some length", &[1; channel::SALT_LEN]);
        let p = proof(&root, "abcd");
        assert_eq!(p.len(), 64);
        assert_eq!(p, proof(&root, "abcd"));
        assert_ne!(p, proof(&root, "abce"));
        let other = Root::derive(b"another key of length", &[1; channel::SALT_LEN]);
        assert_ne!(p, proof(&other, "abcd"));
    }

    #[test]
    fn nonce_validation() {
        assert!(is_valid_nonce("00ff"));
        assert!(!is_valid_nonce(""));
        assert!(!is_valid_nonce("xyz"));
        assert!(!is_valid_nonce(&"a".repeat(MAX_NONCE_LEN + 1)));
    }
}
