//! Real `!` shell subcommand, option, and argument completion via persistent shell coprocesses.

use crate::hub::Hub;
use ref_proto::ShellCompletionItem;
use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;
const INIT_TIMEOUT: Duration = Duration::from_secs(4);
const QUERY_TIMEOUT: Duration = Duration::from_millis(400);
const KILL_WAIT_TIMEOUT: Duration = Duration::from_millis(200);
const COOLDOWN_DURATION: Duration = Duration::from_secs(60);
const MAX_CONSEC_FAILURES: u8 = 2;

const READY_MARKER: &str = "__REF_READY__";
const SPEC_MARKER: &str = "__REF_SPEC__\t";
const ITEM_MARKER: &str = "__REF_ITEM__\t";

/// Bash completion functions that only provide generic file/minimal fallback.
const GENERIC_BASH_FUNCS: &[&str] = &[
    "_minimal",
    "_comp_complete_minimal",
    "_comp_complete_filedir",
];

// ---------------------------------------------------------------------------
// Pure-Rust Shell Tokenizer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedShellLine {
    pub words: Vec<String>,
    pub cword: usize,
    pub comp_line: String,
    pub prefix: String,
}

struct WrapperSpec {
    name: &'static str,
    arg_flags: &'static [&'static str],
    positional_args: usize,
}

const WRAPPERS: &[WrapperSpec] = &[
    WrapperSpec {
        name: "sudo",
        arg_flags: &[
            "-C",
            "--close-from",
            "-c",
            "--login-class",
            "-D",
            "--chdir",
            "-g",
            "--group",
            "-h",
            "--host",
            "-p",
            "--prompt",
            "-R",
            "--chroot",
            "-r",
            "--role",
            "-t",
            "--type",
            "-T",
            "--command-timeout",
            "-U",
            "--other-user",
            "-u",
            "--user",
        ],
        positional_args: 0,
    },
    WrapperSpec {
        name: "doas",
        arg_flags: &["-a", "-C", "-u"],
        positional_args: 0,
    },
    WrapperSpec {
        name: "env",
        arg_flags: &[
            "-C",
            "--chdir",
            "-P",
            "-S",
            "--split-string",
            "-u",
            "--unset",
            "--default-signal",
            "--ignore-signal",
            "--block-signal",
        ],
        positional_args: 0,
    },
    WrapperSpec {
        name: "nohup",
        arg_flags: &[],
        positional_args: 0,
    },
    WrapperSpec {
        name: "time",
        arg_flags: &["-f", "--format", "-o", "--output"],
        positional_args: 0,
    },
    WrapperSpec {
        name: "nice",
        arg_flags: &["-n", "--adjustment"],
        positional_args: 0,
    },
    WrapperSpec {
        name: "ionice",
        arg_flags: &[
            "-c",
            "--class",
            "-n",
            "--classdata",
            "-P",
            "--pgid",
            "-p",
            "--pid",
            "-u",
            "--uid",
        ],
        positional_args: 0,
    },
    WrapperSpec {
        name: "timeout",
        arg_flags: &["-k", "--kill-after", "-s", "--signal"],
        positional_args: 1,
    },
    WrapperSpec {
        name: "setsid",
        arg_flags: &[],
        positional_args: 0,
    },
    WrapperSpec {
        name: "strace",
        arg_flags: &[
            "-A", "-b", "-E", "-e", "-I", "-o", "-P", "-p", "-S", "-s", "-U", "-u",
        ],
        positional_args: 0,
    },
    WrapperSpec {
        name: "ltrace",
        arg_flags: &[
            "-A", "-a", "-e", "-F", "-l", "-n", "-o", "-p", "-s", "-u", "-x",
        ],
        positional_args: 0,
    },
    WrapperSpec {
        name: "chrt",
        arg_flags: &["-D", "-P", "-T"],
        positional_args: 1,
    },
    WrapperSpec {
        name: "taskset",
        arg_flags: &[],
        positional_args: 1,
    },
    WrapperSpec {
        name: "stdbuf",
        arg_flags: &["-e", "--error", "-i", "--input", "-o", "--output"],
        positional_args: 0,
    },
    WrapperSpec {
        name: "watch",
        arg_flags: &["-n", "--interval"],
        positional_args: 0,
    },
    WrapperSpec {
        name: "command",
        arg_flags: &[],
        positional_args: 0,
    },
    WrapperSpec {
        name: "exec",
        arg_flags: &["-a"],
        positional_args: 0,
    },
    WrapperSpec {
        name: "builtin",
        arg_flags: &[],
        positional_args: 0,
    },
    WrapperSpec {
        name: "xargs",
        arg_flags: &[
            "-a",
            "--arg-file",
            "-d",
            "--delimiter",
            "-E",
            "-e",
            "-I",
            "-L",
            "-l",
            "-n",
            "--max-args",
            "-P",
            "--max-procs",
            "-s",
            "--max-chars",
        ],
        positional_args: 0,
    },
    WrapperSpec {
        name: "caffeinate",
        arg_flags: &["-t", "-w"],
        positional_args: 0,
    },
    WrapperSpec {
        name: "pkexec",
        arg_flags: &["--user"],
        positional_args: 0,
    },
];

fn find_wrapper(tok: &str) -> Option<&'static WrapperSpec> {
    let base = tok.rsplit(['/', '\\']).next().unwrap_or(tok);
    WRAPPERS.iter().find(|w| w.name == base)
}

fn wrapper_flag_takes_next_arg(w: &WrapperSpec, tok: &str) -> bool {
    if tok.contains('=') {
        return false;
    }
    if tok.starts_with("--") {
        return w.arg_flags.contains(&tok);
    }
    let Some(short) = tok.strip_prefix('-') else {
        return false;
    };
    for (idx, ch) in short.char_indices() {
        let is_arg_flag = w
            .arg_flags
            .iter()
            .any(|f| f.len() == 2 && f.starts_with('-') && f.as_bytes()[1] as char == ch);
        if is_arg_flag {
            return idx + ch.len_utf8() >= short.len();
        }
    }
    false
}

fn is_env_assign(tok: &str) -> bool {
    tok.split_once('=').is_some_and(|(k, _)| {
        !k.is_empty()
            && k.bytes().enumerate().all(|(idx, b)| {
                b == b'_' || b.is_ascii_alphabetic() || (idx > 0 && b.is_ascii_digit())
            })
    })
}

fn floor_char_boundary(s: &str, mut idx: usize) -> usize {
    idx = idx.min(s.len());
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

/// Returns the active command segment after the last unquoted `;`, `|`, `||`, `&&`, `&`, or `\n`.
fn active_segment(before: &str) -> &str {
    let mut it = before.char_indices().peekable();
    let mut quote: Option<char> = None;
    let mut seg_start = 0usize;
    let mut prev_char: Option<char> = None;

    while let Some((idx, c)) = it.next() {
        if let Some(q) = quote {
            if c == '\\' && q == '"' {
                if let Some((_, nc)) = it.next() {
                    prev_char = Some(nc);
                    continue;
                }
            } else if c == q {
                quote = None;
            }
            prev_char = Some(c);
            continue;
        }
        match c {
            '\\' => {
                if let Some((_, nc)) = it.next() {
                    prev_char = Some(nc);
                    continue;
                }
            }
            '\'' | '"' => quote = Some(c),
            ';' | '\n' => seg_start = idx + c.len_utf8(),
            '|' => {
                seg_start = if it.next_if(|&(_, nc)| nc == '|').is_some() {
                    idx + 2
                } else {
                    idx + 1
                };
            }
            '&' => {
                if it.next_if(|&(_, nc)| nc == '&').is_some() {
                    seg_start = idx + 2;
                } else {
                    let is_redir = matches!(prev_char, Some('>' | '<'))
                        || it.peek().is_some_and(|&(_, nc)| nc == '>');
                    if !is_redir {
                        seg_start = idx + 1;
                    }
                }
            }
            _ => {}
        }
        prev_char = Some(c);
    }
    &before[seg_start..]
}

#[derive(Debug)]
struct SegWord {
    raw_start: usize,
    value: String,
    had_quotes: bool,
}

/// Splits `seg` into unquoted words while preserving each token's starting byte offset.
fn parse_segment_words(seg: &str) -> (Vec<SegWord>, bool) {
    let mut words = Vec::new();
    let mut pos = 0usize;

    'outer: loop {
        while let Some(c) = seg[pos..].chars().next() {
            if c.is_whitespace() {
                pos += c.len_utf8();
            } else {
                break;
            }
        }
        if pos >= seg.len() {
            let trailing_ws = seg.chars().next_back().is_some_and(char::is_whitespace);
            return (words, trailing_ws);
        }

        let raw_start = pos;
        let mut value = String::new();
        let mut had_quotes = false;
        let mut quote: Option<char> = None;
        let mut esc = false;

        while let Some(c) = seg[pos..].chars().next() {
            pos += c.len_utf8();
            if esc {
                esc = false;
                if let Some('"') = quote
                    && !matches!(c, '"' | '\\' | '$' | '`' | '\n')
                {
                    value.push('\\');
                }
                if c != '\n' {
                    value.push(c);
                }
                continue;
            }
            if let Some(q) = quote {
                if c == '\\' && q == '"' {
                    esc = true;
                } else if c == q {
                    quote = None;
                } else {
                    value.push(c);
                }
                continue;
            }
            if c.is_whitespace() {
                let trailing_ws = seg[pos - c.len_utf8()..].trim().is_empty();
                words.push(SegWord {
                    raw_start,
                    value,
                    had_quotes,
                });
                if trailing_ws {
                    return (words, true);
                }
                pos -= c.len_utf8();
                continue 'outer;
            }
            if c == '\\' {
                esc = true;
            } else if c == '\'' || c == '"' {
                had_quotes = true;
                quote = Some(c);
            } else {
                value.push(c);
            }
        }
        words.push(SegWord {
            raw_start,
            value,
            had_quotes,
        });
        return (words, false);
    }
}

#[derive(Default)]
struct WrapperScan {
    wrapper: Option<&'static WrapperSpec>,
    outer_wrapper_idx: Option<usize>,
    expect_flag_arg: bool,
    end_of_opts: bool,
    positional_left: usize,
}

impl WrapperScan {
    fn enter(&mut self, idx: usize, w: &'static WrapperSpec) {
        if self.outer_wrapper_idx.is_none() {
            self.outer_wrapper_idx = Some(idx);
        }
        self.wrapper = Some(w);
        self.expect_flag_arg = false;
        self.end_of_opts = false;
        self.positional_left = w.positional_args;
    }
}

/// Tokenizes `line` up to `cursor`, returning `None` when still on the command word itself.
pub(crate) fn tokenize_shell_line(line: &str, cursor: Option<usize>) -> Option<ParsedShellLine> {
    let end = floor_char_boundary(line, cursor.unwrap_or(line.len()));
    let seg = active_segment(&line[..end]);
    let (seg_words, trailing_ws) = parse_segment_words(seg);
    if seg_words.is_empty() {
        return None;
    }

    let completed_len = if trailing_ws {
        seg_words.len()
    } else {
        seg_words.len() - 1
    };

    let mut scan = WrapperScan::default();
    let mut cmd_idx = None;
    for (idx, w) in seg_words.iter().take(completed_len).enumerate() {
        if scan.expect_flag_arg {
            scan.expect_flag_arg = false;
            continue;
        }
        let Some(wrapper) = scan.wrapper else {
            if !w.had_quotes && is_env_assign(&w.value) {
                continue;
            }
            if let Some(next_w) = find_wrapper(&w.value) {
                scan.enter(idx, next_w);
            } else {
                cmd_idx = Some(idx);
                break;
            }
            continue;
        };
        if !scan.end_of_opts && w.value == "--" {
            scan.end_of_opts = true;
            continue;
        }
        if !scan.end_of_opts && w.value.starts_with('-') {
            if wrapper_flag_takes_next_arg(wrapper, &w.value) {
                scan.expect_flag_arg = true;
            }
            continue;
        }
        if scan.positional_left > 0 {
            scan.positional_left -= 1;
            continue;
        }
        if !w.had_quotes && is_env_assign(&w.value) {
            continue;
        }
        if let Some(next_w) = find_wrapper(&w.value) {
            scan.enter(idx, next_w);
            continue;
        }
        cmd_idx = Some(idx);
        break;
    }

    let start_idx = match cmd_idx {
        Some(idx) => idx,
        None => match scan.outer_wrapper_idx {
            Some(idx) if completed_len > idx => idx,
            _ => return None,
        },
    };

    let raw_start = seg_words[start_idx].raw_start;
    let mut words: Vec<String> = seg_words[start_idx..]
        .iter()
        .map(|w| w.value.clone())
        .collect();
    if trailing_ws {
        words.push(String::new());
    }
    if words.len() < 2 {
        return None;
    }
    let cword = words.len() - 1;
    let prefix = words[cword].clone();
    Some(ParsedShellLine {
        words,
        cword,
        comp_line: seg[raw_start..].to_string(),
        prefix,
    })
}

// ---------------------------------------------------------------------------
// Generic Bash Spec Filtering
// ---------------------------------------------------------------------------

/// Splits a `complete -p ...` line respecting bash single/double quotes and backslash escapes.
fn parse_complete_tokens(spec: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_tok = false;
    let mut quote: Option<char> = None;
    let mut esc = false;

    for c in spec.chars() {
        if esc {
            esc = false;
            in_tok = true;
            cur.push(c);
            continue;
        }
        if let Some(q) = quote {
            if c == '\\' && q == '"' {
                esc = true;
            } else if c == q {
                quote = None;
            } else {
                cur.push(c);
            }
            continue;
        }
        if c.is_whitespace() {
            if in_tok {
                out.push(std::mem::take(&mut cur));
                in_tok = false;
            }
            continue;
        }
        in_tok = true;
        if c == '\\' {
            esc = true;
        } else if c == '\'' || c == '"' {
            quote = Some(c);
        } else {
            cur.push(c);
        }
    }
    if in_tok {
        out.push(cur);
    }
    out
}

/// Returns true when a `complete -p` spec has command-specific completion (not generic file-only).
pub(crate) fn is_command_specific_bash_spec(spec_line: &str) -> bool {
    let tokens = parse_complete_tokens(spec_line);
    let mut iter = tokens.into_iter().peekable();
    if iter.peek().is_some_and(|t| t == "complete") {
        iter.next();
    }
    let mut has_specific = false;
    while let Some(tok) = iter.next() {
        match tok.as_str() {
            "-F" => {
                let Some(func) = iter.next() else {
                    return false;
                };
                if func.is_empty() || GENERIC_BASH_FUNCS.contains(&func.as_str()) {
                    return false;
                }
                has_specific = true;
            }
            "-W" | "-C" | "-G" => {
                let Some(arg) = iter.next() else {
                    return false;
                };
                if !arg.is_empty() {
                    has_specific = true;
                }
            }
            "-A" => {
                let Some(action) = iter.next() else {
                    return false;
                };
                if !action.is_empty() && action != "file" && action != "directory" {
                    has_specific = true;
                }
            }
            "-o" | "-P" | "-S" | "-X" => {
                let _ = iter.next();
            }
            _ => {}
        }
    }
    has_specific
}

// ---------------------------------------------------------------------------
// Mock / Sandbox Completions
// ---------------------------------------------------------------------------

struct MockEntry {
    value: &'static str,
    description: &'static str,
}

const DOCKER_MOCK: &[MockEntry] = &[
    MockEntry {
        value: "compose",
        description: "Define and run multi-container applications",
    },
    MockEntry {
        value: "build",
        description: "Build an image from a Dockerfile",
    },
    MockEntry {
        value: "run",
        description: "Create and run a new container from an image",
    },
    MockEntry {
        value: "ps",
        description: "List containers",
    },
    MockEntry {
        value: "images",
        description: "List images",
    },
    MockEntry {
        value: "--help",
        description: "Print usage",
    },
];

const GIT_MOCK: &[MockEntry] = &[
    MockEntry {
        value: "checkout",
        description: "Switch branches or restore working tree files",
    },
    MockEntry {
        value: "commit",
        description: "Record changes to the repository",
    },
    MockEntry {
        value: "status",
        description: "Show the working tree status",
    },
    MockEntry {
        value: "diff",
        description: "Show changes between commits, commit and working tree, etc",
    },
    MockEntry {
        value: "log",
        description: "Show commit logs",
    },
    MockEntry {
        value: "--amend",
        description: "Amend previous commit",
    },
];

const CARGO_MOCK: &[MockEntry] = &[
    MockEntry {
        value: "build",
        description: "Compile the current package",
    },
    MockEntry {
        value: "test",
        description: "Execute all unit and integration tests",
    },
    MockEntry {
        value: "check",
        description: "Analyze the current package without building",
    },
    MockEntry {
        value: "clippy",
        description: "Checks a package to catch common mistakes",
    },
    MockEntry {
        value: "fmt",
        description: "Formats all bin and lib files of the current crate",
    },
    MockEntry {
        value: "--workspace",
        description: "Check all packages in the workspace",
    },
    MockEntry {
        value: "--all-targets",
        description: "Check all targets",
    },
];

pub(crate) fn mock_completions(parsed: &ParsedShellLine, limit: usize) -> Vec<ShellCompletionItem> {
    let cmd = parsed.words[0]
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(&parsed.words[0]);
    let entries = match cmd {
        "docker" => DOCKER_MOCK,
        "git" => GIT_MOCK,
        "cargo" => CARGO_MOCK,
        _ => return Vec::new(),
    };
    let flags_only = parsed.cword >= 2;
    if flags_only && !parsed.prefix.starts_with('-') {
        return Vec::new();
    }
    let p_lower = parsed.prefix.to_lowercase();
    entries
        .iter()
        .filter(|e| !flags_only || e.value.starts_with('-'))
        .filter(|e| p_lower.is_empty() || e.value.to_lowercase().starts_with(&p_lower))
        .take(limit)
        .map(|e| ShellCompletionItem {
            value: e.value.to_string(),
            description: e.description.to_string(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Persistent Shell Coprocess Backend
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShellKind {
    Bash,
    #[cfg(not(windows))]
    Zsh,
    #[cfg(not(windows))]
    Fish,
}

impl ShellKind {
    fn idx(self) -> usize {
        match self {
            Self::Bash => 0,
            #[cfg(not(windows))]
            Self::Zsh => 1,
            #[cfg(not(windows))]
            Self::Fish => 2,
        }
    }
}

#[derive(Default)]
pub struct ShellCompleter {
    backend: Option<ShellBackend>,
    detected_shells: Option<Vec<(ShellKind, PathBuf)>>,
    next_req_id: u64,
    consec_failures: [u8; 3],
    cooldown_until: [Option<Instant>; 3],
}

impl ShellCompleter {
    fn shells(&mut self) -> Vec<(ShellKind, PathBuf)> {
        self.detected_shells
            .get_or_insert_with(detect_shells)
            .clone()
    }

    fn is_on_cooldown(&self, kind: ShellKind, now: Instant) -> bool {
        self.cooldown_until[kind.idx()].is_some_and(|until| now < until)
    }

    fn record_success(&mut self, kind: ShellKind) {
        let i = kind.idx();
        self.consec_failures[i] = 0;
        self.cooldown_until[i] = None;
    }

    fn record_failure(&mut self, kind: ShellKind, force_cooldown: bool, allow_cooldown: bool) {
        let i = kind.idx();
        self.consec_failures[i] = self.consec_failures[i].saturating_add(1);
        if allow_cooldown && (force_cooldown || self.consec_failures[i] >= MAX_CONSEC_FAILURES) {
            self.cooldown_until[i] = Some(Instant::now() + COOLDOWN_DURATION);
        }
    }

    async fn kill_and_reset(&mut self) {
        if let Some(mut b) = self.backend.take() {
            b.kill_and_wait().await;
        }
    }
}

struct ShellBackend {
    kind: ShellKind,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    #[cfg(not(windows))]
    temp_dir: Option<PathBuf>,
}

#[cfg(not(windows))]
impl Drop for ShellBackend {
    fn drop(&mut self) {
        if let Some(dir) = self.temp_dir.take() {
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

async fn read_lossy_line(
    stdout: &mut BufReader<ChildStdout>,
    buf: &mut Vec<u8>,
) -> std::io::Result<Option<String>> {
    buf.clear();
    let n = stdout.read_until(b'\n', buf).await?;
    if n == 0 {
        return Ok(None);
    }
    Ok(Some(
        String::from_utf8_lossy(buf)
            .trim_end_matches(['\r', '\n'])
            .to_string(),
    ))
}

impl ShellBackend {
    async fn kill_and_wait(&mut self) {
        if let Some(pid) = self.child.id() {
            #[cfg(windows)]
            {
                match tokio::task::spawn_blocking(move || ref_sys::kill_tree(pid, true)).await {
                    Ok(true) => {}
                    Ok(false) => tracing::debug!(pid, "kill_tree did not signal coprocess"),
                    Err(e) => tracing::warn!(pid, "kill_tree join failed: {e}"),
                }
            }
            #[cfg(not(windows))]
            if !ref_sys::kill_tree(pid, true) {
                tracing::debug!(pid, "kill_tree did not signal coprocess group");
            }
        }
        if let Err(e) = self.child.start_kill() {
            tracing::debug!("shell_complete start_kill: {e}");
        }
        if let Err(e) = tokio::time::timeout(KILL_WAIT_TIMEOUT, self.child.wait()).await {
            tracing::debug!("shell_complete wait after kill timed out: {e}");
        }
        #[cfg(not(windows))]
        if let Some(dir) = self.temp_dir.take()
            && let Err(e) = std::fs::remove_dir_all(&dir)
        {
            tracing::debug!(dir = %dir.display(), "cleanup temp_dir failed: {e}");
        }
    }

    async fn wait_for_ready(&mut self, timeout: Duration) -> std::io::Result<()> {
        let mut buf = Vec::new();
        let res = tokio::time::timeout(timeout, async {
            loop {
                let Some(line) = read_lossy_line(&mut self.stdout, &mut buf).await? else {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "shell completion coprocess exited during init",
                    ));
                };
                if line == READY_MARKER {
                    return Ok(());
                }
            }
        })
        .await;
        match res {
            Ok(inner) => inner,
            Err(_) => Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "shell completion coprocess init timed out",
            )),
        }
    }

    async fn query(
        &mut self,
        req_id: u64,
        cwd: &str,
        parsed: &ParsedShellLine,
        limit: usize,
    ) -> std::io::Result<Vec<ShellCompletionItem>> {
        let (payload, _req_cleanup) = self.build_query_payload(req_id, cwd, parsed).await?;
        self.stdin.write_all(payload.as_bytes()).await?;
        self.stdin.flush().await?;

        let begin = format!("__REF_BEGIN_{req_id}__");
        let end = format!("__REF_END_{req_id}__");
        let mut buf = Vec::new();
        let mut in_block = false;
        let mut spec_allowed = self.kind != ShellKind::Bash;
        let mut items = Vec::new();
        let mut seen = HashSet::new();

        loop {
            let Some(trimmed) = read_lossy_line(&mut self.stdout, &mut buf).await? else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "shell completion coprocess closed stdout",
                ));
            };
            if trimmed == begin {
                in_block = true;
                continue;
            }
            if trimmed == end {
                return Ok(items);
            }
            if !in_block {
                continue;
            }
            if self.kind == ShellKind::Bash
                && let Some(spec) = trimmed.strip_prefix(SPEC_MARKER)
            {
                spec_allowed = is_command_specific_bash_spec(spec);
                continue;
            }
            if !spec_allowed {
                continue;
            }
            let Some(rest) = trimmed.strip_prefix(ITEM_MARKER) else {
                continue;
            };
            let (raw_val, raw_desc) = match self.kind {
                ShellKind::Bash => (rest, ""),
                #[cfg(not(windows))]
                ShellKind::Zsh | ShellKind::Fish => rest.split_once('\t').unwrap_or((rest, "")),
            };
            let value = raw_val.trim_end_matches(' ').to_string();
            if value.is_empty() || !seen.insert(value.clone()) {
                continue;
            }
            if items.len() < limit {
                items.push(ShellCompletionItem {
                    value,
                    description: raw_desc.trim().to_string(),
                });
            }
        }
    }

    async fn build_query_payload(
        &self,
        req_id: u64,
        cwd: &str,
        parsed: &ParsedShellLine,
    ) -> std::io::Result<(String, Option<TempFileGuard>)> {
        #[cfg(not(windows))]
        if self.kind == ShellKind::Zsh {
            let dir = self
                .temp_dir
                .as_ref()
                .ok_or_else(|| std::io::Error::other("missing zsh temp dir"))?;
            let req_path = dir.join(format!("req-{req_id}.tsv"));
            let line = format_query(self.kind, req_id, cwd, parsed);
            let write_path = req_path.clone();
            tokio::task::spawn_blocking(move || write_private_file(&write_path, line.as_bytes()))
                .await
                .map_err(std::io::Error::other)??;
            let cmd_line = format!("{}\n", req_path.to_string_lossy());
            return Ok((cmd_line, Some(TempFileGuard(req_path))));
        }
        let _ = req_id;
        Ok((format_query(self.kind, req_id, cwd, parsed), None))
    }
}

fn format_query(kind: ShellKind, req_id: u64, cwd: &str, parsed: &ParsedShellLine) -> String {
    let cwd_enc = encode_printf_hex(cwd);
    let line_enc = encode_printf_hex(&parsed.comp_line);
    let mut out = String::new();
    match kind {
        ShellKind::Bash => {
            let _ = writeln!(
                out,
                "{req_id}\t{cwd_enc}\t{line_enc}\t{}\t{}",
                parsed.cword,
                parsed.words.len()
            );
            for w in &parsed.words {
                out.push_str(&encode_printf_hex(w));
                out.push('\n');
            }
        }
        #[cfg(not(windows))]
        ShellKind::Zsh | ShellKind::Fish => {
            let cmd_enc = encode_printf_hex(&parsed.words[0]);
            let _ = writeln!(out, "{req_id}\t{cwd_enc}\t{line_enc}\t{cmd_enc}");
        }
    }
    out
}

/// Encodes every byte as `\xHH` (stripping NUL) so shell `printf '%b'` decodes without `eval`.
fn encode_printf_hex(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 4);
    for &b in s.as_bytes() {
        if b == 0 {
            continue;
        }
        let _ = write!(out, "\\x{b:02x}");
    }
    out
}

// Keep the per-query emission caps inside the bash, zsh, and fish scripts aligned at 500.
const BASH_COPROC_SCRIPT: &str = r#"
for f in /usr/share/bash-completion/bash_completion /etc/bash_completion /opt/homebrew/etc/profile.d/bash_completion.sh /opt/homebrew/share/bash-completion/bash_completion /usr/local/etc/profile.d/bash_completion.sh /usr/local/share/bash-completion/bash_completion /mingw64/share/git/completion/git-completion.bash /usr/share/git/completion/git-completion.bash; do
  if [ -r "$f" ]; then
    . "$f" 2>/dev/null || true
    break
  fi
done
if [ -r /mingw64/share/git/completion/git-completion.bash ]; then
  . /mingw64/share/git/completion/git-completion.bash 2>/dev/null || true
elif [ -r /usr/share/git/completion/git-completion.bash ]; then
  . /usr/share/git/completion/git-completion.bash 2>/dev/null || true
fi
if [ -n "${HOME:-}" ]; then
  [ -r "$HOME/.bash_completion" ] && . "$HOME/.bash_completion" 2>/dev/null || true
  for d in "$HOME/bash_completion.d" "$HOME/.bash_completion.d"; do
    if [ -d "$d" ]; then
      for f in "$d"/*.bash; do
        [ -r "$f" ] && . "$f" 2>/dev/null || true
      done
    fi
  done
fi

_ref_opt_filenames=0
_ref_opt_dirnames=0
_ref_opt_nospace=0
_ref_opt_noquote=0

compopt() {
  local mode=1 opt
  while [ "$#" -gt 0 ]; do
    case "$1" in
      -o) mode=1; shift; [ "$#" -gt 0 ] || break; opt="$1"; shift ;;
      +o) mode=0; shift; [ "$#" -gt 0 ] || break; opt="$1"; shift ;;
      *) shift; continue ;;
    esac
    case "$opt" in
      filenames) _ref_opt_filenames="$mode" ;;
      dirnames)  _ref_opt_dirnames="$mode" ;;
      nospace)   _ref_opt_nospace="$mode" ;;
      noquote)   _ref_opt_noquote="$mode" ;;
    esac
  done
  return 0
}

_ref_escape_path() {
  local s="$1"
  local out="" i=0 len=${#s} ch
  while [ "$i" -lt "$len" ]; do
    ch="${s:i:1}"
    i=$((i + 1))
    case "$ch" in
      ' '|$'\t'|"'"|'"'|'\'|'$'|'`'|'!'|'*'|'?'|'('|')'|'['|']'|'{'|'}'|';'|'|'|'&'|'<'|'>'|'#')
        out+="\\$ch"
        ;;
      *)
        out+="$ch"
        ;;
    esac
  done
  printf '%s' "$out"
}

_ref_parse_spec() {
  local s="$1"
  _ref_spec_fn=""
  _ref_spec_words=""
  _ref_spec_cmd=""
  _ref_spec_glob=""
  _ref_opt_filenames=0
  _ref_opt_dirnames=0
  _ref_opt_nospace=0
  _ref_opt_noquote=0
  local -a toks=()
  local i=0 len=${#s} cur="" in_tok=0 q="" esc=0 ch
  while [ "$i" -lt "$len" ]; do
    ch="${s:i:1}"
    i=$((i + 1))
    if [ "$esc" -eq 1 ]; then
      esc=0
      in_tok=1
      cur+="$ch"
      continue
    fi
    if [ -n "$q" ]; then
      if [ "$ch" = '\' ] && [ "$q" = '"' ]; then
        esc=1
      elif [ "$ch" = "$q" ]; then
        q=""
      else
        cur+="$ch"
      fi
      continue
    fi
    case "$ch" in
      ' '|$'\t')
        if [ "$in_tok" -eq 1 ]; then
          toks+=("$cur")
          cur=""
          in_tok=0
        fi
        ;;
      '\')
        in_tok=1
        esc=1
        ;;
      "'"|'"')
        in_tok=1
        q="$ch"
        ;;
      *)
        in_tok=1
        cur+="$ch"
        ;;
    esac
  done
  [ "$in_tok" -eq 1 ] && toks+=("$cur")

  local has_real=0 idx=0 n=${#toks[@]} tok val
  if [ "$n" -gt 0 ] && [ "${toks[0]}" = "complete" ]; then
    idx=1
  fi
  while [ "$idx" -lt "$n" ]; do
    tok="${toks[$idx]}"
    idx=$((idx + 1))
    case "$tok" in
      -F)
        [ "$idx" -ge "$n" ] && return 1
        val="${toks[$idx]}"
        idx=$((idx + 1))
        case "$val" in
          _minimal|_comp_complete_minimal|_comp_complete_filedir|"") return 1 ;;
          *) _ref_spec_fn="$val"; has_real=1 ;;
        esac
        ;;
      -W)
        [ "$idx" -ge "$n" ] && return 1
        _ref_spec_words="${toks[$idx]}"
        idx=$((idx + 1))
        [ -n "$_ref_spec_words" ] && has_real=1
        ;;
      -C)
        [ "$idx" -ge "$n" ] && return 1
        _ref_spec_cmd="${toks[$idx]}"
        idx=$((idx + 1))
        [ -n "$_ref_spec_cmd" ] && has_real=1
        ;;
      -G)
        [ "$idx" -ge "$n" ] && return 1
        _ref_spec_glob="${toks[$idx]}"
        idx=$((idx + 1))
        [ -n "$_ref_spec_glob" ] && has_real=1
        ;;
      -A)
        [ "$idx" -ge "$n" ] && return 1
        val="${toks[$idx]}"
        idx=$((idx + 1))
        case "$val" in
          file|directory|"") ;;
          *) has_real=1 ;;
        esac
        ;;
      -o)
        [ "$idx" -lt "$n" ] || continue
        val="${toks[$idx]}"
        idx=$((idx + 1))
        case "$val" in
          filenames) _ref_opt_filenames=1 ;;
          dirnames)  _ref_opt_dirnames=1 ;;
          nospace)   _ref_opt_nospace=1 ;;
          noquote)   _ref_opt_noquote=1 ;;
        esac
        ;;
      -P|-S|-X)
        [ "$idx" -lt "$n" ] && idx=$((idx + 1))
        ;;
    esac
  done
  [ "$has_real" -eq 1 ]
}

_ref_run_one() {
  local req_id="$1" cwd_enc="$2" line_enc="$3" cword="$4" nwords="$5"
  local cwd line i wenc w
  IFS=$' \t\n'
  printf -v cwd '%b' "$cwd_enc"
  printf -v line '%b' "$line_enc"
  COMP_WORDS=()
  for ((i=0; i<nwords; i++)); do
    IFS= read -r wenc || return 1
    printf -v w '%b' "$wenc"
    COMP_WORDS+=("$w")
  done
  COMP_CWORD="$cword"
  COMP_LINE="$line"
  COMP_POINT="${#COMP_LINE}"
  local cmd="${COMP_WORDS[0]}"
  local cur="${COMP_WORDS[$COMP_CWORD]:-}"
  local prev=""
  if [ "$COMP_CWORD" -gt 0 ]; then
    prev="${COMP_WORDS[$((COMP_CWORD - 1))]}"
  fi
  printf '__REF_BEGIN_%s__\n' "$req_id"
  if [ -n "$cmd" ] && cd -- "$cwd" 2>/dev/null; then
    local spec=""
    spec=$(complete -p -- "$cmd" 2>/dev/null || true)
    if [ -z "$spec" ] && declare -F _completion_loader >/dev/null 2>&1; then
      _completion_loader "$cmd" 2>/dev/null || true
      spec=$(complete -p -- "$cmd" 2>/dev/null || true)
    fi
    if [ -n "$spec" ]; then
      printf '__REF_SPEC__\t%s\n' "${spec//$'\n'/ }"
      if _ref_parse_spec "$spec"; then
        COMPREPLY=()
        if [ -n "$_ref_spec_fn" ] && declare -F "$_ref_spec_fn" >/dev/null 2>&1; then
          "$_ref_spec_fn" "$cmd" "$cur" "$prev" 2>/dev/null || true
        elif [ -n "$_ref_spec_words" ]; then
          mapfile -t COMPREPLY < <(compgen -W "$_ref_spec_words" -- "$cur" 2>/dev/null || true)
        elif [ -n "$_ref_spec_cmd" ]; then
          mapfile -t COMPREPLY < <(COMP_LINE="$COMP_LINE" COMP_POINT="$COMP_POINT" compgen -C "$_ref_spec_cmd" -- "$cur" 2>/dev/null || true)
        elif [ -n "$_ref_spec_glob" ]; then
          mapfile -t COMPREPLY < <(compgen -G "$_ref_spec_glob" -- "$cur" 2>/dev/null || true)
        fi
        local item count=0
        local cur_base="${cur##*/}" item_base=""
        for item in "${COMPREPLY[@]}"; do
          item="${item//$'\r'/}"
          item="${item//$'\n'/ }"
          item_base="${item%/}"
          item_base="${item_base##*/}"
          if [ "${item_base:0:1}" = "." ] && [ "${cur_base:0:1}" != "." ]; then
            continue
          fi
          if [ "$_ref_opt_filenames" -eq 1 ] || [ "$_ref_opt_dirnames" -eq 1 ]; then
            if [ -d "$item" ] && [ "${item: -1}" != "/" ]; then
              item="${item}/"
            fi
            if [ "$_ref_opt_noquote" -eq 0 ]; then
              item="$(_ref_escape_path "$item")"
            fi
          fi
          printf '__REF_ITEM__\t%s\n' "$item"
          count=$((count + 1))
          if [ "$count" -ge 500 ]; then
            break
          fi
        done
      fi
    fi
  fi
  printf '__REF_END_%s__\n' "$req_id"
}

printf '__REF_READY__\n'
while IFS=$'\t' read -r req_id cwd_enc line_enc cword nwords; do
  _ref_run_one "$req_id" "$cwd_enc" "$line_enc" "$cword" "$nwords" || break
done
"#;

#[cfg(not(windows))]
const ZSH_COPROC_SCRIPT_INNER: &str = r#"
stty -echo 2>/dev/null || true
PROMPT=''
RPROMPT=''
PS1=''
for d in /opt/homebrew/share/zsh/site-functions /usr/local/share/zsh/site-functions /usr/share/zsh/site-functions /usr/share/zsh/vendor-completions; do
  [[ -d "$d" ]] && fpath=("$d" $fpath)
done
autoload -Uz compinit && compinit -u -D
_ref_items=()
compadd() {
  local -a args=("$@") clean_args=() hits=() descs=()
  local desc_var="" skip_next=0 tok i=1
  while (( i <= $#args )); do
    tok="${args[$i]}"
    if (( skip_next )); then
      skip_next=0
      (( i++ ))
      continue
    fi
    case "$tok" in
      --)
        clean_args+=("${args[@]:$((i-1))}")
        break
        ;;
      -O|-A)
        skip_next=1
        ;;
      -d)
        if (( i < $#args )); then
          desc_var="${args[$((i+1))]}"
        fi
        clean_args+=("$tok")
        ;;
      *)
        clean_args+=("$tok")
        ;;
    esac
    (( i++ ))
  done
  builtin compadd -O hits "${clean_args[@]}" 2>/dev/null
  if [[ -n "$desc_var" ]]; then
    descs=("${(@P)desc_var}")
  fi
  local h d desc
  if (( $#hits > 0 )); then
    for h in "${hits[@]}"; do
      desc=""
      if (( $#descs > 0 )); then
        for d in "${descs[@]}"; do
          if [[ "$d" == "$h "* || "$d" == "$h"$'\t'* || "$d" == "$h -- "* ]]; then
            desc="${d#* -- }"
            [[ "$desc" == "$d" ]] && desc=""
            break
          fi
        done
      fi
      h="${h//$'\r'/}"
      h="${h//$'\n'/ }"
      desc="${desc//$'\r'/}"
      desc="${desc//$'\n'/ }"
      _ref_items+=("${h}"$'\t'"${desc}")
    done
  fi
  builtin compadd "$@" 2>/dev/null
}
_ref_widget() {
  _ref_items=()
  zle complete-word 2>/dev/null
  BUFFER=""
  zle accept-line
}
zle -N _ref_widget
bindkey '^I' _ref_widget
LISTMAX=999999
setopt NO_MENU_COMPLETE NO_AUTO_MENU NO_BEEP NO_LIST_BEEP
zstyle ':completion:*' completer _complete
zstyle ':completion:*' list-prompt ''
print -r -- '__REF_ZPTY_READY__'
while IFS=$'\t' read -r op req_file; do
  [[ "$op" == "r" ]] || continue
  local req_id="" cwd_enc="" line_enc="" cmd_enc=""
  IFS=$'\t' read -r req_id cwd_enc line_enc cmd_enc < "$req_file" || continue
  local cwd="" line="" cmd=""
  printf -v cwd '%b' "$cwd_enc"
  printf -v line '%b' "$line_enc"
  printf -v cmd '%b' "$cmd_enc"
  _ref_items=()
  if [[ -n "$cmd" ]] && (( $+_comps[$cmd] )) && [[ "${_comps[$cmd]}" != (_default|_files|_path_files|_nothing) ]] && cd -- "$cwd" 2>/dev/null; then
    local buf="$line"
    _ref_init_buf="$line"
    zle-line-init() {
      BUFFER="$_ref_init_buf"
      CURSOR=${#BUFFER}
      zle _ref_widget
    }
    zle -N zle-line-init
    vared -p '' -c buf </dev/tty >/dev/tty 2>/dev/null || true
    zle -D zle-line-init 2>/dev/null || true
  fi
  print -r -- "__REF_BEGIN_${req_id}__"
  local item count=0
  for item in "${_ref_items[@]}"; do
    print -r -- "__REF_ITEM__"$'\t'"${item}"
    (( count++ ))
    (( count >= 500 )) && break
  done
  print -r -- "__REF_END_${req_id}__"
done
"#;

#[cfg(not(windows))]
const ZSH_COPROC_SCRIPT_OUTER: &str = r#"
setopt NO_BEEP
zmodload zsh/zpty || exit 1
inner="$1"
zpty zcomp zsh -f -i || exit 1
zpty -w zcomp " source '${inner}'"$'\n' || exit 1

local ready_buf="" chunk=""
while zpty -r zcomp chunk; do
  ready_buf+="$chunk"
  if [[ "$ready_buf" == *"__REF_ZPTY_READY__"* ]]; then
    break
  fi
done
print -r -- '__REF_READY__'

while IFS= read -r req_file; do
  local req_id="" _rest=""
  IFS=$'\t' read -r req_id _rest < "$req_file" || continue
  zpty -w zcomp "r"$'\t'"${req_file}"$'\n' || break
  local zbuf="" line="" end_marker="__REF_END_${req_id}__" in_block=0
  while zpty -r zcomp chunk; do
    zbuf+="$chunk"
    if [[ "$zbuf" == *"${end_marker}"$'\n'* || "$zbuf" == *"${end_marker}"$'\r\n'* ]]; then
      break
    fi
  done
  for line in "${(@f)zbuf}"; do
    line="${line//$'\r'/}"
    if [[ "$line" == "__REF_BEGIN_${req_id}__" ]]; then
      in_block=1
      print -r -- "$line"
    elif [[ "$line" == "$end_marker" ]]; then
      print -r -- "$line"
      break
    elif (( in_block )) && [[ "$line" == __REF_ITEM__$'\t'* ]]; then
      print -r -- "$line"
    fi
  done
done
"#;

#[cfg(not(windows))]
const FISH_COPROC_SCRIPT: &str = r#"
printf '__REF_READY__\n'
while read -l raw_line
    set -l parts (string split \t -- "$raw_line")
    if test (count $parts) -lt 4
        continue
    end
    set -l req_id $parts[1]
    set -l cwd (printf '%b' "$parts[2]" | string collect)
    set -l line (printf '%b' "$parts[3]" | string collect)
    set -l cmd (printf '%b' "$parts[4]" | string collect)
    printf '__REF_BEGIN_%s__\n' "$req_id"
    if test -n "$cmd"; and not string match -q -- '-*' "$cmd"; and cd -- "$cwd" 2>/dev/null
        set -l spec (complete -c "$cmd" 2>/dev/null | string collect)
        if test -n "$spec"
            set -l count 0
            for entry in (complete --do-complete="$line" 2>/dev/null)
                set -l cleaned (string replace -a \r '' -- "$entry")
                printf '__REF_ITEM__\t%s\n' "$cleaned"
                set count (math $count + 1)
                if test $count -ge 500
                    break
                end
            end
        end
    end
    printf '__REF_END_%s__\n' "$req_id"
end
"#;

#[cfg(not(windows))]
fn find_on_path(bin: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let p = dir.join(bin);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn detect_shells() -> Vec<(ShellKind, PathBuf)> {
    #[cfg(windows)]
    {
        ref_tools::windows_shells()
            .into_iter()
            .map(|p| (ShellKind::Bash, p))
            .collect()
    }
    #[cfg(not(windows))]
    {
        let mut out = Vec::new();
        let mut push_unique = |kind: ShellKind, path: PathBuf| {
            if !out.iter().any(|(k, p)| *k == kind && *p == path) {
                out.push((kind, path));
            }
        };
        if let Some(shell_env) = std::env::var_os("SHELL") {
            let p = PathBuf::from(&shell_env);
            let base = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let kind = match base {
                "zsh" => Some(ShellKind::Zsh),
                "fish" => Some(ShellKind::Fish),
                "bash" => Some(ShellKind::Bash),
                _ => None,
            };
            if let Some(k) = kind {
                if p.is_file() {
                    push_unique(k, p);
                } else if let Some(found) = find_on_path(base) {
                    push_unique(k, found);
                }
            }
        }
        if cfg!(target_os = "macos") {
            for (k, name) in [
                (ShellKind::Zsh, "zsh"),
                (ShellKind::Fish, "fish"),
                (ShellKind::Bash, "bash"),
            ] {
                if let Some(p) = find_on_path(name) {
                    push_unique(k, p);
                }
            }
        }
        if let Some(p) = find_on_path("bash") {
            push_unique(ShellKind::Bash, p);
        } else if Path::new("/bin/bash").is_file() {
            push_unique(ShellKind::Bash, PathBuf::from("/bin/bash"));
        }
        out
    }
}

#[cfg_attr(windows, allow(dead_code))]
struct TempFileGuard(PathBuf);

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(not(windows))]
fn create_private_zsh_dir() -> std::io::Result<PathBuf> {
    let base = std::env::temp_dir();
    for _ in 0..16 {
        let dir = base.join(format!("ref-zsh-comp-{:032x}", rand::random::<u128>()));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not create unique private zsh temp dir",
    ))
}

#[cfg(not(windows))]
fn write_private_file(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    f.write_all(data)
}

async fn spawn_shell_backend(kind: ShellKind, bin: &Path) -> std::io::Result<ShellBackend> {
    let mut cmd = Command::new(bin);
    #[cfg(not(windows))]
    let mut temp_dir = None;
    let stdin_script = match kind {
        ShellKind::Bash => Some(BASH_COPROC_SCRIPT),
        #[cfg(not(windows))]
        ShellKind::Zsh => {
            let (dir, outer, inner) = tokio::task::spawn_blocking(|| {
                let dir = create_private_zsh_dir()?;
                let outer = dir.join("outer.zsh");
                let inner = dir.join("inner.zsh");
                if let Err(e) = write_private_file(&outer, ZSH_COPROC_SCRIPT_OUTER.as_bytes())
                    .and_then(|()| write_private_file(&inner, ZSH_COPROC_SCRIPT_INNER.as_bytes()))
                {
                    let _ = std::fs::remove_dir_all(&dir);
                    return Err(e);
                }
                Ok((dir, outer, inner))
            })
            .await
            .map_err(std::io::Error::other)??;
            cmd.arg("-f").arg(&outer).arg(&inner);
            temp_dir = Some(dir);
            None
        }
        #[cfg(not(windows))]
        ShellKind::Fish => {
            cmd.args(["--no-config", "-c", FISH_COPROC_SCRIPT]);
            None
        }
    };

    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("LC_ALL", "C.UTF-8")
        .env("TERM", "dumb")
        .env("NO_COLOR", "1")
        .env("CLICOLOR", "0")
        .env("PAGER", "cat")
        .env("GIT_PAGER", "cat")
        .kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);

    let mut child = cmd.spawn()?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| std::io::Error::other("missing coprocess stdin"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("missing coprocess stdout"))?;
    if let Some(script) = stdin_script {
        stdin.write_all(script.as_bytes()).await?;
        if !script.ends_with('\n') {
            stdin.write_all(b"\n").await?;
        }
        stdin.flush().await?;
    }

    let mut backend = ShellBackend {
        kind,
        child,
        stdin,
        stdout: BufReader::new(stdout),
        #[cfg(not(windows))]
        temp_dir,
    };
    if let Err(e) = backend.wait_for_ready(INIT_TIMEOUT).await {
        backend.kill_and_wait().await;
        return Err(e);
    }
    Ok(backend)
}

async fn ensure_backend_with<'a>(
    completer: &'a mut ShellCompleter,
    shells: &[(ShellKind, PathBuf)],
) -> Option<&'a mut ShellBackend> {
    if completer.backend.is_none() {
        let now = Instant::now();
        for (kind, bin) in shells {
            if completer.is_on_cooldown(*kind, now) {
                continue;
            }
            match spawn_shell_backend(*kind, bin).await {
                Ok(b) => {
                    completer.record_success(*kind);
                    completer.backend = Some(b);
                    break;
                }
                Err(e) => {
                    tracing::warn!(shell = %bin.display(), "shell_complete init failed: {e}");
                    completer.record_failure(*kind, true, true);
                }
            }
        }
    }
    completer.backend.as_mut()
}

async fn query_coprocess_with(
    completer: &mut ShellCompleter,
    shells: &[(ShellKind, PathBuf)],
    cwd: &str,
    parsed: &ParsedShellLine,
    limit: usize,
    timeout: Duration,
) -> Vec<ShellCompletionItem> {
    completer.next_req_id += 1;
    let req_id = completer.next_req_id;
    let Some(backend) = ensure_backend_with(completer, shells).await else {
        return Vec::new();
    };
    let kind = backend.kind;
    let res = tokio::time::timeout(timeout, backend.query(req_id, cwd, parsed, limit)).await;
    match res {
        Ok(Ok(mut items)) => {
            completer.record_success(kind);
            items.truncate(limit);
            items
        }
        Ok(Err(e)) => {
            tracing::warn!("shell_complete coprocess io error (respawning lazily): {e}");
            completer.record_failure(kind, true, true);
            completer.kill_and_reset().await;
            Vec::new()
        }
        Err(_) => {
            tracing::warn!("shell_complete coprocess query timed out (killing and respawning)");
            let has_fallback = shells
                .iter()
                .any(|(k, _)| *k != kind && !completer.is_on_cooldown(*k, Instant::now()));
            completer.record_failure(kind, false, has_fallback);
            completer.kill_and_reset().await;
            Vec::new()
        }
    }
}

/// Completes subcommands, options, and arguments for `line` at `cursor` in `cwd`.
pub async fn complete_shell(
    hub: &Hub,
    cwd: &str,
    line: &str,
    cursor: Option<usize>,
    limit: Option<usize>,
) -> Vec<ShellCompletionItem> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let Some(parsed) = tokenize_shell_line(line, cursor) else {
        return Vec::new();
    };
    if hub.opts.sandbox_cwd.is_some() {
        return mock_completions(&parsed, limit);
    }
    let mut completer = hub.shell_completer.lock().await;
    let shells = completer.shells();
    query_coprocess_with(&mut completer, &shells, cwd, &parsed, limit, QUERY_TIMEOUT).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizer_handles_commands_args_quotes_pipelines_and_wrappers() {
        assert_eq!(tokenize_shell_line("", None), None);
        assert_eq!(tokenize_shell_line("git", None), None);
        assert_eq!(tokenize_shell_line("   git", None), None);

        assert_eq!(
            tokenize_shell_line("git ", None),
            Some(ParsedShellLine {
                words: vec!["git".into(), String::new()],
                cword: 1,
                comp_line: "git ".into(),
                prefix: String::new(),
            })
        );
        assert_eq!(
            tokenize_shell_line("git che", Some(7)),
            Some(ParsedShellLine {
                words: vec!["git".into(), "che".into()],
                cword: 1,
                comp_line: "git che".into(),
                prefix: "che".into(),
            })
        );
        assert_eq!(
            tokenize_shell_line("git che extra", Some(7)),
            Some(ParsedShellLine {
                words: vec!["git".into(), "che".into()],
                cword: 1,
                comp_line: "git che".into(),
                prefix: "che".into(),
            })
        );
        assert_eq!(
            tokenize_shell_line("git commit -m \"hello world\" --am", None),
            Some(ParsedShellLine {
                words: vec![
                    "git".into(),
                    "commit".into(),
                    "-m".into(),
                    "hello world".into(),
                    "--am".into(),
                ],
                cword: 4,
                comp_line: "git commit -m \"hello world\" --am".into(),
                prefix: "--am".into(),
            })
        );
        assert_eq!(
            tokenize_shell_line("echo hi 2>&1 | RUST_LOG=debug sudo -u root git che", None),
            Some(ParsedShellLine {
                words: vec!["git".into(), "che".into()],
                cword: 1,
                comp_line: "git che".into(),
                prefix: "che".into(),
            })
        );
        assert_eq!(
            tokenize_shell_line("sudo -u ", None),
            Some(ParsedShellLine {
                words: vec!["sudo".into(), "-u".into(), String::new()],
                cword: 2,
                comp_line: "sudo -u ".into(),
                prefix: String::new(),
            })
        );
        assert_eq!(
            tokenize_shell_line("timeout 5s cargo ch", None),
            Some(ParsedShellLine {
                words: vec!["cargo".into(), "ch".into()],
                cword: 1,
                comp_line: "cargo ch".into(),
                prefix: "ch".into(),
            })
        );
        assert_eq!(
            tokenize_shell_line("git checkout feat\\ br", None),
            Some(ParsedShellLine {
                words: vec!["git".into(), "checkout".into(), "feat br".into()],
                cword: 2,
                comp_line: "git checkout feat\\ br".into(),
                prefix: "feat br".into(),
            })
        );
        assert_eq!(
            tokenize_shell_line("git checkout \"my br", None),
            Some(ParsedShellLine {
                words: vec!["git".into(), "checkout".into(), "my br".into()],
                cword: 2,
                comp_line: "git checkout \"my br".into(),
                prefix: "my br".into(),
            })
        );
        assert_eq!(
            tokenize_shell_line("git \\", None),
            Some(ParsedShellLine {
                words: vec!["git".into(), String::new()],
                cword: 1,
                comp_line: "git \\".into(),
                prefix: String::new(),
            })
        );
    }

    #[test]
    fn filters_generic_file_only_bash_specs() {
        assert!(!is_command_specific_bash_spec(""));
        assert!(!is_command_specific_bash_spec("complete -F _minimal ls"));
        assert!(!is_command_specific_bash_spec(
            "complete -F _comp_complete_minimal cat"
        ));
        assert!(!is_command_specific_bash_spec(
            "complete -F _comp_complete_filedir foo"
        ));
        assert!(!is_command_specific_bash_spec("complete -o default -f foo"));
        assert!(!is_command_specific_bash_spec("complete -d cd"));
        assert!(!is_command_specific_bash_spec("complete -A file foo"));
        assert!(!is_command_specific_bash_spec("complete -A directory bar"));

        assert!(is_command_specific_bash_spec(
            "complete -o bashdefault -o default -o nospace -F __git_wrap__git_main git"
        ));
        assert!(is_command_specific_bash_spec("complete -F _cargo cargo"));
        assert!(is_command_specific_bash_spec(
            "complete -W 'start stop status' myctl"
        ));
        assert!(is_command_specific_bash_spec(
            "complete -C 'aws_completer' aws"
        ));
        assert!(is_command_specific_bash_spec(
            "complete -G '*.txt' myviewer"
        ));
        assert!(is_command_specific_bash_spec("complete -A user id"));
    }

    #[test]
    fn mock_mode_completions_cover_known_and_unknown_commands() {
        let git_p = tokenize_shell_line("git ch", None).unwrap();
        let git_items = mock_completions(&git_p, 50);
        assert_eq!(git_items.len(), 1);
        assert_eq!(git_items[0].value, "checkout");
        assert!(!git_items[0].description.is_empty());

        let docker_p = tokenize_shell_line("docker ", None).unwrap();
        assert_eq!(mock_completions(&docker_p, 3).len(), 3);
        assert_eq!(mock_completions(&docker_p, 50).len(), 6);

        let cargo_flag = tokenize_shell_line("cargo test --WORK", None).unwrap();
        let cargo_items = mock_completions(&cargo_flag, 50);
        assert_eq!(cargo_items.len(), 1);
        assert_eq!(cargo_items[0].value, "--workspace");

        let ls_p = tokenize_shell_line("ls ", None).unwrap();
        assert!(mock_completions(&ls_p, 50).is_empty());
        let cat_p = tokenize_shell_line("cat foo", None).unwrap();
        assert!(mock_completions(&cat_p, 50).is_empty());
        let echo_p = tokenize_shell_line("echo hi", None).unwrap();
        assert!(mock_completions(&echo_p, 50).is_empty());
    }

    #[test]
    fn hex_encoding_never_passes_raw_shell_metacharacters() {
        let enc = encode_printf_hex("$(touch /tmp/pwn)\n`id`\t'\"\\");
        assert!(!enc.contains('$'));
        assert!(!enc.contains('`'));
        assert!(!enc.contains('\n'));
        assert!(!enc.contains('\t'));
        assert!(!enc.contains('\''));
        assert!(!enc.contains('"'));
        assert!(enc.starts_with("\\x24\\x28"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn coprocess_timeout_kills_hung_child_respawns_and_trips_circuit_breaker() {
        let mut cmd = Command::new("bash");
        cmd.args([
            "-c",
            r#"
while IFS=$'\t' read -r req_id _rest; do
  read -r _w1 || exit 0
  read -r _w2 || exit 0
  sleep 2
  printf '__REF_BEGIN_%s__\n__REF_END_%s__\n' "$req_id" "$req_id"
done
"#,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
        let mut child = cmd.spawn().unwrap();
        let hung_pid = child.id().unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());

        let mut completer = ShellCompleter {
            backend: Some(ShellBackend {
                kind: ShellKind::Bash,
                child,
                stdin,
                stdout,
                temp_dir: None,
            }),
            ..ShellCompleter::default()
        };
        let parsed = tokenize_shell_line("git che", None).unwrap();
        let sole_bash = vec![(ShellKind::Bash, PathBuf::from("/bin/bash"))];
        let timed_out = query_coprocess_with(
            &mut completer,
            &sole_bash,
            "/tmp",
            &parsed,
            50,
            Duration::from_millis(80),
        )
        .await;
        assert!(timed_out.is_empty());
        assert!(
            completer.backend.is_none(),
            "hung backend must be dropped for lazy respawn"
        );
        assert!(
            !ref_sys::pid_alive(hung_pid),
            "hung coprocess must be killed on timeout"
        );
        assert_eq!(completer.consec_failures[ShellKind::Bash.idx()], 1);
        assert!(!completer.is_on_cooldown(ShellKind::Bash, Instant::now()));

        // Without a fallback shell, a second query timeout does NOT disable the sole remaining shell.
        completer.record_failure(ShellKind::Bash, false, false);
        assert!(!completer.is_on_cooldown(ShellKind::Bash, Instant::now()));
        // When a fallback shell exists, 2 consecutive failures trip cooldown for that kind.
        completer.record_success(ShellKind::Zsh);
        completer.record_failure(ShellKind::Zsh, false, true);
        assert!(!completer.is_on_cooldown(ShellKind::Zsh, Instant::now()));
        completer.record_failure(ShellKind::Zsh, false, true);
        assert!(completer.is_on_cooldown(ShellKind::Zsh, Instant::now()));

        // Reset cooldown and install a healthy coprocess emitting non-UTF-8 + valid items.
        completer.record_success(ShellKind::Bash);
        let mut ok_cmd = Command::new("bash");
        ok_cmd
            .args([
                "-c",
                r#"
while IFS=$'\t' read -r req_id _rest; do
  read -r _w1 || exit 0
  read -r _w2 || exit 0
  printf '__REF_BEGIN_%s__\n__REF_SPEC__\tcomplete -F __git_wrap__git_main git\n__REF_ITEM__\tcheckout \n__REF_ITEM__\tbad_\xff_name\n__REF_END_%s__\n' "$req_id" "$req_id"
done
"#,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut ok_child = ok_cmd.spawn().unwrap();
        let ok_stdin = ok_child.stdin.take().unwrap();
        let ok_stdout = BufReader::new(ok_child.stdout.take().unwrap());
        completer.backend = Some(ShellBackend {
            kind: ShellKind::Bash,
            child: ok_child,
            stdin: ok_stdin,
            stdout: ok_stdout,
            temp_dir: None,
        });
        let items = query_coprocess_with(
            &mut completer,
            &sole_bash,
            "/tmp",
            &parsed,
            50,
            Duration::from_millis(400),
        )
        .await;
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].value, "checkout");
        assert!(items[1].value.starts_with("bad_"));
        completer.kill_and_reset().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn broken_shell_init_trips_cooldown_and_falls_back_to_bash() {
        let dir = std::env::temp_dir().join(format!("ref-shell-fallback-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let bad_zsh = dir.join("bad-zsh");
        std::fs::write(&bad_zsh, "#!/bin/sh\nexit 1\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bad_zsh, std::fs::Permissions::from_mode(0o755)).unwrap();

        let shells = vec![
            (ShellKind::Zsh, bad_zsh),
            (ShellKind::Bash, PathBuf::from("/bin/bash")),
        ];
        let mut completer = ShellCompleter::default();
        let backend = ensure_backend_with(&mut completer, &shells)
            .await
            .expect("falls back to bash after bad zsh");
        assert_eq!(backend.kind, ShellKind::Bash);
        assert!(completer.is_on_cooldown(ShellKind::Zsh, Instant::now()));
        completer.kill_and_reset().await;
        let _ = std::fs::remove_dir_all(&dir);
    }
}
