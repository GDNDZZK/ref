//! Test harness: a ref-web process in a private state directory, HTTP and WebSocket clients
//! that behave like a browser (Origin, the key handshake), a fake upstream and a real
//! ref-server.

// Each test file uses a different part of this module.
#![allow(dead_code)]

use std::io;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use rustls::pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsConnector;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

/// The page's half of the channel is the code ref-web uses, from the same file.
#[path = "../../src/channel.rs"]
pub mod channel;
#[path = "../../src/tls_pin.rs"]
pub mod tls_pin;

use channel::{Kind, NONCE_LEN, Nonce, Opener, Root, Sealer};

#[derive(Debug)]
pub enum TestStream {
    Plain(TcpStream),
    Tls(Box<tokio_rustls::client::TlsStream<TcpStream>>),
}

impl AsyncRead for TestStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            TestStream::Plain(s) => Pin::new(s).poll_read(cx, buf),
            TestStream::Tls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for TestStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            TestStream::Plain(s) => Pin::new(s).poll_write(cx, buf),
            TestStream::Tls(s) => Pin::new(s.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            TestStream::Plain(s) => Pin::new(s).poll_flush(cx),
            TestStream::Tls(s) => Pin::new(s.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            TestStream::Plain(s) => Pin::new(s).poll_shutdown(cx),
            TestStream::Tls(s) => Pin::new(s.as_mut()).poll_shutdown(cx),
        }
    }
}

pub type Ws = tokio_tungstenite::WebSocketStream<TestStream>;
const START_TIMEOUT: Duration = Duration::from_secs(15);
/// The loopback address the tests listen on and connect to.
pub const LOOPBACK: &str = "127.0.0.1";
pub const STEP_TIMEOUT: Duration = Duration::from_secs(10);
/// Poll intervals: for conditions that turn true quickly, and for a process that starts up.
pub const POLL_FAST: Duration = Duration::from_millis(20);
const POLL_SLOW: Duration = Duration::from_millis(50);
/// Sent by the fake upstream: close the connection with `FAKE_CLOSE_CODE` / `FAKE_CLOSE_REASON`.
pub const FAKE_CLOSE_TRIGGER: &str = "close-me";
pub const FAKE_CLOSE_CODE: u16 = 4001;
pub const FAKE_CLOSE_REASON: &str = "bye";

static COUNTER: AtomicU32 = AtomicU32::new(0);

pub fn temp_dir(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("ref-web-{tag}-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn free_port() -> u16 {
    std::net::TcpListener::bind((LOOPBACK, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Private state and data directories for ref-web (and a real ref-server).
pub struct Env {
    pub dir: PathBuf,
}

impl Drop for Env {
    fn drop(&mut self) {
        // Processes started from this Env are dropped first (declared later in each test).
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Env {
    pub fn new(tag: &str) -> Env {
        Env { dir: temp_dir(tag) }
    }

    pub fn state_dir(&self) -> PathBuf {
        self.dir.join("state")
    }

    /// `TMPDIR` of the processes we start: a mock backend creates its directory here.
    pub fn tmp_dir(&self) -> PathBuf {
        self.dir.join("tmp")
    }

    fn command(&self, args: &[&str]) -> Command {
        std::fs::create_dir_all(self.tmp_dir()).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_ref-web"));
        command
            .args(args)
            .env("TMPDIR", self.tmp_dir())
            .env("REF_STATE_DIR", self.state_dir())
            .env("REF_DATA_DIR", self.dir.join("data"));
        command
    }

    /// Runs ref-web to completion (for the "already running" and "port in use" cases).
    pub fn run(&self, args: &[&str]) -> std::process::Output {
        let log_id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let stdout_path = self.dir.join(format!("run-stdout-{log_id}.log"));
        let stderr_path = self.dir.join(format!("run-stderr-{log_id}.log"));
        let mut child = self
            .command(args)
            .stdout(Stdio::from(std::fs::File::create(&stdout_path).unwrap()))
            .stderr(Stdio::from(std::fs::File::create(&stderr_path).unwrap()))
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + START_TIMEOUT;
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                let out = std::fs::read_to_string(&stdout_path).unwrap_or_default();
                let err = std::fs::read_to_string(&stderr_path).unwrap_or_default();
                panic!(
                    "ref-web {args:?} did not exit within {START_TIMEOUT:?}\nstdout: {out}\nstderr: {err}"
                );
            }
            std::thread::sleep(POLL_SLOW);
        };
        std::process::Output {
            status,
            stdout: std::fs::read(&stdout_path).unwrap_or_default(),
            stderr: std::fs::read(&stderr_path).unwrap_or_default(),
        }
    }

    /// Starts ref-web in the background (it picks its own port) and waits until it listens.
    pub async fn start(&self, args: &[&str]) -> Web {
        self.start_with_port(None, args).await
    }

    /// Like `start`, on a given port.
    pub async fn start_on(&self, port: u16, args: &[&str]) -> Web {
        self.start_with_port(Some(port), args).await
    }

    async fn start_with_port(&self, port: Option<u16>, args: &[&str]) -> Web {
        match self.try_start(port, args).await {
            Ok(web) => web,
            Err(EarlyExit {
                status,
                stdout,
                stderr,
            }) => panic!("ref-web exited early: {status}\nstdout: {stdout}\nstderr: {stderr}"),
        }
    }

    /// `Err` holds the exit status and captured logs of a ref-web that stopped before it listened.
    async fn try_start(&self, port: Option<u16>, args: &[&str]) -> Result<Web, EarlyExit> {
        let mut all = vec!["--no-open".to_string()];
        if let Some(port) = port {
            all.extend(["--port".to_string(), port.to_string()]);
        }
        all.extend(args.iter().map(|a| a.to_string()));
        let all: Vec<&str> = all.iter().map(String::as_str).collect();
        let log_id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let stdout_path = self.dir.join(format!("stdout-{log_id}.log"));
        let stderr_path = self.dir.join(format!("stderr-{log_id}.log"));
        let child = self
            .command(&all)
            .stdout(Stdio::from(std::fs::File::create(&stdout_path).unwrap()))
            .stderr(Stdio::from(std::fs::File::create(&stderr_path).unwrap()))
            .spawn()
            .unwrap();
        let mut web = Web {
            child,
            host: LOOPBACK.to_string(),
            port: 0,
            tls: false,
            key: String::new(),
            state_dir: self.state_dir(),
            stdout_path,
            stderr_path,
            http: reqwest::Client::builder()
                .no_proxy()
                .danger_accept_invalid_certs(true)
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
        };
        let (port, host, tls) = web
            .wait_for_record(&self.state_dir())
            .await
            .map_err(|status| EarlyExit {
                status,
                stdout: web.stdout(),
                stderr: web.stderr(),
            })?;
        web.port = port;
        web.tls = tls;
        web.host = match host.as_str() {
            "0.0.0.0" => LOOPBACK.to_string(),
            "::" => "::1".to_string(),
            _ => host,
        };
        web.wait_until_listening()
            .await
            .map_err(|status| EarlyExit {
                status,
                stdout: web.stdout(),
                stderr: web.stderr(),
            })?;
        web.key = match args.iter().position(|a| *a == "--key") {
            Some(i) => args[i + 1].to_string(),
            None => std::fs::read_to_string(self.state_dir().join("web/login-token")).unwrap(),
        };
        Ok(web)
    }
}

struct EarlyExit {
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
}

pub struct Web {
    child: Child,
    pub host: String,
    pub port: u16,
    pub tls: bool,
    /// The key the page signs in with: `--key`, or the login token.
    pub key: String,
    state_dir: PathBuf,
    stdout_path: PathBuf,
    stderr_path: PathBuf,
    pub http: reqwest::Client,
}
impl Drop for Web {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Web {
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Waits for the process to exit by itself.
    pub async fn wait_exit(&mut self) -> std::process::ExitStatus {
        poll_until("ref-web to exit", STEP_TIMEOUT, POLL_FAST, async || {
            self.child.try_wait().unwrap()
        })
        .await
    }

    /// SIGKILL: no chance to clean up.
    pub fn kill_hard(&mut self) {
        self.child.kill().unwrap();
        self.child.wait().unwrap();
    }

    pub fn cert_path(&self) -> PathBuf {
        self.state_dir.join("web/tls/cert.pem")
    }

    pub fn stdout(&self) -> String {
        std::fs::read_to_string(&self.stdout_path).unwrap_or_default()
    }

    pub fn stderr(&self) -> String {
        std::fs::read_to_string(&self.stderr_path).unwrap_or_default()
    }

    /// The port, host and TLS flag ref-web chose, from its instance record (written once it has bound).
    async fn wait_for_record(
        &mut self,
        state_dir: &std::path::Path,
    ) -> Result<(u16, String, bool), std::process::ExitStatus> {
        let record = state_dir.join(format!("web/instances/{}.json", self.child.id()));
        poll_until(
            "ref-web to record its port",
            START_TIMEOUT,
            POLL_SLOW,
            async || {
                if let Some(status) = self.child.try_wait().unwrap() {
                    return Some(Err(status));
                }
                let text = std::fs::read_to_string(&record).ok()?;
                let record: serde_json::Value = serde_json::from_str(&text).ok()?;
                let port = record["port"].as_u64()? as u16;
                let host = record["host"].as_str()?.to_string();
                let tls = record["tls"].as_bool().unwrap_or(false);
                Some(Ok((port, host, tls)))
            },
        )
        .await
    }

    async fn wait_until_listening(&mut self) -> Result<(), std::process::ExitStatus> {
        poll_until("ref-web to start", START_TIMEOUT, POLL_SLOW, async || {
            if let Some(status) = self.child.try_wait().unwrap() {
                return Some(Err(status));
            }
            tokio::net::TcpStream::connect((self.host.as_str(), self.port))
                .await
                .is_ok()
                .then_some(Ok(()))
        })
        .await
    }

    pub fn url(&self, path: &str) -> String {
        let scheme = if self.tls { "https" } else { "http" };
        format!("{scheme}://{LOOPBACK}:{}{path}", self.port)
    }

    pub fn origin(&self) -> String {
        let scheme = if self.tls { "https" } else { "http" };
        format!("{scheme}://{LOOPBACK}:{}", self.port)
    }

    pub async fn get(&self, path: &str) -> reqwest::Response {
        self.http.get(self.url(path)).send().await.unwrap()
    }

    /// WebSocket handshake with the given headers (`None` = leave out / keep the default).
    pub async fn try_ws(
        &self,
        origin: Option<String>,
        host: Option<&str>,
    ) -> Result<Ws, tokio_tungstenite::tungstenite::Error> {
        let scheme = if self.tls { "wss" } else { "ws" };
        let mut request = format!("{scheme}://{LOOPBACK}:{}/ws", self.port)
            .into_client_request()
            .unwrap();
        let headers = request.headers_mut();
        if let Some(origin) = origin {
            headers.insert("origin", HeaderValue::from_str(&origin).unwrap());
        }
        if let Some(host) = host {
            headers.insert("host", HeaderValue::from_str(host).unwrap());
        }
        let tcp = TcpStream::connect((LOOPBACK, self.port)).await?;
        let stream = if self.tls {
            let cert_der = tls_pin::load_cert_der(&self.cert_path()).map_err(io::Error::other)?;
            let connector = TlsConnector::from(Arc::new(tls_pin::pinned_client_config(cert_der)));
            let server_name = ServerName::try_from("localhost").unwrap();
            let tls = connector.connect(server_name, tcp).await?;
            TestStream::Tls(Box::new(tls))
        } else {
            TestStream::Plain(tcp)
        };
        tokio_tungstenite::client_async(request, stream)
            .await
            .map(|(ws, _)| ws)
    }
    /// A WebSocket as the page opens it (same-origin Origin), before any handshake.
    pub async fn ws(&self) -> Ws {
        self.try_ws(Some(self.origin()), None).await.unwrap()
    }

    /// A tab that has signed in with the key.
    pub async fn channel(&self) -> Channel {
        match Channel::sign_in(self.ws().await, &self.key).await {
            Ok(channel) => channel,
            Err(close) => panic!("the key was refused: {close:?}"),
        }
    }
}

/// What a tab receives.
#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    Text(String),
    Binary(Vec<u8>),
    /// Close code and reason (1006 and nothing when the socket ended without a frame).
    Close(u16, String),
}

/// A tab as the page is: it did the key handshake and seals what it sends.
pub struct Channel {
    pub ws: Ws,
    sealer: Sealer,
    opener: Opener,
    /// Sealed frame 0: version, cwd, mock.
    pub info: serde_json::Value,
}

/// Close code and reason.
pub type Closed = (u16, String);

impl Channel {
    /// Reads the hello and answers it. `Err` holds the close the server answered with.
    pub async fn sign_in(mut ws: Ws, key: &str) -> Result<Channel, Closed> {
        let (root, sn, cn) = Self::start_handshake(&mut ws, key).await;
        let mac = root.auth_mac(&sn, &cn);
        ws.send(Message::text(channel::auth_json(&cn, &mac)))
            .await
            .unwrap();
        let (sealer, opener) = root.client_channel(&sn, &cn);
        let mut channel = Channel {
            ws,
            sealer,
            opener,
            info: serde_json::Value::Null,
        };
        match channel.next_event().await {
            Event::Text(frame_zero) => {
                channel.info = serde_json::from_str(&frame_zero).unwrap();
                Ok(channel)
            }
            Event::Close(code, reason) => Err((code, reason)),
            other => panic!("frame 0 should be text, got {other:?}"),
        }
    }

    /// Reads the hello; returns the root for `key` and both nonces (the client's is fixed).
    pub async fn start_handshake(ws: &mut Ws, key: &str) -> (Root, Nonce, Nonce) {
        let hello = next_text(ws).await;
        let (salt, sn) = channel::parse_hello(&hello).expect("a hello message");
        let root = Root::derive(key.as_bytes(), &salt);
        (root, sn, [0x5a; NONCE_LEN])
    }

    pub async fn send_text(&mut self, text: &str) {
        self.send_sealed(Kind::Text, text.as_bytes()).await;
    }

    pub async fn send_binary(&mut self, data: &[u8]) {
        self.send_sealed(Kind::Binary, data).await;
    }

    async fn send_sealed(&mut self, kind: Kind, data: &[u8]) {
        let wire = self.seal(kind, data);
        self.send_wire(wire).await;
    }

    /// Seals the next frame without sending it (to tamper with it, repeat it or reorder it).
    pub fn seal(&mut self, kind: Kind, data: &[u8]) -> Vec<u8> {
        self.sealer.seal(kind, data)
    }

    pub async fn send_wire(&mut self, wire: Vec<u8>) {
        let _ = self.ws.send(Message::Binary(wire.into())).await;
    }

    /// The next data frame, opened, or the close.
    pub async fn next_event(&mut self) -> Event {
        loop {
            let message = match tokio::time::timeout(STEP_TIMEOUT, self.ws.next()).await {
                Err(_) => panic!("timed out waiting for a frame"),
                Ok(None | Some(Err(_))) => return Event::Close(1006, String::new()),
                Ok(Some(Ok(message))) => message,
            };
            match message {
                Message::Binary(wire) => {
                    let (kind, plain) = self.opener.open(&wire).expect("a frame that opens");
                    return match kind {
                        Kind::Text => Event::Text(String::from_utf8(plain).unwrap()),
                        Kind::Binary => Event::Binary(plain),
                    };
                }
                Message::Close(frame) => {
                    return match frame {
                        Some(frame) => {
                            Event::Close(u16::from(frame.code), frame.reason.to_string())
                        }
                        None => Event::Close(1005, String::new()),
                    };
                }
                Message::Ping(_) | Message::Pong(_) => {}
                other => panic!("unexpected plain message after the handshake: {other:?}"),
            }
        }
    }

    pub async fn next_text(&mut self) -> String {
        match self.next_event().await {
            Event::Text(text) => text,
            other => panic!("expected text, got {other:?}"),
        }
    }

    pub async fn next_close(&mut self) -> Closed {
        match self.next_event().await {
            Event::Close(code, reason) => (code, reason),
            other => panic!("expected a close, got {other:?}"),
        }
    }
}

pub async fn next_message(ws: &mut Ws) -> Message {
    tokio::time::timeout(STEP_TIMEOUT, ws.next())
        .await
        .expect("timed out waiting for a frame")
        .expect("socket ended")
        .expect("socket error")
}

pub async fn next_text(ws: &mut Ws) -> String {
    match next_message(ws).await {
        Message::Text(t) => t.to_string(),
        other => panic!("expected text, got {other:?}"),
    }
}

/// What the fake upstream saw and did.
pub struct FakeUpstream {
    pub port: u16,
    /// Query string of the handshake request (`token=…`).
    pub handshake_query: tokio::sync::watch::Receiver<Option<String>>,
}

/// A WebSocket server that echoes every text and binary frame unchanged, and closes the
/// connection when it gets `FAKE_CLOSE_TRIGGER`.
pub async fn fake_upstream() -> FakeUpstream {
    let listener = TcpListener::bind((LOOPBACK, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (query_tx, handshake_query) = tokio::sync::watch::channel(None);
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let query_tx = query_tx.clone();
            tokio::spawn(async move {
                // The callback signature (and its large error type) is fixed by tungstenite.
                #[allow(clippy::result_large_err)]
                let callback =
                    |request: &tokio_tungstenite::tungstenite::handshake::server::Request,
                     response| {
                        let query = request.uri().query().map(str::to_string);
                        query_tx.send(query).unwrap();
                        Ok(response)
                    };
                let mut ws = tokio_tungstenite::accept_hdr_async(stream, callback)
                    .await
                    .unwrap();
                while let Some(Ok(message)) = ws.next().await {
                    let reply = match message {
                        Message::Text(t) if t.as_str() == FAKE_CLOSE_TRIGGER => {
                            let close = CloseFrame {
                                code: CloseCode::from(FAKE_CLOSE_CODE),
                                reason: FAKE_CLOSE_REASON.into(),
                            };
                            ws.send(Message::Close(Some(close))).await.unwrap();
                            return;
                        }
                        Message::Text(_) | Message::Binary(_) => message,
                        _ => continue,
                    };
                    if ws.send(reply).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    FakeUpstream {
        port,
        handshake_query,
    }
}

/// A real ref-server in this process, requiring `token`.
pub async fn real_backend(env: &Env, token: &str) -> (ref_server::Running, u16) {
    let mut options = ref_server::HubOptions::new(env.dir.join("server-data"));
    options.home = None;
    options.config_loader = std::sync::Arc::new(|_| ref_config::Config::default());
    let (auth, _) = ref_server::ws::resolve_hosts(&[LOOPBACK.into()], token.into());
    let running = ref_server::start(options, LOOPBACK, 0, auth).await.unwrap();
    let port = running.addr.port();
    (running, port)
}

/// Fails with a hint unless `ref-server` sits next to `ref-web`, where ref-web looks for it.
/// `cargo test -p ref-web` does not build the binaries of other packages, and a test must not
/// run `cargo build` from inside cargo.
pub fn require_ref_server() {
    let web = PathBuf::from(env!("CARGO_BIN_EXE_ref-web"));
    let server = web.with_file_name(format!("ref-server{}", std::env::consts::EXE_SUFFIX));
    assert!(
        server.exists(),
        "{} is missing: run `cargo build -p ref-server` first, or `cargo test --workspace`",
        server.display()
    );
}

/// Direct children of `parent` (`pgrep -P`; the process tests are Linux-only).
#[cfg(target_os = "linux")]
pub fn child_pids(parent: u32) -> Vec<u32> {
    let output = Command::new("pgrep")
        .args(["-P", &parent.to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|l| l.trim().parse().ok())
        .collect()
}

/// True once `pid` is gone or only a zombie waiting to be reaped (reads `/proc`).
#[cfg(target_os = "linux")]
pub fn process_gone(pid: u32) -> bool {
    if !ref_sys::pid_alive(pid) {
        return true;
    }
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
    stat.rsplit(')')
        .next()
        .is_some_and(|rest| rest.trim_start().starts_with('Z'))
}

/// Polls `check` every `poll` until it returns `Some`; panics after `timeout`.
async fn poll_until<T>(
    what: &str,
    timeout: Duration,
    poll: Duration,
    mut check: impl AsyncFnMut() -> Option<T>,
) -> T {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Some(found) = check().await {
            return found;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(poll).await;
    }
}

pub async fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    poll_until(what, STEP_TIMEOUT, POLL_FAST, async || {
        condition().then_some(())
    })
    .await;
}
