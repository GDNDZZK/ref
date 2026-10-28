//! `edit` fallback: exact `old_string` → `new_string` replacement.

use serde_json::Value;

use crate::read::{load_text, not_found};
use crate::util::{canonical_key, display_path, opt_bool, req_str, resolve_path};
use crate::{ToolCtx, ToolOutput};

pub fn run(input: &Value, ctx: &ToolCtx) -> Result<ToolOutput, String> {
    let raw = req_str(input, "path")?;
    let old = req_str(input, "old_string")?;
    let new = req_str(input, "new_string")?;
    let replace_all = opt_bool(input, "replace_all")?.unwrap_or(false);
    let path = resolve_path(&ctx.cwd, raw);
    let shown = display_path(&ctx.cwd, &path);
    if !path.exists() {
        return Err(format!(
            "{}\n`edit` only changes existing files; use `write` to create a new file.",
            not_found(&ctx.cwd, &path, &shown)
        ));
    }
    let old = crate::util::normalize_eol(old);
    let new = crate::util::normalize_eol(new);
    if old.is_empty() {
        return Err("`old_string` must not be empty; use `write` to replace a whole file.".into());
    }
    if old == new {
        return Err("`old_string` and `new_string` are identical; nothing to change.".into());
    }
    let file = load_text(&path, &shown)?;
    let text = &file.text;
    let mut warning = None;
    let new_text = {
        let count = text.matches(old.as_str()).count();
        if count == 1 || (count > 1 && replace_all) {
            if replace_all {
                text.replace(old.as_str(), &new)
            } else {
                text.replacen(old.as_str(), &new, 1)
            }
        } else if count > 1 {
            return Err(format!(
                "`old_string` matches {count} times in {shown}; add surrounding context to make it unique, or set `replace_all: true` to replace every occurrence."
            ));
        } else {
            match fuzzy_spans(text, &old) {
                spans if spans.is_empty() => {
                    return Err(format!(
                        "`old_string` was not found in {shown} (also tried ignoring trailing whitespace and indentation). Re-read the file and copy the exact text."
                    ))
                }
                spans if spans.len() > 1 && !replace_all => {
                    return Err(format!(
                        "`old_string` matches {} times in {shown} when ignoring whitespace differences; add surrounding context to make it unique, or set `replace_all: true`.",
                        spans.len()
                    ))
                }
                spans => {
                    warning = Some(format!(
                        "No exact match; matched {} location(s) ignoring trailing whitespace and indentation, re-indenting `new_string` to fit.",
                        spans.len()
                    ));
                    let mut out = String::with_capacity(text.len());
                    let mut last = 0;
                    for (s, e, indent) in spans {
                        out.push_str(&text[last..s]);
                        out.push_str(&reindent(&old, &new, &indent));
                        last = e;
                    }
                    out.push_str(&text[last..]);
                    out
                }
            }
        }
    };
    std::fs::write(&path, file.encode(&new_text)).map_err(|e| format!("Failed to write {shown}: {e}"))?;
    let key = canonical_key(&path);
    let mut out = super::finish_file(ctx, &key, &shown, text, &new_text);
    if let Some(w) = warning {
        out.push_str("\n\nWarnings:\n");
        out.push_str(&w);
    }
    Ok(ToolOutput::ok(out))
}

fn leading_ws(s: &str) -> &str {
    &s[..s.len() - s.trim_start_matches([' ', '\t']).len()]
}

/// Minimal indentation of non-blank lines.
fn min_indent(lines: &[&str]) -> String {
    lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| leading_ws(l))
        .min_by_key(|w| w.len())
        .unwrap_or("")
        .to_string()
}

/// Byte spans of whole-line matches of `old` in `text` with trailing whitespace ignored and
/// leading indentation normalized (relative indentation must match). Returns (start, end, base indent).
fn fuzzy_spans(text: &str, old: &str) -> Vec<(usize, usize, String)> {
    let old_lines: Vec<&str> = old.trim_matches('\n').split('\n').collect();
    if old_lines.iter().all(|l| l.trim().is_empty()) {
        return vec![];
    }
    let old_base = min_indent(&old_lines);
    let key = |l: &str, base: &str| -> String {
        let l = l.trim_end();
        l.strip_prefix(base).unwrap_or(l.trim_start()).to_string()
    };
    let old_keys: Vec<String> = old_lines.iter().map(|l| key(l, &old_base)).collect();
    // Line start offsets.
    let mut starts = vec![0usize];
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let n = old_lines.len();
    let mut out = vec![];
    let mut i = 0;
    while i + n <= lines.len() {
        let window = &lines[i..i + n];
        let base = min_indent(window);
        let ok = window.iter().zip(&old_keys).all(|(l, k)| {
            if k.is_empty() {
                l.trim().is_empty()
            } else {
                key(l, &base) == *k
            }
        });
        if ok {
            let s = starts[i];
            let last = i + n - 1;
            let e = starts[last] + lines[last].len();
            out.push((s, e, base));
            i += n;
        } else {
            i += 1;
        }
    }
    out
}

/// Re-indent `new` from the indentation base of `old` to `indent`.
fn reindent(old: &str, new: &str, indent: &str) -> String {
    let old_lines: Vec<&str> = old.trim_matches('\n').split('\n').collect();
    let old_base = min_indent(&old_lines);
    let new = new.trim_matches('\n');
    new.split('\n')
        .map(|l| {
            if l.trim().is_empty() {
                String::new()
            } else if let Some(rest) = l.strip_prefix(old_base.as_str()) {
                format!("{indent}{rest}")
            } else {
                format!("{indent}{}", l.trim_start())
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy() {
        let text = "fn a() {\n    let x = 1;  \n    let y = 2;\n}\n";
        let spans = fuzzy_spans(text, "let x = 1;\nlet y = 2;");
        assert_eq!(spans.len(), 1);
        let (s, e, ind) = &spans[0];
        assert_eq!(ind, "    ");
        let out = format!("{}{}{}", &text[..*s], reindent("let x = 1;\nlet y = 2;", "let z = 3;", ind), &text[*e..]);
        assert_eq!(out, "fn a() {\n    let z = 3;\n}\n");
    }
}
