//! `stale`: the file changes on disk between the model's `read` and its `edit`. The real `edit`
//! tool answers: it moves the edit onto the shifted lines, and rejects it when the lines
//! themselves changed. Everything runs in `scratch/`, where edits need no approval.

use super::tools::{bash, read};
use crate::mock::script::{Ctx, Pace, Round, Script};
use serde_json::{Value, json};

pub(super) const PARSER: &str = "scratch/src/parser.rs";
const LIB: &str = "scratch/src/lib.rs";
/// The rename: the old name, then the new one.
const RENAME: (&str, &str) = ("parse_line", "parse_row");
/// The line of `scratch/src/lib.rs` that changes under the model's feet.
const CHANGED_LINE: u32 = 12;
/// Three comment lines on top of the parser, like a `git pull` would bring.
const PULLED: [&str; 3] = [
    "// Report parser.",
    "// Rows look like label:score.",
    "// Keep this file small.",
];

pub const SCRIPTS: &[Script] = &[Script {
    name: "stale",
    about: "A file changes between read and edit: the edit is remapped, then another is rejected",
    pace: Pace::NORMAL,
    play: stale,
}];

/// The `sed` command that adds `PULLED` at the top of the parser.
fn pull_command() -> String {
    let lines: String = PULLED.iter().map(|l| format!("-e '1i{l}' ")).collect();
    format!(
        "sed -i {lines}{PARSER} && echo \"{} lines added\"",
        PULLED.len()
    )
}

/// A change to one line of the library that the model did not see.
fn change_command() -> String {
    format!(
        "sed -i '{CHANGED_LINE}s| *$| // reviewed|' {LIB} && echo \"line {CHANGED_LINE} changed\""
    )
}

/// The numbered lines of the newest `read` of `file` in `results`, with its tag.
fn newest_read<'a>(results: &'a [String], file: &str) -> Option<(&'a str, Vec<(u32, &'a str)>)> {
    let header = format!("[{file}#");
    let result = results.iter().rev().find(|r| r.starts_with(&header))?;
    let tag = result[header.len()..].split(']').next()?;
    let lines = result
        .lines()
        .filter_map(|l| {
            let (n, text) = l.split_once(':')?;
            Some((n.parse().ok()?, text))
        })
        .collect();
    Some((tag, lines))
}

/// An `edit` of `file` that puts `to` in place of `from` in every line of the newest `read` of
/// the file that has `from`, or the other way round when no line has it any more (so the demo
/// can run again). Without a read the tag is made up, which the tool rejects.
pub(super) fn substitute(results: &[String], file: &str, names: (&str, &str)) -> Value {
    let Some((tag, lines)) = newest_read(results, file) else {
        return json!({"path": file, "tag": "0000", "edits": []});
    };
    let (from, to) = if lines.iter().any(|(_, t)| t.contains(names.0)) {
        names
    } else {
        (names.1, names.0)
    };
    let edits: Vec<Value> = lines
        .iter()
        .filter(|(_, text)| text.contains(from))
        .map(|(n, text)| json!({"op": "replace", "start": n, "lines": [text.replace(from, to)]}))
        .collect();
    json!({"path": file, "tag": tag, "edits": edits})
}

fn stale(ctx: &Ctx) -> Round {
    let results = &ctx.tool_results;
    match ctx.round {
        0 => Round::new()
            .text("I'll read the parser first. Its tag goes with every edit.\n")
            .tool("read", "Read the parser", read(PARSER)),
        1 => Round::new()
            .text("Meanwhile the file changes on disk: three lines land at the top.\n")
            .tool("bash", "Add three lines at the top", bash(&pull_command())),
        2 => Round::new()
            .text("The tag is old now. Renaming with it anyway.\n")
            .tool("edit", "Rename in the parser", substitute(results, PARSER, RENAME)),
        3 => Round::new()
            .text("Now the library.\n")
            .tool("read", "Read the library", read(LIB)),
        4 => Round::new()
            .text("Another change lands, this time on a line I am about to edit.\n")
            .tool("bash", "Change a line of the library", bash(&change_command())),
        5 => Round::new()
            .text("Renaming in the library with the tag I have.\n")
            .tool("edit", "Rename in the library", substitute(results, LIB, RENAME)),
        6 => Round::new()
            .text("The edit was rejected. Reading the file again.\n")
            .tool("read", "Read the library again", read(LIB)),
        7 => Round::new()
            .text("Now the edit uses the current lines.\n")
            .tool("edit", "Rename in the library", substitute(results, LIB, RENAME)),
        _ => Round::new().text(
            "Done. The first edit was moved onto the shifted lines. The second was rejected because its lines had changed, and went through after a fresh read.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::script::Step;

    fn ctx(round: usize, results: &[&str]) -> Ctx {
        Ctx {
            prompt: "stale".into(),
            args: String::new(),
            root_prompt: "stale".into(),
            peer_reports: 0,
            typed: vec![],
            round,
            session_id: "ses_test".into(),
            attempt: 1,
            tool_results: results.iter().map(|r| r.to_string()).collect(),
        }
    }

    fn tool(round: &Round) -> (&'static str, Value) {
        round
            .steps
            .iter()
            .find_map(|s| match s {
                Step::Tool(c) => Some((c.name, c.input.clone())),
                _ => None,
            })
            .expect("a tool call")
    }

    const READ: &str = "[scratch/src/parser.rs#AB12]\n1://! Rows.\n12:pub fn parse_line(t: &str) {\n22:    lines.filter_map(parse_line)";

    #[test]
    fn the_rename_covers_every_line_with_the_old_name_and_uses_the_tag_of_the_read() {
        let (name, input) = tool(&stale(&ctx(2, &[READ, "3 lines added"])));
        assert_eq!(name, "edit");
        assert_eq!(input["tag"], "AB12");
        let edits = input["edits"].as_array().unwrap();
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0]["start"], 12);
        assert_eq!(edits[0]["lines"][0], "pub fn parse_row(t: &str) {");
        assert_eq!(edits[1]["start"], 22);
    }

    #[test]
    fn a_file_with_the_new_name_is_renamed_back() {
        let read = READ.replace("parse_line", "parse_row");
        let (_, input) = tool(&stale(&ctx(2, &[&read])));
        let line = input["edits"][0]["lines"][0].as_str().unwrap();
        assert!(line.contains("parse_line"), "{line}");
    }

    #[test]
    fn a_retry_uses_the_newest_read_of_the_file() {
        let old = "[scratch/src/lib.rs#0001]\n12:        let row = parse_line(line);";
        let new = "[scratch/src/lib.rs#0002]\n12:        let row = parse_line(line); // reviewed";
        let first = tool(&stale(&ctx(5, &[old, "changed"]))).1;
        assert_eq!(first["tag"], "0001");
        let retry = tool(&stale(&ctx(7, &[old, "changed", "rejected", new]))).1;
        assert_eq!(retry["tag"], "0002");
        let line = retry["edits"][0]["lines"][0].as_str().unwrap();
        assert!(line.ends_with("// reviewed"), "{line}");
    }

    #[test]
    fn the_shell_commands_start_with_what_the_mock_lets_through() {
        let pull = pull_command();
        assert!(
            pull.starts_with("sed -i -e '1i// Report parser.' "),
            "{pull}"
        );
        assert!(pull.ends_with("&& echo \"3 lines added\""), "{pull}");
        assert!(change_command().contains("12s"));
    }
}
