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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyOutcome {
    /// Copied to the system clipboard (and OSC 52 was emitted if stdout succeeded).
    Native,
    /// Native clipboard backends failed; OSC 52 escape sequence was emitted.
    Osc52Only,
}

#[cfg(not(test))]
fn osc52(text: &str) -> std::io::Result<()> {
    let b64 = base64_encode(text);
    let seq = if std::env::var_os("TMUX").is_some() {
        format!("\x1bPtmux;\x1b\x1b]52;c;{b64}\x07\x1b\\")
    } else {
        format!("\x1b]52;c;{b64}\x07")
    };
    use std::io::Write;
    let mut out = std::io::stdout();
    out.write_all(seq.as_bytes())?;
    out.flush()
}

#[cfg(not(test))]
fn pipe_to(cmd: &str, args: &[&str], text: &str) -> Result<(), String> {
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{cmd}: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| format!("{cmd} stdin: {e}"))?;
    }
    // wl-copy forks and stays alive to serve the selection; don't block on it for long.
    for _ in 0..SPAWN_POLL_ATTEMPTS {
        match child.try_wait() {
            Ok(Some(s)) if s.success() => return Ok(()),
            Ok(Some(s)) => return Err(format!("{cmd} exited with {s}")),
            Ok(None) => std::thread::sleep(SPAWN_POLL_INTERVAL),
            Err(e) => return Err(format!("{cmd} wait: {e}")),
        }
    }
    Ok(())
}

/// Copies text to the system clipboard and emits OSC 52.
pub fn copy(text: &str) -> Result<CopyOutcome, String> {
    #[cfg(test)]
    {
        LAST_COPY.with(|c| *c.borrow_mut() = Some(text.to_string()));
        if let Some(res) = NEXT_COPY_RESULT.with(|r| r.borrow_mut().take()) {
            return res;
        }
        Ok(CopyOutcome::Native)
    }
    #[cfg(not(test))]
    copy_native(text)
}

#[cfg(test)]
thread_local! {
    /// Last text passed to `copy` (tests only; nothing touches the real clipboard).
    static LAST_COPY: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    /// Optional forced return value for the next `copy` call (tests only).
    static NEXT_COPY_RESULT: std::cell::RefCell<Option<Result<CopyOutcome, String>>> =
        const { std::cell::RefCell::new(None) };
}

/// Takes the last copied text (tests only).
#[cfg(test)]
pub(crate) fn take_last_copy() -> Option<String> {
    LAST_COPY.with(|c| c.borrow_mut().take())
}

/// Forces the outcome of the next `copy` call (tests only).
#[cfg(test)]
pub(crate) fn set_next_copy_result(res: Result<CopyOutcome, String>) {
    NEXT_COPY_RESULT.with(|r| *r.borrow_mut() = Some(res));
}

#[cfg(not(test))]
fn copy_native(text: &str) -> Result<CopyOutcome, String> {
    let osc = osc52(text);
    let mut errs = Vec::new();
    match arboard::Clipboard::new() {
        Ok(mut cb) => match cb.set_text(text.to_string()) {
            Ok(()) => {
                // Keep the clipboard owner alive briefly on X11 so managers can grab the content.
                std::thread::spawn(move || {
                    std::thread::sleep(X11_OWNER_KEEP_ALIVE);
                    drop(cb);
                });
                return Ok(CopyOutcome::Native);
            }
            Err(e) => errs.push(format!("arboard: {e}")),
        },
        Err(e) => errs.push(format!("arboard: {e}")),
    }
    let cmds: &[(&str, &[&str])] = if is_wayland() {
        &[
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
        ]
    } else {
        &[
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
        ]
    };
    for (cmd, args) in cmds {
        match pipe_to(cmd, args, text) {
            Ok(()) => return Ok(CopyOutcome::Native),
            Err(e) => errs.push(e),
        }
    }
    match osc {
        Ok(()) => Ok(CopyOutcome::Osc52Only),
        Err(e) => {
            errs.push(format!("osc52: {e}"));
            Err(errs.join("; "))
        }
    }
}

fn encode_png(w: usize, h: usize, rgba: &[u8]) -> Result<Vec<u8>, String> {
    if w == 0 || h == 0 {
        return Err("empty image dimensions".into());
    }
    let need = w
        .checked_mul(h)
        .and_then(|px| px.checked_mul(RGBA_BYTES_PER_PIXEL))
        .ok_or_else(|| "image dimensions overflow".to_string())?;
    if rgba.len() < need {
        return Err(format!("short RGBA buffer ({} < {need})", rgba.len()));
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header().map_err(|e| format!("png header: {e}"))?;
        wr.write_image_data(&rgba[..need])
            .map_err(|e| format!("png encode: {e}"))?;
    }
    Ok(out)
}

fn run_bytes(cmd: &str, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("{cmd}: {e}"))?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr);
        let msg = msg.trim();
        return Err(if msg.is_empty() {
            format!("{cmd} exited with {}", out.status)
        } else {
            format!("{cmd}: {msg}")
        });
    }
    Ok(out.stdout)
}

/// Reads an image from the clipboard, as PNG. Returns `Ok(None)` when no image is present.
pub fn paste_image() -> Result<Option<ImageData>, String> {
    let mut errs = Vec::new();
    let mut arboard_empty = false;
    match arboard::Clipboard::new() {
        Ok(mut cb) => match cb.get_image() {
            Ok(img) => {
                let png = encode_png(img.width, img.height, &img.bytes)?;
                return png_image_data(png).map(Some);
            }
            Err(arboard::Error::ContentNotAvailable) => arboard_empty = true,
            Err(e) => errs.push(format!("arboard: {e}")),
        },
        Err(e) => errs.push(format!("arboard: {e}")),
    }
    if is_wayland() {
        match run_bytes("wl-paste", &["--list-types"]) {
            Ok(raw) => {
                let types = String::from_utf8_lossy(&raw);
                if !types.lines().any(|t| t.trim() == MIME_PNG) {
                    return Ok(None);
                }
                let png = run_bytes("wl-paste", &["--no-newline", "--type", MIME_PNG])?;
                return png_image_data(png).map(Some);
            }
            Err(e) if e.contains("Nothing is copied") => return Ok(None),
            Err(e) => errs.push(e),
        }
    } else {
        match run_bytes("xclip", &["-selection", "clipboard", "-t", MIME_PNG, "-o"]) {
            Ok(png) if png.is_empty() => return Ok(None),
            Ok(png) => return png_image_data(png).map(Some),
            Err(e) if e.contains("not available") => return Ok(None),
            Err(e) => errs.push(e),
        }
    }
    if arboard_empty || errs.is_empty() {
        Ok(None)
    } else {
        Err(errs.join("; "))
    }
}

fn png_image_data(png: Vec<u8>) -> Result<ImageData, String> {
    if !png.starts_with(&PNG_SIGNATURE) {
        return Err("clipboard image is not valid PNG".into());
    }
    Ok(ImageData {
        mime: MIME_PNG.into(),
        data: base64_encode(png),
        ..Default::default()
    })
}

/// Reads text from the clipboard. Returns `Ok(None)` when the clipboard has no text.
pub fn paste_text() -> Result<Option<String>, String> {
    let mut errs = Vec::new();
    let mut arboard_empty = false;
    match arboard::Clipboard::new() {
        Ok(mut cb) => match cb.get_text() {
            Ok(t) => return Ok((!t.is_empty()).then_some(t)),
            Err(arboard::Error::ContentNotAvailable) => arboard_empty = true,
            Err(e) => errs.push(format!("arboard: {e}")),
        },
        Err(e) => errs.push(format!("arboard: {e}")),
    }
    let cmds: &[(&str, &[&str])] = if is_wayland() {
        &[("wl-paste", &["--no-newline"])]
    } else {
        &[
            ("xclip", &["-selection", "clipboard", "-o"]),
            ("xsel", &["--clipboard", "--output"]),
        ]
    };
    for (cmd, args) in cmds {
        match run_bytes(cmd, args) {
            Ok(b) if b.is_empty() => return Ok(None),
            Ok(b) => return Ok(Some(String::from_utf8_lossy(&b).into_owned())),
            Err(e) if e.contains("Nothing is copied") => return Ok(None),
            Err(e) => errs.push(e),
        }
    }
    if arboard_empty || errs.is_empty() {
        Ok(None)
    } else {
        Err(errs.join("; "))
    }
}

/// If `s` names an existing image file (quotes / file:// / escapes tolerated), loads it.
/// Returns `Ok(None)` when `s` is not an existing image path; returns `Err` when an image file
/// exists but cannot be read or exceeds the size limit.
pub fn image_from_path(s: &str) -> Result<Option<(ImageData, String)>, String> {
    let mut p = s.trim().trim_matches(|c| c == '\'' || c == '"').to_string();
    if let Some(rest) = p.strip_prefix("file://") {
        p = percent_decode(rest);
    }
    p = p.replace("\\ ", " ");
    if p.contains('\n') || p.is_empty() || p.len() > MAX_PATH_LEN {
        return Ok(None);
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
        return Ok(None);
    };
    let meta = match std::fs::metadata(&p) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("Cannot read image {p}: {e}")),
    };
    if !meta.is_file() {
        return Ok(None);
    }
    if meta.len() > MAX_IMAGE_BYTES {
        return Err(format!(
            "Image {p} is too large ({} bytes, max {MAX_IMAGE_BYTES})",
            meta.len()
        ));
    }
    let bytes = std::fs::read(&p).map_err(|e| format!("Cannot read image {p}: {e}"))?;
    let name = std::path::Path::new(&p)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(Some((
        ImageData {
            mime: mime.into(),
            data: base64_encode(bytes),
            ..Default::default()
        },
        name,
    )))
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
        assert!(super::encode_png(0, 0, &[]).is_err());
        assert!(super::encode_png(2, 1, &[0, 1, 2]).is_err());
    }

    #[test]
    fn run_bytes_propagates_errors() {
        let missing = super::run_bytes("nonexistent-ref-tui-clipboard-cmd", &[]);
        assert!(missing.is_err());
        let failed = super::run_bytes("sh", &["-c", "echo boom >&2; exit 2"]).unwrap_err();
        assert!(failed.contains("boom"), "{failed}");
        let ok = super::run_bytes("sh", &["-c", "printf ok"]).unwrap();
        assert_eq!(ok, b"ok");
    }

    #[test]
    fn decode() {
        assert_eq!(super::percent_decode("a%20b.png"), "a b.png");
        assert!(matches!(super::image_from_path("/nonexistent/x.png"), Ok(None)));
        assert!(matches!(super::image_from_path("hello"), Ok(None)));
    }

    #[test]
    fn image_from_path_reports_oversized_file() {
        let dir = std::env::temp_dir().join(format!("ref-tui-img-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("big.png");
        let f = std::fs::File::create(&path).unwrap();
        f.set_len(super::MAX_IMAGE_BYTES + 1).unwrap();
        drop(f);
        let err = super::image_from_path(path.to_str().unwrap()).unwrap_err();
        assert!(err.contains("too large"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
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
