//! Configuration: opencode-compatible config files plus hernees extensions.
//!
//! Sources (lowest -> highest priority, deep-merged):
//!   1. `~/.config/opencode/opencode.json`, `opencode.jsonc`
//!   2. `~/.config/hernees/hernees.json`, `hernees.jsonc`
//!   3. `$OPENCODE_CONFIG`, `$HERNEES_CONFIG` (explicit files)
//!   4. project walk from git root (or `/`-most ancestor) down to cwd: `opencode.json(c)`,
//!      `.opencode/opencode.json(c)`, `hernees.json(c)`, `.hernees/hernees.json(c)` — nearest wins.
//!
//! JSONC (comments + trailing commas) supported. `{env:VAR}` and `{file:path}` substituted.
//! Agents also load from `{agent,agents}/*.md` in `~/.config/{opencode,hernees}/` and project
//! `.opencode/`, `.hernees/` dirs (frontmatter fields + body prompt); `{mode,modes}/*.md` are
//! primary agents; `{command,commands}/**/*.md` are commands (body = template).
//!
//! hernees-only keys: top-level `delay: {tool-glob: seconds}`, `compaction: {ratio, auto}`,
//! `retry: {max_delay, initial_delay}`; agent `spawnable`, `manage_sessions`, `delay`;
//! model `limit.compact` (absolute compaction threshold in tokens).

use hn_llm::ModelSpec;
use hn_proto::{AgentInfo, CommandInfo, ModelInfo, ModelRef};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

mod instructions;
pub mod jsonc;
mod load;
pub mod markdown;
mod model;
mod permission;
mod skills;
#[cfg(test)]
mod tests_load;

pub use model::Catalog;
pub use permission::{wildcard, Action, PermissionRules};
pub use skills::{skill_content, skills_prompt, Skill};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ProviderConfig {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub npm: Option<String>,
    /// Base URL shorthand.
    #[serde(default)]
    pub api: Option<String>,
    /// Env var names that may hold the API key.
    #[serde(default)]
    pub env: Vec<String>,
    /// apiKey, baseURL, headers, body, setCacheKey, timeout ... + passthrough.
    #[serde(default)]
    pub options: serde_json::Map<String, Value>,
    #[serde(default)]
    pub models: IndexMap<String, ModelConfig>,
    #[serde(default)]
    pub whitelist: Option<Vec<String>>,
    #[serde(default)]
    pub blacklist: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Limit {
    #[serde(default)]
    pub context: Option<u64>,
    #[serde(default)]
    pub input: Option<u64>,
    #[serde(default)]
    pub output: Option<u64>,
    /// hernees: recommended compaction threshold in tokens (absolute) for this model.
    #[serde(default)]
    pub compact: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Modalities {
    #[serde(default)]
    pub input: Vec<String>,
    #[serde(default)]
    pub output: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ModelConfig {
    /// Real model id sent to the API (defaults to the map key).
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub attachment: Option<bool>,
    #[serde(default)]
    pub reasoning: Option<bool>,
    /// Legacy alias some configs use.
    #[serde(default)]
    pub thinking: Option<bool>,
    #[serde(default)]
    pub tool_call: Option<bool>,
    #[serde(default)]
    pub temperature: Option<bool>,
    /// true | "reasoning_content" | {"field": "..."}
    #[serde(default)]
    pub interleaved: Option<Value>,
    #[serde(default)]
    pub limit: Option<Limit>,
    #[serde(default)]
    pub modalities: Option<Modalities>,
    #[serde(default)]
    pub options: serde_json::Map<String, Value>,
    #[serde(default)]
    pub headers: IndexMap<String, String>,
    /// Variant name -> options overlay ({"disabled": true} hides it).
    #[serde(default)]
    pub variants: IndexMap<String, serde_json::Map<String, Value>>,
    /// Per-model {npm, api} override.
    #[serde(default)]
    pub provider: Option<ModelProviderOverride>,
    #[serde(default)]
    pub cost: Option<Value>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ModelProviderOverride {
    #[serde(default)]
    pub npm: Option<String>,
    #[serde(default)]
    pub api: Option<String>,
}

/// Per-tool delay configuration: tool name (glob allowed, e.g. "bash", "mcp_*", "*") -> seconds.
pub type DelayConfig = IndexMap<String, f64>;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AgentConfig {
    #[serde(default)]
    pub description: Option<String>,
    /// System prompt (appended after the tiny built-in prompt).
    #[serde(default)]
    pub prompt: Option<String>,
    /// "provider/model[#variant]"
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(default)]
    pub temperature: Option<f64>,
    /// opencode: primary | subagent | all. Informational.
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub disable: Option<bool>,
    #[serde(default)]
    pub hidden: Option<bool>,
    #[serde(default)]
    pub color: Option<String>,
    /// opencode-style permission (string or object). Overrides global per key.
    #[serde(default)]
    pub permission: Option<Value>,
    /// Deprecated opencode `tools: {name: bool}` -> allow/deny.
    #[serde(default)]
    pub tools: Option<IndexMap<String, bool>>,
    /// hernees: may be created by other sessions via `create_session`. Default false
    /// (true for opencode `mode: subagent|all` agents that have a description).
    #[serde(default)]
    pub spawnable: Option<bool>,
    /// hernees: may create/close other sessions. Default false.
    #[serde(default)]
    pub manage_sessions: Option<bool>,
    /// hernees: per-tool pre-execution delay in seconds. Overrides global.
    #[serde(default)]
    pub delay: Option<DelayConfig>,
    /// Extra provider options merged into the model options (e.g. reasoningEffort).
    #[serde(default)]
    pub options: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpConfig {
    Local {
        command: Vec<String>,
        #[serde(default)]
        environment: IndexMap<String, String>,
        #[serde(default)]
        cwd: Option<String>,
        #[serde(default)]
        enabled: Option<bool>,
        #[serde(default)]
        timeout: Option<u64>,
    },
    Remote {
        url: String,
        #[serde(default)]
        headers: IndexMap<String, String>,
        #[serde(default)]
        enabled: Option<bool>,
        #[serde(default)]
        timeout: Option<u64>,
        #[serde(default)]
        oauth: Option<Value>,
    },
}

impl McpConfig {
    pub fn enabled(&self) -> bool {
        match self {
            McpConfig::Local { enabled, .. } | McpConfig::Remote { enabled, .. } => enabled.unwrap_or(true),
        }
    }
    pub fn timeout(&self) -> Option<u64> {
        match self {
            McpConfig::Local { timeout, .. } | McpConfig::Remote { timeout, .. } => *timeout,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct CommandConfig {
    pub template: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CompactionConfig {
    /// Recommended compaction threshold as a fraction of the context window (default 0.8).
    #[serde(default = "default_ratio")]
    pub ratio: f64,
    /// Auto-compact (LLM summary) when context reaches the hard limit / overflows. Default true.
    #[serde(default = "default_true")]
    pub auto: bool,
}

fn default_ratio() -> f64 {
    0.8
}
fn default_true() -> bool {
    true
}

impl Default for CompactionConfig {
    fn default() -> Self {
        CompactionConfig { ratio: 0.8, auto: true }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RetryConfig {
    /// Max backoff seconds (default 600). Retries never stop on retryable errors.
    #[serde(default = "default_max_delay")]
    pub max_delay: u64,
    #[serde(default = "default_initial_delay")]
    pub initial_delay: f64,
}
fn default_max_delay() -> u64 {
    600
}
fn default_initial_delay() -> f64 {
    2.0
}
impl Default for RetryConfig {
    fn default() -> Self {
        RetryConfig { max_delay: 600, initial_delay: 2.0 }
    }
}

/// The merged configuration for one working directory.
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub providers: IndexMap<String, ProviderConfig>,
    pub model: Option<ModelRef>,
    pub small_model: Option<ModelRef>,
    pub default_agent: String,
    /// Includes built-in `build` agent if not configured.
    pub agents: IndexMap<String, AgentConfig>,
    pub mcp: IndexMap<String, McpConfig>,
    pub permission: PermissionRules,
    pub delay: DelayConfig,
    pub instructions: Vec<String>,
    pub commands: IndexMap<String, CommandConfig>,
    pub skills: Vec<Skill>,
    pub compaction: CompactionConfig,
    pub retry: RetryConfig,
    /// Raw merged JSON (for debugging / future keys).
    pub raw: Value,
    /// Files that were loaded, in order.
    pub sources: Vec<PathBuf>,
    /// Non-fatal problems (parse errors etc.).
    pub warnings: Vec<String>,
    /// Project root used for discovery (git root or cwd).
    pub root: PathBuf,
    /// models.dev catalog used to fill model metadata (empty if unavailable).
    pub catalog: std::sync::Arc<Catalog>,
    /// Pre-computed per-agent permission rules (`tools` + `permission`).
    pub agent_permission: IndexMap<String, PermissionRules>,
    pub(crate) dirs: load::Dirs,
}

impl Config {
    /// Load + merge all config sources for `cwd`. Never fails hard on a bad file: records a warning.
    pub fn load(cwd: &Path) -> anyhow::Result<Config> {
        load::load(cwd)
    }

    /// Resolve a model reference into a ready-to-use spec (variant merged, keys resolved from
    /// options/env, catalog limits filled from `~/.cache/opencode/models.json` when absent).
    pub fn resolve_model(&self, r: &ModelRef) -> anyhow::Result<ModelSpec> {
        model::resolve(self, r)
    }

    /// All selectable models.
    pub fn models(&self) -> Vec<ModelInfo> {
        model::list(self)
    }

    /// Default model for an agent: agent.model, else top-level `model`, else first configured model.
    pub fn default_model(&self, agent: &str) -> Option<ModelRef> {
        model::default_for(self, agent)
    }

    pub fn agent(&self, name: &str) -> Option<&AgentConfig> {
        self.agents.get(name)
    }

    pub fn skill(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|s| s.name == name)
    }

    /// Agent names that may be spawned via `create_session`.
    pub fn spawnable_agents(&self) -> Vec<&str> {
        self.agents
            .iter()
            .filter(|(_, a)| !a.disable.unwrap_or(false) && agent_spawnable(a))
            .map(|(n, _)| n.as_str())
            .collect()
    }

    pub fn agent_infos(&self) -> Vec<AgentInfo> {
        self.agents
            .iter()
            .filter(|(_, a)| !a.disable.unwrap_or(false))
            .map(|(n, a)| AgentInfo {
                name: n.clone(),
                description: a.description.clone().unwrap_or_default(),
                model: a.model.as_deref().and_then(ModelRef::parse),
                spawnable: agent_spawnable(a),
                manage_sessions: a.manage_sessions.unwrap_or(false),
                color: a.color.clone(),
            })
            .collect()
    }

    /// Permission for a tool call. Agent rules override global; within a rule set the last
    /// matching pattern wins. Default: Allow.
    pub fn permission(&self, agent: &str, tool: &str, subject: &str) -> Action {
        permission::evaluate(self, agent, tool, subject)
    }

    /// Pre-execution delay for a tool (agent overrides global; glob patterns; default 0).
    pub fn delay(&self, agent: &str, tool: &str) -> Duration {
        permission::delay(self, agent, tool)
    }

    /// AGENTS.md / CLAUDE.md / `instructions` contents: Vec<(path, content)>, global first.
    pub fn instruction_files(&self, cwd: &Path) -> Vec<(PathBuf, String)> {
        instructions::collect(self, cwd)
    }

    /// (max_context, recommended_compact_at) for a resolved model.
    pub fn context_limits(&self, spec: &ModelSpec, model: &ModelRef) -> (u64, u64) {
        model::context_limits(self, spec, model)
    }

    /// Custom commands + skills, for `/` autocomplete.
    pub fn command_infos(&self) -> Vec<CommandInfo> {
        let mut v: Vec<CommandInfo> = self
            .commands
            .iter()
            .map(|(n, c)| CommandInfo {
                name: n.clone(),
                description: c.description.clone().unwrap_or_default(),
                source: "custom".into(),
            })
            .collect();
        for s in &self.skills {
            v.push(CommandInfo { name: s.name.clone(), description: s.description.clone(), source: "skill".into() });
        }
        v
    }
}

pub fn agent_spawnable(a: &AgentConfig) -> bool {
    a.spawnable.unwrap_or_else(|| {
        matches!(a.mode.as_deref(), Some("subagent")) && a.description.is_some()
    })
}

/// `~/.config/hernees`
pub fn hernees_config_dir() -> PathBuf {
    if let Ok(d) = std::env::var("HERNEES_CONFIG_DIR") {
        return d.into();
    }
    xdg_config().join("hernees")
}

/// `~/.config/opencode`
pub fn opencode_config_dir() -> PathBuf {
    if let Ok(d) = std::env::var("OPENCODE_CONFIG_DIR") {
        return d.into();
    }
    xdg_config().join("opencode")
}

fn xdg_config() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::{Path, PathBuf};

    /// Unique temp dir removed on drop.
    pub struct TempDir(PathBuf);
    impl TempDir {
        pub fn new(tag: &str) -> TempDir {
            use std::sync::atomic::{AtomicU32, Ordering};
            static N: AtomicU32 = AtomicU32::new(0);
            let p = std::env::temp_dir().join(format!(
                "hn-config-{tag}-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            TempDir(std::fs::canonicalize(&p).unwrap())
        }
        pub fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
