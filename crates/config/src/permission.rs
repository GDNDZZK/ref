//! opencode-style permissions + hernees per-tool delays.

use crate::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Allow,
    Ask,
    Deny,
}

impl Action {
    pub fn parse(s: &str) -> Option<Action> {
        match s {
            "allow" => Some(Action::Allow),
            "ask" => Some(Action::Ask),
            "deny" => Some(Action::Deny),
            _ => None,
        }
    }
}

/// Flattened ordered rules: (tool glob, subject glob, action). Last match wins.
#[derive(Debug, Clone, Default)]
pub struct PermissionRules {
    pub rules: Vec<(String, String, Action)>,
}

impl PermissionRules {
    /// Parse opencode `permission`: `"allow"` (= `{"*": "allow"}`) or
    /// `{ "<tool|glob>": action | { "<pattern>": action } }`, flattened in document order.
    /// `~/` / `$HOME/` at the start of subject patterns expand to the home dir.
    /// Fails if any entry is invalid; see [`parse_lenient`](Self::parse_lenient).
    pub fn parse(v: &Value) -> Result<PermissionRules, String> {
        let (rules, errors) = Self::parse_lenient(v, dirs::home_dir().as_deref());
        if errors.is_empty() {
            Ok(rules)
        } else {
            Err(errors.join("; "))
        }
    }

    /// Parse keeping every valid entry; invalid entries are returned as error strings.
    pub fn parse_lenient(v: &Value, home: Option<&Path>) -> (PermissionRules, Vec<String>) {
        let mut rules = Vec::new();
        let mut errors = Vec::new();
        match v {
            Value::String(s) => match Action::parse(s) {
                Some(a) => rules.push(("*".to_string(), "*".to_string(), a)),
                None => errors.push(format!("invalid action {s:?}")),
            },
            Value::Object(m) => {
                for (tool, rule) in m {
                    match rule {
                        Value::String(s) => match Action::parse(s) {
                            Some(a) => rules.push((tool.clone(), "*".to_string(), a)),
                            None => errors.push(format!("{tool}: invalid action {s:?}")),
                        },
                        Value::Object(pats) => {
                            for (pat, a) in pats {
                                match a.as_str().and_then(Action::parse) {
                                    Some(a) => rules.push((tool.clone(), expand_pattern(pat, home), a)),
                                    None => errors.push(format!("{tool}.{pat}: invalid action {a}")),
                                }
                            }
                        }
                        other => errors.push(format!("{tool}: invalid rule {other}")),
                    }
                }
            }
            Value::Null => {}
            other => errors.push(format!("expected a string or object, got {other}")),
        }
        (PermissionRules { rules }, errors)
    }

    /// Deprecated opencode `tools: {name: bool}` -> allow/deny rules (`write`/`patch` -> `edit`).
    pub fn from_tools(tools: &IndexMap<String, bool>) -> PermissionRules {
        let rules = tools
            .iter()
            .map(|(t, on)| {
                let t = match t.as_str() {
                    "write" | "patch" | "apply_patch" => "edit".to_string(),
                    _ => t.clone(),
                };
                (t, "*".to_string(), if *on { Action::Allow } else { Action::Deny })
            })
            .collect();
        PermissionRules { rules }
    }

    /// Last matching rule's action, if any. `tool` is a hernees tool name; opencode names that
    /// cover it (e.g. `edit` for `write`) also match.
    pub fn evaluate(&self, tool: &str, subject: &str) -> Option<Action> {
        let names = tool_names(tool);
        self.rules
            .iter()
            .rev()
            .find(|(t, p, _)| names.iter().any(|n| wildcard(n, t)) && wildcard(subject, p))
            .map(|(_, _, a)| *a)
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

/// Names a permission/delay key may use for a hernees tool.
fn tool_names(tool: &str) -> Vec<&str> {
    let mut v = vec![tool];
    match tool {
        "write" | "delete" | "patch" | "apply_patch" => v.push("edit"),
        "find" => v.extend(["glob", "list"]),
        _ => {}
    }
    v
}

fn expand_pattern(p: &str, home: Option<&Path>) -> String {
    let Some(h) = home else { return p.to_string() };
    let h = h.to_string_lossy();
    if p == "~" || p == "$HOME" {
        return h.into_owned();
    }
    for pre in ["~/", "$HOME/"] {
        if let Some(rest) = p.strip_prefix(pre) {
            return format!("{}/{}", h.trim_end_matches('/'), rest);
        }
    }
    p.to_string()
}

/// opencode wildcard: `*` any chars (including `/`), `?` one char, everything else literal.
/// A trailing `" *"` also matches the bare prefix (`"git *"` matches `"git"`).
pub fn wildcard(input: &str, pattern: &str) -> bool {
    let input = input.replace('\\', "/");
    let pattern = pattern.replace('\\', "/");
    if let Some(prefix) = pattern.strip_suffix(" *") {
        if glob_match(input.as_bytes(), prefix.as_bytes()) {
            return true;
        }
    }
    glob_match(input.as_bytes(), pattern.as_bytes())
}

fn glob_match(s: &[u8], p: &[u8]) -> bool {
    // Iterative matcher with single-star backtracking; `?` consumes one UTF-8 char.
    let (mut si, mut pi) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    let char_len = |i: usize| -> usize {
        let b = s[i];
        if b < 0x80 {
            1
        } else if b >= 0xF0 {
            4
        } else if b >= 0xE0 {
            3
        } else {
            2
        }
        .min(s.len() - i)
    };
    while si < s.len() {
        if pi < p.len() && p[pi] == b'*' {
            star = Some((pi, si));
            pi += 1;
        } else if pi < p.len() && p[pi] == b'?' {
            si += char_len(si);
            pi += 1;
        } else if pi < p.len() && p[pi] == s[si] {
            si += 1;
            pi += 1;
        } else if let Some((sp, ss)) = star {
            let next = ss + char_len(ss);
            star = Some((sp, next));
            si = next;
            pi = sp + 1;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == b'*')
}

pub(crate) fn agent_rules(a: &AgentConfig, home: Option<&Path>, warnings: &mut Vec<String>, name: &str) -> PermissionRules {
    let mut r = a.tools.as_ref().map(PermissionRules::from_tools).unwrap_or_default();
    if let Some(p) = &a.permission {
        let (pr, errs) = PermissionRules::parse_lenient(p, home);
        for e in errs {
            warnings.push(format!("agent.{name}.permission: {e}"));
        }
        r.rules.extend(pr.rules);
    }
    r
}

pub(crate) fn evaluate(c: &Config, agent: &str, tool: &str, subject: &str) -> Action {
    if let Some(a) = c.agent_permission.get(agent).and_then(|r| r.evaluate(tool, subject)) {
        return a;
    }
    // Agent not pre-computed (e.g. Config built by hand): compute on the fly.
    if !c.agent_permission.contains_key(agent) {
        if let Some(cfg) = c.agents.get(agent) {
            let r = agent_rules(cfg, dirs::home_dir().as_deref(), &mut Vec::new(), agent);
            if let Some(a) = r.evaluate(tool, subject) {
                return a;
            }
        }
    }
    c.permission.evaluate(tool, subject).unwrap_or(Action::Allow)
}

fn delay_lookup(map: &DelayConfig, tool: &str) -> Option<f64> {
    let names = tool_names(tool);
    map.iter().rev().find(|(k, _)| names.iter().any(|n| wildcard(n, k))).map(|(_, v)| *v)
}

pub(crate) fn delay(c: &Config, agent: &str, tool: &str) -> Duration {
    let secs = c
        .agents
        .get(agent)
        .and_then(|a| a.delay.as_ref())
        .and_then(|d| delay_lookup(d, tool))
        .or_else(|| delay_lookup(&c.delay, tool))
        .unwrap_or(0.0);
    if secs.is_finite() && secs > 0.0 {
        Duration::from_secs_f64(secs)
    } else {
        Duration::ZERO
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn wildcards() {
        assert!(wildcard("git status", "git *"));
        assert!(wildcard("git", "git *"));
        assert!(!wildcard("gitx", "git *"));
        assert!(wildcard("src/a/b.rs", "src/*.rs"));
        assert!(wildcard("a.env", "*.env"));
        assert!(!wildcard("a.env.example", "*.env"));
        assert!(wildcard("ab", "a?"));
        assert!(wildcard("a中", "a?"));
        assert!(!wildcard("abc", "a?"));
        assert!(wildcard("", "*"));
        assert!(wildcard("mcp_search", "mcp_*"));
        assert!(wildcard("x.y", "x.y"));
        assert!(!wildcard("xzy", "x.y"));
        assert!(wildcard("aaab", "*a*b"));
    }

    #[test]
    fn parse_and_last_match_wins() {
        let v = json!({
            "*": "ask",
            "bash": {"*": "ask", "git *": "allow", "git push *": "deny"},
            "edit": "allow",
            "read": {"*": "allow", "*.env": "deny", "~/secret/*": "deny"},
        });
        let r = { let (r, e) = PermissionRules::parse_lenient(&v, Some(Path::new("/home/u"))); assert!(e.is_empty()); r };
        assert_eq!(r.evaluate("bash", "git status"), Some(Action::Allow));
        assert_eq!(r.evaluate("bash", "git push origin"), Some(Action::Deny));
        assert_eq!(r.evaluate("bash", "rm -rf /"), Some(Action::Ask));
        assert_eq!(r.evaluate("write", "src/a.rs"), Some(Action::Allow));
        assert_eq!(r.evaluate("delete", "src/a.rs"), Some(Action::Allow));
        assert_eq!(r.evaluate("read", "x/.env"), Some(Action::Deny));
        assert_eq!(r.evaluate("read", "/home/u/secret/k"), Some(Action::Deny));
        assert_eq!(r.evaluate("read", "x/main.rs"), Some(Action::Allow));
        assert_eq!(r.evaluate("grep", "foo"), Some(Action::Ask));
        // Order matters: "*" after specific rules overrides them.
        let r = PermissionRules::parse(&json!({"bash": "deny", "*": "allow"})).unwrap();
        assert_eq!(r.evaluate("bash", "ls"), Some(Action::Allow));
        let r = PermissionRules::parse(&json!("deny")).unwrap();
        assert_eq!(r.evaluate("anything", ""), Some(Action::Deny));
        assert!(PermissionRules::parse(&json!("nope")).is_err());
        let r = PermissionRules::parse(&json!({"glob": "deny", "list": "allow"})).unwrap();
        assert_eq!(r.evaluate("find", "x"), Some(Action::Allow));
    }

    #[test]
    fn agent_overrides_global() {
        let raw = json!({
            "permission": {"bash": "ask", "edit": "deny"},
            "agent": {
                "ro": {"tools": {"write": false, "bash": true}, "permission": {"bash": {"rm *": "deny"}}},
                "free": {"permission": "allow"},
            }
        });
        let mut w = Vec::new();
        let c = crate::load::extract(raw, &mut w);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(c.permission("build", "bash", "ls"), Action::Ask);
        assert_eq!(c.permission("build", "edit", "a"), Action::Deny);
        assert_eq!(c.permission("build", "read", "a"), Action::Allow);
        assert_eq!(c.permission("ro", "bash", "ls"), Action::Allow);
        assert_eq!(c.permission("ro", "bash", "rm -rf x"), Action::Deny);
        assert_eq!(c.permission("ro", "write", "a"), Action::Deny);
        assert_eq!(c.permission("ro", "edit", "a"), Action::Deny);
        assert_eq!(c.permission("free", "edit", "a"), Action::Allow);
        assert_eq!(c.permission("unknown-agent", "bash", "x"), Action::Ask);
    }

    #[test]
    fn delays() {
        let raw = json!({
            "delay": {"*": 1, "bash": 5, "mcp_*": 2.5},
            "agent": {"fast": {"delay": {"bash": 0}}, "slow": {"delay": {"edit": 3}}},
        });
        let mut w = Vec::new();
        let c = crate::load::extract(raw, &mut w);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(c.delay("build", "bash"), Duration::from_secs(5));
        assert_eq!(c.delay("build", "read"), Duration::from_secs(1));
        assert_eq!(c.delay("build", "mcp_search"), Duration::from_millis(2500));
        assert_eq!(c.delay("fast", "bash"), Duration::ZERO);
        assert_eq!(c.delay("fast", "read"), Duration::from_secs(1));
        assert_eq!(c.delay("slow", "write"), Duration::from_secs(3));
        let c = crate::load::extract(json!({}), &mut w);
        assert_eq!(c.delay("build", "bash"), Duration::ZERO);
    }
}
