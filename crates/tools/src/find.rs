//! `find`: glob file search.

use crate::util;
use crate::{ToolCtx, ToolOutput, ToolSpec};
use anyhow::{Result, anyhow, bail};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;
use serde_json::{Value, json};
use std::path::Path;
use std::time::SystemTime;

pub const DEFAULT_LIMIT: usize = 500;
pub const MAX_LIMIT: usize = 5000;

pub fn spec() -> ToolSpec {
    ToolSpec {
        name: "find",
        description: "Find files and directories by glob pattern. Respects .gitignore, includes hidden files, skips .git.\n\
A pattern without `/` matches the file name at any depth (`*.rs`, `Cargo.toml`); a pattern with `/` matches the path relative to `path` \
(`src/**/*.ts`, `**/test_*.py`). `{a,b}` alternatives are supported. Default pattern `**/*` lists everything.\n\
Results are paths relative to the working directory, newest modification first; directories end with `/`."
            .to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "Glob pattern (default `**/*`)."},
                "path": {"type": "string", "description": "Directory to search (default: working directory)."},
                "type": {"type": "string", "enum": ["file", "dir", "any"], "description": "Entry type to return (default `any`)."},
                "limit": {"type": "integer", "minimum": 1, "description": "Maximum results (default 500)."}
            }
        }),
    }
}

pub async fn run(input: Value, ctx: &ToolCtx) -> Result<ToolOutput> {
    let ctx = ctx.clone();
    util::blocking(move || run_sync(&input, &ctx)).await
}

/// Glob matcher with "basename at any depth" semantics for patterns without `/`.
pub(crate) struct PathMatcher {
    path_set: GlobSet,
    name_set: GlobSet,
    has_path: bool,
    has_name: bool,
}

impl PathMatcher {
    /// `spec` may contain several patterns separated by `,` or `;` outside braces.
    pub(crate) fn new(spec: &str) -> Result<PathMatcher> {
        let mut path_b = GlobSetBuilder::new();
        let mut name_b = GlobSetBuilder::new();
        let (mut has_path, mut has_name) = (false, false);
        for pat in split_patterns(spec) {
            let pat = pat.strip_prefix("./").unwrap_or(&pat).to_string();
            let pat = pat.trim_end_matches('/');
            if pat.is_empty() {
                continue;
            }
            let glob = GlobBuilder::new(pat)
                .literal_separator(true)
                .backslash_escape(true)
                .build()
                .map_err(|e| anyhow!("invalid glob {pat:?}: {e}"))?;
            if pat.contains('/') {
                path_b.add(glob);
                has_path = true;
            } else {
                name_b.add(glob);
                has_name = true;
            }
        }
        if !has_path && !has_name {
            bail!("glob pattern must not be empty");
        }
        Ok(PathMatcher {
            path_set: path_b.build().map_err(|e| anyhow!("invalid glob: {e}"))?,
            name_set: name_b.build().map_err(|e| anyhow!("invalid glob: {e}"))?,
            has_path,
            has_name,
        })
    }

    pub(crate) fn is_match(&self, rel: &Path) -> bool {
        (self.has_path && self.path_set.is_match(rel))
            || (self.has_name && rel.file_name().is_some_and(|n| self.name_set.is_match(n)))
    }
}

fn split_patterns(spec: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    for c in spec.chars() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' | ';' if depth <= 0 => {
                out.push(std::mem::take(&mut cur).trim().to_string());
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    out.push(cur.trim().to_string());
    out.retain(|p| !p.is_empty());
    out
}

fn run_sync(input: &Value, ctx: &ToolCtx) -> Result<ToolOutput> {
    let pattern = util::opt_str(input, "pattern")?.filter(|p| !p.trim().is_empty()).unwrap_or_else(|| "**/*".into());
    let kind = util::opt_str(input, "type")?.unwrap_or_else(|| "any".into()).to_ascii_lowercase();
    let (want_files, want_dirs) = match kind.as_str() {
        "file" | "f" | "files" => (true, false),
        "dir" | "d" | "directory" | "dirs" => (false, true),
        "any" | "all" | "" => (true, true),
        other => bail!("invalid `type` {other:?} (expected file, dir or any)"),
    };
    let limit = (util::opt_u64(input, "limit")?.unwrap_or(DEFAULT_LIMIT as u64) as usize).clamp(1, MAX_LIMIT);
    let root = match util::opt_str(input, "path")? {
        Some(p) if !p.trim().is_empty() => util::resolve_path(&ctx.cwd, &p),
        _ => ctx.cwd.clone(),
    };
    let root_display = util::display_path(&ctx.cwd, &root);
    if !root.is_dir() {
        return Ok(ToolOutput::err(format!("Not a directory: {root_display}")));
    }
    if root.parent().is_none() {
        return Ok(ToolOutput::err("Refusing to search from `/`; pass a narrower `path`."));
    }
    let matcher = PathMatcher::new(&pattern)?;

    let mut wb = WalkBuilder::new(&root);
    wb.hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .ignore(true)
        .parents(true)
        .require_git(false)
        .follow_links(false)
        .filter_entry(|e| e.file_name() != ".git");
    let mut found: Vec<(SystemTime, String)> = Vec::new();
    for entry in wb.build() {
        if ctx.cancel.is_cancelled() {
            bail!("find cancelled");
        }
        let Ok(entry) = entry else { continue };
        if entry.depth() == 0 {
            continue;
        }
        let Some(ft) = entry.file_type() else { continue };
        let is_dir = ft.is_dir();
        if (is_dir && !want_dirs) || (!is_dir && !want_files) {
            continue;
        }
        let rel = entry.path().strip_prefix(&root).unwrap_or(entry.path());
        if !matcher.is_match(rel) {
            continue;
        }
        let mtime = entry.metadata().ok().and_then(|m| m.modified().ok()).unwrap_or(SystemTime::UNIX_EPOCH);
        let mut shown = util::display_path(&ctx.cwd, entry.path());
        if is_dir {
            shown.push('/');
        }
        found.push((mtime, shown));
    }
    let total = found.len();
    if total == 0 {
        let mut o = ToolOutput::ok(format!("No files found matching `{pattern}` in {root_display}"));
        o.meta = Some(json!({"count": 0}));
        return Ok(o);
    }
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let mut out: String = found.iter().take(limit).map(|(_, p)| p.as_str()).collect::<Vec<_>>().join("\n");
    if total > limit {
        out.push_str(&format!(
            "\n\n[Showing {limit} of {total} results. Narrow the pattern or use limit={} for more]",
            (limit * 2).min(MAX_LIMIT)
        ));
    }
    let mut o = ToolOutput::ok(out);
    o.meta = Some(json!({"count": total, "truncated": total > limit}));
    Ok(o)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_support::{ctx, temp_dir};

    #[tokio::test]
    async fn find_patterns() {
        let d = temp_dir("find");
        std::fs::create_dir_all(d.join("src/nested")).unwrap();
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::create_dir_all(d.join("target")).unwrap();
        std::fs::write(d.join(".gitignore"), "target/\n").unwrap();
        std::fs::write(d.join("target/out.rs"), "").unwrap();
        std::fs::write(d.join(".git/config"), "").unwrap();
        std::fs::write(d.join("src/old.rs"), "").unwrap();
        let old = SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::options().write(true).open(d.join("src/old.rs")).unwrap().set_modified(old).unwrap();
        std::fs::write(d.join("src/nested/new.rs"), "").unwrap();
        std::fs::write(d.join("README.md"), "").unwrap();
        let c = ctx(&d);

        let o = crate::execute("find", json!({"pattern": "*.rs"}), &c).await;
        assert_eq!(o.text, "src/nested/new.rs\nsrc/old.rs");
        let o = crate::execute("find", json!({"pattern": "src/*.rs"}), &c).await;
        assert_eq!(o.text, "src/old.rs");
        let o = crate::execute("find", json!({"pattern": "**/*.{md,rs}", "type": "file"}), &c).await;
        assert!(o.text.contains("README.md") && o.text.contains("src/old.rs"));
        let o = crate::execute("find", json!({"type": "dir"}), &c).await;
        let mut lines: Vec<&str> = o.text.lines().collect();
        lines.sort();
        assert_eq!(lines, vec!["src/", "src/nested/"]);
        let o = crate::execute("find", json!({}), &c).await;
        assert!(o.text.contains(".gitignore") && !o.text.contains(".git/") && !o.text.contains("target"));
        let o = crate::execute("find", json!({"limit": 2}), &c).await;
        assert!(o.text.contains("[Showing 2 of"));
        let o = crate::execute("find", json!({"pattern": "*.py"}), &c).await;
        assert!(o.text.starts_with("No files found"));
        let o = crate::execute("find", json!({"pattern": "*.rs", "path": "src/nested"}), &c).await;
        assert_eq!(o.text, "src/nested/new.rs");
    }
}
