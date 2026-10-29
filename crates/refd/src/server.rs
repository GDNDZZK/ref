//! HTTP / WebSocket server: access control, client connections, message dispatch,
//! idle timeout and graceful shutdown.

use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use futures::{SinkExt, StreamExt};
use ref_config::ConfigPaths;
use ref_proto::{ClientMsg, InstanceInfo, NoticeLevel, ServerMsg, PROTOCOL_VERSION};
use serde::Deserialize;

use crate::cli::ServeOpts;
use crate::hub::{Hub, HubOpts};
use crate::registry::{self, RegistryEntry};
use crate::session::manage::CreateOpts;
use crate::util;

const PING_EVERY: Duration = Duration::from_secs(20);
const CLIENT_DEAD_AFTER: Duration = Duration::from_secs(90);
const HELLO_TIMEOUT: Duration = Duration::from_secs(30);
/// Max time spent flushing queued messages to a disconnecting client.
const FLUSH_TIMEOUT: Duration = Duration::from_secs(5);
/// With idle-timeout 0: exit if nobody connects within this time after startup.
const FIRST_CONNECT_GRACE: Duration = Duration::from_secs(30);

#[derive(Clone)]
struct Access {
    allow_all: bool,
    ips: Arc<HashSet<IpAddr>>,
}

impl Access {
    fn allows(&self, ip: IpAddr) -> bool {
        let ip = canonical_ip(ip);
        ip.is_loopback() || self.allow_all || self.ips.contains(&ip)
    }
}

fn canonical_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(IpAddr::V6(v6)),
        v4 => v4,
    }
}

async fn resolve_hosts(hosts: &[String]) -> Access {
    let mut ips = HashSet::new();
    let mut allow_all = false;
    for h in hosts {
        let h = h.trim();
        if h == "*" {
            allow_all = true;
            continue;
        }
        if let Ok(ip) = h.trim_matches(['[', ']']).parse::<IpAddr>() {
            ips.insert(canonical_ip(ip));
            continue;
        }
        match tokio::net::lookup_host((h, 0)).await {
            Ok(addrs) => {
                for a in addrs {
                    ips.insert(canonical_ip(a.ip()));
                }
            }
            Err(e) => tracing::warn!("cannot resolve allow-host {h}: {e}"),
        }
    }
    Access { allow_all, ips: Arc::new(ips) }
}

#[derive(Clone)]
struct AppState {
    hub: Arc<Hub>,
    access: Access,
}

#[derive(Deserialize)]
struct TokenQuery {
    #[serde(default)]
    token: String,
}

#[allow(clippy::result_large_err)]
fn check(state: &AppState, addr: SocketAddr, token: &str) -> Result<(), Response> {
    if !state.access.allows(addr.ip()) {
        tracing::warn!("rejected connection from {addr} (not in allow-host list)");
        return Err((StatusCode::FORBIDDEN, "host not allowed").into_response());
    }
    if !util::ct_eq(token, &state.hub.token) {
        return Err((StatusCode::UNAUTHORIZED, "invalid token").into_response());
    }
    Ok(())
}

async fn status_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Query(q): Query<TokenQuery>,
) -> Response {
    if let Err(r) = check(&state, addr, &q.token) {
        return r;
    }
    Json(registry::StatusReply { instance: state.hub.instance.clone(), sessions: state.hub.open_count() as u64 })
        .into_response()
}

async fn ws_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Query(q): Query<TokenQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    if let Err(r) = check(&state, addr, &q.token) {
        return r;
    }
    let hub = state.hub.clone();
    // Clients send a whole message as one frame (e.g. a prompt with large images), so the
    // frame limit must match the message limit (axum's default frame limit is 16 MiB).
    const MAX_WS_MESSAGE: usize = 256 * 1024 * 1024;
    ws.max_message_size(MAX_WS_MESSAGE).max_frame_size(MAX_WS_MESSAGE).on_upgrade(move |socket| async move {
        handle_socket(hub, socket, addr).await;
    })
}

/// A running server.
pub struct Server {
    pub hub: Arc<Hub>,
    pub addr: SocketAddr,
    pub token: String,
    handle: tokio::task::JoinHandle<()>,
}

impl Server {
    /// Wait until the server has shut down completely.
    pub async fn wait(self) {
        let _ = self.handle.await;
    }

    pub fn shutdown(&self) {
        self.hub.shutdown.cancel();
    }
}

/// 127.0.0.1, or 0.0.0.0 when some allow-host is not a loopback address.
fn default_bind(allow_hosts: &[String]) -> &'static str {
    let loopback = |h: &str| {
        let h = h.trim().trim_matches(['[', ']']);
        h.eq_ignore_ascii_case("localhost") || h.parse::<IpAddr>().is_ok_and(|ip| canonical_ip(ip).is_loopback())
    };
    if allow_hosts.iter().all(|h| loopback(h)) {
        "127.0.0.1"
    } else {
        "0.0.0.0"
    }
}

async fn bind(opts: &ServeOpts) -> anyhow::Result<(tokio::net::TcpListener, String)> {
    let bind = match &opts.bind {
        Some(b) => b.trim_matches(['[', ']']).to_string(),
        None => default_bind(&opts.allow_hosts).to_string(),
    };
    let ip: IpAddr = match bind.parse() {
        Ok(ip) => ip,
        Err(_) => tokio::net::lookup_host((bind.as_str(), 0))
            .await
            .with_context(|| format!("cannot resolve bind address {bind}"))?
            .next()
            .map(|a| a.ip())
            .with_context(|| format!("cannot resolve bind address {bind}"))?,
    };
    if opts.port != 0 {
        let l = tokio::net::TcpListener::bind((ip, opts.port))
            .await
            .with_context(|| format!("cannot listen on {bind}:{}", opts.port))?;
        return Ok((l, bind));
    }
    use rand::Rng;
    for _ in 0..200 {
        let port: u16 = rand::thread_rng().gen_range(10000..60000);
        if let Ok(l) = tokio::net::TcpListener::bind((ip, port)).await {
            return Ok((l, bind));
        }
    }
    anyhow::bail!("no free port found in 10000..60000")
}

/// Bind and start serving. Returns once listening.
pub async fn start(opts: ServeOpts, config_paths: Option<ConfigPaths>) -> anyhow::Result<Server> {
    let (listener, bind) = bind(&opts).await?;
    let addr = listener.local_addr()?;
    let access = resolve_hosts(&opts.allow_hosts).await;
    let token = util::random_token();
    let instance = InstanceInfo {
        pid: std::process::id(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        bind,
        port: addr.port(),
        allow_hosts: opts.allow_hosts.clone(),
        idle_timeout: opts.idle_timeout,
        started: ref_proto::now_ms(),
        data_dir: opts.data_dir.to_string_lossy().into_owned(),
    };
    let hub = Hub::new(HubOpts { instance: instance.clone(), token: token.clone(), data_dir: opts.data_dir.clone(), config_paths });
    let registry_path = if opts.registry {
        let entry = RegistryEntry { instance: instance.clone(), token: token.clone() };
        match registry::write_entry(&opts.data_dir, &entry) {
            Ok(p) => Some(p),
            Err(e) => {
                tracing::error!("cannot write registry file: {e}");
                None
            }
        }
    } else {
        None
    };
    let state = AppState { hub: hub.clone(), access };
    let app = Router::new()
        .route("/status", get(status_handler))
        .route("/ws", get(ws_handler))
        .with_state(state);
    tracing::info!("refd {} listening on ws://{}:{} (pid {})", instance.version, instance.bind, addr.port(), instance.pid);

    if opts.signals {
        let hub2 = hub.clone();
        tokio::spawn(async move {
            wait_signal().await;
            tracing::info!("signal received; shutting down");
            hub2.shutdown.cancel();
        });
    }
    tokio::spawn(idle_watch(hub.clone(), opts.idle_timeout));

    let hub2 = hub.clone();
    let handle = tokio::spawn(async move {
        let shutdown = hub2.shutdown.clone();
        let serve = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
            .with_graceful_shutdown(async move { shutdown.cancelled().await });
        if let Err(e) = serve.await {
            tracing::error!("server error: {e}");
            hub2.shutdown.cancel();
        }
        // Graceful shutdown: abort sessions, persist, stop MCP servers.
        hub2.close_all().await;
        hub2.shutdown_mcp().await;
        if let Some(p) = registry_path {
            let _ = std::fs::remove_file(p);
        }
        tracing::info!("refd stopped");
    });
    Ok(Server { hub, addr, token, handle })
}

pub async fn run(opts: ServeOpts) -> anyhow::Result<()> {
    let server = start(opts, None).await?;
    server.wait().await;
    Ok(())
}

async fn wait_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut term = match signal(SignalKind::terminate()) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("cannot install SIGTERM handler: {e}");
            let _ = tokio::signal::ctrl_c().await;
            return;
        }
    };
    let mut hup = signal(SignalKind::hangup()).ok();
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
        _ = async { match hup.as_mut() { Some(h) => { h.recv().await; } None => std::future::pending::<()>().await } } => {}
    }
}

/// Exit when no frontend is connected for long enough (see `--idle-timeout`).
/// Strict semantics: open sessions do not keep the daemon alive (they are persisted and can be
/// resumed later).
async fn idle_watch(hub: Arc<Hub>, timeout: i64) {
    if timeout < 0 {
        return;
    }
    let started = Instant::now();
    let mut last_activity = Instant::now();
    let mut tick = tokio::time::interval(Duration::from_millis(200));
    loop {
        tokio::select! {
            _ = tick.tick() => {}
            _ = hub.shutdown.cancelled() => return,
        }
        if hub.client_count() > 0 {
            last_activity = Instant::now();
            continue;
        }
        let ever = hub.ever_connected.load(std::sync::atomic::Ordering::Relaxed);
        let idle = hub.since_last_client_change().min(last_activity.elapsed());
        if idle_expired(timeout, ever, idle, started.elapsed()) {
            tracing::info!("no frontend connected (idle timeout {timeout}s); shutting down");
            hub.shutdown.cancel();
            return;
        }
    }
}

/// Idle-timeout decision while no frontend is connected. `timeout` < 0 never expires.
/// `ever`: a frontend has connected at least once; `idle`: time since the last one left;
/// `uptime`: time since start.
fn idle_expired(timeout: i64, ever: bool, idle: Duration, uptime: Duration) -> bool {
    if timeout < 0 {
        return false;
    }
    if ever {
        return idle >= Duration::from_secs(timeout as u64);
    }
    // The timeout only takes effect after the first frontend connected. With 0 (the mode
    // used when a frontend spawns refd) give up if nobody connects within a grace period so
    // a failed frontend start does not leave an orphan daemon.
    timeout == 0 && uptime >= FIRST_CONNECT_GRACE
}

async fn handle_socket(hub: Arc<Hub>, socket: WebSocket, addr: SocketAddr) {
    let (mut sink, mut stream) = socket.split();
    // Hello first.
    let hello = tokio::time::timeout(HELLO_TIMEOUT, async {
        while let Some(Ok(m)) = stream.next().await {
            match m {
                WsMessage::Text(t) => return serde_json::from_str::<ClientMsg>(t.as_str()).ok(),
                WsMessage::Close(_) => return None,
                _ => {}
            }
        }
        None
    })
    .await;
    let cwd = match hello {
        Ok(Some(ClientMsg::Hello { cwd, client, protocol })) => {
            tracing::info!("client {client:?} connected from {addr} (protocol {protocol})");
            cwd
        }
        _ => {
            let err = ServerMsg::Error { req: 0, text: "expected hello".into() };
            if let Ok(s) = serde_json::to_string(&err) {
                let _ = sink.send(WsMessage::Text(s.into())).await;
            }
            let _ = sink.close().await;
            return;
        }
    };
    let (cid, mut rx, kill) = hub.register_client(cwd.clone());
    let catalog = hub.catalog(&cwd);
    hub.send(
        &cid,
        &ServerMsg::Welcome { instance: hub.instance.clone(), catalog, protocol: PROTOCOL_VERSION, client_id: cid.clone() },
    );

    let writer_kill = kill.clone();
    let shutdown = hub.shutdown.clone();
    let writer = tokio::spawn(async move {
        let mut ping = tokio::time::interval(PING_EVERY);
        ping.tick().await;
        loop {
            // Every send races the kill token: a client that stops reading (full TCP
            // buffer) must not block this task forever.
            let msg = tokio::select! {
                m = rx.recv() => match m {
                    Some(p) => WsMessage::Text(p.as_ref().into()),
                    None => break,
                },
                _ = ping.tick() => WsMessage::Ping(Default::default()),
                _ = writer_kill.cancelled() => break,
                _ = shutdown.cancelled() => break,
            };
            let ok = tokio::select! {
                r = sink.send(msg) => r.is_ok(),
                _ = writer_kill.cancelled() => false,
            };
            if !ok {
                break;
            }
        }
        // Flush whatever is queued (best effort, bounded) before closing.
        let _ = tokio::time::timeout(FLUSH_TIMEOUT, async {
            while let Ok(p) = rx.try_recv() {
                if sink.send(WsMessage::Text(p.as_ref().into())).await.is_err() {
                    break;
                }
            }
            let _ = sink.close().await;
        })
        .await;
        writer_kill.cancel();
    });

    let mut last_seen = Instant::now();
    let mut check = tokio::time::interval(Duration::from_secs(10));
    loop {
        tokio::select! {
            m = stream.next() => {
                let Some(Ok(m)) = m else { break };
                last_seen = Instant::now();
                match m {
                    WsMessage::Text(t) => match serde_json::from_str::<ClientMsg>(t.as_str()) {
                        Ok(msg) => dispatch(&hub, &cid, msg).await,
                        Err(e) => hub.send(&cid, &ServerMsg::Error { req: 0, text: format!("invalid message: {e}") }),
                    },
                    WsMessage::Binary(b) => match serde_json::from_slice::<ClientMsg>(&b) {
                        Ok(msg) => dispatch(&hub, &cid, msg).await,
                        Err(e) => hub.send(&cid, &ServerMsg::Error { req: 0, text: format!("invalid message: {e}") }),
                    },
                    WsMessage::Close(_) => break,
                    _ => {}
                }
            }
            _ = check.tick() => {
                if last_seen.elapsed() > CLIENT_DEAD_AFTER {
                    tracing::info!("client {cid} timed out");
                    break;
                }
            }
            _ = kill.cancelled() => break,
            _ = hub.shutdown.cancelled() => break,
        }
    }
    hub.unregister_client(&cid);
    kill.cancel();
    let mut writer = writer;
    if tokio::time::timeout(FLUSH_TIMEOUT + Duration::from_secs(1), &mut writer).await.is_err() {
        writer.abort();
    }
    tracing::info!("client {cid} disconnected");
}

fn err(hub: &Hub, cid: &str, req: u64, text: impl Into<String>) {
    hub.send(cid, &ServerMsg::Error { req, text: text.into() });
}

fn ok_or_err(hub: &Hub, cid: &str, r: Result<(), String>) {
    if let Err(e) = r {
        err(hub, cid, 0, e);
    }
}

/// Handle one client message. Slow operations are spawned so the reader keeps going.
pub async fn dispatch(hub: &Arc<Hub>, cid: &str, msg: ClientMsg) {
    let open = |id: &str| hub.session(id).ok_or_else(|| format!("Session {id} is not open"));
    match msg {
        ClientMsg::Hello { cwd, .. } => {
            hub.set_client_cwd(cid, cwd.clone());
            hub.send(cid, &ServerMsg::Catalog { catalog: hub.catalog(&cwd) });
        }
        ClientMsg::CreateSession { req, cwd, agent, model, variant, name } => {
            match hub.create_session(CreateOpts { cwd, agent, model, variant, name, ..Default::default() }) {
                Ok(core) => {
                    hub.set_attached(cid, &core.id, true);
                    core.send_snapshot(cid, req);
                }
                Err(e) => err(hub, cid, req, e),
            }
        }
        ClientMsg::Attach { req, session_id } => match hub.open_session(&session_id) {
            Ok(core) => {
                hub.set_attached(cid, &core.id, true);
                hub.set_watching(cid, &core.id, false);
                core.send_snapshot(cid, req);
            }
            Err(e) => err(hub, cid, req, e),
        },
        ClientMsg::Detach { session_id } => {
            hub.set_attached(cid, &session_id, false);
        }
        ClientMsg::Watch { session_id } => match open(&session_id) {
            Ok(core) => {
                hub.set_watching(cid, &session_id, true);
                core.send_snapshot(cid, 0);
            }
            Err(e) => err(hub, cid, 0, e),
        },
        ClientMsg::Unwatch { session_id } => hub.set_watching(cid, &session_id, false),
        ClientMsg::ListSessions { req } => {
            hub.subscribe_list(cid);
            hub.send(cid, &ServerMsg::SessionList { req, sessions: hub.session_list() });
        }
        ClientMsg::Prompt { session_id, text, images } => {
            if text.trim().is_empty() && images.is_empty() {
                return;
            }
            match hub.open_session(&session_id) {
                Ok(core) => core.prompt(text, images),
                Err(e) => err(hub, cid, 0, e),
            }
        }
        ClientMsg::Abort { session_id } => {
            if let Some(core) = hub.session(&session_id) {
                core.abort();
            }
        }
        ClientMsg::Permission { session_id, call_id, decision } => match open(&session_id) {
            Ok(core) => {
                if !core.permission(&call_id, decision) {
                    // Already resolved (e.g. by another client): make sure this one forgets it.
                    hub.send(cid, &ServerMsg::PermissionResolved { session_id, call_id });
                }
            }
            Err(e) => err(hub, cid, 0, e),
        },
        ClientMsg::SetModel { session_id, model, variant } => {
            ok_or_err(hub, cid, open(&session_id).and_then(|c| c.set_model(&model, variant)))
        }
        ClientMsg::SetVariant { session_id, variant } => {
            ok_or_err(hub, cid, open(&session_id).and_then(|c| c.set_variant(variant)))
        }
        ClientMsg::SetAgent { session_id, agent } => {
            ok_or_err(hub, cid, open(&session_id).and_then(|c| c.set_agent(&agent)))
        }
        ClientMsg::Rename { session_id, name } => {
            ok_or_err(hub, cid, open(&session_id).and_then(|c| c.rename(&name, true, true).map(|_| ())))
        }
        ClientMsg::Compact { session_id } => match open(&session_id) {
            Ok(core) => core.request_compaction(),
            Err(e) => err(hub, cid, 0, e),
        },
        ClientMsg::Undo { session_id } => match open(&session_id) {
            Ok(core) => {
                let (hub, cid) = (hub.clone(), cid.to_string());
                tokio::spawn(async move {
                    if let Err(e) = core.undo().await {
                        hub.send(&cid, &ServerMsg::Notice { session_id: Some(core.id.clone()), level: NoticeLevel::Warning, text: e });
                    }
                });
            }
            Err(e) => err(hub, cid, 0, e),
        },
        ClientMsg::Close { session_id } => {
            let (hub, cid) = (hub.clone(), cid.to_string());
            tokio::spawn(async move { ok_or_err(&hub, &cid, hub.close_session(&session_id).await) });
        }
        ClientMsg::Delete { session_id } => {
            let (hub, cid) = (hub.clone(), cid.to_string());
            tokio::spawn(async move { ok_or_err(&hub, &cid, hub.delete_session(&session_id).await) });
        }
        ClientMsg::Command { session_id, name, arguments } => {
            ok_or_err(hub, cid, run_command(hub, &session_id, &name, &arguments));
        }
        ClientMsg::Reload => {
            let hub = hub.clone();
            tokio::spawn(async move {
                hub.reload().await;
                hub.broadcast_all(&ServerMsg::Notice { session_id: None, level: NoticeLevel::Success, text: "Configuration reloaded".into() });
            });
        }
        ClientMsg::Shutdown => {
            tracing::info!("shutdown requested by client {cid}");
            hub.shutdown.cancel();
        }
        ClientMsg::Ping { req } => hub.send(cid, &ServerMsg::Pong { req }),
    }
}

fn run_command(hub: &Arc<Hub>, session_id: &str, name: &str, arguments: &str) -> Result<(), String> {
    let core = hub.open_session(session_id)?;
    let cfg = hub.config(&core.cwd()).map_err(|e| format!("{e:#}"))?;
    let name = name.trim_start_matches('/');
    let cmd = cfg.commands.get(name).ok_or_else(|| format!("Unknown command /{name}"))?;
    if let Some(agent) = cmd.agent.as_deref().filter(|a| !a.is_empty()) {
        if core.st.lock().info.agent != agent {
            core.set_agent(agent)?;
        }
    }
    if let Some(model) = cmd.model.as_deref().filter(|m| !m.is_empty()) {
        core.set_model(model, None)?;
    }
    let text = util::expand_template(&cmd.template, arguments);
    if text.trim().is_empty() {
        return Err(format!("Command /{name} expanded to an empty prompt"));
    }
    core.prompt(text, vec![]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_timeout_semantics() {
        let s = Duration::from_secs;
        // -1: never.
        assert!(!idle_expired(-1, true, s(100_000), s(100_000)));
        assert!(!idle_expired(-1, false, s(100_000), s(100_000)));
        // 0: right after the last frontend left; before any connection only after the grace.
        assert!(idle_expired(0, true, Duration::ZERO, s(1)));
        assert!(!idle_expired(0, false, s(5), s(5)));
        assert!(idle_expired(0, false, s(31), FIRST_CONNECT_GRACE));
        // N: N seconds after the last frontend left; never before the first connection.
        assert!(!idle_expired(10, true, s(9), s(100)));
        assert!(idle_expired(10, true, s(10), s(100)));
        assert!(!idle_expired(10, false, s(1000), s(1000)));
    }

    #[tokio::test]
    async fn host_access() {
        let none = resolve_hosts(&[]).await;
        assert!(none.allows("127.0.0.1".parse().unwrap()));
        assert!(none.allows("::1".parse().unwrap()));
        assert!(none.allows("::ffff:127.0.0.1".parse().unwrap()));
        assert!(!none.allows("192.168.1.5".parse().unwrap()));
        let some = resolve_hosts(&["192.168.1.5".into(), "[fe80::1]".into()]).await;
        assert!(some.allows("192.168.1.5".parse().unwrap()));
        assert!(some.allows("::ffff:192.168.1.5".parse().unwrap()));
        assert!(some.allows("fe80::1".parse().unwrap()));
        assert!(!some.allows("192.168.1.6".parse().unwrap()));
        let all = resolve_hosts(&["*".into()]).await;
        assert!(all.allows("8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn bind_defaults() {
        assert_eq!(default_bind(&[]), "127.0.0.1");
        assert_eq!(default_bind(&["localhost".into(), "127.0.0.1".into(), "[::1]".into()]), "127.0.0.1");
        assert_eq!(default_bind(&["192.168.1.2".into()]), "0.0.0.0");
        assert_eq!(default_bind(&["*".into()]), "0.0.0.0");
        assert_eq!(default_bind(&["myhost".into()]), "0.0.0.0");
    }
}
