use base64::Engine as _;

use crate::hashline::{Origin, Seen};
use crate::hashline::{header, split_lines, TextFile, MAX_COLUMNS, MAX_SNAPSHOT_BYTES};
use crate::util::{self, canonical_key, display_path, human_size, image_mime, resolve_path, Args};
use crate::*;

pub struct ReadTool;

pub const DEFAULT_LIMIT: usize = 1000;
pub const MAX_OUTPUT_BYTES: usize = 50 * 1024;
const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
const MAX_TEXT_BYTES: u64 = 256 * 1024 * 1024;
const DIR_CHILD_CAP: usize = 12;
const DIR_ROOT_CAP: usize = 500;
const NO_EXPAND: &[&str] = &[".git", "node_modules", "target", "__pycache__", ".venv", "venv", ".hg", ".svn"];

const DESCRIPTION: &str = "Read a file or list a directory. Text output: a `[PATH#TAG]` header, then `N:text` rows (1-indexed). Copy `[PATH#TAG]` into `edit` sections and use the `N` numbers as anchors; never invent a tag. `offset`/`limit` page through long files (default 1000 lines; also accepts `path:50-120`). Lines over 768 chars are cut with `…`. Images are returned inline when supported.";

/// (path, offset, limit, tail-count)
type Selector<'a> = (&'a str, Option<usize>, Option<usize>, Option<usize>);

/// Parse an optional `:N`, `:N-M`, `:N-`, `:N+K`, `:-K` suffix.
fn split_selector(raw: &str) -> Option<Selector<'_>> {
    let (path, sel) = raw.rsplit_once(':')?;
    if path.is_empty() || sel.is_empty() {
        return None;
    }
    let num = |s: &str| -> Option<usize> { (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok()).flatten() };
    if let Some(k) = sel.strip_prefix('-') {
        return Some((path, None, None, Some(num(k)?)));
    }
    if let Some((a, b)) = sel.split_once('+') {
        return Some((path, Some(num(a)?), Some(num(b)?), None));
    }
    if let Some((a, b)) = sel.split_once('-') {
        let start = num(a)?;
        if b.is_empty() {
            return Some((path, Some(start), None, None));
        }
        let end = num(b)?;
        return Some((path, Some(start), Some(end.saturating_sub(start) + 1), None));
    }
    Some((path, Some(num(sel)?), None, None))
}

fn dir_listing(cwd: &std::path::Path, abs: &std::path::Path) -> anyhow::Result<String> {
    fn entries(dir: &std::path::Path) -> Vec<(String, bool, u64)> {
        let mut v: Vec<(String, bool, u64)> = std::fs::read_dir(dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| {
                        let name = e.file_name().to_string_lossy().to_string();
                        let meta = std::fs::metadata(e.path()).ok();
                        let is_dir = meta.as_ref().is_some_and(|m| m.is_dir());
                        (name, is_dir, meta.map(|m| m.len()).unwrap_or(0))
                    })
                    .collect()
            })
            .unwrap_or_default();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        v
    }
    let root = entries(abs);
    let mut out = vec![format!("{}/", display_path(cwd, abs).trim_end_matches('/'))];
    if root.is_empty() {
        out.push("(empty directory)".into());
        return Ok(out.join("\n"));
    }
    for (name, is_dir, size) in root.iter().take(DIR_ROOT_CAP) {
        if *is_dir {
            out.push(format!("{name}/"));
            if NO_EXPAND.contains(&name.as_str()) {
                continue;
            }
            let kids = entries(&abs.join(name));
            for (k, kd, ks) in kids.iter().take(DIR_CHILD_CAP) {
                out.push(if *kd { format!("  {k}/") } else { format!("  {k} ({})", human_size(*ks)) });
            }
            if kids.len() > DIR_CHILD_CAP {
                out.push(format!("  … {} more", kids.len() - DIR_CHILD_CAP));
            }
        } else {
            out.push(format!("{name} ({})", human_size(*size)));
        }
    }
    if root.len() > DIR_ROOT_CAP {
        out.push(format!("… {} more", root.len() - DIR_ROOT_CAP));
    }
    Ok(out.join("\n"))
}

/// Format a window of `lines` as hashline rows. Returns (body, shown_full, last_line_shown).
pub(crate) fn format_window(lines: &[&str], offset: usize, limit: usize, budget: usize) -> (Vec<String>, Vec<u32>, usize) {
    let mut rows = Vec::new();
    let mut shown = Vec::new();
    let mut bytes = 0;
    let mut last = offset.saturating_sub(1);
    for (i, line) in lines.iter().enumerate().skip(offset - 1).take(limit) {
        let n = i + 1;
        let (text, cut) = util::truncate_chars(line, MAX_COLUMNS);
        let row = format!("{n}:{text}");
        if bytes + row.len() + 1 > budget && !rows.is_empty() {
            break;
        }
        bytes += row.len() + 1;
        rows.push(row);
        if !cut {
            shown.push(n as u32);
        }
        last = n;
    }
    (rows, shown, last)
}

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> String {
        DESCRIPTION.to_string()
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File or directory path (relative to cwd, `~` ok). Optional `:50-120` suffix selects lines." },
                "offset": { "type": "integer", "minimum": 1, "description": "First line to show (1-indexed)." },
                "limit": { "type": "integer", "minimum": 1, "description": "Max lines to show (default 1000)." }
            },
            "required": ["path"],
            "additionalProperties": false
        })
    }

    fn permission_subject(&self, input: &Value) -> String {
        util::expand_tilde(Args::lenient(input).s("path")).to_string_lossy().to_string()
    }

    fn title(&self, input: &Value) -> String {
        let a = Args::lenient(input);
        let path = a.s("path");
        match (a.u64_opt("offset").ok().flatten(), a.u64_opt("limit").ok().flatten()) {
            (Some(o), Some(l)) => format!("{path}:{o}-{}", o + l.saturating_sub(1)),
            (Some(o), None) => format!("{path}:{o}-"),
            (None, Some(l)) => format!("{path}:1-{l}"),
            _ => path.to_string(),
        }
    }

    async fn run(&self, input: Value, ctx: &ToolCtx) -> anyhow::Result<ToolOutput> {
        let args = Args::new(&input)?;
        let raw = args.str_req("path")?;
        let mut offset = args.u64_opt("offset")?.map(|v| v as usize);
        let mut limit = args.u64_opt("limit")?.map(|v| v as usize);
        let mut tail = None;
        let mut abs = resolve_path(&ctx.cwd, raw);
        if !abs.exists() {
            if let Some((p, o, l, t)) = split_selector(raw.trim()) {
                let cand = resolve_path(&ctx.cwd, p);
                if cand.exists() {
                    abs = cand;
                    offset = offset.or(o);
                    limit = limit.or(l);
                    tail = t;
                }
            }
        }
        let display = display_path(&ctx.cwd, &abs);
        if offset == Some(0) {
            return Ok(ToolOutput::error("offset 0 is invalid; lines are 1-indexed. Use offset=1.").with_title(display));
        }
        if limit == Some(0) {
            return Ok(ToolOutput::error("limit must be >= 1.").with_title(display));
        }
        let meta = match std::fs::metadata(&abs) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ToolOutput::error(format!("File not found: {display}")).with_title(display));
            }
            Err(e) => return Ok(ToolOutput::error(format!("Cannot access {display}: {e}")).with_title(display)),
        };
        if meta.is_dir() {
            let a = abs.clone();
            let cwd = ctx.cwd.clone();
            let text = tokio::task::spawn_blocking(move || dir_listing(&cwd, &a)).await??;
            return Ok(ToolOutput::text(text).with_title(format!("{}/", display.trim_end_matches('/'))));
        }
        if let Some(mime) = image_mime(&abs) {
            if meta.len() > MAX_IMAGE_BYTES {
                return Ok(ToolOutput::error(format!("Image {display} is {}; the limit is 20MB.", human_size(meta.len()))).with_title(display));
            }
            if !ctx.image_input {
                return Ok(ToolOutput::text(format!("[Image file {display} ({mime}, {}). The current model cannot view images.]", human_size(meta.len()))).with_title(display));
            }
            let bytes = tokio::fs::read(&abs).await?;
            let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
            return Ok(ToolOutput {
                text: format!("[Image {display} ({mime}, {})]", human_size(meta.len())),
                images: vec![hn_proto::ImageData { mime: mime.to_string(), data }],
                title: display,
                is_error: false,
            });
        }
        if meta.len() > MAX_TEXT_BYTES {
            return Ok(ToolOutput::error(format!("{display} is {}; too large to read. Use `grep` or `bash` to inspect parts of it.", human_size(meta.len()))).with_title(display));
        }
        let bytes = tokio::fs::read(&abs).await?;
        let file = match (util::looks_binary(&bytes), TextFile::decode(&bytes)) {
            (false, Some(f)) => f,
            _ => {
                return Ok(ToolOutput::text(format!("[Cannot read binary file '{display}' ({}); not valid UTF-8 text.]", human_size(meta.len()))).with_title(display));
            }
        };
        let lines = split_lines(&file.text);
        let total = lines.len();
        let snapshot = meta.len() <= MAX_SNAPSHOT_BYTES;
        let key = canonical_key(&abs);
        let tag = file.tag();
        let head = if snapshot { header(&display, &tag) } else { format!("[{display}]") };

        if total == 0 {
            if snapshot {
                ctx.store.record(&key, &file.text, Seen::lines([0]), Origin::Display);
            }
            return Ok(ToolOutput::text(format!("{head}\nThe file is empty.")).with_title(display));
        }
        let start = match (offset, tail) {
            (_, Some(k)) => total.saturating_sub(k) + 1,
            (Some(o), _) => o,
            _ => 1,
        };
        if start > total {
            return Ok(ToolOutput::error(format!(
                "Line {start} is beyond end of file ({total} lines total). Use offset=1 to read from the start, or offset={total} to read the last line."
            ))
            .with_title(display));
        }
        let limit = limit.or(tail).unwrap_or(DEFAULT_LIMIT);
        let (rows, mut shown, last) = format_window(&lines, start, limit, MAX_OUTPUT_BYTES);
        let mut text = String::with_capacity(rows.iter().map(|r| r.len() + 1).sum::<usize>() + 128);
        text.push_str(&head);
        text.push('\n');
        text.push_str(&rows.join("\n"));
        if last < total {
            text.push_str(&format!("\n\n[{} more lines in file. Use offset={} to continue]", total - last, last + 1));
        }
        if !snapshot {
            text.push_str("\n\n[File exceeds 4MB: no edit tag was issued. Use `write` or `bash` to modify it.]");
        } else {
            shown.push(0);
            ctx.store.record(&key, &file.text, Seen::lines(shown), Origin::Display);
        }
        Ok(ToolOutput::text(text).with_title(format!("{display}:{start}-{last}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ctx, run_tool};
    use serde_json::json;

    #[test]
    fn selectors() {
        assert_eq!(split_selector("a.rs:50-120"), Some(("a.rs", Some(50), Some(71), None)));
        assert_eq!(split_selector("a.rs:50"), Some(("a.rs", Some(50), None, None)));
        assert_eq!(split_selector("a.rs:50-"), Some(("a.rs", Some(50), None, None)));
        assert_eq!(split_selector("a.rs:5+3"), Some(("a.rs", Some(5), Some(3), None)));
        assert_eq!(split_selector("a.rs:-7"), Some(("a.rs", None, None, Some(7))));
        assert_eq!(split_selector("a.rs:x"), None);
    }

    #[tokio::test]
    async fn read_formats_hashline() {
        let (dir, c) = ctx();
        std::fs::write(dir.path().join("f.txt"), "alpha\nbeta\n\ngamma\n").unwrap();
        let out = run_tool(&ReadTool, json!({"path": "f.txt"}), &c).await;
        let tag = crate::hashline::file_tag("alpha\nbeta\n\ngamma\n");
        assert_eq!(out.text, format!("[f.txt#{tag}]\n1:alpha\n2:beta\n3:\n4:gamma"));
        assert_eq!(out.title, "f.txt:1-4");
        let key = canonical_key(&dir.path().join("f.txt"));
        let snap = c.store.by_tag(&key, &tag).unwrap();
        assert!(snap.seen.contains(4) && !snap.seen.contains(5));
    }

    #[tokio::test]
    async fn read_paging_and_selector() {
        let (dir, c) = ctx();
        let body: String = (1..=30).map(|i| format!("l{i}\n")).collect();
        std::fs::write(dir.path().join("f.txt"), body).unwrap();
        let out = run_tool(&ReadTool, json!({"path": "f.txt", "offset": 5, "limit": 3}), &c).await;
        assert!(out.text.ends_with("5:l5\n6:l6\n7:l7\n\n[23 more lines in file. Use offset=8 to continue]"), "{}", out.text);
        let out = run_tool(&ReadTool, json!({"path": "f.txt:10-11"}), &c).await;
        assert!(out.text.ends_with("10:l10\n11:l11\n\n[19 more lines in file. Use offset=12 to continue]"), "{}", out.text);
        let out = run_tool(&ReadTool, json!({"path": "f.txt:-2"}), &c).await;
        assert!(out.text.ends_with("29:l29\n30:l30"), "{}", out.text);
        let out = run_tool(&ReadTool, json!({"path": "f.txt", "offset": 99}), &c).await;
        assert!(out.is_error && out.text.contains("beyond end of file (30 lines total)"));
        let out = run_tool(&ReadTool, json!({"path": "f.txt", "offset": 0}), &c).await;
        assert!(out.is_error);
    }

    #[tokio::test]
    async fn long_lines_cut_and_unseen() {
        let (dir, c) = ctx();
        let long = "x".repeat(1000);
        std::fs::write(dir.path().join("f.txt"), format!("a\n{long}\n")).unwrap();
        let out = run_tool(&ReadTool, json!({"path": "f.txt"}), &c).await;
        assert!(out.text.contains(&format!("2:{}…", "x".repeat(768))));
        let key = canonical_key(&dir.path().join("f.txt"));
        let snap = c.store.head(&key).unwrap();
        assert!(snap.seen.contains(1) && !snap.seen.contains(2));
    }

    #[tokio::test]
    async fn dirs_binary_images_missing() {
        let (dir, c) = ctx();
        std::fs::create_dir_all(dir.path().join("src/inner")).unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "fn main(){}\n").unwrap();
        std::fs::write(dir.path().join("bin.dat"), [0u8, 1, 2, 3]).unwrap();
        std::fs::write(dir.path().join("pic.png"), [137u8, 80, 78, 71]).unwrap();
        let out = run_tool(&ReadTool, json!({"path": "."}), &c).await;
        assert!(out.text.contains("src/\n  inner/\n  main.rs (12B)"), "{}", out.text);
        let out = run_tool(&ReadTool, json!({"path": "bin.dat"}), &c).await;
        assert!(out.text.contains("Cannot read binary file"));
        let out = run_tool(&ReadTool, json!({"path": "pic.png"}), &c).await;
        assert_eq!(out.images.len(), 1);
        assert_eq!(out.images[0].mime, "image/png");
        let out = run_tool(&ReadTool, json!({"path": "nope.txt"}), &c).await;
        assert!(out.is_error && out.text.contains("File not found"));
        let out = run_tool(&ReadTool, json!({}), &c).await;
        assert!(out.is_error && out.text.contains("missing required field \"path\""));
    }
}
