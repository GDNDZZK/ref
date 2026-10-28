//! Config discovery + merge + typed extraction.

use crate::jsonc::{merge_doc, parse_jsonc, substitute};
use crate::*;
use serde_json::{json, Map};
use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};

/// Directories/files used for discovery. `Dirs::from_env()` for real use; tests build one by hand.
#[derive(Debug, Clone, Default)]
pub(crate) struct Dirs {
    pub home: Option<PathBuf>,
    /// `~/.config/opencode`
    pub opencode: PathBuf,
    /// `~/.config/hernees`
    pub hernees: PathBuf,
    /// `$OPENCODE_CONFIG`, `$HERNEES_CONFIG`
    pub explicit: Vec<PathBuf>,
    /// models.dev catalog (`~/.cache/opencode/models.json`).
    pub catalog: Option<PathBuf>,
}

impl Dirs {
    pub fn from_env() -> Dirs {
        let explicit = ["OPENCODE_CONFIG", "HERNEES_CONFIG"]
            .iter()
            .filter_map(std::env::var_os)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .collect();
        let catalog = std::env::var_os("HERNEES_MODELS_PATH")
            .or_else(|| std::env::var_os("OPENCODE_MODELS_PATH"))
            .map(PathBuf::from)
            .or_else(|| dirs::cache_dir().map(|c| c.join("opencode").join("models.json")));
        Dirs {
            home: dirs::home_dir(),
            opencode: opencode_config_dir(),
            hernees: hernees_config_dir(),
            explicit,
            catalog,
        }
    }

    pub fn global_dirs(&self) -> [&Path; 2] {
        [&self.opencode, &self.hernees]
    }
}

pub(crate) fn load(cwd: &Path) -> anyhow::Result<Config> {
    let env = |k: &str| std::env::var(k).ok();
    Ok(load_with(cwd, &Dirs::from_env(), &env))
}

/// Nearest ancestor (inclusive) containing `.git`.
pub(crate) fn git_root(cwd: &Path) -> Option<PathBuf> {
    cwd.ancestors().find(|d| d.join(".git").exists()).map(Path::to_path_buf)
}

/// Directories from the walk top (git root, else filesystem root) down to `cwd`, farthest first.
pub(crate) fn walk_dirs(cwd: &Path, stop: Option<&Path>) -> Vec<PathBuf> {
    let mut v = Vec::new();
    for d in cwd.ancestors() {
        v.push(d.to_path_buf());
        if Some(d) == stop {
            break;
        }
    }
    v.reverse();
    v
}

pub(crate) fn canonical(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

struct Loader<'a> {
    dirs: &'a Dirs,
    env: &'a dyn Fn(&str) -> Option<String>,
    raw: Value,
    sources: Vec<PathBuf>,
    warnings: Vec<String>,
    seen_files: HashSet<PathBuf>,
    seen_dirs: HashSet<PathBuf>,
}

impl Loader<'_> {
    fn file(&mut self, path: &Path) {
        if !path.is_file() || !self.seen_files.insert(canonical(path)) {
            return;
        }
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                self.warnings.push(format!("{}: {e}", path.display()));
                return;
            }
        };
        let dir = path.parent().unwrap_or(Path::new("/"));
        let text = substitute(&text, dir, self.dirs.home.as_deref(), self.env, &mut self.warnings);
        match parse_jsonc(&text) {
            Ok(v @ Value::Object(_)) => {
                merge_doc(&mut self.raw, v);
                self.sources.push(path.to_path_buf());
            }
            Ok(_) => self.warnings.push(format!("{}: top level is not an object", path.display())),
            Err(e) => self.warnings.push(format!("{}: invalid JSON: {e}", path.display())),
        }
    }

    /// A config directory source: markdown agents / modes / commands.
    fn dir(&mut self, dir: &Path) {
        if !dir.is_dir() || !self.seen_dirs.insert(canonical(dir)) {
            return;
        }
        let mut agents = Map::new();
        for (sub, recursive, primary) in
            [("agent", true, false), ("agents", true, false), ("mode", false, true), ("modes", false, true)]
        {
            for (name, file) in md_files(&dir.join(sub), recursive) {
                let Some(mut fm) = self.md(&file) else { continue };
                let body = fm.remove("\u{0}body").and_then(|b| b.as_str().map(str::to_string)).unwrap_or_default();
                if !body.trim().is_empty() {
                    fm.insert("prompt".into(), Value::String(body.trim().to_string()));
                }
                if primary {
                    fm.insert("mode".into(), json!("primary"));
                }
                agents.insert(name, Value::Object(fm));
                self.sources.push(file);
            }
        }
        let mut commands = Map::new();
        for sub in ["command", "commands"] {
            for (name, file) in md_files(&dir.join(sub), true) {
                let Some(mut fm) = self.md(&file) else { continue };
                let body = fm.remove("\u{0}body").and_then(|b| b.as_str().map(str::to_string)).unwrap_or_default();
                fm.insert("template".into(), Value::String(body.trim().to_string()));
                commands.insert(name, Value::Object(fm));
                self.sources.push(file);
            }
        }
        let mut doc = Map::new();
        if !agents.is_empty() {
            doc.insert("agent".into(), Value::Object(agents));
        }
        if !commands.is_empty() {
            doc.insert("command".into(), Value::Object(commands));
        }
        if !doc.is_empty() {
            merge_doc(&mut self.raw, Value::Object(doc));
        }
    }

    /// Frontmatter map with the body stored under a sentinel key.
    fn md(&mut self, file: &Path) -> Option<Map<String, Value>> {
        let text = match std::fs::read_to_string(file) {
            Ok(t) => t,
            Err(e) => {
                self.warnings.push(format!("{}: {e}", file.display()));
                return None;
            }
        };
        match markdown::parse(&text) {
            Ok((mut fm, body)) => {
                fm.insert("\u{0}body".into(), Value::String(body));
                Some(fm)
            }
            Err(e) => {
                self.warnings.push(format!("{}: invalid frontmatter: {e}", file.display()));
                None
            }
        }
    }
}

/// `*.md` files under `dir` (sorted), with their name = relative path minus `.md`.
fn md_files(dir: &Path, recursive: bool) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    fn rec(base: &Path, dir: &Path, recursive: bool, depth: usize, out: &mut Vec<(String, PathBuf)>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                if recursive && depth < 8 {
                    rec(base, &p, recursive, depth + 1, out);
                }
            } else if p.extension().is_some_and(|e| e == "md") {
                if let Ok(rel) = p.strip_prefix(base) {
                    let s = rel.to_string_lossy().replace('\\', "/");
                    let name = s.strip_suffix(".md").unwrap_or(&s).to_string();
                    out.push((name, p.clone()));
                }
            }
        }
    }
    rec(dir, dir, recursive, 0, &mut out);
    out
}

pub(crate) fn load_with(cwd: &Path, dirs: &Dirs, env: &dyn Fn(&str) -> Option<String>) -> Config {
    let cwd = canonical(cwd);
    let git = git_root(&cwd);
    let mut l = Loader {
        dirs,
        env,
        raw: Value::Object(Map::new()),
        sources: Vec::new(),
        warnings: Vec::new(),
        seen_files: HashSet::new(),
        seen_dirs: HashSet::new(),
    };

    // 1-2. Global config dirs.
    l.file(&dirs.opencode.join("opencode.json"));
    l.file(&dirs.opencode.join("opencode.jsonc"));
    l.dir(&dirs.opencode);
    l.file(&dirs.hernees.join("hernees.json"));
    l.file(&dirs.hernees.join("hernees.jsonc"));
    l.dir(&dirs.hernees);
    // 3. Explicit files.
    for f in &dirs.explicit {
        if !f.is_file() {
            l.warnings.push(format!("{}: config file not found", f.display()));
        }
        l.file(f);
    }
    // 4. Project walk, farthest first. Skipped entirely when cwd is a global config dir.
    let in_global = dirs.global_dirs().iter().any(|g| canonical(g) == cwd);
    let walk = if in_global { Vec::new() } else { walk_dirs(&cwd, git.as_deref()) };
    for d in &walk {
        l.file(&d.join("opencode.json"));
        l.file(&d.join("opencode.jsonc"));
        l.file(&d.join(".opencode/opencode.json"));
        l.file(&d.join(".opencode/opencode.jsonc"));
        l.dir(&d.join(".opencode"));
        l.file(&d.join("hernees.json"));
        l.file(&d.join("hernees.jsonc"));
        l.file(&d.join(".hernees/hernees.json"));
        l.file(&d.join(".hernees/hernees.jsonc"));
        l.dir(&d.join(".hernees"));
    }

    let Loader { raw, sources, mut warnings, .. } = l;
    let mut c = extract(raw, &mut warnings);
    c.sources = sources;
    c.dirs = dirs.clone();
    c.root = git.unwrap_or_else(|| cwd.clone());
    c.skills = skills::discover(dirs, &walk);
    if let Some(p) = &dirs.catalog {
        c.catalog = load_catalog(p, &mut warnings);
    }
    c.warnings = warnings;
    c
}

// ---------------------------------------------------------------------------
// Typed extraction
// ---------------------------------------------------------------------------

const AGENT_KEYS: &[&str] = &[
    "name",
    "description",
    "prompt",
    "model",
    "variant",
    "temperature",
    "top_p",
    "mode",
    "disable",
    "hidden",
    "color",
    "permission",
    "tools",
    "spawnable",
    "manage_sessions",
    "delay",
    "options",
    "steps",
    "maxSteps",
];

pub(crate) fn builtin_build() -> AgentConfig {
    AgentConfig {
        description: Some("Default agent with full tool access".into()),
        spawnable: Some(true),
        manage_sessions: Some(true),
        ..Default::default()
    }
}

fn typed<T: serde::de::DeserializeOwned>(v: Value, what: &str, warnings: &mut Vec<String>) -> Option<T> {
    match serde_json::from_value(v) {
        Ok(t) => Some(t),
        Err(e) => {
            warnings.push(format!("{what}: {e} (ignored)"));
            None
        }
    }
}

fn model_ref(v: Option<&Value>, what: &str, warnings: &mut Vec<String>) -> Option<ModelRef> {
    match v? {
        Value::String(s) if s.is_empty() => None,
        Value::String(s) => {
            let r = ModelRef::parse(s);
            if r.is_none() {
                warnings.push(format!("{what}: invalid model reference {s:?} (expected provider/model)"));
            }
            r
        }
        Value::Null => None,
        _ => {
            warnings.push(format!("{what}: expected a string"));
            None
        }
    }
}

/// Parse one provider entry; models are parsed individually so one bad model only drops itself.
pub(crate) fn parse_provider(id: &str, v: Value, warnings: &mut Vec<String>) -> Option<ProviderConfig> {
    let Value::Object(mut m) = v else {
        warnings.push(format!("provider.{id}: expected an object (ignored)"));
        return None;
    };
    let models = m.remove("models");
    let mut p: ProviderConfig = typed(Value::Object(m), &format!("provider.{id}"), warnings)?;
    match models {
        Some(Value::Object(models)) => {
            for (mid, mv) in models {
                if let Some(mc) = typed::<ModelConfig>(mv, &format!("provider.{id}.models.{mid}"), warnings) {
                    p.models.insert(mid, mc);
                }
            }
        }
        None | Some(Value::Null) => {}
        Some(_) => warnings.push(format!("provider.{id}.models: expected an object (ignored)")),
    }
    Some(p)
}

fn parse_agent(name: &str, v: Value, warnings: &mut Vec<String>) -> Option<AgentConfig> {
    let Value::Object(m) = v else {
        warnings.push(format!("agent.{name}: expected an object (ignored)"));
        return None;
    };
    // opencode: unknown keys are provider options (e.g. reasoningEffort).
    let mut known = Map::new();
    let mut extra = Map::new();
    for (k, v) in m {
        if AGENT_KEYS.contains(&k.as_str()) {
            known.insert(k, v);
        } else {
            extra.insert(k, v);
        }
    }
    let mut a: AgentConfig = typed(Value::Object(known), &format!("agent.{name}"), warnings)?;
    for (k, v) in extra {
        a.options.entry(k).or_insert(v);
    }
    if let Some(d) = a.delay.as_ref() {
        let bad: Vec<_> = d.iter().filter(|(_, v)| !(v.is_finite() && **v >= 0.0)).map(|(k, _)| k.clone()).collect();
        for k in bad {
            warnings.push(format!("agent.{name}.delay.{k}: expected a non-negative number of seconds"));
            a.delay.as_mut().unwrap().shift_remove(&k);
        }
    }
    Some(a)
}

pub(crate) fn extract(raw: Value, warnings: &mut Vec<String>) -> Config {
    let mut c = Config { default_agent: "build".into(), ..Default::default() };
    let Value::Object(obj) = &raw else {
        c.raw = raw;
        return c;
    };
    let get = |k: &str| obj.get(k).filter(|v| !v.is_null());

    // Providers.
    if let Some(v) = get("provider") {
        match v {
            Value::Object(m) => {
                for (id, pv) in m {
                    if let Some(p) = parse_provider(id, pv.clone(), warnings) {
                        c.providers.insert(id.clone(), p);
                    }
                }
            }
            _ => warnings.push("provider: expected an object (ignored)".into()),
        }
    }
    let str_list = |k: &str| -> Option<Vec<String>> {
        get(k)?.as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect())
    };
    if let Some(enabled) = str_list("enabled_providers") {
        c.providers.retain(|id, _| enabled.contains(id));
    }
    if let Some(disabled) = str_list("disabled_providers") {
        c.providers.retain(|id, _| !disabled.contains(id));
    }

    c.model = model_ref(get("model"), "model", warnings);
    c.small_model = model_ref(get("small_model"), "small_model", warnings);
    match get("default_agent") {
        Some(Value::String(s)) if !s.is_empty() => c.default_agent = s.clone(),
        Some(_) => warnings.push("default_agent: expected a string".into()),
        None => {}
    }

    // Agents: legacy top-level `mode` map first, then `agent`.
    let mut agent_raw = Map::new();
    for key in ["mode", "agent"] {
        match get(key) {
            Some(Value::Object(m)) => crate::jsonc::merge_maps(&mut agent_raw, m.clone()),
            Some(_) => warnings.push(format!("{key}: expected an object (ignored)")),
            None => {}
        }
    }
    let mut build = builtin_build();
    let mut agents = IndexMap::new();
    for (name, v) in agent_raw {
        if let Some(a) = parse_agent(&name, v, warnings) {
            agents.insert(name, a);
        }
    }
    if let Some(user) = agents.shift_remove("build") {
        // User overrides layer on top of the built-in defaults.
        let d = build;
        build = AgentConfig {
            description: user.description.or(d.description),
            spawnable: user.spawnable.or(d.spawnable),
            manage_sessions: user.manage_sessions.or(d.manage_sessions),
            ..user
        };
    }
    c.agents.insert("build".into(), build);
    c.agents.extend(agents);
    let home = dirs::home_dir();
    for (name, a) in &c.agents {
        let r = crate::permission::agent_rules(a, home.as_deref(), warnings, name);
        c.agent_permission.insert(name.clone(), r);
    }

    // MCP.
    if let Some(v) = get("mcp") {
        match v {
            Value::Object(m) => {
                for (name, sv) in m {
                    if let Some(s) = typed::<McpConfig>(sv.clone(), &format!("mcp.{name}"), warnings) {
                        c.mcp.insert(name.clone(), s);
                    }
                }
            }
            _ => warnings.push("mcp: expected an object (ignored)".into()),
        }
    }

    // Permission (+ deprecated top-level `tools`).
    let mut rules = PermissionRules::default();
    if let Some(Value::Object(t)) = get("tools") {
        let tools: IndexMap<String, bool> =
            t.iter().filter_map(|(k, v)| v.as_bool().map(|b| (k.clone(), b))).collect();
        rules = PermissionRules::from_tools(&tools);
    }
    if let Some(v) = get("permission") {
        let (r, errors) = PermissionRules::parse_lenient(v, home.as_deref());
        rules.rules.extend(r.rules);
        warnings.extend(errors.into_iter().map(|e| format!("permission: {e}")));
    }
    c.permission = rules;

    if let Some(v) = get("delay") {
        c.delay = parse_delay(v, "delay", warnings);
    }

    if let Some(v) = get("instructions") {
        match v {
            Value::Array(a) => {
                for s in a {
                    match s.as_str() {
                        Some(s) if !c.instructions.iter().any(|x| x == s) => c.instructions.push(s.into()),
                        Some(_) => {}
                        None => warnings.push("instructions: non-string entry ignored".into()),
                    }
                }
            }
            Value::String(s) => c.instructions.push(s.clone()),
            _ => warnings.push("instructions: expected an array".into()),
        }
    }

    if let Some(v) = get("command") {
        match v {
            Value::Object(m) => {
                for (name, cv) in m {
                    if let Some(cmd) = typed::<CommandConfig>(cv.clone(), &format!("command.{name}"), warnings) {
                        c.commands.insert(name.clone(), cmd);
                    }
                }
            }
            _ => warnings.push("command: expected an object (ignored)".into()),
        }
    }

    if let Some(v) = get("compaction") {
        if let Some(cc) = typed::<CompactionConfig>(v.clone(), "compaction", warnings) {
            c.compaction = cc;
        }
    }
    if let Some(v) = get("retry") {
        if let Some(r) = typed::<RetryConfig>(v.clone(), "retry", warnings) {
            c.retry = r;
        }
    }

    c.raw = raw;
    c
}

/// `{tool-glob: seconds}`; non-numeric / negative entries are dropped with a warning.
fn parse_delay(v: &Value, what: &str, warnings: &mut Vec<String>) -> DelayConfig {
    let mut out = DelayConfig::new();
    match v {
        Value::Object(m) => {
            for (k, s) in m {
                match s.as_f64() {
                    Some(f) if f.is_finite() && f >= 0.0 => {
                        out.insert(k.clone(), f);
                    }
                    _ => warnings.push(format!("{what}.{k}: expected a non-negative number of seconds")),
                }
            }
        }
        Value::Number(n) => {
            // Shorthand: one delay for every tool.
            if let Some(f) = n.as_f64().filter(|f| f.is_finite() && *f >= 0.0) {
                out.insert("*".into(), f);
            }
        }
        _ => warnings.push(format!("{what}: expected an object {{tool: seconds}}")),
    }
    out
}

// ---------------------------------------------------------------------------
// models.dev catalog
// ---------------------------------------------------------------------------

pub(crate) fn parse_catalog(v: Value) -> Catalog {
    let mut out = IndexMap::new();
    let mut ignored = Vec::new();
    if let Value::Object(m) = v {
        for (id, pv) in m {
            if let Some(p) = parse_provider(&id, pv, &mut ignored) {
                out.insert(id, p);
            }
        }
    }
    Catalog(out)
}

type CatalogCache = Mutex<Option<(PathBuf, std::time::SystemTime, u64, Arc<Catalog>)>>;

fn load_catalog(path: &Path, warnings: &mut Vec<String>) -> Arc<Catalog> {
    static CACHE: OnceLock<CatalogCache> = OnceLock::new();
    let Ok(meta) = std::fs::metadata(path) else { return Arc::default() };
    let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((p, t, len, cat)) = guard.as_ref() {
        if p == path && *t == mtime && *len == meta.len() {
            return cat.clone();
        }
    }
    let parsed = std::fs::read(path)
        .map_err(anyhow::Error::from)
        .and_then(|b| serde_json::from_slice::<Value>(&b).map_err(Into::into));
    let cat = match parsed {
        Ok(v) => Arc::new(parse_catalog(v)),
        Err(e) => {
            warnings.push(format!("{}: {e}", path.display()));
            Arc::default()
        }
    };
    *guard = Some((path.to_path_buf(), mtime, meta.len(), cat.clone()));
    cat
}
