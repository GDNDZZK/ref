//! Scripts that call tools. They run for real in the sample workspace.

use crate::mock::script::{Ctx, Pace, Round, Script};
use serde_json::{Value, json};

const ARTICLE: &str = include_str!("../assets/article.md");
/// The draft written by `stream-args`: long enough to stream for a while.
const DRAFT_CHARS: usize = 3000;
const BUGGY_LOOP: &str = "    for i in 0..=items.len() {";
const FIXED_LOOP: &str = "    for i in 0..items.len() {";
const SHORT_DOC: &str = "/// Greets `name`.";
const POLITE_DOC: &str = "/// Greets `name` politely.";
const LOG_COMMAND: &str = "seq -f 'log line %g: the quick brown fox jumps over the lazy dog' 1 400";
const RUNNING_COMMAND: &str = "seq 1 8 | while read i; do echo \"step $i of 8\"; sleep 1; done";

pub const SCRIPTS: &[Script] = &[
    Script {
        name: "tools",
        about: "Read, a missing file, grep, find, an approved edit with a diff, bash ok and failing",
        pace: Pace::NORMAL,
        play: tools,
    },
    Script {
        name: "stream-args",
        about: "A write call whose arguments stream slowly",
        pace: Pace::MEDIUM,
        play: stream_args,
    },
    Script {
        name: "permission",
        about: "An edit that waits for your approval (a: allow, s: always, d: deny)",
        pace: Pace::NORMAL,
        play: permission,
    },
    Script {
        name: "delay",
        about: "A delete with a 5 second countdown before it runs (Esc cancels)",
        pace: Pace::NORMAL,
        play: delay,
    },
    Script {
        name: "running",
        about: "A bash command that prints for 8 seconds",
        pace: Pace::NORMAL,
        play: running,
    },
    Script {
        name: "bash-fail",
        about: "A bash command that fails with exit code 1",
        pace: Pace::NORMAL,
        play: bash_fail,
    },
    Script {
        name: "denied",
        about: "A tool call the configuration refuses",
        pace: Pace::NORMAL,
        play: denied,
    },
    Script {
        name: "cancel",
        about: "A command that never ends; press Esc to cancel it",
        pace: Pace::NORMAL,
        play: cancel,
    },
    Script {
        name: "todo",
        about: "A todo list that is updated over several steps",
        pace: Pace::NORMAL,
        play: todo,
    },
    Script {
        name: "image",
        about: "A tool result with an image (logo.png)",
        pace: Pace::NORMAL,
        play: image,
    },
    Script {
        name: "long",
        about: "A tool with 400 lines of output, then a long answer",
        pace: Pace::NORMAL,
        play: long,
    },
];

/// The tag of `file` and the number of the line containing `needle`, from the newest tool
/// result that has both (a `read`; a `grep` result carries the tag but not every line).
fn tag_and_line(results: &[String], file: &str, needle: &str) -> Option<(String, usize)> {
    let header = format!("[{file}#");
    results.iter().rev().find_map(|result| {
        let rest = result.strip_prefix(&header)?;
        let tag = rest.split(']').next()?.to_string();
        let line = result.lines().find(|l| l.contains(needle))?;
        Some((tag, line.split(':').next()?.parse().ok()?))
    })
}

/// An `edit` that replaces one line. Without a fresh `read` the tag is made up, which the
/// tool rejects: the failure is a state worth showing too.
fn replace_line(file: &str, found: Option<(String, usize)>, text: &str) -> Value {
    let (tag, line) = found.unwrap_or_else(|| ("0000".into(), 1));
    json!({
        "path": file,
        "tag": tag,
        "edits": [{"op": "replace", "start": line, "lines": [text]}],
    })
}

/// The `edit` that fixes the off-by-one in `src/parser.rs`, from the newest `read` of it.
/// When the file is fixed already, it puts the bug back so the demo can run again.
pub(super) fn parser_fix(results: &[String]) -> Value {
    let found = tag_and_line(results, "src/parser.rs", "for i in 0..");
    let fixed = found.as_ref().is_some_and(|_| {
        results
            .iter()
            .any(|r| r.contains(FIXED_LOOP) && !r.contains(BUGGY_LOOP))
    });
    let new_line = if fixed { BUGGY_LOOP } else { FIXED_LOOP };
    replace_line("src/parser.rs", found, new_line)
}

pub(super) fn read(path: &str) -> Value {
    json!({ "path": path })
}

pub(super) fn bash(command: &str) -> Value {
    json!({ "command": command })
}

/// Rounds 0..=3 of the `tools` script; `everything` continues after them.
pub fn tools_round(ctx: &Ctx, round: usize) -> Round {
    let results = &ctx.tool_results;
    match round {
        0 => Round::new()
            .reasoning("Let me look at the parser first, and check a file that does not exist.")
            .text("I'll start by reading the parser and searching the workspace.\n")
            .tool("read", "Read the parser", read("src/parser.rs"))
            .tool("read", "Read a file that is not there", read("src/missing.rs"))
            .tool("grep", "Find the users of parse_items", json!({"pattern": "parse_items", "context": 1}))
            .tool("find", "List the Rust files", json!({"pattern": "**/*.rs", "type": "file"})),
        1 => Round::new()
            .text("The loop runs one step too far: `0..=items.len()` indexes past the end. Fixing it:\n")
            .tool(
                "edit",
                "Fix the off-by-one in parse_items",
                parser_fix(results),
            ),
        2 => Round::new()
            .text("Now a quick check of the result.\n")
            .tool("bash", "Print the test result", bash("echo \"cargo test: 12 passed\""))
            .tool("bash", "Run a command that fails", bash("echo \"error: build failed\" >&2; exit 3")),
        _ => Round::new().text(
            "Done. The edit went through, the first `bash` call succeeded and the second one failed with exit code 3, which is how a failing tool looks.",
        ),
    }
}

fn tools(ctx: &Ctx) -> Round {
    tools_round(ctx, ctx.round)
}

fn stream_args(ctx: &Ctx) -> Round {
    match ctx.round {
        0 => {
            let end = ARTICLE
                .char_indices()
                .nth(DRAFT_CHARS)
                .map_or(ARTICLE.len(), |(i, _)| i);
            Round::new()
                .text("Writing a draft. Watch the arguments arrive:\n")
                .tool(
                    "write",
                    "Write the draft",
                    json!({"path": "scratch/draft.md", "content": &ARTICLE[..end]}),
                )
        }
        _ => Round::new().text("The draft is in `scratch/draft.md`."),
    }
}

fn permission(ctx: &Ctx) -> Round {
    match ctx.round {
        0 => Round::new()
            .text("I'll improve a doc comment in `src/lib.rs`.\n")
            .tool("read", "Read the library", read("src/lib.rs")),
        1 => {
            let found = tag_and_line(&ctx.tool_results, "src/lib.rs", "/// Greets");
            let polite = ctx.tool_results.iter().any(|r| r.contains(POLITE_DOC));
            let new_line = if polite { SHORT_DOC } else { POLITE_DOC };
            Round::new().text("This edit needs your approval.\n").tool(
                "edit",
                "Reword the doc comment",
                replace_line("src/lib.rs", found, new_line),
            )
        }
        _ => Round::new().text("That is it: the comment now reads as requested."),
    }
}

fn delay(ctx: &Ctx) -> Round {
    match ctx.round {
        0 => Round::new()
            .text("First a temporary file, then I delete it after the configured delay.\n")
            .tool(
                "bash",
                "Create a temporary file",
                bash("echo temporary > scratch/old.txt"),
            ),
        1 => Round::new().tool(
            "delete",
            "Delete the temporary file",
            json!({"path": "scratch/old.txt"}),
        ),
        _ => Round::new().text("The file is gone."),
    }
}

fn running(ctx: &Ctx) -> Round {
    match ctx.round {
        0 => Round::new()
            .text("Running a command that prints once a second.\n")
            .tool("bash", "Count to eight", bash(RUNNING_COMMAND)),
        _ => Round::new().text("The command finished."),
    }
}

fn bash_fail(ctx: &Ctx) -> Round {
    match ctx.round {
        0 => Round::new().tool(
            "bash",
            "Build the project",
            bash("echo \"error: build failed\" >&2; exit 1"),
        ),
        _ => Round::new().text("The build failed with exit code 1."),
    }
}

fn denied(ctx: &Ctx) -> Round {
    match ctx.round {
        0 => Round::new().text("Let me fetch the documentation.\n").tool(
            "bash",
            "Fetch the docs",
            bash("curl -s https://example.com/docs"),
        ),
        _ => Round::new()
            .text("The fetch was refused by the configuration, so I will work without it."),
    }
}

fn cancel(ctx: &Ctx) -> Round {
    match ctx.round {
        0 => Round::new()
            .text("Starting a long job. Press Esc to cancel it.\n")
            .tool("bash", "Wait for a long time", bash("sleep 300")),
        _ => Round::new().text("The job ended."),
    }
}

fn todos(items: &[(&str, &str)]) -> Value {
    let list: Vec<Value> = items
        .iter()
        .map(|(content, status)| json!({"content": content, "status": status}))
        .collect();
    json!({ "todos": list })
}

/// A todo list over `steps` with the first `done` completed, the next one in progress.
pub(super) fn plan_todos(steps: &[&str], done: usize) -> Value {
    let items: Vec<(&str, &str)> = steps
        .iter()
        .enumerate()
        .map(|(i, step)| {
            let status = match i.cmp(&done) {
                std::cmp::Ordering::Less => "completed",
                std::cmp::Ordering::Equal => "in_progress",
                std::cmp::Ordering::Greater => "pending",
            };
            (*step, status)
        })
        .collect();
    todos(&items)
}

const STEPS: [&str; 3] = [
    "Read the README",
    "Check the workspace",
    "Write the summary",
];

/// Rounds 0..=5 of the `todo` script: the plan, then work and updates.
pub fn todo_round(round: usize) -> Round {
    let plan = |done: usize| plan_todos(&STEPS, done);
    match round {
        0 => Round::new().text("I'll plan this in three steps.\n").tool(
            "todo",
            "Plan the work",
            plan(0),
        ),
        1 => Round::new().tool("read", "Read the README", read("README.md")),
        2 => Round::new().tool("todo", "Step one is done", plan(1)),
        3 => Round::new().tool("bash", "Look at the workspace", bash("ls -la")),
        4 => Round::new().tool("todo", "Everything is done", plan(STEPS.len())),
        _ => Round::new()
            .text("All steps are done. The todo list in the sidebar shows them completed."),
    }
}

fn todo(ctx: &Ctx) -> Round {
    todo_round(ctx.round)
}

/// Round 0 of the `image` script.
pub fn image_round() -> Round {
    Round::new().text("Let me look at the logo.\n").tool(
        "read",
        "Look at the logo",
        read("logo.png"),
    )
}

fn image(ctx: &Ctx) -> Round {
    match ctx.round {
        0 => image_round(),
        _ => Round::new().text("It is a blue-to-purple gradient with a light ring in the middle."),
    }
}

fn long(ctx: &Ctx) -> Round {
    match ctx.round {
        0 => Round::new().text("Printing a lot of lines.\n").tool(
            "bash",
            "Print the log",
            bash(LOG_COMMAND),
        ),
        _ => Round::new().text(ARTICLE),
    }
}
