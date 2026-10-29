//! WebSocket endpoint (`/` and `/ws`), host allow-list, and request dispatch.

use crate::commands;
use crate::hub::Hub;
use crate::session::Input;
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use futures::{SinkExt, StreamExt};
use hn_proto::*;
use serde_json::{json, Value};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct AppState {
    pub hub: Arc<Hub>,
    pub allow: Arc<AllowList>,
}

/// Allowed client addresses.
pub struct AllowList {
    any: bool,
    ips: Vec<IpAddr>,
}

impl AllowList {
    pub fn new(hosts: &[String]) -> AllowList {
        let mut any = false;
        let mut ips = Vec::new();
        for h in hosts {
            let h = h.trim().trim_start_matches('[').trim_end_matches(']');
            if h == "*" {
                any = true;
                continue;
            }
            if h.eq_ignore_ascii_case("localhost") {
                ips.push(IpAddr::from([127, 0, 0, 1]));
                ips.push(IpAddr::from(std::net::Ipv6Addr::LOCALHOST));
                continue;
            }
            match h.parse::<IpAddr>() {
                Ok(ip) => ips.push(ip),
                Err(_) => {
                    // Resolve host names once at startup.
                    match std::net::ToSocketAddrs::to_socket_addrs(&(h, 0)) {
                        Ok(addrs) => ips.extend(addrs.map(|a| a.ip())),
                        Err(e) => tracing::warn!("--allow-host {h}: cannot resolve: {e}"),
                    }
                }
            }
        }
        AllowList { any, ips }
    }

    pub fn allows(&self, ip: IpAddr) -> bool {
        if self.any {
            return true;
        }
        let ip = match ip {
            IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(ip),
            v4 => v4,
        };
        self.ips.contains(&ip)
    }
}

pub fn router(state: AppState) -> Router {
    Router::new().route("/", get(ws_handler)).route("/ws", get(ws_handler)).with_state(state)
}

async fn ws_handler(
    ws: Result<WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
) -> Response {
    if !state.allow.allows(addr.ip()) {
        tracing::warn!("rejected connection from {addr}");
        return (StatusCode::FORBIDDEN, "client address not allowed").into_response();
    }
    match ws {
        Ok(ws) => ws.max_message_size(256 << 20).on_upgrade(move |socket| client_loop(socket, state.hub)).into_response(),
        Err(_) => (StatusCode::OK, format!("hernees-server {} (WebSocket endpoint)\n", env!("CARGO_PKG_VERSION"))).into_response(),
    }
}

async fn client_loop(socket: WebSocket, hub: Arc<Hub>) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerMsg>();
    let cid = hub.add_client(tx);
    tracing::info!("client {cid} connected");
    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let Ok(text) = serde_json::to_string(&msg) else { continue };
            if sink.send(WsMessage::Text(text.into())).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });
    while let Some(frame) = stream.next().await {
        let text = match frame {
            Ok(WsMessage::Text(t)) => t.to_string(),
            Ok(WsMessage::Binary(b)) => String::from_utf8_lossy(&b).into_owned(),
            Ok(WsMessage::Close(_)) | Err(_) => break,
            Ok(_) => continue,
        };
        match serde_json::from_str::<Request>(&text) {
            Ok(req) => {
                // Handle concurrently so slow requests (attach from disk, undo) don't block.
                let hub = hub.clone();
                tokio::spawn(async move {
                    if let Some(resp) = handle(&hub, cid, req).await {
                        hub.send_to(cid, resp);
                    }
                });
            }
            Err(e) => {
                let id = serde_json::from_str::<Value>(&text).ok().and_then(|v| v.get("id").and_then(|i| i.as_u64())).unwrap_or(0);
                hub.send_to(cid, ServerMsg::err(id, format!("invalid request: {e}")));
            }
        }
    }
    hub.remove_client(cid);
    writer.abort();
    tracing::info!("client {cid} disconnected ({} frontend(s) left)", hub.client_count());
}

fn ok(id: u64, v: impl serde::Serialize) -> Option<ServerMsg> {
    Some(ServerMsg::ok(id, v))
}

fn err(id: u64, e: impl std::fmt::Display) -> Option<ServerMsg> {
    Some(ServerMsg::err(id, e))
}

/// Handle a request. Returns the response, or None if it was already sent (Attach/Create).
pub async fn handle(hub: &Arc<Hub>, cid: u64, req: Request) -> Option<ServerMsg> {
    let id = req.id;
    match req.msg {
        ClientMsg::Hello { client, cwd, protocol } => {
            if protocol != PROTOCOL_VERSION {
                return err(id, format!("protocol mismatch: server {PROTOCOL_VERSION}, client {protocol}"));
            }
            hub.hello(cid, &cwd, client != "status");
            tracing::info!("client {cid}: hello from {client} in {cwd}");
            ok(
                id,
                json!({
                    "version": env!("CARGO_PKG_VERSION"),
                    "protocol": PROTOCOL_VERSION,
                    "pid": std::process::id(),
                    "cwd": hub.server_cwd,
                    "clients": hub.client_count(),
                }),
            )
        }
        ClientMsg::Ping => ok(id, "pong"),
        ClientMsg::ListSessions { scope } => match scope {
            ListScope::Open => ok(id, hub.open_list()),
            ListScope::History => {
                let h = hub.clone();
                let list = tokio::task::spawn_blocking(move || h.history_list()).await.unwrap_or_default();
                ok(id, list)
            }
        },
        ClientMsg::CreateSession { cwd, agent, model, name } => {
            let cwd = if cwd.trim().is_empty() { hub.client_cwd(cid) } else { cwd };
            match hub.create_session(&cwd, agent, model, name, None, false).await {
                Ok(s) => match s.attach(hub, cid, true, id) {
                    Ok(()) => None,
                    Err(e) => err(id, e),
                },
                Err(e) => err(id, format!("{e:#}")),
            }
        }
        ClientMsg::Attach { session, foreground } => match hub.open_session(&session).await {
            Ok(s) => match s.attach(hub, cid, foreground, id) {
                Ok(()) => None,
                Err(e) => err(id, e),
            },
            Err(e) => err(id, format!("{e:#}")),
        },
        ClientMsg::Detach { session } => {
            match hub.get(&session) {
                Some(s) => s.detach(hub, cid),
                None => hub.detach_client(cid, &session),
            }
            ok(id, Value::Null)
        }
        ClientMsg::WatchBackground { enabled } => {
            hub.set_watch(cid, enabled);
            ok(id, Value::Null)
        }
        ClientMsg::Prompt { session, text, images } => {
            if text.trim().is_empty() && images.is_empty() {
                return err(id, "empty prompt");
            }
            match hub.open_session(&session).await {
                Ok(s) => match s.enqueue(Input::Human { text, images }) {
                    Ok(idle) => ok(id, json!({"queued": !idle})),
                    Err(e) => err(id, e),
                },
                Err(e) => err(id, format!("{e:#}")),
            }
        }
        ClientMsg::Command { session, name, args } => {
            let s = match hub.open_session(&session).await {
                Ok(s) => s,
                Err(e) => return err(id, format!("{e:#}")),
            };
            let cwd = s.info().cwd;
            let config = hub.config(&cwd).await;
            let name = name.trim_start_matches('/');
            let text = if let Some(c) = config.commands.get(name) {
                if let Some(agent) = c.agent.as_deref().filter(|a| config.agent(a).is_some()) {
                    if agent != s.info().agent {
                        let model = config.default_model(agent);
                        let limits = model.as_ref().and_then(|m| limits(&config, m));
                        s.set_agent(agent, model, limits);
                    }
                }
                if let Some(m) = c.model.as_deref().and_then(ModelRef::parse) {
                    if config.resolve_model(&m).is_ok() {
                        let l = limits(&config, &m);
                        s.set_model(m, l);
                    }
                }
                commands::expand_template(&c.template, &args)
            } else if let Some(sk) = config.skill(name) {
                let sk = sk.clone();
                let content = tokio::task::spawn_blocking(move || hn_config::skill_content(&sk)).await.unwrap_or_default();
                if args.trim().is_empty() {
                    content
                } else {
                    format!("{content}\n\n{}", args.trim())
                }
            } else {
                return err(id, format!("unknown command /{name}"));
            };
            match s.enqueue(Input::Human { text, images: Vec::new() }) {
                Ok(idle) => ok(id, json!({"queued": !idle})),
                Err(e) => err(id, e),
            }
        }
        ClientMsg::Abort { session } => match hub.get(&session) {
            Some(s) => ok(id, json!({"aborted": s.abort()})),
            None => err(id, format!("session {session} is not open")),
        },
        ClientMsg::Permission { session, call_id, decision } => match hub.get(&session) {
            Some(s) => match s.permission(&call_id, decision) {
                Ok(()) => ok(id, Value::Null),
                Err(e) => err(id, e),
            },
            None => err(id, format!("session {session} is not open")),
        },
        ClientMsg::SetModel { session, model } => {
            let s = match hub.open_session(&session).await {
                Ok(s) => s,
                Err(e) => return err(id, format!("{e:#}")),
            };
            let config = hub.config(&s.info().cwd).await;
            if let Err(e) = config.resolve_model(&model) {
                return err(id, format!("{e:#}"));
            }
            let l = limits(&config, &model);
            s.set_model(model, l);
            ok(id, s.info())
        }
        ClientMsg::SetAgent { session, agent } => {
            let s = match hub.open_session(&session).await {
                Ok(s) => s,
                Err(e) => return err(id, format!("{e:#}")),
            };
            let config = hub.config(&s.info().cwd).await;
            match config.agent(&agent) {
                Some(a) if !a.disable.unwrap_or(false) => {}
                _ => return err(id, format!("unknown agent {agent:?}")),
            }
            let model = config.default_model(&agent);
            let l = model.as_ref().and_then(|m| limits(&config, m));
            s.set_agent(&agent, model, l);
            ok(id, s.info())
        }
        ClientMsg::Rename { session, name } => {
            let name = name.trim().to_string();
            if name.is_empty() {
                return err(id, "name must not be empty");
            }
            match hub.open_session(&session).await {
                Ok(s) => {
                    s.rename(&name);
                    ok(id, s.info())
                }
                Err(e) => err(id, format!("{e:#}")),
            }
        }
        ClientMsg::Compact { session } => match hub.open_session(&session).await {
            Ok(s) => match s.request_compact() {
                Ok(idle) => ok(id, json!({"queued": !idle})),
                Err(e) => err(id, e),
            },
            Err(e) => err(id, format!("{e:#}")),
        },
        ClientMsg::Undo { session } => match hub.open_session(&session).await {
            Ok(s) => match s.undo().await {
                Ok(text) => ok(id, text),
                Err(e) => err(id, e),
            },
            Err(e) => err(id, format!("{e:#}")),
        },
        ClientMsg::Close { session } => {
            match hub.get(&session) {
                Some(s) => s.close(hub).await,
                None => {
                    if crate::persist::load_info(&session).is_err() {
                        return err(id, format!("unknown session {session}"));
                    }
                }
            }
            ok(id, Value::Null)
        }
        ClientMsg::Delete { session } => {
            if let Some(s) = hub.get(&session) {
                s.close(hub).await;
            }
            let sid = session.clone();
            match tokio::task::spawn_blocking(move || crate::persist::delete(&sid)).await {
                Ok(Ok(())) => {
                    hub.touch();
                    ok(id, Value::Null)
                }
                Ok(Err(e)) => err(id, format!("{e:#}")),
                Err(e) => err(id, e),
            }
        }
        ClientMsg::ListModels => {
            let config = hub.config(&hub.client_cwd(cid)).await;
            ok(id, config.models())
        }
        ClientMsg::ListAgents => {
            let config = hub.config(&hub.client_cwd(cid)).await;
            ok(id, config.agent_infos())
        }
        ClientMsg::ListCommands { session } => {
            let cwd = hub.get(&session).map(|s| s.info().cwd).unwrap_or_else(|| hub.client_cwd(cid));
            let config = hub.config(&cwd).await;
            ok(id, config.command_infos())
        }
        ClientMsg::Export { session } => {
            if let Some(s) = hub.get(&session) {
                let (info, msgs) = {
                    let st = s.lock();
                    (st.info.clone(), st.msgs.clone())
                };
                return ok(id, commands::export_markdown(&info, &msgs));
            }
            let sid = session.clone();
            match tokio::task::spawn_blocking(move || crate::persist::load(&sid)).await {
                Ok(Ok((info, msgs))) => ok(id, commands::export_markdown(&info, &msgs)),
                Ok(Err(e)) => err(id, format!("{e:#}")),
                Err(e) => err(id, e),
            }
        }
    }
}

fn limits(config: &hn_config::Config, m: &ModelRef) -> Option<(u64, u64)> {
    config.resolve_model(m).ok().map(|spec| config.context_limits(&spec, m))
}
