//! Hashline: per-file snapshot tags, the session snapshot store, and the patch engine.
//!
//! A file's TAG is `xxh32(normalized text, seed 0) & 0xFFFF` as 4 uppercase hex chars, where the
//! text is BOM-stripped, CRLF/CR→LF, and each line's trailing `[ \t\r]` is ignored for hashing.

mod store;
pub mod parse;
pub mod apply;
pub mod engine;

pub use store::{EditStore, Origin, Seen, Snapshot, MAX_SNAPSHOT_BYTES};

use std::collections::BTreeSet;
use xxhash_rust::xxh32::Xxh32;

/// Inline reveal cap for the seen-line guard (lines).
pub const SEEN_REVEAL_CAP: usize = 40;
/// Max columns for a line to count as fully displayed.
pub const MAX_COLUMNS: usize = 768;

/// Compute the 4-hex tag of LF-normalized text (trailing spaces/tabs/CR per line ignored).
pub fn file_tag(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut hasher = Xxh32::new(0);
    let mut run_start = 0;
    let mut line_start = 0;
    for segment in text.split_inclusive('\n') {
        let line = segment.strip_suffix('\n').unwrap_or(segment);
        let kept = line.trim_end_matches([' ', '\t', '\r']).len();
        if kept < line.len() {
            hasher.update(&bytes[run_start..line_start + kept]);
            run_start = line_start + line.len();
        }
        line_start += segment.len();
    }
    hasher.update(&bytes[run_start..]);
    format!("{:04X}", hasher.digest() & 0xffff)
}

/// A decoded text file: normalized LF text plus the encoding details needed to write it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextFile {
    /// BOM-stripped, LF-normalized text.
    pub text: String,
    pub bom: bool,
    pub crlf: bool,
}

impl TextFile {
    /// Decode raw bytes. `None` if not valid UTF-8.
    pub fn decode(bytes: &[u8]) -> Option<TextFile> {
        let raw = std::str::from_utf8(bytes).ok()?;
        Some(Self::from_text(raw))
    }

    pub fn from_text(raw: &str) -> TextFile {
        let (bom, body) = match raw.strip_prefix('\u{feff}') {
            Some(rest) => (true, rest),
            None => (false, raw),
        };
        let crlf = body
            .find('\n')
            .map(|i| i > 0 && body.as_bytes()[i - 1] == b'\r')
            .unwrap_or(false);
        TextFile { text: normalize_lf(body), bom, crlf }
    }

    pub fn tag(&self) -> String {
        file_tag(&self.text)
    }

    /// Re-encode `text` (LF) using this file's BOM/EOL style.
    pub fn encode(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len() + 8);
        if self.bom {
            out.push('\u{feff}');
        }
        if self.crlf {
            out.push_str(&text.replace('\n', "\r\n"));
        } else {
            out.push_str(text);
        }
        out
    }
}

/// `\r\n` and lone `\r` → `\n`.
pub fn normalize_lf(s: &str) -> String {
    if !s.contains('\r') {
        return s.to_string();
    }
    s.replace("\r\n", "\n").replace('\r', "\n")
}

/// Addressable lines of LF text: the trailing newline does not open an extra line.
pub fn split_lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<&str> = text.split('\n').collect();
    if text.ends_with('\n') {
        lines.pop();
    }
    lines
}

pub fn header(path: &str, tag: &str) -> String {
    format!("[{path}#{tag}]")
}

/// `1-3, 7` style compact list.
pub fn format_line_ranges(lines: &[u32]) -> String {
    let set: BTreeSet<u32> = lines.iter().copied().collect();
    let mut parts = Vec::new();
    let mut iter = set.into_iter();
    let Some(first) = iter.next() else { return String::new() };
    let (mut start, mut prev) = (first, first);
    for cur in iter {
        if cur == prev + 1 {
            prev = cur;
            continue;
        }
        parts.push(if start == prev { start.to_string() } else { format!("{start}-{prev}") });
        start = cur;
        prev = cur;
    }
    parts.push(if start == prev { start.to_string() } else { format!("{start}-{prev}") });
    parts.join(", ")
}

/// Rows around `anchors` (±2), anchors marked `*`, others ` `, `...` between runs.
/// Returns (rows, displayed line numbers that were shown in full).
pub fn anchored_context(anchors: &[u32], lines: &[&str]) -> (Vec<String>, Vec<u32>) {
    let n = lines.len() as u32;
    let mut display = BTreeSet::new();
    for &a in anchors {
        if a < 1 || a > n {
            continue;
        }
        let lo = a.saturating_sub(2).max(1);
        let hi = (a + 2).min(n);
        display.extend(lo..=hi);
    }
    let anchor_set: BTreeSet<u32> = anchors.iter().copied().collect();
    let mut rows = Vec::new();
    let mut shown = Vec::new();
    let mut prev: Option<u32> = None;
    for line in display {
        if prev.is_some_and(|p| line > p + 1) {
            rows.push("...".to_string());
        }
        prev = Some(line);
        let marker = if anchor_set.contains(&line) { '*' } else { ' ' };
        let text = lines[(line - 1) as usize];
        let (shown_text, cut) = crate::util::truncate_chars(text, MAX_COLUMNS);
        if !cut {
            shown.push(line);
        }
        rows.push(format!("{marker}{line}:{shown_text}"));
    }
    (rows, shown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_ignores_trailing_ws_and_is_4_hex() {
        let a = file_tag("fn main() {\n    x();\n}\n");
        let b = file_tag("fn main() {  \n    x();\t\n}\n");
        assert_eq!(a, b);
        assert_eq!(a.len(), 4);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_lowercase()));
        assert_ne!(a, file_tag("fn main() {\n    y();\n}\n"));
        // Matches the reference definition: xxh32 over the whitespace-trimmed text.
        let expect = format!("{:04X}", xxhash_rust::xxh32::xxh32(b"a\nb\n", 0) & 0xffff);
        assert_eq!(file_tag("a  \nb\n"), expect);
    }

    #[test]
    fn decode_encode_roundtrip() {
        let raw = "\u{feff}a\r\nb\r\n";
        let f = TextFile::from_text(raw);
        assert!(f.bom && f.crlf);
        assert_eq!(f.text, "a\nb\n");
        assert_eq!(f.encode(&f.text), raw);
        assert_eq!(f.tag(), file_tag("a\nb\n"));
    }

    #[test]
    fn split_lines_semantics() {
        assert!(split_lines("").is_empty());
        assert_eq!(split_lines("a"), vec!["a"]);
        assert_eq!(split_lines("a\n"), vec!["a"]);
        assert_eq!(split_lines("a\n\n"), vec!["a", ""]);
    }

    #[test]
    fn ranges_format() {
        assert_eq!(format_line_ranges(&[7, 1, 2, 3, 9, 10]), "1-3, 7, 9-10");
    }
}
