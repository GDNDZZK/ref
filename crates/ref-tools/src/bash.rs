//! `bash` tool: run a command in its own process group with streamed, bounded output.

use std::io::Write as _;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use serde_json::Value;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::util::{
    ceil_boundary, floor_boundary, opt_str, opt_u64, req_str, resolve_path, strip_ansi,
    MAX_OUTPUT_BYTES,
};
use crate::{ToolCtx, ToolOutput};

const DEFAULT_TIMEOUT: u64 = 300;
const MAX_TIMEOUT: u64 = 7200;
const HEAD_BYTES: usize = 20 * 1024;
const TAIL_BYTES: usize = 30 * 1024;
const KILL_GRACE: Duration = Duration::from_secs(2);
const PIPE_GRACE: Duration = Duration::from_secs(1);

/// Accumulates output: everything in memory up to the limit, then head + rolling tail in memory
/// and the full text in a spill file.
struct Collector {
    full: String,
    spilled: Option<(PathBuf, Option<std::fs::File>)>,
    head: String,
    tail: String,
    total: usize,
    /// Incomplete UTF-8 sequence carried between chunks.
    carry: Vec<u8>,
}

impl Collector {
    fn new() -> Self {
        Collector {
            full: String::new(),
            spilled: None,
            head: String::new(),
            tail: String::new(),
            total: 0,
            carry: vec![],
        }
    }

    /// Decode a raw chunk (lossy, carrying split UTF-8 sequences) and return the cleaned text.
    fn decode(&mut self, bytes: &[u8], flush: bool) -> String {
        self.carry.extend_from_slice(bytes);
        let mut buf = std::mem::take(&mut self.carry);
        if !flush {
            // Keep up to 3 trailing bytes of an incomplete sequence for the next chunk.
            let valid_up_to = match std::str::from_utf8(&buf) {
                Ok(_) => buf.len(),
                Err(e) if e.error_len().is_none() => e.valid_up_to(),
                Err(_) => buf.len(),
            };
            self.carry = buf.split_off(valid_up_to);
        }
        let s = String::from_utf8_lossy(&buf).replace("\r\n", "\n");
        strip_ansi(&s)
    }

    fn push(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.total += text.len();
        match &mut self.spilled {
            None => {
                self.full.push_str(text);
                if self.full.len() > MAX_OUTPUT_BYTES {
                    self.start_spill();
                }
            }
            Some((_, file)) => {
                if let Some(f) = file {
                    if f.write_all(text.as_bytes()).is_err() {
                        *file = None;
                    }
                }
                self.tail.push_str(text);
                if self.tail.len() > TAIL_BYTES * 2 {
                    let cut = ceil_boundary(&self.tail, self.tail.len() - TAIL_BYTES);
                    self.tail.drain(..cut);
                }
            }
        }
    }

    fn start_spill(&mut self) {
        let dir = std::env::temp_dir().join("ref-tool-output");
        let path = dir.join(format!("{}.txt", ref_proto::new_id("bash")));
        let file = std::fs::create_dir_all(&dir)
            .ok()
            .and_then(|_| std::fs::File::create(&path).ok())
            .and_then(|mut f| f.write_all(self.full.as_bytes()).ok().map(|_| f));
        let full = std::mem::take(&mut self.full);
        let h = floor_boundary(&full, HEAD_BYTES);
        self.head = full[..h].to_string();
        let t = ceil_boundary(&full, full.len().saturating_sub(TAIL_BYTES).max(h));
        self.tail = full[t..].to_string();
        self.spilled = Some((path, file));
    }

    fn finish(mut self) -> String {
        let rest = self.decode(&[], true);
        self.push(&rest);
        match self.spilled.take() {
            None => collapse_cr(&self.full),
            Some((path, file)) => {
                if self.tail.len() > TAIL_BYTES {
                    let cut = ceil_boundary(&self.tail, self.tail.len() - TAIL_BYTES);
                    self.tail.drain(..cut);
                }
                // Start the tail at a line boundary when cheap to do so.
                if let Some(i) = self.tail.find('\n') {
                    if i < 1024 {
                        self.tail.drain(..=i);
                    }
                }
                let omitted = self.total.saturating_sub(self.head.len() + self.tail.len());
                let saved = if file.is_some() {
                    format!("full output saved to {}", path.display())
                } else {
                    "full output could not be saved".to_string()
                };
                let mut head = collapse_cr(&self.head);
                if !head.ends_with('\n') {
                    head.push('\n');
                }
                format!(
                    "{head}\n[... {omitted} bytes omitted; {saved}; use `read` with offset or `grep` on it to see more ...]\n\n{}",
                    collapse_cr(&self.tail)
                )
            }
        }
    }
}

/// Terminal-style carriage-return redraws (`10%\r50%\r100%`): keep only the last non-empty
/// segment of each line.
fn collapse_cr(s: &str) -> String {
    if !s.contains('\r') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    for (i, line) in s.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line.rsplit('\r').find(|seg| !seg.is_empty()).unwrap_or(""));
    }
    out
}

/// Create a pipe: async read end + blocking write end (both close-on-exec; Stdio dups them).
fn make_pipe() -> std::io::Result<(tokio::net::unix::pipe::Receiver, std::os::fd::OwnedFd)> {
    use std::os::fd::{FromRawFd, OwnedFd};
    let mut fds = [0i32; 2];
    // SAFETY: fds is a valid 2-element array; on success we take ownership of both fds.
    let rc = unsafe { libc::pipe(fds.as_mut_ptr()) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: freshly created, owned exclusively here.
    let (r, w) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    for fd in fds {
        // SAFETY: valid fds owned above. Close-on-exec so only the dup'ed stdio copies reach the child.
        unsafe {
            libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
        }
    }
    let receiver = tokio::net::unix::pipe::Receiver::from_owned_fd(r)?;
    Ok((receiver, w))
}

fn kill_group(pgid: i32, sig: i32) {
    if pgid > 0 {
        // SAFETY: plain syscall; negative pid targets the process group we created.
        unsafe {
            libc::kill(-pgid, sig);
        }
    }
}

enum End {
    Exited(std::process::ExitStatus),
    Timeout,
    Aborted,
    WaitError(String),
}

pub async fn run(input: &Value, ctx: &ToolCtx) -> ToolOutput {
    let parsed = (|| -> Result<_, String> {
        let command = req_str(input, "command")?.to_string();
        if command.trim().is_empty() {
            return Err("parameter `command` must not be empty".into());
        }
        let timeout = opt_u64(input, "timeout")?
            .unwrap_or(DEFAULT_TIMEOUT)
            .min(MAX_TIMEOUT);
        let cwd = match opt_str(input, "cwd")? {
            Some(c) if !c.trim().is_empty() => resolve_path(&ctx.cwd, c),
            _ => ctx.cwd.clone(),
        };
        Ok((command, timeout, cwd))
    })();
    let (command, timeout, cwd) = match parsed {
        Ok(v) => v,
        Err(e) => return ToolOutput::err(e),
    };
    if !cwd.is_dir() {
        return ToolOutput::err(format!(
            "working directory does not exist: {}",
            cwd.display()
        ));
    }

    // One pipe shared by stdout and stderr keeps their relative order.
    let (reader, writer) = match make_pipe() {
        Ok(p) => p,
        Err(e) => return ToolOutput::err(format!("failed to create pipe: {e}")),
    };
    let writer2 = match writer.try_clone() {
        Ok(w) => w,
        Err(e) => return ToolOutput::err(format!("failed to create pipe: {e}")),
    };
    let mut cmd = Command::new("bash");
    cmd.arg("-c")
        .arg(&command)
        .current_dir(&cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::from(writer))
        .stderr(Stdio::from(writer2))
        .env("TERM", "dumb")
        .env("PAGER", "cat")
        .env("GIT_PAGER", "cat")
        .env("NO_COLOR", "1")
        .kill_on_drop(true);
    // SAFETY: setsid is async-signal-safe; runs in the forked child before exec.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let spawned = cmd.spawn();
    // Drop the Command so the parent's copies of the pipe write ends are closed (EOF detection).
    drop(cmd);
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => return ToolOutput::err(format!("failed to start bash: {e}")),
    };
    let pgid = child.id().map(|p| p as i32).unwrap_or(0);

    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let reader_task = tokio::spawn(async move {
        let mut reader = reader;
        let mut buf = vec![0u8; 16 * 1024];
        while let Ok(n) = reader.read(&mut buf).await {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });

    let mut col = Collector::new();
    let progress = ctx.progress.clone();
    let handle_chunk = |col: &mut Collector, bytes: &[u8]| {
        let text = col.decode(bytes, false);
        if !text.is_empty() {
            if let Some(p) = &progress {
                let _ = p.send(text.clone());
            }
            col.push(&text);
        }
    };

    let deadline =
        (timeout > 0).then(|| tokio::time::Instant::now() + Duration::from_secs(timeout));
    let sleep_until = async {
        match deadline {
            Some(d) => tokio::time::sleep_until(d).await,
            None => std::future::pending::<()>().await,
        }
    };
    tokio::pin!(sleep_until);
    let mut pipes_open = true;
    let end = loop {
        tokio::select! {
            chunk = rx.recv(), if pipes_open => match chunk {
                Some(b) => handle_chunk(&mut col, &b),
                None => pipes_open = false,
            },
            status = child.wait() => match status {
                Ok(s) => break End::Exited(s),
                Err(e) => break End::WaitError(e.to_string()),
            },
            _ = &mut sleep_until => break End::Timeout,
            _ = ctx.abort.cancelled() => break End::Aborted,
        }
    };

    if matches!(end, End::Timeout | End::Aborted) {
        kill_group(pgid, libc::SIGTERM);
        if tokio::time::timeout(KILL_GRACE, child.wait())
            .await
            .is_err()
        {
            kill_group(pgid, libc::SIGKILL);
            let _ = tokio::time::timeout(KILL_GRACE, child.wait()).await;
        }
        // Make sure stragglers that ignored SIGTERM are gone too.
        kill_group(pgid, libc::SIGKILL);
    }

    // Drain remaining output; background processes may keep the pipes open, so cap the wait.
    let drain_deadline = tokio::time::Instant::now() + PIPE_GRACE;
    let mut pipes_hung = false;
    while pipes_open {
        match tokio::time::timeout_at(drain_deadline, rx.recv()).await {
            Ok(Some(b)) => handle_chunk(&mut col, &b),
            Ok(None) => pipes_open = false,
            Err(_) => {
                pipes_hung = true;
                break;
            }
        }
    }
    reader_task.abort();

    let mut text = col.finish();
    if text.trim().is_empty() {
        text = "(no output)".to_string();
    }
    let mut is_error = false;
    match end {
        End::Exited(status) => {
            if let Some(code) = status.code() {
                if code != 0 {
                    is_error = true;
                    text.push_str(&format!("\n[exit code {code}]"));
                }
            } else {
                use std::os::unix::process::ExitStatusExt;
                is_error = true;
                let sig = status.signal().unwrap_or(0);
                text.push_str(&format!("\n[killed by signal {sig}]"));
            }
            if pipes_hung {
                text.push_str("\n[background processes are still running and hold the output pipe; their later output is not captured. Redirect it (`cmd > file 2>&1 &`) to avoid this]");
            }
        }
        End::Timeout => {
            is_error = true;
            text.push_str(&format!(
                "\n[timed out after {timeout} s; process group killed. Pass a larger `timeout` (max {MAX_TIMEOUT}, 0 = none) if needed]"
            ));
        }
        End::Aborted => {
            is_error = true;
            text.push_str("\n[aborted]");
        }
        End::WaitError(e) => {
            is_error = true;
            text.push_str(&format!("\n[failed waiting for process: {e}]"));
        }
    }
    ToolOutput {
        text,
        images: vec![],
        is_error,
    }
}
