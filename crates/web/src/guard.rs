//! Who may talk to ref-web: Host check (DNS rebinding) and the WebSocket Origin. Signing in is
//! the key handshake on `/ws` (`channel`, `tunnel`); the pages themselves are public.

use std::net::IpAddr;
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use subtle::ConstantTimeEq as _;

use crate::routes::AppState;

pub(crate) const LOCALHOST: &str = "localhost";
const DEFAULT_HTTP_PORT: u16 = 80;

/// Only same-origin loads; images may also be `data:` and `blob:` URLs.
const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; style-src 'self'; \
    img-src 'self' data: blob:; connect-src 'self'; frame-ancestors 'none'; \
    base-uri 'none'; form-action 'self'";

/// Which `Host` headers are served.
///
/// A DNS-rebinding page reaches ref-web under a hostname of its own, so only IP literals,
/// `localhost` and the name given with `--host` are accepted, on the listening port.
pub struct HostPolicy {
    port: u16,
    listen_host: String,
}

impl HostPolicy {
    pub fn new(listen_host: &str, port: u16) -> Self {
        HostPolicy {
            port,
            listen_host: listen_host.to_ascii_lowercase(),
        }
    }

    pub fn allows(&self, host_header: &str) -> bool {
        let Some((name, port)) = split_host_port(host_header) else {
            return false;
        };
        let name = name.to_ascii_lowercase();
        let known_name = name == LOCALHOST || name == self.listen_host;
        let ip_literal = name.parse::<IpAddr>().is_ok();
        port == self.port && (known_name || ip_literal)
    }
}

/// `example:80`, `[::1]:7480` and `127.0.0.1` (port defaults to 80) → name without brackets, port.
fn split_host_port(host: &str) -> Option<(&str, u16)> {
    if let Some(rest) = host.strip_prefix('[') {
        let (name, after) = rest.split_once(']')?;
        let port = match after.strip_prefix(':') {
            Some(p) => p.parse().ok()?,
            None if after.is_empty() => DEFAULT_HTTP_PORT,
            None => return None,
        };
        return Some((name, port));
    }
    match host.rsplit_once(':') {
        Some((name, port)) => Some((name, port.parse().ok()?)),
        None => Some((host, DEFAULT_HTTP_PORT)),
    }
}

/// Browsers send `Origin: http://<host>` on a same-origin WebSocket handshake.
pub fn origin_matches_host(origin: &str, host: &str) -> bool {
    origin
        .strip_prefix("http://")
        .is_some_and(|o| o.eq_ignore_ascii_case(host))
}

/// Compares without leaking where the first difference is.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.ct_eq(b).into()
}

pub fn text_response(status: StatusCode, text: &'static str) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        text,
    )
        .into_response()
}

/// Host check for every request.
pub async fn guard(State(state): State<Arc<AppState>>, req: Request, next: Next) -> Response {
    let host_ok = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|h| state.hosts.allows(h));
    if !host_ok {
        return text_response(StatusCode::FORBIDDEN, "Forbidden: host not allowed\n");
    }
    next.run(req).await
}

pub async fn security_headers(req: Request, next: Next) -> Response {
    let mut response = next.run(req).await;
    let headers = response.headers_mut();
    for (name, value) in [
        (header::CONTENT_SECURITY_POLICY, CONTENT_SECURITY_POLICY),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::X_FRAME_OPTIONS, "DENY"),
        // A key in the query string must not travel on in a Referer header.
        (header::REFERRER_POLICY, "no-referrer"),
    ] {
        headers.insert(name, HeaderValue::from_static(value));
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_policy() {
        let p = HostPolicy::new("192.168.1.5", 7480);
        for ok in [
            "127.0.0.1:7480",
            "localhost:7480",
            "LOCALHOST:7480",
            "[::1]:7480",
            "192.168.1.5:7480",
            "10.0.0.2:7480",
        ] {
            assert!(p.allows(ok), "{ok}");
        }
        for bad in [
            "evil.example:7480",
            "127.0.0.1:80",
            "127.0.0.1",
            "localhost:abc",
            "[::1",
            "",
        ] {
            assert!(!p.allows(bad), "{bad}");
        }
        let named = HostPolicy::new("mybox", 8000);
        assert!(named.allows("mybox:8000"));
        assert!(!named.allows("otherbox:8000"));
    }

    #[test]
    fn origin_must_be_the_host_over_http() {
        assert!(origin_matches_host(
            "http://127.0.0.1:7480",
            "127.0.0.1:7480"
        ));
        assert!(!origin_matches_host(
            "http://127.0.0.1:5173",
            "127.0.0.1:7480"
        ));
        assert!(!origin_matches_host(
            "https://127.0.0.1:7480",
            "127.0.0.1:7480"
        ));
        assert!(!origin_matches_host("null", "127.0.0.1:7480"));
    }
}
