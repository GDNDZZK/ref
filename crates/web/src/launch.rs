//! The address ref-web prints and opens, and opening the browser.

use std::net::IpAddr;

const LOOPBACK_V4: &str = "127.0.0.1";
const LOOPBACK_V6: &str = "[::1]";

/// A host without the brackets of an IPv6 literal (`[::1]` → `::1`).
pub(crate) fn bare_host(host: &str) -> &str {
    host.trim_start_matches('[').trim_end_matches(']')
}

/// Host to put in URLs and to connect to for a listening address (`0.0.0.0` → loopback).
pub fn reachable_host(listen_host: &str) -> String {
    let bare = bare_host(listen_host);
    match bare.parse::<IpAddr>() {
        Ok(ip) if ip.is_unspecified() && ip.is_ipv4() => LOOPBACK_V4.to_string(),
        Ok(ip) if ip.is_unspecified() => LOOPBACK_V6.to_string(),
        Ok(IpAddr::V6(ip)) => format!("[{ip}]"),
        _ => bare.to_string(),
    }
}

/// The page path: `/`, or `/?dir=…` so the page opens sessions in `dir`.
pub fn page_path(dir: Option<&str>) -> String {
    match dir.filter(|d| !d.is_empty()) {
        Some(dir) => format!("/?dir={}", percent_encode(dir)),
        None => "/".to_string(),
    }
}

/// The address that signs the browser in. The key sits in the fragment, which a browser never
/// sends to a server.
pub fn key_url(host: &str, port: u16, key: &str, dir: Option<&str>) -> String {
    format!(
        "http://{host}:{port}{}#key={}",
        page_path(dir),
        percent_encode(key)
    )
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
        say_err!("cannot open the browser ({e}); open the address above yourself");
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
    }

    #[test]
    fn the_key_goes_into_the_fragment_encoded() {
        assert_eq!(
            key_url("127.0.0.1", 7480, "tok", Some("/a b")),
            "http://127.0.0.1:7480/?dir=/a%20b#key=tok"
        );
        assert_eq!(key_url("h", 1, "t", None), "http://h:1/#key=t");
        assert_eq!(
            key_url("h", 1, "a b&c+d#e", None),
            "http://h:1/#key=a%20b%26c%2Bd%23e"
        );
    }
}
