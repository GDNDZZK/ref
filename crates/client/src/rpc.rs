//! Protocol v6 RPC client with clean runtime reconnect and backend hot-switching.

use std::collections::HashMap;
use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use ref_proto::{
    ClientFrame, ERR_PROTOCOL_MISMATCH, Message, NotifyKind, PROTOCOL_VERSION, Request,
    ServerFrame, SessionSnapshot, StashGroup,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message as WsMessage;

use crate::{
    BackendTarget, ConnectError, ConnectedBackend, Endpoint, MockBackend, Ws, discover_or_connect,
};

const RECONNECT_DELAY: Duration = Duration::from_secs(1);
const HELLO_ID: u64 = u64::MAX;

/// A page of messages returned by `Request::Messages` (Protocol v6).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagesPage {
    pub messages: Vec<Message>,
    #[serde(default)]
    pub more_before: bool,
    #[serde(default)]
    pub start_seq: u64,
    #[serde(default)]
    pub end_seq: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("{0}")]
    Connect(#[from] ConnectError),
    #[error("websocket error: {0}")]
    WebSocket(String),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("protocol mismatch: {0}")]
    ProtocolMismatch(String),
    #[error("{message}")]
    Rpc {
        message: String,
        code: Option<String>,
    },
    #[error("disconnected from backend")]
    Disconnected,
    #[error("client shut down")]
    Closed,
}

#[derive(Debug)]
pub enum ClientEvent {
    Connected { protocol: u32, endpoint: Endpoint },
    BackendTooOld { protocol: u32 },
    Disconnected(String),
    Refused(String),
    Status(String),
    Warn(String),
    Frame(Box<ServerFrame>),
}

enum Cmd {
    Request {
        req: Request,
        reply: oneshot::Sender<Result<Value, ClientError>>,
    },
    Notify(Request),
    SwitchTarget(BackendTarget),
    Reconnect,
}

enum PumpExit {
    Stop,
    Lost(String),
    Fatal(String),
    Switch(BackendTarget),
    Reconnect,
}

enum ConnectOutcome {
    Connected(Result<ConnectedBackend, ConnectError>),
    Switch(BackendTarget),
    Stop,
}

/// Async Protocol v6 client supporting typed RPCs and runtime backend hot-switching.
#[derive(Clone)]
pub struct RefClient {
    tx: mpsc::UnboundedSender<Cmd>,
}

impl RefClient {
    /// Spawns the transport task targeting `target` and returns `(client, events)`.
    pub fn connect(
        target: BackendTarget,
        client_name: impl Into<String>,
        cwd: impl Into<String>,
        notify: Vec<NotifyKind>,
    ) -> (Self, mpsc::UnboundedReceiver<ClientEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let (ev_tx, ev_rx) = mpsc::unbounded_channel();
        tokio::spawn(run_driver(
            target,
            client_name.into(),
            cwd.into(),
            notify,
            rx,
            ev_tx,
        ));
        (Self { tx }, ev_rx)
    }

    /// Cleanly disconnects from the current backend (if any), drops in-flight RPCs, and connects
    /// to `target` with a fresh `Hello` handshake.
    pub fn switch_target(&self, target: BackendTarget) -> Result<(), ClientError> {
        self.tx
            .send(Cmd::SwitchTarget(target))
            .map_err(|_| ClientError::Closed)
    }

    /// Tears down the current connection and reconnects to the active target.
    pub fn reconnect(&self) -> Result<(), ClientError> {
        self.tx
            .send(Cmd::Reconnect)
            .map_err(|_| ClientError::Closed)
    }

    /// Sends a fire-and-forget request.
    pub fn notify(&self, req: Request) -> Result<(), ClientError> {
        self.tx
            .send(Cmd::Notify(req))
            .map_err(|_| ClientError::Closed)
    }

    /// Sends a request and awaits its response payload.
    pub async fn request(&self, req: Request) -> Result<Value, ClientError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(Cmd::Request {
                req,
                reply: reply_tx,
            })
            .map_err(|_| ClientError::Closed)?;
        reply_rx.await.map_err(|_| ClientError::Closed)?
    }

    pub async fn attach(
        &self,
        session_id: &str,
        foreground: bool,
        limit: Option<u32>,
    ) -> Result<SessionSnapshot, ClientError> {
        let res = self
            .request(Request::Attach {
                session_id: session_id.to_string(),
                foreground,
                limit,
            })
            .await?;
        parse_field(&res, "snapshot")
    }

    pub async fn messages_before(
        &self,
        session_id: &str,
        before: &str,
        limit: Option<u32>,
    ) -> Result<MessagesPage, ClientError> {
        let res = self
            .request(Request::Messages {
                session_id: session_id.to_string(),
                before: Some(before.to_string()),
                start_seq: None,
                end_seq: None,
                limit,
            })
            .await?;
        Ok(serde_json::from_value(res)?)
    }

    pub async fn messages_range(
        &self,
        session_id: &str,
        start_seq: u64,
        end_seq: u64,
        limit: Option<u32>,
    ) -> Result<MessagesPage, ClientError> {
        let res = self
            .request(Request::Messages {
                session_id: session_id.to_string(),
                before: None,
                start_seq: Some(start_seq),
                end_seq: Some(end_seq),
                limit,
            })
            .await?;
        Ok(serde_json::from_value(res)?)
    }

    pub async fn retry_now(&self, session_id: &str) -> Result<bool, ClientError> {
        let res = self
            .request(Request::RetryNow {
                session_id: session_id.to_string(),
            })
            .await?;
        parse_field(&res, "retried")
    }

    pub async fn list_groups(&self) -> Result<Vec<StashGroup>, ClientError> {
        let res = self.request(Request::ListGroups).await?;
        parse_field(&res, "groups")
    }

    pub async fn stash_all(
        &self,
        name: Option<String>,
    ) -> Result<(Option<StashGroup>, Vec<StashGroup>), ClientError> {
        let res = self.request(Request::StashAll { name }).await?;
        let group = match res.get("group") {
            Some(Value::Null) | None => None,
            Some(v) => Some(serde_json::from_value(v.clone())?),
        };
        let groups = parse_field(&res, "groups")?;
        Ok((group, groups))
    }

    pub async fn create_group(
        &self,
        name: Option<String>,
        session_ids: Vec<String>,
        stash_live: bool,
    ) -> Result<(StashGroup, Vec<StashGroup>), ClientError> {
        let res = self
            .request(Request::CreateGroup {
                name,
                session_ids,
                stash_live,
            })
            .await?;
        Ok((parse_field(&res, "group")?, parse_field(&res, "groups")?))
    }

    pub async fn rename_group(
        &self,
        group_id: &str,
        name: &str,
    ) -> Result<Vec<StashGroup>, ClientError> {
        let res = self
            .request(Request::RenameGroup {
                group_id: group_id.to_string(),
                name: name.to_string(),
            })
            .await?;
        parse_field(&res, "groups")
    }

    pub async fn set_group_locked(
        &self,
        group_id: &str,
        locked: bool,
    ) -> Result<Vec<StashGroup>, ClientError> {
        let res = self
            .request(Request::SetGroupLocked {
                group_id: group_id.to_string(),
                locked,
            })
            .await?;
        parse_field(&res, "groups")
    }

    pub async fn delete_group(&self, group_id: &str) -> Result<Vec<StashGroup>, ClientError> {
        let res = self
            .request(Request::DeleteGroup {
                group_id: group_id.to_string(),
            })
            .await?;
        parse_field(&res, "groups")
    }

    pub async fn add_to_group(
        &self,
        group_id: &str,
        session_id: &str,
        position: Option<u32>,
        stash_live: bool,
    ) -> Result<Vec<StashGroup>, ClientError> {
        let res = self
            .request(Request::AddToGroup {
                group_id: group_id.to_string(),
                session_id: session_id.to_string(),
                position,
                stash_live,
            })
            .await?;
        parse_field(&res, "groups")
    }

    pub async fn move_group_member(
        &self,
        from_group_id: Option<String>,
        to_group_id: &str,
        session_id: &str,
        position: u32,
        stash_live: bool,
    ) -> Result<Vec<StashGroup>, ClientError> {
        let res = self
            .request(Request::MoveGroupMember {
                from_group_id,
                to_group_id: to_group_id.to_string(),
                session_id: session_id.to_string(),
                position,
                stash_live,
            })
            .await?;
        parse_field(&res, "groups")
    }

    pub async fn remove_from_group(
        &self,
        group_id: &str,
        session_id: &str,
    ) -> Result<Vec<StashGroup>, ClientError> {
        let res = self
            .request(Request::RemoveFromGroup {
                group_id: group_id.to_string(),
                session_id: session_id.to_string(),
            })
            .await?;
        parse_field(&res, "groups")
    }

    pub async fn merge_groups(
        &self,
        source_group_id: &str,
        target_group_id: &str,
    ) -> Result<Vec<StashGroup>, ClientError> {
        let res = self
            .request(Request::MergeGroups {
                source_group_id: source_group_id.to_string(),
                target_group_id: target_group_id.to_string(),
            })
            .await?;
        parse_field(&res, "groups")
    }

    pub async fn restore_group(
        &self,
        group_id: &str,
    ) -> Result<(Vec<String>, Vec<StashGroup>), ClientError> {
        let res = self
            .request(Request::RestoreGroup {
                group_id: group_id.to_string(),
            })
            .await?;
        Ok((
            parse_field(&res, "restored_ids")?,
            parse_field(&res, "groups")?,
        ))
    }

    pub async fn restore_group_member(
        &self,
        group_id: &str,
        session_id: &str,
    ) -> Result<(bool, Vec<StashGroup>), ClientError> {
        let res = self
            .request(Request::RestoreGroupMember {
                group_id: group_id.to_string(),
                session_id: session_id.to_string(),
            })
            .await?;
        Ok((parse_field(&res, "restored")?, parse_field(&res, "groups")?))
    }
}

fn parse_field<T: serde::de::DeserializeOwned>(v: &Value, key: &str) -> Result<T, ClientError> {
    let field = v.get(key).ok_or_else(|| ClientError::Rpc {
        message: format!("missing `{key}` in response"),
        code: None,
    })?;
    Ok(serde_json::from_value(field.clone())?)
}

fn fail_queued_commands(
    rx: &mut mpsc::UnboundedReceiver<Cmd>,
    pending_switch: &mut Option<BackendTarget>,
) {
    while let Ok(cmd) = rx.try_recv() {
        match cmd {
            Cmd::Request { reply, .. } => {
                let _ = reply.send(Err(ClientError::Disconnected));
            }
            Cmd::Notify(_) => {}
            Cmd::SwitchTarget(next) => *pending_switch = Some(next),
            Cmd::Reconnect => {}
        }
    }
}

async fn shutdown_mock(mock: Option<MockBackend>) {
    if let Some(m) = mock
        && let Err(e) = m.shutdown().await
    {
        tracing::debug!("failed to shut down mock backend: {e}");
    }
}

async fn pause_or_switch(
    rx: &mut mpsc::UnboundedReceiver<Cmd>,
    target: &mut BackendTarget,
    delay: Duration,
) -> bool {
    let sleep = tokio::time::sleep(delay);
    tokio::pin!(sleep);
    loop {
        tokio::select! {
            _ = &mut sleep => return true,
            cmd = rx.recv() => match cmd {
                Some(Cmd::Request { reply, .. }) => {
                    let _ = reply.send(Err(ClientError::Disconnected));
                }
                Some(Cmd::Notify(_)) => {}
                Some(Cmd::SwitchTarget(next)) => {
                    *target = next;
                    return true;
                }
                Some(Cmd::Reconnect) => return true,
                None => return false,
            }
        }
    }
}

async fn run_driver(
    mut target: BackendTarget,
    client_name: String,
    cwd: String,
    notify: Vec<NotifyKind>,
    mut rx: mpsc::UnboundedReceiver<Cmd>,
    events: mpsc::UnboundedSender<ClientEvent>,
) {
    loop {
        let outcome = {
            let conn_fut = discover_or_connect(&target, || {
                let _ = events.send(ClientEvent::Status("starting backend…".into()));
            });
            tokio::pin!(conn_fut);
            loop {
                tokio::select! {
                    res = &mut conn_fut => break ConnectOutcome::Connected(res),
                    cmd = rx.recv() => match cmd {
                        Some(Cmd::Request { reply, .. }) => {
                            let _ = reply.send(Err(ClientError::Disconnected));
                        }
                        Some(Cmd::Notify(_)) | Some(Cmd::Reconnect) => {}
                        Some(Cmd::SwitchTarget(next)) => break ConnectOutcome::Switch(next),
                        None => break ConnectOutcome::Stop,
                    }
                }
            }
        };
        let connected = match outcome {
            ConnectOutcome::Stop => return,
            ConnectOutcome::Switch(next) => {
                target = next;
                continue;
            }
            ConnectOutcome::Connected(Ok(c)) => c,
            ConnectOutcome::Connected(Err(e)) => {
                if events
                    .send(ClientEvent::Disconnected(e.to_string()))
                    .is_err()
                {
                    return;
                }
                let mut pending_switch = None;
                fail_queued_commands(&mut rx, &mut pending_switch);
                if let Some(next) = pending_switch {
                    target = next;
                    continue;
                }
                if !pause_or_switch(&mut rx, &mut target, RECONNECT_DELAY).await {
                    return;
                }
                if events.is_closed() {
                    return;
                }
                continue;
            }
        };

        let mut ws = connected.ws;
        let ep = connected.endpoint;
        let mock = connected.mock;

        let hello = ClientFrame {
            id: Some(HELLO_ID),
            req: Request::Hello {
                client: client_name.clone(),
                version: PROTOCOL_VERSION,
                cwd: cwd.clone(),
                token: ep.token.clone(),
                notify: notify.clone(),
            },
        };
        let hello_txt = match serde_json::to_string(&hello) {
            Ok(t) => t,
            Err(e) => {
                let _ = events.send(ClientEvent::Disconnected(format!(
                    "cannot encode hello: {e}"
                )));
                shutdown_mock(mock).await;
                return;
            }
        };
        if let Err(e) = ws.send(WsMessage::text(hello_txt)).await {
            let _ = ws.close(None).await;
            shutdown_mock(mock).await;
            if events
                .send(ClientEvent::Disconnected(e.to_string()))
                .is_err()
            {
                return;
            }
            if !pause_or_switch(&mut rx, &mut target, RECONNECT_DELAY).await {
                return;
            }
            continue;
        }

        let mut pending: HashMap<u64, oneshot::Sender<Result<Value, ClientError>>> = HashMap::new();
        let exit = pump(&mut ws, &ep, &mut rx, &events, &mut pending).await;
        for (_, tx) in pending.drain() {
            let _ = tx.send(Err(ClientError::Disconnected));
        }
        let _ = ws.close(None).await;
        shutdown_mock(mock).await;

        match exit {
            PumpExit::Stop => return,
            PumpExit::Fatal(reason) => {
                if events.send(ClientEvent::Refused(reason)).is_err() {
                    return;
                }
                loop {
                    match rx.recv().await {
                        Some(Cmd::Request { reply, .. }) => {
                            let _ = reply.send(Err(ClientError::Disconnected));
                        }
                        Some(Cmd::Notify(_)) => {}
                        Some(Cmd::SwitchTarget(next)) => {
                            target = next;
                            break;
                        }
                        Some(Cmd::Reconnect) => break,
                        None => return,
                    }
                }
            }
            PumpExit::Switch(next) => {
                target = next;
                let _ = events.send(ClientEvent::Disconnected("switching backend".into()));
            }
            PumpExit::Reconnect => {
                let _ = events.send(ClientEvent::Disconnected("reconnecting".into()));
            }
            PumpExit::Lost(reason) => {
                if events.send(ClientEvent::Disconnected(reason)).is_err() {
                    return;
                }
                let mut pending_switch = None;
                fail_queued_commands(&mut rx, &mut pending_switch);
                if let Some(next) = pending_switch {
                    target = next;
                    continue;
                }
                if !pause_or_switch(&mut rx, &mut target, RECONNECT_DELAY).await {
                    return;
                }
            }
        }
    }
}

async fn pump(
    ws: &mut Ws,
    ep: &Endpoint,
    rx: &mut mpsc::UnboundedReceiver<Cmd>,
    events: &mpsc::UnboundedSender<ClientEvent>,
    pending: &mut HashMap<u64, oneshot::Sender<Result<Value, ClientError>>>,
) -> PumpExit {
    let mut next_id: u64 = 1;
    loop {
        tokio::select! {
            cmd = rx.recv() => {
                let Some(cmd) = cmd else {
                    return PumpExit::Stop;
                };
                match cmd {
                    Cmd::SwitchTarget(next) => return PumpExit::Switch(next),
                    Cmd::Reconnect => return PumpExit::Reconnect,
                    Cmd::Notify(req) => {
                        let frame = ClientFrame { id: None, req };
                        let Ok(txt) = serde_json::to_string(&frame) else {
                            continue;
                        };
                        if let Err(e) = ws.send(WsMessage::text(txt)).await {
                            return PumpExit::Lost(e.to_string());
                        }
                    }
                    Cmd::Request { req, reply } => {
                        if next_id == HELLO_ID {
                            next_id = 1;
                        }
                        let id = next_id;
                        next_id = next_id.wrapping_add(1).max(1);
                        let frame = ClientFrame { id: Some(id), req };
                        let txt = match serde_json::to_string(&frame) {
                            Ok(t) => t,
                            Err(e) => {
                                let _ = reply.send(Err(ClientError::Json(e)));
                                continue;
                            }
                        };
                        pending.insert(id, reply);
                        if let Err(e) = ws.send(WsMessage::text(txt)).await {
                            if let Some(tx) = pending.remove(&id) {
                                let _ = tx.send(Err(ClientError::WebSocket(e.to_string())));
                            }
                            return PumpExit::Lost(e.to_string());
                        }
                    }
                }
            }
            msg = ws.next() => {
                match msg {
                    Some(Ok(WsMessage::Text(txt))) => {
                        if let Some(exit) = handle_text_frame(txt.as_str(), ep, events, pending) {
                            return exit;
                        }
                    }
                    Some(Ok(WsMessage::Ping(p))) => {
                        let _ = ws.send(WsMessage::Pong(p)).await;
                    }
                    Some(Ok(WsMessage::Close(_))) | None => {
                        return PumpExit::Lost("connection closed".into());
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => return PumpExit::Lost(e.to_string()),
                }
            }
        }
    }
}

fn handle_text_frame(
    txt: &str,
    ep: &Endpoint,
    events: &mpsc::UnboundedSender<ClientEvent>,
    pending: &mut HashMap<u64, oneshot::Sender<Result<Value, ClientError>>>,
) -> Option<PumpExit> {
    let raw: Value = match serde_json::from_str(txt) {
        Ok(v) => v,
        Err(e) => {
            if events
                .send(ClientEvent::Warn(format!("invalid server frame: {e}")))
                .is_err()
            {
                return Some(PumpExit::Stop);
            }
            return None;
        }
    };
    let frame: ServerFrame = match serde_json::from_value(raw) {
        Ok(f) => f,
        Err(e) => {
            if e.to_string().contains("unknown variant") {
                return None;
            }
            if events
                .send(ClientEvent::Warn(format!("malformed server frame: {e}")))
                .is_err()
            {
                return Some(PumpExit::Stop);
            }
            return None;
        }
    };
    if let ServerFrame::Response {
        id: HELLO_ID,
        ok,
        result,
        error,
        code,
    } = &frame
    {
        if *ok {
            let protocol = result
                .get("protocol")
                .and_then(|v| v.as_u64())
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(1);
            if events
                .send(ClientEvent::Connected {
                    protocol,
                    endpoint: ep.clone(),
                })
                .is_err()
            {
                return Some(PumpExit::Stop);
            }
            if protocol < PROTOCOL_VERSION
                && events
                    .send(ClientEvent::BackendTooOld { protocol })
                    .is_err()
            {
                return Some(PumpExit::Stop);
            }
            return None;
        }
        let reason = error.clone().unwrap_or_else(|| "hello rejected".into());
        if code.as_deref() == Some(ERR_PROTOCOL_MISMATCH) {
            return Some(PumpExit::Fatal(reason));
        }
        return Some(PumpExit::Lost(reason));
    }

    if let ServerFrame::Response {
        id,
        ok,
        result,
        error,
        code,
    } = &frame
        && let Some(tx) = pending.remove(id)
    {
        let res = if *ok {
            Ok(result.clone())
        } else {
            Err(ClientError::Rpc {
                message: error.clone().unwrap_or_else(|| "request failed".into()),
                code: code.clone(),
            })
        };
        let _ = tx.send(res);
        return None;
    }

    if events.send(ClientEvent::Frame(Box::new(frame))).is_err() {
        return Some(PumpExit::Stop);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use ref_proto::{
        MessageLayoutMeta, PartLayoutMeta, Role, SessionInfo, SessionStatus, StashMember,
    };
    use serde_json::json;
    use tokio::net::TcpListener;

    async fn mock_ws_server(
        handler: impl Fn(ClientFrame) -> Vec<ServerFrame> + Send + Sync + 'static,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handler = std::sync::Arc::new(handler);
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let h = handler.clone();
                tokio::spawn(async move {
                    let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
                    while let Some(Ok(WsMessage::Text(txt))) = ws.next().await {
                        let req: ClientFrame = serde_json::from_str(txt.as_str()).unwrap();
                        for resp in h(req) {
                            let s = serde_json::to_string(&resp).unwrap();
                            if ws.send(WsMessage::text(s)).await.is_err() {
                                return;
                            }
                        }
                    }
                });
            }
        });
        (format!("ws://{addr}/"), task)
    }

    fn sample_info(id: &str) -> SessionInfo {
        SessionInfo {
            id: id.into(),
            name: id.into(),
            title: "Test".into(),
            agent: "build".into(),
            model: Default::default(),
            cwd: "/tmp".into(),
            created_at: 1,
            updated_at: 2,
            status: SessionStatus::Idle,
            attached: 0,
            live: true,
            parent: None,
            must_report: false,
            context: Default::default(),
            todos: Vec::new(),
            busy_since: None,
            last_error: None,
        }
    }

    fn sample_group(id: &str, name: &str) -> StashGroup {
        StashGroup {
            id: id.into(),
            name: name.into(),
            locked: false,
            created_at: 10,
            updated_at: 20,
            members: vec![StashMember {
                session_id: "ses_1".into(),
                position: 0,
                info: sample_info("ses_1"),
            }],
        }
    }

    #[tokio::test]
    async fn rpc_v6_messages_range_retry_now_and_stash_methods() {
        let (url, server) = mock_ws_server(|frame| {
            let id = frame.id.unwrap_or(0);
            let ok = |result: Value| ServerFrame::Response {
                id,
                ok: true,
                result,
                error: None,
                code: None,
            };
            match frame.req {
                Request::Hello { version, .. } => {
                    assert_eq!(version, PROTOCOL_VERSION);
                    vec![ok(json!({ "protocol": PROTOCOL_VERSION }))]
                }
                Request::Attach { .. } => vec![ok(json!({
                    "snapshot": SessionSnapshot {
                        info: sample_info("ses_1"),
                        messages: vec![],
                        more_before: true,
                        total_messages: 5,
                        start_seq: 5,
                        layout_metas: vec![MessageLayoutMeta {
                            seq: 0,
                            id: "m0".into(),
                            role: Role::User,
                            source_kind: "human".into(),
                            has_error: false,
                            parts: vec![PartLayoutMeta {
                                kind: "text".into(),
                                chars: 12,
                                lines: 1,
                                code_blocks: 0,
                                code_lines: 0,
                                tables: 0,
                                tool: None,
                                tool_lines: 0,
                                width: None,
                                height: None,
                            }],
                        }],
                        pending_permissions: vec![],
                        pending_questions: vec![],
                        queued: vec![],
                    }
                }))],
                Request::Messages {
                    start_seq, end_seq, ..
                } => vec![ok(json!({
                    "messages": [{
                        "id": "m0",
                        "role": "user",
                        "time": 1,
                        "source": { "kind": "human" },
                        "parts": [{ "type": "text", "text": "hi", "time": 1 }]
                    }],
                    "more_before": false,
                    "start_seq": start_seq.unwrap_or(0),
                    "end_seq": end_seq.unwrap_or(1),
                }))],
                Request::RetryNow { .. } => vec![ok(json!({ "retried": true }))],
                Request::ListGroups => vec![ok(json!({ "groups": [sample_group("g1", "Work")] }))],
                Request::StashAll { .. } => vec![ok(json!({
                    "group": sample_group("g1", "Stash"),
                    "groups": [sample_group("g1", "Stash")],
                }))],
                Request::CreateGroup { .. } => vec![ok(json!({
                    "group": sample_group("g2", "Created"),
                    "groups": [sample_group("g2", "Created")],
                }))],
                Request::RenameGroup { .. }
                | Request::SetGroupLocked { .. }
                | Request::DeleteGroup { .. }
                | Request::AddToGroup { .. }
                | Request::MoveGroupMember { .. }
                | Request::RemoveFromGroup { .. }
                | Request::MergeGroups { .. } => {
                    vec![ok(json!({ "groups": [sample_group("g1", "Updated")] }))]
                }
                Request::RestoreGroup { .. } => vec![ok(json!({
                    "restored_ids": ["ses_1"],
                    "groups": [],
                }))],
                Request::RestoreGroupMember { .. } => vec![ok(json!({
                    "restored": true,
                    "groups": [],
                }))],
                _ => vec![],
            }
        })
        .await;

        let (client, mut events) = RefClient::connect(
            BackendTarget::Explicit { url, token: None },
            "test-client",
            "/tmp",
            vec![],
        );
        let ev = events.recv().await.unwrap();
        assert!(matches!(ev, ClientEvent::Connected { protocol: 6, .. }));

        let snap = client.attach("ses_1", true, None).await.unwrap();
        assert_eq!(snap.total_messages, 5);
        assert_eq!(snap.start_seq, 5);
        assert_eq!(snap.layout_metas.len(), 1);

        let page = client
            .messages_range("ses_1", 0, 1, Some(100))
            .await
            .unwrap();
        assert_eq!(page.start_seq, 0);
        assert_eq!(page.end_seq, 1);
        assert_eq!(page.messages.len(), 1);

        assert!(client.retry_now("ses_1").await.unwrap());
        assert_eq!(client.list_groups().await.unwrap().len(), 1);
        assert!(
            client
                .stash_all(Some("S".into()))
                .await
                .unwrap()
                .0
                .is_some()
        );
        assert_eq!(
            client
                .create_group(Some("C".into()), vec!["ses_1".into()], true)
                .await
                .unwrap()
                .0
                .id,
            "g2"
        );
        assert_eq!(client.rename_group("g1", "R").await.unwrap().len(), 1);
        assert_eq!(client.set_group_locked("g1", true).await.unwrap().len(), 1);
        assert_eq!(client.delete_group("g1").await.unwrap().len(), 1);
        assert_eq!(
            client
                .add_to_group("g1", "ses_1", Some(0), true)
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            client
                .move_group_member(Some("g1".into()), "g2", "ses_1", 0, false)
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            client.remove_from_group("g1", "ses_1").await.unwrap().len(),
            1
        );
        assert_eq!(client.merge_groups("g1", "g2").await.unwrap().len(), 1);
        assert_eq!(
            client.restore_group("g1").await.unwrap().0,
            vec!["ses_1".to_string()]
        );
        assert!(client.restore_group_member("g1", "ses_1").await.unwrap().0);

        server.abort();
    }

    #[tokio::test]
    async fn runtime_switch_target_reconnects_to_new_server() {
        let (url1, s1) = mock_ws_server(|frame| {
            let id = frame.id.unwrap_or(0);
            match frame.req {
                Request::Hello { .. } => vec![ServerFrame::Response {
                    id,
                    ok: true,
                    result: json!({ "protocol": PROTOCOL_VERSION }),
                    error: None,
                    code: None,
                }],
                Request::Ping => vec![ServerFrame::Response {
                    id,
                    ok: true,
                    result: json!({ "server": 1 }),
                    error: None,
                    code: None,
                }],
                _ => vec![],
            }
        })
        .await;
        let (url2, s2) = mock_ws_server(|frame| {
            let id = frame.id.unwrap_or(0);
            match frame.req {
                Request::Hello { .. } => vec![ServerFrame::Response {
                    id,
                    ok: true,
                    result: json!({ "protocol": PROTOCOL_VERSION }),
                    error: None,
                    code: None,
                }],
                Request::Ping => vec![ServerFrame::Response {
                    id,
                    ok: true,
                    result: json!({ "server": 2 }),
                    error: None,
                    code: None,
                }],
                _ => vec![],
            }
        })
        .await;

        let (client, mut events) = RefClient::connect(
            BackendTarget::Explicit {
                url: url1,
                token: None,
            },
            "test-client",
            "/tmp",
            vec![],
        );
        assert!(matches!(
            events.recv().await.unwrap(),
            ClientEvent::Connected { .. }
        ));
        assert_eq!(client.request(Request::Ping).await.unwrap()["server"], 1);

        client
            .switch_target(BackendTarget::Explicit {
                url: url2.clone(),
                token: None,
            })
            .unwrap();
        while let Some(ev) = events.recv().await {
            if let ClientEvent::Connected { endpoint, .. } = ev {
                assert_eq!(endpoint.url, url2);
                break;
            }
        }
        assert_eq!(client.request(Request::Ping).await.unwrap()["server"], 2);

        s1.abort();
        s2.abort();
    }
}
