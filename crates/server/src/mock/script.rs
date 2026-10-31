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
    /// Reset the connection without an answer: the client sees a network error.
    pub reset: bool,
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

    /// A request that dies on the wire ("connection reset by peer").
    pub fn connection_reset() -> Round {
        Round {
            reset: true,
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
    /// The latest user message that is neither from another session nor a harness reminder (a
    /// created session's task without its envelope).
    pub root_prompt: String,
    /// How many user messages from other sessions came after `root_prompt`, the current one
    /// included.
    pub peer_reports: usize,
    /// Messages a person typed to a created session after its task. They do not restart the
    /// script: the round keeps counting from the task.
    pub typed: Vec<String>,
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
/// How the harness's own reminders start (agent switch, working directory, must_report).
const HARNESS_REMINDER: &str = "[Reminder]";
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

    let (prompt, round, tool_results, typed) = match since_task(messages) {
        Some(found) => found,
        None => since_prompt(messages),
    };
    let (root_prompt, peer_reports) = conversation_root(messages);
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
        typed,
        round,
        session_id: session_id.to_string(),
        attempt,
        tool_results,
    })
}

/// Walks back to the user's prompt, counting the model's answers and collecting tool results.
fn since_prompt(messages: &[Value]) -> (String, usize, Vec<String>, Vec<String>) {
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
    (prompt, round, tool_results, Vec::new())
}

/// For a created session: the task, the answers and tool results since it, and what a person
/// typed in between. The task stays the prompt, so a typed message does not change the script
/// or restart its rounds. `None` for other conversations, and once another session has written to
/// this one (those messages are answered like anywhere else).
fn since_task(messages: &[Value]) -> Option<(String, usize, Vec<String>, Vec<String>)> {
    let mut users = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| role(m) == "user");
    let (start, first) = users.next()?;
    let task = message_text(first);
    if !task.starts_with(SPAWN_HEADER) {
        return None;
    }
    let mut round = 0;
    let mut tool_results = Vec::new();
    let mut typed = Vec::new();
    for message in &messages[start + 1..] {
        let text = message_text(message);
        match role(message) {
            "assistant" => round += 1,
            "tool" => tool_results.push(text),
            "user" if text.starts_with(PEER_MESSAGE) => return None,
            "user"
                if text.starts_with(TOOL_IMAGES_MARKER) || text.starts_with(HARNESS_REMINDER) => {}
            "user" => typed.push(text.trim().to_string()),
            _ => {}
        }
    }
    Some((task, round, tool_results, typed))
}

/// The latest prompt that did not come from another session (reminders by the harness do not
/// count), and the number of messages from other sessions since it, the current one included.
/// A script that is run again in the same conversation starts over from its new prompt.
fn conversation_root(messages: &[Value]) -> (String, usize) {
    let mut root = String::new();
    let mut reports = 0;
    for message in messages.iter().filter(|m| role(m) == "user") {
        let text = message_text(message);
        if text.starts_with(TOOL_IMAGES_MARKER) {
            continue;
        }
        if text.starts_with(PEER_MESSAGE) {
            reports += 1;
        } else if !text.starts_with(HARNESS_REMINDER) {
            root = task_text(text.trim());
            reports = 0;
        }
    }
    (root, reports)
}

/// The first word of a prompt, lowercase, without surrounding punctuation (but `-` is part of
/// a script name).
fn first_word(prompt: &str) -> String {
    let word = prompt.split_whitespace().next().unwrap_or("");
    let word = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
    word.to_lowercase()
}

/// Suffix of the script that handles the reports of a crew: `orchestrate` has
/// `orchestrate-report`, `orchestrate-long` has `orchestrate-long-report`.
const REPORT_SUFFIX: &str = "-report";

/// The script a prompt asks for: its first word, case-insensitive. A message from another
/// session goes to the `-report` script of the script that started the conversation when it
/// has one, and to `ack` otherwise; anything else gets the default script.
pub fn select(prompt: &str, root_prompt: &str) -> &'static Script {
    use super::scripts;
    if prompt.starts_with(PEER_MESSAGE) {
        let report = format!("{}{REPORT_SUFFIX}", first_word(root_prompt));
        return scripts::find(&report)
            .or_else(|| scripts::find("ack"))
            .expect("report script");
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
        assert_eq!(
            select(REPORT, "orchestrate-long fix it").name,
            "orchestrate-long-report"
        );
        assert_eq!(sender_name(REPORT), "x");
        assert_eq!(sender_name("hello"), "");
    }

    #[test]
    fn reports_are_counted_since_the_latest_prompt() {
        let b = body(json!([
            {"role": "system", "content": "sys"},
            {"role": "user", "content": "orchestrate fix it"},
            {"role": "assistant", "content": "ok"},
            {"role": "user", "content": REPORT},
            {"role": "assistant", "content": "got it"},
            {"role": "user", "content": "[Reminder] The user switched the agent from a to b."},
            {"role": "user", "content": REPORT},
        ]));
        let ctx = reply(analyze(&b, "ses_1", 1));
        assert_eq!(ctx.root_prompt, "orchestrate fix it");
        assert_eq!(ctx.peer_reports, 2);
        assert_eq!(ctx.round, 0);
        assert!(ctx.prompt.starts_with(PEER_MESSAGE));
    }

    #[test]
    fn a_new_prompt_starts_the_count_and_the_root_over() {
        let b = body(json!([
            {"role": "user", "content": "orchestrate fix it"},
            {"role": "user", "content": REPORT},
            {"role": "user", "content": REPORT},
            {"role": "user", "content": "orchestrate again"},
            {"role": "assistant", "content": "ok"},
            {"role": "user", "content": REPORT},
        ]));
        let ctx = reply(analyze(&b, "ses_1", 1));
        assert_eq!(ctx.root_prompt, "orchestrate again");
        assert_eq!(ctx.peer_reports, 1);

        let started = body(json!([
            {"role": "user", "content": REPORT},
            {"role": "user", "content": REPORT},
        ]));
        let ctx = reply(analyze(&started, "ses_1", 1));
        assert_eq!(ctx.root_prompt, "");
        assert_eq!(ctx.peer_reports, 2);
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
