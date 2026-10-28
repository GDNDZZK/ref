//! `grep` tool: regex content search with hashline-tagged output.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::{sinks, SearcherBuilder};
use parking_lot::Mutex;
use serde_json::Value;

use crate::find::{walker, NameGlob};
use crate::store::{Seen, SnapshotStore, MAX_SNAPSHOT_FILE_BYTES};
use crate::util::{
    cap_line, canonical_key, decode_text, display_path, opt_bool, opt_str, opt_u64, req_str,
    resolve_path, split_lines, MAX_OUTPUT_BYTES,
};
use crate::{ToolCtx, ToolOutput};

const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 1000;
const PER_FILE_MULTI: usize = 20;
const MAX_CONTEXT: usize = 20;
const LINE_CAP: usize = 500;
const TIMEOUT: Duration = Duration::from_secs(30);
/// Larger files are skipped entirely.
const MAX_SEARCH_FILE_BYTES: u64 = 64 * 1024 * 1024;

struct Params {
    pattern: String,
    root: PathBuf,
    glob: Option<NameGlob>,
    ignore_case: bool,
    literal: bool,
    context: usize,
    limit: usize,
}

fn parse_params(input: &Value, ctx: &ToolCtx) -> Result<Params, String> {
    let pattern = req_str(input, "pattern")?.to_string();
    if pattern.is_empty() {
        return Err("parameter `pattern` must not be empty".into());
    }
    let root = match opt_str(input, "path")? {
        Some(p) if !p.trim().is_empty() => resolve_path(&ctx.cwd, p),
        _ => ctx.cwd.clone(),
    };
    let glob = match opt_str(input, "glob")? {
        Some(g) if !g.trim().is_empty() => Some(NameGlob::new(g)?),
        _ => None,
    };
    Ok(Params {
        pattern,
        root,
        glob,
        ignore_case: opt_bool(input, "ignore_case")?.unwrap_or(false),
        literal: opt_bool(input, "literal")?.unwrap_or(false),
        context: opt_u64(input, "context")?.map(|v| v as usize).unwrap_or(0).min(MAX_CONTEXT),
        limit: opt_u64(input, "limit")?.map(|v| v as usize).unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT),
    })
}

pub async fn run(input: &Value, ctx: &ToolCtx) -> ToolOutput {
    let p = match parse_params(input, ctx) {
        Ok(p) => p,
        Err(e) => return ToolOutput::err(e),
    };
    let shown_root = display_path(&ctx.cwd, &p.root);
    if !p.root.exists() {
        return ToolOutput::err(crate::read::not_found(&ctx.cwd, &p.root, &shown_root));
    }
    let matcher = {
        let mut b = RegexMatcherBuilder::new();
        b.case_insensitive(p.ignore_case).fixed_strings(p.literal).line_terminator(Some(b'\n'));
        match b.build(&p.pattern) {
            Ok(m) => m,
            Err(e) => return ToolOutput::err(format!("invalid regex `{}`: {e}", p.pattern)),
        }
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel2 = cancel.clone();
    let store = ctx.store.clone();
    let cwd = ctx.cwd.clone();
    let task = tokio::task::spawn_blocking(move || search(&p, &matcher, &cwd, &store, &cancel2, &shown_root));
    let res = tokio::select! {
        r = task => r,
        _ = ctx.abort.cancelled() => {
            cancel.store(true, Ordering::Relaxed);
            return ToolOutput::err("[aborted]");
        }
    };
    match res {
        Ok(text) => ToolOutput::ok(text),
        Err(e) => ToolOutput::err(format!("grep failed: {e}")),
    }
}

struct FileHits {
    path: PathBuf,
    text: String,
    /// All match line numbers (1-based) in this file.
    matches: Vec<usize>,
    snapshot: bool,
}

fn search(
    p: &Params,
    matcher: &RegexMatcher,
    cwd: &Path,
    store: &Mutex<SnapshotStore>,
    cancel: &AtomicBool,
    shown_root: &str,
) -> String {
    let start = Instant::now();
    let mut searcher = SearcherBuilder::new().line_number(true).build();
    let mut files: Vec<FileHits> = vec![];
    let mut shown_total = 0usize; // matches that will be displayed (approx, 20 per extra file)
    let mut stopped_early = false;
    let mut timed_out = false;

    let single_file = p.root.is_file();
    let candidates: Box<dyn Iterator<Item = PathBuf>> = if single_file {
        Box::new(std::iter::once(p.root.clone()))
    } else {
        let root = p.root.clone();
        let glob_root = p.root.clone();
        let glob = p.glob.as_ref();
        Box::new(
            walker(&root, false)
                .filter_map(Result::ok)
                .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
                .filter(move |e| glob.is_none_or(|g| g.is_match(&glob_root, e.path())))
                .map(|e| e.into_path()),
        )
    };

    for path in candidates {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        if start.elapsed() > TIMEOUT {
            timed_out = true;
            break;
        }
        if shown_total >= p.limit {
            stopped_early = true;
            break;
        }
        let Ok(meta) = std::fs::metadata(&path) else { continue };
        if meta.len() > MAX_SEARCH_FILE_BYTES {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Ok(file) = decode_text(bytes) else { continue }; // binary / invalid utf-8
        let mut matches = vec![];
        let r = searcher.search_slice(
            matcher,
            file.text.as_bytes(),
            sinks::Lossy(|lnum, _line| {
                matches.push(lnum as usize);
                Ok(!cancel.load(Ordering::Relaxed))
            }),
        );
        if r.is_err() || matches.is_empty() {
            continue;
        }
        let cap = if files.is_empty() { p.limit } else { PER_FILE_MULTI };
        shown_total += matches.len().min(cap);
        let snapshot = file.text.len() <= MAX_SNAPSHOT_FILE_BYTES;
        files.push(FileHits { path, text: file.text, matches, snapshot });
    }

    if files.is_empty() {
        let mut msg = format!("No matches for /{}/ in {shown_root}", p.pattern);
        if timed_out {
            msg.push_str(" (search timed out after 30s; incomplete, not proof of absence)");
        }
        return msg;
    }

    let multi = files.len() > 1;
    let mut remaining = p.limit;
    let mut out = String::new();
    let mut total_matches = 0usize;
    let mut displayed = 0usize;
    let mut capped_files = 0usize;
    let mut byte_cut = false;
    for f in &files {
        total_matches += f.matches.len();
        if remaining == 0 || byte_cut {
            continue;
        }
        let cap = if multi { PER_FILE_MULTI.min(remaining) } else { remaining };
        let take = f.matches.len().min(cap);
        if take < f.matches.len() {
            capped_files += 1;
        }
        remaining -= take;
        displayed += take;
        let lines = split_lines(&f.text);
        let shown_matches = &f.matches[..take];
        // Rows to display: match lines plus context.
        let mut rows: std::collections::BTreeSet<usize> = Default::default();
        for &m in shown_matches {
            let lo = m.saturating_sub(p.context).max(1);
            let hi = (m + p.context).min(lines.len());
            rows.extend(lo..=hi);
        }
        let match_set: std::collections::HashSet<usize> = shown_matches.iter().copied().collect();
        let key = canonical_key(&f.path);
        let rel = display_path(cwd, &f.path);
        let mut block = String::new();
        let mut seen = vec![];
        let mut prev: Option<usize> = None;
        for n in rows {
            let Some(line) = lines.get(n - 1) else { continue };
            if prev.is_some_and(|p| n > p + 1) {
                block.push_str("...\n");
            }
            prev = Some(n);
            let marker = if match_set.contains(&n) { '*' } else { ' ' };
            let (t, cut) = cap_line(line, LINE_CAP);
            block.push_str(&format!("{marker}{n}:{t}\n"));
            if !cut {
                seen.push(n);
            }
        }
        if take < f.matches.len() {
            block.push_str(&format!(
                "[{} more matches in this file; grep with path={rel} to see them]\n",
                f.matches.len() - take
            ));
        }
        let header = if f.snapshot {
            let tag = store.lock().record(&key, &f.text, Seen::from(seen));
            format!("## {rel}#{tag}\n")
        } else {
            format!("## {rel} (file too large to edit by tag)\n")
        };
        if out.len() + header.len() + block.len() > MAX_OUTPUT_BYTES && !out.is_empty() {
            byte_cut = true;
            displayed -= take;
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&header);
        out.push_str(&block);
    }
    if out.ends_with('\n') {
        out.pop();
    }
    let nfiles = files.len();
    out.push_str(&format!(
        "\n\n[{total_matches} match{} in {nfiles} file{}{}]",
        if total_matches == 1 { "" } else { "es" },
        if nfiles == 1 { "" } else { "s" },
        if displayed < total_matches { format!("; showing {displayed}") } else { String::new() }
    ));
    if capped_files > 0 && multi {
        out.push_str(&format!("\n[At most {PER_FILE_MULTI} matches shown per file when several files match]"));
    }
    if stopped_early || remaining == 0 && displayed < total_matches {
        out.push_str(&format!(
            "\n[Match limit {} reached; more matches may exist. Narrow `path`/`glob`/`pattern` or raise `limit` (max {MAX_LIMIT})]",
            p.limit
        ));
    }
    if byte_cut {
        out.push_str("\n[Output truncated at 50KB; narrow the search]");
    }
    if timed_out {
        out.push_str("\n[Search timed out after 30s; results are incomplete]");
    }
    out
}
