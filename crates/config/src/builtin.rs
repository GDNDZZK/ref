//! Built-in agents and the small built-in provider catalog.

use crate::permission::{ANY, Action, Rule};
use crate::{
    AgentConfig, COMPACTION_AGENT, Config, DEFAULT_AGENT, MODE_PRIMARY, MODE_SUBAGENT,
    PermissionRules,
};
use serde_json::{Value, json};

fn perm(v: Value) -> PermissionRules {
    PermissionRules::from_value(&v).0
}

const PLAN_PROMPT: &str = "You are in read-only planning mode. Analyze the code and discuss a plan with the user, \
but do not create, modify or delete files and do not run commands that change state. \
If the user asks you to implement changes, tell them to switch to the build agent.";

const EXPLORE_PROMPT: &str =
    "You are a file search specialist. You excel at thoroughly navigating and exploring codebases.

Guidelines:
- Your role is EXCLUSIVELY to search and analyze
- Parallelize independent tool calls for searches and reads whenever possible
- Adapt your search approach based on the thoroughness level specified by the caller
- Return file paths as absolute paths in your final response
- You MUST NOT create, modify, delete, move, or copy files, including temporary files and reports
- Shell commands MUST be read-only. NEVER run commands that write files or change system state

Complete the user's search request efficiently and report your findings clearly.";

/// Default system prompt of the hidden `compaction` agent.
pub const COMPACTION_PROMPT: &str = "You compact conversations between a user and a coding agent. When asked, summarize the conversation so far into a structured summary that will be given to another agent (with no other memory) to continue the work.

Use this format (omit sections that do not apply):

## Goal
- What the user is trying to accomplish.

## Instructions and constraints
- Requirements, preferences and scope boundaries stated by the user.

## Discoveries
- Facts learned that are needed to continue (exact paths, symbols, commands, error strings).

## Accomplished
- What has been completed.

## In progress
- What was being worked on when the summary was made, and its exact state.

## Next steps
1. Ordered list of next actions.

## Relevant files
- `path`: why it matters (at most 15, most important first).

Rules: be concise (terse single-line bullets), preserve exact identifiers, carry forward unanswered user requests verbatim, mention whether changes are uncommitted/committed. Do not continue the task and do not call tools. Reply with the summary only.";

/// The tool the model uses to ask the user questions.
const QUESTION_TOOL: &str = "question";
/// The agents that may ask by default, like opencode's: the primary ones.
const QUESTION_AGENTS: [&str; 2] = ["build", "plan"];

/// opencode's default for `question`: allowed for `build` and `plan`, denied (so the tool is not
/// even offered) for every other agent. A rule for it in the global `permission` applies to all
/// agents; a rule in an agent's own `permission` wins over both. Call after the agents are built.
pub(crate) fn question_defaults(cfg: &mut Config) {
    if cfg.permission.evaluate(QUESTION_TOOL, "").is_some() {
        return;
    }
    for (name, agent) in cfg.agents.iter_mut() {
        let action = if QUESTION_AGENTS.contains(&name.as_str()) {
            Action::Allow
        } else {
            Action::Deny
        };
        agent
            .permission
            .rules
            .insert(0, Rule::new(QUESTION_TOOL, ANY, action));
    }
}

/// Built-in agents (before user definitions are merged in).
pub(crate) fn agents() -> Vec<AgentConfig> {
    vec![
        AgentConfig {
            name: DEFAULT_AGENT.into(),
            description: "Default agent with full tool access.".into(),
            creatable: true,
            can_spawn: true,
            mode: MODE_PRIMARY.into(),
            ..Default::default()
        },
        AgentConfig {
            name: "plan".into(),
            description: "Read-only analysis and planning; cannot modify files.".into(),
            prompt: PLAN_PROMPT.into(),
            permission: perm(
                json!({"edit": "deny", "write": "deny", "delete": "deny", "bash": "ask"}),
            ),
            creatable: true,
            can_spawn: false,
            mode: MODE_PRIMARY.into(),
            ..Default::default()
        },
        AgentConfig {
            name: "general".into(),
            description: "General-purpose agent for multi-step tasks; runs in its own session."
                .into(),
            creatable: true,
            can_spawn: false,
            mode: MODE_SUBAGENT.into(),
            ..Default::default()
        },
        AgentConfig {
            name: "explore".into(),
            description:
                "Fast read-only agent for exploring codebases: finding files, searching code \
                          and answering questions about how the code works."
                    .into(),
            prompt: EXPLORE_PROMPT.into(),
            permission: perm(json!({"edit": "deny", "write": "deny", "delete": "deny"})),
            creatable: true,
            can_spawn: false,
            mode: MODE_SUBAGENT.into(),
            ..Default::default()
        },
        AgentConfig {
            name: COMPACTION_AGENT.into(),
            description:
                "Summarizes the conversation when the context is compacted (/compact, automatic)."
                    .into(),
            prompt: COMPACTION_PROMPT.into(),
            creatable: false,
            can_spawn: false,
            mode: MODE_PRIMARY.into(),
            hidden: true,
            ..Default::default()
        },
    ]
}

/// Facts about a built-in provider.
pub(crate) struct ProviderDefaults {
    id: &'static str,
    pub(crate) npm: &'static str,
    pub(crate) base_url: &'static str,
    pub(crate) env: &'static str,
    /// Whether the SDK package alone implies `env` for custom providers that use it.
    conventional_env: bool,
}

const PROVIDERS: &[ProviderDefaults] = &[
    ProviderDefaults {
        id: "anthropic",
        npm: "@ai-sdk/anthropic",
        base_url: "https://api.anthropic.com/v1",
        env: "ANTHROPIC_API_KEY",
        conventional_env: true,
    },
    ProviderDefaults {
        id: "openai",
        npm: "@ai-sdk/openai",
        base_url: "https://api.openai.com/v1",
        env: "OPENAI_API_KEY",
        conventional_env: true,
    },
    ProviderDefaults {
        id: "deepseek",
        npm: "@ai-sdk/openai-compatible",
        base_url: "https://api.deepseek.com/v1",
        env: "DEEPSEEK_API_KEY",
        conventional_env: false,
    },
    ProviderDefaults {
        id: "openrouter",
        npm: "@openrouter/ai-sdk-provider",
        base_url: "https://openrouter.ai/api/v1",
        env: "OPENROUTER_API_KEY",
        conventional_env: false,
    },
];

/// Defaults for a known provider id.
pub(crate) fn provider_defaults(id: &str) -> Option<&'static ProviderDefaults> {
    PROVIDERS.iter().find(|p| p.id == id)
}

/// The conventional API key variable of an SDK package (e.g. `@ai-sdk/anthropic`).
pub(crate) fn env_for_npm(npm: &str) -> Option<&'static str> {
    PROVIDERS
        .iter()
        .find(|p| p.conventional_env && p.npm == npm)
        .map(|p| p.env)
}

/// Built-in provider catalog: (id, env var, opencode-style provider config).
/// Only added when the env var is set and the provider is not configured.
pub(crate) fn catalog() -> Vec<(&'static str, &'static str, Value)> {
    let anthropic_thinking =
        json!({"high": {"thinking": {"type": "enabled", "budgetTokens": 16000}}});
    let openai_variants = {
        let v = |e: &str| json!({"reasoningEffort": e, "reasoningSummary": "auto", "include": ["reasoning.encrypted_content"]});
        json!({"low": v("low"), "medium": v("medium"), "high": v("high")})
    };
    let image = json!({"input": ["text", "image"], "output": ["text"]});
    let known = |id: &str| provider_defaults(id).expect("built-in provider");
    let (anthropic, openai, deepseek, openrouter) = (
        known("anthropic"),
        known("openai"),
        known("deepseek"),
        known("openrouter"),
    );
    vec![
        (
            "anthropic",
            anthropic.env,
            json!({
                "name": "Anthropic",
                "npm": anthropic.npm,
                "env": [anthropic.env],
                "options": {"baseURL": anthropic.base_url},
                "models": {
                    "claude-sonnet-4-5": {"name": "Claude Sonnet 4.5", "attachment": true, "reasoning": true,
                        "limit": {"context": 200000, "output": 64000}, "variants": anthropic_thinking.clone()},
                    "claude-opus-4-1": {"name": "Claude Opus 4.1", "attachment": true, "reasoning": true,
                        "limit": {"context": 200000, "output": 32000}, "variants": anthropic_thinking},
                    "claude-haiku-4-5": {"name": "Claude Haiku 4.5", "attachment": true,
                        "limit": {"context": 200000, "output": 64000}}
                }
            }),
        ),
        (
            "openai",
            openai.env,
            json!({
                "name": "OpenAI",
                "npm": openai.npm,
                "env": [openai.env],
                "options": {"baseURL": openai.base_url},
                "models": {
                    "gpt-5": {"name": "GPT-5", "attachment": true, "reasoning": true, "modalities": image.clone(),
                        "limit": {"context": 400000, "input": 272000, "output": 128000},
                        "options": {"store": false}, "variants": openai_variants.clone()},
                    "gpt-5-mini": {"name": "GPT-5 mini", "attachment": true, "reasoning": true, "modalities": image,
                        "limit": {"context": 400000, "input": 272000, "output": 128000},
                        "options": {"store": false}, "variants": openai_variants}
                }
            }),
        ),
        (
            "deepseek",
            deepseek.env,
            json!({
                "name": "DeepSeek",
                "npm": deepseek.npm,
                "api": "chat",
                "env": [deepseek.env],
                "options": {"baseURL": deepseek.base_url},
                "models": {
                    "deepseek-chat": {"name": "DeepSeek Chat", "limit": {"context": 128000, "output": 8192}},
                    "deepseek-reasoner": {"name": "DeepSeek Reasoner", "reasoning": true,
                        "limit": {"context": 128000, "output": 64000}}
                }
            }),
        ),
        (
            "openrouter",
            openrouter.env,
            json!({
                "name": "OpenRouter",
                "npm": openrouter.npm,
                "api": "chat",
                "env": [openrouter.env],
                "options": {"baseURL": openrouter.base_url},
                "models": {
                    "anthropic/claude-sonnet-4.5": {"name": "Claude Sonnet 4.5", "attachment": true, "reasoning": true,
                        "limit": {"context": 1000000, "output": 64000}},
                    "openai/gpt-5": {"name": "GPT-5", "attachment": true, "reasoning": true,
                        "limit": {"context": 400000, "output": 128000}}
                }
            }),
        ),
    ]
}
