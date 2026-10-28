//! Markdown files with YAML-ish frontmatter (agents and commands).
//!
//! Supported frontmatter subset: `key: value` scalars (quoted strings, booleans, numbers,
//! null, plain strings), flow lists `[a, b]`, block lists (`- item`), block scalars
//! (`|`, `|-`, `>`, `>-`) and nested maps by indentation (any depth), with quoted keys
//! such as `"git *": allow`.

use serde_json::{Map, Number, Value};
use std::path::{Path, PathBuf};

/// Split a markdown document into (frontmatter map, body). Body is trimmed.
/// Without a leading `---` line the whole text is the body.
pub fn split_frontmatter(text: &str) -> (Map<String, Value>, String) {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.lines();
    match lines.next() {
        Some(first) if first.trim_end() == "---" => {}
        _ => return (Map::new(), text.trim().to_string()),
    }
    let mut fm = Vec::new();
    let mut body = Vec::new();
    let mut closed = false;
    for l in lines {
        if !closed {
            if l.trim_end() == "---" {
                closed = true;
            } else {
                fm.push(l);
            }
        } else {
            body.push(l);
        }
    }
    if !closed {
        // Unterminated frontmatter: treat everything as body.
        return (Map::new(), text.trim().to_string());
    }
    (parse_frontmatter(&fm.join("\n")), body.join("\n").trim().to_string())
}

struct Line<'a> {
    indent: usize,
    text: &'a str,
    blank: bool,
}

/// Parse a frontmatter block into a JSON object.
pub fn parse_frontmatter(src: &str) -> Map<String, Value> {
    let lines: Vec<Line> = src
        .lines()
        .map(|raw| {
            let t = raw.trim_start();
            let blank = t.is_empty() || t.starts_with('#');
            Line { indent: raw.len() - t.len(), text: t.trim_end(), blank }
        })
        .collect();
    let mut i = 0;
    let start = lines.iter().find(|l| !l.blank).map(|l| l.indent).unwrap_or(0);
    let mut out = parse_map(&lines, &mut i, start);
    // Anything left over at lower indentation (malformed) is parsed as further top-level keys.
    while i < lines.len() {
        if lines[i].blank {
            i += 1;
            continue;
        }
        let ind = lines[i].indent;
        let before = i;
        out.extend(parse_map(&lines, &mut i, ind));
        if i == before {
            i += 1;
        }
    }
    out
}

fn skip_blank(lines: &[Line], i: &mut usize) {
    while *i < lines.len() && lines[*i].blank {
        *i += 1;
    }
}

fn parse_map(lines: &[Line], i: &mut usize, indent: usize) -> Map<String, Value> {
    let mut map = Map::new();
    loop {
        skip_blank(lines, i);
        if *i >= lines.len() {
            break;
        }
        let l = &lines[*i];
        if l.indent < indent {
            break;
        }
        if l.indent > indent || l.text.starts_with("- ") || l.text == "-" {
            // Unexpected deeper indentation or list item: skip.
            *i += 1;
            continue;
        }
        let Some((key, rest)) = split_key(l.text) else {
            *i += 1;
            continue;
        };
        *i += 1;
        let value = if rest.is_empty() {
            skip_blank(lines, i);
            match lines.get(*i) {
                Some(n) if n.indent > indent && (n.text.starts_with("- ") || n.text == "-") => {
                    let ind = n.indent;
                    parse_list(lines, i, ind)
                }
                // A list may also be written at the same indentation as its key.
                Some(n) if n.indent == indent && n.text.starts_with("- ") => parse_list(lines, i, indent),
                Some(n) if n.indent > indent => {
                    let ind = n.indent;
                    Value::Object(parse_map(lines, i, ind))
                }
                _ => Value::Null,
            }
        } else if let Some(style) = block_style(rest) {
            block_scalar(lines, i, indent, style)
        } else {
            scalar(rest)
        };
        map.insert(key, value);
    }
    map
}

fn parse_list(lines: &[Line], i: &mut usize, indent: usize) -> Value {
    let mut items = Vec::new();
    loop {
        skip_blank(lines, i);
        let Some(l) = lines.get(*i) else { break };
        if l.indent != indent || !(l.text.starts_with("- ") || l.text == "-") {
            break;
        }
        items.push(scalar(l.text[1..].trim()));
        *i += 1;
    }
    Value::Array(items)
}

#[derive(Clone, Copy)]
struct BlockStyle {
    folded: bool,
    strip: bool,
}

fn block_style(rest: &str) -> Option<BlockStyle> {
    match rest {
        "|" | "|+" => Some(BlockStyle { folded: false, strip: false }),
        "|-" => Some(BlockStyle { folded: false, strip: true }),
        ">" | ">+" => Some(BlockStyle { folded: true, strip: false }),
        ">-" => Some(BlockStyle { folded: true, strip: true }),
        _ => None,
    }
}

fn block_scalar(lines: &[Line], i: &mut usize, parent: usize, style: BlockStyle) -> Value {
    let mut parts: Vec<String> = Vec::new();
    let mut block_indent = None;
    while let Some(l) = lines.get(*i) {
        if l.text.is_empty() {
            parts.push(String::new());
            *i += 1;
            continue;
        }
        if l.indent <= parent {
            break;
        }
        let bi = *block_indent.get_or_insert(l.indent);
        let extra = " ".repeat(l.indent.saturating_sub(bi));
        parts.push(format!("{extra}{}", l.text));
        *i += 1;
    }
    while parts.last().is_some_and(|p| p.is_empty()) {
        parts.pop();
    }
    let mut s = if style.folded {
        let mut out = String::new();
        for p in &parts {
            if p.is_empty() {
                out.push('\n');
            } else {
                if !out.is_empty() && !out.ends_with('\n') {
                    out.push(' ');
                }
                out.push_str(p);
            }
        }
        out
    } else {
        parts.join("\n")
    };
    if !style.strip && !s.is_empty() {
        s.push('\n');
    }
    Value::String(s)
}

/// Split `key: rest`. Keys may be quoted. Returns None if the line has no key.
fn split_key(text: &str) -> Option<(String, &str)> {
    if text.starts_with('"') || text.starts_with('\'') {
        let (k, used) = parse_quoted(text)?;
        let rest = text[used..].trim_start();
        let rest = rest.strip_prefix(':')?;
        return Some((k, rest.trim()));
    }
    let bytes = text.as_bytes();
    for (idx, &b) in bytes.iter().enumerate() {
        if b == b':' && (idx + 1 == bytes.len() || bytes[idx + 1] == b' ' || bytes[idx + 1] == b'\t') {
            let k = text[..idx].trim();
            if k.is_empty() {
                return None;
            }
            return Some((k.to_string(), text[idx + 1..].trim()));
        }
    }
    None
}

/// Parse a quoted string at the start of `s`; returns (value, bytes consumed).
fn parse_quoted(s: &str) -> Option<(String, usize)> {
    let q = s.chars().next()?;
    let mut out = String::new();
    let mut chars = s.char_indices().skip(1);
    while let Some((idx, c)) = chars.next() {
        if c == q {
            if q == '\'' && s[idx + 1..].starts_with('\'') {
                out.push('\'');
                chars.next();
                continue;
            }
            return Some((out, idx + 1));
        }
        if c == '\\' && q == '"' {
            match chars.next() {
                Some((_, 'n')) => out.push('\n'),
                Some((_, 't')) => out.push('\t'),
                Some((_, 'r')) => out.push('\r'),
                Some((_, '0')) => out.push('\0'),
                Some((_, o)) => out.push(o),
                None => break,
            }
            continue;
        }
        out.push(c);
    }
    None
}

/// Parse a scalar value.
pub fn scalar(raw: &str) -> Value {
    let s = raw.trim();
    if s.starts_with('"') || s.starts_with('\'') {
        if let Some((v, used)) = parse_quoted(s) {
            let tail = s[used..].trim();
            if tail.is_empty() || tail.starts_with('#') {
                return Value::String(v);
            }
        }
        return Value::String(s.to_string());
    }
    // Strip trailing ` # comment`.
    let s = match s.find(" #") {
        Some(p) => s[..p].trim_end(),
        None => s,
    };
    if s.starts_with('[') && s.ends_with(']') {
        let inner = &s[1..s.len() - 1];
        if inner.trim().is_empty() {
            return Value::Array(vec![]);
        }
        return Value::Array(split_flow(inner).iter().map(|x| scalar(x)).collect());
    }
    if s.starts_with('{') && s.ends_with('}') {
        if let Ok(v) = crate::parse_jsonc(s) {
            return v;
        }
        return Value::String(s.to_string());
    }
    match s {
        "true" | "True" | "TRUE" | "yes" | "on" => return Value::Bool(true),
        "false" | "False" | "FALSE" | "no" | "off" => return Value::Bool(false),
        "null" | "~" | "Null" | "NULL" | "" => return Value::Null,
        _ => {}
    }
    if let Ok(n) = s.parse::<i64>() {
        return Value::Number(n.into());
    }
    if s.chars().any(|c| c.is_ascii_digit())
        && !s.contains(['x', 'X', 'n', 'N', 'i', 'I'])
        && let Ok(f) = s.parse::<f64>()
        && let Some(n) = Number::from_f64(f)
    {
        return Value::Number(n);
    }
    Value::String(s.to_string())
}

/// Split a flow-list body on commas outside quotes.
fn split_flow(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for c in s.chars() {
        match quote {
            Some(q) if c == q => {
                quote = None;
                cur.push(c);
            }
            Some(_) => cur.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                cur.push(c);
            }
            None if c == ',' => out.push(std::mem::take(&mut cur)),
            None => cur.push(c),
        }
    }
    out.push(cur);
    out.into_iter().map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()
}

/// A markdown definition found on disk.
pub struct MdDoc {
    /// Name: relative path below the `agent/`-style dir, without `.md`, joined with `/`.
    pub name: String,
    pub path: PathBuf,
    pub frontmatter: Map<String, Value>,
    pub body: String,
    /// Found under `mode/` or `modes/`.
    pub legacy_mode: bool,
}

/// Collect `*.md` files (recursively) under `root/<sub>` for each sub dir name.
pub fn collect(root: &Path, subdirs: &[&str]) -> (Vec<MdDoc>, Vec<String>) {
    let mut docs = Vec::new();
    let mut warns = Vec::new();
    for sub in subdirs {
        let base = root.join(sub);
        if !base.is_dir() {
            continue;
        }
        let mut files = Vec::new();
        walk(&base, &mut files, 0);
        files.sort();
        for f in files {
            let rel = match f.strip_prefix(&base) {
                Ok(r) => r.with_extension(""),
                Err(_) => continue,
            };
            let name = rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
            match std::fs::read_to_string(&f) {
                Ok(text) => {
                    let (frontmatter, body) = split_frontmatter(&text);
                    docs.push(MdDoc {
                        name,
                        path: f.clone(),
                        frontmatter,
                        body,
                        legacy_mode: sub.starts_with("mode"),
                    });
                }
                Err(e) => warns.push(format!("{}: {e}", f.display())),
            }
        }
    }
    (docs, warns)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 16 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() || (ft.is_symlink() && p.is_dir()) {
            walk(&p, out, depth + 1);
        } else if p.extension().is_some_and(|x| x == "md") {
            out.push(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn frontmatter_agent() {
        let text = r##"---
description: Reviews code: quality and best practices
mode: subagent
model: anthropic/claude-sonnet-4-5
temperature: 0.1
hidden: false
color: "#ff0000"
permission:
  edit: deny
  bash:
    "*": ask
    "git diff": allow
    git log*: allow
  webfetch: deny
tools:
  write: false
delay:
  bash: 2
tags: [a, "b c"]
list:
  - x
  - 2
notes: |
  line one
    indented
  line two
---

You are in code review mode.
"##;
        let (fm, body) = split_frontmatter(text);
        assert_eq!(body, "You are in code review mode.");
        assert_eq!(fm["description"], json!("Reviews code: quality and best practices"));
        assert_eq!(fm["mode"], json!("subagent"));
        assert_eq!(fm["temperature"], json!(0.1));
        assert_eq!(fm["hidden"], json!(false));
        assert_eq!(fm["color"], json!("#ff0000"));
        assert_eq!(
            Value::Object(fm["permission"].as_object().unwrap().clone()),
            json!({"edit": "deny", "bash": {"*": "ask", "git diff": "allow", "git log*": "allow"}, "webfetch": "deny"})
        );
        let keys: Vec<_> = fm["permission"]["bash"].as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["*", "git diff", "git log*"]);
        assert_eq!(fm["tools"], json!({"write": false}));
        assert_eq!(fm["delay"], json!({"bash": 2}));
        assert_eq!(fm["tags"], json!(["a", "b c"]));
        assert_eq!(fm["list"], json!(["x", 2]));
        assert_eq!(fm["notes"], json!("line one\n  indented\nline two\n"));
    }

    #[test]
    fn no_frontmatter() {
        let (fm, body) = split_frontmatter("hello\n---\nworld");
        assert!(fm.is_empty());
        assert_eq!(body, "hello\n---\nworld");
        let (fm, body) = split_frontmatter("---\ndescription: x\n---\n");
        assert_eq!(fm["description"], json!("x"));
        assert_eq!(body, "");
    }

    #[test]
    fn scalars() {
        assert_eq!(scalar("'it''s'"), json!("it's"));
        assert_eq!(scalar("\"a\\nb\""), json!("a\nb"));
        assert_eq!(scalar("42"), json!(42));
        assert_eq!(scalar("1.5"), json!(1.5));
        assert_eq!(scalar("yes"), json!(true));
        assert_eq!(scalar("plain text # c"), json!("plain text"));
        assert_eq!(scalar("nan"), json!("nan"));
        assert_eq!(scalar("{\"a\": 1}"), json!({"a": 1}));
    }
}
