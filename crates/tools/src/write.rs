use std::sync::LazyLock;

use regex::Regex;

use crate::hashline::parse::{has_display_prefix, strip_display_prefix};
use crate::hashline::{header, Origin, Seen, TextFile};
use crate::util::{self, canonical_key, display_path, resolve_path, Args};
use crate::*;

pub struct WriteTool;

const DESCRIPTION: &str = "Create a new file or overwrite one entirely (parent directories are created). Prefer `edit` for changes to existing files. Returns the new `[PATH#TAG]` for follow-up edits. Write raw content only — no `N:` line prefixes.";

static HEADER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*\[[^#\r\n]+#[0-9A-Fa-f]{4}\]\s*$").unwrap());

/// If content looks like copied hashline read output (every content row `N:`/`N|` prefixed,
/// optional `[x#TAG]` header), strip the display prefixes. Returns (content, stripped).
pub fn strip_copied_prefixes(content: &str) -> (String, bool) {
    let lines: Vec<&str> = content.split('\n').collect();
    let mut non_empty = 0;
    let mut prefixed = 0;
    let mut headers = 0;
    for l in &lines {
        let l = l.strip_suffix('\r').unwrap_or(l);
        if l.trim().is_empty() {
            continue;
        }
        if HEADER_RE.is_match(l) {
            headers += 1;
            continue;
        }
        non_empty += 1;
        if has_display_prefix(l) {
            prefixed += 1;
        }
    }
    // Require ≥2 rows (or a header) so a lone "1: item" line is kept literal.
    if non_empty == 0 || prefixed != non_empty || (non_empty < 2 && headers == 0) {
        return (content.to_string(), false);
    }
    // Numbers must be increasing, as in real read output.
    let nums: Vec<u64> = lines
        .iter()
        .filter(|l| !l.trim().is_empty() && !HEADER_RE.is_match(l))
        .filter_map(|l| l.trim_start().trim_start_matches(['*', '+', '-', ' ']).split([':', '|']).next()?.parse().ok())
        .collect();
    if nums.windows(2).any(|w| w[1] <= w[0]) {
        return (content.to_string(), false);
    }
    let out: Vec<String> = lines
        .iter()
        .filter(|l| !HEADER_RE.is_match(l))
        .map(|l| if l.trim().is_empty() { l.to_string() } else { strip_display_prefix(l) })
        .collect();
    (out.join("\n"), true)
}

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &str {
        "write"
    }

    fn description(&self) -> String {
        DESCRIPTION.to_string()
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path (relative to cwd, `~` ok)." },
                "content": { "type": "string", "description": "Full file content." }
            },
            "required": ["path", "content"],
            "additionalProperties": false
        })
    }

    fn permission_subject(&self, input: &Value) -> String {
        util::expand_tilde(Args::lenient(input).s("path")).to_string_lossy().to_string()
    }

    fn title(&self, input: &Value) -> String {
        Args::lenient(input).s("path").to_string()
    }

    async fn run(&self, input: Value, ctx: &ToolCtx) -> anyhow::Result<ToolOutput> {
        let args = Args::new(&input)?;
        let raw_path = args.str_req("path")?;
        let content = match input.get("content") {
            Some(Value::String(s)) => s.as_str(),
            None | Some(Value::Null) => anyhow::bail!("missing required field \"content\""),
            Some(_) => anyhow::bail!("field \"content\" must be a string"),
        };
        let abs = resolve_path(&ctx.cwd, raw_path);
        let display = display_path(&ctx.cwd, &abs);
        if abs.is_dir() {
            return Ok(ToolOutput::error(format!("{display} is a directory.")).with_title(display));
        }
        let _lock = util::lock_files(std::slice::from_ref(&abs)).await;
        let (content, stripped) = strip_copied_prefixes(content);
        // Keep an existing file's BOM / CRLF style when the model wrote plain LF.
        let existing = std::fs::read(&abs).ok().and_then(|b| TextFile::decode(&b));
        let incoming = TextFile::from_text(&content);
        let encoded = match &existing {
            Some(f) if !content.contains('\r') && !incoming.bom => f.encode(&incoming.text),
            _ => content.clone(),
        };
        if let Some(parent) = abs.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                return Ok(ToolOutput::error(format!("Cannot create directory {}: {e}", parent.display())).with_title(display));
            }
        }
        if let Err(e) = tokio::fs::write(&abs, encoded.as_bytes()).await {
            return Ok(ToolOutput::error(format!("Failed to write {display}: {e}")).with_title(display));
        }
        let key = canonical_key(&abs);
        let tag = ctx.store.record(&key, &incoming.text, Seen::All, Origin::Written);
        let n_lines = crate::hashline::split_lines(&incoming.text).len();
        let verb = if existing.is_some() { "Overwrote" } else { "Wrote" };
        let mut text = format!("{}\n{verb} {} bytes / {n_lines} lines.", header(&display, &tag), encoded.len());
        if stripped {
            text.push_str("\nNote: auto-stripped hashline display prefixes (`N:`) from content before writing.");
        }
        Ok(ToolOutput::text(text).with_title(display))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ctx, run_tool};
    use serde_json::json;

    #[test]
    fn prefix_stripping() {
        let (s, did) = strip_copied_prefixes("[a.rs#ABCD]\n1:fn main() {\n2:    x();\n3:}\n");
        assert!(did);
        assert_eq!(s, "fn main() {\n    x();\n}\n");
        let (s, did) = strip_copied_prefixes("1: first\n");
        assert!(!did);
        assert_eq!(s, "1: first\n");
        let (_, did) = strip_copied_prefixes("a\n1:b\n");
        assert!(!did);
        let (_, did) = strip_copied_prefixes("3:x\n1:y\n");
        assert!(!did);
    }

    #[tokio::test]
    async fn write_creates_and_tags() {
        let (dir, c) = ctx();
        let out = run_tool(&WriteTool, json!({"path": "sub/dir/a.txt", "content": "hi\nthere\n"}), &c).await;
        assert!(!out.is_error, "{}", out.text);
        let tag = crate::hashline::file_tag("hi\nthere\n");
        assert_eq!(out.text, format!("[sub/dir/a.txt#{tag}]\nWrote 9 bytes / 2 lines."));
        assert_eq!(std::fs::read_to_string(dir.path().join("sub/dir/a.txt")).unwrap(), "hi\nthere\n");
        // Write → edit without reading works (whole content is known).
        let e = run_tool(&crate::edit::EditTool, json!({"input": format!("[sub/dir/a.txt#{tag}]\nPUT 2.=2:\n+world\n")}), &c).await;
        assert!(!e.is_error, "{}", e.text);
        let out = run_tool(&WriteTool, json!({"path": "b.txt", "content": "[b.txt#ABCD]\n1:x\n2:y"}), &c).await;
        assert!(out.text.contains("auto-stripped"));
        assert_eq!(std::fs::read_to_string(dir.path().join("b.txt")).unwrap(), "x\ny");
        let out = run_tool(&WriteTool, json!({"path": "c.txt"}), &c).await;
        assert!(out.is_error && out.text.contains("missing required field \"content\""));
    }

    #[tokio::test]
    async fn overwrite_keeps_crlf() {
        let (dir, c) = ctx();
        std::fs::write(dir.path().join("w.txt"), "a\r\nb\r\n").unwrap();
        let out = run_tool(&WriteTool, json!({"path": "w.txt", "content": "x\ny\n"}), &c).await;
        assert!(out.text.contains("Overwrote"));
        assert_eq!(std::fs::read_to_string(dir.path().join("w.txt")).unwrap(), "x\r\ny\r\n");
    }
}
