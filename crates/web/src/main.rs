//! `ref-web` — serves the web UI and forwards its WebSocket frames to ref-server.

mod assets;
mod guard;
mod launch;
mod ping;
mod routes;
mod token;
mod tunnel;
mod upstream;

use std::net::IpAddr;
use std::sync::Arc;

use anyhow::Context as _;
use clap::Parser;
use ref_client::Target;
use tokio_util::sync::CancellationToken;

use crate::guard::{HostPolicy, SessionCookie};
use crate::routes::AppState;
use crate::upstream::Upstream;

const DEFAULT_PORT: u16 = 7480;
const DEFAULT_HOST: &str = "127.0.0.1";
const LOCALHOST: &str = "localhost";

#[derive(Parser, Debug)]
#[command(
    name = "ref-web",
    version,
    about = "ref-web — web UI for ref-server",
    after_help = "Only one ref-web runs per port: starting it again opens the browser on the \
        running one (and passes DIR on to the page).\n\n\
        The login token is created on first start and kept in <state dir>/web/login-token. \
        Delete that file (while ref-web is stopped) to get a new one."
)]
struct Cli {
    /// Directory new sessions start in (default: current directory).
    dir: Option<String>,
    /// Port to listen on.
    #[arg(long, default_value_t = DEFAULT_PORT)]
    port: u16,
    /// Address to listen on. Anything but a loopback address opens ref-web to the network
    /// (no TLS).
    #[arg(long, default_value = DEFAULT_HOST)]
    host: String,
    /// Do not open the browser.
    #[arg(long)]
    no_open: bool,
    /// Explicit backend URL (ws://host:port); disables auto-spawn.
    #[arg(long, conflicts_with = "mock")]
    server: Option<String>,
    /// Token for --server.
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

fn start_dir(dir: Option<&str>) -> anyhow::Result<String> {
    let path = match dir {
        Some(p) => std::fs::canonicalize(p).with_context(|| p.to_string())?,
        None => std::env::current_dir()?,
    };
    anyhow::ensure!(path.is_dir(), "{} is not a directory", path.display());
    Ok(path.to_string_lossy().into_owned())
}

fn is_loopback(host: &str) -> bool {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    bare.eq_ignore_ascii_case(LOCALHOST) || bare.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

fn upstream_for(cli: &Cli) -> Upstream {
    if cli.mock {
        return Upstream::mock();
    }
    match &cli.server {
        Some(url) => Upstream::remote(Target::Explicit {
            url: url.clone(),
            token: cli.token.clone(),
        }),
        None => Upstream::remote(Target::Auto),
    }
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    let cwd = start_dir(cli.dir.as_deref())?;
    let login_token = token::load_or_create()?;
    let url_host = launch::reachable_host(&cli.host);
    let login_url = launch::login_url(&url_host, cli.port, &login_token, Some(&cwd));

    let bind_host = cli.host.trim_start_matches('[').trim_end_matches(']');
    let listener = match tokio::net::TcpListener::bind((bind_host, cli.port)).await {
        Ok(listener) => listener,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            return open_running(&cli, &url_host, &login_token, &login_url).await;
        }
        Err(e) => {
            return Err(e).with_context(|| format!("cannot listen on {}:{}", cli.host, cli.port));
        }
    };

    println!("ref-web: {login_url}");
    if !is_loopback(&cli.host) {
        eprintln!(
            "ref-web: warning: listening on {}, reachable from the network. Anyone who has the \
             login URL can run commands as you. Traffic is not encrypted (no TLS).",
            cli.host
        );
    }
    if !cli.no_open {
        launch::open_browser(&login_url);
    }

    let shutdown = CancellationToken::new();
    let state = Arc::new(AppState {
        login_token,
        session: SessionCookie::new(cli.port),
        hosts: HostPolicy::new(&cli.host, cli.port),
        cwd,
        upstream: upstream_for(&cli),
        shutdown: shutdown.clone(),
    });
    let stop = shutdown.clone();
    tokio::spawn(async move {
        wait_for_stop_signal().await;
        stop.cancel();
    });
    let served = axum::serve(listener, routes::router(state.clone()))
        .with_graceful_shutdown(async move { shutdown.cancelled().await })
        .await;
    state.upstream.shutdown().await;
    served.context("server failed")
}

/// The port is taken: if a ref-web has it, point the browser there; otherwise fail.
async fn open_running(
    cli: &Cli,
    url_host: &str,
    login_token: &str,
    login_url: &str,
) -> anyhow::Result<()> {
    if !ping::is_our_ref_web(url_host, cli.port, login_token).await {
        anyhow::bail!(
            "port {} is in use by another program (choose another with --port)",
            cli.port
        );
    }
    println!(
        "ref-web is already running on port {}: {login_url}",
        cli.port
    );
    if cli.server.is_some() || cli.mock || cli.host != DEFAULT_HOST {
        eprintln!("ref-web: the running instance keeps its own --server, --mock and --host");
    }
    if !cli.no_open {
        launch::open_browser(login_url);
    }
    Ok(())
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
