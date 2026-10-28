//! `write` and `delete` tools.

use std::path::Path;

use serde_json::Value;

use crate::store::Seen;
use crate::util::{
    canonical_key, decode_text, display_path, home_dir, normalize_eol, normalize_lexical, opt_bool,
    req_str, resolve_path, split_lines, strip_numbered_prefixes, Eol, TextFile,
};
use crate::{ToolCtx, ToolOutput};

pub fn run_write(input: &Value, ctx: &ToolCtx) -> ToolOutput {
    match write_inner(input, ctx) {
        Ok(o) => o,
        Err(e) => ToolOutput::err(e),
    }
}

pub fn run_delete(input: &Value, ctx: &ToolCtx) -> ToolOutput {
    match delete_inner(input, ctx) {
        Ok(o) => o,
        Err(e) => ToolOutput::err(e),
    }
}

/// Unwrap a path given as `[path#TAG]`.
fn unwrap_header(raw: &str) -> &str {
    let t = raw.trim();
    if let Some(inner) = t.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        if let Some((p, tag)) = inner.rsplit_once('#') {
            if tag.len() == 4 && tag.bytes().all(|b| b.is_ascii_hexdigit()) {
                return p;
            }
        }
    }
    t
}

fn write_inner(input: &Value, ctx: &ToolCtx) -> Result<ToolOutput, String> {
    let raw = unwrap_header(req_str(input, "path")?);
    if raw.is_empty() {
        return Err("parameter `path` must not be empty".into());
    }
    let content = req_str(input, "content")?;
    let path = resolve_path(&ctx.cwd, raw);
    let shown = display_path(&ctx.cwd, &path);
    if path.is_dir() {
        return Err(format!("{shown} is a directory; give a file path"));
    }
    let existed = path.exists();
    // Keep the existing file's BOM / line ending convention.
    let mut file = TextFile { text: String::new(), bom: false, eol: Eol::Lf };
    if existed {
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(f) = decode_text(bytes) {
                file = f;
            }
        }
    } else if content.contains("\r\n") {
        file.eol = Eol::CrLf;
    }
    let mut text = normalize_eol(content.strip_prefix('\u{feff}').unwrap_or(content));
    if content.starts_with('\u{feff}') {
        file.bom = true;
    }
    let mut warnings = vec![];
    {
        let lines: Vec<&str> = text.split('\n').collect();
        let nonempty = lines.iter().filter(|l| !l.is_empty()).count();
        if nonempty >= 1 {
            if let Some(stripped) = strip_numbered_prefixes(&lines) {
                text = stripped.join("\n");
                warnings.push(
                    "Note: every line started with an `N:` line-number prefix copied from read output; the prefixes were stripped before writing.",
                );
            }
        }
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory {}: {e}", display_path(&ctx.cwd, parent)))?;
        }
    }
    let bytes = file.encode(&text);
    std::fs::write(&path, &bytes).map_err(|e| format!("Failed to write {shown}: {e}"))?;
    let key = canonical_key(&path);
    let tag = ctx.store.lock().record(&key, &text, Seen::All);
    let n = split_lines(&text).len();
    let verb = if existed { "Overwrote" } else { "Created" };
    let mut out = format!(
        "[{shown}#{tag}]\n{verb}: wrote {n} line{} ({} bytes) to {shown}",
        if n == 1 { "" } else { "s" },
        bytes.len()
    );
    for w in warnings {
        out.push('\n');
        out.push_str(w);
    }
    Ok(ToolOutput::ok(out))
}

fn delete_inner(input: &Value, ctx: &ToolCtx) -> Result<ToolOutput, String> {
    let raw = unwrap_header(req_str(input, "path")?);
    if raw.is_empty() {
        return Err("parameter `path` must not be empty".into());
    }
    let recursive = opt_bool(input, "recursive")?.unwrap_or(false);
    let path = resolve_path(&ctx.cwd, raw);
    let shown = display_path(&ctx.cwd, &path);
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(crate::read::not_found(&ctx.cwd, &path, &shown))
        }
        Err(e) => return Err(format!("Cannot access {shown}: {e}")),
    };
    let key = canonical_key(&path);
    if meta.is_dir() {
        if !recursive {
            return Err(format!(
                "{shown} is a directory; pass `recursive: true` to delete it and everything inside."
            ));
        }
        check_protected(&key, &ctx.cwd)?;
        let count = count_files(&path);
        std::fs::remove_dir_all(&path).map_err(|e| format!("Failed to delete {shown}: {e}"))?;
        ctx.store.lock().remove(&key);
        Ok(ToolOutput::ok(format!(
            "Deleted directory {shown} ({count} file{})",
            if count == 1 { "" } else { "s" }
        )))
    } else {
        std::fs::remove_file(&path).map_err(|e| format!("Failed to delete {shown}: {e}"))?;
        ctx.store.lock().remove(&key);
        Ok(ToolOutput::ok(format!("Deleted {shown}")))
    }
}

fn check_protected(target: &Path, cwd: &Path) -> Result<(), String> {
    let cwd_c = canonical_key(&normalize_lexical(cwd));
    if target.parent().is_none() {
        return Err("Refusing to delete the filesystem root.".into());
    }
    if cwd_c.starts_with(target) {
        return Err(format!(
            "Refusing to delete {}: it is the working directory or one of its ancestors.",
            target.display()
        ));
    }
    if let Some(h) = home_dir() {
        if canonical_key(&h) == target {
            return Err("Refusing to delete the home directory.".into());
        }
    }
    Ok(())
}

fn count_files(dir: &Path) -> usize {
    let mut n = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(e.path()),
                Ok(_) => n += 1,
                Err(_) => {}
            }
        }
    }
    n
}
