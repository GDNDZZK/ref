//! `find` tool: glob file search honoring .gitignore.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use globset::{GlobBuilder, GlobMatcher};
use serde_json::Value;

use crate::util::{display_path, opt_str, opt_u64, resolve_path};
use crate::{ToolCtx, ToolOutput};

const DEFAULT_LIMIT: usize = 200;
const MAX_LIMIT: usize = 2000;
const TIMEOUT: Duration = Duration::from_secs(30);
/// Hard cap on scanned entries to keep memory bounded.
const MAX_SCANNED: usize = 2_000_000;

/// Glob filter: basename match when the pattern has no `/`, relative-path match otherwise.
pub struct NameGlob {
    matcher: GlobMatcher,
    by_path: bool,
}

impl NameGlob {
    pub fn new(pattern: &str) -> Result<NameGlob, String> {
        let p = pattern.trim().trim_start_matches("./");
        let by_path = p.contains('/');
        let p = p.trim_start_matches('/');
        let glob = GlobBuilder::new(p)
            .literal_separator(by_path)
            .backslash_escape(true)
            .build()
            .map_err(|e| format!("invalid glob `{pattern}`: {e}"))?;
        Ok(NameGlob { matcher: glob.compile_matcher(), by_path })
    }

    pub fn is_match(&self, root: &Path, path: &Path) -> bool {
        if self.by_path {
            let rel = path.strip_prefix(root).unwrap_or(path);
            self.matcher.is_match(rel)
        } else {
            path.file_name().is_some_and(|n| self.matcher.is_match(n))
        }
    }
}

/// Walker used by find and grep: gitignore respected (even outside git repos), hidden skipped
/// unless `hidden`, `.git` always skipped.
pub fn walker(root: &Path, hidden: bool) -> ignore::Walk {
    let mut b = ignore::WalkBuilder::new(root);
    b.hidden(!hidden)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true)
        .ignore(true)
        .parents(true)
        .require_git(false)
        .follow_links(false)
        .sort_by_file_name(|a, b| a.cmp(b))
        .filter_entry(|e| e.file_name() != ".git");
    b.build()
}

pub async fn run(input: &Value, ctx: &ToolCtx) -> ToolOutput {
    let parsed = (|| -> Result<_, String> {
        let pattern = opt_str(input, "pattern")?.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        let path = opt_str(input, "path")?.map(str::to_string);
        let kind = match opt_str(input, "type")? {
            None => None,
            Some("file" | "f") => Some(false),
            Some("dir" | "d" | "directory") => Some(true),
            Some(o) => return Err(format!("parameter `type` must be \"file\" or \"dir\", got \"{o}\"")),
        };
        let limit = opt_u64(input, "limit")?.map(|v| v as usize).unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
        Ok((pattern, path, kind, limit))
    })();
    let (pattern, path, kind, limit) = match parsed {
        Ok(v) => v,
        Err(e) => return ToolOutput::err(e),
    };
    let root = match &path {
        Some(p) => resolve_path(&ctx.cwd, p),
        None => ctx.cwd.clone(),
    };
    let shown_root = display_path(&ctx.cwd, &root);
    if !root.is_dir() {
        if root.exists() {
            return ToolOutput::err(format!("{shown_root} is not a directory"));
        }
        return ToolOutput::err(crate::read::not_found(&ctx.cwd, &root, &shown_root));
    }
    let glob = match pattern.as_deref().map(NameGlob::new).transpose() {
        Ok(g) => g,
        Err(e) => return ToolOutput::err(e),
    };
    // No pattern: list files only (unless type=dir).
    let kind = kind.or(if glob.is_none() { Some(false) } else { None });
    let hidden = pattern.as_deref().is_some_and(|p| p.starts_with('.') || p.contains("/."));
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel2 = cancel.clone();
    let cwd = ctx.cwd.clone();
    let task = tokio::task::spawn_blocking(move || {
        search(&root, glob.as_ref(), kind, hidden, &cancel2)
    });
    let res = tokio::select! {
        r = task => r,
        _ = ctx.abort.cancelled() => {
            cancel.store(true, Ordering::Relaxed);
            return ToolOutput::err("[aborted]");
        }
    };
    let (mut found, timed_out) = match res {
        Ok(v) => v,
        Err(e) => return ToolOutput::err(format!("find failed: {e}")),
    };
    found.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let total = found.len();
    let mut out = String::new();
    for (p, _, is_dir) in found.iter().take(limit) {
        let mut s = display_path(&cwd, p);
        if *is_dir {
            s.push('/');
        }
        out.push_str(&s);
        out.push('\n');
    }
    if total == 0 {
        let what = pattern.as_deref().map(|p| format!(" matching `{p}`")).unwrap_or_default();
        let mut msg = format!("No files found{what} in {shown_root}");
        if timed_out {
            msg.push_str(" (search timed out; results are incomplete, not proof of absence)");
        }
        return ToolOutput::ok(msg);
    }
    if out.ends_with('\n') {
        out.pop();
    }
    if total > limit {
        out.push_str(&format!(
            "\n[Showing {limit} of {total} results (newest first). Narrow the pattern or path, or raise `limit` (max {MAX_LIMIT})]"
        ));
    }
    if timed_out {
        out.push_str("\n[Search timed out after 30s; results are incomplete]");
    }
    ToolOutput::ok(out)
}

type Found = (PathBuf, SystemTime, bool);

fn search(
    root: &Path,
    glob: Option<&NameGlob>,
    kind: Option<bool>,
    hidden: bool,
    cancel: &AtomicBool,
) -> (Vec<Found>, bool) {
    let start = Instant::now();
    let mut out = vec![];
    for (i, entry) in walker(root, hidden).enumerate() {
        if i % 256 == 0 && (cancel.load(Ordering::Relaxed) || start.elapsed() > TIMEOUT) {
            return (out, true);
        }
        if i > MAX_SCANNED {
            return (out, true);
        }
        let Ok(entry) = entry else { continue };
        if entry.depth() == 0 {
            continue;
        }
        let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
        if kind.is_some_and(|want_dir| want_dir != is_dir) {
            continue;
        }
        if let Some(g) = glob {
            if !g.is_match(root, entry.path()) {
                continue;
            }
        }
        let mtime = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        out.push((entry.into_path(), mtime, is_dir));
    }
    (out, false)
}
