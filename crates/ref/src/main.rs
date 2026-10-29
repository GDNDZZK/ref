//! `ref` — TUI frontend for the ref coding-agent backend (`refd`).

mod actions;
mod app;
mod backend;
mod clipboard;
mod commands;
mod conn;
mod editor;
mod history;
mod keys;
mod state;
mod term;
mod ui;
mod util;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "ref", version, about = "ref — coding agent TUI")]
struct Cli {
    /// Working directory for the new session (default: current directory).
    path: Option<String>,
    /// Attach the most recently updated session of this directory.
    #[arg(short = 'c', long = "continue")]
    cont: bool,
    /// Attach a session by id.
    #[arg(short = 's', long = "session")]
    session: Option<String>,
    /// Split view of all open background sessions (does not create a session).
    #[arg(short = 'w', long = "watch")]
    watch: bool,
    /// Model for the first session (`provider/model`).
    #[arg(long)]
    model: Option<String>,
    /// Agent for new sessions.
    #[arg(long)]
    agent: Option<String>,
    /// Remote backend URL (`ws://host:port`).
    #[arg(long)]
    url: Option<String>,
    /// Token for the remote backend.
    #[arg(long)]
    token: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cwd = match &cli.path {
        Some(p) => {
            let p = std::path::Path::new(p);
            let abs = if p.is_absolute() { p.to_path_buf() } else { std::env::current_dir()?.join(p) };
            let abs = std::fs::canonicalize(&abs).map_err(|e| anyhow::anyhow!("{}: {e}", abs.display()))?;
            if !abs.is_dir() {
                anyhow::bail!("{} is not a directory", abs.display());
            }
            abs
        }
        None => std::env::current_dir()?,
    };
    let cwd = cwd.to_string_lossy().to_string();
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?;
    rt.block_on(async move {
        let opts = app::StartOpts {
            cwd: cwd.clone(),
            cont: cli.cont,
            session: cli.session,
            watch: cli.watch,
            model: cli.model,
            agent: cli.agent,
        };
        let (conn, conn_rx) = conn::start(conn::Target { url: cli.url, token: cli.token, cwd });
        let (app_tx, app_rx) = tokio::sync::mpsc::unbounded_channel();
        term::install_panic_hook();
        ui::markdown::prewarm();
        let mut terminal = term::setup()?;
        let app = app::App::new(conn, opts, app_tx);
        let r = app::run(&mut terminal, app, conn_rx, app_rx).await;
        term::restore();
        r
    })?;
    // Background tasks (clipboard helpers) must not keep us alive.
    std::process::exit(0);
}
