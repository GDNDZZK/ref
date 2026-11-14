//! Clipboard: arboard first, then wl-copy/xclip/xsel; always also OSC 52 for copy.
//! Image paste: arboard image → PNG, else wl-paste / xclip.

use std::process::{Command, Stdio};

use base64::Engine;
use ref_proto::ImageData;

/// `pipe_to` waits at most `SPAWN_POLL_ATTEMPTS * SPAWN_POLL_INTERVAL` for the helper to exit.
#[cfg(not(test))]
const SPAWN_POLL_ATTEMPTS: u32 = 40;
#[cfg(not(test))]
const SPAWN_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(5);
/// How long the X11 clipboard owner stays alive after a copy so managers can grab the content.
#[cfg(not(test))]
const X11_OWNER_KEEP_ALIVE: std::time::Duration = std::time::Duration::from_millis(500);
const RGBA_BYTES_PER_PIXEL: usize = 4;
const MAX_PATH_LEN: usize = 4096;
const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
const PNG_SIGNATURE: [u8; 4] = [0x89, b'P', b'N', b'G'];
const MIME_PNG: &str = "image/png";

fn base64_encode(bytes: impl AsRef<[u8]>) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn is_wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
}

#[cfg(not(test))]
fn osc52(text: &str) {
    let b64 = base64_encode(text);
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
    let Ok(mut child) = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(text.as_bytes());
    }
    // wl-copy forks and stays alive to serve the selection; don't block on it for long.
    for _ in 0..SPAWN_POLL_ATTEMPTS {
        match child.try_wait() {
            Ok(Some(s)) => return s.success(),
            Ok(None) => std::thread::sleep(SPAWN_POLL_INTERVAL),
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
    static LAST_COPY: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Takes the last copied text (tests only).
#[cfg(test)]
pub(crate) fn take_last_copy() -> Option<String> {
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
                std::thread::sleep(X11_OWNER_KEEP_ALIVE);
                drop(cb);
            });
        }
    }
    if !ok {
        ok = (is_wayland() && pipe_to("wl-copy", &[], text))
            || pipe_to("xclip", &["-selection", "clipboard"], text)
            || pipe_to("xsel", &["--clipboard", "--input"], text);
    }
    ok
}

fn encode_png(w: usize, h: usize, rgba: &[u8]) -> Option<Vec<u8>> {
    if w == 0 || h == 0 || rgba.len() < w * h * RGBA_BYTES_PER_PIXEL {
        return None;
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header().ok()?;
        wr.write_image_data(&rgba[..w * h * RGBA_BYTES_PER_PIXEL])
            .ok()?;
    }
    Some(out)
}

fn run_bytes(cmd: &str, args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
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
        if is_wayland() {
            let types = run_bytes("wl-paste", &["--list-types"])
                .map(|b| String::from_utf8_lossy(&b).to_string())
                .unwrap_or_default();
            if types.lines().any(|t| t.trim() == MIME_PNG) {
                return run_bytes("wl-paste", &["--no-newline", "--type", MIME_PNG]);
            }
            None
        } else {
            run_bytes("xclip", &["-selection", "clipboard", "-t", MIME_PNG, "-o"])
        }
    })?;
    if !png.starts_with(&PNG_SIGNATURE) {
        return None;
    }
    Some(ImageData {
        mime: MIME_PNG.into(),
        data: base64_encode(png),
        ..Default::default()
    })
}

/// Reads text from the clipboard.
pub fn paste_text() -> Option<String> {
    if let Ok(mut cb) = arboard::Clipboard::new()
        && let Ok(t) = cb.get_text()
    {
        return Some(t);
    }
    let b = if is_wayland() {
        run_bytes("wl-paste", &["--no-newline"])
    } else {
        run_bytes("xclip", &["-selection", "clipboard", "-o"])
            .or_else(|| run_bytes("xsel", &["--clipboard", "--output"]))
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
    if p.contains('\n') || p.is_empty() || p.len() > MAX_PATH_LEN {
        return None;
    }
    if let Some(rest) = p.strip_prefix("~/")
        && let Some(h) = std::env::var_os("HOME")
            .filter(|h| !h.is_empty())
            .map(std::path::PathBuf::from)
            .or_else(dirs::home_dir)
    {
        p = h.join(rest).to_string_lossy().into_owned();
    }
    let lower = p.to_ascii_lowercase();
    let mime = if lower.ends_with(".png") {
        MIME_PNG
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
    if !meta.is_file() || meta.len() > MAX_IMAGE_BYTES {
        return None;
    }
    let bytes = std::fs::read(&p).ok()?;
    let name = std::path::Path::new(&p)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Some((
        ImageData {
            mime: mime.into(),
            data: base64_encode(bytes),
            ..Default::default()
        },
        name,
    ))
}

/// Decodes `%XX` escapes; malformed ones are kept as they are.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if let [b'%', hi, lo, ..] = b[i..]
            && let (Some(hi), Some(lo)) = (hex_value(hi), hex_value(lo))
        {
            out.push(hi << 4 | lo);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(digit: u8) -> Option<u8> {
    char::from(digit).to_digit(16).map(|v| v as u8)
}

#[cfg(test)]
mod tests {
    #[test]
    fn png_roundtrip_header() {
        let png = super::encode_png(2, 1, &[255, 0, 0, 255, 0, 255, 0, 255]).unwrap();
        assert!(png.starts_with(&super::PNG_SIGNATURE));
        assert!(super::encode_png(0, 0, &[]).is_none());
    }

    #[test]
    fn decode() {
        assert_eq!(super::percent_decode("a%20b.png"), "a b.png");
        assert!(super::image_from_path("/nonexistent/x.png").is_none());
        assert!(super::image_from_path("hello").is_none());
    }

    #[test]
    fn decode_keeps_malformed_escapes_before_multibyte_chars() {
        // The two bytes after '%' end inside a multi-byte char.
        assert_eq!(super::percent_decode("%aé"), "%aé");
        assert_eq!(super::percent_decode("%éa"), "%éa");
        assert_eq!(super::percent_decode("%é"), "%é");
        assert_eq!(super::percent_decode("x%日本"), "x%日本");
    }

    #[test]
    fn decode_accepts_only_hex_digits() {
        // `from_str_radix` would take a sign here.
        assert_eq!(super::percent_decode("%+f"), "%+f");
        assert_eq!(super::percent_decode("%4"), "%4");
        assert_eq!(super::percent_decode("%41"), "A");
        assert_eq!(super::percent_decode("%e6%97%a5"), "日");
    }
}
