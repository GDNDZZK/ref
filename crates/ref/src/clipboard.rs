//! Clipboard: write via wl-copy / xclip / xsel (async) plus OSC 52; read images via wl-paste / xclip.

use std::io::Write;
use std::process::Stdio;
use std::time::Duration;

use base64::Engine;
use tokio::io::AsyncWriteExt;

/// OSC 52 escape sequence for `text` (wrapped for tmux passthrough when inside tmux).
pub fn osc52(text: &str) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    let seq = format!("\x1b]52;c;{b64}\x07");
    if std::env::var_os("TMUX").is_some() {
        format!("\x1bPtmux;{}\x1b\\", seq.replace('\x1b', "\x1b\x1b"))
    } else {
        seq
    }
}

/// Copy text: OSC 52 written to the terminal immediately, host clipboard tool run in the background.
pub fn copy(text: String) {
    if cfg!(test) {
        return;
    }
    {
        let mut out = std::io::stdout();
        let _ = out.write_all(osc52(&text).as_bytes());
        let _ = out.flush();
    }
    tokio::spawn(async move {
        let mut candidates: Vec<(&str, Vec<&str>)> = Vec::new();
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            candidates.push(("wl-copy", vec![]));
        }
        if std::env::var_os("DISPLAY").is_some() {
            candidates.push(("xclip", vec!["-selection", "clipboard"]));
            candidates.push(("xsel", vec!["--clipboard", "--input"]));
        }
        for (cmd, args) in candidates {
            if pipe_to(cmd, &args, text.as_bytes()).await {
                break;
            }
        }
    });
}

async fn pipe_to(cmd: &str, args: &[&str], data: &[u8]) -> bool {
    let child = tokio::process::Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(false)
        .spawn();
    let Ok(mut child) = child else { return false };
    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(data).await.is_err() {
            return false;
        }
        drop(stdin);
    }
    // wl-copy / xclip fork to keep serving the selection; the parent exits quickly.
    matches!(tokio::time::timeout(Duration::from_secs(3), child.wait()).await, Ok(Ok(s)) if s.success())
}

/// Read a PNG image from the clipboard, if any.
pub async fn read_image() -> Option<Vec<u8>> {
    let (cmd, args): (&str, Vec<&str>) = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        ("wl-paste", vec!["--no-newline", "-t", "image/png"])
    } else if std::env::var_os("DISPLAY").is_some() {
        ("xclip", vec!["-selection", "clipboard", "-t", "image/png", "-o"])
    } else {
        return None;
    };
    let fut = tokio::process::Command::new(cmd).args(&args).stdin(Stdio::null()).stderr(Stdio::null()).output();
    let out = tokio::time::timeout(Duration::from_secs(3), fut).await.ok()?.ok()?;
    if out.status.success() && out.stdout.starts_with(b"\x89PNG") {
        Some(out.stdout)
    } else {
        None
    }
}

/// Read plain text from the clipboard (used for ctrl+v when no image is present).
pub async fn read_text() -> Option<String> {
    let (cmd, args): (&str, Vec<&str>) = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        ("wl-paste", vec!["--no-newline", "-t", "text/plain"])
    } else if std::env::var_os("DISPLAY").is_some() {
        ("xclip", vec!["-selection", "clipboard", "-o"])
    } else {
        return None;
    };
    let fut = tokio::process::Command::new(cmd).args(&args).stdin(Stdio::null()).stderr(Stdio::null()).output();
    let out = tokio::time::timeout(Duration::from_secs(3), fut).await.ok()?.ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

/// If `s` is a path (or file:// URL, possibly quoted) to an image file, return (mime, bytes, name).
pub fn image_from_path(s: &str) -> Option<(String, Vec<u8>, String)> {
    let s = s.trim();
    if s.contains('\n') || s.len() > 4096 {
        return None;
    }
    let s = s.trim_matches(|c| c == '\'' || c == '"');
    let s = s.strip_prefix("file://").unwrap_or(s);
    let s = s.replace("\\ ", " ");
    let s = percent_decode(&s);
    let lower = s.to_ascii_lowercase();
    let mime = if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else {
        return None;
    };
    let path = std::path::Path::new(&s);
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > 20 * 1024 * 1024 {
        return None;
    }
    let data = std::fs::read(path).ok()?;
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    Some((mime.to_string(), data, name))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

pub fn b64(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc() {
        std::env::remove_var("TMUX");
        assert_eq!(osc52("hi"), "\x1b]52;c;aGk=\x07");
    }

    #[test]
    fn image_paths() {
        let dir = std::env::temp_dir().join(format!("ref-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a b.png");
        std::fs::write(&p, b"\x89PNGxx").unwrap();
        let quoted = format!("'{}'", p.display());
        let (mime, data, name) = image_from_path(&quoted).unwrap();
        assert_eq!(mime, "image/png");
        assert_eq!(data.len(), 6);
        assert_eq!(name, "a b.png");
        let url = format!("file://{}", p.display().to_string().replace(' ', "%20"));
        assert!(image_from_path(&url).is_some());
        assert!(image_from_path("not a path.txt").is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
