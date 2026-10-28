//! Configuration loading, opencode-compatible.
//!
//! Sources (lowest → highest priority, deep-merged):
//!   1. `~/.config/opencode/opencode.json[c]` (or `$OPENCODE_CONFIG_DIR`, `$XDG_CONFIG_HOME/opencode`)
//!   2. `$OPENCODE_CONFIG` file
//!   3. project `opencode.json[c]` and `.opencode/opencode.json[c]` walking up from cwd to git root
//!      (farthest first)
//!   4. `~/.config/ref/ref.json[c]` and project `.ref/ref.json[c]` (ref-specific extensions,
//!      same schema + extra keys)
//!
//! Supports JSONC (comments, trailing commas), `{env:VAR}` and `{file:path}` substitution.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::bail;
use ref_proto::spec::{McpServerConfig, ModelSpec};
use ref_proto::{AgentEntry, CommandEntry, ModelEntry};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

mod agents;
pub mod discover;
pub mod jsonc;
pub mod perm;
mod provider;

use provider::ProviderInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Allow,
    Ask,
    Deny,
}

/// opencode permission value: `"allow"` or `{ "pattern": "allow", ... }` (last match wins).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PermissionRule {
    Action(Action),
    Patterns(Vec<(String, Action)>),
}

/// Permission rules: tool name (or `*`) → rule. Tool keys use opencode names where they differ
/// (`edit` covers edit/write/delete, `glob`/`list` map to find).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Permissions {
    /// Ordered; later entries override earlier ones.
    pub rules: Vec<(String, PermissionRule)>,
}

#[derive(Debug, Clone, Default)]
pub struct AgentConfig {
    pub name: String,
    pub description: String,
    /// Agent prompt (appended to the base system prompt).
    pub prompt: String,
    /// `provider/model`
    pub model: Option<String>,
    pub variant: Option<String>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub color: Option<String>,
    /// May be created by other sessions via create_session (ref extension `spawnable`, default true;
    /// opencode `mode: "primary"` → false unless set explicitly).
    pub spawnable: bool,
    /// May create/close other sessions (ref extension `can_spawn`, default true).
    pub can_spawn: bool,
    pub hidden: bool,
    pub permission: Permissions,
    /// Per-tool pre-execution delay (ref extension `delay`: `{ "bash": 3 }` seconds).
    pub delay: BTreeMap<String, f64>,
    /// Extra model options from unknown agent keys (opencode semantics).
    pub options: serde_json::Map<String, Value>,
    /// opencode `mode`: `primary`, `subagent` or `all` (default).
    pub mode: String,
}

#[derive(Debug, Clone)]
pub struct CompactionConfig {
    /// Auto-compact when the context is about to overflow (default true).
    pub auto: bool,
    /// Recommended compaction point: ratio of the context window if <= 1.0, else absolute tokens.
    /// Default 0.8. A model's `limit.compact` overrides it.
    pub threshold: f64,
    /// Tokens kept free for output when deciding on auto compaction (default min(max_output, 32000)).
    pub reserved: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct CommandConfig {
    pub name: String,
    pub description: String,
    /// Template with `$ARGUMENTS`, `$1`..`$9`.
    pub template: String,
    pub agent: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    /// Absolute path to SKILL.md
    pub path: PathBuf,
}

/// Filesystem locations and environment used for loading. `ConfigPaths::from_env()` uses the real
/// user dirs; tests construct it explicitly for hermetic runs.
#[derive(Clone, Default)]
pub struct ConfigPaths {
    pub home: PathBuf,
    /// opencode global config dir (`~/.config/opencode`).
    pub global_dir: PathBuf,
    /// ref global config dir (`~/.config/ref`).
    pub ref_global_dir: PathBuf,
    /// models.dev cache (`~/.cache/opencode/models.json`).
    pub catalog_path: PathBuf,
    /// opencode credentials (`~/.local/share/opencode/auth.json`).
    pub auth_path: PathBuf,
    /// `$OPENCODE_CONFIG`
    pub explicit_config: Option<PathBuf>,
    /// `$OPENCODE_CONFIG_CONTENT`
    pub config_content: Option<String>,
    /// Skip project-level opencode config (`$OPENCODE_DISABLE_PROJECT_CONFIG`).
    pub disable_project: bool,
    /// Environment used for `{env:..}` substitution and provider API key lookup.
    pub env: BTreeMap<String, String>,
}

impl std::fmt::Debug for ConfigPaths {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `env` and `config_content` may contain secrets.
        f.debug_struct("ConfigPaths")
            .field("home", &self.home)
            .field("global_dir", &self.global_dir)
            .field("ref_global_dir", &self.ref_global_dir)
            .field("catalog_path", &self.catalog_path)
            .field("auth_path", &self.auth_path)
            .field("explicit_config", &self.explicit_config)
            .finish_non_exhaustive()
    }
}

impl ConfigPaths {
    /// Real paths from the process environment (opencode conventions).
    pub fn from_env() -> ConfigPaths {
        let env: BTreeMap<String, String> = std::env::vars().collect();
        let get = |k: &str| env.get(k).filter(|v| !v.is_empty()).cloned();
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        let xdg = |var: &str, default: &str| {
            get(var)
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(default))
        };
        let config_home = xdg("XDG_CONFIG_HOME", ".config");
        let global_dir = get("OPENCODE_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| config_home.join("opencode"));
        let ref_global_dir = get("REF_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| config_home.join("ref"));
        let catalog_path = get("OPENCODE_MODELS_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| xdg("XDG_CACHE_HOME", ".cache").join("opencode/models.json"));
        let auth_path = xdg("XDG_DATA_HOME", ".local/share").join("opencode/auth.json");
        let truthy = |k: &str| get(k).is_some_and(|v| v != "0" && v != "false");
        ConfigPaths {
            explicit_config: get("OPENCODE_CONFIG").map(PathBuf::from),
            config_content: get("OPENCODE_CONFIG_CONTENT"),
            disable_project: truthy("OPENCODE_DISABLE_PROJECT_CONFIG")
                || truthy("OPENCODE_CONFIG_PROJECT_DISABLE"),
            home,
            global_dir,
            ref_global_dir,
            catalog_path,
            auth_path,
            env,
        }
    }
}

#[derive(Clone)]
pub struct Config {
    /// Merged raw JSON (for debugging / unknown keys). May contain secrets: never log it.
    pub raw: Value,
    /// Default model `provider/model` (config `model`), if any.
    pub model: Option<String>,
    pub small_model: Option<String>,
    /// Built-ins (`build`, `plan`) merged with configured agents; disabled ones removed.
    pub agents: BTreeMap<String, AgentConfig>,
    /// Config `default_agent`, else `build`.
    pub default_agent: String,
    /// Global permissions. Default: allow everything.
    pub permission: Permissions,
    /// Global per-tool delay seconds.
    pub delay: BTreeMap<String, f64>,
    pub mcp: BTreeMap<String, McpServerConfig>,
    /// `instructions` entries (paths / globs).
    pub instructions: Vec<String>,
    /// Extra skill paths (`skills` config).
    pub skill_paths: Vec<String>,
    pub compaction: CompactionConfig,
    pub commands: BTreeMap<String, CommandConfig>,
    /// Max retry backoff seconds (default 600; ref key `retry_max_delay`).
    pub retry_max_delay: Duration,
    /// Max tool output chars returned to the model (default 50_000; ref key `tool_output_limit`).
    pub tool_output_limit: usize,
    /// Config files that were loaded, lowest priority first.
    pub sources: Vec<PathBuf>,
    /// Non-fatal problems (unparsable files, invalid entries) for display to the user.
    pub warnings: Vec<String>,
    /// Paths/env used to load this config.
    pub paths: ConfigPaths,
    providers: Vec<ProviderInfo>,
    /// models.dev catalog (only models of relevant providers parsed).
    catalog: Option<std::sync::Arc<provider::Catalog>>,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `raw` and provider options contain API keys: omit them.
        f.debug_struct("Config")
            .field("model", &self.model)
            .field("small_model", &self.small_model)
            .field("agents", &self.agents.keys().collect::<Vec<_>>())
            .field("default_agent", &self.default_agent)
            .field("permission", &self.permission)
            .field("delay", &self.delay)
            .field("mcp", &self.mcp.keys().collect::<Vec<_>>())
            .field("instructions", &self.instructions)
            .field("skill_paths", &self.skill_paths)
            .field("compaction", &self.compaction)
            .field("commands", &self.commands.keys().collect::<Vec<_>>())
            .field("providers", &self.providers)
            .field("sources", &self.sources)
            .field("warnings", &self.warnings)
            .finish_non_exhaustive()
    }
}

const CONFIG_NAMES: &[&str] = &["opencode.json", "opencode.jsonc"];
const REF_NAMES: &[&str] = &["ref.json", "ref.jsonc"];

struct Loader<'a> {
    paths: &'a ConfigPaths,
    raw: Value,
    sources: Vec<PathBuf>,
    warnings: Vec<String>,
}

impl Loader<'_> {
    fn add_text(&mut self, text: &str, base_dir: &Path, label: &Path) {
        let paths = self.paths;
        let getenv = |k: &str| paths.env.get(k).cloned();
        match jsonc::parse_config_text(text, base_dir, &paths.home, &getenv) {
            Ok(v) => {
                jsonc::merge_layer(&mut self.raw, v);
                self.sources.push(label.to_path_buf());
            }
            Err(e) => {
                let msg = format!("{}: {e:#}", label.display());
                tracing::warn!("config {msg}");
                self.warnings.push(msg);
            }
        }
    }

    fn add_file(&mut self, path: &Path) {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => {
                self.warnings.push(format!("{}: {e}", path.display()));
                return;
            }
        };
        let base = path.parent().unwrap_or(Path::new("/")).to_path_buf();
        self.add_text(&text, &base, path);
    }

    fn add_dir_files(&mut self, dir: &Path, names: &[&str]) {
        for n in names {
            self.add_file(&dir.join(n));
        }
    }

    /// Markdown agents/commands of a config directory.
    fn add_md(&mut self, dir: &Path) {
        if let Some(frag) = discover::md_fragment(dir) {
            jsonc::merge_layer(&mut self.raw, frag);
        }
    }
}

fn split_variant(id: &str) -> (&str, Option<&str>) {
    match id.split_once('#') {
        Some((m, v)) if !v.is_empty() => (m, Some(v)),
        Some((m, _)) => (m, None),
        None => (id, None),
    }
}

fn f64_of(v: Option<&Value>) -> Option<f64> {
    v.and_then(|x| x.as_f64())
}

impl Config {
    /// Load and merge all config files relevant to `cwd`.
    pub fn load(cwd: &Path) -> anyhow::Result<Config> {
        Config::load_with(&ConfigPaths::from_env(), cwd)
    }

    /// Load using explicit paths/environment (hermetic; used by tests).
    pub fn load_with(paths: &ConfigPaths, cwd: &Path) -> anyhow::Result<Config> {
        let cwd = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
        let mut l = Loader {
            paths,
            raw: Value::Object(Map::new()),
            sources: vec![],
            warnings: vec![],
        };
        // 1. global opencode dir
        l.add_dir_files(&paths.global_dir, CONFIG_NAMES);
        l.add_md(&paths.global_dir);
        // 2. $OPENCODE_CONFIG
        if let Some(p) = &paths.explicit_config {
            l.add_file(p);
        }
        // 3. project files, farthest first (global dirs excluded)
        let mut proj = discover::project_dirs(&cwd);
        proj.reverse();
        let is_global = |d: &Path| d == paths.global_dir || d == paths.home.join(".config");
        if !paths.disable_project {
            for d in &proj {
                if !is_global(d) {
                    l.add_dir_files(d, CONFIG_NAMES);
                }
            }
            for d in &proj {
                let oc = d.join(".opencode");
                if oc.is_dir() && !is_global(&oc) {
                    l.add_dir_files(&oc, CONFIG_NAMES);
                    l.add_md(&oc);
                }
            }
        }
        // $OPENCODE_CONFIG_CONTENT
        if let Some(text) = &paths.config_content {
            l.add_text(text, &cwd, Path::new("$OPENCODE_CONFIG_CONTENT"));
        }
        // 4. ref-specific
        l.add_dir_files(&paths.ref_global_dir, REF_NAMES);
        l.add_md(&paths.ref_global_dir);
        for d in &proj {
            let r = d.join(".ref");
            if r.is_dir() && r != paths.ref_global_dir {
                l.add_dir_files(&r, REF_NAMES);
                l.add_md(&r);
            }
        }
        let Loader {
            raw,
            sources,
            mut warnings,
            ..
        } = l;
        let auth = provider::load_auth(&paths.auth_path);
        let mut filter = provider::CatalogFilter::default();
        if let Some(Value::Object(p)) = raw.get("provider") {
            filter.keep.extend(p.keys().cloned());
        }
        filter.keep.extend(auth.keys().cloned());
        filter.env.extend(
            paths
                .env
                .iter()
                .filter(|(_, v)| !v.is_empty())
                .map(|(k, _)| k.clone()),
        );
        let catalog = provider::load_catalog(&paths.catalog_path, &filter);
        Ok(Config::from_raw(
            raw,
            sources,
            &mut warnings,
            paths,
            catalog,
            &auth,
        ))
    }

    fn from_raw(
        raw: Value,
        sources: Vec<PathBuf>,
        warnings: &mut Vec<String>,
        paths: &ConfigPaths,
        catalog: Option<std::sync::Arc<provider::Catalog>>,
        auth: &BTreeMap<String, String>,
    ) -> Config {
        let s = |k: &str| {
            raw.get(k)
                .and_then(|v| v.as_str())
                .map(String::from)
                .filter(|s| !s.is_empty())
        };
        let strs = |k: &str| -> Vec<String> {
            raw.get(k)
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        };

        let agents = agents::parse_agents(&raw);
        let default_agent = s("default_agent")
            .filter(|a| {
                let ok = agents.contains_key(a);
                if !ok {
                    warnings.push(format!("default_agent {a:?} not found; using build"));
                }
                ok
            })
            .unwrap_or_else(|| {
                if agents.contains_key("build") {
                    "build".into()
                } else {
                    agents
                        .keys()
                        .next()
                        .cloned()
                        .unwrap_or_else(|| "build".into())
                }
            });

        let mut permission = Permissions::default();
        if let Some(t) = raw.get("tools") {
            permission.rules.extend(perm::parse_tools(t).rules);
        }
        if let Some(p) = raw.get("permission") {
            permission.rules.extend(perm::parse_permission(p).rules);
        }

        let mut mcp = BTreeMap::new();
        if let Some(Value::Object(m)) = raw.get("mcp") {
            for (name, v) in m {
                if v.get("type").is_none() {
                    // `{enabled: bool}` override without a definition.
                    continue;
                }
                match serde_json::from_value::<McpServerConfig>(v.clone()) {
                    Ok(c) => {
                        mcp.insert(name.clone(), c);
                    }
                    Err(e) => warnings.push(format!("mcp {name}: {e}")),
                }
            }
        }

        let compaction = {
            let c = raw.get("compaction");
            let get = |k: &str| c.and_then(|c| c.get(k));
            CompactionConfig {
                auto: get("auto").and_then(|v| v.as_bool()).unwrap_or(true)
                    && !paths
                        .env
                        .get("OPENCODE_DISABLE_AUTOCOMPACT")
                        .is_some_and(|v| v == "1" || v == "true"),
                threshold: f64_of(get("threshold")).filter(|t| *t > 0.0).unwrap_or(0.8),
                reserved: f64_of(get("reserved"))
                    .filter(|r| *r >= 0.0)
                    .map(|r| r as u64),
            }
        };

        let skill_paths = raw
            .get("skills")
            .map(jsonc::skill_paths_of)
            .unwrap_or_default();

        let defs = provider::parse_providers(raw.get("provider"));
        let disabled = strs("disabled_providers");
        let enabled = raw
            .get("enabled_providers")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect::<Vec<_>>()
            });
        let providers = provider::build_providers(&provider::ProviderInputs {
            defs: &defs,
            catalog: catalog.as_deref(),
            auth,
            env: &paths.env,
            disabled: &disabled,
            enabled: enabled.as_deref(),
        });

        Config {
            model: s("model"),
            small_model: s("small_model"),
            default_agent,
            agents,
            permission,
            delay: raw.get("delay").map(perm::parse_delay).unwrap_or_default(),
            mcp,
            instructions: strs("instructions"),
            skill_paths,
            compaction,
            commands: agents::parse_commands(&raw),
            retry_max_delay: Duration::from_secs_f64(
                f64_of(raw.get("retry_max_delay"))
                    .filter(|v| *v > 0.0)
                    .unwrap_or(600.0),
            ),
            tool_output_limit: f64_of(raw.get("tool_output_limit"))
                .filter(|v| *v > 0.0)
                .map(|v| v as usize)
                .unwrap_or(50_000),
            sources,
            warnings: std::mem::take(warnings),
            paths: paths.clone(),
            providers,
            catalog,
            raw,
        }
    }

    /// All selectable models (configured providers' models + models.dev catalog models for
    /// providers with credentials). Providers in config order (catalog providers after, by id);
    /// models in config order, catalog-only models sorted by key.
    pub fn models(&self) -> Vec<ModelEntry> {
        let mut out = Vec::new();
        for p in &self.providers {
            for (key, m) in provider::listed_models(p) {
                let api = provider::model_api_kind(p, m);
                out.push(ModelEntry {
                    id: format!("{}/{}", p.id, key),
                    provider_name: p.name.clone(),
                    name: m.name.clone().unwrap_or_else(|| key.clone()),
                    variants: provider::variant_names(m, api),
                    context: m.context(),
                    image_input: m.image_input(),
                    reasoning: m.is_reasoning(),
                });
            }
        }
        out
    }

    /// Provider ids usable by ref, in priority order.
    pub fn provider_ids(&self) -> Vec<String> {
        self.providers.iter().map(|p| p.id.clone()).collect()
    }

    /// Resolve `provider/model` (+ optional variant, or `provider/model#variant`) to a callable
    /// spec. Missing metadata (limits, cost, modalities) comes from the models.dev cache.
    pub fn resolve_model(&self, id: &str, variant: Option<&str>) -> anyhow::Result<ModelSpec> {
        let (id, v2) = split_variant(id.trim());
        let variant = variant.filter(|v| !v.is_empty()).or(v2);
        let Some((pid, key)) = id.split_once('/') else {
            bail!("invalid model id {id:?}: expected provider/model");
        };
        let Some(p) = self.providers.iter().find(|p| p.id == pid) else {
            bail!("unknown provider {pid:?}{}", self.suggest(id));
        };
        let Some(m) = provider::find_model(p, key, self.catalog.as_deref()) else {
            bail!("unknown model {id:?}{}", self.suggest(id));
        };
        provider::build_spec(p, key, &m, variant)
    }

    fn suggest(&self, id: &str) -> String {
        let all = self.models();
        let lid = id.to_lowercase();
        let tail = lid.rsplit('/').next().unwrap_or(&lid).to_string();
        let mut scored: Vec<(usize, &str)> = all
            .iter()
            .map(|m| {
                let l = m.id.to_lowercase();
                let d = if l.contains(&tail) || lid.contains(&l) {
                    0
                } else {
                    provider::levenshtein(&lid, &l).min(provider::levenshtein(
                        &tail,
                        l.split_once('/').map_or(&l[..], |x| x.1),
                    ))
                };
                (d, m.id.as_str())
            })
            .filter(|(d, _)| *d <= 3.max(tail.len() / 3))
            .collect();
        scored.sort();
        scored.dedup_by(|a, b| a.1 == b.1);
        if scored.is_empty() {
            if all.is_empty() {
                return " (no models configured)".into();
            }
            return String::new();
        }
        let list: Vec<&str> = scored.iter().take(5).map(|x| x.1).collect();
        format!("; did you mean: {}", list.join(", "))
    }

    /// The model to use for an agent: agent.model, else config `model`, else first available.
    /// Returns (`provider/model`, variant).
    pub fn default_model_for(&self, agent: &str) -> Option<(String, Option<String>)> {
        if let Some(a) = self.agents.get(agent) {
            if let Some(m) = &a.model {
                let (m, v) = split_variant(m);
                return Some((m.to_string(), a.variant.clone().or(v.map(String::from))));
            }
        }
        if let Some(m) = &self.model {
            let (m, v) = split_variant(m);
            let variant = self
                .agents
                .get(agent)
                .and_then(|a| a.variant.clone())
                .or(v.map(String::from));
            return Some((m.to_string(), variant));
        }
        self.models().into_iter().next().map(|m| (m.id, None))
    }

    /// Resolve the model spec for an agent session, applying the agent's temperature/top_p/options.
    pub fn resolve_for_agent(
        &self,
        agent: &str,
        id: &str,
        variant: Option<&str>,
    ) -> anyhow::Result<ModelSpec> {
        let mut spec = self.resolve_model(id, variant)?;
        if let Some(a) = self.agents.get(agent) {
            self.apply_agent(a, &mut spec);
        }
        Ok(spec)
    }

    fn apply_agent(&self, a: &AgentConfig, spec: &mut ModelSpec) {
        let temp_ok = self
            .providers
            .iter()
            .find(|p| p.id == spec.provider_id)
            .and_then(|p| provider::find_model(p, &spec.model_key, self.catalog.as_deref()))
            .and_then(|m| m.temperature)
            != Some(false);
        if temp_ok {
            spec.temperature = a.temperature;
        }
        spec.top_p = a.top_p;
        if !a.options.is_empty() {
            jsonc::merge_map(&mut spec.options, &a.options);
        }
    }

    pub fn agent_entries(&self) -> Vec<AgentEntry> {
        self.agents
            .values()
            .map(|a| {
                let (model, variant) = self.default_model_for(&a.name).unwrap_or_default();
                AgentEntry {
                    name: a.name.clone(),
                    description: a.description.clone(),
                    model,
                    variant,
                    color: a.color.clone(),
                    spawnable: a.spawnable,
                    can_spawn: a.can_spawn,
                }
            })
            .collect()
    }

    pub fn command_entries(&self) -> Vec<CommandEntry> {
        self.commands
            .values()
            .map(|c| CommandEntry {
                name: c.name.clone(),
                description: c.description.clone(),
            })
            .collect()
    }

    /// Permission for a tool call: agent rules override global rules (last match wins).
    /// `subject` is the pattern subject (bash command, file path) when relevant. Default: Allow.
    pub fn permission(&self, agent: &str, tool: &str, subject: Option<&str>) -> Action {
        let mut sets = vec![&self.permission];
        if let Some(a) = self.agents.get(agent) {
            sets.push(&a.permission);
        }
        perm::evaluate(&sets, tool, subject, &self.paths.home).unwrap_or(Action::Allow)
    }

    /// Pre-execution delay for a tool: agent `delay` overrides global `delay`. Supports `*` key.
    pub fn delay(&self, agent: &str, tool: &str) -> Duration {
        let agent_d = self
            .agents
            .get(agent)
            .and_then(|a| perm::delay_lookup(&a.delay, tool));
        let secs = agent_d
            .or_else(|| perm::delay_lookup(&self.delay, tool))
            .unwrap_or(0.0);
        Duration::from_secs_f64(secs.clamp(0.0, 86_400.0))
    }

    /// Recommended compaction token count for a model.
    pub fn compact_at(&self, model: &ModelSpec) -> u64 {
        let explicit = self
            .providers
            .iter()
            .find(|p| p.id == model.provider_id)
            .and_then(|p| provider::find_model(p, &model.model_key, self.catalog.as_deref()))
            .and_then(|m| m.limit.and_then(|l| l.compact))
            .filter(|c| *c > 0.0);
        let at = match explicit {
            Some(c) => c as u64,
            None => {
                let t = self.compaction.threshold;
                if t <= 1.0 {
                    (model.context as f64 * t) as u64
                } else {
                    t as u64
                }
            }
        };
        at.min(model.context)
    }

    /// Tokens kept free for output when deciding on auto compaction.
    pub fn reserved_tokens(&self, model: &ModelSpec) -> u64 {
        self.compaction
            .reserved
            .unwrap_or_else(|| model.max_output.min(32_000))
    }
}

/// Instruction files for a working directory as (path, content); render each as
/// `Instructions from: <path>\n<content>`. Order: global (`~/.config/opencode/AGENTS.md`, else
/// `~/.claude/CLAUDE.md`), project files from git root down to cwd (AGENTS.md; CLAUDE.md only if no
/// AGENTS.md exists anywhere), then configured `instructions` paths/globs. Deduped by canonical
/// path; files larger than 200KB are skipped.
pub fn instructions(config: &Config, cwd: &Path) -> Vec<(PathBuf, String)> {
    let paths = &config.paths;
    let cwd = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let mut set = discover::InstructionSet::default();

    let global_agents = paths.global_dir.join("AGENTS.md");
    if !set.add(&global_agents) {
        set.add(&paths.home.join(".claude/CLAUDE.md"));
    }

    let mut proj = discover::project_dirs(&cwd);
    proj.reverse(); // farthest first, nearest last
    for name in ["AGENTS.md", "CLAUDE.md"] {
        let found: Vec<PathBuf> = proj
            .iter()
            .map(|d| d.join(name))
            .filter(|p| p.is_file())
            .collect();
        if !found.is_empty() {
            for p in found {
                set.add(&p);
            }
            break;
        }
    }

    for entry in &config.instructions {
        let entry = entry.trim();
        if entry.is_empty() || entry.starts_with("http://") || entry.starts_with("https://") {
            continue; // URLs are not fetched
        }
        let expanded = jsonc::expand_home(entry, &paths.home);
        if expanded.is_absolute() {
            let s = expanded.to_string_lossy();
            for f in discover::glob_files(Path::new("/"), s.trim_start_matches('/')) {
                set.add(&f);
            }
        } else {
            // Relative: search cwd, then each parent up to the project root.
            for d in proj.iter().rev() {
                for f in discover::glob_files(d, entry) {
                    set.add(&f);
                }
            }
        }
    }
    set.files
}

/// Discover skills (SKILL.md with name/description frontmatter). Later sources override
/// earlier ones with the same name; result sorted by name.
pub fn skills(config: &Config, cwd: &Path) -> Vec<Skill> {
    let paths = &config.paths;
    let home = &paths.home;
    let cwd = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let mut roots: Vec<(PathBuf, bool)> = vec![
        (paths.global_dir.join("skill"), false),
        (paths.global_dir.join("skills"), false),
        (home.join(".claude/skills"), false),
        (home.join(".agents/skills"), false),
        (paths.ref_global_dir.join("skills"), false),
    ];
    let mut proj = discover::project_dirs(&cwd);
    proj.reverse();
    for d in &proj {
        for sub in [
            ".claude/skills",
            ".agents/skills",
            ".opencode/skill",
            ".opencode/skills",
            ".ref/skills",
        ] {
            roots.push((d.join(sub), false));
        }
    }
    for p in &config.skill_paths {
        if p.starts_with("http://") || p.starts_with("https://") {
            continue;
        }
        let mut path = jsonc::expand_home(p, home);
        if path.is_relative() {
            path = cwd.join(path);
        }
        roots.push((path, true));
    }

    let mut seen_roots = std::collections::HashSet::new();
    let mut by_name: BTreeMap<String, Skill> = BTreeMap::new();
    for (root, include_root) in roots {
        let canon = std::fs::canonicalize(&root).unwrap_or(root.clone());
        if !canon.is_dir() || !seen_roots.insert(canon) {
            continue;
        }
        for file in discover::scan_skills(&root, include_root) {
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            let (fm, _) = discover::parse_frontmatter(&text);
            let dir_name = file
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let name = fm
                .get("name")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or(dir_name);
            let Some(description) = fm
                .get("description")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            let path = std::fs::canonicalize(&file).unwrap_or(file);
            by_name.insert(
                name.clone(),
                Skill {
                    name,
                    description,
                    path,
                },
            );
        }
    }
    by_name.into_values().collect()
}

/// Data directory for sessions etc: `$REF_DATA_DIR` or `~/.local/share/ref`.
pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("REF_DATA_DIR") {
        return PathBuf::from(d);
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("ref")
}

#[cfg(test)]
mod tests;
