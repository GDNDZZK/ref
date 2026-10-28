use std::path::PathBuf;
use std::time::SystemTime;

use crate::util::{self, display_path, resolve_path, Args};
use crate::*;

pub struct FindTool;

const DEFAULT_LIMIT: usize = 200;
const MAX_LIMIT: usize = 5000;

const DESCRIPTION: &str = "List files and directories matching a glob, newest first (gitignore respected; hidden skipped unless `hidden: true`). A bare glob like `*.rs` matches at any depth; `src/*.rs` only direct children of src (`src/**/*.rs` recurses). Directories end with `/`. No pattern = everything under `path`.";

fn build_glob(pattern: &str) -> anyhow::Result<globset::GlobMatcher> {
    let p = pattern.trim().trim_start_matches("./");
    let p = if p.contains('/') { p.to_string() } else { format!("**/{p}") };
    Ok(globset::GlobBuilder::new(&p).literal_separator(true).build()?.compile_matcher())
}

#[async_trait]
impl Tool for FindTool {
    fn name(&self) -> &str {
        "find"
    }

    fn description(&self) -> String {
        DESCRIPTION.to_string()
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Glob, e.g. `*.rs`, `src/**/*.ts`, `**/test_*`." },
                "path": { "type": "string", "description": "Directory to search (default: cwd)." },
                "hidden": { "type": "boolean", "description": "Include hidden files/dirs (default false)." },
                "limit": { "type": "integer", "minimum": 1, "description": "Max results (default 200)." }
            },
            "additionalProperties": false
        })
    }

    fn permission_subject(&self, input: &Value) -> String {
        let p = Args::lenient(input).s("path");
        if p.is_empty() { ".".into() } else { util::expand_tilde(p).to_string_lossy().to_string() }
    }

    fn title(&self, input: &Value) -> String {
        let a = Args::lenient(input);
        let pat = a.s("pattern");
        let p = a.s("path");
        let p = if p.is_empty() { "." } else { p };
        if pat.is_empty() { p.to_string() } else { format!("{pat} in {p}") }
    }

    async fn run(&self, input: Value, ctx: &ToolCtx) -> anyhow::Result<ToolOutput> {
        let args = Args::new(&input)?;
        let pattern = args.str_opt("pattern")?.map(str::to_string);
        let path = args.str_opt("path")?.unwrap_or(".");
        let hidden = args.bool_opt("hidden")?.unwrap_or(false);
        let limit = args.u64_opt("limit")?.map(|l| (l as usize).clamp(1, MAX_LIMIT)).unwrap_or(DEFAULT_LIMIT);
        let root = resolve_path(&ctx.cwd, path);
        let title = match &pattern {
            Some(p) => format!("{p} in {}", display_path(&ctx.cwd, &root)),
            None => display_path(&ctx.cwd, &root),
        };
        if !root.is_dir() {
            return Ok(ToolOutput::error(format!("Not a directory: {path}")).with_title(title));
        }
        let matcher = match pattern.as_deref().map(build_glob).transpose() {
            Ok(m) => m,
            Err(e) => return Ok(ToolOutput::error(format!("Invalid glob: {e}")).with_title(title)),
        };
        let cwd = ctx.cwd.clone();
        let cancel = ctx.cancel.clone();
        let task = tokio::task::spawn_blocking(move || {
            let mut walker = ignore::WalkBuilder::new(&root);
            walker.hidden(!hidden).git_ignore(true).git_global(true).git_exclude(true).require_git(false);
            if hidden {
                // Never list VCS internals.
                walker.filter_entry(|e| e.file_name() != ".git");
            }
            let mut found: Vec<(SystemTime, PathBuf, bool)> = Vec::new();
            for entry in walker.build() {
                if cancel.is_cancelled() {
                    return None;
                }
                let Ok(entry) = entry else { continue };
                if entry.depth() == 0 {
                    continue;
                }
                let rel = entry.path().strip_prefix(&root).unwrap_or(entry.path());
                if let Some(m) = &matcher {
                    if !m.is_match(rel) {
                        continue;
                    }
                }
                let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
                let mtime = entry.metadata().ok().and_then(|m| m.modified().ok()).unwrap_or(SystemTime::UNIX_EPOCH);
                found.push((mtime, entry.path().to_path_buf(), is_dir));
            }
            found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            let total = found.len();
            let mut lines: Vec<String> = found
                .iter()
                .take(limit)
                .map(|(_, p, d)| {
                    let s = display_path(&cwd, p);
                    if *d { format!("{s}/") } else { s }
                })
                .collect();
            if total == 0 {
                return Some("No files found.".to_string());
            }
            if total > limit {
                lines.push(format!("… {} more (raise `limit` or narrow the pattern)", total - limit));
            }
            Some(lines.join("\n"))
        });
        let out = tokio::select! {
            r = task => match r? {
                Some(t) => ToolOutput::text(t),
                None => ToolOutput::error("Find cancelled."),
            },
            _ = ctx.cancel.cancelled() => ToolOutput::error("Find cancelled."),
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
    async fn find_globs() {
        let (dir, c) = ctx();
        std::fs::create_dir_all(dir.path().join("src/deep")).unwrap();
        std::fs::write(dir.path().join("src/a.rs"), "").unwrap();
        std::fs::write(dir.path().join("src/deep/b.rs"), "").unwrap();
        std::fs::write(dir.path().join("README.md"), "").unwrap();
        std::fs::write(dir.path().join(".secret.rs"), "").unwrap();
        let out = run_tool(&FindTool, json!({"pattern": "*.rs"}), &c).await;
        let mut lines: Vec<&str> = out.text.lines().collect();
        lines.sort();
        assert_eq!(lines, vec!["src/a.rs", "src/deep/b.rs"]);
        let out = run_tool(&FindTool, json!({"pattern": "src/*.rs"}), &c).await;
        assert_eq!(out.text, "src/a.rs");
        let out = run_tool(&FindTool, json!({"pattern": "*.rs", "hidden": true}), &c).await;
        assert!(out.text.contains(".secret.rs"));
        let out = run_tool(&FindTool, json!({}), &c).await;
        assert!(out.text.contains("src/\n") || out.text.contains("src/deep/"), "{}", out.text);
        let out = run_tool(&FindTool, json!({"limit": 2}), &c).await;
        assert!(out.text.contains("… 3 more"), "{}", out.text);
        let out = run_tool(&FindTool, json!({"pattern": "*.zzz"}), &c).await;
        assert_eq!(out.text, "No files found.");
    }

    #[tokio::test]
    async fn newest_first() {
        let (dir, c) = ctx();
        std::fs::write(dir.path().join("old.txt"), "").unwrap();
        let f = std::fs::File::options().write(true).open(dir.path().join("old.txt")).unwrap();
        f.set_modified(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1000)).unwrap();
        std::fs::write(dir.path().join("new.txt"), "").unwrap();
        let out = run_tool(&FindTool, json!({"pattern": "*.txt"}), &c).await;
        assert_eq!(out.text, "new.txt\nold.txt");
    }
}
