//! System prompt, tool-schema `purpose` injection and compaction prompts.

use std::path::Path;

use ref_config::{Config, Skill};
use ref_proto::spec::ToolDef;
use ref_proto::{Message, MessageOrigin};
use serde_json::{json, Map, Value};

pub const PURPOSE_DESC: &str = "Brief purpose of this call, shown to the user (in the user's language)";

pub const BASE_PROMPT: &str = "You are ref, an AI coding agent running in the user's terminal. You operate on the user's machine through the provided tools and work autonomously until the task is done.\nEvery tool call takes a `purpose` argument: a few words, in the user's language, describing why you are making the call.";

pub struct PromptInputs<'a> {
    pub cwd: &'a str,
    /// Session creation time (ms) — the date stays fixed for cache stability.
    pub created: i64,
    pub session_id: &'a str,
    pub session_name: &'a str,
    pub agent: &'a str,
    pub config: &'a Config,
    pub instructions: &'a [(std::path::PathBuf, String)],
    pub skills: &'a [Skill],
}

pub fn env_block(cwd: &str, created: i64) -> String {
    let date = chrono::DateTime::from_timestamp_millis(created)
        .map(|t| t.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string())
        .unwrap_or_default();
    let git = if crate::util::is_git_repo(Path::new(cwd)) { "yes" } else { "no" };
    format!(
        "<env>\nWorking directory: {cwd}\nPlatform: {}\nDate: {date}\nGit repository: {git}\n</env>",
        std::env::consts::OS
    )
}

/// System prompt blocks in stable order: base+env, agent prompt, instructions, skills, session.
pub fn system_blocks(p: &PromptInputs) -> Vec<String> {
    let mut blocks = vec![format!("{BASE_PROMPT}\n{}", env_block(p.cwd, p.created))];
    let agent = p.config.agents.get(p.agent);
    if let Some(a) = agent {
        if !a.prompt.trim().is_empty() {
            blocks.push(a.prompt.trim().to_string());
        }
    }
    for (path, content) in p.instructions {
        blocks.push(format!("Instructions from: {}\n{}", path.display(), content.trim_end()));
    }
    if !p.skills.is_empty() {
        let mut s = String::from("Skills (load with the skill tool when relevant):");
        for sk in p.skills {
            s.push_str(&format!("\n- {}: {}", sk.name, sk.description));
        }
        blocks.push(s);
    }
    let mut s = format!(
        "You are session \"{}\" (id {}). Other sessions on this machine can message you; use list_sessions / send_message to coordinate.",
        p.session_name, p.session_id
    );
    if agent.is_none_or(|a| a.can_spawn) {
        let spawnable: Vec<_> = p.config.agents.values().filter(|a| a.spawnable && !a.hidden).collect();
        if !spawnable.is_empty() {
            s.push_str("\nAgents available for create_session:");
            for a in spawnable {
                let d = if a.description.is_empty() { "(no description)" } else { a.description.as_str() };
                s.push_str(&format!("\n- {}: {}", a.name, d));
            }
        }
    }
    blocks.push(s);
    blocks
}

/// Add the required `purpose` string property first in `properties` and `required`.
pub fn with_purpose(mut def: ToolDef) -> ToolDef {
    let mut params = match def.parameters {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    params.insert("type".into(), json!("object"));
    let mut props = Map::new();
    props.insert("purpose".into(), json!({"type": "string", "description": PURPOSE_DESC}));
    if let Some(Value::Object(old)) = params.remove("properties") {
        for (k, v) in old {
            if k != "purpose" {
                props.insert(k, v);
            }
        }
    }
    let mut req = vec![json!("purpose")];
    if let Some(Value::Array(old)) = params.remove("required") {
        req.extend(old.into_iter().filter(|v| v.as_str() != Some("purpose")));
    }
    // Rebuild preserving a stable key order: type, properties, required, then the rest.
    let mut out = Map::new();
    out.insert("type".into(), json!("object"));
    out.insert("properties".into(), Value::Object(props));
    out.insert("required".into(), Value::Array(req));
    for (k, v) in params {
        if k != "type" {
            out.insert(k, v);
        }
    }
    def.parameters = Value::Object(out);
    def
}

/// Split `purpose` out of tool input. Returns (purpose, input without purpose).
pub fn split_purpose(input: &Value) -> (String, Value) {
    match input {
        Value::Object(m) => {
            let mut m = m.clone();
            let purpose = match m.remove("purpose") {
                Some(Value::String(s)) => s,
                Some(Value::Null) | None => String::new(),
                Some(other) => other.to_string(),
            };
            (purpose, Value::Object(m))
        }
        other => (String::new(), other.clone()),
    }
}

/// Index of the first message of the current LLM context (the latest compaction summary).
pub fn context_start(messages: &[Message]) -> usize {
    messages.iter().rposition(|m| m.origin == MessageOrigin::Compaction).unwrap_or(0)
}

pub const COMPACTION_PREFIX: &str = "Summary of the conversation so far (context was compacted):\n\n";
pub const COMPACTION_CONTINUE: &str = "\n\nContinue the task from here.";

pub fn compaction_text(summary: &str, continue_task: bool) -> String {
    let mut s = format!("{COMPACTION_PREFIX}{}", summary.trim());
    if continue_task {
        s.push_str(COMPACTION_CONTINUE);
    }
    s
}

pub const SUMMARY_INSTRUCTION: &str = "You MUST summarize the conversation above into a structured summary that will be given to another agent to resume the work. Summarize only what the user and the assistant said and did; leave out setup instructions (AGENTS.md, environment).

Use exactly this template:

## Objective
## Requirements
## Decisions
## Work State
### Completed
### Active
### Blocked
## Next Move
## Relevant Files
(max 15, `path`: reason)
## Important Context

Rules: terse bullets; preserve exact paths, identifiers, commands and error messages; carry forward unanswered user requests and the current workflow state; do not mention the summarization itself.
Do not continue the task or call tools. Return only the structured summary.";

pub const TITLE_SYSTEM: &str = "Generate a short title (max 6 words) for this conversation in the same language as the user's message. Reply with the title only.";

/// Clean up a generated title.
pub fn clean_title(raw: &str) -> Option<String> {
    // Drop an inline reasoning block (`<think>…</think>`) some models put into the text.
    let raw = match raw.find("</think>") {
        Some(i) if raw.trim_start().starts_with("<think") => &raw[i + "</think>".len()..],
        _ => raw,
    };
    let line = raw
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("<think") && !l.starts_with("</think"))?;
    let t = line
        .trim_start_matches(['#', '*', ' '])
        .trim_start_matches("Title:")
        .trim()
        .trim_matches(|c: char| matches!(c, '"' | '\'' | '`' | '*' | '“' | '”' | '「' | '」'))
        .trim_end_matches(['.', '。'])
        .trim();
    if t.is_empty() {
        return None;
    }
    Some(crate::util::ellipsize(t, 60))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ref_proto::{Message, Role};

    #[test]
    fn purpose_injected_first() {
        let d = ToolDef {
            name: "t".into(),
            description: "d".into(),
            parameters: json!({"type": "object", "properties": {"a": {"type": "string"}}, "required": ["a"]}),
        };
        let d = with_purpose(d);
        let props = d.parameters["properties"].as_object().unwrap();
        assert_eq!(props.keys().next().unwrap(), "purpose");
        assert_eq!(d.parameters["required"], json!(["purpose", "a"]));
        let empty = with_purpose(ToolDef { name: "e".into(), description: String::new(), parameters: json!({}) });
        assert_eq!(empty.parameters["required"], json!(["purpose"]));
        // Byte-stable.
        let again = with_purpose(empty.clone());
        assert_eq!(serde_json::to_string(&again.parameters).unwrap(), serde_json::to_string(&empty.parameters).unwrap());
    }

    #[test]
    fn purpose_split() {
        let (p, rest) = split_purpose(&json!({"purpose": "look", "path": "a"}));
        assert_eq!(p, "look");
        assert_eq!(rest, json!({"path": "a"}));
        let (p, rest) = split_purpose(&json!("bad"));
        assert_eq!(p, "");
        assert_eq!(rest, json!("bad"));
    }

    #[test]
    fn compaction_selection() {
        let mut msgs = vec![
            Message::new(Role::User, MessageOrigin::User),
            Message::new(Role::Assistant, MessageOrigin::User),
        ];
        assert_eq!(context_start(&msgs), 0);
        msgs.push(Message::new(Role::User, MessageOrigin::Compaction));
        msgs.push(Message::new(Role::Assistant, MessageOrigin::User));
        assert_eq!(context_start(&msgs), 2);
        msgs.push(Message::new(Role::User, MessageOrigin::Compaction));
        assert_eq!(context_start(&msgs), 4);
        assert!(compaction_text("s", true).ends_with("Continue the task from here."));
    }

    #[test]
    fn titles() {
        assert_eq!(clean_title("\"Fix the parser.\"\n").as_deref(), Some("Fix the parser"));
        assert_eq!(clean_title("Title: 修复登录问题。").as_deref(), Some("修复登录问题"));
        assert_eq!(clean_title("  \n"), None);
        assert_eq!(clean_title("<think>hmm\nmore</think>\n\nRefactor parser").as_deref(), Some("Refactor parser"));
        assert_eq!(clean_title("<think>x</think>修复 bug").as_deref(), Some("修复 bug"));
    }
}
