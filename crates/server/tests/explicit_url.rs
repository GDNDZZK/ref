//! An explicit backend URL works with and without a trailing slash, with a token.

use std::time::Duration;

use ref_client::Endpoint;

const TOKEN: &str = "url-test-token";
const TIMEOUT: Duration = Duration::from_secs(10);

async fn start_backend(tag: &str) -> (ref_server::Running, u16) {
    let dir = std::env::temp_dir().join(format!("ref-url-{tag}-{}", std::process::id()));
    let mut options = ref_server::HubOptions::new(dir);
    options.home = None;
    options.config_loader = std::sync::Arc::new(|_| ref_config::Config::default());
    let (auth, _) = ref_server::ws::resolve_hosts(&["127.0.0.1".into()], TOKEN.into());
    let running = ref_server::start(options, "127.0.0.1", 0, auth)
        .await
        .unwrap();
    let port = running.addr.port();
    (running, port)
}

#[tokio::test]
async fn explicit_urls_connect_with_or_without_a_trailing_slash() {
    let (_running, port) = start_backend("slash").await;
    for url in [
        format!("ws://127.0.0.1:{port}"),
        format!("ws://127.0.0.1:{port}/"),
        format!("ws://127.0.0.1:{port}/?x=1"),
        format!("ws://127.0.0.1:{port}?x=1"),
    ] {
        let endpoint = Endpoint {
            url: url.clone(),
            token: Some(TOKEN.into()),
        };
        if let Err(e) = ref_client::connect(&endpoint, TIMEOUT).await {
            panic!("{url}: {e}");
        }
    }
}
