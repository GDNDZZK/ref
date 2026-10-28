//! `bash`: run a shell command with combined, streamed, truncated output.

use crate::util;
use crate::{ToolCtx, ToolOutput, ToolSpec};
use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use std::io::Write as _;
use std::os::fd::{FromRawFd, OwnedFd};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;

pub const DEFAULT_TIMEOUT_SECS: u64 = 120;
pub const MAX_TIMEOUT_SECS: u64 = 3600;
/// Inline output budget; above this (or [`MAX_LINES`]) the middle is elided.
pub const MAX_BYTES: usize = 50 * 1024;
pub const MAX_LINES: usize = 2000;
pub const HEAD_BYTES: usize = 20 * 1024;
pub const TAIL_BYTES: usize = 30 * 1024;
const HEAD_LINES: usize = 800;
const TAIL_LINES: usize = 1200;
/// How long to keep reading after the shell exited (background jobs may hold the pipe).
const DRAIN_GRACE: Duration = Duration::from_millis(200);
/// SIGTERM -> SIGKILL grace period.
const KILL_GRACE: Duration = Duration::from_secs(2);
/// Partial lines are flushed once they grow beyond this.
const PENDING_FLUSH: usize = 8 * 1024;

pub fn spec() -> ToolSpec {
    ToolSpec {
        name: "bash",
        description: "Run a shell command with `bash -c` (non-interactive, stdin closed) and return combined stdout+stderr.\n\
- Each call runs in a fresh shell: use `cwd` instead of `cd`, and `&&` to chain dependent commands.\n\
- `timeout` in seconds (default 120, max 3600; 0 = no timeout). On timeout the whole process group is killed.\n\
- Output over 50KB or 2000 lines is truncated in the middle; the full output is saved to a file you can `read`.\n\
- Prefer the dedicated tools for files: `read` (not cat/head/tail), `grep`, `find`, `edit`/`write` (not sed/echo redirection).\n\
- Non-zero exit codes are reported as `[exit code N]`. Background jobs (`cmd &`) must redirect their output (e.g. `> log 2>&1 &`), otherwise it is lost once the command returns.\n\
- Color is disabled (NO_COLOR=1, TERM=dumb) and pagers are set to `cat`."
            .to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "command": {"type": "string", "description": "Shell command to execute."},
                "timeout": {"type": "integer", "minimum": 0, "description": "Timeout in seconds (default 120, max 3600; 0 disables the timeout)."},
                "cwd": {"type": "string", "description": "Working directory, relative to the session directory (default: session directory)."}
            },
            "required": ["command"]
        }),
    }
}

// ---------------------------------------------------------------------------
// Output collection
// ---------------------------------------------------------------------------

/// Turns raw bytes into clean text pieces: UTF-8 decoding across chunk boundaries,
/// ANSI stripping, CRLF -> LF and carriage-return progress collapsing.
#[derive(Default)]
struct LineAssembler {
    pending: Vec<u8>,
}

impl LineAssembler {
    /// Returns cleaned text for all complete lines (or a forced flush of a long partial line).
    fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let cut = match self.pending.iter().rposition(|&b| b == b'\n') {
            Some(i) => i + 1,
            None if self.pending.len() > PENDING_FLUSH => {
                // Flush a long partial line at a UTF-8 boundary.
                match std::str::from_utf8(&self.pending) {
                    Ok(_) => self.pending.len(),
                    Err(e) if e.error_len().is_none() => e.valid_up_to(),
                    Err(_) => self.pending.len(),
                }
            }
            None => return String::new(),
        };
        let rest = self.pending.split_off(cut);
        let chunk = std::mem::replace(&mut self.pending, rest);
        clean(&String::from_utf8_lossy(&chunk))
    }

    fn finish(&mut self) -> String {
        let chunk = std::mem::take(&mut self.pending);
        clean(&String::from_utf8_lossy(&chunk))
    }
}

fn clean(s: &str) -> String {
    let s = util::strip_ansi(s);
    if !s.contains('\r') {
        return s;
    }
    let mut out = String::with_capacity(s.len());
    for seg in s.split_inclusive('\n') {
        let (line, nl) = seg.strip_suffix('\n').map_or((seg, ""), |l| (l, "\n"));
        let line = line.trim_end_matches('\r');
        // `progress 10%\rprogress 20%` -> keep the last redraw.
        out.push_str(line.rsplit('\r').next().unwrap_or(line));
        out.push_str(nl);
    }
    out
}

type Sink = std::sync::Arc<dyn Fn(&str) + Send + Sync>;

static SPILL_COUNTER: AtomicU64 = AtomicU64::new(0);

fn spill_path() -> PathBuf {
    let n = SPILL_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let id = xxhash_rust::xxh32::xxh32(format!("{nanos}-{n}-{}", std::process::id()).as_bytes(), 0);
    std::env::temp_dir()
        .join("ref-tool-output")
        .join(format!("{:08x}{:x}.log", id, nanos as u64 & 0xffff_ffff))
}

/// Keeps head + rolling tail in memory, spills the full output to a file when large.
struct Collector {
    sink: Option<Sink>,
    /// All output while not spilled; afterwards unused.
    mem: String,
    spilled: Option<(PathBuf, std::fs::File)>,
    head: String,
    tail: String,
    total_bytes: usize,
    total_lines: usize,
    spill_failed: bool,
}

impl Collector {
    fn new(sink: Option<Sink>) -> Self {
        Collector {
            sink,
            mem: String::new(),
            spilled: None,
            head: String::new(),
            tail: String::new(),
            total_bytes: 0,
            total_lines: 0,
            spill_failed: false,
        }
    }

    fn push(&mut self, piece: &str) {
        if piece.is_empty() {
            return;
        }
        if let Some(sink) = &self.sink {
            sink(piece);
        }
        self.total_bytes += piece.len();
        self.total_lines += piece.matches('\n').count();
        if self.spilled.is_none() && !self.spill_failed {
            self.mem.push_str(piece);
            if self.mem.len() > MAX_BYTES {
                self.spill();
            }
            return;
        }
        if let Some((_, f)) = &mut self.spilled {
            let _ = f.write_all(piece.as_bytes());
        }
        if self.spill_failed && self.spilled.is_none() && self.head.is_empty() {
            // Could not create the file: still keep head/tail.
            self.split_mem();
        }
        self.tail.push_str(piece);
        if self.tail.len() > TAIL_BYTES * 2 {
            let cut = util::ceil_char_boundary(&self.tail, self.tail.len() - TAIL_BYTES);
            self.tail.drain(..cut);
        }
    }

    fn split_mem(&mut self) {
        let mem = std::mem::take(&mut self.mem);
        let h = util::floor_char_boundary(&mem, HEAD_BYTES);
        self.head = mem[..h].to_string();
        self.tail = mem[h..].to_string();
    }

    fn spill(&mut self) {
        let path = spill_path();
        let file = path
            .parent()
            .map(std::fs::create_dir_all)
            .transpose()
            .and_then(|_| std::fs::File::create(&path));
        match file {
            Ok(mut f) => {
                let _ = f.write_all(self.mem.as_bytes());
                self.spilled = Some((path, f));
            }
            Err(e) => {
                tracing::warn!("bash: cannot create output file {}: {e}", path.display());
                self.spill_failed = true;
            }
        }
        self.split_mem();
    }

    /// Final text and (truncated, saved path).
    fn finish(mut self) -> (String, bool, Option<PathBuf>) {
        if self.spilled.is_none() && !self.spill_failed && self.head.is_empty() {
            if self.mem.lines().count() <= MAX_LINES {
                return (self.mem, false, None);
            }
            self.spill();
        }
        let path = self.spilled.take().map(|(p, mut f)| {
            let _ = f.flush();
            p
        });
        let head = take_head(&self.head);
        let tail = take_tail(&self.tail);
        let shown = head.matches('\n').count() + tail.matches('\n').count();
        let elided = self.total_lines.saturating_sub(shown);
        let mut text = String::with_capacity(head.len() + tail.len() + 64);
        text.push_str(head);
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&format!("[… {elided} lines elided …]\n"));
        text.push_str(tail);
        (text, true, path)
    }
}

/// Head part: at most HEAD_BYTES / HEAD_LINES, cut after a newline when possible.
fn take_head(s: &str) -> &str {
    let mut end = util::floor_char_boundary(s, HEAD_BYTES);
    if let Some((i, _)) = s[..end].match_indices('\n').nth(HEAD_LINES - 1) {
        end = i + 1;
    }
    if end < s.len()
        && let Some(i) = s[..end].rfind('\n') {
            end = i + 1;
        }
    &s[..end]
}

/// Tail part: at most TAIL_BYTES / TAIL_LINES, starting after a newline when possible.
fn take_tail(s: &str) -> &str {
    let mut start = util::ceil_char_boundary(s, s.len().saturating_sub(TAIL_BYTES));
    let body = s[start..].strip_suffix('\n').unwrap_or(&s[start..]);
    if let Some((i, _)) = body.rmatch_indices('\n').nth(TAIL_LINES - 1) {
        start += i + 1;
    } else if start > 0
        && let Some(i) = s[start..].find('\n')
            && start + i + 1 < s.len() {
                start += i + 1;
            }
    &s[start..]
}

// ---------------------------------------------------------------------------
// Process execution
// ---------------------------------------------------------------------------

/// Creates a CLOEXEC pipe: (read end, write end).
fn make_pipe() -> std::io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: `fds` is a valid two-element buffer; on success both fds are fresh and owned by us.
    let rc = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: pipe2 succeeded, the descriptors are open and not owned elsewhere.
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

fn signal_group(pid: Option<u32>, sig: libc::c_int) {
    if let Some(pid) = pid
        && pid > 0 {
            // SAFETY: plain syscall; negative pid targets the process group we created.
            unsafe {
                libc::kill(-(pid as libc::pid_t), sig);
            }
        }
}

fn spawn_shell(command: &str, cwd: &std::path::Path, out: &OwnedFd) -> std::io::Result<tokio::process::Child> {
    let mut last_err = None;
    for shell in ["bash", "sh"] {
        let mut cmd = tokio::process::Command::new(shell);
        cmd.arg("-c")
            .arg(command)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::from(out.try_clone()?))
            .stderr(Stdio::from(out.try_clone()?))
            .env("TERM", "dumb")
            .env("PAGER", "cat")
            .env("GIT_PAGER", "cat")
            .env("MANPAGER", "cat")
            .env("NO_COLOR", "1")
            .env("CLICOLOR", "0")
            .env("GIT_TERMINAL_PROMPT", "0")
            .process_group(0)
            .kill_on_drop(false);
        match cmd.spawn() {
            Ok(child) => return Ok(child),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => last_err = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last_err.unwrap_or_else(|| std::io::Error::other("no shell found")))
}

/// Kills the process group if the tool future is dropped before the command finished.
struct GroupGuard {
    pid: Option<u32>,
    armed: bool,
}

impl Drop for GroupGuard {
    fn drop(&mut self) {
        if self.armed {
            signal_group(self.pid, libc::SIGKILL);
        }
    }
}

enum Ending {
    Exited(std::process::ExitStatus),
    TimedOut(std::process::ExitStatus),
    Cancelled(std::process::ExitStatus),
}

pub async fn run(input: Value, ctx: &ToolCtx) -> Result<ToolOutput> {
    let command = util::req_str(&input, "command")?;
    if command.trim().is_empty() {
        bail!("`command` must not be empty");
    }
    let timeout_secs = util::opt_u64(&input, "timeout")?.unwrap_or(DEFAULT_TIMEOUT_SECS).min(MAX_TIMEOUT_SECS);
    let cwd = match util::opt_str(&input, "cwd")? {
        Some(c) if !c.trim().is_empty() => util::resolve_path(&ctx.cwd, &c),
        _ => ctx.cwd.clone(),
    };
    if !cwd.is_dir() {
        return Ok(ToolOutput::err(format!(
            "Working directory does not exist: {}",
            util::display_path(&ctx.cwd, &cwd)
        )));
    }

    let (read_end, write_end) = make_pipe().map_err(|e| anyhow!("cannot create pipe: {e}"))?;
    let started = Instant::now();
    let spawned = spawn_shell(&command, &cwd, &write_end);
    // Our copy of the write end must be closed so EOF arrives once the children exit.
    drop(write_end);
    let mut child = spawned.map_err(|e| anyhow!("failed to start shell: {e}"))?;
    let pid = child.id();
    let mut guard = GroupGuard { pid, armed: true };
    let mut reader = tokio::net::unix::pipe::Receiver::from_owned_fd(read_end)
        .map_err(|e| anyhow!("cannot read command output: {e}"))?;

    let mut asm = LineAssembler::default();
    let mut col = Collector::new(ctx.on_output.clone());
    let mut buf = vec![0u8; 64 * 1024];
    let mut eof = false;

    let deadline = (timeout_secs > 0).then(|| tokio::time::Instant::now() + Duration::from_secs(timeout_secs));
    let sleep_until = async move {
        match deadline {
            Some(d) => tokio::time::sleep_until(d).await,
            None => std::future::pending::<()>().await,
        }
    };
    tokio::pin!(sleep_until);

    // Phase 1: run until exit, timeout or cancel.
    let mut stop: Option<bool> = None; // Some(true) = timeout, Some(false) = cancel
    let mut status = None;
    while status.is_none() && stop.is_none() {
        tokio::select! {
            r = reader.read(&mut buf), if !eof => match r {
                Ok(0) | Err(_) => eof = true,
                Ok(n) => col.push(&asm.push(&buf[..n])),
            },
            s = child.wait() => status = Some(s.map_err(|e| anyhow!("waiting for command failed: {e}"))?),
            _ = &mut sleep_until => stop = Some(true),
            _ = ctx.cancel.cancelled() => stop = Some(false),
        }
    }

    // Phase 2: terminate the process group if needed.
    if stop.is_some() {
        signal_group(pid, libc::SIGTERM);
        let grace = tokio::time::sleep(KILL_GRACE);
        tokio::pin!(grace);
        loop {
            tokio::select! {
                r = reader.read(&mut buf), if !eof => match r {
                    Ok(0) | Err(_) => eof = true,
                    Ok(n) => col.push(&asm.push(&buf[..n])),
                },
                s = child.wait() => {
                    status = s.ok();
                    break;
                }
                _ = &mut grace => {
                    signal_group(pid, libc::SIGKILL);
                    let _ = child.start_kill();
                    status = child.wait().await.ok();
                    break;
                }
            }
        }
        // Make sure stragglers in the group are gone too.
        signal_group(pid, libc::SIGKILL);
    }

    // Phase 3: drain remaining output briefly (background jobs may keep the pipe open).
    let drain = tokio::time::sleep(DRAIN_GRACE);
    tokio::pin!(drain);
    while !eof {
        tokio::select! {
            r = reader.read(&mut buf) => match r {
                Ok(0) | Err(_) => eof = true,
                Ok(n) => col.push(&asm.push(&buf[..n])),
            },
            _ = &mut drain => break,
        }
    }
    drop(reader);
    guard.armed = false;
    col.push(&asm.finish());
    let duration = started.elapsed();

    let status = status.ok_or_else(|| anyhow!("Command failed: missing exit status"))?;
    let ending = match stop {
        None => Ending::Exited(status),
        Some(true) => Ending::TimedOut(status),
        Some(false) => Ending::Cancelled(status),
    };
    Ok(build_output(col, ending, duration, timeout_secs))
}

fn build_output(col: Collector, ending: Ending, duration: Duration, timeout_secs: u64) -> ToolOutput {
    let total_lines = col.total_lines;
    let total_bytes = col.total_bytes;
    let (mut text, truncated, path) = col.finish();
    if text.ends_with('\n') {
        text.pop();
    }
    let mut notes: Vec<String> = Vec::new();
    if truncated {
        match &path {
            Some(p) => notes.push(format!(
                "[Output truncated: {total_lines} lines ({}). Full output saved to {}; use `read` with offset/limit or `grep` on it]",
                util::human_bytes(total_bytes as u64),
                p.display()
            )),
            None => notes.push(format!(
                "[Output truncated: {total_lines} lines ({})]",
                util::human_bytes(total_bytes as u64)
            )),
        }
    }
    let (status, is_error) = match &ending {
        Ending::Exited(s) => (s, false),
        Ending::TimedOut(s) => {
            notes.push(format!("[Command timed out after {timeout_secs} seconds]"));
            (s, true)
        }
        Ending::Cancelled(s) => {
            notes.push("[Command cancelled]".to_string());
            (s, true)
        }
    };
    let exit_code = status.code();
    if matches!(ending, Ending::Exited(_)) {
        match exit_code {
            Some(0) => {}
            Some(c) => notes.push(format!("[exit code {c}]")),
            None => {
                use std::os::unix::process::ExitStatusExt;
                notes.push(format!("[killed by signal {}]", status.signal().unwrap_or(0)));
            }
        }
    }
    if duration >= Duration::from_secs(1) {
        notes.push(format!("[wall time {:.1}s]", duration.as_secs_f64()));
    }
    if text.trim().is_empty() {
        text = "(no output)".to_string();
    }
    if !notes.is_empty() {
        text.push_str("\n\n");
        text.push_str(&notes.join("\n"));
    }
    ToolOutput {
        text,
        images: Vec::new(),
        meta: Some(json!({
            "exit_code": exit_code,
            "duration_ms": duration.as_millis() as u64,
            "truncated": truncated,
            "output_path": path.map(|p| p.to_string_lossy().into_owned()),
            "timed_out": matches!(ending, Ending::TimedOut(_)),
            "cancelled": matches!(ending, Ending::Cancelled(_)),
        })),
        is_error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_support::{ctx, temp_dir};
    use std::sync::{Arc, Mutex};

    async fn bash(c: &ToolCtx, v: Value) -> ToolOutput {
        crate::execute("bash", v, c).await
    }

    #[tokio::test]
    async fn output_order_and_exit_code() {
        let d = temp_dir("bash");
        let mut c = ctx(&d);
        let streamed = Arc::new(Mutex::new(String::new()));
        let s2 = streamed.clone();
        c.on_output = Some(Arc::new(move |t: &str| s2.lock().unwrap().push_str(t)));
        let o = bash(&c, json!({"command": "echo one; echo two >&2; printf '\\033[31mred\\033[0m\\n'; exit 3"})).await;
        assert!(!o.is_error);
        assert_eq!(o.text, "one\ntwo\nred\n\n[exit code 3]");
        assert_eq!(o.meta.as_ref().unwrap()["exit_code"], json!(3));
        assert_eq!(&*streamed.lock().unwrap(), "one\ntwo\nred\n");
        let o = bash(&c, json!({"command": "pwd", "cwd": "."})).await;
        assert_eq!(o.text, d.to_string_lossy());
        let o = bash(&c, json!({"command": "true"})).await;
        assert_eq!(o.text, "(no output)");
        let o = bash(&c, json!({"command": "echo $TERM $PAGER $NO_COLOR"})).await;
        assert_eq!(o.text, "dumb cat 1");
        let o = bash(&c, json!({"command": "pwd", "cwd": "missing"})).await;
        assert!(o.is_error);
    }

    #[tokio::test]
    async fn timeout_kills_group() {
        let d = temp_dir("bash-timeout");
        let c = ctx(&d);
        let t = Instant::now();
        let o = bash(&c, json!({"command": "echo start; sleep 30 & sleep 30; echo never", "timeout": 1})).await;
        assert!(o.is_error);
        assert!(o.text.starts_with("start\n\n[Command timed out after 1 seconds]"), "{}", o.text);
        assert!(t.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn cancel() {
        let d = temp_dir("bash-cancel");
        let c = ctx(&d);
        let token = c.cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            token.cancel();
        });
        let t = Instant::now();
        let o = bash(&c, json!({"command": "echo hi; trap '' TERM; sleep 20"})).await;
        assert!(o.text.contains("hi\n\n[Command cancelled]"), "{}", o.text);
        assert!(t.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn background_process_does_not_hang() {
        let d = temp_dir("bash-bg");
        let c = ctx(&d);
        let t = Instant::now();
        let o = bash(&c, json!({"command": "sleep 5 & echo done"})).await;
        assert_eq!(o.text, "done");
        assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
    }

    #[tokio::test]
    async fn truncation_and_spill() {
        let d = temp_dir("bash-trunc");
        let c = ctx(&d);
        let o = bash(&c, json!({"command": "seq 1 100000"})).await;
        let m = o.meta.as_ref().unwrap();
        assert_eq!(m["truncated"], json!(true));
        assert!(o.text.starts_with("1\n2\n"));
        assert!(o.text.contains("lines elided …]"));
        assert!(o.text.contains("\n100000\n\n[Output truncated: 100000 lines"), "{}", &o.text[o.text.len() - 300..]);
        assert!(o.text.len() < MAX_BYTES + 1024);
        let path = m["output_path"].as_str().unwrap();
        let full = std::fs::read_to_string(path).unwrap();
        assert_eq!(full.lines().count(), 100000);
        std::fs::remove_file(path).unwrap();

        // Many short lines under the byte budget: line cap applies.
        let o = bash(&c, json!({"command": "seq 1 3000"})).await;
        assert_eq!(o.meta.as_ref().unwrap()["truncated"], json!(true));
        assert!(o.text.lines().count() <= MAX_LINES + 10);
        if let Some(p) = o.meta.as_ref().unwrap()["output_path"].as_str() {
            std::fs::remove_file(p).unwrap();
        }
        let o = bash(&c, json!({"command": "seq 1 1999"})).await;
        assert_eq!(o.meta.as_ref().unwrap()["truncated"], json!(false));
    }

    #[test]
    fn clean_progress_and_utf8() {
        assert_eq!(clean("a\r\nprog 1\rprog 2\nz"), "a\nprog 2\nz");
        let mut asm = LineAssembler::default();
        let bytes = "héllo\n".as_bytes();
        assert_eq!(asm.push(&bytes[..2]), "");
        assert_eq!(asm.push(&bytes[2..]), "héllo\n");
    }
}
