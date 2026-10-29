//! Small helpers shared across modules.

use std::path::Path;
use std::time::Duration;

use rand::Rng;

/// Random 32-hex-char token.
pub fn random_token() -> String {
    let mut rng = rand::thread_rng();
    (0..16).map(|_| format!("{:02x}", rng.gen::<u8>())).collect()
}

/// Constant-time string comparison (for tokens).
pub fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Is a process with this pid alive?
pub fn pid_alive(pid: u32) -> bool {
    if pid == 0 || pid > i32::MAX as u32 {
        return false;
    }
    // SAFETY: kill with signal 0 only checks for existence/permission.
    let r = unsafe { libc::kill(pid as i32, 0) };
    r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Truncate `s` to at most `max` chars, appending a notice when cut.
pub fn truncate_output(s: String, max: usize) -> String {
    let total = s.chars().count();
    if total <= max {
        return s;
    }
    let cut: String = s.chars().take(max).collect();
    format!("{cut}\n\n[output truncated: showing {max} of {total} characters]")
}

/// Truncate to `max` chars with an ellipsis (for display strings).
pub fn ellipsize(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Rough token estimate (4 chars per token).
pub fn estimate_tokens(chars: usize) -> u64 {
    (chars as u64).div_ceil(4)
}

/// Exponential backoff for retry `attempt` (1-based): 1s, 2s, 4s, ... capped at `max`,
/// ±10% jitter; a server-provided `retry_after` (capped at `max`) wins if larger.
pub fn backoff(attempt: u32, retry_after: Option<Duration>, max: Duration) -> Duration {
    let exp = attempt.saturating_sub(1).min(30);
    let base = Duration::from_secs(1u64 << exp).min(max);
    let jitter = rand::thread_rng().gen_range(0.9..1.1);
    let mut d = base.mul_f64(jitter).min(max);
    if let Some(ra) = retry_after {
        d = d.max(ra.min(max));
    }
    d
}

/// Last path component (for default names).
pub fn basename(p: &str) -> String {
    Path::new(p)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| p.to_string())
}

/// Whether `dir` is inside a git repository.
pub fn is_git_repo(dir: &Path) -> bool {
    let mut cur = Some(dir);
    while let Some(d) = cur {
        if d.join(".git").exists() {
            return true;
        }
        cur = d.parent();
    }
    false
}

/// Write a file atomically (tmp + rename) with mode 0600.
pub fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(
        ".{}.{}.tmp",
        path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        std::process::id()
    ));
    {
        let mut f = open_private(&tmp, true)?;
        f.write_all(data)?;
        f.sync_data().ok();
    }
    std::fs::rename(&tmp, path)
}

/// Open a file for writing with mode 0600 (truncate or append).
pub fn open_private(path: &Path, truncate: bool) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut o = std::fs::OpenOptions::new();
    o.create(true).write(true).mode(0o600);
    if truncate {
        o.truncate(true);
    } else {
        o.append(true);
    }
    o.open(path)
}

/// Split command arguments shell-like (quotes `"`/`'` group words).
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut has = false;
    for c in s.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                has = true;
            }
            None if c.is_whitespace() => {
                if has || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            None => cur.push(c),
        }
    }
    if has || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Expand a command template: `$ARGUMENTS`, `$1`..`$9`. If the template references no
/// argument placeholders and arguments are given, they are appended.
pub fn expand_template(template: &str, arguments: &str) -> String {
    let args = split_args(arguments);
    let mut out = String::with_capacity(template.len() + arguments.len());
    let mut used = false;
    let mut rest = template;
    while let Some(pos) = rest.find('$') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 1..];
        if let Some(r) = after.strip_prefix("ARGUMENTS") {
            out.push_str(arguments);
            used = true;
            rest = r;
        } else if let Some(d) = after.chars().next().filter(|c| ('1'..='9').contains(c)) {
            let i = d as usize - '1' as usize;
            out.push_str(args.get(i).map(String::as_str).unwrap_or(""));
            used = true;
            rest = &after[1..];
        } else {
            out.push('$');
            rest = after;
        }
    }
    out.push_str(rest);
    if !used && !arguments.trim().is_empty() {
        out.push_str("\n\n");
        out.push_str(arguments.trim());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template() {
        assert_eq!(expand_template("do $ARGUMENTS now", "a b"), "do a b now");
        assert_eq!(expand_template("$2-$1-$3", "x \"y z\""), "y z-x-");
        assert_eq!(expand_template("cost $5", ""), "cost ");
        assert_eq!(expand_template("plain", "extra"), "plain\n\nextra");
        assert_eq!(expand_template("$$HOME", ""), "$$HOME");
    }

    #[test]
    fn backoff_bounds() {
        let max = Duration::from_secs(600);
        let d1 = backoff(1, None, max);
        assert!(d1 >= Duration::from_millis(900) && d1 <= Duration::from_millis(1100));
        let d = backoff(40, None, max);
        assert!(d <= max);
        assert_eq!(backoff(1, Some(Duration::from_secs(5000)), max), max);
        assert!(backoff(1, Some(Duration::from_secs(30)), max) >= Duration::from_secs(30));
    }

    #[test]
    fn truncation() {
        assert_eq!(truncate_output("abc".into(), 5), "abc");
        let t = truncate_output("日本語テキスト".into(), 3);
        assert!(t.starts_with("日本語\n\n[output truncated"));
        assert!(ct_eq("ab", "ab") && !ct_eq("ab", "ac") && !ct_eq("a", "ab"));
    }
}
