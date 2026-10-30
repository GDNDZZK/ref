//! `read`: hashline file view, directory listing, images.

use crate::hashline::{Seen, format_header};
use crate::util::{self, TextFile};
use crate::{ToolCtx, ToolOutput, ToolSpec};
use anyhow::{Result, bail};
use base64::Engine;
use ref_proto::ImageData;
use regex::Regex;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

pub const DEFAULT_LIMIT: usize = 500;
pub const MAX_LIMIT: usize = 3000;
/// Byte budget for one page of numbered rows.
pub const PAGE_BYTES: usize = 60 * 1024;
/// Display cap per line (chars); longer lines end with `…` and are not counted as seen.
pub const MAX_LINE_CHARS: usize = 1000;
pub const MAX_DIR_ENTRIES: usize = 500;
pub const MAX_IMAGE_BYTES: u64 = 10 * 1024 * 1024;
/// Larger files are refused (use grep / bash to inspect them).
pub const MAX_READ_BYTES: u64 = 64 * 1024 * 1024;

pub fn spec() -> ToolSpec {
    ToolSpec {
        name: "read",
        description: "Read a text file, list a directory, or view an image.\n\
\n\
Text files are returned in hashline format: a header `[path#TAG]` followed by rows `N:content` \
(N = 1-based line number, no padding; the `N:` prefix is not part of the file). TAG is a 4-hex \
snapshot id of the whole file; pass it to `edit` together with the line numbers shown.\n\
- Default page: 500 lines (max `limit` 3000, ~60KB). A trailing notice tells you the `offset` to \
continue from. Lines over 1000 chars are cut with `…` (those lines cannot be edited until seen whole).\n\
- `offset`/`limit` select a range; a path suffix also works: `file.rs:50-80`, `file.rs:50+10`, `file.rs:50`.\n\
- Directories list their entries (subdirectories end with `/`).\n\
- Images (png, jpg, gif, webp) are returned as images. Binary files are refused."
            .to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File or directory path (relative to the working directory, or absolute)."},
                "offset": {"type": "integer", "minimum": 1, "description": "1-based line to start from (default 1)."},
                "limit": {"type": "integer", "minimum": 1, "description": "Maximum number of lines to return (default 500, max 3000)."}
            },
            "required": ["path"]
        }),
    }
}

pub async fn run(input: Value, ctx: &ToolCtx) -> Result<ToolOutput> {
    let ctx = ctx.clone();
    util::blocking(move || run_sync(&input, &ctx)).await
}

static RANGE_SUFFIX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(.+?):(?:(\d+)(?:([-+])(\d*))?|-(\d+))$").expect("valid regex"));

/// Line selection derived from offset/limit or a path suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Range {
    /// Start line (1-based), optional max line count.
    From(usize, Option<usize>),
    /// Last N lines.
    Last(usize),
}

/// Splits `path:50-80`-style suffixes when the literal path does not exist.
fn split_range_suffix(cwd: &Path, raw: &str) -> Option<(PathBuf, Range)> {
    let caps = RANGE_SUFFIX_RE.captures(raw)?;
    let base = caps.get(1)?.as_str();
    let base_abs = util::resolve_path(cwd, base);
    if !base_abs.exists() {
        return None;
    }
    if let Some(last) = caps.get(5) {
        return Some((base_abs, Range::Last(last.as_str().parse().ok()?)));
    }
    let start: usize = caps.get(2)?.as_str().parse().ok()?;
    let start = start.max(1);
    let range = match (
        caps.get(3).map(|m| m.as_str()),
        caps.get(4).map(|m| m.as_str()),
    ) {
        (None, _) => Range::From(start, None),
        (Some("-"), Some("")) => Range::From(start, None),
        (Some("-"), Some(end)) => {
            let end: usize = end.parse().ok()?;
            if end < start {
                return None;
            }
            Range::From(start, Some(end - start + 1))
        }
        (Some(_), Some("")) => Range::From(start, None),
        (Some(_), Some(count)) => Range::From(start, Some(count.parse::<usize>().ok()?.max(1))),
        _ => return None,
    };
    Some((base_abs, range))
}

fn run_sync(input: &Value, ctx: &ToolCtx) -> Result<ToolOutput> {
    let raw = util::req_str(input, "path")?;
    let (raw, _) = util::unwrap_header(&raw);
    if raw.is_empty() {
        bail!("`path` must not be empty");
    }
    let offset = util::opt_u64(input, "offset")?.map(|v| v as usize);
    let limit = util::opt_u64(input, "limit")?.map(|v| v as usize);

    let mut abs = util::resolve_path(&ctx.cwd, &raw);
    let mut suffix_range = None;
    if !abs.exists()
        && let Some((base, range)) = split_range_suffix(&ctx.cwd, &raw)
    {
        abs = base;
        suffix_range = Some(range);
    }
    let display = util::display_path(&ctx.cwd, &abs);
    let meta = match std::fs::metadata(&abs) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ToolOutput::err(format!("File not found: {display}")));
        }
        Err(e) => bail!("cannot access {display}: {e}"),
    };
    if meta.is_dir() {
        return list_dir(&abs, &display);
    }
    // Explicit offset/limit win over a suffix range.
    let range = match (offset, limit, suffix_range) {
        (None, None, Some(r)) => r,
        (None, Some(l), Some(Range::From(s, _))) => Range::From(s, Some(l)),
        _ => Range::From(offset.unwrap_or(1).max(1), limit),
    };

    if meta.len() > MAX_READ_BYTES {
        return Ok(ToolOutput::err(format!(
            "File too large to read: {display} ({}). Use `grep` or `bash` (e.g. `sed -n '100,200p'`) to inspect parts of it.",
            util::human_bytes(meta.len())
        )));
    }
    let bytes = std::fs::read(&abs)?;
    if let Some(mime) = util::image_mime(&bytes) {
        if meta.len() > MAX_IMAGE_BYTES {
            return Ok(ToolOutput::err(format!(
                "Image file too large: {} exceeds {} limit.",
                util::human_bytes(meta.len()),
                util::human_bytes(MAX_IMAGE_BYTES)
            )));
        }
        let mut text = format!(
            "Read image file {display} [{mime}] ({})",
            util::human_bytes(meta.len())
        );
        let mut sent = (mime.to_string(), bytes);
        if ctx.resize_images {
            let fitted = match crate::imgfit::fit(&sent.1, &crate::imgfit::Limits::default()) {
                Ok(f) => f,
                Err(e) => {
                    return Ok(ToolOutput::err(format!(
                        "Image file {display} could not be decoded ({e}); it was not attached."
                    )));
                }
            };
            let note = fitted.note();
            if let Some(data) = fitted.data {
                text.push_str(&format!(
                    ", sent as {} {}x{} ({})",
                    fitted.mime,
                    fitted.size.0,
                    fitted.size.1,
                    util::human_bytes(data.len() as u64)
                ));
                sent = (fitted.mime.to_string(), data);
            }
            if let Some(n) = note {
                text.push('\n');
                text.push_str(&n);
            }
        }
        let data = base64::engine::general_purpose::STANDARD.encode(&sent.1);
        let mut out = ToolOutput::ok(text);
        out.images.push(ImageData::new(sent.0, data));
        out.meta = Some(json!({"path": abs.to_string_lossy(), "image": true}));
        return Ok(out);
    }
    let text = match (util::looks_binary(&bytes), String::from_utf8(bytes)) {
        (false, Ok(t)) => t,
        _ => {
            return Ok(ToolOutput::err(format!(
                "Cannot read binary file '{display}' ({}); not valid UTF-8 text. Use `bash` (e.g. `xxd`, `file`) to inspect it.",
                util::human_bytes(meta.len())
            )));
        }
    };
    let file = TextFile::decode(&text);
    let mut out = render_text(ctx, &abs, &display, &file.text, range);
    out.meta = Some(json!({"path": abs.to_string_lossy()}));
    Ok(out)
}

/// Renders a page of `text` in hashline format and records the snapshot.
fn render_text(ctx: &ToolCtx, abs: &Path, display: &str, text: &str, range: Range) -> ToolOutput {
    let (lines, _) = util::split_lines(text);
    let total = lines.len();
    let tag = crate::hashline::file_hash(text);
    let header = format_header(display, &tag);
    if total == 0 {
        ctx.store.record(abs, text, Seen::lines([]));
        return ToolOutput::ok(format!("{header}\n(empty file)"));
    }
    let (start, limit) = match range {
        Range::From(s, l) => (s, l),
        Range::Last(n) => (total.saturating_sub(n.max(1)) + 1, Some(n.max(1))),
    };
    if start > total {
        ctx.store.record(abs, text, Seen::lines([]));
        return ToolOutput::err(format!(
            "Line {start} is beyond end of file ({total} lines total). Use offset=1 to read from the start, or offset={total} to read the last line."
        ));
    }
    let limit = limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let end_wanted = (start - 1 + limit).min(total); // inclusive, 1-based

    let mut body = String::new();
    let mut seen = Vec::new();
    let mut cut_any = false;
    let mut last_shown = start - 1;
    let mut byte_limited = false;
    for n in start..=end_wanted {
        let (shown, cut) = util::truncate_chars(lines[n - 1], MAX_LINE_CHARS);
        let row = format!("{n}:{shown}\n");
        if !body.is_empty() && body.len() + row.len() > PAGE_BYTES {
            byte_limited = true;
            break;
        }
        body.push_str(&row);
        if cut {
            cut_any = true;
        } else {
            seen.push(n as u32);
        }
        last_shown = n;
    }
    if body.ends_with('\n') {
        body.pop();
    }
    ctx.store.record(abs, text, Seen::lines(seen));

    let mut out = format!("{header}\n{body}");
    let mut notices = Vec::new();
    if byte_limited {
        notices.push(format!(
            "[Showing lines {start}-{last_shown} of {total} ({}KB limit). Use offset={} to continue]",
            PAGE_BYTES / 1024,
            last_shown + 1
        ));
    } else if last_shown < total {
        notices.push(format!(
            "[{} more lines in file. Use offset={} to continue]",
            total - last_shown,
            last_shown + 1
        ));
    }
    if cut_any {
        notices.push(format!(
            "[Some lines were cut at {MAX_LINE_CHARS} chars (ending in `…`); they cannot be edited by line number. Use `bash` to view or change them.]"
        ));
    }
    if !notices.is_empty() {
        out.push_str("\n\n");
        out.push_str(&notices.join("\n"));
    }
    ToolOutput::ok(out)
}

fn list_dir(abs: &Path, display: &str) -> Result<ToolOutput> {
    let mut entries: Vec<String> = Vec::new();
    for e in std::fs::read_dir(abs)? {
        let Ok(e) = e else { continue };
        let name = e.file_name().to_string_lossy().into_owned();
        if name == ".git" {
            continue;
        }
        // Follow symlinks for the dir check so linked dirs show `/`.
        let is_dir = std::fs::metadata(e.path())
            .map(|m| m.is_dir())
            .unwrap_or(false);
        entries.push(if is_dir { format!("{name}/") } else { name });
    }
    entries.sort();
    let total = entries.len();
    let dir_label = if display.ends_with('/') {
        display.to_string()
    } else {
        format!("{display}/")
    };
    let mut out = format!("Directory {dir_label} ({total} entries)");
    if total == 0 {
        out.push_str("\n(empty directory)");
    }
    for e in entries.iter().take(MAX_DIR_ENTRIES) {
        out.push('\n');
        out.push_str(e);
    }
    if total > MAX_DIR_ENTRIES {
        out.push_str(&format!(
            "\n\n[{} more entries not shown. Use `find` with a pattern to narrow the listing]",
            total - MAX_DIR_ENTRIES
        ));
    }
    let mut o = ToolOutput::ok(out);
    o.meta = Some(json!({"path": abs.to_string_lossy(), "dir": true}));
    Ok(o)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_support::{ctx, temp_dir};

    async fn read(c: &ToolCtx, v: Value) -> ToolOutput {
        crate::execute("read", v, c).await
    }

    #[tokio::test]
    async fn basic_format_and_snapshot() {
        let d = temp_dir("read-basic");
        std::fs::write(d.join("a.txt"), "hello\nworld  \n").unwrap();
        let c = ctx(&d);
        let o = read(&c, json!({"path": "a.txt"})).await;
        let tag = crate::hashline::file_hash("hello\nworld  \n");
        assert!(!o.is_error);
        assert_eq!(o.text, format!("[a.txt#{tag}]\n1:hello\n2:world  "));
        let snap = c.store.get(&d.join("a.txt"), &tag).unwrap();
        assert!(snap.seen.contains(1) && snap.seen.contains(2) && !snap.seen.contains(3));
    }

    #[tokio::test]
    async fn paging_and_suffix() {
        let d = temp_dir("read-page");
        let text: String = (1..=1000).map(|i| format!("line{i}\n")).collect();
        std::fs::write(d.join("big.txt"), &text).unwrap();
        let c = ctx(&d);
        let o = read(&c, json!({"path": "big.txt"})).await;
        assert!(
            o.text
                .contains("\n500:line500\n\n[500 more lines in file. Use offset=501 to continue]")
        );
        let o = read(&c, json!({"path": "big.txt", "offset": 998})).await;
        assert!(o.text.ends_with("998:line998\n999:line999\n1000:line1000"));
        let o = read(&c, json!({"path": "big.txt:50-52"})).await;
        assert!(o.text.ends_with("\n50:line50\n51:line51\n52:line52\n\n[948 more lines in file. Use offset=53 to continue]"), "{}", o.text);
        let o = read(&c, json!({"path": "big.txt:10+2"})).await;
        assert!(o.text.contains("\n10:line10\n11:line11\n\n["));
        let o = read(&c, json!({"path": "big.txt", "offset": 2000})).await;
        assert!(o.is_error && o.text.contains("beyond end of file (1000 lines total)"));
        let tag = crate::hashline::file_hash(&text);
        let snap = c.store.get(&d.join("big.txt"), &tag).unwrap();
        assert!(snap.seen.contains(51) && snap.seen.contains(998) && !snap.seen.contains(700));
    }

    #[tokio::test]
    async fn suffix_only_when_literal_missing() {
        let d = temp_dir("read-literal");
        std::fs::write(d.join("x:1-2"), "literal\n").unwrap();
        let c = ctx(&d);
        let o = read(&c, json!({"path": "x:1-2"})).await;
        assert!(o.text.contains("1:literal"));
    }

    #[tokio::test]
    async fn byte_budget_and_long_lines() {
        let d = temp_dir("read-bytes");
        let long = "x".repeat(1500);
        let text: String = (0..200).map(|_| format!("{long}\n")).collect();
        std::fs::write(d.join("w.txt"), &text).unwrap();
        let c = ctx(&d);
        let o = read(&c, json!({"path": "w.txt"})).await;
        assert!(o.text.contains("…"));
        assert!(o.text.contains("KB limit). Use offset="));
        assert!(o.text.len() < PAGE_BYTES + 1000);
        let snap = c.store.latest(&d.join("w.txt")).unwrap();
        assert!(!snap.seen.contains(1), "cut lines are not seen");
    }

    #[tokio::test]
    async fn crlf_bom_empty_binary_image_dir() {
        let d = temp_dir("read-misc");
        std::fs::write(d.join("c.txt"), "\u{FEFF}a \r\nb\r\n").unwrap();
        std::fs::write(d.join("e.txt"), "").unwrap();
        std::fs::write(d.join("b.bin"), [0u8, 1, 2, 3]).unwrap();
        std::fs::write(d.join("i.png"), crate::imgfit::test_png(400, 300, false)).unwrap();
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::write(d.join(".hidden"), "h").unwrap();
        let c = ctx(&d);
        let o = read(&c, json!({"path": "c.txt"})).await;
        assert_eq!(
            o.text,
            format!(
                "[c.txt#{}]\n1:a \n2:b",
                crate::hashline::file_hash("a \nb\n")
            )
        );
        let o = read(&c, json!({"path": "e.txt"})).await;
        assert_eq!(o.text, "[e.txt#5D05]\n(empty file)");
        let o = read(&c, json!({"path": "b.bin"})).await;
        assert!(o.is_error && o.text.contains("binary"));
        let o = read(&c, json!({"path": "i.png"})).await;
        assert_eq!(o.images.len(), 1);
        assert_eq!(o.images[0].mime, "image/png");
        assert!(
            o.text.starts_with("Read image file i.png [image/png] (")
                && !o.text.contains("sent as"),
            "{}",
            o.text
        );
        let o = read(&c, json!({"path": "."})).await;
        assert!(
            o.text
                .contains("\n.hidden\nb.bin\nc.txt\ne.txt\ni.png\nsub/"),
            "{}",
            o.text
        );
        assert!(!o.text.contains(".git"));
        let o = read(&c, json!({"path": "nope.txt"})).await;
        assert!(o.is_error && o.text.contains("not found"));
    }

    #[tokio::test]
    async fn images_are_downscaled_or_refused() {
        let d = temp_dir("read-img");
        std::fs::write(d.join("big.png"), crate::imgfit::test_png(1000, 3000, true)).unwrap();
        std::fs::write(d.join("bad.png"), b"\x89PNG\r\n\x1a\nrest").unwrap();
        let mut c = ctx(&d);
        let o = read(&c, json!({"path": "big.png"})).await;
        assert!(
            !o.is_error && o.images.len() == 1 && o.images[0].mime == "image/jpeg",
            "{}",
            o.text
        );
        assert!(o.text.contains(", sent as image/jpeg 523x1568 (") && o.text.contains("\n[Image: original 1000x3000, displayed at 523x1568. Multiply coordinates by 1.91"), "{}", o.text);
        let o = read(&c, json!({"path": "bad.png"})).await;
        assert!(
            o.is_error && o.images.is_empty() && o.text.contains("could not be decoded"),
            "{}",
            o.text
        );
        c.resize_images = false;
        let o = read(&c, json!({"path": "big.png"})).await;
        assert!(
            o.images[0].mime == "image/png" && !o.text.contains("sent as"),
            "{}",
            o.text
        );
    }
}
