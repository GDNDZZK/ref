use clap::Parser;
use ref_proto::InstanceInfo;
use ref_server::{HubOptions, instance, ops, ws};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(
    name = "ref-server",
    version,
    about = "Backend of the ref coding agent (WebSocket server)"
)]
struct Cli {
    /// Port to listen on (0 or absent: random free port in 10000..=65535).
    #[arg(long)]
    port: Option<u16>,
    /// Bind address (default 127.0.0.1, or 0.0.0.0 when a non-loopback host is allowed).
    #[arg(long)]
    host: Option<String>,
    /// Hosts allowed to connect (repeatable / comma separated; `*` = any).
    /// Default: 127.0.0.1, ::1, localhost.
    #[arg(long = "allow-host", value_delimiter = ',')]
    allow_host: Vec<String>,
    /// Seconds to keep running after the last frontend disconnects
    /// (0 = exit immediately, -1 = never). Only effective after the first connection.
    #[arg(long, default_value_t = -1, allow_negative_numbers = true)]
    idle_timeout: i64,
    /// Shared secret clients must present (default: random).
    #[arg(long)]
    token: Option<String>,
    /// Log file (default: <state_dir>/logs/server-<pid>.log).
    #[arg(long)]
    log: Option<PathBuf>,
    /// List running instances and their configuration.
    #[arg(long)]
    status: bool,
    /// With --status: print JSON.
    #[arg(long)]
    json: bool,
    /// Stop running instance(s) (all, or the one with PID).
    #[arg(long, value_name = "PID", num_args = 0..=1)]
    stop: Option<Option<u32>>,
    /// Run a self-contained mock backend with a scripted model, in a temporary directory that
    /// is deleted on exit. Prints one line `{"port":N,"token":"..."}`. Exits when the last
    /// client leaves, and at once when stdin is closed (started with `&`, or with stdin from
    /// /dev/null): keep stdin open, e.g. `sleep 3600 | ref-server --mock`.
    #[arg(
        long,
        conflicts_with_all = ["status", "stop", "host", "allow_host", "idle_timeout", "token", "log"]
    )]
    mock: bool,
}

fn is_loopback(h: &str) -> bool {
    let h = h.trim().trim_start_matches('[').trim_end_matches(']');
    h == "localhost"
        || h.parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

fn init_logging(path: &PathBuf) -> anyhow::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let filter =
        tracing_subscriber::EnvFilter::try_from_env("REF_LOG").unwrap_or_else(|_| "info".into());
    tracing_subscriber::fmt()
        .with_writer(std::sync::Mutex::new(file))
        .with_ansi(false)
        .with_env_filter(filter)
        .init();
    Ok(())
}

fn main() {
    let cli = Cli::parse();
    #[cfg(unix)]
    if cli.status || cli.stop.is_some() {
        // Behave like a normal CLI when piped into `head` etc. (exit quietly on EPIPE).
        // SAFETY: setting a signal disposition before any threads are started.
        unsafe {
            libc::signal(libc::SIGPIPE, libc::SIG_DFL);
        }
    }
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("ref-server: cannot start runtime: {e}");
            std::process::exit(1);
        }
    };
    let code = rt.block_on(async move {
        if cli.status {
            return instance::status(cli.json).await;
        }
        if let Some(pid) = cli.stop {
            return instance::stop(pid).await;
        }
        let result = if cli.mock {
            serve_mock(cli.port).await
        } else {
            serve(cli).await
        };
        match result {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("ref-server: {e:#}");
                tracing::error!("fatal: {e:#}");
                1
            }
        }
    });
    rt.shutdown_timeout(Duration::from_secs(2));
    std::process::exit(code);
}

async fn serve(cli: Cli) -> anyhow::Result<()> {
    let pid = std::process::id();
    let log = cli.log.clone().unwrap_or_else(|| {
        ref_proto::state_dir()
            .join("logs")
            .join(format!("server-{pid}.log"))
    });
    if let Err(e) = init_logging(&log) {
        eprintln!("ref-server: cannot open log file {}: {e}", log.display());
    }
    let allow: Vec<String> = if cli.allow_host.is_empty() {
        vec!["127.0.0.1".into(), "::1".into(), "localhost".into()]
    } else {
        cli.allow_host
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    };
    let bind = match &cli.host {
        Some(h) => h
            .trim()
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_string(),
        None if allow.iter().any(|h| !is_loopback(h)) => "0.0.0.0".into(),
        None => "127.0.0.1".into(),
    };
    let port = match cli.port {
        Some(p) if p != 0 => p,
        _ => instance::pick_port(&bind).ok_or_else(|| anyhow::anyhow!("no free port found"))?,
    };
    let token = cli
        .token
        .clone()
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| ref_server::util::random_hex(32));
    let (auth, warnings) = ws::resolve_hosts(&allow, token.clone());
    for w in warnings {
        tracing::warn!("{w}");
        eprintln!("ref-server: warning: {w}");
    }
    let mut opts = HubOptions::new(ref_proto::data_dir());
    opts.idle_timeout = cli.idle_timeout;
    let running = ref_server::start(opts, &bind, port, auth).await?;
    let info = InstanceInfo {
        pid,
        port: running.addr.port(),
        bind: bind.clone(),
        allow_hosts: allow.clone(),
        idle_timeout: cli.idle_timeout,
        started_at: ref_proto::now_ms(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        token,
    };
    *running
        .hub
        .instance
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(info.clone());
    let dir = ref_proto::instances_dir();
    let inst_file = instance::write_instance(&dir, &info)?;
    tracing::info!(port = info.port, %bind, idle_timeout = cli.idle_timeout, "ref-server listening");
    eprintln!(
        "ref-server listening on ws://{}:{} (pid {pid})",
        bind, info.port
    );

    let hub = running.hub.clone();
    let mut signals = StopSignals::new()?;
    tokio::select! {
        _ = hub.shutdown.cancelled() => {}
        _ = tokio::signal::ctrl_c() => tracing::info!("SIGINT"),
        name = signals.recv() => tracing::info!("{name}"),
    }
    hub.shutdown.cancel();
    tracing::info!("shutting down");
    let _ = tokio::time::timeout(Duration::from_secs(15), ops::shutdown_all(&hub)).await;
    let _ = std::fs::remove_file(&inst_file);
    running.task.abort();
    tracing::info!("bye");
    Ok(())
}

/// Resolves when stdin reaches end of file: the process that started the mock backend is gone.
async fn stdin_closed() {
    use tokio::io::AsyncReadExt;
    let mut stdin = tokio::io::stdin();
    let mut buf = [0u8; 256];
    loop {
        match stdin.read(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
    }
}

/// `--mock`: serve until stdin closes, a signal arrives or the last client leaves.
async fn serve_mock(port: Option<u16>) -> anyhow::Result<()> {
    let token = ref_server::util::random_hex(32);
    let mock = ref_server::mock::start(port.unwrap_or(0), token.clone()).await?;
    let log = mock.log_path();
    if let Err(e) = init_logging(&log) {
        eprintln!("ref-server: cannot open log file {}: {e}", log.display());
    }
    let port = mock.running.addr.port();
    tracing::info!(port, root = %mock.root().display(), "ref-server mock listening");
    println!("{}", serde_json::json!({ "port": port, "token": token }));

    let hub = mock.running.hub.clone();
    let mut signals = StopSignals::new()?;
    tokio::select! {
        _ = hub.shutdown.cancelled() => {}
        _ = stdin_closed() => tracing::info!("stdin closed"),
        _ = tokio::signal::ctrl_c() => tracing::info!("SIGINT"),
        name = signals.recv() => tracing::info!("{name}"),
    }
    hub.shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(15), ops::shutdown_all(&hub)).await;
    mock.running.task.abort();
    tracing::info!("bye");
    Ok(())
}

/// OS requests to stop, besides ctrl-c.
#[cfg(unix)]
struct StopSignals {
    term: tokio::signal::unix::Signal,
    hup: tokio::signal::unix::Signal,
}

#[cfg(unix)]
impl StopSignals {
    fn new() -> std::io::Result<Self> {
        use tokio::signal::unix::{SignalKind, signal};
        Ok(StopSignals {
            term: signal(SignalKind::terminate())?,
            hup: signal(SignalKind::hangup())?,
        })
    }

    async fn recv(&mut self) -> &'static str {
        tokio::select! {
            _ = self.term.recv() => "SIGTERM",
            _ = self.hup.recv() => "SIGHUP",
        }
    }
}

/// OS requests to stop, besides ctrl-c.
#[cfg(windows)]
struct StopSignals {
    brk: tokio::signal::windows::CtrlBreak,
    close: tokio::signal::windows::CtrlClose,
    shutdown: tokio::signal::windows::CtrlShutdown,
}

#[cfg(windows)]
impl StopSignals {
    fn new() -> std::io::Result<Self> {
        use tokio::signal::windows::{ctrl_break, ctrl_close, ctrl_shutdown};
        Ok(StopSignals {
            brk: ctrl_break()?,
            close: ctrl_close()?,
            shutdown: ctrl_shutdown()?,
        })
    }

    async fn recv(&mut self) -> &'static str {
        tokio::select! {
            _ = self.brk.recv() => "CTRL_BREAK",
            _ = self.close.recv() => "CTRL_CLOSE",
            _ = self.shutdown.recv() => "CTRL_SHUTDOWN",
        }
    }
}
