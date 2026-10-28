//! JSONC preprocessing (comments + trailing commas), `{env:}` / `{file:}` substitution and
//! document merging.

use regex::Regex;
use serde_json::{Map, Value};
use std::path::Path;
use std::sync::OnceLock;

/// Strip `//` and `/* */` comments (outside strings) and trailing commas before `}` / `]`.
/// Comment bytes are replaced by spaces (newlines kept) so serde_json error positions still
/// point at the right line/column.
pub fn strip(text: &str) -> String {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let b = text.as_bytes();
    let n = b.len();
    let mut out: Vec<u8> = Vec::with_capacity(n);
    let mut i = 0;
    let mut in_str = false;
    while i < n {
        let c = b[i];
        if in_str {
            out.push(c);
            if c == b'\\' && i + 1 < n {
                out.push(b[i + 1]);
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' => {
                in_str = true;
                out.push(c);
                i += 1;
            }
            b'/' if i + 1 < n && b[i + 1] == b'/' => {
                while i < n && b[i] != b'\n' {
                    out.push(b' ');
                    i += 1;
                }
            }
            b'/' if i + 1 < n && b[i + 1] == b'*' => {
                out.extend_from_slice(b"  ");
                i += 2;
                while i < n && !(b[i] == b'*' && i + 1 < n && b[i + 1] == b'/') {
                    out.push(if b[i] == b'\n' { b'\n' } else { b' ' });
                    i += 1;
                }
                if i < n {
                    out.extend_from_slice(b"  ");
                    i += 2;
                }
            }
            b'}' | b']' => {
                let mut j = out.len();
                while j > 0 && out[j - 1].is_ascii_whitespace() {
                    j -= 1;
                }
                if j > 0 && out[j - 1] == b',' {
                    out[j - 1] = b' ';
                }
                out.push(c);
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    // Only ASCII bytes were inserted/replaced, and multi-byte sequences are either copied whole
    // or replaced byte-by-byte with spaces, so the output is valid UTF-8.
    String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

/// Parse JSONC text (comments, trailing commas) into a JSON value.
pub fn parse_jsonc(text: &str) -> anyhow::Result<Value> {
    let s = strip(text);
    if s.trim().is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    Ok(serde_json::from_str(&s)?)
}

fn env_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\{env:([^}]+)\}").unwrap())
}

fn file_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\{file:[^}]+\}").unwrap())
}

/// JSON-string-escape `s` without the surrounding quotes.
pub(crate) fn json_escape(s: &str) -> String {
    let q = serde_json::to_string(s).unwrap_or_default();
    q[1..q.len() - 1].to_string()
}

pub(crate) fn expand_home(p: &str, home: Option<&Path>) -> std::path::PathBuf {
    if let Some(h) = home {
        if p == "~" {
            return h.to_path_buf();
        }
        if let Some(rest) = p.strip_prefix("~/") {
            return h.join(rest);
        }
    }
    std::path::PathBuf::from(p)
}

/// Apply `{env:VAR}` then `{file:path}` substitutions on raw config text (opencode semantics).
/// Env values are inserted raw; if the result no longer parses, they are re-inserted
/// JSON-string-escaped. File contents are trimmed and always escaped. `{file:}` tokens on a
/// line commented out with `//` are left alone. Missing files become "" with a warning.
pub fn substitute(
    text: &str,
    dir: &Path,
    home: Option<&Path>,
    env: &dyn Fn(&str) -> Option<String>,
    warnings: &mut Vec<String>,
) -> String {
    let sub_env = |escape: bool| -> String {
        env_re()
            .replace_all(text, |c: &regex::Captures| {
                let v = env(&c[1]).unwrap_or_default();
                if escape {
                    json_escape(&v)
                } else {
                    v
                }
            })
            .into_owned()
    };
    let has_env = text.contains("{env:");
    let raw = substitute_files(&sub_env(false), dir, home, warnings);
    if !has_env || parse_jsonc(&raw).is_ok() {
        return raw;
    }
    // Warnings from this second pass would duplicate the first pass's.
    substitute_files(&sub_env(true), dir, home, &mut Vec::new())
}

fn substitute_files(text: &str, dir: &Path, home: Option<&Path>, warnings: &mut Vec<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    for m in file_re().find_iter(text) {
        let start = m.start();
        out.push_str(&text[cursor..start]);
        cursor = m.end();
        let line_start = text[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
        if text[line_start..start].trim_start().starts_with("//") {
            out.push_str(m.as_str());
            continue;
        }
        let token = m.as_str();
        let p = &token["{file:".len()..token.len() - 1];
        let mut path = expand_home(p, home);
        if !path.is_absolute() {
            path = dir.join(path);
        }
        match std::fs::read_to_string(&path) {
            Ok(c) => out.push_str(&json_escape(c.trim())),
            Err(e) => warnings.push(format!("{token}: cannot read {}: {e}", path.display())),
        }
    }
    out.push_str(&text[cursor..]);
    out
}

/// Deep merge `over` into `base`: objects merge recursively (later wins), everything else is
/// replaced.
pub fn deep_merge(base: &mut Value, over: Value) {
    match (base, over) {
        (Value::Object(b), Value::Object(o)) => merge_maps(b, o),
        (b, o) => *b = o,
    }
}

pub(crate) fn merge_maps(b: &mut Map<String, Value>, o: Map<String, Value>) {
    for (k, v) in o {
        match b.get_mut(&k) {
            Some(existing) if existing.is_object() && v.is_object() => deep_merge(existing, v),
            _ => {
                b.insert(k, v);
            }
        }
    }
}

/// Merge one config document into the accumulated one: deep merge, except `instructions` /
/// `plugin` arrays are concatenated + de-duplicated and each `mcp.<name>` entry is replaced whole.
pub fn merge_doc(base: &mut Value, doc: Value) {
    let Value::Object(doc) = doc else { return };
    if !base.is_object() {
        *base = Value::Object(Map::new());
    }
    let b = base.as_object_mut().unwrap();
    for (k, v) in doc {
        match (k.as_str(), b.get_mut(&k)) {
            ("instructions" | "plugin", Some(Value::Array(existing))) if v.is_array() => {
                for item in v.as_array().unwrap() {
                    if !existing.contains(item) {
                        existing.push(item.clone());
                    }
                }
            }
            ("mcp", Some(Value::Object(existing))) if v.is_object() => {
                for (name, server) in v.as_object().unwrap() {
                    existing.insert(name.clone(), server.clone());
                }
            }
            (_, Some(existing)) if existing.is_object() && v.is_object() => deep_merge(existing, v),
            _ => {
                b.insert(k, v);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn strips_comments_and_trailing_commas() {
        let t = r#"{
  // line comment
  "url": "http://x//y", /* block
  comment */ "a": [1, 2, 3,],
  "s": "has /* not a comment */ and \" quote, }",
  "o": {"k": 1,   },
}"#;
        let v = parse_jsonc(t).unwrap();
        assert_eq!(v["url"], "http://x//y");
        assert_eq!(v["a"], json!([1, 2, 3]));
        assert_eq!(v["s"], "has /* not a comment */ and \" quote, }");
        assert_eq!(v["o"], json!({"k": 1}));
    }

    #[test]
    fn trailing_comma_before_comment() {
        let v = parse_jsonc("{\"a\": 1, // c\n}").unwrap();
        assert_eq!(v, json!({"a": 1}));
        let v = parse_jsonc("{\"a\": [\"x\", /* c */ ] , }").unwrap();
        assert_eq!(v, json!({"a": ["x"]}));
        assert_eq!(parse_jsonc("  ").unwrap(), json!({}));
        let v = parse_jsonc("{\"中文\": \"值 // 不是注释\", /* 注释 */}").unwrap();
        assert_eq!(v["中文"], "值 // 不是注释");
    }

    #[test]
    fn invalid_reports_error() {
        assert!(parse_jsonc("{\"a\": }").is_err());
    }

    #[test]
    fn env_and_file_substitution() {
        let dir = std::env::temp_dir().join(format!("hn-config-sub-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub/key.txt"), "  line1\n\"quoted\"\n  ").unwrap();
        let env = |k: &str| match k {
            "HN_TEST_KEY" => Some("sk-123".to_string()),
            "HN_WIN" => Some("C:\\path".to_string()),
            _ => None,
        };
        let mut w = Vec::new();
        let text = r#"{
  "a": "{env:HN_TEST_KEY}",
  "missing": "{env:HN_NOPE}",
  "f": "{file:sub/key.txt}",
  // "c": "{file:nope.txt}",
  "g": "{file:does-not-exist}"
}"#;
        let out = substitute(text, &dir, None, &env, &mut w);
        let v = parse_jsonc(&out).unwrap();
        assert_eq!(v["a"], "sk-123");
        assert_eq!(v["missing"], "");
        assert_eq!(v["f"], "line1\n\"quoted\"");
        assert_eq!(v["g"], "");
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(out.contains("{file:nope.txt}"));

        // Raw substitution breaks JSON -> escaped retry.
        let out = substitute(r#"{"p": "{env:HN_WIN}"}"#, &dir, None, &env, &mut w);
        assert_eq!(parse_jsonc(&out).unwrap()["p"], "C:\\path");
        // Raw substitution of non-string JSON stays raw.
        let env2 = |_: &str| Some("42".to_string());
        let out = substitute(r#"{"n": {env:X}}"#, &dir, None, &env2, &mut w);
        assert_eq!(parse_jsonc(&out).unwrap()["n"], 42);

        // ~/ expansion
        let out = substitute(r#"{"h": "{file:~/sub/key.txt}"}"#, Path::new("/"), Some(&dir), &env, &mut w);
        assert_eq!(parse_jsonc(&out).unwrap()["h"], "line1\n\"quoted\"");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn merge_precedence() {
        let mut base = json!({
            "model": "a/b",
            "provider": {"p": {"options": {"baseURL": "x", "apiKey": "k"}, "models": {"m1": {}}}},
            "instructions": ["A.md", "B.md"],
            "mcp": {"s": {"type": "local", "command": ["x"], "environment": {"A": "1"}}},
        });
        merge_doc(
            &mut base,
            json!({
                "model": "c/d",
                "provider": {"p": {"options": {"baseURL": "y"}, "models": {"m2": {}}}},
                "instructions": ["B.md", "C.md"],
                "mcp": {"s": {"type": "remote", "url": "http://u"}},
            }),
        );
        assert_eq!(base["model"], "c/d");
        assert_eq!(base["provider"]["p"]["options"], json!({"baseURL": "y", "apiKey": "k"}));
        assert_eq!(base["provider"]["p"]["models"], json!({"m1": {}, "m2": {}}));
        assert_eq!(base["instructions"], json!(["A.md", "B.md", "C.md"]));
        assert_eq!(base["mcp"]["s"], json!({"type": "remote", "url": "http://u"}));
    }
}
