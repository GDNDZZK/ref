mod common;

use std::io::Read as _;

use common::*;
use tokio_tungstenite::tungstenite::Error as WsError;

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
    let server = format!("ws://{LOOPBACK}:{}/", upstream.port);
    let web = env
        .start(&["--server", &server, "--token", BACKEND_TOKEN])
        .await;
    (web, upstream)
}

#[tokio::test]
async fn pages_are_public_and_carry_the_security_headers() {
    let env = Env::new("pages");
    let (web, _upstream) = web_with_fake_upstream(&env).await;

    let page = web.get("/").await;
    assert_eq!(page.status(), 200, "no sign-in is needed to load the page");
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

    // Every response, errors included, keeps the key in a query from travelling on.
    for path in ["/", "/no/such/file.js", "/api/ping", "/api/ping?nonce=00ff"] {
        let response = web.get(path).await;
        assert_eq!(
            response.headers()["referrer-policy"],
            "no-referrer",
            "{path}"
        );
        assert!(response.headers().get("set-cookie").is_none(), "{path}");
    }
}

#[tokio::test]
async fn login_and_info_routes_are_gone() {
    let env = Env::new("gone");
    let (web, _upstream) = web_with_fake_upstream(&env).await;
    // An old bookmark falls back to the page, like any path without a file extension.
    let old_bookmark = web.get(&format!("/login?token={}&dir=/x", web.key)).await;
    assert_eq!(old_bookmark.status(), 200);
    assert!(old_bookmark.headers().get("set-cookie").is_none());
    assert_eq!(web.get("/api/info").await.status(), 404);
}

#[tokio::test]
async fn the_ping_proves_the_key_without_revealing_it() {
    let env = Env::new("ping");
    let (web, _upstream) = web_with_fake_upstream(&env).await;
    let ping = |nonce: &'static str| {
        let web = &web;
        async move {
            let response = web.get(&format!("/api/ping?nonce={nonce}")).await;
            response.json::<serde_json::Value>().await.unwrap()
        }
    };
    let first = ping("00ff").await;
    assert_eq!(first["app"], "ref-web");
    assert_eq!(first["proof"].as_str().unwrap().len(), 64, "{first}");
    assert_ne!(first["proof"], ping("00fe").await["proof"]);

    // The proof is keyed with k_auth, derived from the key and the salt in the state directory.
    let salt = std::fs::read_to_string(env.state_dir().join("web/salt")).unwrap();
    let root = channel::Root::derive(
        web.key.as_bytes(),
        &channel::from_hex_array(salt.trim()).unwrap(),
    );
    assert_eq!(
        first["proof"].as_str().unwrap(),
        channel::to_hex(&root.ping_proof("00ff"))
    );
    assert!(!first.to_string().contains(&web.key));

    for bad in ["/api/ping", "/api/ping?nonce=", "/api/ping?nonce=zz"] {
        assert_eq!(web.get(bad).await.status(), 400, "{bad}");
    }
}

#[tokio::test]
async fn frame_zero_tells_the_page_version_cwd_and_mock() {
    let env = Env::new("frame0");
    let (web, _upstream) = web_with_fake_upstream(&env).await;
    let channel = web.channel().await;
    assert_eq!(channel.info["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(
        channel.info["cwd"],
        std::env::current_dir().unwrap().to_str().unwrap()
    );
    assert_eq!(channel.info["mock"], false);
}

#[tokio::test]
async fn static_files_are_served_compressed_when_accepted() {
    let env = Env::new("static");
    let (web, _upstream) = web_with_fake_upstream(&env).await;
    let original = dist_file("index.html");

    let plain = web
        .http
        .get(web.url("/index.html"))
        .header("accept-encoding", "identity")
        .send()
        .await
        .unwrap();
    assert!(plain.headers().get("content-encoding").is_none());
    assert_eq!(plain.bytes().await.unwrap().as_ref(), original.as_slice());

    let negotiated = web
        .http
        .get(web.url("/index.html"))
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
async fn websocket_checks_origin_and_host() {
    let env = Env::new("auth");
    let (web, _upstream) = web_with_fake_upstream(&env).await;

    let foreign_origin = web
        .try_ws(Some("http://evil.example".into()), None)
        .await
        .err()
        .unwrap();
    assert_eq!(status_of(foreign_origin), 403);
    let other_port = web
        .try_ws(Some(format!("http://{LOOPBACK}:5173")), None)
        .await
        .err()
        .unwrap();
    assert_eq!(status_of(other_port), 403);
    let no_origin = web.try_ws(None, None).await.err().unwrap();
    assert_eq!(status_of(no_origin), 403);
    let rebinding = web
        .try_ws(
            Some("http://rebind.example:7480".into()),
            Some("rebind.example:7480"),
        )
        .await
        .err()
        .unwrap();
    assert_eq!(status_of(rebinding), 403);

    let wrong_host = web
        .http
        .get(web.url("/api/ping?nonce=00ff"))
        .header("host", "rebind.example")
        .send()
        .await
        .unwrap();
    assert_eq!(wrong_host.status(), 403, "Host is checked on every route");

    assert!(web.try_ws(Some(web.origin()), None).await.is_ok());
}

#[tokio::test]
async fn frames_are_forwarded_unchanged_and_the_backend_token_is_added() {
    let env = Env::new("forward");
    let (web, upstream) = web_with_fake_upstream(&env).await;
    let mut tab = web.channel().await;

    let texts = [
        r#"{"id":1,"op":"hello",   "cwd":"/x","caf\u00e9":"\ud83d\ude00","n":1.0}"#,
        "  not json at all\n",
        r#"{"kind":"future_frame"}"#,
        "",
    ];
    for text in texts {
        tab.send_text(text).await;
        assert_eq!(tab.next_event().await, Event::Text(text.to_string()));
    }
    let binary = vec![0u8, 1, 2, 255];
    tab.send_binary(&binary).await;
    assert_eq!(tab.next_event().await, Event::Binary(binary));

    let query = upstream.handshake_query.borrow().clone().unwrap();
    assert_eq!(query, format!("token={BACKEND_TOKEN}"));
}

#[tokio::test]
async fn each_tab_gets_its_own_upstream_connection() {
    let env = Env::new("tabs");
    let (web, _upstream) = web_with_fake_upstream(&env).await;
    let (mut first, mut second) = (web.channel().await, web.channel().await);
    first.send_text("one").await;
    second.send_text("two").await;
    assert_eq!(first.next_text().await, "one");
    assert_eq!(second.next_text().await, "two");
}

#[tokio::test]
async fn upstream_close_is_passed_on() {
    let env = Env::new("close");
    let (web, _upstream) = web_with_fake_upstream(&env).await;
    let mut tab = web.channel().await;
    tab.send_text(FAKE_CLOSE_TRIGGER).await;
    let (code, reason) = tab.next_close().await;
    assert_eq!(code, FAKE_CLOSE_CODE);
    assert_eq!(reason, FAKE_CLOSE_REASON);
}

#[tokio::test]
async fn unreachable_backend_closes_the_tab_with_a_fixed_reason() {
    let env = Env::new("down");
    let dead = format!("ws://{LOOPBACK}:{}/", free_port());
    let web = env.start(&["--server", &dead]).await;
    // The backend is contacted after the key check, so frame 0 never comes.
    let closed = Channel::sign_in(web.ws().await, &web.key).await.err();
    // The address of the backend is in the log, not in the close reason.
    assert_eq!(closed, Some((1011, "backend unavailable".to_string())));
}

#[tokio::test]
async fn a_real_backend_accepts_a_hello_without_token() {
    let env = Env::new("real");
    let (_backend, backend_port) = real_backend(&env, BACKEND_TOKEN).await;
    // No trailing slash: the form users type.
    let server = format!("ws://{LOOPBACK}:{backend_port}");
    let web = env
        .start(&["--server", &server, "--token", BACKEND_TOKEN])
        .await;
    let mut tab = web.channel().await;

    let hello = format!(
        r#"{{"id":1,"op":"hello","client":"ref-web","version":{},"cwd":"/tmp"}}"#,
        ref_proto::PROTOCOL_VERSION
    );
    tab.send_text(&hello).await;
    let response: serde_json::Value = serde_json::from_str(&tab.next_text().await).unwrap();
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["result"]["protocol"], ref_proto::PROTOCOL_VERSION);

    tab.send_text(r#"{"id":2,"op":"get_config"}"#).await;
    loop {
        let frame: serde_json::Value = serde_json::from_str(&tab.next_text().await).unwrap();
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
    let server = format!("ws://{LOOPBACK}:{backend_port}/");
    let web = env.start(&["--server", &server, "--token", "wrong"]).await;
    // Refused while connecting, or right after frame 0, depending on the backend's timing.
    let code = match Channel::sign_in(web.ws().await, &web.key).await {
        Err((code, _)) => code,
        Ok(mut tab) => tab.next_close().await.0,
    };
    assert_eq!(code, 1011);
}

#[tokio::test]
async fn a_second_start_opens_the_running_instance() {
    let env = Env::new("single");
    let (web, _upstream) = web_with_fake_upstream(&env).await;
    let port = web.port.to_string();
    let other_dir = dunce::canonicalize(&env.dir).unwrap();

    let second = env.run(&["--no-open", "--port", &port, other_dir.to_str().unwrap()]);
    assert!(second.status.success(), "{second:?}");
    let out = String::from_utf8_lossy(&second.stdout);
    assert!(out.contains("already running"), "{out}");
    // The address carries the directory percent-encoded: compare the decoded query.
    let url = out
        .split_whitespace()
        .find(|word| word.starts_with("http://"))
        .unwrap_or_else(|| panic!("no address in {out}"));
    let url = reqwest::Url::parse(url).unwrap();
    let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(query["dir"], other_dir.to_str().unwrap(), "{out}");
    assert_eq!(url.path(), "/");
    assert_eq!(
        url.fragment(),
        Some(format!("key={}", web.key).as_str()),
        "the key sits in the fragment, which is never sent"
    );
    assert!(
        !query.contains_key("key") && !query.contains_key("token"),
        "{out}"
    );
}

#[tokio::test]
async fn a_port_held_by_another_program_is_an_error() {
    let env = Env::new("busy");
    let squatter = std::net::TcpListener::bind((LOOPBACK, 0)).unwrap();
    let port = squatter.local_addr().unwrap().port().to_string();
    let output = env.run(&["--no-open", "--port", &port]);
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("in use by another program"), "{err}");
}

#[tokio::test]
async fn a_ref_web_with_another_key_is_reported_as_such() {
    let first_env = Env::new("key-a");
    let first = first_env
        .start(&["--server", &format!("ws://{LOOPBACK}:1/")])
        .await;
    let port = first.port.to_string();

    // Another state directory: no record to find, the port is simply taken.
    let second_env = Env::new("key-b");
    let output = second_env.run(&["--no-open", "--port", &port]);
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(
        err.contains("or by a ref-web with a different key"),
        "{err}"
    );
    assert!(!err.contains(&first.key), "{err}");

    // The same state directory: the record names the instance.
    let other_key = "another key of enough length";
    let output = first_env.run(&["--no-open", "--port", &port, "--key", other_key]);
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(
        err.contains(&format!(
            "a ref-web with a different key is running on port {port} (pid {})",
            first.pid()
        )),
        "{err}"
    );
    assert!(err.contains("pass the same --key"), "{err}");
    assert!(
        !err.contains(&first.key) && !err.contains(other_key),
        "{err}"
    );

    // Without --port, a different key starts an instance of its own.
    let third = first_env.start(&["--key", other_key]).await;
    assert_ne!(third.port, first.port);
}

#[tokio::test]
async fn the_key_and_the_salt_survive_a_restart() {
    let env = Env::new("restart");
    let (first, _upstream) = web_with_fake_upstream(&env).await;
    let (key, port) = (first.key.clone(), first.port);
    let salt = hello_salt(&first).await;
    drop(first);

    let upstream = fake_upstream().await;
    let server = format!("ws://{LOOPBACK}:{}/", upstream.port);
    let second = env.start_on(port, &["--server", &server]).await;
    assert_eq!(second.key, key);
    assert_eq!(hello_salt(&second).await, salt, "stored keys stay valid");
    second.channel().await;
}

/// The salt in the hello message.
async fn hello_salt(web: &Web) -> channel::Salt {
    let mut ws = web.ws().await;
    channel::parse_hello(&next_text(&mut ws).await).unwrap().0
}

/// A program that answers every request like a ref-web would, without knowing the key.
async fn impostor_ref_web() -> u16 {
    let listener = tokio::net::TcpListener::bind((LOOPBACK, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
                let mut request = [0u8; 1024];
                let read = stream.read(&mut request).await.unwrap();
                assert!(read > 0, "the client sent a request");
                let body = r#"{"app":"ref-web","version":"0.1.0","proof":"00"}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            });
        }
    });
    port
}

// Multi-threaded: `env.run` blocks this thread while the impostor has to answer.
#[tokio::test(flavor = "multi_thread")]
async fn a_program_that_only_looks_like_ref_web_does_not_get_the_key() {
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
    assert!(!all.contains(token.trim()), "the key leaked: {all}");
}
