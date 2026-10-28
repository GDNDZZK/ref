//! Streamable HTTP transport (MCP 2025-03-26 / 2025-06-18) with legacy HTTP+SSE
//! (2024-11-05) fallback, plus a small Server-Sent Events parser.

use crate::client::{Shared, contains_response, rpc_error};
use anyhow::{Context, Result, anyhow, bail};
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use reqwest::{StatusCode, Url};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const SESSION_HEADER: &str = "mcp-session-id";
const VERSION_HEADER: &str = "mcp-protocol-version";
/// Upper bound for a single buffered SSE event / JSON body.
const MAX_EVENT: usize = 16 * 1024 * 1024;

// ---------------------------------------------------------------------------
// SSE parser
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct SseEvent {
    /// Event type; empty means the default ("message").
    pub event: String,
    pub data: String,
    pub id: Option<String>,
}

/// Incremental parser following the WHATWG event-stream rules
/// (CR, LF and CRLF line endings, comments, multi-line data, optional BOM).
#[derive(Default)]
pub(crate) struct SseParser {
    line: Vec<u8>,
    pending_cr: bool,
    started: bool,
    event: String,
    data: String,
    has_data: bool,
    id: Option<String>,
    oversized: bool,
}

impl SseParser {
    pub(crate) fn feed(&mut self, chunk: &[u8], out: &mut Vec<SseEvent>) {
        let mut bytes = chunk;
        if !self.started && !bytes.is_empty() {
            self.started = true;
            if let Some(rest) = bytes.strip_prefix(b"\xEF\xBB\xBF".as_slice()) {
                bytes = rest;
            }
        }
        for &b in bytes {
            if self.pending_cr {
                self.pending_cr = false;
                if b == b'\n' {
                    continue; // CRLF: line already processed at CR.
                }
            }
            match b {
                b'\r' => {
                    self.pending_cr = true;
                    self.end_line(out);
                }
                b'\n' => self.end_line(out),
                _ => {
                    if self.line.len() < MAX_EVENT {
                        self.line.push(b);
                    } else {
                        self.oversized = true;
                    }
                }
            }
        }
    }

    /// Flush at end of stream (lenient: dispatches an unterminated final event).
    pub(crate) fn finish(&mut self, out: &mut Vec<SseEvent>) {
        if !self.line.is_empty() {
            self.end_line(out);
        }
        self.end_line(out);
    }

    fn end_line(&mut self, out: &mut Vec<SseEvent>) {
        let line = std::mem::take(&mut self.line);
        if line.is_empty() {
            self.dispatch(out);
            return;
        }
        let line = String::from_utf8_lossy(&line);
        if line.starts_with(':') {
            return; // comment / keep-alive
        }
        let (field, value) = match line.find(':') {
            Some(i) => {
                let v = &line[i + 1..];
                (&line[..i], v.strip_prefix(' ').unwrap_or(v))
            }
            None => (&line[..], ""),
        };
        match field {
            "event" => self.event = value.to_string(),
            "data" => {
                if self.data.len() + value.len() > MAX_EVENT {
                    self.oversized = true;
                } else {
                    self.data.push_str(value);
                    self.data.push('\n');
                }
                self.has_data = true;
            }
            "id" if !value.contains('\0') => self.id = Some(value.to_string()),
            _ => {} // "retry" and unknown fields are ignored
        }
    }

    fn dispatch(&mut self, out: &mut Vec<SseEvent>) {
        let event = std::mem::take(&mut self.event);
        let mut data = std::mem::take(&mut self.data);
        let has_data = std::mem::take(&mut self.has_data);
        let oversized = std::mem::take(&mut self.oversized);
        if !has_data {
            return;
        }
        if oversized {
            tracing::warn!("dropping oversized SSE event");
            return;
        }
        if data.ends_with('\n') {
            data.pop();
        }
        out.push(SseEvent { event, data, id: self.id.clone() });
    }
}

// ---------------------------------------------------------------------------
// Transport
// ---------------------------------------------------------------------------

enum Mode {
    Streamable,
    /// Legacy HTTP+SSE: POST to `endpoint`, responses arrive on the GET stream.
    Legacy { endpoint: Url },
}

pub(crate) struct HttpTransport {
    client: reqwest::Client,
    url: Url,
    headers: HeaderMap,
    shared: Arc<Shared>,
    timeout: Duration,
    mode: StdMutex<Mode>,
    session_id: StdMutex<Option<String>>,
    protocol_version: StdMutex<Option<String>>,
    /// The original initialize request, replayed on session expiry.
    init_msg: StdMutex<Option<Value>>,
    reinit_lock: tokio::sync::Mutex<()>,
    reinit_seq: std::sync::atomic::AtomicU64,
    cancel: CancellationToken,
    tasks: StdMutex<Vec<tokio::task::JoinHandle<()>>>,
}

fn is_loopback(url: &Url) -> bool {
    let Some(host) = url.host_str() else { return false };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

fn content_type(resp: &reqwest::Response) -> String {
    resp.headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn is_initialize(msg: &Value) -> bool {
    msg.get("method").and_then(Value::as_str) == Some("initialize")
}

/// Request id if `msg` is a request (has both method and id).
fn request_id(msg: &Value) -> Option<Value> {
    msg.get("method")?;
    msg.get("id").filter(|v| !v.is_null()).cloned()
}

async fn error_body(resp: reqwest::Response) -> String {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    let body: String = body.trim().chars().take(500).collect();
    if body.is_empty() { format!("HTTP {status}") } else { format!("HTTP {status}: {body}") }
}

impl HttpTransport {
    pub(crate) fn new(
        url: &str,
        headers: &BTreeMap<String, String>,
        shared: Arc<Shared>,
        timeout: Duration,
    ) -> Result<Self> {
        let url = Url::parse(url).with_context(|| format!("invalid MCP url {url:?}"))?;
        let mut map = HeaderMap::new();
        for (k, v) in headers {
            let name = HeaderName::from_bytes(k.as_bytes()).with_context(|| format!("invalid header name {k:?}"))?;
            let value = HeaderValue::from_str(v).with_context(|| format!("invalid value for header {k:?}"))?;
            map.insert(name, value);
        }
        let mut builder = reqwest::Client::builder()
            .connect_timeout(timeout)
            .user_agent(concat!("ref/", env!("CARGO_PKG_VERSION")));
        if is_loopback(&url) {
            builder = builder.no_proxy();
        }
        let client = builder.build().context("building HTTP client")?;
        Ok(HttpTransport {
            client,
            url,
            headers: map,
            shared,
            timeout,
            mode: StdMutex::new(Mode::Streamable),
            session_id: StdMutex::new(None),
            protocol_version: StdMutex::new(None),
            init_msg: StdMutex::new(None),
            reinit_lock: tokio::sync::Mutex::new(()),
            reinit_seq: Default::default(),
            cancel: CancellationToken::new(),
            tasks: StdMutex::new(Vec::new()),
        })
    }

    pub(crate) fn set_protocol_version(&self, v: &str) {
        *self.protocol_version.lock().unwrap() = Some(v.to_string());
    }

    fn session(&self) -> Option<String> {
        self.session_id.lock().unwrap().clone()
    }

    fn legacy_endpoint(&self) -> Option<Url> {
        match &*self.mode.lock().unwrap() {
            Mode::Legacy { endpoint } => Some(endpoint.clone()),
            Mode::Streamable => None,
        }
    }

    fn spawn_task(&self, fut: impl std::future::Future<Output = ()> + Send + 'static) {
        let cancel = self.cancel.clone();
        let handle = tokio::spawn(async move {
            tokio::select! {
                _ = cancel.cancelled() => {}
                _ = fut => {}
            }
        });
        let mut tasks = self.tasks.lock().unwrap();
        tasks.retain(|t| !t.is_finished());
        tasks.push(handle);
    }

    /// Common headers: user headers, session id and protocol version.
    fn base_request(&self, method: reqwest::Method, url: Url) -> reqwest::RequestBuilder {
        let mut req = self.client.request(method, url).headers(self.headers.clone());
        if let Some(sid) = self.session() {
            req = req.header(SESSION_HEADER, sid);
        }
        if let Some(v) = self.protocol_version.lock().unwrap().clone() {
            req = req.header(VERSION_HEADER, v);
        }
        req
    }

    async fn post(&self, url: Url, msg: &Value) -> Result<reqwest::Response> {
        let body = serde_json::to_vec(msg)?;
        self.base_request(reqwest::Method::POST, url)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json, text/event-stream")
            .body(body)
            .send()
            .await
            .map_err(|e| anyhow!("MCP HTTP request failed: {e}"))
    }

    pub(crate) async fn send(&self, msg: &Value) -> Result<()> {
        if self.cancel.is_cancelled() {
            bail!("MCP HTTP transport closed");
        }
        if is_initialize(msg) {
            *self.init_msg.lock().unwrap() = Some(msg.clone());
        }
        if let Some(endpoint) = self.legacy_endpoint() {
            return self.send_legacy(endpoint, msg).await;
        }
        let req_id = request_id(msg);
        let mut retried = false;
        loop {
            let sid_used = self.session();
            let resp = self.post(self.url.clone(), msg).await?;
            let status = resp.status();

            if is_initialize(msg) {
                if matches!(status.as_u16(), 400 | 404 | 405) && sid_used.is_none() {
                    let primary = error_body(resp).await;
                    tracing::debug!(server = %self.shared.name, "streamable HTTP initialize failed ({primary}); trying legacy SSE");
                    let endpoint = self
                        .open_legacy()
                        .await
                        .map_err(|e| anyhow!("{primary} (legacy SSE fallback also failed: {e:#})"))?;
                    return self.send_legacy(endpoint, msg).await;
                }
                if let Some(sid) = resp.headers().get(SESSION_HEADER).and_then(|v| v.to_str().ok()) {
                    *self.session_id.lock().unwrap() = Some(sid.to_string());
                }
            }

            if status == StatusCode::NOT_FOUND && sid_used.is_some() && !retried && !is_initialize(msg) {
                tracing::debug!(server = %self.shared.name, "MCP session expired; re-initializing");
                retried = true;
                self.reinitialize(sid_used).await.context("re-initializing expired MCP session")?;
                continue;
            }
            if !status.is_success() {
                let err = error_body(resp).await;
                if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
                    bail!("MCP server rejected credentials: {err}");
                }
                bail!("{err}");
            }
            if status == StatusCode::ACCEPTED || req_id.is_none() {
                // Notifications/responses: body (if any) is irrelevant, but dispatch
                // anything the server chose to send.
                if req_id.is_none() && status != StatusCode::ACCEPTED {
                    let _ = pump(&self.shared, resp, None, false).await;
                }
                return Ok(());
            }
            let id = req_id.as_ref().unwrap();
            match pump(&self.shared, resp, Some(id), false).await {
                Ok(Some(_)) => {}
                Ok(None) => self.shared.fail_pending(id, "MCP server closed the response without a result"),
                Err(e) => self.shared.fail_pending(id, &format!("MCP response stream failed: {e:#}")),
            }
            return Ok(());
        }
    }

    /// Replay initialize (with a private id) after a 404 session-expired response.
    async fn reinitialize(&self, expired: Option<String>) -> Result<()> {
        let _guard = self.reinit_lock.lock().await;
        if self.session() != expired {
            return Ok(()); // someone else already re-initialized
        }
        *self.session_id.lock().unwrap() = None;
        let mut init = self.init_msg.lock().unwrap().clone().ok_or_else(|| anyhow!("no initialize request recorded"))?;
        let seq = self.reinit_seq.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let id = json!(format!("ref-reinit-{seq}"));
        init["id"] = id.clone();
        let resp = self.post(self.url.clone(), &init).await?;
        if !resp.status().is_success() {
            bail!("{}", error_body(resp).await);
        }
        if let Some(sid) = resp.headers().get(SESSION_HEADER).and_then(|v| v.to_str().ok()) {
            *self.session_id.lock().unwrap() = Some(sid.to_string());
        }
        let reply = tokio::time::timeout(self.timeout, pump(&self.shared, resp, Some(&id), true))
            .await
            .map_err(|_| anyhow!("timed out"))??
            .ok_or_else(|| anyhow!("no initialize response"))?;
        let reply = match reply {
            Value::Array(items) => items.into_iter().find(|m| contains_response(m, &id)).unwrap_or(Value::Null),
            other => other,
        };
        if let Some(err) = reply.get("error") {
            return Err(rpc_error(err));
        }
        if let Some(v) = reply.pointer("/result/protocolVersion").and_then(Value::as_str) {
            self.set_protocol_version(v);
        }
        let note = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        let resp = self.post(self.url.clone(), &note).await?;
        if !resp.status().is_success() {
            bail!("notifications/initialized: {}", error_body(resp).await);
        }
        // The previous GET stream (if any) was bound to the expired session.
        self.start_get_stream();
        Ok(())
    }

    /// Open the optional server->client GET SSE stream (streamable mode only).
    pub(crate) fn start_get_stream(&self) {
        if self.legacy_endpoint().is_some() {
            return;
        }
        let shared = self.shared.clone();
        let this = GetStream {
            client: self.client.clone(),
            url: self.url.clone(),
            headers: self.headers.clone(),
            session: self.session(),
            version: self.protocol_version.lock().unwrap().clone(),
        };
        self.spawn_task(async move { this.run(shared).await });
    }

    /// Legacy transport: GET the SSE stream and wait for the `endpoint` event.
    async fn open_legacy(&self) -> Result<Url> {
        let resp = self
            .client
            .get(self.url.clone())
            .headers(self.headers.clone())
            .header(ACCEPT, "text/event-stream")
            .send()
            .await
            .map_err(|e| anyhow!("GET failed: {e}"))?;
        if !resp.status().is_success() {
            bail!("{}", error_body(resp).await);
        }
        if !content_type(&resp).starts_with("text/event-stream") {
            bail!("GET did not return text/event-stream");
        }
        let (tx, rx) = tokio::sync::oneshot::channel::<String>();
        let shared = self.shared.clone();
        self.spawn_task(async move {
            let mut tx = Some(tx);
            let res = read_sse(resp, |ev| {
                match ev.event.as_str() {
                    "endpoint" => {
                        if let Some(tx) = tx.take() {
                            let _ = tx.send(ev.data.trim().to_string());
                        }
                    }
                    "" | "message" => dispatch_data(&shared, &ev.data),
                    other => tracing::trace!(server = %shared.name, "ignoring SSE event {other}"),
                }
                true
            })
            .await;
            if let Err(e) = res {
                tracing::debug!(server = %shared.name, "legacy SSE stream error: {e:#}");
            }
            shared.fail_all("MCP SSE stream closed");
        });
        let path = tokio::time::timeout(self.timeout, rx)
            .await
            .map_err(|_| anyhow!("timed out waiting for endpoint event"))?
            .map_err(|_| anyhow!("SSE stream closed before endpoint event"))?;
        let endpoint = self.url.join(&path).with_context(|| format!("invalid endpoint {path:?}"))?;
        *self.mode.lock().unwrap() = Mode::Legacy { endpoint: endpoint.clone() };
        Ok(endpoint)
    }

    async fn send_legacy(&self, endpoint: Url, msg: &Value) -> Result<()> {
        let body = serde_json::to_vec(msg)?;
        let resp = self
            .client
            .post(endpoint)
            .headers(self.headers.clone())
            .header(CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(|e| anyhow!("MCP HTTP request failed: {e}"))?;
        if !resp.status().is_success() {
            bail!("{}", error_body(resp).await);
        }
        Ok(())
    }

    pub(crate) async fn close(&self) {
        self.cancel.cancel();
        for t in self.tasks.lock().unwrap().drain(..) {
            t.abort();
        }
        if self.legacy_endpoint().is_none()
            && let Some(sid) = self.session()
        {
            let req = self
                .client
                .delete(self.url.clone())
                .headers(self.headers.clone())
                .header(SESSION_HEADER, sid)
                .timeout(Duration::from_secs(1));
            let _ = req.send().await;
        }
    }
}

impl Drop for HttpTransport {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

struct GetStream {
    client: reqwest::Client,
    url: Url,
    headers: HeaderMap,
    session: Option<String>,
    version: Option<String>,
}

impl GetStream {
    async fn run(self, shared: Arc<Shared>) {
        let mut backoff = Duration::from_secs(1);
        loop {
            let mut req = self.client.get(self.url.clone()).headers(self.headers.clone()).header(ACCEPT, "text/event-stream");
            if let Some(s) = &self.session {
                req = req.header(SESSION_HEADER, s);
            }
            if let Some(v) = &self.version {
                req = req.header(VERSION_HEADER, v);
            }
            let resp = match req.send().await {
                Ok(r) => r,
                Err(e) => {
                    tracing::debug!(server = %shared.name, "GET SSE stream failed: {e}");
                    return;
                }
            };
            if !resp.status().is_success() || !content_type(&resp).starts_with("text/event-stream") {
                // 405 = server doesn't offer a GET stream; anything else: give up quietly.
                tracing::debug!(server = %shared.name, "no GET SSE stream (HTTP {})", resp.status());
                return;
            }
            let started = std::time::Instant::now();
            let res = read_sse(resp, |ev| {
                if ev.event.is_empty() || ev.event == "message" {
                    dispatch_data(&shared, &ev.data);
                }
                true
            })
            .await;
            if let Err(e) = res {
                tracing::debug!(server = %shared.name, "GET SSE stream error: {e:#}");
            }
            if started.elapsed() > Duration::from_secs(60) {
                backoff = Duration::from_secs(1);
            } else {
                backoff = (backoff * 2).min(Duration::from_secs(60));
            }
            tokio::time::sleep(backoff).await;
        }
    }
}

fn dispatch_data(shared: &Shared, data: &str) {
    if data.trim().is_empty() {
        return;
    }
    match serde_json::from_str::<Value>(data) {
        Ok(v) => shared.dispatch(v),
        Err(e) => tracing::debug!(server = %shared.name, "invalid JSON in SSE event: {e}"),
    }
}

/// Read an SSE response, calling `on_event` for each event until it returns false.
async fn read_sse(mut resp: reqwest::Response, mut on_event: impl FnMut(SseEvent) -> bool) -> Result<()> {
    let mut parser = SseParser::default();
    let mut events = Vec::new();
    loop {
        let chunk = resp.chunk().await.map_err(|e| anyhow!("{e}"))?;
        match &chunk {
            Some(bytes) => parser.feed(bytes, &mut events),
            None => parser.finish(&mut events),
        }
        for ev in events.drain(..) {
            if !on_event(ev) {
                return Ok(());
            }
        }
        if chunk.is_none() {
            return Ok(());
        }
    }
}

/// Consume a POST response body (JSON or SSE), dispatching messages to `shared`.
/// Returns the message containing the response for `id` once seen (reading stops
/// there). With `intercept`, that message is returned instead of dispatched.
async fn pump(shared: &Shared, resp: reqwest::Response, id: Option<&Value>, intercept: bool) -> Result<Option<Value>> {
    let ct = content_type(&resp);
    let mut found = None;
    let mut handle = |msg: Value| -> bool {
        if let Some(id) = id
            && contains_response(&msg, id)
        {
            if !intercept {
                shared.dispatch(msg.clone());
            }
            found = Some(msg);
            return false;
        }
        shared.dispatch(msg);
        true
    };
    if ct.starts_with("text/event-stream") {
        read_sse(resp, |ev| {
            if !(ev.event.is_empty() || ev.event == "message") || ev.data.trim().is_empty() {
                return true;
            }
            match serde_json::from_str::<Value>(&ev.data) {
                Ok(v) => handle(v),
                Err(e) => {
                    tracing::debug!(server = %shared.name, "invalid JSON in SSE event: {e}");
                    true
                }
            }
        })
        .await?;
    } else {
        let body = resp.bytes().await.map_err(|e| anyhow!("{e}"))?;
        if body.len() > MAX_EVENT {
            bail!("response body too large ({} bytes)", body.len());
        }
        if !body.iter().all(u8::is_ascii_whitespace) {
            let v: Value = serde_json::from_slice(&body).context("invalid JSON response body")?;
            handle(v);
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_all(chunks: &[&[u8]]) -> Vec<SseEvent> {
        let mut p = SseParser::default();
        let mut out = Vec::new();
        for c in chunks {
            p.feed(c, &mut out);
        }
        p.finish(&mut out);
        out
    }

    #[test]
    fn sse_basic_and_split() {
        let ev = parse_all(&[b"\xEF\xBB\xBFevent: endpoint\r", b"\ndata: /msg?x=1\r\n\r\n", b": ping\n\ndata:a\ndata: b\n\n"]);
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].event, "endpoint");
        assert_eq!(ev[0].data, "/msg?x=1");
        assert_eq!(ev[1].event, "");
        assert_eq!(ev[1].data, "a\nb");
    }

    #[test]
    fn sse_cr_only_and_unterminated() {
        let ev = parse_all(&[b"id: 7\rdata: {\"x\":1}\r\r", b"data: tail"]);
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].id.as_deref(), Some("7"));
        assert_eq!(ev[0].data, "{\"x\":1}");
        assert_eq!(ev[1].data, "tail");
    }

    #[test]
    fn sse_event_without_data_is_skipped() {
        let ev = parse_all(&[b"event: x\n\nretry: 10\n\ndata\n\n"]);
        assert_eq!(ev, vec![SseEvent { event: String::new(), data: String::new(), id: None }]);
    }
}
