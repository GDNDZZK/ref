//! Scripts that ask the user questions with the `question` tool.

use crate::mock::script::{Ctx, Pace, Round, Script};
use serde_json::{Value, json};

/// Start and end of the tool result that carries the answers (see `question::format_answers`).
const ANSWERED_PREFIX: &str = "User has answered your questions: ";
const ANSWERED_SUFFIX: &str = ". You can now continue";

pub const SCRIPTS: &[Script] = &[
    Script {
        name: "question",
        about: "Asks two questions (a single and a multiple choice, with defaults) and repeats the answers",
        pace: Pace::NORMAL,
        play: question,
    },
    Script {
        name: "question-one",
        about: "Asks one single-choice question with a default (Enter accepts it)",
        pace: Pace::NORMAL,
        play: question_one,
    },
];

fn option(label: &str, description: &str, default: bool) -> Value {
    json!({"label": label, "description": description, "default": default})
}

/// The answers in the tool result, or `None` when the question was dismissed.
fn answers(ctx: &Ctx) -> Option<&str> {
    let result = ctx.tool_results.first()?;
    let rest = result.strip_prefix(ANSWERED_PREFIX)?;
    rest.split(ANSWERED_SUFFIX).next()
}

fn reply(ctx: &Ctx) -> Round {
    match answers(ctx) {
        Some(a) => Round::new().text(format!("You chose: {a}.")),
        None => {
            Round::new().text("The question was dismissed, so I continue with my own defaults.")
        }
    }
}

fn question(ctx: &Ctx) -> Round {
    match ctx.round {
        0 => Round::new()
            .text("I need two decisions before I continue.\n")
            .tool(
                "question",
                "Ask how to fix the loop and what to check",
                json!({"questions": [
                    {
                        "question": "How should the loop bound in `parse_items` be fixed?",
                        "header": "Loop bound",
                        "options": [
                            option("Exclusive range", "Use `0..items.len()`", true),
                            option("Iterator", "Use `items.iter()` instead of indexing", false),
                            option("Guard the index", "Keep the loop and check the index", false),
                        ],
                    },
                    {
                        "question": "Which checks should run afterwards?",
                        "header": "Checks",
                        "multiple": true,
                        "options": [
                            option("Unit tests", "cargo test", true),
                            option("Clippy", "cargo clippy", true),
                            option("Formatting", "cargo fmt --check", false),
                        ],
                    },
                ]}),
            ),
        _ => reply(ctx),
    }
}

fn question_one(ctx: &Ctx) -> Round {
    match ctx.round {
        0 => Round::new().text("One quick question.\n").tool(
            "question",
            "Ask which file to change first",
            json!({"questions": [{
                "question": "Which file should I change first?",
                "header": "First file",
                "options": [
                    option("src/parser.rs", "Where the bug is", true),
                    option("src/lib.rs", "The public API", false),
                    option("README.md", "The documentation", false),
                ],
            }]}),
        ),
        _ => reply(ctx),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::scripts::tests::ctx;

    #[test]
    fn the_calls_are_valid_questions() {
        for (name, count) in [("question", 2), ("question-one", 1)] {
            let round = (crate::mock::scripts::find(name).unwrap().play)(&ctx(name, 0));
            let crate::mock::script::Step::Tool(call) = round.steps.last().unwrap() else {
                panic!("{name} ends in a tool call");
            };
            let parsed = crate::question::parse(&call.input).unwrap();
            assert_eq!(parsed.len(), count);
            assert!(parsed.iter().all(|q| q.options.iter().any(|o| o.default)));
        }
    }

    #[test]
    fn the_answer_is_repeated_or_the_dismissal_noted() {
        let mut c = ctx("question-one", 1);
        c.tool_results = vec![
            "User has answered your questions: \"Which file should I change first?\"=\"src/lib.rs\". You can now continue with the user's answers in mind.".into(),
        ];
        let text = format!("{:?}", question_one(&c).steps);
        assert!(text.contains("src/lib.rs"), "{text}");
        c.tool_results = vec!["The user dismissed the question without answering.".into()];
        assert!(format!("{:?}", question_one(&c).steps).contains("dismissed"));
    }
}
