mod common;

use std::io::Read as _;

use common::*;
use futures_util::SinkExt as _;
use tokio_tungstenite::tungstenite::Error as WsError;
use tokio_tungstenite::tungstenite::Message;

const BACKEND_TOKEN: &str = "backend-secret";

fn status_of(error: WsError) -> u16 {
    match error {
        WsError::Http(response) => response.status().as_u16(),
        other => panic!("expected an HTTP rejection, got {other:?}"),
    }
}

fn dist_file(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/dist");
    std::fs::read(path.join(name)).unwrap()
}

/// ref-web in front of a fake upstream that echoes frames.
async fn web_with_fake_upstream(env: &Env) -> (Web, FakeUpstream) {
    let upstream = fake_upstream().await;
    let server = format!("ws://127.0.0.1:{}/", upstream.port);
    let web = env
        .start(&["--server", &server, "--token", BACKEND_TOKEN])
        .await;
    (web, upstream)
}

#[tokio::test]
async fn login_trades_the_token_for_a_cookie() {
    let env = Env::new("login");
    let (web, _upstream) = web_with_fake_upstream(&env).await;

    let page = web.http.get(web.url("/")).send().await.unwrap();
    assert_eq!(page.status(), 401, "pages need a login");

    let wrong = web
        .http
        .get(web.url("/login?token=nope"))
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), 403);
    assert!(wrong.headers().get("set-cookie").is_none());

    let login_url = format!("/login?token={}&dir=/some%20dir", web.token);
    let login = web.http.get(web.url(&login_url)).send().await.unwrap();
    assert_eq!(login.status(), 303);
    let location = login.headers()["location"].to_str().unwrap();
    assert_eq!(location, "/?dir=/some%20dir");
    assert!(
        !location.contains(&web.token),
        "no token in the address bar"
    );
    let cookie = login.headers()["set-cookie"].to_str().unwrap();
    assert!(
        cookie.starts_with(&format!("ref_web_{}=", web.port)),
        "the name carries the port: {cookie}"
    );
    assert!(
        !cookie.contains(&web.token),
        "the cookie is not the login token: {cookie}"
    );
    assert!(
        cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict"),
        "{cookie}"
    );
    assert_eq!(login.headers()["referrer-policy"], "no-referrer");

    let page = web.get("/").await;
    assert_eq!(page.status(), 200);
    assert!(
        page.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/html")
    );
    let csp = page.headers()["content-security-policy"].to_str().unwrap();
    assert!(
        csp.starts_with(
            "default-src 'self'; style-src 'self'; img-src 'self' data: blob:; connect-src 'self'"
        ),
        "{csp}"
    );
    assert!(!csp.contains("unsafe"), "{csp}");

    let info: serde_json::Value = web.get("/api/info").await.json().await.unwrap();
    assert_eq!(
        info["cwd"],
        std::env::current_dir().unwrap().to_str().unwrap()
    );
    assert_eq!(info["mock"], false);
    let anonymous = web.http.get(web.url("/api/info")).send().await.unwrap();
    assert_eq!(anonymous.status(), 401);

    let ping: serde_json::Value = web
        .http
        .get(web.url("/api/ping?nonce=00ff"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(ping["app"], "ref-web");
    assert_eq!(ping["proof"].as_str().unwrap().len(), 64, "{ping}");
    let other_nonce: serde_json::Value = web
        .http
        .get(web.url("/api/ping?nonce=00fe"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_ne!(ping["proof"], other_nonce["proof"]);
    for bad in ["/api/ping", "/api/ping?nonce=", "/api/ping?nonce=zz"] {
        let response = web.http.get(web.url(bad)).send().await.unwrap();
        assert_eq!(response.status(), 400, "{bad}");
    }
}

#[tokio::test]
async fn static_files_are_served_compressed_when_accepted() {
    let env = Env::new("static");
    let (web, _upstream) = web_with_fake_upstream(&env).await;
    let original = dist_file("index.html");

    let plain = web
        .http
        .get(web.url("/index.html"))
        .header("cookie", web.cookie())
        .header("accept-encoding", "identity")
        .send()
        .await
        .unwrap();
    assert!(plain.headers().get("content-encoding").is_none());
    assert_eq!(plain.bytes().await.unwrap().as_ref(), original.as_slice());

    let negotiated = web
        .http
        .get(web.url("/index.html"))
        .header("cookie", web.cookie())
        .header("accept-encoding", "br, gzip")
        .send()
        .await
        .unwrap();
    let encoding = negotiated
        .headers()
        .get("content-encoding")
        .map(|v| v.to_str().unwrap().to_string());
    let body = negotiated.bytes().await.unwrap();
    let mut decoded = Vec::new();
    match encoding.as_deref() {
        None => decoded = body.to_vec(),
        Some("br") => {
            brotli::Decompressor::new(body.as_ref(), 4096)
                .read_to_end(&mut decoded)
                .unwrap();
        }
        Some("gzip") => {
            flate2::read::GzDecoder::new(body.as_ref())
                .read_to_end(&mut decoded)
                .unwrap();
        }
        Some(other) => panic!("unexpected encoding {other}"),
    }
    assert_eq!(decoded, original);

    assert_eq!(web.get("/no/such/file.js").await.status(), 404);
    assert_eq!(
        web.get("/some/page/route").await.status(),
        200,
        "SPA fallback"
    );
}

#[tokio::test]
async fn websocket_checks_cookie_origin_and_host() {
    let env = Env::new("auth");
    let (web, _upstream) = web_with_fake_upstream(&env).await;

    let no_cookie = web
        .try_ws(None, Some(web.origin()), None)
        .await
        .err()
        .unwrap();
    assert_eq!(status_of(no_cookie), 401);
    let bad_cookie = web
        .try_ws(
            Some(format!("{}=wrong", web.cookie_name())),
            Some(web.origin()),
            None,
        )
        .await
        .err()
        .unwrap();
    assert_eq!(status_of(bad_cookie), 401);
    let foreign_origin = web
        .try_ws(Some(web.cookie()), Some("http://evil.example".into()), None)
        .await
        .err()
        .unwrap();
    assert_eq!(status_of(foreign_origin), 403);
    let other_port = web
        .try_ws(
            Some(web.cookie()),
            Some("http://127.0.0.1:5173".into()),
            None,
        )
        .await
        .err()
        .unwrap();
    assert_eq!(status_of(other_port), 403);
    let no_origin = web
        .try_ws(Some(web.cookie()), None, None)
        .await
        .err()
        .unwrap();
    assert_eq!(status_of(no_origin), 403);
    let rebinding = web
        .try_ws(
            Some(web.cookie()),
            Some("http://rebind.example:7480".into()),
            Some("rebind.example:7480"),
        )
        .await
        .err()
        .unwrap();
    assert_eq!(status_of(rebinding), 403);

    let wrong_host = web
        .http
        .get(web.url("/api/info"))
        .header("cookie", web.cookie())
        .header("host", "rebind.example")
        .send()
        .await
        .unwrap();
    assert_eq!(wrong_host.status(), 403, "Host is checked on every route");

    assert!(
        web.try_ws(Some(web.cookie()), Some(web.origin()), None)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn frames_are_forwarded_unchanged_and_the_backend_token_is_added() {
    let env = Env::new("forward");
    let (web, upstream) = web_with_fake_upstream(&env).await;
    let mut ws = web.ws().await;

    let texts = [
        r#"{"id":1,"op":"hello",   "cwd":"/x","caf\u00e9":"\ud83d\ude00","n":1.0}"#,
        "  not json at all\n",
        r#"{"kind":"future_frame"}"#,
        "",
    ];
    for text in texts {
        ws.send(Message::text(text)).await.unwrap();
        assert_eq!(next_text(&mut ws).await, text);
    }
    let binary = vec![0u8, 1, 2, 255];
    ws.send(Message::binary(binary.clone())).await.unwrap();
    match next_message(&mut ws).await {
        Message::Binary(b) => assert_eq!(b.as_ref(), binary.as_slice()),
        other => panic!("expected binary, got {other:?}"),
    }

    let query = upstream.handshake_query.borrow().clone().unwrap();
    assert_eq!(query, format!("token={BACKEND_TOKEN}"));
}

#[tokio::test]
async fn each_tab_gets_its_own_upstream_connection() {
    let env = Env::new("tabs");
    let (web, _upstream) = web_with_fake_upstream(&env).await;
    let (mut first, mut second) = (web.ws().await, web.ws().await);
    first.send(Message::text("one")).await.unwrap();
    second.send(Message::text("two")).await.unwrap();
    assert_eq!(next_text(&mut first).await, "one");
    assert_eq!(next_text(&mut second).await, "two");
}

#[tokio::test]
async fn upstream_close_is_passed_on() {
    let env = Env::new("close");
    let (web, _upstream) = web_with_fake_upstream(&env).await;
    let mut ws = web.ws().await;
    ws.send(Message::text(FAKE_CLOSE_TRIGGER)).await.unwrap();
    match next_message(&mut ws).await {
        Message::Close(Some(frame)) => {
            assert_eq!(u16::from(frame.code), FAKE_CLOSE_CODE);
            assert_eq!(frame.reason.as_str(), FAKE_CLOSE_REASON);
        }
        other => panic!("expected a close frame, got {other:?}"),
    }
}

#[tokio::test]
async fn unreachable_backend_closes_the_tab_with_the_reason() {
    let env = Env::new("down");
    let dead = format!("ws://127.0.0.1:{}/", free_port());
    let web = env.start(&["--server", &dead]).await;
    let mut ws = web.ws().await;
    match next_message(&mut ws).await {
        Message::Close(Some(frame)) => {
            assert_eq!(u16::from(frame.code), 1011);
            assert!(
                frame
                    .reason
                    .starts_with("cannot connect to ws://127.0.0.1:"),
                "{}",
                frame.reason
            );
        }
        other => panic!("expected a close frame, got {other:?}"),
    }
}

#[tokio::test]
async fn a_real_backend_accepts_a_hello_without_token() {
    let env = Env::new("real");
    let (_backend, backend_port) = real_backend(&env, BACKEND_TOKEN).await;
    // No trailing slash: the form users type.
    let server = format!("ws://127.0.0.1:{backend_port}");
    let web = env
        .start(&["--server", &server, "--token", BACKEND_TOKEN])
        .await;
    let mut ws = web.ws().await;

    let hello = r#"{"id":1,"op":"hello","client":"ref-web","version":2,"cwd":"/tmp"}"#;
    ws.send(Message::text(hello)).await.unwrap();
    let response: serde_json::Value = serde_json::from_str(&next_text(&mut ws).await).unwrap();
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["result"]["protocol"], ref_proto::PROTOCOL_VERSION);

    ws.send(Message::text(r#"{"id":2,"op":"get_config"}"#))
        .await
        .unwrap();
    loop {
        let frame: serde_json::Value = serde_json::from_str(&next_text(&mut ws).await).unwrap();
        if frame["id"] == 2 {
            assert_eq!(frame["ok"], true, "{frame}");
            break;
        }
    }
}

#[tokio::test]
async fn a_wrong_backend_token_is_reported_by_the_backend() {
    let env = Env::new("badtoken");
    let (_backend, backend_port) = real_backend(&env, BACKEND_TOKEN).await;
    let server = format!("ws://127.0.0.1:{backend_port}/");
    let web = env.start(&["--server", &server, "--token", "wrong"]).await;
    let mut ws = web.ws().await;
    match next_message(&mut ws).await {
        Message::Close(Some(frame)) => assert_eq!(u16::from(frame.code), 1011),
        other => panic!("expected a close frame, got {other:?}"),
    }
}

#[tokio::test]
async fn a_second_start_opens_the_running_instance() {
    let env = Env::new("single");
    let (web, _upstream) = web_with_fake_upstream(&env).await;
    let port = web.port.to_string();
    let other_dir = env.dir.canonicalize().unwrap();

    let second = env.run(&["--no-open", "--port", &port, other_dir.to_str().unwrap()]);
    assert!(second.status.success(), "{second:?}");
    let out = String::from_utf8_lossy(&second.stdout);
    assert!(out.contains("already running"), "{out}");
    assert!(out.contains(&format!("token={}", web.token)), "{out}");
    assert!(
        out.contains(&format!("dir={}", other_dir.display())),
        "{out}"
    );
}

#[tokio::test]
async fn a_port_held_by_another_program_is_an_error() {
    let env = Env::new("busy");
    let squatter = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = squatter.local_addr().unwrap().port().to_string();
    let output = env.run(&["--no-open", "--port", &port]);
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("in use by another program"), "{err}");
}

#[tokio::test]
async fn the_login_token_survives_a_restart_but_sessions_do_not() {
    let env = Env::new("token");
    let (first, _upstream) = web_with_fake_upstream(&env).await;
    let (token, port, old_cookie) = (first.token.clone(), first.port, first.cookie());
    drop(first);

    let upstream = fake_upstream().await;
    let server = format!("ws://127.0.0.1:{}/", upstream.port);
    let second = env.start_on(port, &["--server", &server]).await;
    assert_eq!(second.token, token);
    assert_ne!(second.cookie(), old_cookie, "a new random session id");
    let with_old_cookie = second
        .http
        .get(second.url("/api/info"))
        .header("cookie", old_cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(
        with_old_cookie.status(),
        401,
        "sessions end with the process"
    );
    assert_eq!(second.get("/api/info").await.status(), 200);
}

#[tokio::test]
async fn two_instances_keep_separate_cookies() {
    let env = Env::new("two");
    let (first, _upstream) = web_with_fake_upstream(&env).await;
    let (second, _upstream) = web_with_fake_upstream(&env).await;
    assert_ne!(first.cookie_name(), second.cookie_name());

    // Both cookies sit in one browser jar, which sends them to every port of the host.
    let both = format!("{}; {}", first.cookie(), second.cookie());
    for web in [&first, &second] {
        let response = web
            .http
            .get(web.url("/api/info"))
            .header("cookie", &both)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
    }
    // And one instance does not accept the other's session.
    let crossed = first
        .http
        .get(first.url("/api/info"))
        .header("cookie", second.cookie())
        .send()
        .await
        .unwrap();
    assert_eq!(crossed.status(), 401);
}

/// A program that answers every request like a ref-web would, without knowing the login token.
async fn impostor_ref_web() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
                let mut request = [0u8; 1024];
                let _ = stream.read(&mut request).await;
                let body = r#"{"app":"ref-web","version":"0.1.0","proof":"00"}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    });
    port
}

// Multi-threaded: `env.run` blocks this thread while the impostor has to answer.
#[tokio::test(flavor = "multi_thread")]
async fn a_program_that_only_looks_like_ref_web_does_not_get_the_login_token() {
    let env = Env::new("impostor");
    let port = impostor_ref_web().await.to_string();
    let output = env.run(&["--no-open", "--port", &port]);
    assert!(!output.status.success(), "{output:?}");
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(all.contains("in use by another program"), "{all}");
    let token = std::fs::read_to_string(env.state_dir().join("web/login-token")).unwrap();
    assert!(!all.contains(token.trim()), "the login URL leaked: {all}");
}
