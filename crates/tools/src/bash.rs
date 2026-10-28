use std::io::Write as _;
use std::os::fd::{FromRawFd, OwnedFd};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncReadExt;

use crate::util::{self, display_path, resolve_path, Args};
use crate::*;

pub struct BashTool;

const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_TIMEOUT_SECS: u64 = 3600;
const MAX_OUTPUT_BYTES: usize = 50 * 1024;
const MAX_OUTPUT_LINES: usize = 2000;
/// After the shell exits, keep reading output from lingering children for at most this long.
const DRAIN_GRACE: Duration = Duration::from_millis(250);
const KILL_GRACE: Duration = Duration::from_secs(2);

const DESCRIPTION: &str = "Run a shell command (`bash -c`) in the working directory; stdout+stderr are merged, stdin is closed, colors/pagers are off. Use `workdir` instead of `cd`. Default timeout 120s (`timeout` in seconds, max 3600). Long output keeps the last 50KB / 2000 lines; the full log path is shown. Prefer `read`/`grep`/`find`/`edit` over cat/grep/find/sed for files.";

fn find_shell() -> (PathBuf, &'static str) {
    for p in ["/bin/bash", "/usr/bin/bash", "/usr/local/bin/bash"] {
        if std::path::Path::new(p).exists() {
            return (PathBuf::from(p), "-c");
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let cand = dir.join("bash");
            if cand.is_file() {
                return (cand, "-c");
            }
        }
    }
    if let Some(sh) = std::env::var_os("SHELL").filter(|s| !s.is_empty()) {
        return (PathBuf::from(sh), "-c");
    }
    (PathBuf::from("/bin/sh"), "-c")
}

/// Create a pipe (read, write) with CLOEXEC set.
fn pipe() -> std::io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: fds is a valid 2-element array; pipe2 initializes it on success.
    let rc = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: on success both fds are freshly opened and owned by us.
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

fn kill_group(pgid: i32, sig: libc::c_int) {
    if pgid > 0 {
        // SAFETY: plain syscall; negative pid targets the process group.
        unsafe {
            libc::kill(-pgid, sig);
        }
    }
}

/// Incremental UTF-8 decoder that carries incomplete trailing sequences between chunks.
#[derive(Default)]
struct Utf8Carry {
    pending: Vec<u8>,
}

impl Utf8Carry {
    fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let valid_up_to = match std::str::from_utf8(&self.pending) {
            Ok(_) => self.pending.len(),
            Err(e) if e.error_len().is_none() => e.valid_up_to(),
            Err(_) => self.pending.len(),
        };
        let rest = self.pending.split_off(valid_up_to);
        let out = String::from_utf8_lossy(&self.pending).into_owned();
        self.pending = rest;
        out
    }
    fn finish(&mut self) -> String {
        let out = String::from_utf8_lossy(&self.pending).into_owned();
        self.pending.clear();
        out
    }
}

/// Collects output: everything in memory until it exceeds the model budget, then spills the
/// full log to a file and keeps only a tail in memory.
struct OutputBuf {
    mem: String,
    total_bytes: usize,
    total_lines: usize,
    spill: Option<(PathBuf, std::fs::File)>,
    spill_path: PathBuf,
    spill_failed: bool,
}

impl OutputBuf {
    fn new(call_id: &str) -> Self {
        let safe: String = call_id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
        let name = if safe.is_empty() { format!("bash-{}", std::process::id()) } else { safe };
        OutputBuf {
            mem: String::new(),
            total_bytes: 0,
            total_lines: 0,
            spill: None,
            spill_path: std::env::temp_dir().join("hernees-output").join(format!("{name}.log")),
            spill_failed: false,
        }
    }

    fn push(&mut self, s: &str) {
        self.total_bytes += s.len();
        self.total_lines += s.matches('\n').count();
        self.mem.push_str(s);
        let over = self.mem.len() > MAX_OUTPUT_BYTES || self.total_lines > MAX_OUTPUT_LINES;
        if over && self.spill.is_none() && !self.spill_failed {
            let opened = std::fs::create_dir_all(self.spill_path.parent().unwrap_or(&self.spill_path))
                .and_then(|_| std::fs::File::create(&self.spill_path))
                .and_then(|mut f| f.write_all(self.mem.as_bytes()).map(|_| f));
            match opened {
                Ok(f) => self.spill = Some((self.spill_path.clone(), f)),
                Err(_) => self.spill_failed = true,
            }
        } else if let Some((_, f)) = self.spill.as_mut() {
            let _ = f.write_all(s.as_bytes());
        }
        // Keep at most ~2× the budget in memory.
        if self.mem.len() > MAX_OUTPUT_BYTES * 2 {
            let cut = util::floor_char_boundary(&self.mem, self.mem.len() - MAX_OUTPUT_BYTES - 1024);
            self.mem.drain(..cut);
        }
    }

    /// Final model-facing text (tail-truncated with a notice when needed).
    fn render(&self) -> String {
        let text = collapse_cr(&util::strip_ansi(&self.mem));
        let total_lines = self.total_lines + usize::from(!self.mem.is_empty() && !self.mem.ends_with('\n'));
        let truncated = self.total_bytes > MAX_OUTPUT_BYTES || total_lines > MAX_OUTPUT_LINES;
        if !truncated {
            return text.trim_end_matches('\n').to_string();
        }
        let body = text.trim_end_matches('\n');
        let mut lines: Vec<&str> = body.split('\n').collect();
        // The first in-memory line may be partial after the tail drain; drop it.
        if self.total_bytes > self.mem.len() && lines.len() > 1 {
            lines.remove(0);
        }
        let mut kept: Vec<&str> = Vec::new();
        let mut bytes = 0;
        for l in lines.iter().rev() {
            if kept.len() >= MAX_OUTPUT_LINES || bytes + l.len() + 1 > MAX_OUTPUT_BYTES {
                break;
            }
            bytes += l.len() + 1;
            kept.push(l);
        }
        kept.reverse();
        let where_ = match &self.spill {
            Some((p, _)) => format!("; full output: {}", p.display()),
            None => String::new(),
        };
        format!("[output truncated: showing last {} lines of {total_lines}{where_}]\n{}", kept.len(), kept.join("\n"))
    }
}

/// Emulate carriage-return overwrites (progress bars): keep text after the last `\r` per line.
fn collapse_cr(s: &str) -> String {
    if !s.contains('\r') {
        return s.to_string();
    }
    s.split('\n')
        .map(|line| {
            let line = line.strip_suffix('\r').unwrap_or(line);
            match line.rfind('\r') {
                Some(i) => &line[i + 1..],
                None => line,
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

enum End {
    Exited(std::process::ExitStatus),
    Timeout,
    Cancelled,
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> String {
        DESCRIPTION.to_string()
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "Shell command to run." },
                "timeout": { "type": "integer", "minimum": 1, "maximum": MAX_TIMEOUT_SECS, "description": "Timeout in seconds (default 120, max 3600)." },
                "workdir": { "type": "string", "description": "Directory to run in (default: cwd)." }
            },
            "required": ["command"],
            "additionalProperties": false
        })
    }

    fn permission_subject(&self, input: &Value) -> String {
        Args::lenient(input).s("command").to_string()
    }

    fn title(&self, input: &Value) -> String {
        let cmd = Args::lenient(input).s("command");
        let first = cmd.lines().next().unwrap_or("").trim();
        let (t, _) = util::truncate_chars(first, 120);
        if cmd.trim().lines().count() > 1 { format!("$ {t} …") } else { format!("$ {t}") }
    }

    async fn run(&self, input: Value, ctx: &ToolCtx) -> anyhow::Result<ToolOutput> {
        let args = Args::new(&input)?;
        let command = args.str_req("command")?;
        if command.trim().is_empty() {
            anyhow::bail!("field \"command\" is empty");
        }
        let title = self.title(&input);
        let timeout_secs = args.u64_opt("timeout")?.unwrap_or(DEFAULT_TIMEOUT_SECS).clamp(1, MAX_TIMEOUT_SECS);
        let workdir = match args.str_opt("workdir")? {
            Some(w) => resolve_path(&ctx.cwd, w),
            None => ctx.cwd.clone(),
        };
        if !workdir.is_dir() {
            return Ok(ToolOutput::error(format!("workdir does not exist or is not a directory: {}", display_path(&ctx.cwd, &workdir))).with_title(title));
        }

        let (shell, flag) = find_shell();
        let (read_fd, write_fd) = pipe()?;
        let write_fd2 = write_fd.try_clone()?;
        let mut cmd = tokio::process::Command::new(&shell);
        cmd.arg(flag)
            .arg(command)
            .current_dir(&workdir)
            .stdin(Stdio::null())
            .stdout(Stdio::from(write_fd))
            .stderr(Stdio::from(write_fd2))
            .env("TERM", "dumb")
            .env("PAGER", "cat")
            .env("GIT_PAGER", "cat")
            .env("NO_COLOR", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .kill_on_drop(true);
        // SAFETY: setsid is async-signal-safe; it puts the shell in a new session/process group
        // so the whole tree can be signalled via the negative pgid.
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return Ok(ToolOutput::error(format!("Failed to start {}: {e}", shell.display())).with_title(title)),
        };
        // Drop our copies of the write end (held by `cmd`) so EOF arrives when children exit.
        drop(cmd);
        let pgid = child.id().map(|p| p as i32).unwrap_or(0);
        let mut reader = tokio::net::unix::pipe::Receiver::from_owned_fd(read_fd)?;

        let mut out = OutputBuf::new(&ctx.call_id);
        let mut dec = Utf8Carry::default();
        let mut chunk = vec![0u8; 16 * 1024];
        let deadline = tokio::time::sleep(Duration::from_secs(timeout_secs));
        tokio::pin!(deadline);
        let mut status = None;
        let mut eof = false;

        let emit = |out: &mut OutputBuf, s: String| {
            if !s.is_empty() {
                (ctx.on_output)(&util::strip_ansi(&s));
                out.push(&s);
            }
        };

        let end = loop {
            tokio::select! {
                r = reader.read(&mut chunk), if !eof => match r {
                    Ok(0) | Err(_) => eof = true,
                    Ok(n) => { let s = dec.push(&chunk[..n]); emit(&mut out, s); }
                },
                st = child.wait(), if status.is_none() => {
                    status = Some(st?);
                }
                _ = &mut deadline => break End::Timeout,
                _ = ctx.cancel.cancelled() => break End::Cancelled,
            }
            if let Some(st) = status {
                if eof {
                    break End::Exited(st);
                }
                // Shell exited; drain what lingering children already wrote, then stop.
                loop {
                    match tokio::time::timeout(DRAIN_GRACE, reader.read(&mut chunk)).await {
                        Ok(Ok(n)) if n > 0 => {
                            let s = dec.push(&chunk[..n]);
                            emit(&mut out, s);
                        }
                        _ => break,
                    }
                }
                break End::Exited(st);
            }
        };

        if matches!(end, End::Timeout | End::Cancelled) {
            kill_group(pgid, libc::SIGTERM);
            let exited = tokio::time::timeout(KILL_GRACE, async {
                // Keep draining while waiting so the child isn't blocked on a full pipe.
                loop {
                    tokio::select! {
                        r = reader.read(&mut chunk), if !eof => match r {
                            Ok(0) | Err(_) => eof = true,
                            Ok(n) => { let s = dec.push(&chunk[..n]); emit(&mut out, s); }
                        },
                        st = child.wait() => return st.ok(),
                    }
                }
            })
            .await;
            kill_group(pgid, libc::SIGKILL);
            if exited.is_err() {
                let _ = child.kill().await;
            }
        }
        let tail = dec.finish();
        emit(&mut out, tail);

        let mut text = out.render();
        let push_line = |text: &mut String, line: &str| {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(line);
        };
        let is_error = match end {
            End::Exited(st) => {
                use std::os::unix::process::ExitStatusExt;
                if text.is_empty() {
                    text.push_str("(no output)");
                }
                match (st.code(), st.signal()) {
                    (Some(c), _) => push_line(&mut text, &format!("[exit code: {c}]")),
                    (None, Some(sig)) => push_line(&mut text, &format!("[killed by signal {sig}]")),
                    _ => push_line(&mut text, "[exit status unknown]"),
                }
                false
            }
            End::Timeout => {
                push_line(&mut text, &format!("[command timed out after {timeout_secs}s and was killed; pass a larger `timeout` (max {MAX_TIMEOUT_SECS}) or run it in the background]"));
                true
            }
            End::Cancelled => {
                push_line(&mut text, "[command cancelled by user; process group killed]");
                true
            }
        };
        Ok(ToolOutput { text, is_error, title, images: Vec::new() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ctx, ctx_with_output, run_tool};
    use serde_json::json;
    use std::time::Instant;

    #[tokio::test]
    async fn runs_and_reports_exit_code() {
        let (dir, c) = ctx();
        let out = run_tool(&BashTool, json!({"command": "echo hi; echo err >&2; pwd"}), &c).await;
        assert!(!out.is_error);
        let pwd = std::fs::canonicalize(dir.path()).unwrap();
        assert_eq!(out.text, format!("hi\nerr\n{}\n[exit code: 0]", pwd.display()));
        assert_eq!(out.title, "$ echo hi; echo err >&2; pwd");
        let out = run_tool(&BashTool, json!({"command": "exit 3"}), &c).await;
        assert!(!out.is_error);
        assert_eq!(out.text, "(no output)\n[exit code: 3]");
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let out = run_tool(&BashTool, json!({"command": "basename \"$PWD\"; echo $TERM $NO_COLOR", "workdir": "sub"}), &c).await;
        assert_eq!(out.text, "sub\ndumb 1\n[exit code: 0]");
        let out = run_tool(&BashTool, json!({"command": "printf '\\033[31mred\\033[0m\\n'"}), &c).await;
        assert_eq!(out.text, "red\n[exit code: 0]");
        let out = run_tool(&BashTool, json!({}), &c).await;
        assert!(out.is_error && out.text.contains("missing required field \"command\""));
    }

    #[tokio::test]
    async fn streams_output() {
        let (_dir, c, sink) = ctx_with_output();
        let out = run_tool(&BashTool, json!({"command": "echo one; sleep 0.2; echo two"}), &c).await;
        assert!(out.text.starts_with("one\ntwo"));
        assert_eq!(sink.lock().unwrap().as_str(), "one\ntwo\n");
    }

    #[tokio::test]
    async fn timeout_kills_group() {
        let (_dir, c) = ctx();
        let start = Instant::now();
        let out = run_tool(&BashTool, json!({"command": "echo start; sleep 30 & sleep 30", "timeout": 1}), &c).await;
        assert!(out.is_error);
        assert!(out.text.contains("start") && out.text.contains("timed out after 1s"), "{}", out.text);
        assert!(start.elapsed() < Duration::from_secs(6));
    }

    #[tokio::test]
    async fn cancel_kills_group() {
        let (_dir, c) = ctx();
        let cancel = c.cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            cancel.cancel();
        });
        let start = Instant::now();
        let out = run_tool(&BashTool, json!({"command": "trap '' TERM; sleep 30"}), &c).await;
        assert!(out.is_error && out.text.contains("cancelled"), "{}", out.text);
        // SIGTERM ignored by the shell → SIGKILL after the 2s grace.
        assert!(start.elapsed() < Duration::from_secs(6));
    }

    #[tokio::test]
    async fn background_child_does_not_hang() {
        let (_dir, c) = ctx();
        let start = Instant::now();
        let out = run_tool(&BashTool, json!({"command": "sleep 5 & echo done"}), &c).await;
        assert_eq!(out.text, "done\n[exit code: 0]");
        assert!(start.elapsed() < Duration::from_secs(3));
    }

    #[tokio::test]
    async fn truncates_long_output_and_spills() {
        let (_dir, c) = ctx();
        let out = run_tool(&BashTool, json!({"command": "seq 1 5000"}), &c).await;
        assert!(out.text.starts_with("[output truncated: showing last 2000 lines of 5000; full output: "), "{}", &out.text[..200]);
        assert!(out.text.ends_with("5000\n[exit code: 0]"));
        let path = out.text.split("full output: ").nth(1).unwrap().split(']').next().unwrap();
        let full = std::fs::read_to_string(path).unwrap();
        assert_eq!(full.lines().count(), 5000);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn utf8_carry_and_cr() {
        let mut d = Utf8Carry::default();
        let bytes = "é".as_bytes();
        assert_eq!(d.push(&bytes[..1]), "");
        assert_eq!(d.push(&bytes[1..]), "é");
        assert_eq!(collapse_cr("10%\r50%\r100%\ndone\r\n"), "100%\ndone\n");
    }
}
