//! Scripts about the flow around a conversation: failures, context limits and other sessions.

use super::stale::{PARSER, substitute};
use super::tools::{TODO_ROUNDS, bash, image_round, read, todo_round, tools_round};
use crate::mock::script::{Ctx, Pace, Round, Script};
use serde_json::json;

/// The provider answers `retry` with this status (overloaded) until the third attempt.
const OVERLOADED: u16 = 529;
const UNAUTHORIZED: u16 = 401;
const UNAVAILABLE: u16 = 503;
/// `outage`: the third request fails this often (overloaded, overloaded, reset, unavailable)
/// and the next attempt succeeds. The waits are the engine's: 2, 4, 8 and 16 seconds.
const OUTAGE_FAILURES: u32 = 4;
/// The line of the sample parser `outage` edits, before and after the fix.
const LENIENT_SCORE: &str = "score.trim().parse().unwrap_or(0)";
const STRICT_SCORE: &str = "score.trim().parse().ok()?";
const FAILED_ATTEMPTS: u32 = 2;
/// Rounds of `everything`: the `tools` rounds, then the `todo` rounds, then `image`.
/// How long the created session "works" before it reports.
const REPORT_DELAY_MS: u64 = 1500;
const TOOLS_ROUNDS: usize = 4;
const SUMMARY_FOR_COMPACT: &str = "## Goal\n- Demonstrate compact_context.\n\n## Next steps\n1. Continue with whatever the user asks next.";

pub const SCRIPTS: &[Script] = &[
    Script {
        name: "retry",
        about: "The provider is overloaded twice (status Retrying), then the answer arrives",
        pace: Pace::NORMAL,
        play: retry,
    },
    Script {
        name: "outage",
        about: "Two tools run, then the provider fails four times in a row (the waits grow), then the run goes on",
        pace: Pace::NORMAL,
        play: outage,
    },
    Script {
        name: "fail",
        about: "The provider rejects the request (401), the turn ends with an error",
        pace: Pace::NORMAL,
        play: fail,
    },
    Script {
        name: "fill",
        about: "Reports a nearly full context, so the next prompt compacts it automatically",
        pace: Pace::NORMAL,
        play: fill,
    },
    Script {
        name: "compact-tool",
        about: "The model compacts its own context with compact_context",
        pace: Pace::NORMAL,
        play: compact_tool,
    },
    Script {
        name: "peer",
        about: "Creates a reviewer session that reports back with send_message",
        pace: Pace::NORMAL,
        play: peer,
    },
    Script {
        name: "peer-child",
        about: "Used by `peer` in the created session; reports to the given session id",
        pace: Pace::NORMAL,
        play: peer_child,
    },
    Script {
        name: "ack",
        about: "What a message from another session gets: an echo of it",
        pace: Pace::NORMAL,
        play: ack,
    },
    Script {
        name: "everything",
        about: "tools, then todo, then image, in one long turn",
        pace: Pace::NORMAL,
        play: everything,
    },
];

fn retry(ctx: &Ctx) -> Round {
    if ctx.attempt <= FAILED_ATTEMPTS {
        return Round::error(OVERLOADED, "Overloaded: too many requests, try again later");
    }
    Round::new().text(format!(
        "The provider failed {FAILED_ATTEMPTS} times and answered on attempt {}.",
        ctx.attempt
    ))
}

/// The error of failed attempt `attempt` (1-based) of the request that `outage` loses.
fn outage_error(attempt: u32) -> Round {
    match attempt {
        1 | 2 => Round::error(OVERLOADED, "Overloaded: too many requests, try again later"),
        3 => Round::connection_reset(),
        _ => Round::error(UNAVAILABLE, "Service unavailable"),
    }
}

fn outage(ctx: &Ctx) -> Round {
    match ctx.round {
        0 => Round::new().text("I'll run the tests first.\n").tool(
            "bash",
            "Run the tests",
            bash("echo \"cargo test: 1 failed (parser::bad_score), 11 passed\""),
        ),
        1 => Round::new()
            .text("One test fails. Reading the parser.\n")
            .tool("read", "Read the parser", read(PARSER)),
        2 if ctx.attempt <= OUTAGE_FAILURES => outage_error(ctx.attempt),
        2 => Round::new()
            .text("A bad score should count as zero. Editing it.\n")
            .tool(
                "edit",
                "Make a bad score count as zero",
                substitute(&ctx.tool_results, PARSER, (STRICT_SCORE, LENIENT_SCORE)),
            ),
        3 => Round::new().text("Running the tests again.\n").tool(
            "bash",
            "Run the tests again",
            bash("echo \"cargo test: 12 passed\""),
        ),
        _ => Round::new().text("All 12 tests pass now."),
    }
}

fn fail(_: &Ctx) -> Round {
    Round::error(UNAUTHORIZED, "Invalid API key (this is a mock error)")
}

fn fill(_: &Ctx) -> Round {
    Round::new()
        .text("The context now looks almost full. Send any prompt and it is compacted first.")
        .filling_context()
}

fn compact_tool(ctx: &Ctx) -> Round {
    match ctx.round {
        0 => Round::new().text("I'll compact my context.\n").tool(
            "compact_context",
            "Compact the context",
            json!({ "summary": SUMMARY_FOR_COMPACT }),
        ),
        _ => Round::new().text("Done. From here on I only remember the summary."),
    }
}

fn peer(ctx: &Ctx) -> Round {
    match ctx.round {
        0 => Round::new().text("I'll ask a reviewer to look at the workspace.\n").tool(
            "create_session",
            "Start a reviewer",
            json!({
                "name": "helper",
                "agent": "reviewer",
                "prompt": format!("peer-child {}", ctx.session_id),
                "must_report": true,
            }),
        ),
        _ => Round::new().text(
            "The reviewer is working in the background. Its report arrives as a message from its session.",
        ),
    }
}

/// The created session: `ctx.args` is the id of the session to report to.
fn peer_child(ctx: &Ctx) -> Round {
    match ctx.round {
        0 => Round::new()
            .text("Looking at the workspace.\n")
            .pause(REPORT_DELAY_MS)
            .tool(
                "send_message",
                "Report the findings",
                json!({
                    "to": ctx.args,
                    "message": "Review done: src/parser.rs loops one item past the end (`0..=items.len()`).",
                }),
            ),
        _ => Round::new().text("Reported."),
    }
}

fn ack(ctx: &Ctx) -> Round {
    let quoted: String = ctx.prompt.lines().map(|l| format!("> {l}\n")).collect();
    Round::new().text(format!(
        "Got this message from another session:\n\n{quoted}"
    ))
}

fn everything(ctx: &Ctx) -> Round {
    let todo_end = TOOLS_ROUNDS + TODO_ROUNDS;
    match ctx.round {
        r if r < TOOLS_ROUNDS => tools_round(ctx, r),
        r if r < todo_end => todo_round(r - TOOLS_ROUNDS, 0, 0),
        r if r == todo_end => image_round(),
        _ => Round::new().text("That was everything: tools, a todo list and an image."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::script::Step;

    fn ctx(round: usize, attempt: u32, results: &[&str]) -> Ctx {
        Ctx {
            prompt: "outage".into(),
            args: String::new(),
            root_prompt: "outage".into(),
            peer_reports: 0,
            typed: vec![],
            round,
            session_id: "ses_test".into(),
            attempt,
            tool_results: results.iter().map(|r| r.to_string()).collect(),
        }
    }

    #[test]
    fn the_third_request_fails_four_times_in_a_row_and_then_goes_through() {
        let status = |attempt| outage(&ctx(2, attempt, &[])).error.map(|e| e.status);
        assert_eq!(status(1), Some(OVERLOADED));
        assert_eq!(status(2), Some(OVERLOADED));
        assert_eq!(status(3), None, "a reset has no status");
        assert!(outage(&ctx(2, 3, &[])).reset);
        assert_eq!(status(4), Some(UNAVAILABLE));
        let ok = outage(&ctx(2, 5, &[]));
        assert!(ok.error.is_none() && !ok.reset);
        assert!(matches!(ok.steps.last(), Some(Step::Tool(c)) if c.name == "edit"));
    }

    #[test]
    fn the_first_requests_and_the_last_ones_never_fail() {
        for round in [0, 1, 3, 4] {
            let r = outage(&ctx(round, 1, &[]));
            assert!(r.error.is_none() && !r.reset, "round {round}");
        }
    }

    #[test]
    fn the_fix_toggles_the_score_line_so_the_demo_runs_again() {
        let strict = "[scratch/src/parser.rs#AB12]\n16:        score: score.trim().parse().ok()?,";
        let Some(Step::Tool(call)) = outage(&ctx(2, 5, &[strict])).steps.pop() else {
            panic!("an edit");
        };
        assert_eq!(call.input["tag"], "AB12");
        assert_eq!(
            call.input["edits"][0]["lines"][0],
            "        score: score.trim().parse().unwrap_or(0),"
        );
        let lenient = strict.replace(STRICT_SCORE, LENIENT_SCORE);
        let Some(Step::Tool(call)) = outage(&ctx(2, 5, &[&lenient])).steps.pop() else {
            panic!("an edit");
        };
        assert!(
            call.input["edits"][0]["lines"][0]
                .as_str()
                .unwrap()
                .contains(STRICT_SCORE)
        );
    }
}
