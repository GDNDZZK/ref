//! Remote transports: Streamable HTTP (2025-03-26+) and legacy HTTP+SSE (2024-11-05).

use crate::rpc::{truncate, InTx, Incoming};
use crate::sse::SseParser;
use anyhow::{anyhow, bail, Context, Result};
use futures::StreamExt;
use indexmap::IndexMap;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, CONTENT_TYPE};
use reqwest::{Response, StatusCode, Url};
use serde_json::{json, Value};
use std::sync::Mutex;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const SESSION_HEADER: &str = "mcp-session-id";
const VERSION_HEADER: &str = "mcp-protocol-version";

/// Non-success HTTP status from the server.
#[derive(Debug)]
pub(crate) struct HttpStatusError {
    pub status: StatusCode,
    pub body: String,
}

impl std::fmt::Display for HttpStatusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HTTP {}", self.status)?;
        if matches!(self.status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
            write!(f, " (check the server's `headers`; OAuth is not supported)")?;
        }
        if !self.body.trim().is_empty() {
            write!(f, ": {}", truncate(self.body.trim(), 500))?;
        }
        Ok(())
    }
}

impl std::error::Error for HttpStatusError {}

impl HttpStatusError {
    /// Whether a failed Streamable HTTP initialize should be retried with legacy SSE.
    pub fn suggests_sse_fallback(&self) -> bool {
        self.status.is_client_error()
            && !matches!(self.status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS)
    }
}

pub(crate) fn build_client(url: &Url) -> Result<reqwest::Client> {
    let mut b = reqwest::Client::builder();
    let host = url.host_str().unwrap_or("").trim_start_matches('[').trim_end_matches(']');
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback());
    if loopback {
        // Never route local servers through an HTTP proxy.
        b = b.no_proxy();
    }
    b.connect_timeout(Duration::from_secs(30))
        .user_agent(concat!("hernees/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("failed to build HTTP client")
}

pub(crate) fn build_headers(headers: &IndexMap<String, String>) -> Result<HeaderMap> {
    let mut map = HeaderMap::new();
    for (k, v) in headers {
        let name = HeaderName::from_bytes(k.as_bytes()).with_context(|| format!("invalid header name `{k}`"))?;
        // Never include the value in errors: it is usually a secret.
        let value = HeaderValue::from_str(v).map_err(|_| anyhow!("invalid value for header `{k}`"))?;
        map.insert(name, value);
    }
    Ok(map)
}

fn content_type(resp: &Response) -> String {
    resp.headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase()
}

async fn status_error(resp: Response) -> anyhow::Error {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    anyhow::Error::new(HttpStatusError { status, body })
}

fn id_matches(have: Option<&Value>, want: &Value) -> bool {
    match have {
        Some(h) if h == want => true,
        Some(h) => match (h, want) {
            (Value::String(s), Value::Number(n)) | (Value::Number(n), Value::String(s)) => *s == n.to_string(),
            _ => false,
        },
        None => false,
    }
}

fn is_response(v: &Value, want: &Value) -> bool {
    v.get("method").is_none() && id_matches(v.get("id"), want)
}

// ---------------------------------------------------------------------------
// Streamable HTTP
// ---------------------------------------------------------------------------

pub(crate) struct Streamable {
    label: String,
    client: reqwest::Client,
    url: Url,
    headers: HeaderMap,
    session: Mutex<Option<String>>,
    protocol_version: Mutex<Option<String>>,
    init_params: Mutex<Option<Value>>,
    reinit_lock: tokio::sync::Mutex<()>,
    listener_started: Mutex<bool>,
    in_tx: InTx,
    stop: CancellationToken,
}

impl Streamable {
    pub fn new(label: &str, url: &str, headers: &IndexMap<String, String>, in_tx: InTx) -> Result<Streamable> {
        let url = Url::parse(url).with_context(|| format!("invalid MCP url `{url}`"))?;
        Ok(Streamable {
            label: label.to_string(),
            client: build_client(&url)?,
            url,
            headers: build_headers(headers)?,
            session: Mutex::new(None),
            protocol_version: Mutex::new(None),
            init_params: Mutex::new(None),
            reinit_lock: tokio::sync::Mutex::new(()),
            listener_started: Mutex::new(false),
            in_tx,
            stop: CancellationToken::new(),
        })
    }

    pub fn set_protocol_version(&self, v: &str) {
        *self.protocol_version.lock().unwrap() = Some(v.to_string());
    }

    fn request(&self, method: reqwest::Method) -> reqwest::RequestBuilder {
        let mut rb = self.client.request(method, self.url.clone()).headers(self.headers.clone());
        if let Some(s) = &*self.session.lock().unwrap() {
            rb = rb.header(SESSION_HEADER, s);
        }
        if let Some(v) = &*self.protocol_version.lock().unwrap() {
            rb = rb.header(VERSION_HEADER, v);
        }
        rb
    }

    async fn post(&self, msg: &Value) -> Result<Response> {
        let resp = self
            .request(reqwest::Method::POST)
            .header(ACCEPT, "application/json, text/event-stream")
            .header(CONTENT_TYPE, "application/json")
            .body(serde_json::to_vec(msg)?)
            .send()
            .await
            .with_context(|| format!("POST {} failed", self.url))?;
        if let Some(s) = resp.headers().get(SESSION_HEADER).and_then(|v| v.to_str().ok()) {
            *self.session.lock().unwrap() = Some(s.to_string());
        }
        Ok(resp)
    }

    pub async fn send(&self, msg: &Value, req_id: Option<i64>) -> Result<()> {
        if msg.get("method").and_then(Value::as_str) == Some("initialize") {
            *self.init_params.lock().unwrap() = msg.get("params").cloned();
        }
        let sent_session = self.session.lock().unwrap().clone();
        let mut resp = self.post(msg).await?;
        if sent_session.is_some() {
            resp = match self.check_expired(resp).await? {
                Some(r) => r,
                None => {
                    tracing::debug!(server = %self.label, "MCP session expired; re-initializing");
                    self.reinit(sent_session.as_deref()).await.context("re-initializing expired MCP session failed")?;
                    self.post(msg).await?
                }
            };
        }
        if !resp.status().is_success() {
            return Err(status_error(resp).await);
        }
        if msg.get("method").and_then(Value::as_str) == Some("notifications/initialized") {
            self.start_listener();
        }
        match req_id {
            // Accepted without a body: the response will arrive on the standalone GET stream.
            Some(_) if resp.status() == StatusCode::ACCEPTED => Ok(()),
            Some(id) => {
                let want = json!(id);
                match self.read_response(resp, Some(&want)).await? {
                    Some(m) => {
                        let _ = self.in_tx.send(Incoming::Msg(m));
                        Ok(())
                    }
                    None => bail!("server closed the response stream without answering request {id}"),
                }
            }
            None => {
                if resp.status() == StatusCode::ACCEPTED || resp.content_length() == Some(0) {
                    return Ok(());
                }
                // Unexpected body for a notification/response: forward anything in it.
                if content_type(&resp).contains("text/event-stream") {
                    let this_tx = self.in_tx.clone();
                    let label = self.label.clone();
                    tokio::spawn(async move {
                        if let Err(e) = forward_sse(resp, &this_tx).await {
                            tracing::debug!(server = %label, "SSE stream error: {e:#}");
                        }
                    });
                } else {
                    let _ = self.read_response(resp, None).await;
                }
                Ok(())
            }
        }
    }

    /// 404 (or 400 "not initialized") after we sent a session id means the session expired:
    /// returns None. Otherwise gives the response back (or the error it represents).
    async fn check_expired(&self, resp: Response) -> Result<Option<Response>> {
        match resp.status() {
            StatusCode::NOT_FOUND => Ok(None),
            StatusCode::BAD_REQUEST => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                let lower = body.to_ascii_lowercase();
                if lower.contains("not initialized") || lower.contains("session") {
                    Ok(None)
                } else {
                    Err(anyhow::Error::new(HttpStatusError { status, body }))
                }
            }
            _ => Ok(Some(resp)),
        }
    }

    async fn reinit(&self, failed_session: Option<&str>) -> Result<()> {
        let _g = self.reinit_lock.lock().await;
        {
            let cur = self.session.lock().unwrap();
            if cur.as_deref() != failed_session {
                return Ok(()); // someone else already re-initialized
            }
        }
        let params = self.init_params.lock().unwrap().clone().ok_or_else(|| anyhow!("no initialize params"))?;
        *self.session.lock().unwrap() = None;
        let want = json!("hn-reinit");
        let resp = self.post(&json!({"jsonrpc": "2.0", "id": want, "method": "initialize", "params": params})).await?;
        if !resp.status().is_success() {
            return Err(status_error(resp).await);
        }
        let reply = self
            .read_response(resp, Some(&want))
            .await?
            .ok_or_else(|| anyhow!("no initialize response"))?;
        if let Some(err) = reply.get("error") {
            bail!("initialize failed: {err}");
        }
        let resp = self.post(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).await?;
        if !resp.status().is_success() {
            return Err(status_error(resp).await);
        }
        Ok(())
    }

    /// Read a JSON or SSE response body. Messages other than the response to `want`
    /// are forwarded to the dispatcher; the matching one is returned.
    async fn read_response(&self, resp: Response, want: Option<&Value>) -> Result<Option<Value>> {
        let ct = content_type(&resp);
        if ct.contains("text/event-stream") {
            let mut parser = SseParser::default();
            let mut stream = resp.bytes_stream();
            loop {
                match stream.next().await {
                    Some(chunk) => {
                        for ev in parser.push(&chunk.context("error reading SSE response")?) {
                            if let Some(m) = self.handle_event(&ev.event, &ev.data, want) {
                                return Ok(Some(m));
                            }
                        }
                    }
                    None => {
                        let found = parser.finish().and_then(|ev| self.handle_event(&ev.event, &ev.data, want));
                        return Ok(found);
                    }
                }
            }
        }
        let body = resp.text().await.context("error reading response body")?;
        if body.trim().is_empty() {
            return Ok(None);
        }
        Ok(self.handle_payload(&body, want))
    }

    fn handle_event(&self, event: &str, data: &str, want: Option<&Value>) -> Option<Value> {
        if event.is_empty() || event == "message" {
            self.handle_payload(data, want)
        } else {
            None
        }
    }

    fn handle_payload(&self, data: &str, want: Option<&Value>) -> Option<Value> {
        if data.trim().is_empty() {
            return None;
        }
        let v: Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => {
                tracing::debug!(server = %self.label, "ignoring non-JSON payload: {}", truncate(data, 300));
                return None;
            }
        };
        let items = match v {
            Value::Array(a) => a,
            other => vec![other],
        };
        let mut found = None;
        for m in items {
            if found.is_none() && want.is_some_and(|w| is_response(&m, w)) {
                found = Some(m);
            } else {
                let _ = self.in_tx.send(Incoming::Msg(m));
            }
        }
        found
    }

    /// Optional standalone GET stream for server-initiated messages (e.g. list_changed).
    fn start_listener(&self) {
        {
            let mut started = self.listener_started.lock().unwrap();
            if *started {
                return;
            }
            *started = true;
        }
        let rb = self.request(reqwest::Method::GET).header(ACCEPT, "text/event-stream");
        let (tx, label, stop) = (self.in_tx.clone(), self.label.clone(), self.stop.clone());
        tokio::spawn(async move {
            let mut failures = 0u32;
            loop {
                let Some(attempt) = rb.try_clone() else { return };
                let r = tokio::select! {
                    _ = stop.cancelled() => return,
                    r = attempt.send() => r,
                };
                match r {
                    Ok(resp) if resp.status().is_success() && content_type(&resp).contains("text/event-stream") => {
                        failures = 0;
                        tokio::select! {
                            _ = stop.cancelled() => return,
                            r = forward_sse(resp, &tx) => {
                                if let Err(e) = r { tracing::debug!(server = %label, "GET stream error: {e:#}"); }
                            }
                        }
                    }
                    Ok(resp) => {
                        tracing::debug!(server = %label, "no standalone SSE stream (HTTP {})", resp.status());
                        return;
                    }
                    Err(e) => {
                        tracing::debug!(server = %label, "GET stream failed: {e}");
                        failures += 1;
                    }
                }
                if failures >= 5 {
                    return;
                }
                tokio::select! {
                    _ = stop.cancelled() => return,
                    _ = tokio::time::sleep(Duration::from_secs(1 << failures.min(5))) => {}
                }
            }
        });
    }

    pub async fn close(&self) {
        self.stop.cancel();
        if self.session.lock().unwrap().is_some() {
            let fut = self.request(reqwest::Method::DELETE).send();
            let _ = tokio::time::timeout(Duration::from_secs(2), fut).await;
        }
    }
}

impl Drop for Streamable {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

/// Forward every SSE `message` event of `resp` to the dispatcher until it ends.
async fn forward_sse(resp: Response, tx: &InTx) -> Result<()> {
    let mut parser = SseParser::default();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        for ev in parser.push(&chunk?) {
            forward_event(&ev.event, &ev.data, tx);
        }
    }
    if let Some(ev) = parser.finish() {
        forward_event(&ev.event, &ev.data, tx);
    }
    Ok(())
}

fn forward_event(event: &str, data: &str, tx: &InTx) {
    if !(event.is_empty() || event == "message") || data.trim().is_empty() {
        return;
    }
    match serde_json::from_str::<Value>(data) {
        Ok(v) => {
            let _ = tx.send(Incoming::Msg(v));
        }
        Err(_) => tracing::debug!("ignoring non-JSON SSE data: {}", truncate(data, 300)),
    }
}

// ---------------------------------------------------------------------------
// Legacy HTTP+SSE (protocol 2024-11-05)
// ---------------------------------------------------------------------------

pub(crate) struct LegacySse {
    client: reqwest::Client,
    endpoint: Url,
    headers: HeaderMap,
    stop: CancellationToken,
}

impl LegacySse {
    /// Open the GET event stream and wait for the `endpoint` event.
    pub async fn connect(label: &str, url: &str, headers: &IndexMap<String, String>, in_tx: InTx) -> Result<LegacySse> {
        let base = Url::parse(url).with_context(|| format!("invalid MCP url `{url}`"))?;
        let client = build_client(&base)?;
        let headers = build_headers(headers)?;
        let resp = client
            .get(base.clone())
            .headers(headers.clone())
            .header(ACCEPT, "text/event-stream")
            .send()
            .await
            .with_context(|| format!("GET {base} failed"))?;
        if !resp.status().is_success() {
            return Err(status_error(resp).await);
        }
        let ct = content_type(&resp);
        if !ct.contains("text/event-stream") {
            bail!("SSE endpoint returned content-type `{ct}`, expected text/event-stream");
        }
        let base_origin = base.origin();
        let stop = CancellationToken::new();
        let (ep_tx, ep_rx) = tokio::sync::oneshot::channel::<Result<Url>>();
        {
            let (label, stop) = (label.to_string(), stop.clone());
            tokio::spawn(async move {
                let mut ep_tx = Some(ep_tx);
                let mut parser = SseParser::default();
                let mut stream = resp.bytes_stream();
                let reason = loop {
                    let chunk = tokio::select! {
                        _ = stop.cancelled() => return,
                        c = stream.next() => c,
                    };
                    let events = match chunk {
                        Some(Ok(c)) => parser.push(&c),
                        Some(Err(e)) => break format!("SSE stream error: {e}"),
                        None => break "SSE stream ended".to_string(),
                    };
                    for ev in events {
                        if ev.event == "endpoint" {
                            let r = base.join(ev.data.trim()).with_context(|| format!("invalid endpoint `{}`", ev.data));
                            if let Some(tx) = ep_tx.take() {
                                let _ = tx.send(r);
                            }
                        } else {
                            forward_event(&ev.event, &ev.data, &in_tx);
                        }
                    }
                };
                tracing::debug!(server = %label, "{reason}");
                if let Some(tx) = ep_tx.take() {
                    let _ = tx.send(Err(anyhow!("{reason} before `endpoint` event")));
                }
                let _ = in_tx.send(Incoming::Closed(reason));
            });
        }
        let endpoint = ep_rx.await.map_err(|_| anyhow!("SSE reader stopped"))??;
        if endpoint.origin() != base_origin {
            stop.cancel();
            bail!("SSE endpoint origin {} does not match server origin", endpoint.origin().ascii_serialization());
        }
        Ok(LegacySse { client, endpoint, headers, stop })
    }

    pub async fn send(&self, msg: &Value) -> Result<()> {
        let resp = self
            .client
            .post(self.endpoint.clone())
            .headers(self.headers.clone())
            .header(CONTENT_TYPE, "application/json")
            .body(serde_json::to_vec(msg)?)
            .send()
            .await
            .with_context(|| format!("POST {} failed", self.endpoint))?;
        if !resp.status().is_success() {
            return Err(status_error(resp).await);
        }
        Ok(())
    }

    pub fn close(&self) {
        self.stop.cancel();
    }
}

impl Drop for LegacySse {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
