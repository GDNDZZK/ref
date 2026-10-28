//! Agent and command parsing.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use crate::perm::{parse_delay, parse_permission, parse_tools};
use crate::{AgentConfig, CommandConfig, Permissions};

const PLAN_PROMPT: &str =
    "Plan mode: analyze and propose a plan; do not modify files unless the user explicitly asks.";

/// Built-in agents as raw config values (user config is deep-merged on top).
pub fn builtin_agents() -> Map<String, Value> {
    let mut m = Map::new();
    m.insert(
        "build".into(),
        json!({
            "description": "Default agent with all tools",
            "prompt": "",
            "spawnable": true,
            "can_spawn": true,
        }),
    );
    m.insert(
        "plan".into(),
        json!({
            "description": "Read-only analysis and planning; edits require approval",
            "prompt": PLAN_PROMPT,
            "spawnable": true,
            "can_spawn": true,
            "permission": {"edit": "ask", "write": "ask", "delete": "ask", "bash": "ask"},
        }),
    );
    m
}

/// Keys with a meaning for ref (or explicitly ignored); everything else goes to `options`.
const KNOWN_AGENT_KEYS: &[&str] = &[
    "name",
    "description",
    "mode",
    "model",
    "variant",
    "prompt",
    "temperature",
    "top_p",
    "color",
    "hidden",
    "disable",
    "disabled",
    "permission",
    "tools",
    "steps",
    "maxSteps",
    "options",
    "spawnable",
    "can_spawn",
    "delay",
];

fn str_of(v: Option<&Value>) -> Option<String> {
    v.and_then(|x| x.as_str())
        .map(String::from)
        .filter(|s| !s.is_empty())
}

/// Parse one agent object. Returns `None` if disabled.
pub fn parse_agent(name: &str, v: &Value) -> Option<AgentConfig> {
    let o = v.as_object()?;
    let truthy = |k: &str| o.get(k).and_then(|b| b.as_bool()).unwrap_or(false);
    if truthy("disable") || truthy("disabled") {
        return None;
    }
    let mode = str_of(o.get("mode")).unwrap_or_else(|| "all".into());
    let spawnable = o
        .get("spawnable")
        .and_then(|b| b.as_bool())
        .unwrap_or(mode != "primary");
    let can_spawn = o.get("can_spawn").and_then(|b| b.as_bool()).unwrap_or(true);

    let mut permission = Permissions::default();
    if let Some(t) = o.get("tools") {
        permission.rules.extend(parse_tools(t).rules);
    }
    if let Some(p) = o.get("permission") {
        permission.rules.extend(parse_permission(p).rules);
    }

    let mut options = Map::new();
    if let Some(Value::Object(opts)) = o.get("options") {
        options.extend(opts.clone());
    }
    for (k, val) in o {
        if !KNOWN_AGENT_KEYS.contains(&k.as_str()) {
            options.insert(k.clone(), val.clone());
        }
    }

    let model = str_of(o.get("model"));
    let mut variant = str_of(o.get("variant"));
    let model = model.map(|m| match m.split_once('#') {
        Some((base, v)) => {
            if variant.is_none() && !v.is_empty() {
                variant = Some(v.to_string());
            }
            base.to_string()
        }
        None => m,
    });

    Some(AgentConfig {
        name: name.to_string(),
        description: str_of(o.get("description")).unwrap_or_default(),
        prompt: o
            .get("prompt")
            .and_then(|p| p.as_str())
            .unwrap_or("")
            .trim()
            .to_string(),
        model,
        variant,
        temperature: o.get("temperature").and_then(|t| t.as_f64()),
        top_p: o.get("top_p").and_then(|t| t.as_f64()),
        color: str_of(o.get("color")),
        spawnable,
        can_spawn,
        hidden: truthy("hidden"),
        permission,
        delay: o.get("delay").map(parse_delay).unwrap_or_default(),
        options,
        mode,
    })
}

/// Built-ins + `mode` (legacy, forced primary) + `agent` maps, in that order.
pub fn parse_agents(raw: &Value) -> BTreeMap<String, AgentConfig> {
    let mut merged = Value::Object(builtin_agents());
    if let Some(Value::Object(modes)) = raw.get("mode") {
        let mut m = Map::new();
        for (k, v) in modes {
            let mut v = v.clone();
            if let Value::Object(o) = &mut v {
                o.insert("mode".into(), Value::String("primary".into()));
            }
            m.insert(k.clone(), v);
        }
        crate::jsonc::deep_merge(&mut merged, Value::Object(m));
    }
    if let Some(a @ Value::Object(_)) = raw.get("agent") {
        crate::jsonc::deep_merge(&mut merged, a.clone());
    }
    let mut out = BTreeMap::new();
    if let Value::Object(m) = merged {
        for (name, v) in &m {
            if let Some(a) = parse_agent(name, v) {
                out.insert(name.clone(), a);
            }
        }
    }
    out
}

pub fn parse_commands(raw: &Value) -> BTreeMap<String, CommandConfig> {
    let mut out = BTreeMap::new();
    let Some(Value::Object(m)) = raw.get("command") else {
        return out;
    };
    for (name, v) in m {
        let (template, o) = match v {
            Value::String(s) => (s.clone(), Map::new()),
            Value::Object(o) => match o.get("template").and_then(|t| t.as_str()) {
                Some(t) => (t.to_string(), o.clone()),
                None => {
                    tracing::warn!("command {name}: missing template");
                    continue;
                }
            },
            _ => continue,
        };
        out.insert(
            name.clone(),
            CommandConfig {
                name: name.clone(),
                description: str_of(o.get("description")).unwrap_or_default(),
                template,
                agent: str_of(o.get("agent")),
                model: str_of(o.get("model")),
            },
        );
    }
    out
}
