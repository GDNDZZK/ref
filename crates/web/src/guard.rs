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
const DEFAULT_HTTPS_PORT: u16 = 443;
/// Only same-origin loads; images may also be `data:` and `blob:` URLs.
const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; style-src 'self'; \
    img-src 'self' data: blob:; connect-src 'self'; frame-ancestors 'none'; \
    base-uri 'none'; form-action 'self'";

/// Which `Host` headers are served.
///
/// A DNS-rebinding page reaches ref-web under a hostname of its own, so only IP literals,
/// `localhost` and the name given with `--host` are accepted, on the listening port.
#[derive(Clone)]
pub struct HostPolicy {
    port: u16,
    listen_host: String,
    tls: bool,
}

impl HostPolicy {
    pub fn new(listen_host: &str, port: u16, tls: bool) -> Self {
        HostPolicy {
            port,
            listen_host: listen_host.to_ascii_lowercase(),
            tls,
        }
    }

    pub fn tls(&self) -> bool {
        self.tls
    }

    pub fn allows(&self, host_header: &str) -> bool {
        let default_port = if self.tls {
            DEFAULT_HTTPS_PORT
        } else {
            DEFAULT_HTTP_PORT
        };
        let Some((name, port)) = split_host_port(host_header, default_port) else {
            return false;
        };
        let name = name.to_ascii_lowercase();
        let known_name = name == LOCALHOST || name == self.listen_host;
        let ip_literal = name.parse::<IpAddr>().is_ok();
        port == self.port && (known_name || ip_literal)
    }
}

/// `example:80`, `[::1]:7480` and `127.0.0.1` (port defaults to `default_port`) -> name without brackets, port.
fn split_host_port(host: &str, default_port: u16) -> Option<(&str, u16)> {
    if let Some(rest) = host.strip_prefix('[') {
        let (name, after) = rest.split_once(']')?;
        let port = match after.strip_prefix(':') {
            Some(p) => p.parse().ok()?,
            None if after.is_empty() => default_port,
            None => return None,
        };
        return Some((name, port));
    }
    match host.rsplit_once(':') {
        Some((name, port)) => Some((name, port.parse().ok()?)),
        None => Some((host, default_port)),
    }
}

/// Browsers send `Origin: http://<host>` (or `https://<host>` with TLS) on a same-origin WebSocket handshake.
pub fn origin_matches_host(origin: &str, host: &str, tls: bool) -> bool {
    let scheme = if tls { "https://" } else { "http://" };
    origin
        .strip_prefix(scheme)
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
        let p = HostPolicy::new("192.168.1.5", 7480, false);
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
        let named = HostPolicy::new("mybox", 8000, false);
        assert!(named.allows("mybox:8000"));
        assert!(!named.allows("otherbox:8000"));

        let tls_default = HostPolicy::new("0.0.0.0", 443, true);
        assert!(tls_default.allows("127.0.0.1"));
        assert!(tls_default.allows("[::1]"));
        let plain_default = HostPolicy::new("0.0.0.0", 80, false);
        assert!(plain_default.allows("127.0.0.1"));
        assert!(!tls_default.allows("127.0.0.1:80"));
    }

    #[test]
    fn origin_must_match_host_and_scheme() {
        assert!(origin_matches_host(
            "http://127.0.0.1:7480",
            "127.0.0.1:7480",
            false
        ));
        assert!(!origin_matches_host(
            "http://127.0.0.1:5173",
            "127.0.0.1:7480",
            false
        ));
        assert!(!origin_matches_host(
            "https://127.0.0.1:7480",
            "127.0.0.1:7480",
            false
        ));
        assert!(origin_matches_host(
            "https://127.0.0.1:7480",
            "127.0.0.1:7480",
            true
        ));
        assert!(!origin_matches_host(
            "http://127.0.0.1:7480",
            "127.0.0.1:7480",
            true
        ));
        assert!(!origin_matches_host("null", "127.0.0.1:7480", false));
        assert!(!origin_matches_host("null", "127.0.0.1:7480", true));
    }
}
