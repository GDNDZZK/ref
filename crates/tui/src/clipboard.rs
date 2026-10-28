//! Clipboard: copy via wl-copy / xclip / xsel / pbcopy with OSC 52 fallback;
//! paste images via wl-paste / xclip / pngpaste.

use base64::Engine;
use std::io::Write;
use std::process::{Command, Stdio};

fn have(cmd: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(cmd).is_file()))
        .unwrap_or(false)
}

fn wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
}

fn pipe_to(cmd: &str, args: &[&str], data: &[u8]) -> bool {
    let child = Command::new(cmd).args(args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    let Ok(mut child) = child else { return false };
    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(data).is_err() {
            return false;
        }
    }
    // wl-copy forks to serve the selection; waiting returns quickly.
    child.wait().map(|s| s.success()).unwrap_or(false)
}

/// OSC 52 sequence (written straight to the terminal).
fn osc52(text: &str) {
    let b64 = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    let seq = if std::env::var_os("TMUX").is_some() {
        format!("\x1bPtmux;\x1b\x1b]52;c;{b64}\x07\x1b\\")
    } else {
        format!("\x1b]52;c;{b64}\x07")
    };
    let mut out = std::io::stdout();
    let _ = out.write_all(seq.as_bytes());
    let _ = out.flush();
}

/// Copy text to the system clipboard. Returns the method used.
pub fn copy(text: &str) -> &'static str {
    let data = text.as_bytes();
    if wayland() && have("wl-copy") && pipe_to("wl-copy", &[], data) {
        return "wl-copy";
    }
    if cfg!(target_os = "macos") && have("pbcopy") && pipe_to("pbcopy", &[], data) {
        return "pbcopy";
    }
    if std::env::var_os("DISPLAY").is_some() {
        if have("xclip") && pipe_to("xclip", &["-selection", "clipboard"], data) {
            return "xclip";
        }
        if have("xsel") && pipe_to("xsel", &["--clipboard", "--input"], data) {
            return "xsel";
        }
    }
    osc52(text);
    "osc52"
}

fn run(cmd: &str, args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new(cmd).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    if out.status.success() && !out.stdout.is_empty() {
        Some(out.stdout)
    } else {
        None
    }
}

/// Clipboard content as an image (mime, bytes), if it holds one.
pub fn paste_image() -> Option<(String, Vec<u8>)> {
    if wayland() && have("wl-paste") {
        if let Some(types) = run("wl-paste", &["--list-types"]) {
            let types = String::from_utf8_lossy(&types);
            for mime in ["image/png", "image/jpeg", "image/webp", "image/gif"] {
                if types.lines().any(|l| l.trim() == mime) {
                    if let Some(b) = run("wl-paste", &["--no-newline", "-t", mime]) {
                        return Some((mime.into(), b));
                    }
                }
            }
        }
    }
    if std::env::var_os("DISPLAY").is_some() && have("xclip") {
        if let Some(t) = run("xclip", &["-selection", "clipboard", "-t", "TARGETS", "-o"]) {
            let t = String::from_utf8_lossy(&t);
            for mime in ["image/png", "image/jpeg"] {
                if t.lines().any(|l| l.trim() == mime) {
                    if let Some(b) = run("xclip", &["-selection", "clipboard", "-t", mime, "-o"]) {
                        return Some((mime.into(), b));
                    }
                }
            }
        }
    }
    if cfg!(target_os = "macos") && have("pngpaste") {
        if let Some(b) = run("pngpaste", &["-"]) {
            return Some(("image/png".into(), b));
        }
    }
    None
}

/// Clipboard text (for ctrl+v when no image is present).
pub fn paste_text() -> Option<String> {
    let b = if wayland() && have("wl-paste") {
        run("wl-paste", &["--no-newline", "-t", "text/plain"]).or_else(|| run("wl-paste", &["--no-newline"]))
    } else if std::env::var_os("DISPLAY").is_some() && have("xclip") {
        run("xclip", &["-selection", "clipboard", "-o"])
    } else if cfg!(target_os = "macos") {
        run("pbpaste", &[])
    } else {
        None
    }?;
    String::from_utf8(b).ok()
}

/// Mime type for an image path by extension.
pub fn image_mime(path: &str) -> Option<&'static str> {
    let ext = std::path::Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => return None,
    })
}

/// If `text` is a path to an existing image file, read it.
pub fn image_from_path(text: &str) -> Option<(String, Vec<u8>, String)> {
    let t = text.trim().trim_matches(|c| c == '\'' || c == '"');
    let t = t.strip_prefix("file://").unwrap_or(t);
    if t.contains('\n') || t.is_empty() {
        return None;
    }
    let t = t.replace("\\ ", " ");
    let mime = image_mime(&t)?;
    let path = if let Some(rest) = t.strip_prefix("~/") {
        std::path::PathBuf::from(std::env::var("HOME").ok()?).join(rest)
    } else {
        std::path::PathBuf::from(&t)
    };
    let bytes = std::fs::read(&path).ok()?;
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    Some((mime.into(), bytes, name))
}

pub fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}
