//! `grep`: regex search with hashline-anchored output.

use crate::hashline::{Seen, file_hash, format_header};
use crate::util::{self, TextFile};
use crate::{ToolCtx, ToolOutput, ToolSpec};
use anyhow::{Result, anyhow, bail};
use ignore::WalkBuilder;
use regex::{Regex, RegexBuilder};
use serde_json::{Value, json};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

pub const DEFAULT_LIMIT: usize = 200;
pub const MAX_LIMIT: usize = 2000;
pub const MAX_CONTEXT: usize = 20;
pub const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_LINE_CHARS: usize = 500;
pub const MAX_OUTPUT_BYTES: usize = 60 * 1024;

pub fn spec() -> ToolSpec {
    ToolSpec {
        name: "grep",
        description: "Search file contents with a regular expression (Rust regex syntax; set `literal` for plain text). \
Searches a file or a directory tree; respects .gitignore, includes hidden files, skips .git, binary files and files over 4MB.\n\
Output is grouped by file in hashline format: `[path#TAG]` header, then `*N:line` for matching lines and ` N:line` for context lines, \
`...` between non-adjacent rows. TAG and line numbers can be used directly with `edit` for the rows shown (no re-read needed).\n\
`glob` filters files: `*.rs` matches at any depth, `src/**/*.ts` is relative to the search path. Lines are matched one at a time (no multi-line matches)."
            .to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "Regex (Rust syntax), or plain text if `literal` is true."},
                "path": {"type": "string", "description": "File or directory to search (default: working directory)."},
                "glob": {"type": "string", "description": "Only search files matching this glob, e.g. `*.rs` or `src/**/*.ts`."},
                "ignore_case": {"type": "boolean", "description": "Case-insensitive search (default false)."},
                "literal": {"type": "boolean", "description": "Treat `pattern` as a literal string (default false)."},
                "context": {"type": "integer", "minimum": 0, "description": "Context lines before and after each match (default 0, max 20)."},
                "limit": {"type": "integer", "minimum": 1, "description": "Maximum matching lines to return (default 200, max 2000)."}
            },
            "required": ["pattern"]
        }),
    }
}

pub async fn run(input: Value, ctx: &ToolCtx) -> Result<ToolOutput> {
    let ctx = ctx.clone();
    util::blocking(move || run_sync(&input, &ctx)).await
}

struct FileResult {
    path: PathBuf,
    text: String,
    /// (line 1-based, is_match)
    rows: Vec<(usize, bool)>,
    matches: usize,
}

fn search_file(path: &Path, re: &Regex, context: usize, remaining: usize) -> Option<FileResult> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > MAX_FILE_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if util::looks_binary(&bytes) {
        return None;
    }
    let raw = String::from_utf8(bytes).ok()?;
    let text = TextFile::decode(&raw).text;
    let (lines, _) = util::split_lines(&text);
    let mut hits = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if re.is_match(l) {
            hits.push(i + 1);
            if hits.len() >= remaining {
                break;
            }
        }
    }
    if hits.is_empty() {
        return None;
    }
    let mut rows: Vec<(usize, bool)> = Vec::new();
    let total = lines.len();
    let mut hit_iter = hits.iter().peekable();
    let mut last = 0usize;
    while let Some(&h) = hit_iter.next() {
        let from = h.saturating_sub(context).max(1).max(last + 1);
        let to = (h + context).min(total);
        for l in from..=to {
            let is_match = l == h || hits.binary_search(&l).is_ok();
            rows.push((l, is_match));
            last = l;
        }
        while hit_iter.peek().is_some_and(|&&n| n <= last) {
            hit_iter.next();
        }
    }
    let matches = hits.len();
    drop(lines);
    Some(FileResult {
        path: path.to_path_buf(),
        text,
        rows,
        matches,
    })
}

fn run_sync(input: &Value, ctx: &ToolCtx) -> Result<ToolOutput> {
    let pattern = util::req_str(input, "pattern")?;
    if pattern.is_empty() {
        bail!("Pattern must not be empty");
    }
    let literal = util::opt_bool(input, "literal")?.unwrap_or(false);
    let ignore_case = util::opt_bool(input, "ignore_case")?.unwrap_or(false);
    let context = (util::opt_u64(input, "context")?.unwrap_or(0) as usize).min(MAX_CONTEXT);
    let limit = (util::opt_u64(input, "limit")?.unwrap_or(DEFAULT_LIMIT as u64) as usize)
        .clamp(1, MAX_LIMIT);
    let glob = util::opt_str(input, "glob")?.filter(|g| !g.trim().is_empty());
    let src = if literal {
        regex::escape(&pattern)
    } else {
        pattern.clone()
    };
    let re = RegexBuilder::new(&src)
        .case_insensitive(ignore_case)
        .size_limit(64 * 1024 * 1024)
        .build()
        .map_err(|e| {
            anyhow!("Invalid regex: {e}. Set `literal: true` to search for plain text.")
        })?;

    let root = match util::opt_str(input, "path")? {
        Some(p) if !p.trim().is_empty() => util::resolve_path(&ctx.cwd, &util::unwrap_header(&p).0),
        _ => ctx.cwd.clone(),
    };
    let root_display = util::display_path(&ctx.cwd, &root);
    if !root.exists() {
        return Ok(ToolOutput::err(format!("Path not found: {root_display}")));
    }

    let files: Vec<PathBuf> = if root.is_file() {
        vec![root.clone()]
    } else {
        collect_files(&root, glob.as_deref(), ctx)?
    };

    // Search in sorted order, in parallel batches, stopping at the match limit.
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(16);
    let mut results: Vec<FileResult> = Vec::new();
    let mut total_matches = 0usize;
    let mut hit_limit = false;
    'outer: for batch in files.chunks(threads * 16) {
        if ctx.cancel.is_cancelled() {
            bail!("grep cancelled");
        }
        let remaining = limit - total_matches;
        let per_thread: Vec<Vec<(usize, FileResult)>> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..threads)
                .map(|t| {
                    let re = &re;
                    s.spawn(move || {
                        let mut out = Vec::new();
                        for (i, f) in batch.iter().enumerate().skip(t).step_by(threads) {
                            if let Some(r) = search_file(f, re, context, remaining) {
                                out.push((i, r));
                            }
                        }
                        out
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().unwrap_or_default())
                .collect()
        });
        let mut batch_results: Vec<(usize, FileResult)> =
            per_thread.into_iter().flatten().collect();
        batch_results.sort_by_key(|(i, _)| *i);
        for (_, mut r) in batch_results {
            let room = limit - total_matches;
            if r.matches > room {
                truncate_rows(&mut r, room);
            }
            total_matches += r.matches;
            results.push(r);
            if total_matches >= limit {
                hit_limit = true;
                break 'outer;
            }
        }
    }

    if results.is_empty() {
        let mut o = ToolOutput::ok("No matches found.");
        o.meta = Some(json!({"matches": 0, "files": 0}));
        return Ok(o);
    }

    let mut out = String::new();
    let mut shown_files = 0;
    let mut bytes_limited = false;
    for r in &results {
        let display = util::display_path(&ctx.cwd, &r.path);
        let tag = file_hash(&r.text);
        let (lines, _) = util::split_lines(&r.text);
        let mut block = format_header(&display, &tag);
        let mut seen = Vec::new();
        let mut prev = 0;
        for &(l, is_match) in &r.rows {
            if prev != 0 && l != prev + 1 {
                block.push_str("\n...");
            }
            let (text, cut) = util::truncate_chars(lines[l - 1], MAX_LINE_CHARS);
            let _ = write!(block, "\n{}{l}:{text}", if is_match { '*' } else { ' ' });
            if !cut {
                seen.push(l as u32);
            }
            prev = l;
        }
        if !out.is_empty() && out.len() + block.len() > MAX_OUTPUT_BYTES {
            bytes_limited = true;
            break;
        }
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&block);
        ctx.store.record(&r.path, &r.text, Seen::lines(seen));
        shown_files += 1;
    }
    let mut notices = Vec::new();
    if hit_limit {
        notices.push(format!(
            "[Match limit of {limit} reached; results may be incomplete. Narrow `pattern`/`path`/`glob` or raise `limit`.]"
        ));
    }
    if bytes_limited {
        notices.push(format!(
            "[Output limit reached: showing {shown_files} of {} files with matches. Narrow the search.]",
            results.len()
        ));
    }
    if !notices.is_empty() {
        out.push_str("\n\n");
        out.push_str(&notices.join("\n"));
    }
    let mut o = ToolOutput::ok(out);
    o.meta = Some(
        json!({"matches": total_matches, "files": results.len(), "truncated": hit_limit || bytes_limited}),
    );
    Ok(o)
}

/// Keeps only the first `keep` matches (and context up to the last kept match + its context).
fn truncate_rows(r: &mut FileResult, keep: usize) {
    let mut count = 0;
    let mut cut_at = r.rows.len();
    for (i, &(_, m)) in r.rows.iter().enumerate() {
        if m {
            if count == keep {
                cut_at = i;
                break;
            }
            count += 1;
        }
    }
    r.rows.truncate(cut_at);
    // Drop trailing context that belonged to the dropped match (keep context after last match).
    r.matches = count;
}

pub(crate) fn collect_files(
    root: &Path,
    glob: Option<&str>,
    ctx: &ToolCtx,
) -> Result<Vec<PathBuf>> {
    let mut wb = WalkBuilder::new(root);
    wb.hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .ignore(true)
        .parents(true)
        .require_git(false)
        .follow_links(false)
        .filter_entry(|e| e.file_name() != ".git");
    let matcher = glob.map(crate::find::PathMatcher::new).transpose()?;
    let mut files = Vec::new();
    for entry in wb.build() {
        if ctx.cancel.is_cancelled() {
            bail!("grep cancelled");
        }
        let Ok(entry) = entry else { continue };
        if entry.file_type().is_some_and(|t| t.is_file()) {
            if let Some(m) = &matcher {
                let rel = entry.path().strip_prefix(root).unwrap_or(entry.path());
                if !m.is_match(rel) {
                    continue;
                }
            }
            files.push(entry.into_path());
        }
    }
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_support::{ctx, temp_dir};

    fn setup() -> (crate::util::test_support::TempDir, ToolCtx) {
        let d = temp_dir("grep");
        std::fs::create_dir_all(d.join("src/nested")).unwrap();
        std::fs::write(
            d.join("src/a.rs"),
            "fn main() {\n    let x = 1;\n    foo(x);\n}\n",
        )
        .unwrap();
        std::fs::write(d.join("src/nested/b.rs"), "// foo here\nbar\nFOO\n").unwrap();
        std::fs::write(d.join("notes.md"), "foo in markdown\n").unwrap();
        std::fs::write(d.join(".hidden.rs"), "foo hidden\n").unwrap();
        std::fs::write(d.join(".gitignore"), "ignored/\n").unwrap();
        std::fs::create_dir_all(d.join("ignored")).unwrap();
        std::fs::write(d.join("ignored/x.rs"), "foo ignored\n").unwrap();
        std::fs::write(d.join("bin.dat"), b"foo\0bin").unwrap();
        let c = ctx(&d);
        (d, c)
    }

    #[tokio::test]
    async fn grouped_output_and_snapshot() {
        let (d, c) = setup();
        let o = crate::execute("grep", json!({"pattern": "foo", "glob": "*.rs"}), &c).await;
        assert!(!o.is_error, "{}", o.text);
        let a = std::fs::read_to_string(d.join("src/a.rs")).unwrap();
        let b = std::fs::read_to_string(d.join("src/nested/b.rs")).unwrap();
        let h = std::fs::read_to_string(d.join(".hidden.rs")).unwrap();
        let expected = format!(
            "[.hidden.rs#{}]\n*1:foo hidden\n\n[src/a.rs#{}]\n*3:    foo(x);\n\n[src/nested/b.rs#{}]\n*1:// foo here",
            file_hash(&h),
            file_hash(&a),
            file_hash(&b)
        );
        assert_eq!(o.text, expected);
        let snap = c.store.get(&d.join("src/a.rs"), &file_hash(&a)).unwrap();
        assert!(snap.seen.contains(3) && !snap.seen.contains(2));
    }

    #[tokio::test]
    async fn context_case_literal_limit() {
        let (_d, c) = setup();
        let o = crate::execute(
            "grep",
            json!({"pattern": "foo", "path": "src/nested/b.rs", "ignore_case": true, "context": 1}),
            &c,
        )
        .await;
        assert!(
            o.text.ends_with("*1:// foo here\n 2:bar\n*3:FOO"),
            "{}",
            o.text
        );
        let o = crate::execute("grep", json!({"pattern": "foo(", "literal": true}), &c).await;
        assert!(o.text.contains("*3:    foo(x);") && !o.text.contains("hidden"));
        let o = crate::execute("grep", json!({"pattern": "foo(", "literal": false}), &c).await;
        assert!(o.is_error && o.text.contains("Invalid regex"));
        let o = crate::execute("grep", json!({"pattern": "foo", "limit": 2}), &c).await;
        assert!(o.text.contains("Match limit of 2 reached"));
        assert_eq!(o.text.matches("\n*").count(), 2);
        let o = crate::execute("grep", json!({"pattern": "zzzz"}), &c).await;
        assert_eq!(o.text, "No matches found.");
    }

    #[tokio::test]
    async fn skips_ignored_and_binary() {
        let (_d, c) = setup();
        let o = crate::execute("grep", json!({"pattern": "foo"}), &c).await;
        assert!(!o.text.contains("ignored/x.rs"));
        assert!(!o.text.contains("bin.dat"));
        assert!(o.text.contains("notes.md"));
    }

    #[tokio::test]
    async fn context_gaps() {
        let d = temp_dir("grep-gaps");
        let text: String = (1..=20)
            .map(|i| {
                if i == 3 || i == 15 {
                    "hit\n".to_string()
                } else {
                    format!("l{i}\n")
                }
            })
            .collect();
        std::fs::write(d.join("g.txt"), &text).unwrap();
        let c = ctx(&d);
        let o = crate::execute("grep", json!({"pattern": "hit", "context": 1}), &c).await;
        assert!(
            o.text
                .ends_with(" 2:l2\n*3:hit\n 4:l4\n...\n 14:l14\n*15:hit\n 16:l16"),
            "{}",
            o.text
        );
    }
}
