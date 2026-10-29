//! stdio transport: newline-delimited JSON over a child process' stdin/stdout.

use crate::rpc::{truncate, InTx, Incoming};
use anyhow::{anyhow, bail, Context, Result};
use indexmap::IndexMap;
use serde_json::Value;
use std::collections::VecDeque;
use std::path::Path;
use std::process::Stdio as PStdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

const STDERR_TAIL: usize = 20;

pub(crate) struct Stdio {
    /// Outgoing serialized lines (without newline). None once closed.
    out: Mutex<Option<mpsc::UnboundedSender<String>>>,
    child: tokio::sync::Mutex<Option<Child>>,
    pid: Option<u32>,
}

impl Stdio {
    pub fn spawn(
        label: &str,
        command: &[String],
        environment: &IndexMap<String, String>,
        cwd: &Path,
        in_tx: InTx,
    ) -> Result<Stdio> {
        let Some((prog, args)) = command.split_first() else {
            bail!("empty `command` for local MCP server");
        };
        let mut cmd = Command::new(prog);
        cmd.args(args)
            .envs(environment)
            .current_dir(cwd)
            .stdin(PStdio::piped())
            .stdout(PStdio::piped())
            .stderr(PStdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        cmd.process_group(0);
        let mut child = cmd
            .spawn()
            .with_context(|| format!("failed to spawn `{}` (cwd {})", command.join(" "), cwd.display()))?;
        let pid = child.id();
        let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
        let stderr = child.stderr.take().ok_or_else(|| anyhow!("no stderr"))?;

        // stderr → tracing, keeping a short tail for error messages.
        let tail = Arc::new(Mutex::new(VecDeque::<String>::new()));
        let stderr_task = {
            let tail = tail.clone();
            let label = label.to_string();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::debug!(server = %label, "stderr: {line}");
                    let mut t = tail.lock().unwrap();
                    if t.len() >= STDERR_TAIL {
                        t.pop_front();
                    }
                    t.push_back(truncate(&line, 500));
                }
            })
        };

        // writer
        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
        {
            let label = label.to_string();
            let in_tx = in_tx.clone();
            tokio::spawn(async move {
                let mut stdin = stdin;
                while let Some(line) = out_rx.recv().await {
                    let r = async {
                        stdin.write_all(line.as_bytes()).await?;
                        stdin.write_all(b"\n").await?;
                        stdin.flush().await
                    }
                    .await;
                    if let Err(e) = r {
                        tracing::debug!(server = %label, "stdin write failed: {e}");
                        // Usually the process died: let the stdout reader report that first
                        // (it includes the exit + stderr tail, the first reason wins).
                        tokio::time::sleep(Duration::from_millis(800)).await;
                        let _ = in_tx.send(Incoming::Closed(format!("write to server stdin failed: {e}")));
                        return;
                    }
                }
                // dropping stdin closes the pipe
            });
        }

        // reader
        {
            let label = label.to_string();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stdout);
                let mut buf = Vec::new();
                let reason = loop {
                    buf.clear();
                    match reader.read_until(b'\n', &mut buf).await {
                        Ok(0) => break "server process exited".to_string(),
                        Ok(_) => {
                            let line = String::from_utf8_lossy(&buf);
                            let line = line.trim();
                            if line.is_empty() {
                                continue;
                            }
                            match serde_json::from_str::<Value>(line) {
                                Ok(v) => {
                                    if in_tx.send(Incoming::Msg(v)).is_err() {
                                        return;
                                    }
                                }
                                Err(_) => tracing::debug!(server = %label, "non-JSON stdout: {}", truncate(line, 300)),
                            }
                        }
                        Err(e) => break format!("read from server stdout failed: {e}"),
                    }
                };
                let _ = tokio::time::timeout(Duration::from_millis(300), stderr_task).await;
                let tail: Vec<String> = tail.lock().unwrap().iter().cloned().collect();
                let reason = if tail.is_empty() { reason } else { format!("{reason}; stderr: {}", tail.join("\n")) };
                let _ = in_tx.send(Incoming::Closed(reason));
            });
        }

        Ok(Stdio { out: Mutex::new(Some(out_tx)), child: tokio::sync::Mutex::new(Some(child)), pid })
    }

    pub fn send(&self, msg: &Value) -> Result<()> {
        let line = serde_json::to_string(msg)?;
        match &*self.out.lock().unwrap() {
            Some(tx) => tx.send(line).map_err(|_| anyhow!("server stdin closed")),
            None => bail!("transport closed"),
        }
    }

    /// Close stdin, give the server a moment to exit, then kill its process group.
    pub async fn close(&self) {
        self.out.lock().unwrap().take();
        let Some(mut child) = self.child.lock().await.take() else { return };
        if tokio::time::timeout(Duration::from_millis(500), child.wait()).await.is_err() {
            self.signal_group(Signal::Term);
            if tokio::time::timeout(Duration::from_millis(1500), child.wait()).await.is_err() {
                let _ = child.start_kill();
                let _ = child.wait().await;
            }
        }
        // Make sure grandchildren die too.
        self.signal_group(Signal::Kill);
    }

    #[allow(unused_variables)]
    fn signal_group(&self, sig: Signal) {
        #[cfg(unix)]
        if let Some(pid) = self.pid {
            let s = match sig {
                Signal::Term => libc::SIGTERM,
                Signal::Kill => libc::SIGKILL,
            };
            // SAFETY: plain syscall; the group was created by us via process_group(0).
            unsafe {
                libc::killpg(pid as libc::pid_t, s);
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Signal {
    Term,
    Kill,
}

impl Drop for Stdio {
    fn drop(&mut self) {
        // `kill_on_drop` only reaches the direct child; take the whole group down.
        if self.child.get_mut().is_some() {
            self.signal_group(Signal::Kill);
        }
    }
}
