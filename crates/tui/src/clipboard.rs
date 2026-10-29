//! Clipboard: arboard first, then wl-copy/xclip/xsel; always also OSC 52 for copy.
//! Image paste: arboard image → PNG, else wl-paste / xclip.

use std::process::{Command, Stdio};

use base64::Engine;
use ref_proto::ImageData;

#[cfg(not(test))]
fn osc52(text: &str) {
    let b64 = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    let seq = if std::env::var_os("TMUX").is_some() {
        format!("\x1bPtmux;\x1b\x1b]52;c;{b64}\x07\x1b\\")
    } else {
        format!("\x1b]52;c;{b64}\x07")
    };
    use std::io::Write;
    let mut out = std::io::stdout();
    let _ = out.write_all(seq.as_bytes());
    let _ = out.flush();
}

#[cfg(not(test))]
fn pipe_to(cmd: &str, args: &[&str], text: &str) -> bool {
    let Ok(mut child) = Command::new(cmd).args(args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(text.as_bytes());
    }
    // wl-copy forks and stays alive to serve the selection; don't block on it for long.
    for _ in 0..40 {
        match child.try_wait() {
            Ok(Some(s)) => return s.success(),
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(5)),
            Err(_) => return false,
        }
    }
    true
}

/// Copies text to the system clipboard. Returns true if a native clipboard accepted it
/// (OSC 52 is always emitted as well).
pub fn copy(text: &str) -> bool {
    #[cfg(test)]
    {
        LAST_COPY.with(|c| *c.borrow_mut() = Some(text.to_string()));
        true
    }
    #[cfg(not(test))]
    copy_native(text)
}

#[cfg(test)]
thread_local! {
    /// Last text passed to `copy` (tests only; nothing touches the real clipboard).
    pub static LAST_COPY: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Takes the last copied text (tests only).
#[cfg(test)]
pub fn take_last_copy() -> Option<String> {
    LAST_COPY.with(|c| c.borrow_mut().take())
}

#[cfg(not(test))]
fn copy_native(text: &str) -> bool {
    osc52(text);
    let mut ok = false;
    if let Ok(mut cb) = arboard::Clipboard::new() {
        ok = cb.set_text(text.to_string()).is_ok();
        // Keep the clipboard owner alive briefly on X11 so managers can grab the content.
        if ok {
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(500));
                drop(cb);
            });
        }
    }
    if !ok {
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
        ok = (wayland && pipe_to("wl-copy", &[], text))
            || pipe_to("xclip", &["-selection", "clipboard"], text)
            || pipe_to("xsel", &["--clipboard", "--input"], text);
    }
    ok
}

fn encode_png(w: usize, h: usize, rgba: &[u8]) -> Option<Vec<u8>> {
    if w == 0 || h == 0 || rgba.len() < w * h * 4 {
        return None;
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header().ok()?;
        wr.write_image_data(&rgba[..w * h * 4]).ok()?;
    }
    Some(out)
}

fn run_bytes(cmd: &str, args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new(cmd).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    (out.status.success() && !out.stdout.is_empty()).then_some(out.stdout)
}

/// Reads an image from the clipboard, as PNG.
pub fn paste_image() -> Option<ImageData> {
    let png = (|| {
        if let Ok(mut cb) = arboard::Clipboard::new()
            && let Ok(img) = cb.get_image()
        {
            return encode_png(img.width, img.height, &img.bytes);
        }
        None
    })()
    .or_else(|| {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            let types = run_bytes("wl-paste", &["--list-types"]).map(|b| String::from_utf8_lossy(&b).to_string()).unwrap_or_default();
            if types.lines().any(|t| t.trim() == "image/png") {
                return run_bytes("wl-paste", &["--no-newline", "--type", "image/png"]);
            }
            None
        } else {
            run_bytes("xclip", &["-selection", "clipboard", "-t", "image/png", "-o"])
        }
    })?;
    if !png.starts_with(&[0x89, b'P', b'N', b'G']) {
        return None;
    }
    Some(ImageData { mime: "image/png".into(), data: base64::engine::general_purpose::STANDARD.encode(png) })
}

/// Reads text from the clipboard.
pub fn paste_text() -> Option<String> {
    if let Ok(mut cb) = arboard::Clipboard::new()
        && let Ok(t) = cb.get_text()
    {
        return Some(t);
    }
    let b = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        run_bytes("wl-paste", &["--no-newline"])
    } else {
        run_bytes("xclip", &["-selection", "clipboard", "-o"]).or_else(|| run_bytes("xsel", &["--clipboard", "--output"]))
    }?;
    Some(String::from_utf8_lossy(&b).into_owned())
}

/// If `s` names an existing image file (quotes / file:// / escapes tolerated), loads it.
pub fn image_from_path(s: &str) -> Option<(ImageData, String)> {
    let mut p = s.trim().trim_matches(|c| c == '\'' || c == '"').to_string();
    if let Some(rest) = p.strip_prefix("file://") {
        p = percent_decode(rest);
    }
    p = p.replace("\\ ", " ");
    if p.contains('\n') || p.is_empty() || p.len() > 4096 {
        return None;
    }
    if let Some(rest) = p.strip_prefix("~/")
        && let Some(h) = std::env::var_os("HOME")
    {
        p = format!("{}/{rest}", h.to_string_lossy());
    }
    let lower = p.to_ascii_lowercase();
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
    let meta = std::fs::metadata(&p).ok()?;
    if !meta.is_file() || meta.len() > 20 * 1024 * 1024 {
        return None;
    }
    let bytes = std::fs::read(&p).ok()?;
    let name = std::path::Path::new(&p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Some((ImageData { mime: mime.into(), data: base64::engine::general_purpose::STANDARD.encode(bytes) }, name))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn png_roundtrip_header() {
        let png = super::encode_png(2, 1, &[255, 0, 0, 255, 0, 255, 0, 255]).unwrap();
        assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
        assert!(super::encode_png(0, 0, &[]).is_none());
    }

    #[test]
    fn decode() {
        assert_eq!(super::percent_decode("a%20b.png"), "a b.png");
        assert!(super::image_from_path("/nonexistent/x.png").is_none());
        assert!(super::image_from_path("hello").is_none());
    }
}
