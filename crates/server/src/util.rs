//! Small helpers: ids, names, text truncation, atomic writes, time formatting.

use rand::Rng;
use std::path::Path;

const BASE62: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Time-sortable id: `<prefix>_` + 12 hex digits of epoch millis + 8 random base62 chars.
pub fn new_id(prefix: &str) -> String {
    let ms = ref_proto::now_ms().max(0) as u64;
    let mut s = format!("{prefix}_{:012x}", ms & 0xffff_ffff_ffff);
    let mut rng = rand::rng();
    for _ in 0..8 {
        s.push(BASE62[rng.random_range(0..BASE62.len())] as char);
    }
    s
}

pub fn random_hex(n: usize) -> String {
    let mut rng = rand::rng();
    (0..n)
        .map(|_| char::from_digit(rng.random_range(0..16u32), 16).unwrap_or('0'))
        .collect()
}

/// Validates and normalizes a peer name: trimmed, 1..=64 chars, no control characters.
pub fn normalize_name(raw: &str) -> Result<String, String> {
    let name = raw.trim();
    let n = name.chars().count();
    if n == 0 {
        return Err("Session name must not be empty.".into());
    }
    if n > 64 {
        return Err("Session name must be at most 64 characters.".into());
    }
    if name.chars().any(char::is_control) {
        return Err("Session name must not contain control characters.".into());
    }
    Ok(name.to_string())
}

/// First `max` characters of `s` (char-boundary safe), with `…` appended when truncated.
pub fn truncate_chars(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

/// Keeps the head and tail of a long text (byte budget), noting the omission.
pub fn truncate_middle(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let head = floor_boundary(s, max_bytes * 2 / 5);
    let tail = ceil_boundary(s, s.len() - max_bytes * 3 / 5);
    format!(
        "{}\n\n[... {} bytes truncated ...]\n\n{}",
        &s[..head],
        tail - head,
        &s[tail..]
    )
}

/// Last `max_bytes` of a string (char-boundary safe).
pub fn tail_bytes(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    &s[ceil_boundary(s, s.len() - max_bytes)..]
}

pub fn floor_boundary(s: &str, mut i: usize) -> usize {
    i = i.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

pub fn ceil_boundary(s: &str, mut i: usize) -> usize {
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i.min(s.len())
}

/// Atomic file write: temp file in the same directory + rename. `private` => mode 0600.
pub fn write_atomic(path: &Path, data: &[u8], private: bool) -> std::io::Result<()> {
    use std::io::Write;
    // Windows: files under the user profile are already private to the user.
    #[cfg(not(unix))]
    let _ = private;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("tmp{}", random_hex(6)));
    let res = (|| {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            if private {
                opts.mode(0o600);
            }
        }
        let mut f = opts.open(&tmp)?;
        f.write_all(data)?;
        f.sync_all().ok();
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res
}

/// `YYYY-MM-DD HH:MM:SS` in local time.
pub fn fmt_time(ms: i64) -> String {
    use chrono::TimeZone;
    match chrono::Local.timestamp_millis_opt(ms).single() {
        Some(t) => t.format("%Y-%m-%d %H:%M:%S").to_string(),
        None => ms.to_string(),
    }
}

/// `YYYY-MM-DD` in local time.
pub fn fmt_date(ms: i64) -> String {
    use chrono::TimeZone;
    match chrono::Local.timestamp_millis_opt(ms).single() {
        Some(t) => t.format("%Y-%m-%d").to_string(),
        None => String::new(),
    }
}

/// Human duration like `3h 12m` / `45s`.
pub fn fmt_duration(secs: i64) -> String {
    let s = secs.max(0);
    let (d, h, m, sec) = (s / 86400, s / 3600 % 24, s / 60 % 60, s % 60);
    if d > 0 {
        format!("{d}d {h}h {m}m")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m {sec}s")
    } else {
        format!("{sec}s")
    }
}

/// Expand a leading `~/`.
pub fn expand_home(p: &str, home: Option<&Path>) -> std::path::PathBuf {
    if p == "~"
        && let Some(h) = home
    {
        return h.to_path_buf();
    }
    if let Some(rest) = p.strip_prefix("~/")
        && let Some(h) = home
    {
        return h.join(rest);
    }
    std::path::PathBuf::from(p)
}

/// Whether a process with this pid exists.
/// A relative path written with `/` on every platform, as clients expect it in the protocol.
pub fn relative_slash(rel: &Path) -> String {
    let parts: Vec<_> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect();
    parts.join("/")
}

pub fn pid_alive(pid: u32) -> bool {
    ref_sys::pid_alive(pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_are_joined_with_slashes() {
        let rel = Path::new("src").join("sub").join("a.rs");
        assert_eq!(relative_slash(&rel), "src/sub/a.rs");
        assert_eq!(relative_slash(Path::new("")), "");
        assert_eq!(relative_slash(Path::new("one")), "one");
    }

    #[test]
    fn ids_sortable_and_prefixed() {
        let a = new_id("ses");
        assert!(a.starts_with("ses_"));
        assert_eq!(a.len(), 4 + 20);
    }

    #[test]
    fn names() {
        assert_eq!(normalize_name("  ワーカー🚀 ").unwrap(), "ワーカー🚀");
        assert!(normalize_name("   ").is_err());
        assert!(normalize_name("a\nb").is_err());
        assert!(normalize_name(&"x".repeat(65)).is_err());
        assert!(normalize_name(&"é".repeat(64)).is_ok());
    }

    #[test]
    fn truncation() {
        assert_eq!(truncate_chars("héllo", 2), "hé…");
        assert_eq!(truncate_chars("hi", 5), "hi");
        let long = "a".repeat(1000);
        let t = truncate_middle(&long, 100);
        assert!(t.len() < 200 && t.contains("truncated"));
        assert_eq!(tail_bytes("héllo", 3), "llo");
    }
}
