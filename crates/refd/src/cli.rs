use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(name = "refd", version, about = "ref backend daemon")]
pub struct Cli {
    /// Port to listen on (0 = random free port in 10000..60000).
    #[arg(long, default_value_t = 0)]
    pub port: u16,
    /// Address to bind (default 127.0.0.1, or 0.0.0.0 when --allow-host is given).
    #[arg(long)]
    pub bind: Option<String>,
    /// Remote host (IP or hostname, `*` = any) allowed to connect (token still required).
    #[arg(long = "allow-host", value_name = "HOST")]
    pub allow_host: Vec<String>,
    /// Seconds without any frontend before exiting (0 = as soon as the last one leaves, -1 = never).
    #[arg(long, default_value_t = -1, allow_negative_numbers = true)]
    pub idle_timeout: i64,
    /// Print running instances and exit (0 if any is alive, else 1).
    #[arg(long)]
    pub status: bool,
    /// With --status: JSON output.
    #[arg(long, requires = "status")]
    pub json: bool,
    /// Ask a running instance (all instances if no PID is given) to shut down.
    #[arg(long, value_name = "PID", num_args = 0..=1)]
    pub stop: Option<Option<u32>>,
}

/// Options for running the server (also used by in-process tests).
#[derive(Debug, Clone)]
pub struct ServeOpts {
    pub port: u16,
    pub bind: Option<String>,
    pub allow_hosts: Vec<String>,
    pub idle_timeout: i64,
    /// Data directory (sessions, registry).
    pub data_dir: std::path::PathBuf,
    /// Write the registry file.
    pub registry: bool,
    /// Install SIGINT/SIGTERM handlers.
    pub signals: bool,
}

impl Cli {
    pub fn serve_opts(&self) -> ServeOpts {
        ServeOpts {
            port: self.port,
            bind: self.bind.clone(),
            allow_hosts: self.allow_host.clone(),
            idle_timeout: self.idle_timeout,
            data_dir: ref_config::data_dir(),
            registry: true,
            signals: true,
        }
    }
}
