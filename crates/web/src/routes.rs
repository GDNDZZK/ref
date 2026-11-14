//! HTTP routes: the ping, the WebSocket and the embedded frontend.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::{ConnectInfo, Query, State};
use axum::http::header::{ACCEPT_ENCODING, CACHE_CONTROL, CONTENT_ENCODING, CONTENT_TYPE, VARY};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri};
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::assets::{self, Asset};
use crate::guard::{self, HostPolicy, text_response};
use crate::key::Key;
use crate::ping;
use crate::tunnel;
use crate::upstream::Upstream;

pub const PING_PATH: &str = "/api/ping";
const WS_PATH: &str = "/ws";
const API_PREFIX: &str = "/api/";
const INDEX_PATH: &str = "/index.html";
const HASHED_ASSETS_PREFIX: &str = "/assets/";
pub const APP_NAME: &str = "ref-web";
const IMMUTABLE: &str = "public, max-age=31536000, immutable";
const REVALIDATE: &str = "no-cache";
/// Content codings (RFC 9110), as negotiated and as sent in `Content-Encoding`.
const BROTLI: &str = "br";
const GZIP: &str = "gzip";

pub struct AppState {
    /// What a browser must prove it knows before the tunnel opens.
    pub key: Key,
    /// Tunnels that have not finished the key handshake yet (bounded, see `tunnel`).
    pub handshakes: Arc<Semaphore>,
    pub handshake_timeout: Duration,
    pub hosts: HostPolicy,
    /// Directory new sessions start in ([DIR]).
    pub cwd: String,
    pub upstream: Upstream,
    pub heartbeat: tunnel::Heartbeat,
    pub shutdown: CancellationToken,
    /// Open tunnels: an upgraded connection is no longer tracked by the HTTP server, so
    /// shutdown waits for them here to let each tab receive its close frame.
    pub tunnels: TaskTracker,
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route(PING_PATH, get(ping))
        .route(WS_PATH, get(tunnel::ws_handler))
        .fallback(get(static_file))
        .layer(middleware::from_fn_with_state(state.clone(), guard::guard))
        .layer(middleware::from_fn(guard::security_headers))
        .with_state(state)
}

fn is_local_caller(peer: &SocketAddr) -> bool {
    peer.ip().to_canonical().is_loopback()
}

#[derive(Deserialize)]
struct PingQuery {
    nonce: Option<String>,
}

#[derive(Serialize)]
struct Ping {
    app: &'static str,
    version: &'static str,
    /// HMAC of the caller's nonce, keyed with the key (see `ping`).
    proof: String,
}

/// Lets a second ref-web recognise this one (see `ping`). A remote host gets no (nonce, proof)
/// pairs to collect: it is refused.
async fn ping(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(query): Query<PingQuery>,
) -> Response {
    if !is_local_caller(&peer) {
        return text_response(StatusCode::FORBIDDEN, "Forbidden: local callers only\n");
    }
    let Some(nonce) = query.nonce.filter(|n| ping::is_valid_nonce(n)) else {
        return text_response(
            StatusCode::BAD_REQUEST,
            "Bad request: missing or invalid nonce\n",
        );
    };
    Json(Ping {
        app: APP_NAME,
        version: env!("CARGO_PKG_VERSION"),
        proof: ping::proof(state.key.root(), &nonce),
    })
    .into_response()
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
    let last_segment = path.rsplit_once('/').map_or(path, |(_, last)| last);
    let is_page_route = !last_segment.contains('.')
        && !path.starts_with(API_PREFIX)
        && path != WS_PATH
        && !path.starts_with(HASHED_ASSETS_PREFIX);
    is_page_route.then(|| assets::find(INDEX_PATH)).flatten()
}

fn asset_response(asset: &Asset, request_headers: &HeaderMap) -> Response {
    let accepts = |coding: &str| accepts_encoding(request_headers, coding);
    let (body, encoding) = match (asset.br, asset.gz) {
        (Some(br), _) if accepts(BROTLI) => (br, Some(BROTLI)),
        (_, Some(gz)) if accepts(GZIP) => (gz, Some(GZIP)),
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
        response
            .headers_mut()
            .insert(CONTENT_ENCODING, HeaderValue::from_static(encoding));
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
            let (name, params) = item.split_once(';').unwrap_or((item, ""));
            let name = name.trim();
            let q_zero = params.split(';').any(|p| {
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
    fn only_loopback_peers_may_ping() {
        for local in [
            "127.0.0.1:1",
            "[::1]:1",
            "[::ffff:127.0.0.1]:1",
            "127.9.9.9:1",
        ] {
            assert!(is_local_caller(&local.parse().unwrap()), "{local}");
        }
        for remote in [
            "192.168.1.5:1",
            "[fe80::1]:1",
            "[::ffff:10.0.0.1]:1",
            "8.8.8.8:1",
        ] {
            assert!(!is_local_caller(&remote.parse().unwrap()), "{remote}");
        }
    }

    #[test]
    fn spa_fallback_only_for_page_routes() {
        assert!(spa_fallback("/sessions/abc").is_some());
        assert!(spa_fallback("/missing.js").is_none());
        assert!(spa_fallback("/assets/nothing").is_none());
        assert!(spa_fallback("/api/nothing").is_none());
    }
}
