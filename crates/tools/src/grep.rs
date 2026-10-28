use std::path::{Path, PathBuf};

use grep_regex::RegexMatcherBuilder;
use grep_searcher::{Searcher, SearcherBuilder, Sink, SinkContext, SinkMatch};

use crate::hashline::{header, Origin, Seen, TextFile, MAX_SNAPSHOT_BYTES};
use crate::util::{self, canonical_key, display_path, resolve_path, Args};
use crate::*;

pub struct GrepTool;

const FILES_PER_PAGE: usize = 20;
const MATCHES_MULTI: usize = 20;
const MATCHES_SINGLE: usize = 200;
const MAX_OUTPUT_BYTES: usize = 50 * 1024;
const MAX_LINE: usize = 512;
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

const DESCRIPTION: &str = "Search file contents with a Rust regex (gitignore respected, hidden files skipped). Output is hashline: a file header `[PATH#TAG]` (one file) or `## PATH#TAG` (per file), then `*N:text` for matches and ` N:text` for context. The header and numbers can be used directly in `edit` (`[PATH#TAG]`). `glob` filters files (`*.rs` matches at any depth). Pages of 20 files: pass `skip` for the next page.";

#[derive(Clone)]
struct Opts {
    before: usize,
    after: usize,
    max_matches: usize,
}

struct Row {
    line: u64,
    text: String,
    is_match: bool,
}

struct Collect {
    rows: Vec<Row>,
    matches: usize,
    max: usize,
    more: bool,
    cancel: tokio_util::sync::CancellationToken,
}

fn line_text(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    let s = s.trim_end_matches(['\n', '\r']);
    s.strip_prefix('\u{feff}').unwrap_or(s).to_string()
}

impl Sink for Collect {
    type Error = std::io::Error;

    fn matched(&mut self, _s: &Searcher, m: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        if self.cancel.is_cancelled() {
            return Ok(false);
        }
        let start = m.line_number().unwrap_or(0);
        for (i, l) in m.lines().enumerate() {
            if self.matches >= self.max {
                self.more = true;
                return Ok(false);
            }
            self.matches += 1;
            self.rows.push(Row { line: start + i as u64, text: line_text(l), is_match: true });
        }
        Ok(true)
    }

    fn context(&mut self, _s: &Searcher, c: &SinkContext<'_>) -> Result<bool, Self::Error> {
        if let Some(n) = c.line_number() {
            self.rows.push(Row { line: n, text: line_text(c.bytes()), is_match: false });
        }
        Ok(true)
    }
}

struct FileHit {
    abs: PathBuf,
    tag: Option<String>,
    text: Option<String>,
    rows: Vec<Row>,
    more: bool,
}

fn search_file(
    path: &Path,
    matcher: &grep_regex::RegexMatcher,
    opts: &Opts,
    cancel: &tokio_util::sync::CancellationToken,
) -> Option<FileHit> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > MAX_FILE_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if util::looks_binary(&bytes) {
        return None;
    }
    let mut searcher = SearcherBuilder::new()
        .line_number(true)
        .before_context(opts.before)
        .after_context(opts.after)
        .build();
    let mut sink = Collect { rows: Vec::new(), matches: 0, max: opts.max_matches, more: false, cancel: cancel.clone() };
    searcher.search_slice(matcher, &bytes, &mut sink).ok()?;
    if sink.matches == 0 {
        return None;
    }
    let file = (meta.len() <= MAX_SNAPSHOT_BYTES).then(|| TextFile::decode(&bytes)).flatten();
    Some(FileHit {
        abs: path.to_path_buf(),
        tag: file.as_ref().map(|f| f.tag()),
        text: file.map(|f| f.text),
        rows: sink.rows,
        more: sink.more,
    })
}

/// Render rows; returns (lines, fully shown line numbers).
fn render_rows(rows: &[Row]) -> (Vec<String>, Vec<u32>) {
    let mut out = Vec::new();
    let mut shown = Vec::new();
    let mut prev: Option<u64> = None;
    for r in rows {
        if prev.is_some_and(|p| r.line > p + 1) {
            out.push("...".to_string());
        }
        if prev.is_some_and(|p| r.line <= p) {
            continue;
        }
        prev = Some(r.line);
        let (t, cut) = util::truncate_chars(&r.text, MAX_LINE);
        if !cut {
            shown.push(r.line as u32);
        }
        out.push(format!("{}{}:{t}", if r.is_match { '*' } else { ' ' }, r.line));
    }
    (out, shown)
}

fn build_glob(glob: &str) -> anyhow::Result<globset::GlobMatcher> {
    let g = if glob.contains('/') { glob.trim_start_matches("./").to_string() } else { format!("**/{glob}") };
    Ok(globset::GlobBuilder::new(&g).literal_separator(true).build()?.compile_matcher())
}

struct Search {
    root: PathBuf,
    cwd: PathBuf,
    pattern: String,
    glob: Option<String>,
    ignore_case: bool,
    opts: Opts,
    skip: usize,
}

fn run_search(s: Search, store: &EditStore, cancel: tokio_util::sync::CancellationToken) -> anyhow::Result<ToolOutput> {
    let matcher = match RegexMatcherBuilder::new().case_insensitive(s.ignore_case).build(&s.pattern) {
        Ok(m) => m,
        Err(e) => return Ok(ToolOutput::error(format!("Invalid regex {:?}: {e}", s.pattern))),
    };
    let single = s.root.is_file();
    let mut opts = s.opts.clone();
    if single {
        opts.max_matches = opts.max_matches.max(1);
        let Some(hit) = search_file(&s.root, &matcher, &opts, &cancel) else {
            return Ok(ToolOutput::text("No matches found."));
        };
        let display = display_path(&s.cwd, &hit.abs);
        let (rows, shown) = render_rows(&hit.rows);
        let head = match &hit.tag {
            Some(t) => header(&display, t),
            None => format!("[{display}]"),
        };
        let mut text = format!("{head}\n{}", rows.join("\n"));
        let mut last_shown = u32::MAX;
        if text.len() > MAX_OUTPUT_BYTES {
            let cut = util::floor_char_boundary(&text, MAX_OUTPUT_BYTES);
            let cut = text[..cut].rfind('\n').unwrap_or(cut);
            text.truncate(cut);
            // Last row that survived the cut.
            last_shown = text
                .lines()
                .last()
                .and_then(|l| l.get(1..)?.split(':').next()?.parse().ok())
                .unwrap_or(0);
            text.push_str("\n\n[output truncated at 50KB; narrow the pattern]");
        }
        if hit.more {
            text.push_str(&format!("\n\n[more matches in this file beyond {}; narrow the pattern or read the file]", opts.max_matches));
        }
        if let (Some(_), Some(body)) = (&hit.tag, &hit.text) {
            let key = canonical_key(&hit.abs);
            let shown = shown.into_iter().filter(|l| *l <= last_shown).chain([0]);
            store.record(&key, body, Seen::lines(shown), Origin::Display);
        }
        return Ok(ToolOutput::text(text));
    }

    let glob = s.glob.as_deref().map(build_glob).transpose()?;
    let mut walker = ignore::WalkBuilder::new(&s.root);
    walker.hidden(true).git_ignore(true).git_global(true).git_exclude(true).require_git(false).sort_by_file_name(|a, b| a.cmp(b));
    let mut hits: Vec<FileHit> = Vec::new();
    let mut matched_files = 0usize;
    let mut more_files = false;
    for entry in walker.build() {
        if cancel.is_cancelled() {
            break;
        }
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.path();
        if let Some(g) = &glob {
            let rel = path.strip_prefix(&s.root).unwrap_or(path);
            if !g.is_match(rel) {
                continue;
            }
        }
        let Some(hit) = search_file(path, &matcher, &opts, &cancel) else { continue };
        matched_files += 1;
        if matched_files <= s.skip {
            continue;
        }
        if hits.len() == FILES_PER_PAGE {
            more_files = true;
            break;
        }
        hits.push(hit);
    }
    if cancel.is_cancelled() {
        return Ok(ToolOutput::error("Search cancelled."));
    }
    if hits.is_empty() {
        return Ok(ToolOutput::text(if s.skip > 0 && matched_files > 0 {
            format!("No more matches (only {matched_files} file(s) matched; skip={} is past the end).", s.skip)
        } else {
            "No matches found.".to_string()
        }));
    }
    let mut blocks = Vec::new();
    let mut total = 0usize;
    let mut truncated = false;
    for hit in &hits {
        let display = display_path(&s.cwd, &hit.abs);
        let (rows, shown) = render_rows(&hit.rows);
        let mut block = match &hit.tag {
            Some(t) => format!("## {display}#{t}\n"),
            None => format!("## {display}\n"),
        };
        block.push_str(&rows.join("\n"));
        if hit.more {
            block.push_str(&format!("\n[more matches in this file beyond {}; grep it alone for up to {MATCHES_SINGLE}]", opts.max_matches));
        }
        if total + block.len() > MAX_OUTPUT_BYTES && !blocks.is_empty() {
            truncated = true;
            break;
        }
        total += block.len() + 2;
        if let (Some(_), Some(body)) = (&hit.tag, &hit.text) {
            let key = canonical_key(&hit.abs);
            store.record(&key, body, Seen::lines(shown.into_iter().chain([0])), Origin::Display);
        }
        blocks.push(block);
    }
    let shown_files = blocks.len();
    let mut text = blocks.join("\n\n");
    if truncated || more_files {
        let next = s.skip + shown_files;
        text.push_str(&format!("\n\n[more files matched; use skip={next} to see the next page]"));
    }
    Ok(ToolOutput::text(text))
}

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> String {
        DESCRIPTION.to_string()
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Rust regex." },
                "path": { "type": "string", "description": "File or directory to search (default: cwd)." },
                "glob": { "type": "string", "description": "Filter files, e.g. `*.rs` (any depth) or `src/**/*.ts`." },
                "ignore_case": { "type": "boolean", "description": "Case-insensitive match (default false)." },
                "context": { "type": "integer", "minimum": 0, "description": "Context lines before/after each match (default: 1 before, 3 after)." },
                "limit": { "type": "integer", "minimum": 1, "description": "Max matches per file (default 20; 200 when path is a single file)." },
                "skip": { "type": "integer", "minimum": 0, "description": "Matching files to skip (pagination, 20 files per page)." }
            },
            "required": ["pattern"],
            "additionalProperties": false
        })
    }

    fn permission_subject(&self, input: &Value) -> String {
        let p = Args::lenient(input).s("path");
        if p.is_empty() { ".".into() } else { util::expand_tilde(p).to_string_lossy().to_string() }
    }

    fn title(&self, input: &Value) -> String {
        let a = Args::lenient(input);
        let p = a.s("path");
        format!("\"{}\" in {}", a.s("pattern"), if p.is_empty() { "." } else { p })
    }

    async fn run(&self, input: Value, ctx: &ToolCtx) -> anyhow::Result<ToolOutput> {
        let args = Args::new(&input)?;
        let pattern = args.str_req("pattern")?.to_string();
        let path = args.str_opt("path")?.unwrap_or(".");
        let root = resolve_path(&ctx.cwd, path);
        let title = format!("\"{pattern}\" in {}", display_path(&ctx.cwd, &root));
        if !root.exists() {
            return Ok(ToolOutput::error(format!("Path not found: {path}")).with_title(title));
        }
        let single = root.is_file();
        let context = args.u64_opt("context")?.map(|c| c.min(20) as usize);
        let limit = args.u64_opt("limit")?.map(|l| l.clamp(1, 1000) as usize);
        let search = Search {
            cwd: ctx.cwd.clone(),
            pattern,
            glob: args.str_opt("glob")?.map(str::to_string),
            ignore_case: args.bool_opt("ignore_case")?.unwrap_or(false),
            opts: Opts {
                before: context.unwrap_or(1),
                after: context.unwrap_or(3),
                max_matches: limit.unwrap_or(if single { MATCHES_SINGLE } else { MATCHES_MULTI }),
            },
            skip: args.u64_opt("skip")?.unwrap_or(0) as usize,
            root,
        };
        let store = ctx.store.clone();
        let cancel = ctx.cancel.clone();
        let task = tokio::task::spawn_blocking(move || run_search(search, &store, cancel));
        let out = tokio::select! {
            r = task => r??,
            _ = ctx.cancel.cancelled() => ToolOutput::error("Search cancelled."),
        };
        Ok(out.with_title(title))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ctx, run_tool};
    use serde_json::json;

    #[tokio::test]
    async fn single_file_output() {
        let (dir, c) = ctx();
        let body = "a\nb\nfoo one\nc\nd\ne\nf\ng\nfoo two\n";
        std::fs::write(dir.path().join("x.txt"), body).unwrap();
        let out = run_tool(&GrepTool, json!({"pattern": "foo", "path": "x.txt", "context": 1}), &c).await;
        let tag = crate::hashline::file_tag(body);
        assert_eq!(out.text, format!("[x.txt#{tag}]\n 2:b\n*3:foo one\n 4:c\n...\n 8:g\n*9:foo two"));
        assert_eq!(out.title, "\"foo\" in x.txt");
        // Rows recorded as seen: editing line 3 works, line 6 is guarded.
        let e = run_tool(&crate::edit::EditTool, json!({"input": format!("[x.txt#{tag}]\nPUT 3.=3:\n+FOO\n")}), &c).await;
        assert!(!e.is_error, "{}", e.text);
    }

    #[tokio::test]
    async fn multi_file_grouping_glob_and_paging() {
        let (dir, c) = ctx();
        std::fs::create_dir_all(dir.path().join("src/deep")).unwrap();
        std::fs::write(dir.path().join("src/a.rs"), "fn needle() {}\n").unwrap();
        std::fs::write(dir.path().join("src/deep/b.rs"), "x\nNEEDLE\n").unwrap();
        std::fs::write(dir.path().join("c.md"), "needle\n").unwrap();
        std::fs::write(dir.path().join(".hidden.rs"), "needle\n").unwrap();
        std::fs::write(dir.path().join(".gitignore"), "ignored.rs\n").unwrap();
        std::fs::write(dir.path().join("ignored.rs"), "needle\n").unwrap();
        let out = run_tool(&GrepTool, json!({"pattern": "needle", "glob": "*.rs", "ignore_case": true, "context": 0}), &c).await;
        let ta = crate::hashline::file_tag("fn needle() {}\n");
        let tb = crate::hashline::file_tag("x\nNEEDLE\n");
        assert_eq!(out.text, format!("## src/a.rs#{ta}\n*1:fn needle() {{}}\n\n## src/deep/b.rs#{tb}\n*2:NEEDLE"));
        let out = run_tool(&GrepTool, json!({"pattern": "needle", "context": 0}), &c).await;
        assert!(out.text.contains("## c.md#") && !out.text.contains("hidden") && !out.text.contains("ignored"), "{}", out.text);
        for i in 0..25 {
            std::fs::write(dir.path().join(format!("p{i:02}.txt")), "zzz\n").unwrap();
        }
        let out = run_tool(&GrepTool, json!({"pattern": "zzz"}), &c).await;
        assert!(out.text.contains("use skip=20"), "{}", out.text);
        assert_eq!(out.text.matches("## ").count(), 20);
        let out = run_tool(&GrepTool, json!({"pattern": "zzz", "skip": 20}), &c).await;
        assert_eq!(out.text.matches("## ").count(), 5);
        let out = run_tool(&GrepTool, json!({"pattern": "("}), &c).await;
        assert!(out.is_error && out.text.contains("Invalid regex"));
        let out = run_tool(&GrepTool, json!({"pattern": "nomatchhere"}), &c).await;
        assert_eq!(out.text, "No matches found.");
    }

    #[tokio::test]
    async fn per_file_match_limit() {
        let (dir, c) = ctx();
        let body: String = (0..30).map(|i| format!("hit {i}\n")).collect();
        std::fs::write(dir.path().join("m.txt"), &body).unwrap();
        let out = run_tool(&GrepTool, json!({"pattern": "hit", "path": "m.txt", "limit": 5, "context": 0}), &c).await;
        assert_eq!(out.text.matches("\n*").count(), 5);
        assert!(out.text.contains("more matches in this file"));
    }
}
