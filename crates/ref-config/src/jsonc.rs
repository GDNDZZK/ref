//! JSONC preprocessing, `{env:..}` / `{file:..}` substitution and JSON merging helpers.

use std::path::Path;
use std::sync::OnceLock;

use anyhow::{bail, Context};
use regex::Regex;
use serde_json::{Map, Value};

/// Strip `//` and `/* */` comments (string-aware) and trailing commas before `]` / `}`.
/// Newlines inside comments are preserved so parser line numbers stay meaningful.
pub fn strip_jsonc(src: &str) -> String {
    let b = src.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            out.push(c);
            if c == b'\\' && i + 1 < b.len() {
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
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                    if b[i] == b'\n' {
                        out.push(b'\n');
                    }
                    i += 1;
                }
                i = (i + 2).min(b.len());
                out.push(b' ');
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }

    // Second pass: trailing commas.
    let mut res: Vec<u8> = Vec::with_capacity(out.len());
    let mut i = 0;
    let mut in_str = false;
    while i < out.len() {
        let c = out[i];
        if in_str {
            res.push(c);
            if c == b'\\' && i + 1 < out.len() {
                res.push(out[i + 1]);
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if c == b'"' {
            in_str = true;
        } else if c == b',' {
            let mut j = i + 1;
            while j < out.len() && out[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < out.len() && (out[j] == b'}' || out[j] == b']') {
                i += 1;
                continue;
            }
        }
        res.push(c);
        i += 1;
    }
    String::from_utf8_lossy(&res).into_owned()
}

fn env_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\{env:([^}]+)\}").unwrap())
}

fn file_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\{file:([^}]+)\}").unwrap())
}

/// JSON-string-escape `s` without the surrounding quotes.
pub fn json_escape(s: &str) -> String {
    let q = serde_json::to_string(s).unwrap_or_default();
    q[1..q.len() - 1].to_string()
}

/// Expand `~/` (and bare `~`) against `home`.
pub fn expand_home(p: &str, home: &Path) -> std::path::PathBuf {
    if p == "~" {
        home.to_path_buf()
    } else if let Some(rest) = p.strip_prefix("~/") {
        home.join(rest)
    } else {
        std::path::PathBuf::from(p)
    }
}

/// opencode-style variable substitution on raw config text.
/// `{env:VAR}` → value or "" (JSON-escaped when `escape_env`); `{file:path}` → trimmed, JSON-escaped
/// file contents (lines starting with `//` are left alone).
pub fn substitute(
    text: &str,
    base_dir: &Path,
    home: &Path,
    getenv: &dyn Fn(&str) -> Option<String>,
    escape_env: bool,
) -> String {
    let text = env_re().replace_all(text, |c: &regex::Captures| {
        let v = getenv(c[1].trim()).unwrap_or_default();
        if escape_env {
            json_escape(&v)
        } else {
            v
        }
    });
    if !text.contains("{file:") {
        return text.into_owned();
    }
    let mut out = String::with_capacity(text.len());
    for (idx, line) in text.split('\n').enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        if line.trim_start().starts_with("//") || !line.contains("{file:") {
            out.push_str(line);
            continue;
        }
        let replaced = file_re().replace_all(line, |c: &regex::Captures| {
            let raw = c[1].trim();
            let mut p = expand_home(raw, home);
            if p.is_relative() {
                p = base_dir.join(p);
            }
            match std::fs::read_to_string(&p) {
                Ok(s) => json_escape(s.trim()),
                Err(e) => {
                    tracing::warn!("config {{file:{}}}: {}", p.display(), e);
                    String::new()
                }
            }
        });
        out.push_str(&replaced);
    }
    out
}

/// Parse a config document: substitution, JSONC stripping, JSON parse. Must be an object.
pub fn parse_config_text(
    text: &str,
    base_dir: &Path,
    home: &Path,
    getenv: &dyn Fn(&str) -> Option<String>,
) -> anyhow::Result<Value> {
    let first = substitute(text, base_dir, home, getenv, false);
    let v = match serde_json::from_str::<Value>(&strip_jsonc(&first)) {
        Ok(v) => v,
        Err(e) => {
            // Retry with JSON-escaped env values (opencode behavior).
            let second = substitute(text, base_dir, home, getenv, true);
            serde_json::from_str::<Value>(&strip_jsonc(&second))
                .map_err(|_| e)
                .context("invalid JSON")?
        }
    };
    if !v.is_object() {
        bail!("config root must be a JSON object");
    }
    Ok(v)
}

/// Deep merge: objects merged recursively, everything else replaced by `over`.
pub fn deep_merge(base: &mut Value, over: Value) {
    match (base, over) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                match b.get_mut(&k) {
                    Some(bv) if bv.is_object() && v.is_object() => deep_merge(bv, v),
                    _ => {
                        b.insert(k, v);
                    }
                }
            }
        }
        (b, o) => *b = o,
    }
}

/// Top-level keys whose arrays are unioned across documents instead of replaced.
const UNION_KEYS: &[&str] = &["instructions", "plugin"];

/// Merge one config document into the accumulated config.
pub fn merge_layer(base: &mut Value, over: Value) {
    let Value::Object(mut over) = over else {
        return;
    };
    if let Value::Object(b) = base {
        for key in UNION_KEYS {
            if let (Some(Value::Array(old)), Some(Value::Array(new))) =
                (b.get(*key), over.get(*key))
            {
                let mut all = old.clone();
                for v in new {
                    if !all.contains(v) {
                        all.push(v.clone());
                    }
                }
                over.insert(key.to_string(), Value::Array(all));
            }
        }
        // `skills` may be an array or {paths, urls}: union the path lists.
        if let (Some(old), Some(new)) = (b.get("skills"), over.get("skills")) {
            let mut all = skill_paths_of(old);
            for p in skill_paths_of(new) {
                if !all.contains(&p) {
                    all.push(p);
                }
            }
            over.insert(
                "skills".into(),
                Value::Array(all.into_iter().map(Value::String).collect()),
            );
        }
    }
    deep_merge(base, Value::Object(over));
}

/// `skills` config: `["path", ...]` or `{ "paths": [...], "urls": [...] }` → paths.
pub fn skill_paths_of(v: &Value) -> Vec<String> {
    let arr = match v {
        Value::Array(a) => Some(a),
        Value::Object(o) => o.get("paths").and_then(|p| p.as_array()),
        _ => None,
    };
    arr.map(|a| {
        a.iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect()
    })
    .unwrap_or_default()
}

/// Fill keys missing in `target` from `defaults` recursively (target wins; new keys appended).
pub fn merge_defaults(target: &mut Value, defaults: &Value) {
    if let (Value::Object(t), Value::Object(d)) = (target, defaults) {
        for (k, dv) in d {
            match t.get_mut(k) {
                Some(tv) => merge_defaults(tv, dv),
                None => {
                    t.insert(k.clone(), dv.clone());
                }
            }
        }
    }
}

/// Deep-merge `over` into a map.
pub fn merge_map(base: &mut Map<String, Value>, over: &Map<String, Value>) {
    let mut b = Value::Object(std::mem::take(base));
    deep_merge(&mut b, Value::Object(over.clone()));
    if let Value::Object(m) = b {
        *base = m;
    }
}
