//! Small helpers: ids, text truncation, relative times.

use hn_proto::{Millis, Part};

pub fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", ulid::Ulid::new().to_string().to_lowercase())
}

pub fn rand_suffix(n: usize) -> String {
    use rand::Rng;
    const CHARS: &[u8] = b"abcdefghijkmnpqrstuvwxyz23456789";
    let mut r = rand::thread_rng();
    (0..n).map(|_| CHARS[r.gen_range(0..CHARS.len())] as char).collect()
}

/// Truncate to at most `max` chars; returns (text, was_truncated).
pub fn truncate_chars(s: &str, max: usize) -> (String, bool) {
    match s.char_indices().nth(max) {
        Some((i, _)) => (s[..i].to_string(), true),
        None => (s.to_string(), false),
    }
}

pub fn ellipsize(s: &str, max: usize) -> String {
    let (t, cut) = truncate_chars(s, max);
    if cut {
        format!("{t}…")
    } else {
        t
    }
}

pub fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// "just now" | "9m ago" | "2h ago" | "3d ago"
pub fn relative_age(since: Millis, now: Millis) -> String {
    let secs = ((now - since).max(0)) / 1000;
    if secs < 60 {
        return "just now".into();
    }
    let mins = secs / 60;
    if mins < 60 {
        return format!("{mins}m ago");
    }
    let hours = mins / 60;
    if hours < 24 {
        return format!("{hours}h ago");
    }
    format!("{}d ago", hours / 24)
}

pub fn fmt_time(ms: Millis) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|t| t.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_default()
}

/// Concatenated text parts of a message.
pub fn parts_text(parts: &[Part]) -> String {
    let mut out = String::new();
    for p in parts {
        if let Part::Text { text, .. } = p {
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(text);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_is_char_safe() {
        assert_eq!(truncate_chars("你好世界", 2), ("你好".to_string(), true));
        assert_eq!(truncate_chars("ab", 5), ("ab".to_string(), false));
        assert_eq!(ellipsize("abcdef", 3), "abc…");
    }

    #[test]
    fn ages() {
        assert_eq!(relative_age(0, 30_000), "just now");
        assert_eq!(relative_age(0, 3 * 60_000), "3m ago");
        assert_eq!(relative_age(0, 5 * 3_600_000), "5h ago");
    }
}
