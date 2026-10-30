//! Scripts about the flow around a conversation: failures, context limits and other sessions.

use super::tools::{image_round, todo_round, tools_round};
use crate::mock::script::{Ctx, Pace, Round, Script};
use serde_json::json;

/// The provider answers `retry` with this status (overloaded) until the third attempt.
const OVERLOADED: u16 = 529;
const UNAUTHORIZED: u16 = 401;
const FAILED_ATTEMPTS: u32 = 2;
/// Rounds of `everything`: the `tools` rounds, then the `todo` rounds, then `image`.
/// How long the created session "works" before it reports.
const REPORT_DELAY_MS: u64 = 1500;
const TOOLS_ROUNDS: usize = 4;
const TODO_ROUNDS: usize = 5;
const SUMMARY_FOR_COMPACT: &str = "## Goal\n- Demonstrate compact_context.\n\n## Next steps\n1. Continue with whatever the user asks next.";

pub const SCRIPTS: &[Script] = &[
    Script {
        name: "retry",
        about: "The provider is overloaded twice (status Retrying), then the answer arrives",
        pace: Pace::NORMAL,
        play: retry,
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
        r if r < todo_end => todo_round(r - TOOLS_ROUNDS),
        r if r == todo_end => image_round(),
        _ => Round::new().text("That was everything: tools, a todo list and an image."),
    }
}
