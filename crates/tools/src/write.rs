//! `write`: create or overwrite a file.

use crate::hashline::{Seen, format_header};
use crate::util::{self, TextFile};
use crate::{ToolCtx, ToolOutput, ToolSpec};
use anyhow::{Result, anyhow, bail};
use regex::Regex;
use serde_json::{Value, json};
use std::sync::LazyLock;

pub fn spec() -> ToolSpec {
    ToolSpec {
        name: "write",
        description: "Create a new file or overwrite an existing one with `content` (parent directories are created). \
Prefer `edit` for changing existing files; use `write` for new files or full rewrites.\n\
`content` is the raw file text (no `N:` prefixes). An existing file keeps its line-ending style (CRLF/LF) and BOM.\n\
Returns a `[path#TAG]` header; you can `edit` the written file directly with that TAG and the line numbers of your content."
            .to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File path (relative to the working directory, or absolute)."},
                "content": {"type": "string", "description": "Complete file content."}
            },
            "required": ["path", "content"]
        }),
    }
}

pub async fn run(input: Value, ctx: &ToolCtx) -> Result<ToolOutput> {
    let ctx = ctx.clone();
    util::blocking(move || run_sync(&input, &ctx)).await
}

static ROW_PREFIX_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d+):").expect("valid regex"));
static HEADER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*\[[^#\r\n]+#[0-9a-fA-F]{4}\]\s*$").expect("valid regex"));

/// If `content` is pasted read output (`[path#TAG]` header and/or rows `N:` counting up by
/// one from 1 on every line), returns the stripped text.
fn strip_read_output(content: &str) -> Option<String> {
    let (lines, trailing) = util::split_lines(content);
    let mut rows = lines.as_slice();
    let had_header = rows.first().is_some_and(|l| HEADER_RE.is_match(l));
    if had_header {
        rows = &rows[1..];
    }
    if rows.len() < 2 && !had_header {
        return None;
    }
    if rows.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(rows.len());
    for (i, r) in rows.iter().enumerate() {
        let caps = ROW_PREFIX_RE.captures(r)?;
        if caps[1].parse::<usize>().ok()? != i + 1 {
            return None;
        }
        out.push(&r[caps[0].len()..]);
    }
    Some(util::join_lines(&out, trailing))
}

fn run_sync(input: &Value, ctx: &ToolCtx) -> Result<ToolOutput> {
    let raw_path = util::req_str(input, "path")?;
    let (raw_path, _) = util::unwrap_header(&raw_path);
    if raw_path.is_empty() {
        bail!("`path` must not be empty");
    }
    let content = match input.get("content") {
        Some(Value::String(s)) => s.clone(),
        None | Some(Value::Null) => bail!("missing required parameter `content`"),
        Some(other) => other.to_string(),
    };
    let abs = util::resolve_path(&ctx.cwd, &raw_path);
    let display = util::display_path(&ctx.cwd, &abs);
    if abs.is_dir() {
        return Ok(ToolOutput::err(format!("{display} is a directory; cannot write a file there.")));
    }

    // Existing file: keep its conventions, diff against it.
    let old = match std::fs::read(&abs) {
        Ok(b) => Some(String::from_utf8_lossy(&b).into_owned()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => bail!("cannot read existing {display}: {e}"),
    };
    let old_file = old.as_deref().map(TextFile::decode);

    let mut notes = Vec::new();
    let mut new = TextFile::decode(&content);
    if let Some(stripped) = strip_read_output(&new.text) {
        new.text = stripped;
        notes.push("Note: stripped `N:` display prefixes (pasted read output) from content before writing.");
    }
    let encoding = match &old_file {
        // Keep the existing file's conventions unless the new content explicitly uses CRLF/BOM.
        Some(o) => TextFile { bom: o.bom || new.bom, crlf: o.crlf || new.crlf, text: String::new() },
        None => TextFile { bom: new.bom, crlf: new.crlf, text: String::new() },
    };
    let bytes = encoding.encode(&new.text);

    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent).map_err(|e| anyhow!("cannot create directory {}: {e}", parent.display()))?;
    }
    std::fs::write(&abs, &bytes).map_err(|e| anyhow!("failed to write {display}: {e}"))?;
    if new.text.starts_with("#!") && old.is_none() {
        make_executable(&abs);
    }

    let tag = ctx.store.record(&abs, &new.text, Seen::All);
    let line_count = util::split_lines(&new.text).0.len();
    let old_text = old_file.as_ref().map(|o| o.text.as_str()).unwrap_or("");
    let (diff, added, removed) = util::unified_diff(old_text, &new.text, &display);
    let verb = if old.is_some() { "Overwrote" } else { "Created" };
    let mut text = format!(
        "{}\n{verb} {display}: {line_count} lines ({} bytes).",
        format_header(&display, &tag),
        bytes.len()
    );
    for n in notes {
        text.push('\n');
        text.push_str(n);
    }
    let mut out = ToolOutput::ok(text);
    out.meta = Some(json!({
        "path": abs.to_string_lossy(),
        "diff": diff,
        "created": old.is_none(),
        "added": added,
        "removed": removed,
    }));
    Ok(out)
}

fn make_executable(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(m) = std::fs::metadata(path) {
        let mut p = m.permissions();
        p.set_mode(p.mode() | ((p.mode() & 0o444) >> 2));
        let _ = std::fs::set_permissions(path, p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_support::{ctx, temp_dir};

    #[tokio::test]
    async fn create_overwrite_and_conventions() {
        let d = temp_dir("write");
        let c = ctx(&d);
        let o = crate::execute("write", json!({"path": "sub/dir/a.txt", "content": "one\ntwo\n"}), &c).await;
        assert!(!o.is_error, "{}", o.text);
        let tag = crate::hashline::file_hash("one\ntwo\n");
        assert_eq!(o.text, format!("[sub/dir/a.txt#{tag}]\nCreated sub/dir/a.txt: 2 lines (8 bytes)."));
        assert_eq!(o.meta.as_ref().unwrap()["created"], json!(true));
        assert!(c.store.get(&d.join("sub/dir/a.txt"), &tag).unwrap().seen == Seen::All);

        std::fs::write(d.join("crlf.txt"), "\u{FEFF}x\r\ny\r\n").unwrap();
        let o = crate::execute("write", json!({"path": "crlf.txt", "content": "x\nz\n"}), &c).await;
        assert!(o.text.contains("Overwrote crlf.txt"));
        assert_eq!(std::fs::read_to_string(d.join("crlf.txt")).unwrap(), "\u{FEFF}x\r\nz\r\n");
        let m = o.meta.unwrap();
        assert_eq!(m["created"], json!(false));
        assert!(m["diff"].as_str().unwrap().contains("-y\n+z"));
    }

    #[tokio::test]
    async fn strips_pasted_read_output() {
        let d = temp_dir("write-strip");
        let c = ctx(&d);
        let o = crate::execute("write", json!({"path": "p.txt", "content": "[p.txt#ABCD]\n1:a\n2:b\n"}), &c).await;
        assert!(o.text.contains("stripped"));
        assert_eq!(std::fs::read_to_string(d.join("p.txt")).unwrap(), "a\nb\n");
        // Numbers that don't count from 1 are content.
        let o = crate::execute("write", json!({"path": "q.txt", "content": "10:00 start\n11:00 end\n"}), &c).await;
        assert!(!o.text.contains("stripped"));
        assert_eq!(std::fs::read_to_string(d.join("q.txt")).unwrap(), "10:00 start\n11:00 end\n");
    }
}
