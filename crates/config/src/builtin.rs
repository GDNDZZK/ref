//! Built-in agents and the small built-in provider catalog.

use crate::{AgentConfig, PermissionRules};
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

/// Built-in agents (before user definitions are merged in).
pub fn agents() -> Vec<AgentConfig> {
    vec![
        AgentConfig {
            name: "build".into(),
            description: "Default agent with full tool access.".into(),
            creatable: true,
            can_spawn: true,
            mode: "primary".into(),
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
            mode: "primary".into(),
            ..Default::default()
        },
        AgentConfig {
            name: "general".into(),
            description: "General-purpose agent for multi-step tasks; runs in its own session."
                .into(),
            creatable: true,
            can_spawn: false,
            mode: "subagent".into(),
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
            mode: "subagent".into(),
            ..Default::default()
        },
        AgentConfig {
            name: "compaction".into(),
            description:
                "Summarizes the conversation when the context is compacted (/compact, automatic)."
                    .into(),
            prompt: COMPACTION_PROMPT.into(),
            creatable: false,
            can_spawn: false,
            mode: "primary".into(),
            hidden: true,
            ..Default::default()
        },
    ]
}

/// Defaults for a known provider id: (npm, base_url, env var).
pub fn provider_defaults(id: &str) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match id {
        "anthropic" => (
            "@ai-sdk/anthropic",
            "https://api.anthropic.com/v1",
            "ANTHROPIC_API_KEY",
        ),
        "openai" => (
            "@ai-sdk/openai",
            "https://api.openai.com/v1",
            "OPENAI_API_KEY",
        ),
        "deepseek" => (
            "@ai-sdk/openai-compatible",
            "https://api.deepseek.com/v1",
            "DEEPSEEK_API_KEY",
        ),
        "openrouter" => (
            "@openrouter/ai-sdk-provider",
            "https://openrouter.ai/api/v1",
            "OPENROUTER_API_KEY",
        ),
        _ => return None,
    })
}

/// Built-in provider catalog: (id, env var, opencode-style provider config).
/// Only added when the env var is set and the provider is not configured.
pub fn catalog() -> Vec<(&'static str, &'static str, Value)> {
    let anthropic_thinking =
        json!({"high": {"thinking": {"type": "enabled", "budgetTokens": 16000}}});
    let openai_variants = {
        let v = |e: &str| json!({"reasoningEffort": e, "reasoningSummary": "auto", "include": ["reasoning.encrypted_content"]});
        json!({"low": v("low"), "medium": v("medium"), "high": v("high")})
    };
    let image = json!({"input": ["text", "image"], "output": ["text"]});
    vec![
        (
            "anthropic",
            "ANTHROPIC_API_KEY",
            json!({
                "name": "Anthropic",
                "npm": "@ai-sdk/anthropic",
                "env": ["ANTHROPIC_API_KEY"],
                "options": {"baseURL": "https://api.anthropic.com/v1"},
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
            "OPENAI_API_KEY",
            json!({
                "name": "OpenAI",
                "npm": "@ai-sdk/openai",
                "env": ["OPENAI_API_KEY"],
                "options": {"baseURL": "https://api.openai.com/v1"},
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
            "DEEPSEEK_API_KEY",
            json!({
                "name": "DeepSeek",
                "npm": "@ai-sdk/openai-compatible",
                "api": "chat",
                "env": ["DEEPSEEK_API_KEY"],
                "options": {"baseURL": "https://api.deepseek.com/v1"},
                "models": {
                    "deepseek-chat": {"name": "DeepSeek Chat", "limit": {"context": 128000, "output": 8192}},
                    "deepseek-reasoner": {"name": "DeepSeek Reasoner", "reasoning": true,
                        "limit": {"context": 128000, "output": 64000}}
                }
            }),
        ),
        (
            "openrouter",
            "OPENROUTER_API_KEY",
            json!({
                "name": "OpenRouter",
                "npm": "@openrouter/ai-sdk-provider",
                "api": "chat",
                "env": ["OPENROUTER_API_KEY"],
                "options": {"baseURL": "https://openrouter.ai/api/v1"},
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
