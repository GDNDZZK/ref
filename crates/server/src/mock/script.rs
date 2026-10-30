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
    /// Fail the whole request with an HTTP error (must be the only step).
    Error(HttpError),
}

#[derive(Debug, Clone, Default)]
pub struct Round {
    pub steps: Vec<Step>,
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
            steps: vec![Step::Error(HttpError {
                status,
                message: message.into(),
            })],
            fill_context: false,
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
    let prompt = task_text(prompt.trim());
    let args = prompt
        .split_once(char::is_whitespace)
        .map_or("", |(_, rest)| rest.trim())
        .to_string();
    Task::Reply(Ctx {
        prompt,
        args,
        round,
        session_id: session_id.to_string(),
        attempt,
        tool_results,
    })
}

/// The script a prompt asks for: its first word, case-insensitive. Messages from other
/// sessions get `ack`; anything else the default script.
pub fn select(prompt: &str) -> &'static Script {
    use super::scripts;
    if prompt.starts_with("[Message from session") {
        return scripts::find("ack").expect("ack script");
    }
    let word = prompt.split_whitespace().next().unwrap_or("");
    let word = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
    scripts::find(&word.to_lowercase())
        .unwrap_or_else(|| scripts::find("default").expect("default"))
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
        assert_eq!(select("Markdown").name, "markdown");
        assert_eq!(select("stream-args now").name, "stream-args");
        assert_eq!(select("`help`!").name, "help");
        assert_eq!(select("something else").name, "default");
        assert_eq!(
            select("[Message from session \"x\" (id y)]\nhi").name,
            "ack"
        );
        assert_eq!(select("").name, "default");
    }
}
