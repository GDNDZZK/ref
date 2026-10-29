use std::process::ExitCode;

use clap::Parser;
use refd::cli::Cli;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("refd: failed to start runtime: {e}");
            return ExitCode::from(2);
        }
    };
    let code = rt.block_on(async move {
        if cli.status {
            return refd::registry::status_cmd(cli.json).await;
        }
        if let Some(pid) = cli.stop {
            return refd::registry::stop_cmd(pid).await;
        }
        match refd::server::run(cli.serve_opts()).await {
            Ok(()) => 0,
            Err(e) => {
                tracing::error!("refd: {e:#}");
                eprintln!("refd: {e:#}");
                1
            }
        }
    });
    rt.shutdown_timeout(std::time::Duration::from_secs(2));
    ExitCode::from(code as u8)
}
