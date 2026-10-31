//! `orchestrate`: one main session directs a crew of helper sessions and collects their
//! reports. Every wait is a named constant, so the timeline is the same on every run.
//!
//! Seconds after the prompt (measured with `ref-server --mock`, see the paragraph in the
//! development docs): helpers are created about 1.5 s apart and the fixer's edit waits for
//! approval. When the edit is approved at 9 s, the reports arrive in the order scout, docs,
//! fixer, tester. An earlier approval makes the fixer report earlier, so the order is not fixed.

use super::tools::{bash, parser_fix, plan_todos, read};
use crate::mock::script::{Ctx, Pace, Round, Script, sender_name};
use serde_json::{Value, json};

/// Faster than `Pace::MEDIUM`: at medium speed the first helper appears after 3.6 s.
const PACE: Pace = Pace {
    chars: 12,
    every_ms: 30,
};

/// Helpers in the order the main session creates them: (name, agent).
const CREW: [(&str, &str); 4] = [
    ("scout", "explore"),
    ("fixer", "build"),
    ("tester", "build"),
    ("docs", "build"),
];
const REVIEWER: (&str, &str) = ("reviewer", "reviewer");
const MAIN_STEPS: [&str; 5] = [
    "Find the bug",
    "Fix the bug",
    "Test the fix",
    "Update the docs",
    "Collect the reports",
];
/// Reports expected from the crew before the review is ordered.
const CREW_REPORTS: usize = CREW.len();
const ALL_REPORTS: usize = CREW_REPORTS + 1;

/// Pause before each `create_session` after the first, so the helpers appear 1.5 s apart.
const CREATE_PAUSE_MS: u64 = 1050;
/// Pause before the main session says that the helpers are working.
const WRAP_UP_PAUSE_MS: u64 = 1200;
/// The main session reads the review before it ticks the last job, and again before it
/// writes the summary.
const REVIEW_READ_PAUSE_MS: u64 = 1000;
const SUMMARY_PAUSE_MS: u64 = 1000;
/// How long each helper "thinks" before a tool call.
const SCOUT_THINK_MS: u64 = 1570;
const FIXER_THINK_MS: u64 = 1000;
const DOCS_THINK_MS: u64 = 1150;
const REVIEW_THINK_MS: u64 = 800;
/// The scout and the reviewer read their findings for a while before they report.
const SCOUT_REPORT_PAUSE_MS: u64 = 3000;
const REVIEW_REPORT_PAUSE_MS: u64 = 2400;
/// The fixer waits this long before it asks for approval of its edit.
const FIXER_EDIT_PAUSE_MS: u64 = 1650;
/// After the approved edit the fixer runs the tests and reports.
const FIXER_TEST_PAUSE_MS: u64 = 1500;
const FIXER_REPORT_PAUSE_MS: u64 = 1000;
/// The tester updates its todo list once per step.
const TESTER_STEP_PAUSE_MS: u64 = 1200;
const TESTER_BASELINE_SECS: u32 = 3;
const DOCS_REPORT_PAUSE_MS: u64 = 1150;

pub const SCRIPTS: &[Script] = &[
    Script {
        name: "orchestrate",
        about: "A main session creates four helpers (scout, fixer, tester, docs) and a reviewer, and collects their reports",
        pace: PACE,
        play: orchestrate,
    },
    Script {
        name: "orchestrate-report",
        about: "Used by `orchestrate`: what the main session does with each report",
        pace: PACE,
        play: report,
    },
    Script {
        name: "orch-scout",
        about: "Used by `orchestrate`: the scout searches and reports",
        pace: PACE,
        play: scout,
    },
    Script {
        name: "orch-fixer",
        about: "Used by `orchestrate`: the fixer edits (waits for approval) and reports",
        pace: PACE,
        play: fixer,
    },
    Script {
        name: "orch-tester",
        about: "Used by `orchestrate`: the tester runs the baseline and reports",
        pace: PACE,
        play: tester,
    },
    Script {
        name: "orch-docs",
        about: "Used by `orchestrate`: docs adds a changelog line and reports",
        pace: PACE,
        play: docs,
    },
    Script {
        name: "orch-review",
        about: "Used by `orchestrate`: the reviewer checks the change and reports",
        pace: PACE,
        play: review,
    },
];

/// Round `n` (1-based) of `orchestrate` starts the n-th helper. The first one follows the plan
/// at once.
fn create(ctx: &Ctx, n: usize) -> Round {
    let (name, agent) = CREW[n - 1];
    let input = json!({
        "name": name,
        "agent": agent,
        "prompt": format!("orch-{name} {}", ctx.session_id),
        "must_report": true,
    });
    let round = Round::new().text(format!("Starting {name}.\n"));
    let round = if n == 1 {
        round
    } else {
        round.pause(CREATE_PAUSE_MS)
    };
    round.tool("create_session", &format!("Start {name}"), input)
}

fn orchestrate(ctx: &Ctx) -> Round {
    match ctx.round {
        0 => Round::new()
            .text("The parser bug has four parts: find it, fix it, test it, update the docs. I'll hand them out.\n")
            .tool("todo", "Plan the work", plan_todos(&MAIN_STEPS, 0)),
        r if r <= CREW.len() => create(ctx, r),
        _ => Round::new()
            .pause(WRAP_UP_PAUSE_MS)
            .text("Four helpers are working. Their reports will arrive here."),
    }
}

/// The closing sentence after each report but the last of the crew.
const STILL_WORKING: [&str; CREW_REPORTS - 1] = [
    "Three helpers are still working.",
    "Two helpers are still working.",
    "One helper is still working.",
];

const SUMMARY: &str = "## Done\n\n- **Found:** `parse_items` loops one item past the end at `src/parser.rs:20`.\n- **Fixed:** `0..=items.len()` is now `0..items.len()`.\n- **Tested:** the baseline had one failing test, the parser tests pass after the fix.\n- **Docs:** a changelog line is in `NOTES.txt`.\n- **Reviewed:** the change is minimal and no other call site is affected.\n\nThe parser bug is fixed.";

fn reviewer_session(ctx: &Ctx) -> Value {
    json!({
        "name": REVIEWER.0,
        "agent": REVIEWER.1,
        "prompt": format!("orch-review {}", ctx.session_id),
        "must_report": true,
    })
}

/// What the main session does with a report. The report's number is the count of reports in
/// the conversation, so it stays right when several arrive while the main session is busy: the
/// queued ones join the conversation together at the next request.
fn report(ctx: &Ctx) -> Round {
    let k = ctx.peer_reports;
    let first_round = ctx.round == 0;
    let mark_done = |done: usize| plan_todos(&MAIN_STEPS, done);
    match k {
        0 => Round::new().text("A report arrived."),
        1..CREW_REPORTS if first_round => Round::new()
            .text(format!(
                "Report {k} of {CREW_REPORTS} from {}.\n",
                sender_name(&ctx.prompt)
            ))
            .tool("todo", "Mark the job done", mark_done(k)),
        1..CREW_REPORTS => Round::new().text(STILL_WORKING[k - 1]),
        CREW_REPORTS if first_round => Round::new()
            .text("All four have reported. I'll have the change reviewed before I sign off.\n")
            .tool("todo", "Mark the job done", mark_done(k))
            .tool(
                "create_session",
                "Start the reviewer",
                reviewer_session(ctx),
            ),
        CREW_REPORTS => Round::new().text("Waiting for the review."),
        _ if first_round => Round::new().pause(REVIEW_READ_PAUSE_MS).tool(
            "todo",
            "Everything is done",
            mark_done(ALL_REPORTS),
        ),
        _ => Round::new().pause(SUMMARY_PAUSE_MS).text(SUMMARY),
    }
}

fn send_message(to: &str, message: &str) -> Value {
    json!({ "to": to, "message": message })
}

/// The last round of every helper, after its report went out.
fn reported() -> Round {
    Round::new().text("Reported.")
}

/// A helper's tool round: a line of text, a pause, the tool.
fn step(text: &str, pause_ms: u64, name: &'static str, purpose: &str, input: Value) -> Round {
    Round::new()
        .text(text)
        .pause(pause_ms)
        .tool(name, purpose, input)
}

fn scout(ctx: &Ctx) -> Round {
    match ctx.round {
        0 => step(
            "Searching for the callers of parse_items.\n",
            SCOUT_THINK_MS,
            "grep",
            "Find the users of parse_items",
            json!({"pattern": "parse_items", "context": 1}),
        ),
        1 => step("Listing the Rust files.\n", SCOUT_THINK_MS, "find", "List the Rust files", json!({"pattern": "**/*.rs", "type": "file"})),
        2 => step("Reading the parser.\n", SCOUT_THINK_MS, "read", "Read the parser", read("src/parser.rs")),
        3 => Round::new().pause(SCOUT_REPORT_PAUSE_MS).tool(
            "send_message",
            "Report the findings",
            send_message(&ctx.args, "scout: parse_items has 3 callers; only src/parser.rs:20 loops past the end (`0..=items.len()`)."),
        ),
        _ => reported(),
    }
}

fn fixer(ctx: &Ctx) -> Round {
    match ctx.round {
        0 => step("Reading the parser.\n", FIXER_THINK_MS, "read", "Read the parser", read("src/parser.rs")),
        1 => step(
            "The loop needs one fewer step. Editing it.\n",
            FIXER_EDIT_PAUSE_MS,
            "edit",
            "Fix the off-by-one in parse_items",
            parser_fix(&ctx.tool_results),
        ),
        2 => step(
            "Running the parser tests.\n",
            FIXER_TEST_PAUSE_MS,
            "bash",
            "Run the parser tests",
            bash("echo \"cargo test parser: 12 passed\""),
        ),
        3 => Round::new().pause(FIXER_REPORT_PAUSE_MS).tool(
            "send_message",
            "Report the fix",
            send_message(&ctx.args, "fixer: replaced `0..=items.len()` with `0..items.len()` at src/parser.rs:20; cargo test parser passes."),
        ),
        _ => reported(),
    }
}

const TESTER_STEPS: [&str; 3] = [
    "Run the parser tests",
    "Read the failure",
    "Note the baseline",
];

const TESTER_REPORT: &str = "Tests added for the parser fix.";
const TESTER_REPORT_TYPED: &str = "Tests added for the parser fix. Also covered: empty input.";

/// The tester reports what it did, and what a person asked it to cover while it worked.
fn tester_report(ctx: &Ctx) -> &'static str {
    if ctx.typed.is_empty() {
        TESTER_REPORT
    } else {
        TESTER_REPORT_TYPED
    }
}

fn tester(ctx: &Ctx) -> Round {
    let plan = |done: usize| plan_todos(&TESTER_STEPS, done);
    let baseline = format!(
        "sleep {TESTER_BASELINE_SECS}; echo \"cargo test: 1 failed (parser::off_by_one), 11 passed\""
    );
    match ctx.round {
        0 => Round::new()
            .text("I'll record how the tests behave before the fix.\n")
            .tool("todo", "Plan the test run", plan(0)),
        r @ (1 | 2) => {
            Round::new()
                .pause(TESTER_STEP_PAUSE_MS)
                .tool("todo", "Update the plan", plan(r))
        }
        3 => step(
            "Running the baseline.\n",
            TESTER_STEP_PAUSE_MS,
            "bash",
            "Run the tests before the fix",
            bash(&baseline),
        ),
        4 => Round::new()
            .tool("todo", "Everything is done", plan(TESTER_STEPS.len()))
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
        1 => step(
            "Checking where the function is documented.\n",
            DOCS_THINK_MS,
            "grep",
            "Find the users of parse_items",
            json!({"pattern": "parse_items"}),
        ),
        2 => step(
            "Adding a changelog line.\n",
            DOCS_THINK_MS,
            "bash",
            "Add the changelog line",
            bash("echo \"- Fix off-by-one in parse_items\" >> NOTES.txt"),
        ),
        3 => Round::new().pause(DOCS_REPORT_PAUSE_MS).tool(
            "send_message",
            "Report the docs",
            send_message(&ctx.args, "docs: added a changelog line to NOTES.txt."),
        ),
        _ => reported(),
    }
}

fn review(ctx: &Ctx) -> Round {
    match ctx.round {
        0 => step("Looking at the diff.\n", REVIEW_THINK_MS, "bash", "Show the changed files", bash("echo \"src/parser.rs | 2 +-\"")),
        1 => step("Reading the fixed code.\n", REVIEW_THINK_MS, "read", "Read the parser", read("src/parser.rs")),
        2 => Round::new().pause(REVIEW_REPORT_PAUSE_MS).tool(
            "send_message",
            "Report the review",
            send_message(&ctx.args, "reviewer: the change is minimal and correct; no other call site loops past the end."),
        ),
        _ => reported(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::script::Step;

    const MAIN: &str = "ses_main";
    const ENVELOPE: &str = "[Message from session \"scout\" (id ses_s)]\nreport";

    fn ctx(round: usize, reports: usize, prompt: &str) -> Ctx {
        Ctx {
            prompt: prompt.into(),
            args: MAIN.into(),
            root_prompt: "orchestrate fix it".into(),
            peer_reports: reports,
            typed: vec![],
            round,
            session_id: MAIN.into(),
            attempt: 1,
            tool_results: vec![],
        }
    }

    fn tools(round: &Round) -> Vec<(&'static str, Value)> {
        round
            .steps
            .iter()
            .filter_map(|s| match s {
                Step::Tool(c) => Some((c.name, c.input.clone())),
                _ => None,
            })
            .collect()
    }

    fn text(round: &Round) -> String {
        round
            .steps
            .iter()
            .filter_map(|s| match s {
                Step::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    }

    fn statuses(input: &Value) -> Vec<&str> {
        input["todos"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["status"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn the_main_session_plans_then_creates_one_helper_per_round() {
        let plan = orchestrate(&ctx(0, 0, "orchestrate fix it"));
        assert!(text(&plan).starts_with("The parser bug has four parts"));
        let todo = &tools(&plan)[0];
        assert_eq!(todo.0, "todo");
        assert_eq!(todo.1["todos"].as_array().unwrap().len(), 5);
        assert_eq!(statuses(&todo.1)[0], "in_progress");

        for (round, (name, agent)) in CREW.iter().enumerate() {
            let r = orchestrate(&ctx(round + 1, 0, "orchestrate fix it"));
            let calls = tools(&r);
            assert_eq!(calls.len(), 1);
            let (tool, input) = &calls[0];
            assert_eq!(*tool, "create_session");
            assert_eq!(input["name"], *name);
            assert_eq!(input["agent"], *agent);
            assert_eq!(input["prompt"], format!("orch-{name} {MAIN}"));
            assert_eq!(input["must_report"], true);
            assert_eq!(text(&r), format!("Starting {name}.\n"));
        }
        let last = orchestrate(&ctx(5, 0, "orchestrate fix it"));
        assert!(tools(&last).is_empty());
        assert!(text(&last).contains("Four helpers are working"));
    }

    #[test]
    fn the_first_helper_is_created_without_a_wait() {
        let pauses = |round| {
            orchestrate(&ctx(round, 0, ""))
                .steps
                .iter()
                .filter(|s| matches!(s, Step::Pause(_)))
                .count()
        };
        assert_eq!(pauses(1), 0);
        assert_eq!(pauses(2), 1);
    }

    #[test]
    fn each_of_the_first_three_reports_ticks_one_more_job() {
        for k in 1..CREW_REPORTS {
            let first = report(&ctx(0, k, ENVELOPE));
            assert_eq!(text(&first), format!("Report {k} of 4 from scout.\n"));
            let todo = &tools(&first)[0];
            let done = statuses(&todo.1)
                .iter()
                .filter(|s| **s == "completed")
                .count();
            assert_eq!(done, k);
            let closing = report(&ctx(1, k, ENVELOPE));
            assert!(tools(&closing).is_empty());
            assert!(
                text(&closing).contains("still working"),
                "{}",
                text(&closing)
            );
        }
    }

    #[test]
    fn the_fourth_report_orders_the_review() {
        let first = report(&ctx(0, 4, ENVELOPE));
        assert!(text(&first).starts_with("All four have reported"));
        let calls = tools(&first);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "todo");
        assert_eq!(
            statuses(&calls[0].1)
                .iter()
                .filter(|s| **s == "completed")
                .count(),
            4
        );
        assert_eq!(calls[1].0, "create_session");
        assert_eq!(calls[1].1["name"], "reviewer");
        assert_eq!(calls[1].1["agent"], "reviewer");
        assert_eq!(calls[1].1["prompt"], format!("orch-review {MAIN}"));
        assert_eq!(
            text(&report(&ctx(1, 4, ENVELOPE))),
            "Waiting for the review."
        );
    }

    #[test]
    fn the_fifth_report_ends_with_the_summary() {
        let todo = report(&ctx(0, 5, ENVELOPE));
        assert!(
            statuses(&tools(&todo)[0].1)
                .iter()
                .all(|s| *s == "completed")
        );
        let summary = text(&report(&ctx(1, 5, ENVELOPE)));
        assert!(summary.starts_with("## Done"));
        assert_eq!(summary.lines().filter(|l| l.starts_with("- **")).count(), 5);
        assert!(summary.ends_with("The parser bug is fixed."));
    }

    #[test]
    fn the_waited_for_sentences_occur_once_in_the_whole_script() {
        let mut spoken = String::new();
        for round in 0..6 {
            spoken += &text(&orchestrate(&ctx(round, 0, "")));
        }
        for k in 1..=5 {
            for round in 0..2 {
                spoken += &text(&report(&ctx(round, k, ENVELOPE)));
            }
        }
        for sentence in [
            "Four helpers are working",
            "All four have reported",
            "Waiting for the review",
            "The parser bug is fixed.",
        ] {
            assert_eq!(spoken.matches(sentence).count(), 1, "{sentence}");
        }
    }

    #[test]
    fn every_worker_reports_to_the_main_session_once_and_then_stops() {
        type Play = fn(&Ctx) -> Round;
        let workers: [(Play, usize, &str); 5] = [
            (scout, 3, "scout: parse_items has 3 callers"),
            (fixer, 3, "fixer: replaced `0..=items.len()`"),
            (tester, 4, "Tests added for the parser fix."),
            (docs, 3, "docs: added a changelog line"),
            (review, 2, "reviewer: the change is minimal"),
        ];
        for (play, report_round, expected) in workers {
            let mut sent = Vec::new();
            for round in 0..=report_round + 1 {
                for (tool, input) in tools(&play(&ctx(round, 0, ""))) {
                    if tool == "send_message" {
                        sent.push((round, input));
                    }
                }
            }
            assert_eq!(sent.len(), 1, "{expected}");
            assert_eq!(sent[0].0, report_round, "{expected}");
            assert_eq!(sent[0].1["to"], MAIN);
            assert!(sent[0].1["message"].as_str().unwrap().starts_with(expected));
            let after = play(&ctx(report_round + 1, 0, ""));
            assert_eq!(text(&after), "Reported.");
            assert!(tools(&after).is_empty());
        }
    }

    /// The request a created tester sends in round `round`, after the human messages `typed`.
    fn tester_round(round: usize, typed: &[&str]) -> Round {
        use serde_json::json;
        let mut messages = vec![json!({"role": "user", "content":
            format!("[Session created by \"main\" (id {MAIN})]\norch-tester {MAIN}\n[When you finish or get blocked, report]")})];
        for i in 0..round {
            messages.push(json!({"role": "assistant", "content": "ok"}));
            messages.push(json!({"role": "tool", "tool_call_id": format!("c{i}"), "content": "r"}));
            // A typed message is delivered at a round boundary, in the middle of the task.
            if i == 1 {
                for text in typed {
                    messages.push(json!({"role": "user", "content": text}));
                }
            }
        }
        if round < 2 {
            for text in typed {
                messages.push(json!({"role": "user", "content": text}));
            }
        }
        let body = json!({"messages": messages, "tools": [{"type": "function"}]});
        let crate::mock::script::Task::Reply(ctx) = crate::mock::script::analyze(&body, "ses_t", 1)
        else {
            panic!("not a reply");
        };
        assert_eq!(ctx.round, round, "typed messages do not restart the rounds");
        assert_eq!(ctx.prompt, format!("orch-tester {MAIN}"));
        (crate::mock::script::select(&ctx.prompt, &ctx.root_prompt).play)(&ctx)
    }

    fn report_of(round: &Round) -> String {
        let (name, input) = tools(round)
            .into_iter()
            .find(|(n, _)| *n == "send_message")
            .unwrap();
        assert_eq!(name, "send_message");
        input["message"].as_str().unwrap().to_string()
    }

    #[test]
    fn the_tester_reports_what_a_person_asked_it_to_cover() {
        assert_eq!(
            report_of(&tester_round(4, &[])),
            "Tests added for the parser fix."
        );
        for typed in [
            &["also cover empty input"][..],
            &["first", "also cover empty input"][..],
        ] {
            assert_eq!(
                report_of(&tester_round(4, typed)),
                "Tests added for the parser fix. Also covered: empty input."
            );
        }
    }

    #[test]
    fn a_typed_message_does_not_restart_or_switch_the_tester() {
        // Whenever it arrives, the script goes on with the same round.
        for round in 0..=5 {
            let plain = format!("{:?}", tester_round(round, &[]).steps.len());
            let typed = format!(
                "{:?}",
                tester_round(round, &["also cover empty input"]).steps.len()
            );
            assert_eq!(plain, typed, "round {round}");
        }
    }

    #[test]
    fn the_fixer_edits_the_line_found_by_its_read() {
        let mut c = ctx(1, 0, "");
        c.tool_results = vec![
            "[src/parser.rs#AB12]\n19:fn parse_items() {\n20:    for i in 0..=items.len() {".into(),
        ];
        let edit = &tools(&fixer(&c))[0];
        assert_eq!(edit.0, "edit");
        assert_eq!(edit.1["tag"], "AB12");
        assert_eq!(edit.1["edits"][0]["start"], 20);
    }

    /// The next request of a conversation with these user messages, answered by its script.
    fn answer(users: &[String], round: usize) -> Round {
        let mut messages = Vec::new();
        for (i, text) in users.iter().enumerate() {
            messages.push(serde_json::json!({"role": "user", "content": text}));
            if i + 1 < users.len() {
                messages.push(serde_json::json!({"role": "assistant", "content": "ok"}));
            }
        }
        for _ in 0..round {
            messages.push(serde_json::json!({"role": "assistant", "content": "ok"}));
        }
        let body = serde_json::json!({"messages": messages, "tools": [{"type": "function"}]});
        let crate::mock::script::Task::Reply(ctx) = crate::mock::script::analyze(&body, MAIN, 1)
        else {
            panic!("not a reply");
        };
        (crate::mock::script::select(&ctx.prompt, &ctx.root_prompt).play)(&ctx)
    }

    #[test]
    fn running_orchestrate_again_in_the_same_conversation_starts_over() {
        let mut users: Vec<String> = vec!["orchestrate first run".into()];
        users.extend((0..ALL_REPORTS).map(|_| ENVELOPE.to_string()));
        users.push("orchestrate second run".into());

        // The plan comes first again, not the summary.
        let plan = answer(&users, 0);
        assert!(text(&plan).starts_with("The parser bug has four parts"));

        for k in 1..=CREW_REPORTS {
            users.push(ENVELOPE.into());
            let round = answer(&users, 0);
            if k < CREW_REPORTS {
                assert!(text(&round).starts_with(&format!("Report {k} of 4")), "{k}");
                continue;
            }
            let calls = tools(&round);
            assert_eq!(
                calls.last().unwrap().1["name"],
                "reviewer",
                "reviewer again"
            );
        }
        users.push(ENVELOPE.into());
        let todo = answer(&users, 0);
        assert!(
            statuses(&tools(&todo)[0].1)
                .iter()
                .all(|s| *s == "completed")
        );
        assert!(text(&answer(&users, 1)).ends_with("The parser bug is fixed."));
    }
}
