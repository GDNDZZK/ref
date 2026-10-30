//! Login URLs, opening the browser, and recognising a ref-web that already runs.

use std::net::IpAddr;
use std::time::Duration;

use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

use crate::routes::{LOGIN_PATH, PING_PATH};

const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const PING_APP_FIELD: &str = "\"app\":\"ref-web\"";
const LOOPBACK_V4: &str = "127.0.0.1";
const LOOPBACK_V6: &str = "[::1]";

/// Host to put in URLs and to connect to for a listening address (`0.0.0.0` → loopback).
pub fn reachable_host(listen_host: &str) -> String {
    let bare = listen_host.trim_start_matches('[').trim_end_matches(']');
    match bare.parse::<IpAddr>() {
        Ok(ip) if ip.is_unspecified() && ip.is_ipv4() => LOOPBACK_V4.to_string(),
        Ok(ip) if ip.is_unspecified() => LOOPBACK_V6.to_string(),
        Ok(IpAddr::V6(ip)) => format!("[{ip}]"),
        _ => bare.to_string(),
    }
}

/// Where the login redirects to: `/`, or `/?dir=…` so the page opens sessions in `dir`.
pub fn page_path(dir: Option<&str>) -> String {
    match dir.filter(|d| !d.is_empty()) {
        Some(dir) => format!("/?dir={}", percent_encode(dir)),
        None => "/".to_string(),
    }
}

pub fn login_url(host: &str, port: u16, token: &str, dir: Option<&str>) -> String {
    let mut url = format!("http://{host}:{port}{LOGIN_PATH}?token={token}");
    if let Some(dir) = dir.filter(|d| !d.is_empty()) {
        url.push_str("&dir=");
        url.push_str(&percent_encode(dir));
    }
    url
}

/// Encodes everything except unreserved characters and `/`.
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn open_browser(url: &str) {
    if let Err(e) = open::that_detached(url) {
        eprintln!("ref-web: cannot open the browser ({e}); open the login URL above yourself");
    }
}

/// True when a ref-web answers on `host:port`.
pub async fn is_ref_web(host: &str, port: u16) -> bool {
    let probe = async {
        let mut stream = tokio::net::TcpStream::connect(format!("{host}:{port}")).await?;
        let request =
            format!("GET {PING_PATH} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).await?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await?;
        Ok::<_, std::io::Error>(response)
    };
    match tokio::time::timeout(PROBE_TIMEOUT, probe).await {
        Ok(Ok(response)) => String::from_utf8_lossy(&response).contains(PING_APP_FIELD),
        Ok(Err(_)) | Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_for_urls() {
        assert_eq!(reachable_host("0.0.0.0"), "127.0.0.1");
        assert_eq!(reachable_host("::"), "[::1]");
        assert_eq!(reachable_host("::1"), "[::1]");
        assert_eq!(reachable_host("192.168.1.5"), "192.168.1.5");
        assert_eq!(reachable_host("mybox"), "mybox");
    }

    #[test]
    fn dir_is_percent_encoded() {
        assert_eq!(page_path(None), "/");
        assert_eq!(page_path(Some("")), "/");
        assert_eq!(
            page_path(Some("/home/u/my proj&x")),
            "/?dir=/home/u/my%20proj%26x"
        );
        assert_eq!(
            login_url("127.0.0.1", 7480, "tok", Some("/a b")),
            "http://127.0.0.1:7480/login?token=tok&dir=/a%20b"
        );
        assert_eq!(login_url("h", 1, "t", None), "http://h:1/login?token=t");
    }
}
