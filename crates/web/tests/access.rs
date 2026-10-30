//! Access control and process behavior of ref-web as a whole: token file, Host and cookie
//! handling on the wire, and stopping on SIGTERM.

mod common;

use common::*;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

/// A backend address nothing listens on; these tests never open a tab.
const NO_BACKEND: &str = "ws://127.0.0.1:1/";
const TOKEN_FILE: &str = "web/login-token";
const VALID_TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
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
    assert_eq!(web.token, VALID_TOKEN, "the token itself is kept");
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
    assert_eq!(web.token, VALID_TOKEN);
}

/// Sends raw bytes and returns what the server answers until it closes the connection.
async fn raw_request(port: u16, request: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
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
    // The ping route needs no login, so only the Host check can refuse this.
    let answer = raw_request(web.port, "GET /api/ping?nonce=00 HTTP/1.0\r\n\r\n").await;
    assert!(
        answer.starts_with("HTTP/1.") && answer.contains(" 403 "),
        "{answer}"
    );
    let with_host = format!(
        "GET /api/ping?nonce=00 HTTP/1.0\r\nHost: 127.0.0.1:{}\r\n\r\n",
        web.port
    );
    assert!(raw_request(web.port, &with_host).await.contains(" 200 "));
}

#[tokio::test]
async fn any_matching_cookie_logs_in_when_the_name_is_repeated() {
    let env = Env::new("cookies");
    let web = env.start(&["--server", NO_BACKEND]).await;
    let name = web.cookie_name().to_string();
    let wrong = format!("{name}=wrong");
    let status = |cookies: Vec<String>| {
        let web = &web;
        async move {
            let mut request = web.http.get(web.url("/api/info"));
            for line in cookies {
                request = request.header("cookie", line);
            }
            request.send().await.unwrap().status().as_u16()
        }
    };
    let both_in_one_line = format!("{wrong}; {}", web.cookie());
    assert_eq!(status(vec![both_in_one_line]).await, 200);
    assert_eq!(status(vec![wrong.clone(), web.cookie()]).await, 200);
    assert_eq!(status(vec![web.cookie(), wrong.clone()]).await, 200);
    assert_eq!(status(vec![wrong.clone(), wrong.clone()]).await, 401);
    assert_eq!(status(vec![format!("{wrong}; {wrong}")]).await, 401);
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
                .get(web.url("/api/info"))
                .header("cookie", web.cookie())
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
    use tokio_tungstenite::tungstenite::Message;

    let env = Env::new("sigterm");
    let upstream = fake_upstream().await;
    let server = format!("ws://127.0.0.1:{}/", upstream.port);
    let mut web = env.start(&["--server", &server]).await;
    let mut tab = web.ws().await;
    // A round trip proves the tunnel is up before ref-web is told to stop.
    futures_util::SinkExt::send(&mut tab, Message::text("ping"))
        .await
        .unwrap();
    assert_eq!(next_text(&mut tab).await, "ping");

    assert!(ref_sys::terminate(web.pid()));
    let status = web.wait_exit().await;
    assert!(status.success(), "{status}");
    match next_message(&mut tab).await {
        Message::Close(Some(frame)) => assert_eq!(u16::from(frame.code), 1001),
        other => panic!("expected a going-away close, got {other:?}"),
    }
}
