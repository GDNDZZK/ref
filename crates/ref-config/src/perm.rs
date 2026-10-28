//! Permission rule parsing and evaluation (opencode wildcard semantics, last match wins).

use std::collections::BTreeMap;
use std::path::Path;

use regex::Regex;
use serde_json::Value;

use crate::{Action, PermissionRule, Permissions};

/// Keys accepted by opencode but not meaningful for ref.
const IGNORED_KEYS: &[&str] = &["doom_loop", "external_directory"];

pub fn parse_action(v: &Value) -> Option<Action> {
    match v.as_str()?.trim().to_ascii_lowercase().as_str() {
        "allow" => Some(Action::Allow),
        "ask" => Some(Action::Ask),
        "deny" => Some(Action::Deny),
        _ => None,
    }
}

/// Parse an opencode `permission` value (string or map).
pub fn parse_permission(v: &Value) -> Permissions {
    let mut rules = Vec::new();
    match v {
        Value::String(_) => {
            if let Some(a) = parse_action(v) {
                rules.push(("*".to_string(), PermissionRule::Action(a)));
            } else {
                tracing::warn!("invalid permission value {v}");
            }
        }
        Value::Object(map) => {
            for (k, val) in map {
                if IGNORED_KEYS.contains(&k.as_str()) {
                    continue;
                }
                match val {
                    Value::String(_) => match parse_action(val) {
                        Some(a) => rules.push((k.clone(), PermissionRule::Action(a))),
                        None => tracing::warn!("invalid permission for {k}: {val}"),
                    },
                    Value::Object(pats) => {
                        let list: Vec<(String, Action)> = pats
                            .iter()
                            .filter_map(|(p, a)| {
                                let act = parse_action(a);
                                if act.is_none() {
                                    tracing::warn!("invalid permission for {k} {p}: {a}");
                                }
                                act.map(|a| (p.clone(), a))
                            })
                            .collect();
                        rules.push((k.clone(), PermissionRule::Patterns(list)));
                    }
                    _ => tracing::warn!("invalid permission for {k}: {val}"),
                }
            }
        }
        Value::Null => {}
        _ => tracing::warn!("invalid permission value {v}"),
    }
    Permissions { rules }
}

/// Legacy `tools: { name: bool }` → allow/deny rules.
pub fn parse_tools(v: &Value) -> Permissions {
    let mut rules = Vec::new();
    if let Value::Object(map) = v {
        for (k, val) in map {
            if let Some(b) = val.as_bool() {
                let a = if b { Action::Allow } else { Action::Deny };
                rules.push((k.clone(), PermissionRule::Action(a)));
            }
        }
    }
    Permissions { rules }
}

/// Compile an opencode wildcard: `*` any sequence, `?` one char, trailing ` *` also matches the
/// bare prefix (`git *` matches `git`). Anchored, dot matches newline.
pub fn wildcard_regex(pattern: &str) -> Regex {
    let (body, tail) = match pattern.strip_suffix(" *") {
        Some(b) => (b, "( .*)?"),
        None => (pattern, ""),
    };
    let mut re = String::from("(?s)^");
    for ch in body.chars() {
        match ch {
            '*' => re.push_str(".*"),
            '?' => re.push('.'),
            c => re.push_str(&regex::escape(&c.to_string())),
        }
    }
    re.push_str(tail);
    re.push('$');
    Regex::new(&re).unwrap_or_else(|_| Regex::new("^$").unwrap())
}

pub fn wildcard_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') && !pattern.contains('?') {
        return pattern == text;
    }
    wildcard_regex(pattern).is_match(text)
}

/// Does a config key (opencode tool name, alias, or wildcard) apply to a ref tool name?
pub fn key_applies(key: &str, tool: &str) -> bool {
    if wildcard_match(key, tool) {
        return true;
    }
    match key {
        "edit" => matches!(
            tool,
            "write" | "delete" | "patch" | "apply_patch" | "multiedit"
        ),
        "glob" | "list" => tool == "find",
        "shell" => tool == "bash",
        _ => false,
    }
}

fn expand_pattern(p: &str, home: &Path) -> String {
    let h = home.to_string_lossy();
    if p == "~" || p == "$HOME" {
        h.into_owned()
    } else if let Some(r) = p.strip_prefix("~/") {
        format!("{}/{}", h.trim_end_matches('/'), r)
    } else if let Some(r) = p.strip_prefix("$HOME/") {
        format!("{}/{}", h.trim_end_matches('/'), r)
    } else {
        p.to_string()
    }
}

/// Evaluate rule sets in order; the last matching rule wins. `None` if nothing matched.
pub fn evaluate(
    sets: &[&Permissions],
    tool: &str,
    subject: Option<&str>,
    home: &Path,
) -> Option<Action> {
    let subj = subject.unwrap_or("*");
    let expand = tool != "bash";
    let mut result = None;
    for set in sets {
        for (key, rule) in &set.rules {
            if !key_applies(key, tool) {
                continue;
            }
            match rule {
                PermissionRule::Action(a) => result = Some(*a),
                PermissionRule::Patterns(pats) => {
                    for (p, a) in pats {
                        let p = if expand {
                            expand_pattern(p, home)
                        } else {
                            p.clone()
                        };
                        if wildcard_match(&p, subj) {
                            result = Some(*a);
                        }
                    }
                }
            }
        }
    }
    result
}

/// Delay lookup in one map: exact key, alias key, last matching wildcard key, then `*`.
pub fn delay_lookup(map: &BTreeMap<String, f64>, tool: &str) -> Option<f64> {
    if let Some(v) = map.get(tool) {
        return Some(*v);
    }
    let mut found = None;
    for (k, v) in map {
        if k != "*" && key_applies(k, tool) {
            found = Some(*v);
        }
    }
    found.or_else(|| map.get("*").copied())
}

pub fn parse_delay(v: &Value) -> BTreeMap<String, f64> {
    let mut out = BTreeMap::new();
    match v {
        Value::Object(m) => {
            for (k, val) in m {
                if let Some(f) = val.as_f64() {
                    out.insert(k.clone(), f.max(0.0));
                }
            }
        }
        Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                out.insert("*".into(), f.max(0.0));
            }
        }
        _ => {}
    }
    out
}
