//! `GET /api/ping`: lets a second ref-web recognise the first one without being fooled by
//! another program on the port. The caller sends a random nonce; only a process that knows
//! the key can answer with its HMAC. Without that check, the address (with the key) would be
//! opened on whatever listens there. Only loopback peers get an answer (`routes::ping`).

use std::time::Duration;

use serde::Deserialize;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

use crate::channel::{self, Root};
use crate::guard::constant_time_eq;
use crate::key::Key;
use crate::routes::PING_PATH;
use crate::token;

const NONCE_PARAM: &str = "nonce";
/// Longest nonce the server accepts.
const MAX_NONCE_LEN: usize = 128;
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const HEADERS_END: &str = "\r\n\r\n";

/// Hex proof for `nonce`, keyed with the key the channel is made from.
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

/// True when a ref-web that shares our key answers on `host:port`.
pub async fn is_our_ref_web(host: &str, port: u16, key: &Key) -> bool {
    let nonce = token::random_hex();
    let probe = async {
        let mut stream = tokio::net::TcpStream::connect(format!("{host}:{port}")).await?;
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
