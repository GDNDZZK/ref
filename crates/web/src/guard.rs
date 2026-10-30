//! Who may talk to ref-web: Host check (DNS rebinding), login cookie, WebSocket Origin.

use std::net::IpAddr;
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::header::{CONTENT_TYPE, COOKIE, HOST};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use subtle::ConstantTimeEq as _;

use crate::routes::{AppState, LOGIN_PATH, PING_PATH};
use crate::token;

const COOKIE_NAME_PREFIX: &str = "ref_web_";
const COOKIE_MAX_AGE_SECS: u32 = 30 * 24 * 60 * 60;
const LOCALHOST: &str = "localhost";
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

/// The login cookie. Browsers share cookies between ports of a host, so other servers on
/// `localhost` receive it too. Hence the name carries the port (two ref-web instances do not
/// overwrite each other's cookie), and the value is a random id that only this process knows
/// and that stops working when it exits, not the login token that sits in a file.
pub struct SessionCookie {
    name: String,
    id: String,
}

impl SessionCookie {
    pub fn new(port: u16) -> Self {
        SessionCookie {
            name: format!("{COOKIE_NAME_PREFIX}{port}"),
            id: token::random_hex(),
        }
    }

    /// Value of the `Set-Cookie` header that logs the browser in.
    pub fn set_cookie(&self) -> String {
        format!(
            "{}={}; Path=/; HttpOnly; SameSite=Strict; Max-Age={COOKIE_MAX_AGE_SECS}",
            self.name, self.id
        )
    }

    pub fn is_sent_in(&self, headers: &HeaderMap) -> bool {
        headers
            .get_all(COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|line| line.split(';'))
            .filter_map(|pair| pair.trim().split_once('='))
            .any(|(name, value)| {
                name == self.name && constant_time_eq(value.as_bytes(), self.id.as_bytes())
            })
    }
}

pub fn text_response(status: StatusCode, text: &'static str) -> Response {
    (status, [(CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response()
}

/// Host check for every request, login cookie for everything except the login and ping routes.
pub async fn guard(State(state): State<Arc<AppState>>, req: Request, next: Next) -> Response {
    let host_ok = req
        .headers()
        .get(HOST)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|h| state.hosts.allows(h));
    if !host_ok {
        return text_response(StatusCode::FORBIDDEN, "Forbidden: host not allowed\n");
    }
    let path = req.uri().path();
    let open_route = path == LOGIN_PATH || path == PING_PATH;
    if !open_route && !state.session.is_sent_in(req.headers()) {
        return text_response(
            StatusCode::UNAUTHORIZED,
            "Not logged in. Run ref-web again: it opens the login URL again.\n",
        );
    }
    next.run(req).await
}

pub async fn security_headers(req: Request, next: Next) -> Response {
    let mut response = next.run(req).await;
    let headers = response.headers_mut();
    for (name, value) in [
        ("content-security-policy", CONTENT_SECURITY_POLICY),
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
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

    #[test]
    fn cookie_lookup() {
        let cookie = SessionCookie::new(7480);
        let sent = |line: String| {
            let mut h = HeaderMap::new();
            h.insert(COOKIE, line.parse().unwrap());
            cookie.is_sent_in(&h)
        };
        let (name, id) = (cookie.name.clone(), cookie.id.clone());
        assert!(sent(format!("a=1; {name}={id}; b=2")));
        assert!(!sent(format!("{name}=other")));
        assert!(!sent(format!("x{name}={id}")));
        assert!(!sent(format!("ref_web_7481={id}")));
        assert!(!cookie.is_sent_in(&HeaderMap::new()));
    }

    #[test]
    fn cookie_name_has_the_port_and_value_is_random() {
        let (a, b) = (SessionCookie::new(1), SessionCookie::new(1));
        assert_eq!(a.name, "ref_web_1");
        assert_ne!(a.id, b.id);
        let header = a.set_cookie();
        assert!(
            header.starts_with(&format!("ref_web_1={}; ", a.id)),
            "{header}"
        );
        assert!(header.contains("HttpOnly") && header.contains("SameSite=Strict"));
    }
}
