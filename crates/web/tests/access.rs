//! Access control and process behavior of ref-web as a whole: token file, `--key`, Host
//! handling on the wire, and stopping on SIGTERM.

mod common;

use common::*;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

/// A backend address nothing listens on; these tests never open a tab.
const NO_BACKEND: &str = "ws://127.0.0.1:1/";
const TOKEN_FILE: &str = "web/login-token";
const VALID_TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
/// Long enough for a ref-web that accepts its arguments to print its address.
const PORT_ZERO_SETTLE: std::time::Duration = std::time::Duration::from_millis(1500);
/// How long the half-written token stays unfinished in the race test.
const HALF_WRITTEN_FOR: std::time::Duration = std::time::Duration::from_millis(300);

fn write_token_file(env: &Env, content: &str) -> std::path::PathBuf {
    let path = env.state_dir().join(TOKEN_FILE);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, content).unwrap();
    path
}

#[cfg(unix)]
fn mode_of(path: &std::path::Path) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[cfg(unix)]
#[tokio::test]
async fn the_token_file_is_owner_only_even_when_it_existed_with_wider_rights() {
    use std::os::unix::fs::PermissionsExt as _;

    let fresh = Env::new("token-mode");
    let _web = fresh.start(&["--server", NO_BACKEND]).await;
    assert_eq!(mode_of(&fresh.state_dir().join(TOKEN_FILE)), 0o600);

    let old = Env::new("token-mode-old");
    let path = write_token_file(&old, VALID_TOKEN);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    let web = old.start(&["--server", NO_BACKEND]).await;
    assert_eq!(mode_of(&path), 0o600);
    assert_eq!(web.key, VALID_TOKEN, "the token itself is kept");
}

#[tokio::test]
async fn a_token_file_that_never_gets_finished_is_an_error() {
    let env = Env::new("token-broken");
    write_token_file(&env, "abc");
    let port = free_port().to_string();
    let output = env.run(&["--no-open", "--port", &port, "--server", NO_BACKEND]);
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("does not hold a valid login token"), "{err}");
}

#[tokio::test]
async fn a_token_file_that_is_being_written_is_waited_for() {
    let env = Env::new("token-race");
    let path = write_token_file(&env, "abc");
    let finish = async {
        tokio::time::sleep(HALF_WRITTEN_FOR).await;
        std::fs::write(&path, VALID_TOKEN).unwrap();
    };
    let (web, ()) = tokio::join!(env.start(&["--server", NO_BACKEND]), finish);
    assert_eq!(web.key, VALID_TOKEN);
}

/// Sends raw bytes and returns what the server answers until it closes the connection.
async fn raw_request(port: u16, request: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect((LOOPBACK, port))
        .await
        .unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut answer = Vec::new();
    tokio::time::timeout(STEP_TIMEOUT, stream.read_to_end(&mut answer))
        .await
        .expect("the server kept the connection open")
        .unwrap();
    String::from_utf8_lossy(&answer).into_owned()
}

#[tokio::test]
async fn a_request_without_host_is_refused() {
    let env = Env::new("no-host");
    let web = env.start(&["--server", NO_BACKEND]).await;
    // The ping route is open, so only the Host check can refuse this.
    let answer = raw_request(web.port, "GET /api/ping?nonce=00 HTTP/1.0\r\n\r\n").await;
    assert!(
        answer.starts_with("HTTP/1.") && answer.contains(" 403 "),
        "{answer}"
    );
    let with_host = format!(
        "GET /api/ping?nonce=00 HTTP/1.0\r\nHost: {LOOPBACK}:{}\r\n\r\n",
        web.port
    );
    assert!(raw_request(web.port, &with_host).await.contains(" 200 "));
}

const SPOKEN_KEY: &str = "a key typed on the command line";

#[tokio::test]
async fn a_key_from_the_command_line_is_used_and_never_written() {
    let env = Env::new("own-key");
    let upstream = fake_upstream().await;
    let server = format!("ws://{LOOPBACK}:{}/", upstream.port);
    let web = env.start(&["--server", &server, "--key", SPOKEN_KEY]).await;
    assert_eq!(web.key, SPOKEN_KEY);
    web.channel().await;
    let wrong = Channel::sign_in(web.ws().await, "not the key at all, no").await;
    assert_eq!(wrong.err().map(|(code, _)| code), Some(4401));

    let web_dir = env.state_dir().join("web");
    assert!(
        !web_dir.join("login-token").exists(),
        "the token file is not created"
    );
    assert!(web_dir.join("salt").exists());
    for entry in std::fs::read_dir(&web_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            let text = std::fs::read(&path).unwrap();
            let leaked = text
                .windows(SPOKEN_KEY.len())
                .any(|w| w == SPOKEN_KEY.as_bytes());
            assert!(!leaked, "{} holds the key", path.display());
        }
    }
    #[cfg(unix)]
    assert_eq!(mode_of(&web_dir.join("salt")), 0o600);
}

#[tokio::test]
async fn a_short_key_is_refused_from_the_option_and_from_the_environment() {
    let env = Env::new("short-key");
    let output = env.run(&["--no-open", "--server", NO_BACKEND, "--key", "too short"]);
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("at least 16 characters"), "{err}");
    assert!(err.contains("Leave it out"), "{err}");
    assert!(!err.contains("too short"), "the key is not echoed: {err}");

    let from_env = std::process::Command::new(env!("CARGO_BIN_EXE_ref-web"))
        .args(["--no-open", "--server", NO_BACKEND])
        .env("REF_WEB_KEY", "short")
        .env("REF_STATE_DIR", env.state_dir())
        .env("REF_DATA_DIR", env.dir.join("data"))
        .output()
        .unwrap();
    assert!(!from_env.status.success());
    let err = String::from_utf8_lossy(&from_env.stderr);
    assert!(err.contains("at least 16 characters"), "{err}");
    assert!(!env.state_dir().join("web/login-token").exists());
}

#[tokio::test]
async fn a_wide_listen_address_still_only_serves_ip_literals_on_its_own_port() {
    let env = Env::new("wide-host");
    let web = env
        .start(&["--host", "0.0.0.0", "--server", NO_BACKEND])
        .await;
    let port = web.port;
    let status = |host: String| {
        let web = &web;
        async move {
            web.http
                .get(web.url("/api/ping?nonce=00ff"))
                .header("host", host)
                .send()
                .await
                .unwrap()
                .status()
                .as_u16()
        }
    };
    for allowed in [
        format!("0.0.0.0:{port}"),
        format!("192.168.1.20:{port}"),
        format!("[::1]:{port}"),
        format!("localhost:{port}"),
    ] {
        assert_eq!(status(allowed.clone()).await, 200, "{allowed}");
    }
    for refused in [
        format!("evil.example:{port}"),
        "evil.example".to_string(),
        format!("192.168.1.20:{}", port.wrapping_add(1)),
    ] {
        assert_eq!(status(refused.clone()).await, 403, "{refused}");
    }
}

#[cfg(unix)]
#[tokio::test]
async fn sigterm_closes_the_tabs_and_exits_successfully() {
    let env = Env::new("sigterm");
    let upstream = fake_upstream().await;
    let server = format!("ws://{LOOPBACK}:{}/", upstream.port);
    let mut web = env.start(&["--server", &server]).await;
    let mut tab = web.channel().await;
    // A round trip proves the tunnel is up before ref-web is told to stop.
    tab.send_text("ping").await;
    assert_eq!(tab.next_text().await, "ping");

    assert!(ref_sys::terminate(web.pid()));
    let status = web.wait_exit().await;
    assert!(status.success(), "{status}");
    assert_eq!(tab.next_close().await.0, 1001);
}

#[tokio::test]
async fn port_zero_is_rejected_before_anything_starts() {
    let env = Env::new("port-zero");
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_ref-web"))
        .args(["--no-open", "--port", "0", "--server", NO_BACKEND])
        .env("REF_STATE_DIR", env.state_dir())
        .env("REF_DATA_DIR", env.dir.join("data"))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // A ref-web that accepted port 0 would keep running; give it a moment, then stop it.
    let deadline = std::time::Instant::now() + PORT_ZERO_SETTLE;
    let mut exited = false;
    while !exited && std::time::Instant::now() < deadline {
        tokio::time::sleep(POLL_FAST).await;
        exited = child.try_wait().unwrap().is_some();
    }
    if !exited {
        child.kill().unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert!(exited, "ref-web --port 0 kept running");
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("--port"), "{err}");
    assert!(
        output.stdout.is_empty(),
        "no address for port 0: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}
