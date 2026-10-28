//! Shared helpers: argument parsing, path resolution/display, text utilities, per-file locks.

use anyhow::{anyhow, bail, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

// ---------------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------------

/// Thin accessor over a tool's JSON input with model-friendly error messages.
pub struct Args<'a>(&'a serde_json::Map<String, Value>);

static EMPTY_MAP: LazyLock<serde_json::Map<String, Value>> = LazyLock::new(serde_json::Map::new);

impl<'a> Args<'a> {
    pub fn new(input: &'a Value) -> Result<Self> {
        match input {
            Value::Object(map) => Ok(Args(map)),
            Value::Null => Ok(Args(&EMPTY_MAP)),
            _ => bail!("tool input must be a JSON object"),
        }
    }

    /// Lenient constructor for title/permission helpers (never fails).
    pub fn lenient(input: &'a Value) -> Self {
        match input {
            Value::Object(map) => Args(map),
            _ => Args(&EMPTY_MAP),
        }
    }

    fn get(&self, key: &str) -> Option<&'a Value> {
        match self.0.get(key) {
            None | Some(Value::Null) => None,
            Some(v) => Some(v),
        }
    }

    pub fn str_req(&self, key: &str) -> Result<&'a str> {
        match self.get(key) {
            None => Err(anyhow!("missing required field \"{key}\"")),
            Some(Value::String(s)) => Ok(s.as_str()),
            Some(_) => Err(anyhow!("field \"{key}\" must be a string")),
        }
    }

    pub fn str_opt(&self, key: &str) -> Result<Option<&'a str>> {
        match self.get(key) {
            None => Ok(None),
            Some(Value::String(s)) if s.is_empty() => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.as_str())),
            Some(_) => Err(anyhow!("field \"{key}\" must be a string")),
        }
    }

    pub fn u64_opt(&self, key: &str) -> Result<Option<u64>> {
        match self.get(key) {
            None => Ok(None),
            Some(Value::Number(n)) => {
                if let Some(v) = n.as_u64() {
                    Ok(Some(v))
                } else if let Some(f) = n.as_f64().filter(|f| *f >= 0.0 && f.fract() == 0.0) {
                    Ok(Some(f as u64))
                } else {
                    Err(anyhow!("field \"{key}\" must be a non-negative integer"))
                }
            }
            Some(Value::String(s)) if s.trim().is_empty() => Ok(None),
            Some(Value::String(s)) => s
                .trim()
                .parse::<u64>()
                .map(Some)
                .map_err(|_| anyhow!("field \"{key}\" must be a non-negative integer")),
            Some(_) => Err(anyhow!("field \"{key}\" must be a non-negative integer")),
        }
    }

    pub fn bool_opt(&self, key: &str) -> Result<Option<bool>> {
        match self.get(key) {
            None => Ok(None),
            Some(Value::Bool(b)) => Ok(Some(*b)),
            Some(Value::String(s)) => match s.trim() {
                "true" | "1" | "yes" => Ok(Some(true)),
                "false" | "0" | "no" | "" => Ok(Some(false)),
                _ => Err(anyhow!("field \"{key}\" must be a boolean")),
            },
            Some(Value::Number(n)) => Ok(Some(n.as_f64().unwrap_or(0.0) != 0.0)),
            Some(_) => Err(anyhow!("field \"{key}\" must be a boolean")),
        }
    }

    /// Raw string for titles (empty if absent / wrong type).
    pub fn s(&self, key: &str) -> &'a str {
        match self.get(key) {
            Some(Value::String(s)) => s.as_str(),
            _ => "",
        }
    }
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from)
}

/// Expand a leading `~` / `~/`.
pub fn expand_tilde(raw: &str) -> PathBuf {
    if raw == "~" {
        if let Some(h) = home_dir() {
            return h;
        }
    } else if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(h) = home_dir() {
            return h.join(rest);
        }
    }
    PathBuf::from(raw)
}

/// Lexically normalize `.` and `..` components (no filesystem access).
pub fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    out
}

/// Resolve a model-supplied path against the session cwd (`~` expanded, lexically normalized).
pub fn resolve_path(cwd: &Path, raw: &str) -> PathBuf {
    let raw = raw.trim();
    let p = expand_tilde(raw);
    let joined = if p.is_absolute() { p } else { cwd.join(p) };
    normalize_lexical(&joined)
}

/// Canonical key for the snapshot store / file locks (symlinks resolved when possible).
pub fn canonical_key(path: &Path) -> PathBuf {
    if let Ok(c) = std::fs::canonicalize(path) {
        return c;
    }
    // Canonicalize the parent if the file itself doesn't exist (yet).
    if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
        if let Ok(c) = std::fs::canonicalize(parent) {
            return c.join(name);
        }
    }
    normalize_lexical(path)
}

/// Path as shown to the model: cwd-relative when inside cwd, else `~`-shortened absolute.
pub fn display_path(cwd: &Path, abs: &Path) -> String {
    if let Ok(rel) = abs.strip_prefix(cwd) {
        let s = rel.to_string_lossy().to_string();
        return if s.is_empty() { ".".into() } else { s };
    }
    // Also try canonical forms (cwd may be a symlink).
    if let (Ok(ccwd), Ok(cabs)) = (std::fs::canonicalize(cwd), std::fs::canonicalize(abs)) {
        if let Ok(rel) = cabs.strip_prefix(&ccwd) {
            let s = rel.to_string_lossy().to_string();
            return if s.is_empty() { ".".into() } else { s };
        }
    }
    if let Some(home) = home_dir() {
        if let Ok(rel) = abs.strip_prefix(&home) {
            return format!("~/{}", rel.to_string_lossy());
        }
    }
    abs.to_string_lossy().to_string()
}

// ---------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------

/// Truncate to at most `max` chars, appending `…` when cut. Returns (text, was_cut).
pub fn truncate_chars(s: &str, max: usize) -> (String, bool) {
    match s.char_indices().nth(max) {
        None => (s.to_string(), false),
        Some((idx, _)) => (format!("{}…", &s[..idx]), true),
    }
}

/// Largest char boundary <= `idx`.
pub fn floor_char_boundary(s: &str, idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    let mut i = idx;
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

static ANSI_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"(?x)
        \x1b\[[0-?]*[ -/]*[@-~]            # CSI
      | \x1b\][^\x07\x1b]*(?:\x07|\x1b\\)  # OSC
      | \x1b[P^_][^\x1b]*\x1b\\            # DCS/PM/APC
      | \x1b[@-Z\\-_]                      # 2-byte
      | \x9b[0-?]*[ -/]*[@-~]              # 8-bit CSI
    ",
    )
    .expect("valid ansi regex")
});

/// Remove ANSI escape sequences.
pub fn strip_ansi(s: &str) -> String {
    if !s.contains('\x1b') && !s.contains('\u{9b}') {
        return s.to_string();
    }
    ANSI_RE.replace_all(s, "").into_owned()
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{bytes}B")
    } else {
        format!("{v:.1}{}", UNITS[u])
    }
}

/// Image mime type by extension (png/jpg/jpeg/gif/webp).
pub fn image_mime(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

/// Heuristic binary detection: NUL byte in the first 8 KiB.
pub fn looks_binary(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(8192)].contains(&0)
}

// ---------------------------------------------------------------------------
// Per-file locks (serialize concurrent mutations of the same file)
// ---------------------------------------------------------------------------

type LockMap = Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>;
static FILE_LOCKS: LazyLock<LockMap> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Guards held while mutating files; dropping releases them.
pub struct FileLocks {
    _guards: Vec<tokio::sync::OwnedMutexGuard<()>>,
}

/// Acquire locks for all paths (deduplicated, sorted to avoid lock-order deadlocks).
pub async fn lock_files(paths: &[PathBuf]) -> FileLocks {
    let mut keys: Vec<PathBuf> = paths.iter().map(|p| canonical_key(p)).collect();
    keys.sort();
    keys.dedup();
    let mutexes: Vec<_> = {
        let mut map = FILE_LOCKS.lock().unwrap_or_else(|e| e.into_inner());
        // Opportunistic cleanup of idle entries.
        if map.len() > 1024 {
            map.retain(|_, m| Arc::strong_count(m) > 1);
        }
        keys.iter()
            .map(|k| map.entry(k.clone()).or_default().clone())
            .collect()
    };
    let mut guards = Vec::with_capacity(mutexes.len());
    for m in mutexes {
        guards.push(m.lock_owned().await);
    }
    FileLocks { _guards: guards }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_and_display() {
        let cwd = Path::new("/work/proj");
        assert_eq!(resolve_path(cwd, "src/../a.rs"), PathBuf::from("/work/proj/a.rs"));
        assert_eq!(resolve_path(cwd, "/etc/x"), PathBuf::from("/etc/x"));
        assert_eq!(display_path(cwd, Path::new("/work/proj/src/a.rs")), "src/a.rs");
        assert_eq!(display_path(cwd, Path::new("/work/proj")), ".");
    }

    #[test]
    fn ansi_is_stripped() {
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m \x1b]0;title\x07ok"), "red ok");
    }

    #[test]
    fn args_errors() {
        let v = serde_json::json!({"limit": "x", "n": 5});
        let a = Args::new(&v).unwrap();
        assert_eq!(a.str_req("path").unwrap_err().to_string(), "missing required field \"path\"");
        assert!(a.u64_opt("limit").is_err());
        assert_eq!(a.u64_opt("n").unwrap(), Some(5));
        assert!(Args::new(&serde_json::json!([1])).is_err());
    }

    #[test]
    fn truncation() {
        assert_eq!(truncate_chars("abcdef", 3), ("abc…".to_string(), true));
        assert_eq!(truncate_chars("abc", 3), ("abc".to_string(), false));
    }
}
