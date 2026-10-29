//! `hernees-server`: the hernees backend (sessions, agent loop, LLM calls, tools, MCP,
//! persistence), serving the hn-proto WebSocket protocol to frontends.

mod commands;
mod compaction;
mod cost;
mod hub;
mod llm;
mod mcp;
mod peer;
mod persist;
mod prompt;
mod registry;
mod session;
mod status;
mod title;
mod util;
mod ws;

use clap::Parser;
use hn_proto::InstanceInfo;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(name = "hernees-server", version, about = "hernees backend (WebSocket server)")]
struct Cli {
    /// Port to listen on (default: a random free port >= 10000).
    #[arg(long)]
    port: Option<u16>,
    /// Address to bind.
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    /// Allowed client address (repeatable). Default: 127.0.0.1, ::1. "*" allows any.
    #[arg(long = "allow-host", value_name = "HOST")]
    allow_host: Vec<String>,
    /// Seconds without any frontend before exiting: 0 = exit when the last frontend
    /// disconnects, -1 = never. Counts only after the first frontend connected (with 0, the
    /// server also exits if no frontend connects within 120s of startup).
    #[arg(long, default_value_t = -1, allow_negative_numbers = true)]
    idle_timeout: i64,
    /// Show running instances and exit (exit code 0 if any is running).
    #[arg(long, alias = "list")]
    status: bool,
    /// Machine-readable output for --status.
    #[arg(long)]
    json: bool,
}

/// Grace period before the first frontend connects (for idle-timeout 0 only).
const STARTUP_GRACE: Duration = Duration::from_secs(120);

fn main() {
    let cli = Cli::parse();
    let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("hernees-server: cannot start runtime: {e}");
            std::process::exit(1);
        }
    };
    if cli.status {
        let code = rt.block_on(status::run(cli.json));
        std::process::exit(code);
    }
    init_logging();
    let code = rt.block_on(async {
        match serve(cli).await {
            Ok(()) => 0,
            Err(e) => {
                tracing::error!("{e:#}");
                eprintln!("hernees-server: {e:#}");
                1
            }
        }
    });
    rt.shutdown_timeout(Duration::from_secs(2));
    std::process::exit(code);
}

fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_env("HERNEES_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::stderr).with_ansi(false).init();
}

async fn bind(host: &str, port: Option<u16>) -> anyhow::Result<tokio::net::TcpListener> {
    let ip: std::net::IpAddr = match host {
        "localhost" => std::net::IpAddr::from([127, 0, 0, 1]),
        h => h.trim_start_matches('[').trim_end_matches(']').parse().map_err(|e| anyhow::anyhow!("--host {h}: {e}"))?,
    };
    if let Some(p) = port {
        return tokio::net::TcpListener::bind(SocketAddr::new(ip, p))
            .await
            .map_err(|e| anyhow::anyhow!("cannot listen on {ip}:{p}: {e}"));
    }
    use rand::Rng;
    for _ in 0..100 {
        let p = rand::thread_rng().gen_range(10000..60000u16);
        if let Ok(l) = tokio::net::TcpListener::bind(SocketAddr::new(ip, p)).await {
            return Ok(l);
        }
    }
    anyhow::bail!("no free port found on {ip}")
}

async fn serve(cli: Cli) -> anyhow::Result<()> {
    let allow_hosts = if cli.allow_host.is_empty() {
        vec!["127.0.0.1".to_string(), "::1".to_string()]
    } else {
        cli.allow_host.clone()
    };
    let listener = bind(&cli.host, cli.port).await?;
    let addr = listener.local_addr()?;
    let hub = hub::Hub::new();
    let state = ws::AppState { hub: hub.clone(), allow: Arc::new(ws::AllowList::new(&allow_hosts)) };
    let pid = std::process::id();
    let info = InstanceInfo {
        pid,
        port: addr.port(),
        bind: cli.host.clone(),
        allow_hosts: allow_hosts.clone(),
        idle_timeout: cli.idle_timeout,
        started_at: hn_proto::now_ms(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        exe: std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
    };
    if let Err(e) = registry::register(&info) {
        tracing::warn!("cannot write instance file: {e:#}");
    }
    tracing::info!(
        "hernees-server {} listening on ws://{addr}/ (pid {pid}, idle-timeout {}, allow {:?}, data {})",
        info.version,
        cli.idle_timeout,
        allow_hosts,
        hn_proto::data_dir().display()
    );

    let app = ws::router(state).into_make_service_with_connect_info::<SocketAddr>();
    let stop = tokio_util::sync::CancellationToken::new();
    let stop2 = stop.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).with_graceful_shutdown(async move { stop2.cancelled().await }).await
    });

    let reason = tokio::select! {
        r = shutdown_signal() => r,
        r = idle_watch(hub.clone(), cli.idle_timeout) => r,
    };
    tracing::info!("shutting down: {reason}");
    hub.shutdown().await;
    registry::unregister(pid);
    stop.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(2), server).await;
    Ok(())
}

async fn shutdown_signal() -> &'static str {
    use tokio::signal::unix::{signal, SignalKind};
    let mut term = match signal(SignalKind::terminate()) {
        Ok(s) => s,
        Err(_) => {
            let _ = tokio::signal::ctrl_c().await;
            return "SIGINT";
        }
    };
    let mut hup = signal(SignalKind::hangup()).ok();
    tokio::select! {
        _ = tokio::signal::ctrl_c() => "SIGINT",
        _ = term.recv() => "SIGTERM",
        _ = async { match hup.as_mut() { Some(h) => { h.recv().await; } None => std::future::pending().await } } => "SIGHUP",
    }
}

/// Resolves when the server should exit because no frontend is connected.
async fn idle_watch(hub: Arc<hub::Hub>, timeout: i64) -> &'static str {
    if timeout < 0 {
        return std::future::pending().await;
    }
    let mut rx = hub.subscribe_clients();
    // The timeout only applies once a frontend has connected. Exception: `0` (the mode the
    // TUI auto-spawns with) gives up after a grace period so a server whose spawning frontend
    // died before connecting does not linger forever.
    if *rx.borrow() == 0 {
        if timeout == 0 {
            match tokio::time::timeout(STARTUP_GRACE, rx.wait_for(|n| *n > 0)).await {
                Ok(Ok(_)) => {}
                Ok(Err(_)) => return "client registry closed",
                Err(_) => return "no frontend connected during the startup grace period",
            }
        } else if rx.wait_for(|n| *n > 0).await.is_err() {
            return "client registry closed";
        }
    }
    let timeout = Duration::from_secs(timeout as u64);
    loop {
        if rx.wait_for(|n| *n == 0).await.is_err() {
            return "client registry closed";
        }
        match tokio::time::timeout(timeout, rx.wait_for(|n| *n > 0)).await {
            Ok(Ok(_)) => continue,
            Ok(Err(_)) => return "client registry closed",
            Err(_) => return "idle timeout (no frontends connected)",
        }
    }
}
