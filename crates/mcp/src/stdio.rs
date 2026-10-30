//! stdio transport: newline-delimited JSON-RPC over a child process's stdin/stdout.

use crate::client::Shared;
use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;
use std::collections::BTreeMap;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::Mutex;

/// Maximum size of one incoming JSON-RPC line.
const MAX_FRAME: usize = 16 * 1024 * 1024;

pub(crate) struct StdioTransport {
    stdin: Mutex<Option<ChildStdin>>,
    child: Mutex<Option<Child>>,
    reader: tokio::task::JoinHandle<()>,
}

impl StdioTransport {
    pub(crate) fn spawn(
        name: &str,
        command: &[String],
        env: &BTreeMap<String, String>,
        cwd: Option<&str>,
        shared: Arc<Shared>,
    ) -> Result<Self> {
        let Some((program, args)) = command.split_first() else {
            bail!("MCP server \"{name}\": empty command");
        };
        let mut cmd = Command::new(ref_sys::resolve_program(program));
        cmd.args(args)
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(dir) = cwd.filter(|d| !d.is_empty()) {
            cmd.current_dir(dir);
        }
        let mut child = cmd
            .spawn()
            .with_context(|| format!("MCP server \"{name}\": failed to spawn {program}"))?;
        let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
        if let Some(stderr) = child.stderr.take() {
            let name = name.to_string();
            // Drain stderr continuously so the child never blocks on a full pipe.
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::debug!(server = %name, "stderr: {line}");
                }
            });
        }
        let reader = tokio::spawn(read_loop(BufReader::new(stdout), shared));
        Ok(StdioTransport {
            stdin: Mutex::new(Some(stdin)),
            child: Mutex::new(Some(child)),
            reader,
        })
    }

    pub(crate) async fn send(&self, msg: &Value) -> Result<()> {
        let mut line = serde_json::to_vec(msg)?;
        line.push(b'\n');
        let mut guard = self.stdin.lock().await;
        let stdin = guard
            .as_mut()
            .ok_or_else(|| anyhow!("MCP stdio transport closed"))?;
        stdin
            .write_all(&line)
            .await
            .context("writing to MCP server stdin")?;
        stdin.flush().await.context("flushing MCP server stdin")?;
        Ok(())
    }

    /// Close stdin, give the process 2s to exit, then kill it.
    pub(crate) async fn close(&self) {
        drop(self.stdin.lock().await.take());
        if let Some(mut child) = self.child.lock().await.take() {
            match tokio::time::timeout(Duration::from_secs(2), child.wait()).await {
                Ok(_) => {}
                Err(_) => {
                    let _ = child.kill().await;
                }
            }
        }
        self.reader.abort();
    }
}

async fn read_loop<R: tokio::io::AsyncRead + Unpin>(mut reader: BufReader<R>, shared: Arc<Shared>) {
    let mut buf = Vec::new();
    loop {
        buf.clear();
        // Bounded read_until: stop accumulating past MAX_FRAME.
        let n = match (&mut reader)
            .take(MAX_FRAME as u64 + 1)
            .read_until(b'\n', &mut buf)
            .await
        {
            Ok(n) => n,
            Err(e) => {
                tracing::debug!(server = %shared.name, "stdout read error: {e}");
                break;
            }
        };
        if n == 0 {
            break;
        }
        if buf.len() > MAX_FRAME && buf.last() != Some(&b'\n') {
            tracing::warn!(server = %shared.name, "MCP message exceeds {MAX_FRAME} bytes; closing");
            break;
        }
        let line = String::from_utf8_lossy(&buf);
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(line) {
            Ok(msg) => shared.dispatch(msg),
            // Some servers print banners/logs to stdout; skip non-JSON lines.
            Err(_) => tracing::debug!(server = %shared.name, "non-JSON stdout line: {line}"),
        }
    }
    shared.fail_all("MCP server process exited");
}
