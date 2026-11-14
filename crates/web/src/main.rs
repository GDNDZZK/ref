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
mod tls;
mod tls_pin;
mod token;
mod tunnel;
mod upstream;
mod ws;

use anyhow::Context as _;
use clap::Parser;
use ref_client::Target;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::guard::HostPolicy;
use crate::key::Key;
use crate::routes::{AppState, ClientAddr};
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
        #key=<key>: the browser never sends that part to a server.\n\n\
        Non-loopback --host enables HTTPS with a self-signed certificate kept in \
        <state dir>/web/tls/ (check the printed SHA-256 fingerprint on first visit); pass \
        --no-tls to serve plain HTTP instead (for example behind a reverse proxy)."
)]
struct Cli {
    /// Directory new sessions start in (default: current directory).
    dir: Option<String>,
    /// Port to listen on (1-65535). Default: a random free port in 10000-65535; the port of the
    /// last run is tried first, so the address stays the same.
    #[arg(long, value_parser = clap::value_parser!(u16).range(1..))]
    port: Option<u16>,
    /// Address to listen on. Anything other than a loopback address opens ref-web to the network
    /// and enables HTTPS with a self-signed certificate by default (pass --no-tls for plain HTTP).
    #[arg(long, default_value = DEFAULT_HOST)]
    host: String,
    /// Serve plain HTTP even on a non-loopback --host (e.g. behind a TLS-terminating reverse proxy).
    /// On a loopback --host this is a silent no-op.
    #[arg(long)]
    no_tls: bool,
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

    let use_tls = !tls::is_loopback(&cli.host) && !cli.no_tls;
    let loaded_tls = if use_tls {
        Some(tls::load_or_create(&cli.host)?)
    } else {
        None
    };
    let listener = listen::bind(&cli.host, cli.port).await?;
    let port = listener.local_addr()?.port();
    let url_host = launch::reachable_host(&cli.host);
    let url = launch::key_url(&url_host, port, key.text(), Some(&cwd), use_tls);
    let record_path = instance::write(&instance::Record::current(&cli.host, port, use_tls))?;

    if let Some(loaded) = &loaded_tls {
        if loaded.renewed {
            say!("certificate renewed; new SHA-256 {}", loaded.fingerprint);
        }
        say!("certificate SHA-256 {}", loaded.fingerprint);
    }
    say!("{url}");
    if let Some(loaded) = &loaded_tls {
        say_err!(
            "listening on {}, reachable from the network. Traffic is encrypted with the key and \
             with TLS (self-signed certificate, SHA-256 {}). Check the fingerprint in the \
             browser's warning page. Anyone with the key can run commands as you.",
            cli.host,
            loaded.fingerprint
        );
    } else if !tls::is_loopback(&cli.host) {
        say_err!(
            "listening on {} without TLS. The traffic is encrypted with the key, but anyone on the \
             network can change the page you load and steal the key. Anyone with the key can run \
             commands as you.",
            cli.host
        );
    }
    if !cli.no_open {
        launch::open_browser(&url);
    }

    let shutdown = CancellationToken::new();
    let hosts = HostPolicy::new(&cli.host, port, use_tls);
    let state = Arc::new(AppState {
        key,
        handshakes: Arc::new(Semaphore::new(MAX_HANDSHAKES)),
        handshake_timeout: HANDSHAKE_TIMEOUT,
        hosts: hosts.clone(),
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
    let app = routes::router(state.clone()).into_make_service_with_connect_info::<ClientAddr>();
    let served = if let Some(loaded) = loaded_tls {
        let tls_listener = tls::TlsListener::new(listener, loaded.acceptor, hosts);
        axum::serve(tls_listener, app)
            .with_graceful_shutdown(async move { shutdown.cancelled().await })
            .await
    } else {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move { shutdown.cancelled().await })
            .await
    };
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
    for record in instance::list_live()? {
        if cli.port.is_some_and(|port| port != record.port) {
            continue;
        }
        let host = launch::reachable_host(&record.host);
        if !ping::is_our_ref_web(&host, record.port, key, record.tls).await {
            anyhow::ensure!(
                cli.port.is_none(),
                "a ref-web with a different key is running on port {} (pid {}); stop it, or pass \
                 the same --key",
                record.port,
                record.pid
            );
            continue;
        }
        let url = launch::key_url(&host, record.port, key.text(), Some(cwd), record.tls);
        if record.tls
            && let Ok(cert) = tls::cert_path()
            && let Ok(der) = tls_pin::load_cert_der(&cert)
        {
            say!(
                "certificate SHA-256 {}",
                tls_pin::cert_fingerprint(der.as_ref())
            );
        }
        println!(
            "{} is already running on port {} (pid {}): {url}",
            routes::APP_NAME,
            record.port,
            record.pid
        );
        let ignored_options =
            cli.server.is_some() || cli.mock || cli.host != DEFAULT_HOST || cli.no_tls;
        if ignored_options {
            say_err!("the running instance keeps its own --server, --mock, --host and --no-tls");
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
