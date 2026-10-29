//! Configuration loading, compatible with opencode's `opencode.json(c)` format
//! (providers/models, agents, permission, mcp, instructions, skills, command)
//! plus ref-specific extensions.
//!
//! Public API in this file is a contract used by `ref-server`; keep it stable.
//!
//! # Files (merged in order, later wins; objects deep-merged, arrays replaced)
//! 1. `~/.config/opencode/opencode.json`, `opencode.jsonc`, `config.json`
//! 2. `~/.config/ref/ref.json`, `ref.jsonc`
//! 3. `$OPENCODE_CONFIG` file if set, `$REF_CONFIG` file if set
//! 4. For each directory from the filesystem root (or $HOME) down to `cwd`:
//!    `opencode.json(c)`, `.opencode/opencode.json(c)`, `ref.json(c)`, `.ref/ref.json(c)`
//!
//! JSONC: `//` and `/* */` comments and trailing commas are accepted.
//! Substitution in string values: `{env:VAR}` -> env value or "", `{file:path}` -> trimmed
//! file contents (`~` and relative paths resolved against the config file dir).
//!
//! Markdown agents: `{agent,agents,mode,modes}/**/*.md` under `~/.config/opencode`,
//! `~/.config/ref`, and every ancestor `.opencode` / `.ref` dir. YAML-ish frontmatter
//! (simple `key: value`, nested `permission:` maps one level deep), body = prompt,
//! file stem = agent name. Markdown commands: `{command,commands}/**/*.md` likewise.

use ref_proto::ModelRef;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

mod builtin;
mod jsonc;
mod load;
mod markdown;
mod parse;
mod permission;
#[cfg(test)]
mod tests;

pub use builtin::COMPACTION_PROMPT;
pub use jsonc::parse_jsonc;

/// Name of the hidden built-in agent that writes compaction summaries. It is not a
/// session agent: it cannot be selected for sessions and is not listed to frontends.
pub const COMPACTION_AGENT: &str = "compaction";
pub use load::load_with;
pub use markdown::split_frontmatter;
pub use permission::{Action, PermissionRules, Rule};

/// Fully resolved configuration (for one working directory).
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub providers: BTreeMap<String, ProviderConfig>,
    /// Agents by name (built-ins merged with user definitions; disabled ones removed).
    pub agents: BTreeMap<String, AgentConfig>,
    /// `default_agent` (falls back to "build").
    pub default_agent: String,
    /// Top-level `model` ("provider/model") — used when the agent has no model.
    pub model: Option<ModelRef>,
    /// Top-level `small_model` (used for title generation if set).
    pub small_model: Option<ModelRef>,
    /// Global permission rules (`permission` key).
    pub permission: PermissionRules,
    /// Global pre-execution delays in seconds, by tool-name glob (`delay` key, ref extension).
    /// e.g. `{"bash": 3, "delete": 5, "*": 0}`; last matching key wins.
    pub delay: Vec<(String, f64)>,
    pub mcp: BTreeMap<String, McpConfig>,
    /// `instructions`: extra instruction file paths/globs (relative to the config file dir or cwd).
    pub instructions: Vec<String>,
    /// `skills.paths` / `skills: [..]` extra skill directories.
    pub skill_paths: Vec<PathBuf>,
    /// User-defined slash commands (`command` key + markdown commands).
    pub commands: BTreeMap<String, CommandConfig>,
    pub compaction: CompactionConfig,
    /// Retry: max delay between attempts in seconds (default 600). Retries are unlimited.
    pub retry_max_delay: u64,
    /// `tui` key passthrough.
    pub tui: Value,
    /// Directories that were scanned for config (for skills / agents discovery).
    pub config_dirs: Vec<PathBuf>,
    /// Non-fatal problems found while loading (shown to the user).
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    pub npm: Option<String>,
    /// `api` field, if given, overrides the npm-based API choice:
    /// "chat" | "openai-chat" | "responses" | "openai-responses" | "anthropic" | "messages".
    pub api: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    /// `env: [..]` names of env vars to read the API key from (first non-empty wins).
    pub env: Vec<String>,
    pub headers: BTreeMap<String, String>,
    /// Remaining `options` keys (setCacheKey, timeout, chunkTimeout, ...).
    pub options: Map<String, Value>,
    pub models: BTreeMap<String, ModelConfig>,
}

#[derive(Debug, Clone, Default)]
pub struct ModelConfig {
    /// Key in `models` map.
    pub key: String,
    /// `id` field (wire model id), defaults to key.
    pub id: String,
    pub name: String,
    /// `limit.context` (0 = unknown).
    pub context: u64,
    /// `limit.output` (0 = unknown).
    pub output: u64,
    /// `limit.input` (0 = unknown).
    pub input: u64,
    /// Supports image input: `attachment == true` or `modalities.input` contains "image".
    pub images: bool,
    /// `reasoning` or `thinking` flag.
    pub reasoning: bool,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub headers: BTreeMap<String, String>,
    /// `options` (camelCase, opencode style).
    pub options: Map<String, Value>,
    /// `variants`: name -> options overlay (entries with `disabled: true` removed).
    pub variants: BTreeMap<String, Map<String, Value>>,
    /// ref extension: `limit.compact` absolute recommended compaction threshold (tokens).
    pub compact_at: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct AgentConfig {
    pub name: String,
    pub description: String,
    /// Agent prompt (system prompt prefix). Empty = no extra prompt.
    pub prompt: String,
    pub model: Option<ModelRef>,
    pub variant: Option<String>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    /// Extra provider options merged over the model options.
    pub options: Map<String, Value>,
    /// Agent-level permission rules (evaluated after global rules => higher priority).
    pub permission: PermissionRules,
    /// Agent-level delays (higher priority than global).
    pub delay: Vec<(String, f64)>,
    /// ref extension `creatable` (default true): may be created by other sessions.
    pub creatable: bool,
    /// ref extension `can_spawn` (default false for custom agents, true for "build"):
    /// may create / close other sessions.
    pub can_spawn: bool,
    pub color: Option<String>,
    /// opencode `mode` ("primary" | "subagent" | "all"); informational.
    pub mode: String,
    /// opencode `hidden`.
    pub hidden: bool,
}

#[derive(Debug, Clone)]
pub enum McpConfig {
    Local {
        command: Vec<String>,
        environment: BTreeMap<String, String>,
        cwd: Option<String>,
        enabled: bool,
        timeout_ms: Option<u64>,
    },
    Remote {
        url: String,
        headers: BTreeMap<String, String>,
        enabled: bool,
        timeout_ms: Option<u64>,
    },
}

impl McpConfig {
    pub fn enabled(&self) -> bool {
        match self {
            McpConfig::Local { enabled, .. } | McpConfig::Remote { enabled, .. } => *enabled,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CommandConfig {
    pub name: String,
    /// Template with `$ARGUMENTS`, `$1`..`$9` placeholders.
    pub template: String,
    pub description: String,
    pub agent: Option<String>,
    pub model: Option<ModelRef>,
}

#[derive(Debug, Clone)]
pub struct CompactionConfig {
    /// Automatically compact when context reaches `auto_ratio` of the window (default true).
    pub auto: bool,
    /// Recommended compaction point as fraction of context window (default 0.6) or absolute
    /// tokens if > 1.0. Reported to the model via the `context_window` tool.
    pub recommend: f64,
    /// Automatic compaction threshold as fraction (default 0.9) or absolute tokens if > 1.0.
    pub threshold: f64,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        CompactionConfig { auto: true, recommend: 0.6, threshold: 0.9 }
    }
}

impl CompactionConfig {
    fn resolve(v: f64, max: u64) -> u64 {
        if v > 1.0 { (v as u64).min(max.max(1)) } else { (max as f64 * v) as u64 }
    }
    pub fn recommend_tokens(&self, max: u64, model_override: Option<u64>) -> u64 {
        model_override.unwrap_or_else(|| Self::resolve(self.recommend, max))
    }
    pub fn auto_tokens(&self, max: u64) -> u64 {
        Self::resolve(self.threshold, max)
    }
}

impl Config {
    /// Load and merge all config files relevant to `cwd`.
    /// Never fails hard: problems are collected in `warnings`.
    pub fn load(cwd: &Path) -> Config {
        load::load(cwd)
    }

    /// Look up a model.
    pub fn model(&self, r: &ModelRef) -> Option<(&ProviderConfig, &ModelConfig)> {
        let p = self.providers.get(&r.provider)?;
        let m = p.models.get(&r.model)?;
        Some((p, m))
    }

    /// Default model for an agent: agent.model -> top-level model -> first configured model.
    pub fn default_model_for(&self, agent: &str) -> Option<ModelRef> {
        if let Some(a) = self.agents.get(agent) {
            if let Some(m) = &a.model {
                let mut m = m.clone();
                if m.variant.is_none() {
                    m.variant = a.variant.clone();
                }
                return Some(m);
            }
        }
        if let Some(m) = &self.model {
            return Some(m.clone());
        }
        for (pid, p) in &self.providers {
            if let Some((mid, _)) = p.models.iter().next() {
                return Some(ModelRef { provider: pid.clone(), model: mid.clone(), variant: None });
            }
        }
        None
    }

    /// Resolve the permission action for a tool call: global rules, then agent rules
    /// (agent wins), last match wins; default `Allow`.
    /// `subject` is the matchable string for the call (bash command, file path, or "").
    pub fn permission_for(&self, agent: &str, tool: &str, subject: &str) -> Action {
        let mut act = self.permission.evaluate(tool, subject);
        if let Some(a) = self.agents.get(agent) {
            if let Some(x) = a.permission.evaluate(tool, subject) {
                act = Some(x);
            }
        }
        act.unwrap_or(Action::Allow)
    }

    /// Whether a tool is fully disabled (its matching rule with subject "*" is Deny
    /// and there are no pattern-specific allow/ask rules) — such tools are not sent to the model.
    pub fn tool_disabled(&self, agent: &str, tool: &str) -> bool {
        let g = self.permission.disabled(tool);
        let a = self.agents.get(agent).and_then(|a| a.permission.disabled_opt(tool));
        a.unwrap_or(g)
    }

    /// Pre-execution delay in seconds: agent delay (if any key matches) else global.
    pub fn delay_for(&self, agent: &str, tool: &str) -> f64 {
        fn find(list: &[(String, f64)], tool: &str) -> Option<f64> {
            let mut r = None;
            for (pat, secs) in list {
                if permission::glob_match(pat, tool) {
                    r = Some(*secs);
                }
            }
            r
        }
        if let Some(a) = self.agents.get(agent) {
            if let Some(d) = find(&a.delay, tool) {
                return d;
            }
        }
        find(&self.delay, tool).unwrap_or(0.0)
    }
}
