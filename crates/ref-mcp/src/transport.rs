//! Transports (stdio, Streamable HTTP, legacy HTTP+SSE) and the MCP handshake.

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use parking_lot::Mutex;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, CONTENT_TYPE};
use reqwest::{StatusCode, Url};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::rpc::{truncate, Core, RpcErr};
use crate::sse::SseParser;
use crate::McpServerConfig;

pub(crate) const PROTOCOL_VERSION: &str = "2025-06-18";
const STDERR_CAP: usize = 4096;
const SESSION_HEADER: &str = "mcp-session-id";
const VERSION_HEADER: &str = "mcp-protocol-version";

/// A tool as reported by the server.
#[derive(Debug, Clone)]
pub(crate) struct RawTool {
    pub name: String,
    pub description: String,
    pub schema: Value,
}

// ───────────────────────────── connection ─────────────────────────────

pub(crate) struct Conn {
    pub core: Arc<Core>,
    kind: Kind,
}

#[derive(Clone)]
enum Kind {
    Stdio(StdioHandle),
    Http(Arc<Http>),
    Sse(Arc<LegacySse>),
}

impl Kind {
    async fn send(&self, msg: &Value, id: Option<u64>) -> Result<(), RpcErr> {
        match self {
            Kind::Stdio(s) => s.send(msg),
            Kind::Http(h) => h.post(msg, id).await,
            Kind::Sse(s) => s.post(msg).await,
        }
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        self.core.close("connection closed");
    }
}

impl Conn {
    /// Send a JSON-RPC request and wait for its response.
    pub async fn request(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> Result<Value, RpcErr> {
        if self.core.is_closed() {
            return Err(RpcErr::Closed(self.closed_reason()));
        }
        let (id, rx) = self.core.register();
        if self.core.is_closed() {
            self.core.unregister(id);
            return Err(RpcErr::Closed(self.closed_reason()));
        }
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let work = async {
            self.kind.send(&msg, Some(id)).await?;
            match rx.await {
                Ok(r) => r,
                Err(_) => Err(RpcErr::Closed(self.closed_reason())),
            }
        };
        let res = tokio::select! {
            r = work => r,
            _ = tokio::time::sleep(timeout) => Err(RpcErr::Timeout(timeout)),
            _ = cancel.cancelled() => Err(RpcErr::Cancelled),
        };
        self.core.unregister(id);
        let res = match res {
            Err(RpcErr::Closed(_)) => Err(RpcErr::Closed(self.closed_reason())),
            other => other,
        };
        if let Err(e @ (RpcErr::Timeout(_) | RpcErr::Cancelled)) = &res {
            if method != "initialize" && !self.core.is_closed() {
                let reason = if matches!(e, RpcErr::Cancelled) { "cancelled by user".to_string() } else { e.to_string() };
                let note = json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/cancelled",
                    "params": {"requestId": id, "reason": reason}
                });
                let kind = self.kind.clone();
                tokio::spawn(async move {
                    let _ = tokio::time::timeout(Duration::from_secs(5), kind.send(&note, None)).await;
                });
            }
        }
        res
    }

    pub async fn notify(&self, method: &str, params: Value) -> Result<(), RpcErr> {
        let msg = json!({"jsonrpc": "2.0", "method": method, "params": params});
        self.kind.send(&msg, None).await
    }

    /// Why the connection closed (process exit status + stderr tail for local servers).
    pub fn closed_reason(&self) -> String {
        self.core.close_reason()
    }

    pub fn stderr_tail(&self) -> String {
        match &self.kind {
            Kind::Stdio(s) => s.stderr_tail(),
            _ => String::new(),
        }
    }

    pub fn transport_name(&self) -> &'static str {
        match self.kind {
            Kind::Stdio(_) => "stdio",
            Kind::Http(_) => "streamable-http",
            Kind::Sse(_) => "sse",
        }
    }

    /// Gracefully close: end HTTP session / kill the process (waits briefly).
    pub async fn close(&self, reason: &str) {
        if let Kind::Http(h) = &self.kind {
            if !self.core.is_closed() {
                let _ = tokio::time::timeout(Duration::from_secs(2), h.delete_session()).await;
            }
        }
        self.core.close(reason.to_string());
        if let Kind::Stdio(s) = &self.kind {
            let _ = tokio::time::timeout(Duration::from_secs(3), s.exited.cancelled()).await;
        }
    }
}

// ───────────────────────────── handshake ─────────────────────────────

/// Connect, initialize and list tools. `timeout` bounds the whole handshake.
pub(crate) async fn connect(cfg: &McpServerConfig, timeout: Duration) -> Result<(Arc<Conn>, Vec<RawTool>), String> {
    let deadline = Instant::now() + timeout;
    match cfg {
        McpServerConfig::Local { command, environment, cwd, .. } => {
            let core = Arc::new(Core::new());
            let handle = spawn_stdio(command, environment, cwd.as_deref(), core.clone())?;
            let conn = Arc::new(Conn { core, kind: Kind::Stdio(handle) });
            match handshake(&conn, deadline).await {
                Ok(tools) => Ok((conn, tools)),
                Err(e) => {
                    let msg = with_stderr(e.to_string(), &conn);
                    conn.core.close(msg.clone());
                    Err(msg)
                }
            }
        }
        McpServerConfig::Remote { url, headers, .. } => {
            let url = Url::parse(url).map_err(|e| format!("invalid url {url:?}: {e}"))?;
            let headers = header_map(headers)?;
            let client = reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(30))
                .build()
                .map_err(|e| format!("http client: {e}"))?;

            let core = Arc::new(Core::new());
            let http = Arc::new(Http {
                client: client.clone(),
                url: url.clone(),
                headers: headers.clone(),
                session_id: Mutex::new(None),
                protocol_version: Mutex::new(None),
                core: core.clone(),
            });
            let conn = Arc::new(Conn { core, kind: Kind::Http(http.clone()) });
            let init = initialize(&conn, deadline).await;
            let streamable_err = match init {
                Ok(version) => {
                    *http.protocol_version.lock() = Some(version);
                    let tools = finish_handshake(&conn, deadline).await.map_err(|e| e.to_string())?;
                    tokio::spawn(http.clone().listen());
                    return Ok((conn, tools));
                }
                Err(RpcErr::Http { status, body }) if matches!(status, 400 | 404 | 405) => {
                    RpcErr::Http { status, body }
                }
                Err(e) => return Err(e.to_string()),
            };
            drop(conn);
            tracing::debug!("mcp: streamable HTTP failed ({streamable_err}), trying legacy SSE at {url}");

            let core = Arc::new(Core::new());
            let remaining = deadline.saturating_duration_since(Instant::now());
            let sse = match LegacySse::connect(client, url, headers, core.clone(), remaining).await {
                Ok(s) => s,
                Err(e) => return Err(format!("streamable HTTP: {streamable_err}; SSE: {e}")),
            };
            let conn = Arc::new(Conn { core, kind: Kind::Sse(sse) });
            initialize(&conn, deadline).await.map_err(|e| e.to_string())?;
            let tools = finish_handshake(&conn, deadline).await.map_err(|e| e.to_string())?;
            Ok((conn, tools))
        }
    }
}

fn with_stderr(msg: String, conn: &Conn) -> String {
    let tail = conn.stderr_tail();
    if tail.is_empty() || msg.contains(&tail) {
        msg
    } else {
        format!("{msg}; stderr: {tail}")
    }
}

fn remaining(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

async fn handshake(conn: &Conn, deadline: Instant) -> Result<Vec<RawTool>, RpcErr> {
    initialize(conn, deadline).await?;
    finish_handshake(conn, deadline).await
}

/// Returns the negotiated protocol version.
async fn initialize(conn: &Conn, deadline: Instant) -> Result<String, RpcErr> {
    let params = json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": {},
        "clientInfo": {"name": "ref", "version": env!("CARGO_PKG_VERSION")}
    });
    let res = conn.request("initialize", params, remaining(deadline), &CancellationToken::new()).await?;
    if !res.is_object() {
        return Err(RpcErr::Transport(format!("invalid initialize result: {}", truncate(&res.to_string(), 200))));
    }
    Ok(res
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(PROTOCOL_VERSION)
        .to_string())
}

async fn finish_handshake(conn: &Conn, deadline: Instant) -> Result<Vec<RawTool>, RpcErr> {
    conn.notify("notifications/initialized", json!({})).await?;
    list_tools(conn, remaining(deadline)).await
}

/// `tools/list` following `nextCursor` pagination. Tools are deduplicated by name and sorted.
pub(crate) async fn list_tools(conn: &Conn, timeout: Duration) -> Result<Vec<RawTool>, RpcErr> {
    let deadline = Instant::now() + timeout;
    let cancel = CancellationToken::new();
    let mut cursor: Option<String> = None;
    let mut seen_cursors = HashSet::new();
    let mut names = HashSet::new();
    let mut out = Vec::new();
    for _ in 0..1000 {
        let params = match &cursor {
            Some(c) => json!({"cursor": c}),
            None => json!({}),
        };
        let res = conn.request("tools/list", params, remaining(deadline), &cancel).await?;
        for t in res.get("tools").and_then(Value::as_array).into_iter().flatten() {
            if let Some(tool) = parse_tool(t) {
                if names.insert(tool.name.clone()) {
                    out.push(tool);
                }
            }
        }
        match res.get("nextCursor").and_then(Value::as_str) {
            Some(c) if !c.is_empty() && seen_cursors.insert(c.to_string()) => cursor = Some(c.to_string()),
            _ => break,
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

fn parse_tool(t: &Value) -> Option<RawTool> {
    let name = t.get("name").and_then(Value::as_str).filter(|s| !s.is_empty())?.to_string();
    let text = |k: &str| t.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
    let description = text("description")
        .or_else(|| text("title"))
        .or_else(|| t.get("annotations").and_then(|a| a.get("title")).and_then(Value::as_str))
        .unwrap_or(&name)
        .to_string();
    let mut schema = match t.get("inputSchema") {
        Some(Value::Object(o)) => Value::Object(o.clone()),
        _ => json!({"type": "object", "properties": {}}),
    };
    if let Some(o) = schema.as_object_mut() {
        if !o.contains_key("type") {
            o.insert("type".into(), json!("object"));
        }
        if o.get("type").and_then(Value::as_str) == Some("object") && !o.contains_key("properties") {
            o.insert("properties".into(), json!({}));
        }
    }
    Some(RawTool { name, description, schema })
}

fn header_map(headers: &BTreeMap<String, String>) -> Result<HeaderMap, String> {
    let mut map = HeaderMap::new();
    for (k, v) in headers {
        let name = HeaderName::from_bytes(k.as_bytes()).map_err(|e| format!("invalid header name {k:?}: {e}"))?;
        let value = HeaderValue::from_str(v).map_err(|e| format!("invalid value for header {k:?}: {e}"))?;
        map.insert(name, value);
    }
    Ok(map)
}

// ───────────────────────────── stdio ─────────────────────────────

#[derive(Clone)]
pub(crate) struct StdioHandle {
    tx: mpsc::UnboundedSender<Vec<u8>>,
    stderr: Arc<Mutex<VecDeque<u8>>>,
    /// Cancelled once the child process has been reaped.
    exited: CancellationToken,
}

impl StdioHandle {
    fn send(&self, msg: &Value) -> Result<(), RpcErr> {
        let mut line = serde_json::to_vec(msg).map_err(|e| RpcErr::Transport(e.to_string()))?;
        line.push(b'\n');
        self.tx.send(line).map_err(|_| RpcErr::Closed("server stdin closed".into()))
    }

    fn stderr_tail(&self) -> String {
        stderr_tail(&self.stderr)
    }
}

fn stderr_tail(buf: &Mutex<VecDeque<u8>>) -> String {
    let bytes: Vec<u8> = buf.lock().iter().copied().collect();
    String::from_utf8_lossy(&bytes).trim().to_string()
}

fn spawn_stdio(
    command: &[String],
    env: &BTreeMap<String, String>,
    cwd: Option<&str>,
    core: Arc<Core>,
) -> Result<StdioHandle, String> {
    let Some((program, args)) = command.split_first() else {
        return Err("empty command".to_string());
    };
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args)
        .envs(env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(cwd) = cwd.filter(|c| !c.is_empty()) {
        cmd.current_dir(cwd);
    }
    // Own process group so the whole tree (e.g. npx → node) can be terminated together and
    // terminal signals aimed at the parent do not hit the server.
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = cmd.spawn().map_err(|e| format!("failed to spawn {program:?}: {e}"))?;
    let pid = child.id();
    let (Some(mut stdin), Some(stdout), Some(mut stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        return Err("failed to capture child stdio".to_string());
    };

    let stderr_buf = Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_CAP)));
    let exited = CancellationToken::new();
    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();

    // stdin writer
    {
        let core = core.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    msg = rx.recv() => {
                        let Some(msg) = msg else { break };
                        let r = async { stdin.write_all(&msg).await?; stdin.flush().await }.await;
                        if let Err(e) = r {
                            core.close(format!("failed to write to server stdin: {e}"));
                            break;
                        }
                    }
                    _ = core.closed.cancelled() => break,
                }
            }
        });
    }

    // stderr ring buffer
    let stderr_task = {
        let buf = stderr_buf.clone();
        tokio::spawn(async move {
            let mut chunk = [0u8; 2048];
            loop {
                match stderr.read(&mut chunk).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let mut b = buf.lock();
                        b.extend(&chunk[..n]);
                        let excess = b.len().saturating_sub(STDERR_CAP);
                        b.drain(..excess);
                    }
                }
            }
        })
    };

    // stdout reader
    {
        let core = core.clone();
        let tx = tx.clone();
        let exited = exited.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut line = Vec::new();
            loop {
                line.clear();
                let r = tokio::select! {
                    r = reader.read_until(b'\n', &mut line) => r,
                    _ = core.closed.cancelled() => return,
                };
                match r {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        for reply in core.handle_text(&String::from_utf8_lossy(&line)) {
                            if let Ok(mut b) = serde_json::to_vec(&reply) {
                                b.push(b'\n');
                                let _ = tx.send(b);
                            }
                        }
                    }
                }
            }
            // stdout closed: give the process a moment to exit so the waiter reports the status.
            let _ = tokio::time::timeout(Duration::from_secs(1), exited.cancelled()).await;
            core.close("server closed stdout");
        });
    }

    // process waiter / killer
    {
        let core = core.clone();
        let exited = exited.clone();
        let buf = stderr_buf.clone();
        tokio::spawn(async move {
            let status = tokio::select! {
                s = child.wait() => Some(s),
                _ = core.closed.cancelled() => None,
            };
            match status {
                Some(status) => {
                    let _ = tokio::time::timeout(Duration::from_millis(500), stderr_task).await;
                    let desc = match status {
                        Ok(s) => format!("server process exited ({s})"),
                        Err(e) => format!("failed to wait for server process: {e}"),
                    };
                    let tail = stderr_tail(&buf);
                    core.close(if tail.is_empty() { desc } else { format!("{desc}; stderr: {tail}") });
                    kill_group(pid, true);
                }
                None => {
                    // Graceful: TERM the group, then KILL after a grace period.
                    kill_group(pid, false);
                    if tokio::time::timeout(Duration::from_secs(2), child.wait()).await.is_err() {
                        kill_group(pid, true);
                        let _ = child.kill().await;
                    } else {
                        kill_group(pid, true);
                    }
                }
            }
            exited.cancel();
        });
    }

    Ok(StdioHandle { tx, stderr: stderr_buf, exited })
}

/// Signal the child's process group (the child is its leader).
#[allow(unused_variables)]
fn kill_group(pid: Option<u32>, force: bool) {
    #[cfg(unix)]
    if let Some(pid) = pid.and_then(|p| i32::try_from(p).ok()).filter(|p| *p > 0) {
        let sig = if force { libc::SIGKILL } else { libc::SIGTERM };
        // SAFETY: plain syscall; a negative pid addresses the process group.
        unsafe {
            libc::kill(-pid, sig);
        }
    }
}

// ───────────────────────────── Streamable HTTP ─────────────────────────────

pub(crate) struct Http {
    client: reqwest::Client,
    url: Url,
    headers: HeaderMap,
    session_id: Mutex<Option<String>>,
    protocol_version: Mutex<Option<String>>,
    core: Arc<Core>,
}

impl Http {
    fn request_headers(&self) -> HeaderMap {
        let mut h = self.headers.clone();
        if let Some(sid) = self.session_id.lock().as_deref() {
            if let Ok(v) = HeaderValue::from_str(sid) {
                h.insert(SESSION_HEADER, v);
            }
        }
        if let Some(ver) = self.protocol_version.lock().as_deref() {
            if let Ok(v) = HeaderValue::from_str(ver) {
                h.insert(VERSION_HEADER, v);
            }
        }
        h
    }

    fn capture_session(&self, resp: &reqwest::Response) {
        if let Some(sid) = resp.headers().get(SESSION_HEADER).and_then(|v| v.to_str().ok()) {
            let mut cur = self.session_id.lock();
            if cur.as_deref() != Some(sid) {
                *cur = Some(sid.to_string());
            }
        }
    }

    /// POST one message. For requests (`id` set) the response is delivered through `core`.
    async fn post(self: &Arc<Self>, msg: &Value, id: Option<u64>) -> Result<(), RpcErr> {
        let resp = self
            .client
            .post(self.url.clone())
            .headers(self.request_headers())
            .header(ACCEPT, "application/json, text/event-stream")
            .header(CONTENT_TYPE, "application/json")
            .json(msg)
            .send()
            .await
            .map_err(|e| RpcErr::Transport(error_chain(&e)))?;
        self.capture_session(&resp);
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            if status == StatusCode::NOT_FOUND && self.session_id.lock().is_some() {
                self.core.close("MCP session expired (HTTP 404)");
            }
            return Err(RpcErr::Http { status: status.as_u16(), body: truncate(&body, 500) });
        }
        if status == StatusCode::ACCEPTED || id.is_none() {
            return Ok(());
        }
        let ctype = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ctype.starts_with("text/event-stream") {
            let this = self.clone();
            let id = id.unwrap_or_default();
            tokio::spawn(async move { this.read_sse(resp, Some(id)).await });
            Ok(())
        } else {
            let body = resp.bytes().await.map_err(|e| RpcErr::Transport(error_chain(&e)))?;
            let text = String::from_utf8_lossy(&body);
            let replies = self.core.handle_text(&text);
            self.send_replies(replies);
            if let Some(id) = id {
                if self.core.is_pending(id) {
                    self.core.fail(
                        id,
                        RpcErr::Transport(format!("unexpected HTTP response body: {}", truncate(text.trim(), 300))),
                    );
                }
            }
            Ok(())
        }
    }

    fn send_replies(self: &Arc<Self>, replies: Vec<Value>) {
        for reply in replies {
            let this = self.clone();
            tokio::spawn(async move {
                let _ = this.post(&reply, None).await;
            });
        }
    }

    /// Read an SSE response stream. When `id` is set, stop once that request is no longer pending.
    async fn read_sse(self: Arc<Self>, resp: reqwest::Response, id: Option<u64>) {
        let mut stream = resp.bytes_stream();
        let mut parser = SseParser::default();
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        let mut ended_err = None;
        loop {
            tokio::select! {
                chunk = stream.next() => match chunk {
                    Some(Ok(bytes)) => {
                        for ev in parser.feed(&bytes) {
                            if ev.event == "message" {
                                let replies = self.core.handle_text(&ev.data);
                                self.send_replies(replies);
                            }
                        }
                    }
                    Some(Err(e)) => { ended_err = Some(error_chain(&e)); break }
                    None => break,
                },
                _ = tick.tick() => {}
                _ = self.core.closed.cancelled() => return,
            }
            if let Some(id) = id {
                if !self.core.is_pending(id) {
                    return;
                }
            }
        }
        if let Some(id) = id {
            let msg = match ended_err {
                Some(e) => format!("SSE response stream failed: {e}"),
                None => "SSE response stream ended without a response".to_string(),
            };
            self.core.fail(id, RpcErr::Transport(msg));
        }
    }

    /// Optional GET stream for server-initiated messages (e.g. tools/list_changed).
    async fn listen(self: Arc<Self>) {
        let mut backoff = Duration::from_secs(1);
        while !self.core.is_closed() {
            let started = Instant::now();
            let req = self
                .client
                .get(self.url.clone())
                .headers(self.request_headers())
                .header(ACCEPT, "text/event-stream")
                .send();
            let resp = tokio::select! {
                r = req => r,
                _ = self.core.closed.cancelled() => return,
            };
            match resp {
                Ok(r) if r.status().is_success() => {
                    let is_sse = r
                        .headers()
                        .get(CONTENT_TYPE)
                        .and_then(|v| v.to_str().ok())
                        .is_some_and(|c| c.to_ascii_lowercase().starts_with("text/event-stream"));
                    if !is_sse {
                        return;
                    }
                    self.clone().read_sse(r, None).await;
                }
                // 405 = server does not offer a GET stream; other 4xx: give up as well.
                Ok(r) if r.status().is_client_error() => return,
                Ok(_) | Err(_) => {}
            }
            if started.elapsed() > Duration::from_secs(60) {
                backoff = Duration::from_secs(1);
            }
            tokio::select! {
                _ = tokio::time::sleep(backoff) => {}
                _ = self.core.closed.cancelled() => return,
            }
            backoff = (backoff * 2).min(Duration::from_secs(60));
        }
    }

    async fn delete_session(&self) {
        if self.session_id.lock().is_none() {
            return;
        }
        let _ = self.client.delete(self.url.clone()).headers(self.request_headers()).send().await;
    }
}

// ───────────────────────────── legacy HTTP+SSE ─────────────────────────────

pub(crate) struct LegacySse {
    client: reqwest::Client,
    headers: HeaderMap,
    endpoint: Mutex<Option<Url>>,
    core: Arc<Core>,
}

impl LegacySse {
    async fn connect(
        client: reqwest::Client,
        url: Url,
        headers: HeaderMap,
        core: Arc<Core>,
        timeout: Duration,
    ) -> Result<Arc<Self>, String> {
        let fut = async {
            let resp = client
                .get(url.clone())
                .headers(headers.clone())
                .header(ACCEPT, "text/event-stream")
                .send()
                .await
                .map_err(|e| error_chain(&e))?;
            let status = resp.status();
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                return Err(RpcErr::Http { status: status.as_u16(), body: truncate(&body, 300) }.to_string());
            }
            let this = Arc::new(LegacySse { client, headers, endpoint: Mutex::new(None), core });
            let (tx, rx) = oneshot::channel();
            tokio::spawn(this.clone().read(resp, url, tx));
            match rx.await {
                Ok(Ok(())) => Ok(this),
                Ok(Err(e)) => Err(e),
                Err(_) => Err("SSE stream ended before endpoint event".to_string()),
            }
        };
        match tokio::time::timeout(timeout, fut).await {
            Ok(r) => r,
            Err(_) => {
                Err(format!("timed out after {:.1}s waiting for SSE endpoint event", timeout.as_secs_f64()))
            }
        }
    }

    async fn read(self: Arc<Self>, resp: reqwest::Response, base: Url, ready: oneshot::Sender<Result<(), String>>) {
        let mut ready = Some(ready);
        let mut stream = resp.bytes_stream();
        let mut parser = SseParser::default();
        let reason = loop {
            let chunk = tokio::select! {
                c = stream.next() => c,
                _ = self.core.closed.cancelled() => return,
            };
            match chunk {
                Some(Ok(bytes)) => {
                    for ev in parser.feed(&bytes) {
                        match ev.event.as_str() {
                            "endpoint" => match base.join(ev.data.trim()) {
                                Ok(u) => {
                                    *self.endpoint.lock() = Some(u);
                                    if let Some(r) = ready.take() {
                                        let _ = r.send(Ok(()));
                                    }
                                }
                                Err(e) => {
                                    if let Some(r) = ready.take() {
                                        let _ = r.send(Err(format!("invalid endpoint {:?}: {e}", ev.data)));
                                    }
                                }
                            },
                            "message" => {
                                for reply in self.core.handle_text(&ev.data) {
                                    let this = self.clone();
                                    tokio::spawn(async move {
                                        let _ = this.post(&reply).await;
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Some(Err(e)) => break format!("SSE stream failed: {}", error_chain(&e)),
                None => break "SSE stream closed by server".to_string(),
            }
        };
        if let Some(r) = ready.take() {
            let _ = r.send(Err(reason.clone()));
        }
        self.core.close(reason);
    }

    async fn post(&self, msg: &Value) -> Result<(), RpcErr> {
        let Some(endpoint) = self.endpoint.lock().clone() else {
            return Err(RpcErr::Transport("no SSE endpoint".into()));
        };
        let resp = self
            .client
            .post(endpoint)
            .headers(self.headers.clone())
            .header(CONTENT_TYPE, "application/json")
            .json(msg)
            .send()
            .await
            .map_err(|e| RpcErr::Transport(error_chain(&e)))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(RpcErr::Http { status: status.as_u16(), body: truncate(&body, 500) });
        }
        Ok(())
    }
}

fn error_chain(e: &dyn std::error::Error) -> String {
    let mut s = e.to_string();
    let mut src = e.source();
    while let Some(c) = src {
        let cs = c.to_string();
        if !s.contains(&cs) {
            s.push_str(": ");
            s.push_str(&cs);
        }
        src = c.source();
    }
    s
}
