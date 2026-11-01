//! Test harness: a ref-web process in a private state directory, HTTP and WebSocket clients
//! that behave like a browser (cookie, Origin), a fake upstream and a real ref-server.

// Each test file uses a different part of this module.
#![allow(dead_code)]

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

pub type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

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
        self.command(args).output().unwrap()
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
            Err(status) => panic!("ref-web exited early: {status}"),
        }
    }

    /// `Err` holds the exit status of a ref-web that stopped before it listened.
    async fn try_start(
        &self,
        port: Option<u16>,
        args: &[&str],
    ) -> Result<Web, std::process::ExitStatus> {
        let mut all = vec!["--no-open".to_string()];
        if let Some(port) = port {
            all.extend(["--port".to_string(), port.to_string()]);
        }
        all.extend(args.iter().map(|a| a.to_string()));
        let all: Vec<&str> = all.iter().map(String::as_str).collect();
        let child = self
            .command(&all)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut web = Web {
            child,
            port: 0,
            token: String::new(),
            session_cookie: String::new(),
            http: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
        };
        web.port = web.wait_for_record(&self.state_dir()).await?;
        web.wait_until_listening().await?;
        web.token = std::fs::read_to_string(self.state_dir().join("web/login-token")).unwrap();
        web.session_cookie = web.log_in().await;
        Ok(web)
    }
}

pub struct Web {
    child: Child,
    pub port: u16,
    pub token: String,
    /// `name=value` of the session cookie that `/login` handed out.
    session_cookie: String,
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

    /// The port ref-web chose, from its instance record (written once it has bound).
    async fn wait_for_record(
        &mut self,
        state_dir: &std::path::Path,
    ) -> Result<u16, std::process::ExitStatus> {
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
                Some(Ok(record["port"].as_u64()? as u16))
            },
        )
        .await
    }

    async fn wait_until_listening(&mut self) -> Result<(), std::process::ExitStatus> {
        poll_until("ref-web to start", START_TIMEOUT, POLL_SLOW, async || {
            if let Some(status) = self.child.try_wait().unwrap() {
                return Some(Err(status));
            }
            tokio::net::TcpStream::connect((LOOPBACK, self.port))
                .await
                .is_ok()
                .then_some(Ok(()))
        })
        .await
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{LOOPBACK}:{}{path}", self.port)
    }

    pub fn cookie(&self) -> String {
        self.session_cookie.clone()
    }

    /// Name of the session cookie.
    pub fn cookie_name(&self) -> &str {
        self.session_cookie.split('=').next().unwrap()
    }

    async fn log_in(&self) -> String {
        let response = self
            .http
            .get(self.url(&format!("/login?token={}", self.token)))
            .send()
            .await
            .unwrap();
        let set_cookie = response.headers()["set-cookie"].to_str().unwrap();
        set_cookie.split(';').next().unwrap().to_string()
    }

    pub fn origin(&self) -> String {
        format!("http://{LOOPBACK}:{}", self.port)
    }

    /// A GET with the login cookie.
    pub async fn get(&self, path: &str) -> reqwest::Response {
        self.http
            .get(self.url(path))
            .header("cookie", self.cookie())
            .send()
            .await
            .unwrap()
    }

    /// WebSocket handshake with the given headers (`None` = leave out / keep the default).
    pub async fn try_ws(
        &self,
        cookie: Option<String>,
        origin: Option<String>,
        host: Option<&str>,
    ) -> Result<Ws, tokio_tungstenite::tungstenite::Error> {
        let mut request = format!("ws://{LOOPBACK}:{}/ws", self.port)
            .into_client_request()
            .unwrap();
        let headers = request.headers_mut();
        if let Some(cookie) = cookie {
            headers.insert("cookie", HeaderValue::from_str(&cookie).unwrap());
        }
        if let Some(origin) = origin {
            headers.insert("origin", HeaderValue::from_str(&origin).unwrap());
        }
        if let Some(host) = host {
            headers.insert("host", HeaderValue::from_str(host).unwrap());
        }
        tokio_tungstenite::connect_async(request)
            .await
            .map(|(ws, _)| ws)
    }

    /// A WebSocket as the page opens it: login cookie and same-origin Origin.
    pub async fn ws(&self) -> Ws {
        self.try_ws(Some(self.cookie()), Some(self.origin()), None)
            .await
            .unwrap()
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
