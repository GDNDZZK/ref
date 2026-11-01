//! `ref-web` — serves the web UI and forwards its WebSocket frames to ref-server.

#[macro_use]
mod console;

mod assets;
mod channel;
#[cfg(test)]
mod channel_tests;
mod guard;
mod instance;
mod key;
mod launch;
mod listen;
mod ping;
mod routes;
mod token;
mod tunnel;
mod upstream;

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use clap::Parser;
use ref_client::Target;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::guard::{HostPolicy, LOCALHOST};
use crate::key::Key;
use crate::routes::AppState;
use crate::tunnel::{HANDSHAKE_TIMEOUT, Heartbeat, MAX_HANDSHAKES};
use crate::upstream::Upstream;

const DEFAULT_HOST: &str = "127.0.0.1";
/// How long shutdown waits for tabs to receive their close frame.
const TUNNEL_CLOSE_GRACE: Duration = Duration::from_secs(3);

#[derive(Parser, Debug)]
#[command(
    name = "ref-web",
    version,
    about = "ref-web — web UI for ref-server",
    after_help = "Starting ref-web again opens the browser on the running one (and passes DIR on \
        to the page) when it has the same key.\n\n\
        Without --key, the key is a random login token, created on first start and kept in \
        <state dir>/web/login-token. Delete that file (while ref-web is stopped) to get a new \
        one; every browser then has to enter the new key. The address ref-web prints ends with \
        #key=<key>: the browser never sends that part to a server."
)]
struct Cli {
    /// Directory new sessions start in (default: current directory).
    dir: Option<String>,
    /// Port to listen on (1-65535). Default: a random free port in 10000-65535; the port of the
    /// last run is tried first, so the address stays the same.
    #[arg(long, value_parser = clap::value_parser!(u16).range(1..))]
    port: Option<u16>,
    /// Address to listen on. Anything but a loopback address opens ref-web to the network
    /// (no TLS).
    #[arg(long, default_value = DEFAULT_HOST)]
    host: String,
    /// Key the browser signs in with, at least 16 characters. Default: a random login token
    /// kept in <state dir>/web/login-token. It shows up in `ps` and the shell history; the
    /// environment variable REF_WEB_KEY does not.
    #[arg(long, env = "REF_WEB_KEY", hide_env_values = true)]
    key: Option<String>,
    /// Do not open the browser.
    #[arg(long)]
    no_open: bool,
    /// Explicit backend URL (ws://host:port); disables auto-spawn.
    #[arg(long, conflicts_with = "mock")]
    server: Option<String>,
    /// Token of the backend given with --server (not the key of the browser, see --key).
    #[arg(long, requires = "server")]
    token: Option<String>,
    /// Use a private fake backend (development).
    #[arg(long)]
    mock: bool,
}

fn main() -> anyhow::Result<()> {
    init_logging();
    let cli = Cli::parse();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(run(cli))
}

fn init_logging() {
    let filter =
        tracing_subscriber::EnvFilter::try_from_env("REF_LOG").unwrap_or_else(|_| "warn".into());
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .init();
}

/// The absolute start directory. `dunce` keeps Windows paths free of the `\\?\` prefix, which
/// the backend and the page would show to the user.
fn start_dir(dir: Option<&str>) -> anyhow::Result<String> {
    let path = match dir {
        Some(p) => dunce::canonicalize(p).with_context(|| p.to_string())?,
        None => std::env::current_dir()?,
    };
    anyhow::ensure!(path.is_dir(), "{} is not a directory", path.display());
    Ok(path.to_string_lossy().into_owned())
}

fn is_loopback(host: &str) -> bool {
    let bare = launch::bare_host(host);
    bare.eq_ignore_ascii_case(LOCALHOST) || bare.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

fn upstream_for(cli: &Cli) -> Upstream {
    if cli.mock {
        return Upstream::mock();
    }
    match &cli.server {
        Some(url) => Upstream::Remote(Target::Explicit {
            url: url.clone(),
            token: cli.token.clone(),
        }),
        None => Upstream::Remote(Target::Auto),
    }
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    let cwd = start_dir(cli.dir.as_deref())?;
    let key = key::load(cli.key.as_deref()).await?;
    if open_running(&cli, &key, &cwd).await? {
        return Ok(());
    }

    let listener = listen::bind(&cli.host, cli.port).await?;
    let port = listener.local_addr()?.port();
    let url_host = launch::reachable_host(&cli.host);
    let url = launch::key_url(&url_host, port, key.text(), Some(&cwd));
    let record_path = instance::write(&instance::Record::current(&cli.host, port, false))?;

    say!("{url}");
    if !is_loopback(&cli.host) {
        say_err!(
            "warning: listening on {}, reachable from the network. Traffic is encrypted with the \
             key, but not with TLS. Anyone with the key can run commands as you.",
            cli.host
        );
    }
    if !cli.no_open {
        launch::open_browser(&url);
    }

    let shutdown = CancellationToken::new();
    let state = Arc::new(AppState {
        key,
        handshakes: Arc::new(Semaphore::new(MAX_HANDSHAKES)),
        handshake_timeout: HANDSHAKE_TIMEOUT,
        hosts: HostPolicy::new(&cli.host, port),
        cwd,
        upstream: upstream_for(&cli),
        heartbeat: Heartbeat::DEFAULT,
        shutdown: shutdown.clone(),
        tunnels: TaskTracker::new(),
    });
    let stop = shutdown.clone();
    tokio::spawn(async move {
        wait_for_stop_signal().await;
        stop.cancel();
    });
    let app = routes::router(state.clone()).into_make_service_with_connect_info::<SocketAddr>();
    let served = axum::serve(listener, app)
        .with_graceful_shutdown(async move { shutdown.cancelled().await })
        .await;
    state.tunnels.close();
    if tokio::time::timeout(TUNNEL_CLOSE_GRACE, state.tunnels.wait())
        .await
        .is_err()
    {
        say_err!("some tabs did not close in time");
    }
    state.upstream.shutdown().await;
    instance::remove(&record_path);
    served.context("server failed")
}

/// If a ref-web with our key is running (on `--port` when given), points the browser at it
/// and returns true. With `--port`, another key on that port is an error.
async fn open_running(cli: &Cli, key: &Key, cwd: &str) -> anyhow::Result<bool> {
    for record in instance::list_live() {
        if cli.port.is_some_and(|port| port != record.port) {
            continue;
        }
        let host = launch::reachable_host(&record.host);
        if !ping::is_our_ref_web(&host, record.port, key).await {
            anyhow::ensure!(
                cli.port.is_none(),
                "a ref-web with a different key is running on port {} (pid {}); stop it, or pass \
                 the same --key",
                record.port,
                record.pid
            );
            continue;
        }
        let url = launch::key_url(&host, record.port, key.text(), Some(cwd));
        println!(
            "{} is already running on port {} (pid {}): {url}",
            routes::APP_NAME,
            record.port,
            record.pid
        );
        let ignored_options = cli.server.is_some() || cli.mock || cli.host != DEFAULT_HOST;
        if ignored_options {
            say_err!("the running instance keeps its own --server, --mock and --host");
        }
        if !cli.no_open {
            launch::open_browser(&url);
        }
        return Ok(true);
    }
    Ok(false)
}

#[cfg(unix)]
async fn wait_for_stop_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    let mut term = signal(SignalKind::terminate()).expect("install SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}

#[cfg(not(unix))]
async fn wait_for_stop_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_dir_is_absolute_and_must_be_a_directory() {
        let here = std::env::current_dir().unwrap();
        assert_eq!(
            start_dir(None).unwrap(),
            here.to_string_lossy().into_owned()
        );
        assert_eq!(
            start_dir(Some(".")).unwrap(),
            dunce::canonicalize(&here).unwrap().to_string_lossy()
        );
        let manifest = format!("{}/Cargo.toml", env!("CARGO_MANIFEST_DIR"));
        assert!(start_dir(Some(&manifest)).is_err());
        assert!(start_dir(Some("/no/such/dir/anywhere")).is_err());
    }
}
