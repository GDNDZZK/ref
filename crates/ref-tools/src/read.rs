//! `read` tool: hashline-formatted file view, directory listing, images.

use std::path::{Path, PathBuf};

use base64::Engine as _;
use ref_proto::ImagePart;
use serde_json::Value;

use crate::store::Seen;
use crate::util::{
    canonical_key, cap_line, decode_text, display_path, edit_distance, opt_u64, req_str,
    resolve_path, split_lines, TextFile, MAX_OUTPUT_BYTES, MAX_OUTPUT_LINES,
};
use crate::{ToolCtx, ToolOutput};

pub const MAX_LINE_CHARS: usize = 2000;
const MAX_DIR_ENTRIES: usize = 500;
const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
/// Text files larger than this are refused (use grep / bash instead).
const MAX_READ_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Load a text file as normalized text. Error messages are model-facing.
pub fn load_text(path: &Path, shown: &str) -> Result<TextFile, String> {
    let meta = std::fs::metadata(path).map_err(|e| io_msg(&e, shown))?;
    if meta.is_dir() {
        return Err(format!("{shown} is a directory, not a file"));
    }
    if !meta.is_file() {
        return Err(format!(
            "{shown} is not a regular file (FIFO, socket or device); refusing to read it"
        ));
    }
    if meta.len() > MAX_READ_FILE_BYTES {
        return Err(format!(
            "{shown} is too large ({}); use `grep` or `bash` to inspect parts of it",
            crate::util::human_bytes(meta.len())
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| io_msg(&e, shown))?;
    let size = bytes.len() as u64;
    decode_text(bytes).map_err(|why| {
        format!(
            "Cannot read {shown} as text ({}): {why} (binary data or a non-UTF-8 encoding). Use `bash` (e.g. `file`, `xxd`, `iconv`) to inspect it.",
            crate::util::human_bytes(size)
        )
    })
}

pub fn io_msg(e: &std::io::Error, shown: &str) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => format!("File not found: {shown}"),
        std::io::ErrorKind::PermissionDenied => format!("Permission denied: {shown}"),
        _ => format!("Cannot access {shown}: {e}"),
    }
}

/// Up to 3 similar names from the parent directory of a missing path.
pub fn suggestions(cwd: &Path, path: &Path) -> Vec<String> {
    let Some(parent) = path.parent() else {
        return vec![];
    };
    let Some(name) = path.file_name().map(|n| n.to_string_lossy().to_lowercase()) else {
        return vec![];
    };
    let Ok(rd) = std::fs::read_dir(parent) else {
        return vec![];
    };
    let mut scored: Vec<(usize, String, PathBuf)> = vec![];
    for e in rd.flatten().take(10_000) {
        let cand = e.file_name().to_string_lossy().to_string();
        let lc = cand.to_lowercase();
        let d = edit_distance(&name, &lc);
        let threshold = (name.chars().count() / 3).max(2);
        let related =
            lc.contains(&name) || (name.len() >= 3 && name.contains(&lc) && lc.len() >= 3);
        let stem_eq = Path::new(&lc).file_stem() == Path::new(&name).file_stem();
        if d <= threshold || related || stem_eq {
            let score = if stem_eq { 0 } else { d };
            scored.push((score, cand, e.path()));
        }
    }
    scored.sort();
    scored
        .into_iter()
        .take(3)
        .map(|(_, _, p)| display_path(cwd, &p))
        .collect()
}

pub fn not_found(cwd: &Path, path: &Path, shown: &str) -> String {
    let sugg = suggestions(cwd, path);
    if sugg.is_empty() {
        format!("File not found: {shown}")
    } else {
        format!(
            "File not found: {shown}. Did you mean: {}?",
            sugg.join(", ")
        )
    }
}

/// Parse an omp-style `:A-B` / `:N` / `:N-` line selector suffix.
fn split_selector(raw: &str) -> Option<(&str, usize, Option<usize>)> {
    let (base, sel) = raw.rsplit_once(':')?;
    if base.is_empty() || sel.is_empty() {
        return None;
    }
    let num = |s: &str| -> Option<usize> {
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        s.parse().ok()
    };
    if let Some((a, b)) = sel.split_once('-') {
        let a = num(a)?;
        if b.is_empty() {
            return Some((base, a, None));
        }
        let b = num(b)?;
        if b < a {
            return None;
        }
        Some((base, a, Some(b - a + 1)))
    } else {
        // `:N` is open-ended from N (omp semantics).
        Some((base, num(sel)?, None))
    }
}

fn image_mime(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_string_lossy().to_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => return None,
    })
}

pub fn run(input: &Value, ctx: &ToolCtx) -> ToolOutput {
    match run_inner(input, ctx) {
        Ok(o) => o,
        Err(e) => ToolOutput::err(e),
    }
}

fn run_inner(input: &Value, ctx: &ToolCtx) -> Result<ToolOutput, String> {
    let raw = req_str(input, "path")?;
    if raw.trim().is_empty() {
        return Err("parameter `path` must not be empty".into());
    }
    let mut offset = opt_u64(input, "offset")?.map(|v| v as usize);
    let mut limit = opt_u64(input, "limit")?.map(|v| v as usize);
    let mut path = resolve_path(&ctx.cwd, raw);
    if !path.exists() {
        if let Some((base, start, count)) = split_selector(raw) {
            let p = resolve_path(&ctx.cwd, base);
            if p.exists() {
                path = p;
                offset = offset.or(Some(start));
                limit = limit.or(count);
            }
        }
    }
    let shown = display_path(&ctx.cwd, &path);
    let meta = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(not_found(&ctx.cwd, &path, &shown))
        }
        Err(e) => return Err(io_msg(&e, &shown)),
    };
    if meta.is_dir() {
        return Ok(ToolOutput::ok(list_dir(&path, &shown)?));
    }
    if let Some(mime) = image_mime(&path) {
        if meta.len() > MAX_IMAGE_BYTES {
            return Err(format!(
                "Image {shown} is too large ({}; max 20 MB)",
                crate::util::human_bytes(meta.len())
            ));
        }
        if !ctx.image_input {
            return Ok(ToolOutput::ok(format!(
                "[Image file {shown} ({mime}, {}); the current model does not accept image input]",
                crate::util::human_bytes(meta.len())
            )));
        }
        let bytes = std::fs::read(&path).map_err(|e| io_msg(&e, &shown))?;
        let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let mut out = ToolOutput::ok(format!(
            "[Image {shown} ({mime}, {})]",
            crate::util::human_bytes(meta.len())
        ));
        out.images.push(ImagePart {
            id: ref_proto::new_id("prt"),
            mime: mime.to_string(),
            data,
            name: path.file_name().map(|n| n.to_string_lossy().to_string()),
        });
        return Ok(out);
    }
    let file = load_text(&path, &shown)?;
    Ok(ToolOutput::ok(render(
        ctx, &path, &shown, &file.text, offset, limit,
    )))
}

/// Render `text` in hashline format, recording the snapshot and the displayed lines.
fn render(
    ctx: &ToolCtx,
    path: &Path,
    shown: &str,
    text: &str,
    offset: Option<usize>,
    limit: Option<usize>,
) -> String {
    let lines = split_lines(text);
    let total = lines.len();
    let key = canonical_key(path);
    let mut seen = Vec::new();
    let mut body = String::new();
    let mut footer = String::new();
    if total == 0 {
        body.push_str("(empty file)");
    } else {
        let start = offset.unwrap_or(1).max(1);
        let limit = limit.unwrap_or(MAX_OUTPUT_LINES).clamp(1, MAX_OUTPUT_LINES);
        if start > total {
            let tag = ctx.store.lock().record(&key, text, Seen::none());
            return format!(
                "[{shown}#{tag}]\nLine {start} is beyond end of file ({total} lines total). Use offset=1 to read from the start."
            );
        }
        let end = (start - 1 + limit).min(total); // inclusive, 1-based
        let mut truncated_cols = false;
        let mut cut_lines = vec![];
        let mut last = start - 1;
        for (i, line) in lines.iter().enumerate().take(end).skip(start - 1) {
            let n = i + 1;
            let (shown_line, cut) = cap_line(line, MAX_LINE_CHARS);
            let row = format!("{n}:{shown_line}\n");
            if body.len() + row.len() > MAX_OUTPUT_BYTES && n > start {
                break;
            }
            body.push_str(&row);
            if cut {
                truncated_cols = true;
                cut_lines.push(n);
            } else {
                seen.push(n);
            }
            last = n;
        }
        if body.ends_with('\n') {
            body.pop();
        }
        if truncated_cols {
            footer.push_str(&format!(
                "\n[Lines {} are longer than {MAX_LINE_CHARS} chars and were cut with `…`; change them with `edit` old_string/new_string instead of line numbers]",
                crate::util::format_line_list(&cut_lines)
            ));
        }
        if last < total {
            if last < end {
                footer.push_str(&format!(
                    "\n[Showing lines {start}-{last} of {total} (50 KB output limit). Use offset={} to continue]",
                    last + 1
                ));
            } else {
                footer.push_str(&format!(
                    "\n[{} more lines. Use offset={} to continue]",
                    total - last,
                    last + 1
                ));
            }
        }
    }
    let tag = ctx.store.lock().record(&key, text, Seen::from(seen));
    format!("[{shown}#{tag}]\n{body}{footer}")
}

fn list_dir(path: &Path, shown: &str) -> Result<String, String> {
    let rd = std::fs::read_dir(path).map_err(|e| io_msg(&e, shown))?;
    let mut entries: Vec<String> = vec![];
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name == ".git" {
            continue;
        }
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false)
            || (e.file_type().map(|t| t.is_symlink()).unwrap_or(false) && e.path().is_dir());
        entries.push(if is_dir { format!("{name}/") } else { name });
    }
    entries.sort();
    let total = entries.len();
    let header = if shown.ends_with('/') {
        shown.to_string()
    } else {
        format!("{shown}/")
    };
    if total == 0 {
        return Ok(format!("{header}\n(empty directory)"));
    }
    let mut out = header;
    for e in entries.iter().take(MAX_DIR_ENTRIES) {
        out.push('\n');
        out.push_str(e);
    }
    if total > MAX_DIR_ENTRIES {
        out.push_str(&format!(
            "\n[{} more entries not shown; use `find` with a pattern to narrow down]",
            total - MAX_DIR_ENTRIES
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector() {
        assert_eq!(split_selector("a.rs:3-5"), Some(("a.rs", 3, Some(3))));
        assert_eq!(split_selector("a.rs:7"), Some(("a.rs", 7, None)));
        assert_eq!(split_selector("a.rs:7-"), Some(("a.rs", 7, None)));
        assert_eq!(split_selector("a.rs:x"), None);
    }
}
