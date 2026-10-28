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

use ref_proto::spec::{McpServerConfig, ModelSpec};
use ref_proto::{AgentEntry, CommandEntry, ModelEntry};
use serde::{Deserialize, Serialize};
use serde_json::Value;

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

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    /// Absolute path to SKILL.md
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Config {
    /// Merged raw JSON (for debugging / unknown keys).
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
    /// Max retry backoff seconds (default 600).
    pub retry_max_delay: Duration,
    /// Max tool output chars returned to the model (default 50_000).
    pub tool_output_limit: usize,
    // providers: implementation-defined (resolved through `resolve_model` / `models`)
}

impl Config {
    /// Load and merge all config files relevant to `cwd`.
    pub fn load(cwd: &Path) -> anyhow::Result<Config> {
        let _ = cwd;
        todo!()
    }

    /// All selectable models (configured providers' models + models.dev catalog models for
    /// providers with credentials), sorted by provider then name.
    pub fn models(&self) -> Vec<ModelEntry> {
        todo!()
    }

    /// Resolve `provider/model` (+ optional variant) to a callable spec. Fills missing metadata
    /// (limits, cost, modalities, env key) from the models.dev cache (`~/.cache/opencode/models.json`).
    pub fn resolve_model(&self, id: &str, variant: Option<&str>) -> anyhow::Result<ModelSpec> {
        let _ = (id, variant);
        todo!()
    }

    /// The model to use for an agent: agent.model, else config `model`, else first available.
    pub fn default_model_for(&self, agent: &str) -> Option<(String, Option<String>)> {
        let _ = agent;
        todo!()
    }

    pub fn agent_entries(&self) -> Vec<AgentEntry> {
        todo!()
    }

    pub fn command_entries(&self) -> Vec<CommandEntry> {
        todo!()
    }

    /// Permission for a tool call: agent rules override global rules. `subject` is the pattern
    /// subject (bash command, file path) when relevant.
    pub fn permission(&self, agent: &str, tool: &str, subject: Option<&str>) -> Action {
        let _ = (agent, tool, subject);
        todo!()
    }

    /// Pre-execution delay for a tool: agent `delay` overrides global `delay`. Supports `*` key.
    pub fn delay(&self, agent: &str, tool: &str) -> Duration {
        let _ = (agent, tool);
        todo!()
    }

    /// Recommended compaction token count for a model.
    pub fn compact_at(&self, model: &ModelSpec) -> u64 {
        let _ = model;
        todo!()
    }
}

/// Instruction files for a working directory, rendered as `Instructions from: <path>\n<content>`.
/// AGENTS.md (then CLAUDE.md fallback) walking up from cwd to the git root / home, global
/// `~/.config/opencode/AGENTS.md` (or `~/.claude/CLAUDE.md`), plus `instructions` globs.
pub fn instructions(config: &Config, cwd: &Path) -> Vec<(PathBuf, String)> {
    let _ = (config, cwd);
    todo!()
}

/// Discover skills (SKILL.md with name/description frontmatter).
pub fn skills(config: &Config, cwd: &Path) -> Vec<Skill> {
    let _ = (config, cwd);
    todo!()
}

/// Data directory for sessions etc: `$REF_DATA_DIR` or `~/.local/share/ref`.
pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("REF_DATA_DIR") {
        return PathBuf::from(d);
    }
    dirs::data_dir().unwrap_or_else(|| PathBuf::from("/tmp")).join("ref")
}
