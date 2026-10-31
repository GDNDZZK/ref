//! What a mock script is, and how the last request picks one.
//!
//! A script answers the user's prompt over one or more *rounds*. A round is one LLM request:
//! the model streams some reasoning / text and maybe tool calls; the engine runs the tools and
//! asks again, which is the next round. The round number is the count of assistant messages
//! after the last user message.

use serde_json::{Map, Value, json};

/// How fast text and tool arguments stream: `chars` characters every `every_ms`.
#[derive(Debug, Clone, Copy)]
pub struct Pace {
    pub chars: usize,
    pub every_ms: u64,
}

impl Pace {
    pub const NORMAL: Pace = Pace {
        chars: 14,
        every_ms: 25,
    };
    pub const MEDIUM: Pace = Pace {
        chars: 6,
        every_ms: 30,
    };
    /// A few words per second, for frame-by-frame inspection.
    pub const SLOW: Pace = Pace {
        chars: 2,
        every_ms: 70,
    };
}

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub name: &'static str,
    pub purpose: String,
    /// Arguments without `purpose`.
    pub input: Value,
}

impl ToolCall {
    /// The raw argument JSON as a model would stream it: `purpose` comes first.
    pub fn raw_arguments(&self) -> String {
        let mut args = Map::new();
        args.insert("purpose".into(), json!(self.purpose));
        if let Value::Object(input) = &self.input {
            args.extend(input.clone());
        }
        Value::Object(args).to_string()
    }
}

#[derive(Debug, Clone)]
pub struct HttpError {
    pub status: u16,
    pub message: String,
}

#[derive(Debug, Clone)]
pub enum Step {
    Reasoning(String),
    Text(String),
    Tool(ToolCall),
    /// Stop streaming for a while.
    Pause(u64),
}

#[derive(Debug, Clone, Default)]
pub struct Round {
    pub steps: Vec<Step>,
    /// Fail the whole request with this HTTP error instead of streaming the steps.
    pub error: Option<HttpError>,
    /// Report the context as almost full, so the next prompt triggers automatic compaction.
    pub fill_context: bool,
}

impl Round {
    pub fn new() -> Round {
        Round::default()
    }

    pub fn reasoning(mut self, text: impl Into<String>) -> Round {
        self.steps.push(Step::Reasoning(text.into()));
        self
    }

    pub fn text(mut self, text: impl Into<String>) -> Round {
        self.steps.push(Step::Text(text.into()));
        self
    }

    pub fn tool(mut self, name: &'static str, purpose: &str, input: Value) -> Round {
        self.steps.push(Step::Tool(ToolCall {
            name,
            purpose: purpose.into(),
            input,
        }));
        self
    }

    pub fn pause(mut self, ms: u64) -> Round {
        self.steps.push(Step::Pause(ms));
        self
    }

    pub fn error(status: u16, message: impl Into<String>) -> Round {
        Round {
            error: Some(HttpError {
                status,
                message: message.into(),
            }),
            ..Round::default()
        }
    }

    pub fn filling_context(mut self) -> Round {
        self.fill_context = true;
        self
    }
}

/// Everything a script knows about the request it answers.
#[derive(Debug, Clone)]
pub struct Ctx {
    /// Text of the last user message.
    pub prompt: String,
    /// The prompt without its first word (the script name).
    pub args: String,
    /// The conversation's first user message (a created session's task without its envelope).
    pub root_prompt: String,
    /// How many user messages in the conversation come from other sessions, the current one
    /// included.
    pub peer_reports: usize,
    pub round: usize,
    pub session_id: String,
    /// Identical requests seen so far, this one included (a retry repeats its request).
    pub attempt: u32,
    /// Results of the tools run since the last user message, oldest first.
    pub tool_results: Vec<String>,
}

pub struct Script {
    pub name: &'static str,
    /// One line for the `help` table.
    pub about: &'static str,
    pub pace: Pace,
    pub play: fn(&Ctx) -> Round,
}

/// What a request to the mock provider asks for.
#[derive(Debug)]
pub enum Task {
    /// A short conversation title for the given text.
    Title(String),
    /// A summary of the conversation (manual or automatic compaction).
    Summary,
    /// Answer the user's prompt.
    Reply(Ctx),
}

/// Marker the LLM client puts before images that came from tool results.
const TOOL_IMAGES_MARKER: &str = "Attached image(s) from tool result:";
/// First line of the prompt a session created by `create_session` starts with; the task
/// follows on the next line.
const SPAWN_HEADER: &str = "[Session created by";
/// First line of the reminder appended to a created session's task.
const REPORT_REMINDER: &str = "[When you finish or get blocked";
/// How a message from another session starts (see `peers`).
pub const PEER_MESSAGE: &str = "[Message from session";

fn message_text(message: &Value) -> String {
    match &message["content"] {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => String::new(),
    }
}

fn role(message: &Value) -> &str {
    message["role"].as_str().unwrap_or("")
}

/// The task of a created session without the header and the report reminder around it.
fn task_text(prompt: &str) -> String {
    if !prompt.starts_with(SPAWN_HEADER) {
        return prompt.to_string();
    }
    prompt
        .lines()
        .skip(1)
        .take_while(|l| !l.starts_with(REPORT_REMINDER))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// Classifies a chat-completions request body. `attempt` is how often the same request came.
pub fn analyze(body: &Value, session_id: &str, attempt: u32) -> Task {
    let messages = body["messages"].as_array().map_or(&[][..], Vec::as_slice);
    let first = messages.first().map(message_text).unwrap_or_default();
    if first == crate::title::TITLE_PROMPT {
        let text = messages.get(1).map(message_text).unwrap_or_default();
        let text = text
            .strip_prefix(crate::title::TITLE_PROMPT)
            .unwrap_or(&text);
        return Task::Title(text.trim().to_string());
    }
    let no_tools = body["tools"].as_array().is_none_or(Vec::is_empty);
    let last = messages.last().map(message_text).unwrap_or_default();
    if no_tools && last == crate::compaction::SUMMARY_REQUEST {
        return Task::Summary;
    }

    // Walk back to the user's prompt, counting the model's answers and collecting tool results.
    let mut round = 0;
    let mut tool_results = Vec::new();
    let mut prompt = String::new();
    for message in messages.iter().rev() {
        let text = message_text(message);
        match role(message) {
            "assistant" => round += 1,
            "tool" => tool_results.push(text),
            "user" if text.starts_with(TOOL_IMAGES_MARKER) => {}
            "user" => {
                prompt = text;
                break;
            }
            _ => {}
        }
    }
    tool_results.reverse();
    let users: Vec<String> = messages
        .iter()
        .filter(|m| role(m) == "user")
        .map(message_text)
        .filter(|t| !t.starts_with(TOOL_IMAGES_MARKER))
        .collect();
    let root_prompt = users
        .first()
        .map(|t| task_text(t.trim()))
        .unwrap_or_default();
    let peer_reports = users.iter().filter(|t| t.starts_with(PEER_MESSAGE)).count();
    let prompt = task_text(prompt.trim());
    let args = prompt
        .split_once(char::is_whitespace)
        .map_or("", |(_, rest)| rest.trim())
        .to_string();
    Task::Reply(Ctx {
        prompt,
        args,
        root_prompt,
        peer_reports,
        round,
        session_id: session_id.to_string(),
        attempt,
        tool_results,
    })
}

/// The first word of a prompt, lowercase, without surrounding punctuation (but `-` is part of
/// a script name).
fn first_word(prompt: &str) -> String {
    let word = prompt.split_whitespace().next().unwrap_or("");
    let word = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
    word.to_lowercase()
}

/// Script that handles the reports of a crew directed by `orchestrate`.
const ORCHESTRATE: &str = "orchestrate";
const ORCHESTRATE_REPORT: &str = "orchestrate-report";

/// The script a prompt asks for: its first word, case-insensitive. A message from another
/// session goes to `orchestrate-report` when the conversation started with `orchestrate`, and
/// to `ack` otherwise; anything else gets the default script.
pub fn select(prompt: &str, root_prompt: &str) -> &'static Script {
    use super::scripts;
    if prompt.starts_with(PEER_MESSAGE) {
        let name = if first_word(root_prompt) == ORCHESTRATE {
            ORCHESTRATE_REPORT
        } else {
            "ack"
        };
        return scripts::find(name).expect("report script");
    }
    scripts::find(&first_word(prompt)).unwrap_or_else(|| scripts::find("default").expect("default"))
}

/// The name of the session that sent a message from another session.
pub fn sender_name(prompt: &str) -> &str {
    prompt
        .strip_prefix(PEER_MESSAGE)
        .and_then(|rest| rest.trim_start().strip_prefix('"'))
        .and_then(|rest| rest.split('"').next())
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(messages: Value) -> Value {
        json!({"messages": messages, "tools": [{"type": "function"}]})
    }

    fn reply(task: Task) -> Ctx {
        match task {
            Task::Reply(ctx) => ctx,
            other => panic!("not a reply: {other:?}"),
        }
    }

    #[test]
    fn rounds_count_answers_after_the_last_prompt() {
        let b = body(json!([
            {"role": "system", "content": "sys"},
            {"role": "user", "content": "old prompt"},
            {"role": "assistant", "content": "old answer"},
            {"role": "user", "content": "Tools please"},
            {"role": "assistant", "content": null, "tool_calls": []},
            {"role": "tool", "tool_call_id": "c1", "content": "[a.rs#ABCD]"},
            {"role": "user", "content": [
                {"type": "text", "text": "Attached image(s) from tool result:"}
            ]},
        ]));
        let ctx = reply(analyze(&b, "ses_1", 1));
        assert_eq!(ctx.round, 1);
        assert_eq!(ctx.prompt, "Tools please");
        assert_eq!(ctx.args, "please");
        assert_eq!(ctx.tool_results, ["[a.rs#ABCD]"]);
    }

    #[test]
    fn created_sessions_get_their_task_without_the_envelope() {
        let b = body(json!([
            {"role": "user", "content": "[Session created by \"main\" (id ses_p)]\npeer-child ses_p\n[When you finish or get blocked, report]"},
        ]));
        let ctx = reply(analyze(&b, "ses_c", 1));
        assert_eq!(ctx.prompt, "peer-child ses_p");
        assert_eq!(ctx.args, "ses_p");
    }

    #[test]
    fn titles_and_summaries_are_recognized() {
        let title = json!({"messages": [
            {"role": "system", "content": crate::title::TITLE_PROMPT},
            {"role": "user", "content": format!("{}\n\nfix the bug", crate::title::TITLE_PROMPT)},
        ]});
        assert!(matches!(analyze(&title, "s", 1), Task::Title(t) if t == "fix the bug"));
        let summary = json!({"messages": [
            {"role": "user", "content": "hi"},
            {"role": "user", "content": crate::compaction::SUMMARY_REQUEST},
        ]});
        assert!(matches!(analyze(&summary, "s", 1), Task::Summary));
    }

    #[test]
    fn scripts_are_chosen_by_first_word() {
        assert_eq!(select("Markdown", "").name, "markdown");
        assert_eq!(select("stream-args now", "").name, "stream-args");
        assert_eq!(select("`help`!", "").name, "help");
        assert_eq!(select("something else", "").name, "default");
        assert_eq!(select("", "").name, "default");
        assert_eq!(select("orch-scout ses_1", "").name, "orch-scout");
    }

    const REPORT: &str = "[Message from session \"x\" (id y)]\nhi";

    #[test]
    fn messages_from_sessions_go_to_ack_unless_the_conversation_orchestrates() {
        assert_eq!(select(REPORT, "peer").name, "ack");
        assert_eq!(select(REPORT, "").name, "ack");
        assert_eq!(
            select(REPORT, "orchestrate fix it").name,
            "orchestrate-report"
        );
        assert_eq!(select(REPORT, "Orchestrate").name, "orchestrate-report");
        assert_eq!(select(REPORT, "orchestrated").name, "ack");
        assert_eq!(sender_name(REPORT), "x");
        assert_eq!(sender_name("hello"), "");
    }

    #[test]
    fn the_root_prompt_and_the_reports_are_counted_over_the_whole_conversation() {
        let b = body(json!([
            {"role": "system", "content": "sys"},
            {"role": "user", "content": "orchestrate fix it"},
            {"role": "assistant", "content": "ok"},
            {"role": "user", "content": REPORT},
            {"role": "assistant", "content": "got it"},
            {"role": "user", "content": REPORT},
        ]));
        let ctx = reply(analyze(&b, "ses_1", 1));
        assert_eq!(ctx.root_prompt, "orchestrate fix it");
        assert_eq!(ctx.peer_reports, 2);
        assert_eq!(ctx.round, 0);
        assert!(ctx.prompt.starts_with(PEER_MESSAGE));
    }

    #[test]
    fn the_root_prompt_of_a_created_session_is_its_task() {
        let b = body(json!([
            {"role": "user", "content": "[Session created by \"main\" (id ses_p)]\norch-scout ses_p\n[When you finish or get blocked, report]"},
            {"role": "assistant", "content": null, "tool_calls": []},
            {"role": "tool", "tool_call_id": "c1", "content": "x"},
        ]));
        let ctx = reply(analyze(&b, "ses_c", 1));
        assert_eq!(ctx.root_prompt, "orch-scout ses_p");
        assert_eq!(ctx.peer_reports, 0);
        assert_eq!(ctx.round, 1);
    }
}
