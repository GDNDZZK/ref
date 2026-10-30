//! Shared helpers: path resolution, argument parsing, text normalization, diffs.

use anyhow::{Result, anyhow, bail};
use regex::Regex;
use serde_json::Value;
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------------

/// Required string argument.
pub fn req_str(input: &Value, key: &str) -> Result<String> {
    opt_str(input, key)?.ok_or_else(|| anyhow!("missing required parameter `{key}`"))
}

/// Optional string argument (`null` = absent). Numbers/bools are stringified leniently.
pub fn opt_str(input: &Value, key: &str) -> Result<Option<String>> {
    match input.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(Value::Number(n)) => Ok(Some(n.to_string())),
        Some(Value::Bool(b)) => Ok(Some(b.to_string())),
        Some(_) => bail!("parameter `{key}` must be a string"),
    }
}

/// Optional non-negative integer argument; accepts numeric strings and integral floats.
pub fn opt_u64(input: &Value, key: &str) -> Result<Option<u64>> {
    value_u64(input.get(key), key)
}

pub fn value_u64(v: Option<&Value>, key: &str) -> Result<Option<u64>> {
    let bad = || anyhow!("parameter `{key}` must be a non-negative integer");
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => {
            if let Some(u) = n.as_u64() {
                Ok(Some(u))
            } else if let Some(f) = n.as_f64() {
                if f >= 0.0 && f.fract() == 0.0 && f <= u64::MAX as f64 {
                    Ok(Some(f as u64))
                } else {
                    Err(bad())
                }
            } else {
                Err(bad())
            }
        }
        Some(Value::String(s)) if s.trim().is_empty() => Ok(None),
        Some(Value::String(s)) => s.trim().parse::<u64>().map(Some).map_err(|_| bad()),
        Some(_) => Err(bad()),
    }
}

/// Optional boolean argument; accepts "true"/"false" strings.
pub fn opt_bool(input: &Value, key: &str) -> Result<Option<bool>> {
    match input.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(Value::String(s)) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "yes" | "1" => Ok(Some(true)),
            "false" | "no" | "0" | "" => Ok(Some(false)),
            _ => bail!("parameter `{key}` must be a boolean"),
        },
        Some(Value::Number(n)) => Ok(Some(n.as_f64().unwrap_or(0.0) != 0.0)),
        Some(_) => bail!("parameter `{key}` must be a boolean"),
    }
}

/// Runs synchronous tool work on the blocking pool.
pub async fn blocking<F>(f: F) -> Result<crate::ToolOutput>
where
    F: FnOnce() -> Result<crate::ToolOutput> + Send + 'static,
{
    tokio::task::spawn_blocking(f).await.map_err(|e| anyhow!("tool task failed: {e}"))?
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// Unwraps a `[path#TAG]` header pasted as a path. Returns (path, tag).
pub fn unwrap_header(raw: &str) -> (String, Option<String>) {
    let t = raw.trim();
    if let Some(inner) = t.strip_prefix('[').and_then(|s| s.strip_suffix(']'))
        && let Some((p, tag)) = inner.rsplit_once('#')
            && tag.len() == 4 && tag.chars().all(|c| c.is_ascii_hexdigit()) {
                return (p.to_string(), Some(tag.to_ascii_uppercase()));
            }
    (t.to_string(), None)
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from).or_else(dirs::home_dir)
}

/// Resolves a user path: expands `~`, joins relative paths to `cwd`, normalizes `.`/`..` lexically.
pub fn resolve_path(cwd: &Path, raw: &str) -> PathBuf {
    let raw = raw.trim();
    let expanded: PathBuf = if raw == "~" {
        home_dir().unwrap_or_else(|| PathBuf::from("~"))
    } else if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\").filter(|_| cfg!(windows))) {
        match home_dir() {
            Some(h) => h.join(rest),
            None => PathBuf::from(raw),
        }
    } else {
        PathBuf::from(raw)
    };
    let joined = if expanded.is_absolute() { expanded } else { cwd.join(expanded) };
    normalize_lexical(&joined)
}

/// Lexical normalization (no filesystem access).
pub fn normalize_lexical(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Prefix(x) => out.push(x.as_os_str()),
            Component::RootDir => out.push(c.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() && !out.has_root() {
                    out.push("..");
                }
            }
            Component::Normal(x) => out.push(x),
        }
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    out
}

/// Path shown to the model: relative to `cwd` when inside it, else absolute.
pub fn display_path(cwd: &Path, abs: &Path) -> String {
    let cwd = normalize_lexical(cwd);
    match abs.strip_prefix(&cwd) {
        Ok(rel) if rel.as_os_str().is_empty() => ".".to_string(),
        Ok(rel) => rel.to_string_lossy().into_owned(),
        Err(_) => abs.to_string_lossy().into_owned(),
    }
}

/// Key used by the snapshot store (canonical when the file exists).
pub fn store_key(abs: &Path) -> PathBuf {
    std::fs::canonicalize(abs).unwrap_or_else(|_| abs.to_path_buf())
}

// ---------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------

/// A text file decoded into LF-normalized form plus the info needed to restore it.
#[derive(Debug, Clone, Default)]
pub struct TextFile {
    pub bom: bool,
    pub crlf: bool,
    /// BOM-stripped, LF-normalized text.
    pub text: String,
}

impl TextFile {
    pub fn decode(raw: &str) -> TextFile {
        let (bom, body) = match raw.strip_prefix('\u{FEFF}') {
            Some(b) => (true, b),
            None => (false, raw),
        };
        let crlf_count = body.matches("\r\n").count();
        let lf_count = body.matches('\n').count();
        let crlf = crlf_count > 0 && crlf_count * 2 >= lf_count;
        TextFile { bom, crlf, text: normalize_to_lf(body) }
    }

    /// Re-encodes LF text with this file's BOM / line-ending convention.
    pub fn encode(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len() + 8);
        if self.bom {
            out.push('\u{FEFF}');
        }
        if self.crlf {
            out.push_str(&text.replace('\n', "\r\n"));
        } else {
            out.push_str(text);
        }
        out
    }
}

/// `\r\n` -> `\n`, lone `\r` -> `\n`.
pub fn normalize_to_lf(s: &str) -> String {
    if !s.contains('\r') {
        return s.to_string();
    }
    s.replace("\r\n", "\n").replace('\r', "\n")
}

/// Splits LF text into lines. A trailing newline does not produce an extra empty line.
/// Returns (lines, has_trailing_newline). `""` -> `([], false)`.
pub fn split_lines(text: &str) -> (Vec<&str>, bool) {
    if text.is_empty() {
        return (Vec::new(), false);
    }
    let trailing = text.ends_with('\n');
    let body = if trailing { &text[..text.len() - 1] } else { text };
    (body.split('\n').collect(), trailing)
}

pub fn join_lines<S: AsRef<str>>(lines: &[S], trailing_newline: bool) -> String {
    let mut out = String::new();
    for (i, l) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(l.as_ref());
    }
    if trailing_newline && !lines.is_empty() {
        out.push('\n');
    }
    out
}

/// Truncates to at most `max` chars, appending `…` when cut. Returns (text, was_cut).
pub fn truncate_chars(s: &str, max: usize) -> (String, bool) {
    match s.char_indices().nth(max) {
        None => (s.to_string(), false),
        Some((idx, _)) => (format!("{}…", &s[..idx]), true),
    }
}

/// Largest index <= `i` that is a char boundary.
pub fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Smallest index >= `i` that is a char boundary.
pub fn ceil_char_boundary(s: &str, mut i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    while !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// Binary sniff: NUL in the first 8 KiB.
pub fn looks_binary(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(8192)].contains(&0)
}

static ANSI_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[PX^_][^\x1b]*\x1b\\|\x1b[@-Z\\-_]")
        .expect("valid regex")
});

pub fn strip_ansi(s: &str) -> String {
    if !s.contains('\x1b') {
        return s.to_string();
    }
    ANSI_RE.replace_all(s, "").into_owned()
}

pub fn human_bytes(n: u64) -> String {
    if n < 1024 {
        format!("{n} B")
    } else if n < 1024 * 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{:.1} MB", n as f64 / (1024.0 * 1024.0))
    }
}

/// Unified diff (3 context lines) plus added/removed line counts.
pub fn unified_diff(old: &str, new: &str, label: &str) -> (String, usize, usize) {
    let diff = similar::TextDiff::from_lines(old, new);
    let mut added = 0;
    let mut removed = 0;
    for change in diff.iter_all_changes() {
        match change.tag() {
            similar::ChangeTag::Insert => added += 1,
            similar::ChangeTag::Delete => removed += 1,
            similar::ChangeTag::Equal => {}
        }
    }
    let text = diff
        .unified_diff()
        .context_radius(3)
        .header(&format!("a/{label}"), &format!("b/{label}"))
        .to_string();
    (text, added, removed)
}

/// Formats a sorted list of 1-based line numbers compactly: `5-7, 10`.
pub fn format_line_list(lines: &[u32]) -> String {
    let mut parts = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let start = lines[i];
        let mut end = start;
        while i + 1 < lines.len() && lines[i + 1] == end + 1 {
            i += 1;
            end = lines[i];
        }
        parts.push(if start == end { start.to_string() } else { format!("{start}-{end}") });
        i += 1;
    }
    parts.join(", ")
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

static SPILL_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Fresh path under `<tmp>/ref-tool-output/` for a tool's full output (`ext`: file extension).
pub fn spill_path(ext: &str) -> PathBuf {
    let n = SPILL_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let id = xxhash_rust::xxh32::xxh32(format!("{nanos}-{n}-{}", std::process::id()).as_bytes(), 0);
    std::env::temp_dir()
        .join("ref-tool-output")
        .join(format!("{:08x}{:x}.{ext}", id, nanos as u64 & 0xffff_ffff))
}

/// Detects supported raster images by magic bytes.
pub fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    None
}

#[cfg(test)]
pub mod test_support {
    use crate::{HashStore, ToolCtx};
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use tokio_util::sync::CancellationToken;

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Temp directory removed on drop.
    pub struct TempDir(PathBuf);

    impl std::ops::Deref for TempDir {
        type Target = PathBuf;
        fn deref(&self) -> &PathBuf {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Fresh empty directory under the system temp dir (canonicalized).
    pub fn temp_dir(tag: &str) -> TempDir {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("ref-tools-test-{tag}-{}-{nanos}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(std::fs::canonicalize(&dir).unwrap())
    }

    pub fn ctx(cwd: &std::path::Path) -> ToolCtx {
        ToolCtx {
            cwd: cwd.to_path_buf(),
            store: Arc::new(HashStore::new()),
            cancel: CancellationToken::new(),
            on_output: None,
            resize_images: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_and_display() {
        let cwd = Path::new("/work/proj");
        assert_eq!(resolve_path(cwd, "src/../a.rs"), PathBuf::from("/work/proj/a.rs"));
        assert_eq!(resolve_path(cwd, "/etc/./hosts"), PathBuf::from("/etc/hosts"));
        assert_eq!(resolve_path(cwd, "../other"), PathBuf::from("/work/other"));
        assert_eq!(display_path(cwd, Path::new("/work/proj/src/a.rs")), "src/a.rs");
        assert_eq!(display_path(cwd, Path::new("/work/other")), "/work/other");
        if let Some(h) = home_dir() {
            assert_eq!(resolve_path(cwd, "~/x"), h.join("x"));
        }
    }

    #[test]
    fn header_unwrap() {
        assert_eq!(unwrap_header("[src/a.rs#ab12]"), ("src/a.rs".into(), Some("AB12".into())));
        assert_eq!(unwrap_header("src/a.rs"), ("src/a.rs".into(), None));
    }

    #[test]
    fn text_roundtrip() {
        let tf = TextFile::decode("\u{FEFF}a\r\nb\r\n");
        assert!(tf.bom && tf.crlf);
        assert_eq!(tf.text, "a\nb\n");
        assert_eq!(tf.encode("a\nc\n"), "\u{FEFF}a\r\nc\r\n");
        assert_eq!(split_lines(""), (vec![], false));
        assert_eq!(split_lines("a\n"), (vec!["a"], true));
        assert_eq!(split_lines("a\n\nb"), (vec!["a", "", "b"], false));
        assert_eq!(join_lines(&["a", "b"], true), "a\nb\n");
    }

    #[test]
    fn misc() {
        assert_eq!(truncate_chars("héllo", 2), ("hé…".into(), true));
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m"), "red");
        assert_eq!(format_line_list(&[1, 2, 3, 5, 7, 8]), "1-3, 5, 7-8");
        assert_eq!(value_u64(Some(&serde_json::json!("12")), "x").unwrap(), Some(12));
        assert!(value_u64(Some(&serde_json::json!(-1)), "x").is_err());
    }
}
