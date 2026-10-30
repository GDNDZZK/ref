//! `GET /api/ping`: lets a second ref-web recognise the first one without being fooled by
//! another program on the port. The caller sends a random nonce; only a process that knows
//! the login token can answer with its HMAC. Without that check, the login URL (with the
//! token) would be opened on whatever listens there.

use std::time::Duration;

use hmac::{Hmac, Mac as _};
use serde::Deserialize;
use sha2::Sha256;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

use crate::guard::constant_time_eq;
use crate::routes::PING_PATH;
use crate::token;

const NONCE_PARAM: &str = "nonce";
/// Longest nonce the server accepts.
const MAX_NONCE_LEN: usize = 128;
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const HEADERS_END: &str = "\r\n\r\n";

/// Hex HMAC-SHA256 of `nonce`, keyed with the login token.
pub fn proof(login_token: &str, nonce: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(login_token.as_bytes())
        .expect("HMAC accepts keys of any length");
    mac.update(nonce.as_bytes());
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
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

/// True when a ref-web that shares our login token answers on `host:port`.
pub async fn is_our_ref_web(host: &str, port: u16, login_token: &str) -> bool {
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
        && constant_time_eq(body.proof.as_bytes(), proof(login_token, &nonce).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proof_depends_on_token_and_nonce() {
        let p = proof("token", "abcd");
        assert_eq!(p.len(), 64);
        assert_eq!(p, proof("token", "abcd"));
        assert_ne!(p, proof("other", "abcd"));
        assert_ne!(p, proof("token", "abce"));
    }

    #[test]
    fn nonce_validation() {
        assert!(is_valid_nonce("00ff"));
        assert!(!is_valid_nonce(""));
        assert!(!is_valid_nonce("xyz"));
        assert!(!is_valid_nonce(&"a".repeat(MAX_NONCE_LEN + 1)));
    }
}
