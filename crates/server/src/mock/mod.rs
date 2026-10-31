//! Mock mode (`ref-server --mock`): a complete backend with a scripted "LLM", for developing
//! and testing frontends without a provider account.
//!
//! ```text
//!  frontend --ws--> Hub --chat completions over loopback HTTP--> llm.rs --> scripts/
//!                    |
//!                    +-- everything lives in one temporary directory (see `Mock::root`)
//! ```
//!
//! Nothing outside that directory is read or written: no user or project configuration, no
//! real working directories (see `HubOptions::sandbox_cwd`), no real session history.
//!
//! Known limits, not enforced: `Export` writes to a path the client chooses, and `Shell`
//! (`!command` in a frontend) runs a real command; both can reach outside the workspace.
//! Tools called by the scripts only touch the workspace.

mod attempts;
mod config;
mod history;
mod llm;
mod png;
mod script;
mod scripts;
mod workspace;

use crate::{HubOptions, Running, ws};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const ROOT_PREFIX: &str = "ref-mock-";
const ROOT_SUFFIX_LEN: usize = 8;
const LOG_FILE: &str = "server.log";

/// A running mock backend. Dropping it deletes its temporary directory.
pub struct Mock {
    pub running: Running,
    root: PathBuf,
}

impl Mock {
    pub fn log_path(&self) -> PathBuf {
        self.root.join("logs").join(LOG_FILE)
    }

    /// The working directory every session gets.
    pub fn workspace(&self) -> PathBuf {
        self.root.join("workspace")
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl Drop for Mock {
    fn drop(&mut self) {
        // The hub outlives this drop and its session database is open in the directory.
        let _ = self.running.hub.store.close();
        if let Err(e) = std::fs::remove_dir_all(&self.root) {
            eprintln!("ref-server: cannot remove {}: {e}", self.root.display());
        }
    }
}

/// Creates `<tmp>/ref-mock-XXXXXXXX/` and everything in it, and starts listening on
/// 127.0.0.1:`port` (0: any free port).
pub async fn start(port: u16, token: String) -> anyhow::Result<Mock> {
    let name = format!("{ROOT_PREFIX}{}", crate::util::random_hex(ROOT_SUFFIX_LEN));
    let root = std::env::temp_dir().join(name);
    std::fs::create_dir(&root)?;
    match start_in(&root, port, token).await {
        Ok(running) => Ok(Mock { running, root }),
        Err(e) => {
            let _ = std::fs::remove_dir_all(&root);
            Err(e)
        }
    }
}

async fn start_in(root: &Path, port: u16, token: String) -> anyhow::Result<Running> {
    let workspace = root.join("workspace");
    std::fs::create_dir_all(root.join("logs"))?;
    std::fs::create_dir_all(&workspace)?;
    let workspace = dunce::canonicalize(workspace)?;
    workspace::create(&workspace)?;

    let llm = llm::start().await?;
    let cfg = config::load(&root.join("config"), &workspace, llm.port)?;

    let data_dir = root.join("data");
    std::fs::create_dir_all(&data_dir)?;
    let store = crate::store::Store::new(data_dir.clone());
    history::save(&store, &cfg, &workspace.to_string_lossy()).map_err(anyhow::Error::msg)?;
    drop(store);

    let mut opts = HubOptions::new(data_dir);
    opts.home = None;
    opts.config_loader = Arc::new(move |_| cfg.clone());
    opts.idle_timeout = 0;
    opts.sandbox_cwd = Some(workspace);
    let allow = vec!["127.0.0.1".to_string()];
    let (auth, _) = ws::resolve_hosts(&allow, token);
    crate::start(opts, "127.0.0.1", port, auth).await
}
