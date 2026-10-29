//! HTTP proxy handling: local providers bypass env proxies (even with NO_PROXY entries
//! reqwest cannot parse), and empty error bodies from a proxy are labelled as such.
mod common;

use common::*;
use hn_proto::*;
use serde_json::json;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// A "proxy" that answers every request with `502` and no body. Returns (port, hit counter).
async fn bad_gateway_proxy() -> (u16, Arc<std::sync::atomic::AtomicUsize>) {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let h = hits.clone();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = l.accept().await {
            h.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf).await;
                let _ = s.write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
            });
        }
    });
    (port, hits)
}

fn spawn_with_proxy(env: &Env, proxy_port: u16) -> Server {
    let port = free_port();
    let log_path = env.path(&format!("server-{port}.log"));
    let log = std::fs::File::create(&log_path).unwrap();
    let proxy = format!("http://127.0.0.1:{proxy_port}/");
    let child = env
        .command()
        .args(["--port", &port.to_string()])
        .env("HTTP_PROXY", &proxy)
        .env("HTTPS_PROXY", &proxy)
        .env("ALL_PROXY", format!("socks://127.0.0.1:{proxy_port}/"))
        .env("NO_PROXY", "*zhihu.com,localhost,*.local,127.*,10.*,172.2*,192.168.*")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::from(log))
        .spawn()
        .unwrap();
    Server { child, port, log: log_path }
}

/// Prompt and return the error of the first retry status.
async fn first_retry_error(base_url: &str, proxy_port: u16) -> String {
    let env = Env::new(
        0,
        json!({
            "provider": {"p": {
                "npm": "@ai-sdk/openai-compatible",
                "options": {"baseURL": base_url, "apiKey": "k"},
                "models": {"m": {}}
            }},
            "model": "p/m",
            "retry": {"initial_delay": 30, "max_delay": 60},
        }),
    );
    let srv = spawn_with_proxy(&env, proxy_port);
    let mut c = Client::connect(&srv.url(), &env.work()).await;
    let info = c.create(&env.work(), None).await;
    c.prompt(&info.id, "hi").await;
    let m = c
        .wait_for("retry", T, |m| {
            matches!(m, ServerMsg::Event { event: Event::Info { info }, .. } if matches!(info.status, SessionStatus::Retry { .. }))
        })
        .await;
    c.req(ClientMsg::Abort { session: info.id.clone() }).await.unwrap();
    match m {
        ServerMsg::Event { event: Event::Info { info: SessionInfo { status: SessionStatus::Retry { error, .. }, .. } }, .. } => error,
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn loopback_provider_bypasses_env_proxy() {
    let (proxy, hits) = bad_gateway_proxy().await;
    // Nothing listens on port 9: the real error is "connection refused", not the proxy's 502.
    let err = first_retry_error("http://127.0.0.1:9/v1", proxy).await;
    assert!(err.starts_with("connection error"), "{err}");
    assert!(!err.contains("502"), "{err}");
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0, "the proxy must not be used");
}

#[tokio::test]
async fn empty_502_from_proxy_names_the_proxy() {
    let (proxy, hits) = bad_gateway_proxy().await;
    let err = first_retry_error("http://provider.invalid/v1", proxy).await;
    assert_eq!(err, format!("HTTP 502 (empty body; possibly from proxy http://127.0.0.1:{proxy}/)"));
    assert!(hits.load(std::sync::atomic::Ordering::SeqCst) >= 1);
}
