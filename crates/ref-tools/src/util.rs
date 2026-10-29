//! Shared helpers: parameter parsing, path resolution/display, text normalization, truncation.

use std::path::{Component, Path, PathBuf};

use serde_json::Value;

/// Default cap for a single tool result.
pub const MAX_OUTPUT_BYTES: usize = 50 * 1024;
pub const MAX_OUTPUT_LINES: usize = 2000;

pub type ToolResult<T> = Result<T, String>;

// ───────────────────────────── params ─────────────────────────────

pub fn req_str<'a>(input: &'a Value, key: &str) -> ToolResult<&'a str> {
    match input.get(key) {
        None | Some(Value::Null) => Err(format!("missing required parameter `{key}`")),
        Some(Value::String(s)) => Ok(s.as_str()),
        Some(_) => Err(format!("parameter `{key}` must be a string")),
    }
}

pub fn opt_str<'a>(input: &'a Value, key: &str) -> ToolResult<Option<&'a str>> {
    match input.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.as_str())),
        Some(_) => Err(format!("parameter `{key}` must be a string")),
    }
}

pub fn opt_bool(input: &Value, key: &str) -> ToolResult<Option<bool>> {
    match input.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(Value::String(s)) if s == "true" => Ok(Some(true)),
        Some(Value::String(s)) if s == "false" => Ok(Some(false)),
        Some(_) => Err(format!("parameter `{key}` must be a boolean")),
    }
}

/// Non-negative integer parameter (accepts integral floats and numeric strings).
pub fn opt_u64(input: &Value, key: &str) -> ToolResult<Option<u64>> {
    let bad = || format!("parameter `{key}` must be a non-negative integer");
    match input.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => {
            if let Some(u) = n.as_u64() {
                Ok(Some(u))
            } else if let Some(f) = n.as_f64() {
                if f >= 0.0 && f.fract() == 0.0 && f < 1e15 {
                    Ok(Some(f as u64))
                } else {
                    Err(bad())
                }
            } else {
                Err(bad())
            }
        }
        Some(Value::String(s)) => s.trim().parse::<u64>().map(Some).map_err(|_| bad()),
        Some(_) => Err(bad()),
    }
}

// ───────────────────────────── paths ─────────────────────────────

pub fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

/// Lexically normalize `.` and `..` components (no filesystem access).
pub fn normalize_lexical(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Resolve a user/model supplied path: absolute, `~/…`, or relative to `cwd`.
pub fn resolve_path(cwd: &Path, raw: &str) -> PathBuf {
    let raw = raw.trim();
    let p = if raw == "~" {
        home_dir().unwrap_or_else(|| PathBuf::from("~"))
    } else if let Some(rest) = raw.strip_prefix("~/") {
        match home_dir() {
            Some(h) => h.join(rest),
            None => PathBuf::from(raw),
        }
    } else {
        let p = PathBuf::from(raw);
        if p.is_absolute() {
            p
        } else {
            cwd.join(p)
        }
    };
    normalize_lexical(&p)
}

/// Canonical key for the snapshot store (falls back to the lexical path).
pub fn canonical_key(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| normalize_lexical(p))
}

/// Path shown to the model: cwd-relative inside the workspace, `~`-shortened or absolute outside.
pub fn display_path(cwd: &Path, p: &Path) -> String {
    let cwd_n = normalize_lexical(cwd);
    if let Ok(rel) = p.strip_prefix(&cwd_n) {
        let s = rel.to_string_lossy().to_string();
        return if s.is_empty() { ".".to_string() } else { s };
    }
    // Try canonical forms (symlinked cwd, e.g. /tmp on macOS).
    if let (Ok(cc), Ok(pc)) = (std::fs::canonicalize(&cwd_n), std::fs::canonicalize(p)) {
        if let Ok(rel) = pc.strip_prefix(&cc) {
            let s = rel.to_string_lossy().to_string();
            return if s.is_empty() { ".".to_string() } else { s };
        }
    }
    if let Some(h) = home_dir() {
        if let Ok(rel) = p.strip_prefix(&h) {
            return format!("~/{}", rel.to_string_lossy());
        }
    }
    p.to_string_lossy().to_string()
}

// ───────────────────────────── text ─────────────────────────────

/// Line ending convention of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eol {
    Lf,
    CrLf,
}

/// Decoded text file: LF-normalized, BOM-stripped text plus how to restore it.
#[derive(Debug, Clone)]
pub struct TextFile {
    pub text: String,
    pub bom: bool,
    pub eol: Eol,
}

impl TextFile {
    /// Re-encode normalized text with this file's BOM / line ending convention.
    pub fn encode(&self, text: &str) -> Vec<u8> {
        let mut out = String::with_capacity(text.len() + 8);
        if self.bom {
            out.push('\u{feff}');
        }
        match self.eol {
            Eol::Lf => out.push_str(text),
            Eol::CrLf => out.push_str(&text.replace('\n', "\r\n")),
        }
        out.into_bytes()
    }
}

/// True if the first 8 KiB contain a NUL byte.
pub fn looks_binary(bytes: &[u8]) -> bool {
    let n = bytes.len().min(8192);
    bytes[..n].contains(&0)
}

/// Normalize CRLF to LF. A lone CR is content (e.g. inside a string literal), not a line break.
pub fn normalize_eol(s: &str) -> String {
    if s.contains("\r\n") {
        s.replace("\r\n", "\n")
    } else {
        s.to_string()
    }
}

/// Decode file bytes as text. Errors (as a message fragment) for binary content.
///
/// CRLF is normalized only when EVERY line break is CRLF; files with mixed endings keep their
/// `\r` bytes as line content so untouched lines are written back byte-identical.
pub fn decode_text(bytes: Vec<u8>) -> Result<TextFile, &'static str> {
    if looks_binary(&bytes) {
        return Err("contains NUL bytes");
    }
    let s = String::from_utf8(bytes).map_err(|_| "not valid UTF-8 text")?;
    let (bom, s) = match s.strip_prefix('\u{feff}') {
        Some(rest) => (true, rest.to_string()),
        None => (false, s),
    };
    let lf = s.matches('\n').count();
    let crlf = s.matches("\r\n").count();
    if lf > 0 && crlf == lf {
        Ok(TextFile {
            text: s.replace("\r\n", "\n"),
            bom,
            eol: Eol::CrLf,
        })
    } else {
        Ok(TextFile {
            text: s,
            bom,
            eol: Eol::Lf,
        })
    }
}

/// Addressable lines of normalized text (a trailing newline does not create an extra line).
pub fn split_lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return vec![];
    }
    let mut v: Vec<&str> = text.split('\n').collect();
    if text.ends_with('\n') {
        v.pop();
    }
    v
}

/// Join lines back; `trailing_nl` adds a final newline when there is at least one line.
pub fn join_lines<S: AsRef<str>>(lines: &[S], trailing_nl: bool) -> String {
    let mut out = String::new();
    for (i, l) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(l.as_ref());
    }
    if trailing_nl && !lines.is_empty() {
        out.push('\n');
    }
    out
}

/// Cut a line to `max` chars; returns (text, was_truncated).
pub fn cap_line(line: &str, max: usize) -> (String, bool) {
    match line.char_indices().nth(max) {
        None => (line.to_string(), false),
        Some((idx, _)) => (format!("{}…", &line[..idx]), true),
    }
}

/// Largest char boundary <= i.
pub fn floor_boundary(s: &str, mut i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Smallest char boundary >= i.
pub fn ceil_boundary(s: &str, mut i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    while !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// Remove ANSI escape sequences (CSI, OSC, and two-byte escapes) and stray carriage-return redraws.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.peek().copied() {
            Some('[') => {
                chars.next();
                // CSI: parameters/intermediates until a final byte in 0x40..=0x7e
                for d in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&d) {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next();
                // OSC: until BEL or ESC \
                while let Some(d) = chars.next() {
                    if d == '\u{07}' {
                        break;
                    }
                    if d == '\u{1b}' {
                        if chars.peek() == Some(&'\\') {
                            chars.next();
                        }
                        break;
                    }
                }
            }
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }
    out
}

/// Byte size in human form.
pub fn human_bytes(n: u64) -> String {
    if n < 1024 {
        format!("{n} B")
    } else if n < 1024 * 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{:.1} MB", n as f64 / (1024.0 * 1024.0))
    }
}

/// Compact list of line numbers: `5-7, 10`.
pub fn format_line_list(lines: &[usize]) -> String {
    let mut v: Vec<usize> = lines.to_vec();
    v.sort_unstable();
    v.dedup();
    let mut parts = vec![];
    let mut i = 0;
    while i < v.len() {
        let start = v[i];
        let mut end = start;
        while i + 1 < v.len() && v[i + 1] == end + 1 {
            i += 1;
            end = v[i];
        }
        parts.push(if start == end {
            start.to_string()
        } else {
            format!("{start}-{end}")
        });
        i += 1;
    }
    parts.join(", ")
}

/// Levenshtein distance (chars), for "did you mean" suggestions.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// If every non-empty line starts with `N:` with consecutive N, return the stripped lines.
pub fn strip_numbered_prefixes(lines: &[&str]) -> Option<Vec<String>> {
    let mut expected: Option<u64> = None;
    let mut out = Vec::with_capacity(lines.len());
    let mut any = false;
    for l in lines {
        if l.is_empty() {
            out.push(String::new());
            continue;
        }
        let (n, rest) = parse_num_prefix(l)?;
        if let Some(e) = expected {
            if n != e {
                return None;
            }
        }
        expected = Some(n + 1);
        any = true;
        out.push(rest.to_string());
    }
    any.then_some(out)
}

/// Parse `N:rest` (N ≥ 1, decimal).
pub fn parse_num_prefix(l: &str) -> Option<(u64, &str)> {
    let digits = l.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 || digits > 9 || l.as_bytes().get(digits) != Some(&b':') {
        return None;
    }
    let n: u64 = l[..digits].parse().ok()?;
    if n == 0 {
        return None;
    }
    Some((n, &l[digits + 1..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi_and_lines() {
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m \x1b]0;t\x07ok"), "red ok");
        assert_eq!(split_lines("a\nb\n"), vec!["a", "b"]);
        assert_eq!(split_lines("a\nb"), vec!["a", "b"]);
        assert!(split_lines("").is_empty());
        assert_eq!(split_lines("\n"), vec![""]);
        assert_eq!(format_line_list(&[10, 5, 6, 7]), "5-7, 10");
        assert_eq!(cap_line("héllo", 2), ("hé…".to_string(), true));
        assert_eq!(
            strip_numbered_prefixes(&["3:a", "4:b"]),
            Some(vec!["a".into(), "b".into()])
        );
        assert_eq!(strip_numbered_prefixes(&["3:a", "5:b"]), None);
    }

    #[test]
    fn decode_eol_bom() {
        let t = decode_text(b"\xef\xbb\xbfa\r\nb\r\n".to_vec()).unwrap();
        assert!(t.bom);
        assert_eq!(t.eol, Eol::CrLf);
        assert_eq!(t.text, "a\nb\n");
        assert_eq!(t.encode("x\n"), b"\xef\xbb\xbfx\r\n".to_vec());
    }
}
