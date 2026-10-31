//! `orchestrate-long`: the crew of `orchestrate`, but every helper works for a minute or more
//! and prints its progress every few seconds, so the sessions are still busy when the windows
//! are closed. The main session and the reports are the ones of `orchestrate`.
//!
//! Seconds after the prompt (measured with `ref-server --mock`, see the development docs): the
//! helpers are created at 1.5, 3, 4.5 and 6 s, the fixer asks to edit at 8 s, and when the edit
//! is approved at 10 s the helpers report at 66 (scout), 76 (docs), 79 (tester) and 91 s
//! (fixer). The reviewer reports at 96 s and the summary ends at 100 s. The main session is
//! idle from 7 s on, waiting for the reports.

use super::orchestrate::{
    DOCS_THINK_MS, FIXER_THINK_MS, PACE, SCOUT_THINK_MS, TESTER_STEPS, directing, report, reported,
    send_message, step, tester_report,
};
use super::tools::{bash, parser_fix, plan_todos, read};
use crate::mock::script::{Ctx, Round, Script};

/// Scripts of the helpers are named `orch-long-scout`, `orch-long-fixer` and so on.
const PREFIX: &str = "orch-long";
/// A progress line every this many seconds.
const TICK_SECS: u32 = 3;
/// The fixer waits this long before it asks for approval of its edit; at about 8 s.
const FIXER_EDIT_PAUSE_MS: u64 = 3250;
/// Pause before a helper starts its long command, and before it reports after it.
const RUN_PAUSE_MS: u64 = 1000;
const REPORT_PAUSE_MS: u64 = 1000;

/// What a helper's single long command prints: `{verb} n/{total} {unit}` after each tick, then
/// the closing command.
struct Job {
    name: &'static str,
    verb: &'static str,
    total: u32,
    unit: &'static str,
    ticks: u32,
    /// Shell command that runs when the progress is complete.
    finish: &'static str,
}

const SCOUT: Job = Job {
    name: "scout",
    verb: "read",
    total: 40,
    unit: "files",
    ticks: 20,
    finish: "echo \"scout: parse_items has 3 callers\"",
};
const FIXER: Job = Job {
    name: "fixer",
    verb: "patched",
    total: 5,
    unit: "files",
    ticks: 26,
    finish: "echo \"cargo test parser: 12 passed\"",
};
const TESTER: Job = Job {
    name: "tester",
    verb: "ran",
    total: 30,
    unit: "tests",
    ticks: 24,
    finish: "echo \"cargo test: 1 failed (parser::off_by_one), 11 passed\"",
};
const DOCS: Job = Job {
    name: "docs",
    verb: "updated",
    total: 9,
    unit: "pages",
    ticks: 22,
    finish: "echo \"- Fix off-by-one in parse_items\" >> NOTES.txt",
};

impl Job {
    /// The command: it starts with `seq`, which the mock configuration lets through.
    fn command(&self) -> String {
        format!(
            "seq 1 {ticks} | while read n; do echo \"{name}: {verb} $((n*{total}/{ticks}))/{total} {unit}\"; sleep {TICK_SECS}; done; {finish}",
            ticks = self.ticks,
            name = self.name,
            verb = self.verb,
            total = self.total,
            unit = self.unit,
            finish = self.finish,
        )
    }

    /// The round that runs the command.
    fn run(&self, text: &str) -> Round {
        step(
            text,
            RUN_PAUSE_MS,
            "bash",
            &format!("Work through the {}", self.unit),
            bash(&self.command()),
        )
    }
}

pub const SCRIPTS: &[Script] = &[
    Script {
        name: "orchestrate-long",
        about: "Like `orchestrate`, but each helper works for a minute or more and prints its progress",
        pace: PACE,
        play: orchestrate_long,
    },
    Script {
        name: "orchestrate-long-report",
        about: "Used by `orchestrate-long`: what the main session does with each report",
        pace: PACE,
        play: report,
    },
    Script {
        name: "orch-long-scout",
        about: "Used by `orchestrate-long`: the scout reads files for a minute",
        pace: PACE,
        play: scout,
    },
    Script {
        name: "orch-long-fixer",
        about: "Used by `orchestrate-long`: the fixer edits (waits for approval) and patches for 78 s",
        pace: PACE,
        play: fixer,
    },
    Script {
        name: "orch-long-tester",
        about: "Used by `orchestrate-long`: the tester runs tests for over a minute",
        pace: PACE,
        play: tester,
    },
    Script {
        name: "orch-long-docs",
        about: "Used by `orchestrate-long`: docs updates pages for over a minute",
        pace: PACE,
        play: docs,
    },
];

fn orchestrate_long(ctx: &Ctx) -> Round {
    directing(ctx, PREFIX)
}

fn report_round(ctx: &Ctx, label: &str, message: &str) -> Round {
    Round::new().pause(REPORT_PAUSE_MS).tool(
        "send_message",
        label,
        send_message(&ctx.args, message),
    )
}

fn scout(ctx: &Ctx) -> Round {
    match ctx.round {
        0 => step(
            "Reading the parser.\n",
            SCOUT_THINK_MS,
            "read",
            "Read the parser",
            read("src/parser.rs"),
        ),
        1 => SCOUT.run("Going through every file.\n"),
        2 => report_round(
            ctx,
            "Report the findings",
            "scout: parse_items has 3 callers; only src/parser.rs:20 loops past the end (`0..=items.len()`).",
        ),
        _ => reported(),
    }
}

fn fixer(ctx: &Ctx) -> Round {
    match ctx.round {
        0 => step(
            "Reading the parser.\n",
            FIXER_THINK_MS,
            "read",
            "Read the parser",
            read("src/parser.rs"),
        ),
        1 => step(
            "The loop needs one fewer step. Editing it.\n",
            FIXER_EDIT_PAUSE_MS,
            "edit",
            "Fix the off-by-one in parse_items",
            parser_fix(&ctx.tool_results),
        ),
        2 => FIXER.run("Patching the other files that use it.\n"),
        3 => report_round(
            ctx,
            "Report the fix",
            "fixer: replaced `0..=items.len()` with `0..items.len()` at src/parser.rs:20; cargo test parser passes.",
        ),
        _ => reported(),
    }
}

fn tester(ctx: &Ctx) -> Round {
    match ctx.round {
        0 => Round::new()
            .text("I'll record how the tests behave before the fix.\n")
            .tool("todo", "Plan the test run", plan_todos(&TESTER_STEPS, 0)),
        1 => TESTER.run("Running the baseline.\n"),
        2 => Round::new()
            .tool(
                "todo",
                "Everything is done",
                plan_todos(&TESTER_STEPS, TESTER_STEPS.len()),
            )
            .tool(
                "send_message",
                "Report the baseline",
                send_message(&ctx.args, tester_report(ctx)),
            ),
        _ => reported(),
    }
}

fn docs(ctx: &Ctx) -> Round {
    match ctx.round {
        0 => step(
            "Reading the README.\n",
            DOCS_THINK_MS,
            "read",
            "Read the README",
            read("README.md"),
        ),
        1 => DOCS.run("Updating the pages that mention it.\n"),
        2 => report_round(
            ctx,
            "Report the docs",
            "docs: added a changelog line to NOTES.txt.",
        ),
        _ => reported(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::script::Step;
    use serde_json::Value;

    const MAIN: &str = "ses_main";

    fn ctx(round: usize) -> Ctx {
        Ctx {
            prompt: String::new(),
            args: MAIN.into(),
            root_prompt: "orchestrate-long".into(),
            peer_reports: 0,
            typed: vec![],
            round,
            session_id: MAIN.into(),
            attempt: 1,
            tool_results: vec![],
        }
    }

    fn tool_calls(round: &Round) -> Vec<(&'static str, Value)> {
        round
            .steps
            .iter()
            .filter_map(|s| match s {
                Step::Tool(c) => Some((c.name, c.input.clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn each_helper_runs_one_command_of_a_minute_or_more() {
        for job in [&SCOUT, &FIXER, &TESTER, &DOCS] {
            let secs = job.ticks * TICK_SECS;
            assert!((60..=80).contains(&secs), "{}: {secs} s", job.name);
            let command = job.command();
            assert!(command.starts_with("seq "), "{command}");
            assert!(command.contains(&format!("{}/{}", job.total, job.ticks)));
        }
    }

    #[test]
    fn the_progress_never_goes_back_and_ends_at_the_total() {
        for job in [&SCOUT, &FIXER, &TESTER, &DOCS] {
            let shown: Vec<u32> = (1..=job.ticks).map(|n| n * job.total / job.ticks).collect();
            assert!(shown.windows(2).all(|w| w[0] <= w[1]), "{}", job.name);
            assert_eq!(shown.last(), Some(&job.total), "{}", job.name);
        }
    }

    #[test]
    fn every_helper_runs_exactly_one_long_command_before_it_reports() {
        type Play = fn(&Ctx) -> Round;
        let helpers: [(Play, usize); 4] = [(scout, 2), (fixer, 3), (tester, 2), (docs, 2)];
        for (play, report_round) in helpers {
            let mut long = 0;
            let mut sent = 0;
            for round in 0..=report_round + 1 {
                for (tool, input) in tool_calls(&play(&ctx(round))) {
                    match tool {
                        "bash" if input["command"].as_str().unwrap().contains("sleep") => {
                            assert!(sent == 0, "the command comes before the report");
                            long += 1;
                        }
                        "send_message" => {
                            assert_eq!(round, report_round);
                            sent += 1;
                        }
                        _ => {}
                    }
                }
            }
            assert_eq!((long, sent), (1, 1));
        }
    }

    #[test]
    fn the_main_session_starts_the_long_helpers() {
        let first = directing(&ctx(2), PREFIX);
        let calls = tool_calls(&first);
        assert_eq!(calls[0].0, "create_session");
        assert_eq!(calls[0].1["name"], "fixer");
        assert_eq!(calls[0].1["prompt"], format!("orch-long-fixer {MAIN}"));
        let crate::mock::script::Step::Text(t) = &first.steps[0] else {
            panic!("text first")
        };
        assert_eq!(t, "Starting fixer.\n");
    }
}
