//! Permission rules (opencode-compatible).
//!
//! Accepted forms:
//! - `"permission": "allow"` (applies to every tool)
//! - `"permission": {"bash": "ask", "edit": "deny", "*": "allow"}`
//! - `"permission": {"bash": {"git *": "allow", "rm *": "ask", "*": "ask"}}`
//!
//! Tool keys may be globs (`*`, `?`), e.g. `"mcp_*"`. Rules are evaluated in
//! declaration order and the LAST match wins. opencode's legacy aliases are
//! mapped: `write`/`patch`/`multiedit` -> also apply to our `edit`/`write`, see `aliases`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Allow,
    Ask,
    Deny,
}

impl Action {
    pub fn parse(s: &str) -> Option<Action> {
        match s.trim().to_ascii_lowercase().as_str() {
            "allow" | "auto" | "yes" | "true" => Some(Action::Allow),
            "ask" | "confirm" | "prompt" => Some(Action::Ask),
            "deny" | "reject" | "no" | "false" => Some(Action::Deny),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    /// Tool name glob.
    pub tool: String,
    /// Subject glob (bash command, path); "*" for any.
    pub pattern: String,
    pub action: Action,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PermissionRules {
    pub rules: Vec<Rule>,
}

/// Simple glob: `*` matches any sequence (including `/` and spaces), `?` one char.
/// A pattern ending in `" *"` also matches the bare prefix (so `"git *"` matches `"git"`).
pub fn glob_match(pattern: &str, text: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix(" *") {
        if text == prefix {
            return true;
        }
    }
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// opencode tool-name aliases -> our tool names.
fn aliases(key: &str) -> Vec<String> {
    match key {
        "edit" => vec!["edit".into(), "write".into(), "delete".into()],
        "write" | "patch" | "multiedit" | "apply_patch" => vec!["write".into(), "edit".into()],
        "glob" | "list" | "ls" => vec!["find".into()],
        "shell" => vec!["bash".into()],
        k => vec![k.to_string()],
    }
}

impl PermissionRules {
    /// Parse an opencode `permission` value. Unknown action strings are ignored (reported via Err list).
    pub fn from_value(v: &Value) -> (PermissionRules, Vec<String>) {
        let mut rules = Vec::new();
        let mut warns = Vec::new();
        match v {
            Value::String(s) => match Action::parse(s) {
                Some(a) => rules.push(Rule { tool: "*".into(), pattern: "*".into(), action: a }),
                None => warns.push(format!("invalid permission action {s:?}")),
            },
            Value::Object(map) => {
                for (k, val) in map {
                    for tool in aliases(k) {
                        match val {
                            Value::String(s) => match Action::parse(s) {
                                Some(a) => rules.push(Rule { tool: tool.clone(), pattern: "*".into(), action: a }),
                                None => warns.push(format!("invalid permission action {s:?} for {k}")),
                            },
                            Value::Object(pats) => {
                                for (pat, pv) in pats {
                                    match pv.as_str().and_then(Action::parse) {
                                        Some(a) => rules.push(Rule {
                                            tool: tool.clone(),
                                            pattern: pat.clone(),
                                            action: a,
                                        }),
                                        None => warns.push(format!("invalid permission action for {k}.{pat}")),
                                    }
                                }
                            }
                            Value::Bool(b) => rules.push(Rule {
                                tool: tool.clone(),
                                pattern: "*".into(),
                                action: if *b { Action::Allow } else { Action::Deny },
                            }),
                            _ => warns.push(format!("invalid permission value for {k}")),
                        }
                    }
                }
            }
            Value::Null => {}
            _ => warns.push("invalid permission value".into()),
        }
        (PermissionRules { rules }, warns)
    }

    /// opencode legacy `tools: {"bash": false}` map -> deny rules.
    pub fn from_tools_map(v: &Value) -> PermissionRules {
        let mut rules = Vec::new();
        if let Value::Object(map) = v {
            for (k, val) in map {
                if let Value::Bool(b) = val {
                    for tool in aliases(k) {
                        rules.push(Rule {
                            tool,
                            pattern: "*".into(),
                            action: if *b { Action::Allow } else { Action::Deny },
                        });
                    }
                }
            }
        }
        PermissionRules { rules }
    }

    pub fn extend(&mut self, other: PermissionRules) {
        self.rules.extend(other.rules);
    }

    /// Last matching rule's action, if any.
    pub fn evaluate(&self, tool: &str, subject: &str) -> Option<Action> {
        let mut out = None;
        for r in &self.rules {
            if glob_match(&r.tool, tool) && (r.pattern == "*" || glob_match(&r.pattern, subject)) {
                out = Some(r.action);
            }
        }
        out
    }

    /// `Some(true)` if the last rule for this tool with pattern "*" is Deny and no later
    /// pattern-specific rule allows/asks. `None` if no rule mentions the tool.
    pub fn disabled_opt(&self, tool: &str) -> Option<bool> {
        let mut out = None;
        for r in &self.rules {
            if !glob_match(&r.tool, tool) {
                continue;
            }
            if r.pattern == "*" {
                out = Some(r.action == Action::Deny);
            } else if r.action != Action::Deny {
                out = Some(false);
            }
        }
        out
    }

    pub fn disabled(&self, tool: &str) -> bool {
        self.disabled_opt(tool).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn globbing() {
        assert!(glob_match("git *", "git"));
        assert!(glob_match("git *", "git status"));
        assert!(!glob_match("git *", "gitx"));
        assert!(glob_match("*", ""));
        assert!(glob_match("mcp_*", "mcp_search"));
        assert!(glob_match("a?c", "abc"));
        assert!(glob_match("*.rs", "src/main.rs"));
    }

    #[test]
    fn last_wins() {
        let (r, w) = PermissionRules::from_value(&json!({"*": "allow", "bash": {"*": "ask", "git *": "allow"}}));
        assert!(w.is_empty());
        assert_eq!(r.evaluate("read", ""), Some(Action::Allow));
        assert_eq!(r.evaluate("bash", "rm -rf /"), Some(Action::Ask));
        assert_eq!(r.evaluate("bash", "git log"), Some(Action::Allow));
    }

    #[test]
    fn disabled() {
        let (r, _) = PermissionRules::from_value(&json!({"bash": "deny"}));
        assert!(r.disabled("bash"));
        assert!(!r.disabled("read"));
        let (r, _) = PermissionRules::from_value(&json!({"bash": {"*": "deny", "ls *": "allow"}}));
        assert!(!r.disabled("bash"));
    }
}
