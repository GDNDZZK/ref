//! Turning the merged JSON document into a typed [`Config`].

use crate::builtin;
use crate::load::{Accum, Ctx, Layer, accumulate};
use crate::{
    AgentConfig, CommandConfig, CompactionConfig, Config, McpConfig, ModelConfig, PermissionRules,
    ProviderConfig,
};
use ref_proto::ModelRef;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub(crate) fn build(ctx: &mut Ctx, layers: &[Layer]) -> Config {
    let (merged, acc) = accumulate(ctx, layers);
    let Accum {
        permission,
        agent_permission,
        instructions,
        skills,
    } = acc;

    let mut cfg = Config {
        permission,
        instructions,
        skill_paths: skills,
        retry_max_delay: 600,
        images_auto_resize: true,
        ..Default::default()
    };

    cfg.providers = providers(ctx, &merged);
    cfg.model = model_ref(ctx, merged.get("model"), "model");
    cfg.small_model = model_ref(ctx, merged.get("small_model"), "small_model");
    cfg.delay = delay_list(ctx, merged.get("delay"), "delay");
    cfg.mcp = mcp(ctx, merged.get("mcp"));
    cfg.commands = commands(ctx, merged.get("command"));
    cfg.compaction = compaction(merged.get("compaction"));
    if let Some(r) = merged.get("retry") {
        match r.get("max_delay").and_then(Value::as_f64) {
            Some(s) if s >= 0.0 => cfg.retry_max_delay = s as u64,
            Some(_) => ctx.warn("retry.max_delay must be >= 0"),
            None => {}
        }
    }
    if let Some(v) = merged.get("images").and_then(|i| i.get("auto_resize")) {
        match v.as_bool() {
            Some(b) => cfg.images_auto_resize = b,
            None => ctx.warn("images.auto_resize must be true or false"),
        }
    }
    cfg.tui = merged.get("tui").cloned().unwrap_or(Value::Null);
    cfg.agents = agents(ctx, &merged, agent_permission);
    builtin::question_defaults(&mut cfg);

    let wanted = merged
        .get("default_agent")
        .and_then(Value::as_str)
        .unwrap_or("build");
    cfg.default_agent = if cfg.agents.contains_key(wanted) {
        wanted.to_string()
    } else {
        if merged.contains_key("default_agent") {
            ctx.warn(format!("default_agent {wanted:?} not found"));
        }
        if cfg.agents.contains_key("build") {
            "build".into()
        } else {
            cfg.agents
                .values()
                .find(|a| a.mode != "subagent" && !a.hidden)
                .or_else(|| cfg.agents.values().next())
                .map(|a| a.name.clone())
                .unwrap_or_else(|| "build".into())
        }
    };
    cfg
}

fn model_ref(ctx: &mut Ctx, v: Option<&Value>, what: &str) -> Option<ModelRef> {
    match v? {
        Value::String(s) if s.trim().is_empty() => None,
        Value::String(s) => {
            let r = ModelRef::parse(s.trim());
            if r.is_none() {
                ctx.warn(format!(
                    "{what}: invalid model {s:?} (expected \"provider/model\")"
                ));
            }
            r
        }
        Value::Null => None,
        _ => {
            ctx.warn(format!("{what}: model must be a string"));
            None
        }
    }
}

fn str_map(v: Option<&Value>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if let Some(Value::Object(m)) = v {
        for (k, v) in m {
            match v {
                Value::String(s) => {
                    out.insert(k.clone(), s.clone());
                }
                Value::Number(_) | Value::Bool(_) => {
                    out.insert(k.clone(), v.to_string());
                }
                _ => {}
            }
        }
    }
    out
}

fn str_list(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => vec![],
    }
}

fn get_str(m: &Map<String, Value>, k: &str) -> Option<String> {
    m.get(k).and_then(Value::as_str).map(str::to_string)
}

fn delay_list(ctx: &mut Ctx, v: Option<&Value>, what: &str) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    match v {
        Some(Value::Object(m)) => {
            for (k, v) in m {
                match v.as_f64() {
                    Some(s) if s >= 0.0 => out.push((k.clone(), s)),
                    _ => ctx.warn(format!(
                        "{what}.{k}: expected a non-negative number of seconds"
                    )),
                }
            }
        }
        Some(Value::Number(n)) => out.push(("*".into(), n.as_f64().unwrap_or(0.0).max(0.0))),
        None | Some(Value::Null) => {}
        Some(_) => ctx.warn(format!(
            "{what}: expected an object {{\"tool-glob\": seconds}}"
        )),
    }
    out
}

fn compaction(v: Option<&Value>) -> CompactionConfig {
    let mut c = CompactionConfig::default();
    if let Some(Value::Object(m)) = v {
        if let Some(b) = m.get("auto").and_then(Value::as_bool) {
            c.auto = b;
        }
        if let Some(x) = m
            .get("recommend")
            .and_then(Value::as_f64)
            .filter(|x| *x > 0.0)
        {
            c.recommend = x;
        }
        if let Some(x) = m
            .get("threshold")
            .and_then(Value::as_f64)
            .filter(|x| *x > 0.0)
        {
            c.threshold = x;
        }
    }
    c
}

// ---------------------------------------------------------------------------
// Providers
// ---------------------------------------------------------------------------

fn providers(ctx: &mut Ctx, merged: &Map<String, Value>) -> BTreeMap<String, ProviderConfig> {
    let disabled = str_list(merged.get("disabled_providers"));
    let enabled = merged.get("enabled_providers").map(|v| str_list(Some(v)));
    let allowed = |id: &str| {
        !disabled.iter().any(|d| d == id)
            && enabled.as_ref().is_none_or(|e| e.iter().any(|x| x == id))
    };

    let mut out = BTreeMap::new();
    match merged.get("provider") {
        Some(Value::Object(m)) => {
            for (id, p) in m {
                if !allowed(id) {
                    continue;
                }
                match p {
                    Value::Object(p) => {
                        out.insert(id.clone(), provider(ctx, id, p));
                    }
                    _ => ctx.warn(format!("provider.{id}: expected an object")),
                }
            }
        }
        None | Some(Value::Null) => {}
        Some(_) => ctx.warn("provider: expected an object"),
    }
    for (id, var, def) in builtin::catalog() {
        if out.contains_key(id) || !allowed(id) || ctx.env_nonempty(var).is_none() {
            continue;
        }
        if let Value::Object(p) = def {
            out.insert(id.to_string(), provider(ctx, id, &p));
        }
    }
    out
}

fn provider(ctx: &mut Ctx, id: &str, p: &Map<String, Value>) -> ProviderConfig {
    let defaults = builtin::provider_defaults(id);
    let npm = get_str(p, "npm").or_else(|| defaults.map(|d| d.0.to_string()));
    let mut base_url = None;
    let mut api = get_str(p, "api");
    // opencode/models.dev use `api` for the base URL.
    if api
        .as_deref()
        .is_some_and(|a| a.starts_with("http://") || a.starts_with("https://"))
    {
        base_url = api.take();
    }
    let mut env = str_list(p.get("env"));
    if env.is_empty()
        && let Some(d) = defaults
    {
        env.push(d.2.to_string());
    }
    let mut options = match p.get("options") {
        Some(Value::Object(o)) => o.clone(),
        _ => Map::new(),
    };
    let mut api_key = None;
    let mut headers = BTreeMap::new();
    for k in ["baseURL", "baseUrl", "base_url"] {
        if let Some(v) = options.remove(k)
            && let Some(s) = v.as_str().filter(|s| !s.trim().is_empty())
        {
            base_url = Some(s.trim().to_string());
        }
    }
    for k in ["apiKey", "api_key"] {
        if let Some(v) = options.remove(k)
            && let Some(s) = v.as_str().filter(|s| !s.trim().is_empty())
        {
            api_key = Some(s.trim().to_string());
        }
    }
    if let Some(h) = options.remove("headers") {
        headers = str_map(Some(&h));
    }
    if base_url.is_none() {
        base_url = defaults.map(|d| d.1.to_string());
    }
    if api_key.is_none() {
        let conventional = match npm.as_deref() {
            Some("@ai-sdk/anthropic") => Some("ANTHROPIC_API_KEY"),
            Some("@ai-sdk/openai") => Some("OPENAI_API_KEY"),
            _ => None,
        };
        api_key = env
            .iter()
            .map(String::as_str)
            .chain(conventional)
            .find_map(|v| ctx.env_nonempty(v));
    }

    let whitelist = str_list(p.get("whitelist"));
    let blacklist = str_list(p.get("blacklist"));
    let mut models = BTreeMap::new();
    match p.get("models") {
        Some(Value::Object(ms)) => {
            for (key, m) in ms {
                if (!whitelist.is_empty() && !whitelist.contains(key)) || blacklist.contains(key) {
                    continue;
                }
                match m {
                    Value::Object(m) => {
                        if m.get("disabled").and_then(Value::as_bool) == Some(true) {
                            continue;
                        }
                        models.insert(key.clone(), model(key, m));
                    }
                    Value::Null => {
                        models.insert(key.clone(), model(key, &Map::new()));
                    }
                    _ => ctx.warn(format!("provider.{id}.models.{key}: expected an object")),
                }
            }
        }
        None | Some(Value::Null) => {}
        Some(_) => ctx.warn(format!("provider.{id}.models: expected an object")),
    }

    ProviderConfig {
        id: id.to_string(),
        name: get_str(p, "name").unwrap_or_else(|| id.to_string()),
        npm,
        api,
        base_url,
        api_key,
        env,
        headers,
        options,
        models,
    }
}

fn model(key: &str, m: &Map<String, Value>) -> ModelConfig {
    let limit = m.get("limit");
    let lim = |k: &str| limit.and_then(|l| l.get(k)).and_then(Value::as_u64);
    let images = m
        .get("attachment")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || m.get("modalities")
            .and_then(|x| x.get("input"))
            .and_then(Value::as_array)
            .is_some_and(|a| a.iter().any(|x| x.as_str() == Some("image")));
    let flag = |k: &str| m.get(k).and_then(Value::as_bool).unwrap_or(false);
    let mut variants = BTreeMap::new();
    if let Some(Value::Object(vs)) = m.get("variants") {
        for (name, v) in vs {
            let mut o = v.as_object().cloned().unwrap_or_default();
            if o.remove("disabled").and_then(|d| d.as_bool()) == Some(true) {
                continue;
            }
            variants.insert(name.clone(), o);
        }
    }
    ModelConfig {
        key: key.to_string(),
        id: get_str(m, "id")
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| key.to_string()),
        name: get_str(m, "name")
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| key.to_string()),
        context: lim("context").unwrap_or(0),
        output: lim("output").unwrap_or(0),
        input: lim("input").unwrap_or(0),
        images,
        reasoning: flag("reasoning") || flag("thinking"),
        // opencode's `temperature: true` is a capability flag; only numbers set a value.
        temperature: m.get("temperature").and_then(Value::as_f64),
        top_p: m.get("top_p").and_then(Value::as_f64),
        headers: str_map(m.get("headers")),
        options: m
            .get("options")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default(),
        variants,
        compact_at: lim("compact"),
        threshold: limit
            .and_then(|l| l.get("threshold"))
            .and_then(Value::as_f64)
            .filter(|x| *x > 0.0),
    }
}

// ---------------------------------------------------------------------------
// MCP, commands
// ---------------------------------------------------------------------------

fn mcp(ctx: &mut Ctx, v: Option<&Value>) -> BTreeMap<String, McpConfig> {
    let mut out = BTreeMap::new();
    let Some(Value::Object(m)) = v else {
        return out;
    };
    for (name, s) in m {
        let Value::Object(s) = s else {
            ctx.warn(format!("mcp.{name}: expected an object"));
            continue;
        };
        let enabled = s.get("enabled").and_then(Value::as_bool).unwrap_or(true);
        let timeout_ms = s.get("timeout").and_then(Value::as_u64).filter(|t| *t > 0);
        let ty = get_str(s, "type").unwrap_or_else(|| {
            if s.contains_key("url") {
                "remote".into()
            } else if s.contains_key("command") {
                "local".into()
            } else {
                String::new()
            }
        });
        match ty.as_str() {
            "local" | "stdio" => {
                let command = match s.get("command") {
                    Some(Value::Array(a)) => a
                        .iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect(),
                    Some(Value::String(c)) => {
                        let mut v: Vec<String> = c.split_whitespace().map(str::to_string).collect();
                        if let Some(Value::Array(args)) = s.get("args") {
                            v.extend(args.iter().filter_map(|x| x.as_str().map(str::to_string)));
                        }
                        v
                    }
                    _ => Vec::new(),
                };
                if command.is_empty() {
                    ctx.warn(format!(
                        "mcp.{name}: local server requires a non-empty `command`"
                    ));
                    continue;
                }
                let environment = str_map(s.get("environment").or_else(|| s.get("env")));
                out.insert(
                    name.clone(),
                    McpConfig::Local {
                        command,
                        environment,
                        cwd: get_str(s, "cwd"),
                        enabled,
                        timeout_ms,
                    },
                );
            }
            "remote" | "http" | "sse" | "streamable-http" => {
                let Some(url) = get_str(s, "url").filter(|u| !u.is_empty()) else {
                    ctx.warn(format!("mcp.{name}: remote server requires `url`"));
                    continue;
                };
                out.insert(
                    name.clone(),
                    McpConfig::Remote {
                        url,
                        headers: str_map(s.get("headers")),
                        enabled,
                        timeout_ms,
                    },
                );
            }
            "" => {
                // Only `{enabled: ..}` toggles (for servers defined elsewhere) are silently ignored.
                if s.keys().any(|k| k != "enabled") {
                    ctx.warn(format!("mcp.{name}: missing `type` (local/remote)"));
                }
            }
            t => ctx.warn(format!("mcp.{name}: unknown type {t:?}")),
        }
    }
    out
}

fn commands(ctx: &mut Ctx, v: Option<&Value>) -> BTreeMap<String, CommandConfig> {
    let mut out = BTreeMap::new();
    let Some(Value::Object(m)) = v else {
        return out;
    };
    for (name, c) in m {
        let Value::Object(c) = c else {
            ctx.warn(format!("command.{name}: expected an object"));
            continue;
        };
        let Some(template) = get_str(c, "template") else {
            ctx.warn(format!("command.{name}: missing `template`"));
            continue;
        };
        let model = model_ref(ctx, c.get("model"), &format!("command.{name}"));
        out.insert(
            name.clone(),
            CommandConfig {
                name: name.clone(),
                template,
                description: get_str(c, "description").unwrap_or_default(),
                agent: get_str(c, "agent").filter(|s| !s.is_empty()),
                model,
            },
        );
    }
    out
}

// ---------------------------------------------------------------------------
// Agents
// ---------------------------------------------------------------------------

const AGENT_KEYS: &[&str] = &[
    "name",
    "description",
    "prompt",
    "model",
    "variant",
    "temperature",
    "top_p",
    "options",
    "permission",
    "tools",
    "disable",
    "disabled",
    "mode",
    "hidden",
    "color",
    "creatable",
    "can_spawn",
    "spawn",
    "delay",
    "steps",
    "maxSteps",
    "$schema",
];

fn agents(
    ctx: &mut Ctx,
    merged: &Map<String, Value>,
    mut perms: BTreeMap<String, PermissionRules>,
) -> BTreeMap<String, AgentConfig> {
    let mut out: BTreeMap<String, AgentConfig> = builtin::agents()
        .into_iter()
        .map(|a| (a.name.clone(), a))
        .collect();
    for key in ["mode", "agent"] {
        let Some(v) = merged.get(key) else { continue };
        let Value::Object(m) = v else {
            ctx.warn(format!("{key}: expected an object"));
            continue;
        };
        for (name, a) in m {
            let Value::Object(a) = a else {
                ctx.warn(format!("{key}.{name}: expected an object"));
                continue;
            };
            let disable = a
                .get("disable")
                .or_else(|| a.get("disabled"))
                .and_then(Value::as_bool)
                == Some(true);
            if disable {
                out.remove(name);
                continue;
            }
            let entry = out.entry(name.clone()).or_insert_with(|| AgentConfig {
                name: name.clone(),
                creatable: true,
                can_spawn: false,
                mode: if key == "mode" {
                    "primary".into()
                } else {
                    "all".into()
                },
                ..Default::default()
            });
            apply_agent(ctx, entry, a, &format!("{key}.{name}"));
            if key == "mode" {
                entry.mode = "primary".into();
            }
        }
    }
    for (name, a) in out.iter_mut() {
        if let Some(r) = perms.remove(name) {
            a.permission.extend(r);
        }
    }
    out
}

fn apply_agent(ctx: &mut Ctx, a: &mut AgentConfig, m: &Map<String, Value>, what: &str) {
    if let Some(s) = get_str(m, "description") {
        a.description = s;
    }
    if let Some(s) = get_str(m, "prompt") {
        a.prompt = s;
    }
    if m.contains_key("model") {
        a.model = model_ref(ctx, m.get("model"), what);
    }
    if let Some(s) = get_str(m, "variant") {
        a.variant = Some(s).filter(|s| !s.is_empty());
    }
    if let Some(x) = m.get("temperature").and_then(Value::as_f64) {
        a.temperature = Some(x);
    }
    if let Some(x) = m.get("top_p").and_then(Value::as_f64) {
        a.top_p = Some(x);
    }
    if let Some(Value::Object(o)) = m.get("options") {
        crate::load::deep_merge(&mut a.options, o);
    }
    for (k, v) in m {
        if !AGENT_KEYS.contains(&k.as_str()) {
            a.options.insert(k.clone(), v.clone());
        }
    }
    if let Some(s) = get_str(m, "mode") {
        a.mode = s;
    }
    if let Some(b) = m.get("hidden").and_then(Value::as_bool) {
        a.hidden = b;
    }
    if let Some(s) = get_str(m, "color") {
        a.color = Some(s);
    }
    if let Some(b) = m.get("creatable").and_then(Value::as_bool) {
        a.creatable = b;
    }
    if let Some(b) = m
        .get("can_spawn")
        .or_else(|| m.get("spawn"))
        .and_then(Value::as_bool)
    {
        a.can_spawn = b;
    }
    if m.contains_key("delay") {
        let d = delay_list(ctx, m.get("delay"), &format!("{what}.delay"));
        // Later keys win in `delay_for`, so appending keeps override semantics.
        for (k, v) in d {
            a.delay.retain(|(x, _)| x != &k);
            a.delay.push((k, v));
        }
    }
}
