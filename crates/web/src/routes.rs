//! HTTP routes: login, ping, info and the embedded frontend.

use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::{Query, State};
use axum::http::header::{
    ACCEPT_ENCODING, CACHE_CONTROL, CONTENT_ENCODING, CONTENT_TYPE, LOCATION, REFERRER_POLICY,
    SET_COOKIE, VARY,
};
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::assets::{self, Asset};
use crate::guard::{self, HostPolicy, SessionCookie, constant_time_eq, text_response};
use crate::launch;
use crate::ping;
use crate::tunnel;
use crate::upstream::Upstream;

pub const LOGIN_PATH: &str = "/login";
pub const PING_PATH: &str = "/api/ping";
const INFO_PATH: &str = "/api/info";
const WS_PATH: &str = "/ws";
const API_PREFIX: &str = "/api/";
const INDEX_PATH: &str = "/index.html";
const HASHED_ASSETS_PREFIX: &str = "/assets/";
pub const APP_NAME: &str = "ref-web";
const IMMUTABLE: &str = "public, max-age=31536000, immutable";
const REVALIDATE: &str = "no-cache";
const NO_STORE: &str = "no-store";

pub struct AppState {
    /// What `/login?token=` must present.
    pub login_token: String,
    pub session: SessionCookie,
    pub hosts: HostPolicy,
    /// Directory new sessions start in ([DIR]).
    pub cwd: String,
    pub upstream: Upstream,
    pub heartbeat: tunnel::Heartbeat,
    pub shutdown: CancellationToken,
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route(LOGIN_PATH, get(login))
        .route(PING_PATH, get(ping))
        .route(INFO_PATH, get(info))
        .route(WS_PATH, get(tunnel::ws_handler))
        .fallback(get(static_file))
        .layer(middleware::from_fn_with_state(state.clone(), guard::guard))
        .layer(middleware::from_fn(guard::security_headers))
        .with_state(state)
}

#[derive(Deserialize)]
struct LoginQuery {
    token: Option<String>,
    /// Directory the page should open sessions in; handed on to the page as `?dir=`.
    dir: Option<String>,
}

/// Trades the login token for the session cookie and redirects to a URL without the token.
async fn login(State(state): State<Arc<AppState>>, Query(query): Query<LoginQuery>) -> Response {
    let token_ok = query
        .token
        .as_deref()
        .is_some_and(|t| constant_time_eq(t.as_bytes(), state.login_token.as_bytes()));
    if !token_ok {
        return text_response(StatusCode::FORBIDDEN, "Forbidden: invalid login token\n");
    }
    let location = launch::page_path(query.dir.as_deref());
    (
        StatusCode::SEE_OTHER,
        [
            (LOCATION, location),
            (SET_COOKIE, state.session.set_cookie()),
            (CACHE_CONTROL, NO_STORE.to_string()),
            (REFERRER_POLICY, "no-referrer".to_string()),
        ],
    )
        .into_response()
}

#[derive(Deserialize)]
struct PingQuery {
    nonce: Option<String>,
}

#[derive(Serialize)]
struct Ping {
    app: &'static str,
    version: &'static str,
    /// HMAC of the caller's nonce, keyed with the login token.
    proof: String,
}

/// Lets a second ref-web recognise this one (see `ping`).
async fn ping(State(state): State<Arc<AppState>>, Query(query): Query<PingQuery>) -> Response {
    let Some(nonce) = query.nonce.filter(|n| ping::is_valid_nonce(n)) else {
        return text_response(
            StatusCode::BAD_REQUEST,
            "Bad request: missing or invalid nonce\n",
        );
    };
    Json(Ping {
        app: APP_NAME,
        version: env!("CARGO_PKG_VERSION"),
        proof: ping::proof(&state.login_token, &nonce),
    })
    .into_response()
}

#[derive(Serialize)]
struct Info {
    version: &'static str,
    cwd: String,
    mock: bool,
}

async fn info(State(state): State<Arc<AppState>>) -> Response {
    let info = Info {
        version: env!("CARGO_PKG_VERSION"),
        cwd: state.cwd.clone(),
        mock: state.upstream.is_mock(),
    };
    ([(CACHE_CONTROL, NO_STORE)], Json(info)).into_response()
}

/// Serves an embedded file. Paths without a file extension that match no file are page
/// routes of the single-page app and get `index.html`.
async fn static_file(headers: HeaderMap, uri: Uri) -> Response {
    let path = uri.path();
    let path = if path == "/" { INDEX_PATH } else { path };
    let asset = assets::find(path).or_else(|| spa_fallback(path));
    match asset {
        Some(asset) => asset_response(asset, &headers),
        None => text_response(StatusCode::NOT_FOUND, "Not found\n"),
    }
}

fn spa_fallback(path: &str) -> Option<&'static Asset> {
    let last_segment = path.rsplit('/').next().unwrap_or("");
    let is_page_route = !last_segment.contains('.')
        && !path.starts_with(API_PREFIX)
        && path != WS_PATH
        && !path.starts_with(HASHED_ASSETS_PREFIX);
    is_page_route.then(|| assets::find(INDEX_PATH)).flatten()
}

fn asset_response(asset: &Asset, request_headers: &HeaderMap) -> Response {
    let accepts = |coding: &str| accepts_encoding(request_headers, coding);
    let (body, encoding) = match (asset.br, asset.gz) {
        (Some(br), _) if accepts("br") => (br, Some("br")),
        (_, Some(gz)) if accepts("gzip") => (gz, Some("gzip")),
        _ => (asset.raw, None),
    };
    let cache = if asset.path.starts_with(HASHED_ASSETS_PREFIX) {
        IMMUTABLE
    } else {
        REVALIDATE
    };
    let mut response = (
        [
            (CONTENT_TYPE, asset.mime),
            (CACHE_CONTROL, cache),
            (VARY, "Accept-Encoding"),
        ],
        body,
    )
        .into_response();
    if let Some(encoding) = encoding {
        response.headers_mut().insert(
            CONTENT_ENCODING,
            encoding.parse().expect("static header value"),
        );
    }
    response
}

/// Whether `Accept-Encoding` lists `coding` with a non-zero quality.
fn accepts_encoding(headers: &HeaderMap, coding: &str) -> bool {
    headers
        .get_all(ACCEPT_ENCODING)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|line| line.split(','))
        .any(|item| {
            let mut parts = item.split(';');
            let name = parts.next().unwrap_or("").trim();
            let q_zero = parts.any(|p| {
                p.trim()
                    .strip_prefix("q=")
                    .is_some_and(|q| q.trim().parse::<f32>().is_ok_and(|q| q == 0.0))
            });
            name.eq_ignore_ascii_case(coding) && !q_zero
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accept(value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(ACCEPT_ENCODING, value.parse().unwrap());
        h
    }

    #[test]
    fn encoding_negotiation() {
        assert!(accepts_encoding(&accept("gzip, deflate, br"), "br"));
        assert!(accepts_encoding(&accept("gzip;q=0.5"), "gzip"));
        assert!(!accepts_encoding(&accept("br;q=0, gzip"), "br"));
        assert!(!accepts_encoding(&accept("identity"), "gzip"));
        assert!(!accepts_encoding(&HeaderMap::new(), "br"));
    }

    #[test]
    fn spa_fallback_only_for_page_routes() {
        assert!(spa_fallback("/sessions/abc").is_some());
        assert!(spa_fallback("/missing.js").is_none());
        assert!(spa_fallback("/assets/nothing").is_none());
        assert!(spa_fallback("/api/nothing").is_none());
    }
}
