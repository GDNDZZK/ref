//! Representative protocol values used for the fixtures. Built from the real types, so a change
//! of a type breaks the build or the fixtures test instead of silently drifting.

use crate::*;
use serde_json::{Value, json};

/// 2025-10-09 08:53:20 UTC.
const T0: i64 = 1_760_000_000_000;
const SECOND: i64 = 1000;

pub const SESSION_ID: &str = "ses_a1b2c3d4";
pub const MESSAGE_ID: &str = "msg_0f1e2d3c";
pub const CALL_ID: &str = "call_9x8y7z";

pub fn model() -> ModelRef {
    ModelRef {
        provider: "anthropic".into(),
        model: "claude-sonnet-4".into(),
        variant: Some("high".into()),
    }
}

pub fn context() -> ContextUsage {
    ContextUsage {
        used: 42_000,
        max: 200_000,
        recommend: 150_000,
        auto: 180_000,
    }
}

pub fn todos() -> Vec<Todo> {
    let todo = |content: &str, status| Todo {
        content: content.into(),
        status,
    };
    vec![
        todo("Reproduce the failing test", TodoStatus::Completed),
        todo("Fix the off-by-one in parse_items", TodoStatus::InProgress),
        todo("Add a regression test", TodoStatus::Pending),
        todo("Update the changelog", TodoStatus::Cancelled),
    ]
}

pub fn info() -> SessionInfo {
    SessionInfo {
        id: SESSION_ID.into(),
        name: "parser-fix".into(),
        title: "Fix the failing parser test".into(),
        agent: "build".into(),
        model: model(),
        cwd: "/home/user/project".into(),
        created_at: T0,
        updated_at: T0 + 90 * SECOND,
        status: SessionStatus::Busy,
        attached: 1,
        live: true,
        parent: None,
        must_report: false,
        context: context(),
        todos: todos(),
        busy_since: Some(T0 + 60 * SECOND),
        last_error: None,
    }
}

pub fn text_part() -> Part {
    Part::Text {
        text: "The loop runs one step too far.\n\n```rust\nfor i in 0..items.len() {\n    let item = &items[i];\n}\n```".into(),
        time: T0 + 10 * SECOND,
    }
}

pub fn reasoning_part() -> Part {
    Part::Reasoning {
        text: "The test fails with an index out of bounds, so the range is probably inclusive."
            .into(),
        time: T0 + 5 * SECOND,
        time_end: Some(T0 + 9 * SECOND),
        provider_data: Some(json!({"signature": "EqQBCkYIAxgC"})),
    }
}

pub fn image_stub() -> ImageData {
    ImageData {
        mime: "image/png".into(),
        data: String::new(),
        id: Some(format!("{MESSAGE_ID}:1:0")),
        size: Some(48_213),
    }
}

pub fn image_part() -> Part {
    Part::Image {
        image: image_stub(),
        name: Some("screenshot.png".into()),
    }
}

/// A tool part with the fields every sample shares; scenarios override some of them.
pub struct ToolSample {
    pub name: &'static str,
    pub purpose: &'static str,
    pub input: Value,
    pub raw_input: String,
    pub state: ToolState,
    pub output: Option<String>,
    pub meta: Option<Value>,
    pub images: Vec<ImageData>,
    pub finished: bool,
    pub run_at: Option<i64>,
}

impl ToolSample {
    pub fn new(name: &'static str, state: ToolState) -> Self {
        ToolSample {
            name,
            purpose: "",
            input: Value::Null,
            raw_input: String::new(),
            state,
            output: None,
            meta: None,
            images: Vec::new(),
            finished: false,
            run_at: None,
        }
    }

    pub fn part(self) -> Part {
        Part::Tool {
            call_id: CALL_ID.into(),
            name: self.name.into(),
            purpose: self.purpose.into(),
            input: self.input,
            raw_input: self.raw_input,
            state: self.state,
            output: self.output,
            meta: self.meta,
            images: self.images,
            time: T0 + 20 * SECOND,
            time_end: self.finished.then_some(T0 + 22 * SECOND),
            run_at: self.run_at,
            provider_data: None,
        }
    }
}

const EDIT_PURPOSE: &str = "Fix the off-by-one in parse_items";
const EDIT_RAW: &str = r#"{"purpose": "Fix the off-by-one in parse_items", "path": "src/parser.rs", "tag": "a1f3", "edits": [{"op":"replace","start":42,"end":42,"lines":["    for i in 0..items.len() {"]}]}"#;
const EDIT_DIFF: &str = "--- a/src/parser.rs\n+++ b/src/parser.rs\n@@ -40,5 +40,5 @@\n fn parse_items(items: &[Item]) -> Result<Vec<Out>> {\n     let mut out = Vec::new();\n-    for i in 0..=items.len() {\n+    for i in 0..items.len() {\n         let item = &items[i];\n";

pub fn edit_input() -> Value {
    json!({
        "path": "src/parser.rs",
        "tag": "a1f3",
        "edits": [{"op": "replace", "start": 42, "end": 42, "lines": ["    for i in 0..items.len() {"]}],
    })
}

fn edit(state: ToolState) -> ToolSample {
    ToolSample {
        purpose: EDIT_PURPOSE,
        input: edit_input(),
        raw_input: EDIT_RAW.into(),
        ..ToolSample::new("edit", state)
    }
}

fn bash(state: ToolState) -> ToolSample {
    ToolSample {
        purpose: "Run the parser tests",
        input: json!({"command": "cargo test -p parser", "timeout": 120}),
        raw_input:
            r#"{"purpose":"Run the parser tests","command":"cargo test -p parser","timeout":120}"#
                .into(),
        ..ToolSample::new("bash", state)
    }
}

/// Tool parts in every state and with the kinds of `output` / `meta` the tools produce.
/// The name is the fixture file name.
pub fn tool_parts() -> Vec<(&'static str, Part)> {
    let streaming = ToolSample {
        raw_input: r#"{"purpose": "Fix the off-by-one in parse_items", "path": "src/pars"#.into(),
        ..ToolSample::new("edit", ToolState::Streaming)
    };
    let awaiting = edit(ToolState::AwaitingPermission);
    let delayed = ToolSample {
        run_at: Some(T0 + 25 * SECOND),
        ..bash(ToolState::Delayed)
    };
    let running = ToolSample {
        meta: Some(json!({"live_output": "   Compiling parser v0.3.1\n"})),
        ..bash(ToolState::Running)
    };
    let done_edit = ToolSample {
        output: Some(
            "Edited src/parser.rs [src/parser.rs#b7c2]\n  42 |     for i in 0..items.len() {"
                .into(),
        ),
        meta: Some(
            json!({"path": "/home/user/project/src/parser.rs", "diff": EDIT_DIFF, "added": 1, "removed": 1}),
        ),
        finished: true,
        ..edit(ToolState::Done)
    };
    let done_bash = ToolSample {
        output: Some("running 12 tests\ntest result: ok. 12 passed; 0 failed\n".into()),
        meta: Some(
            json!({"exit_code": 0, "duration_ms": 4210, "truncated": false, "output_path": null, "timed_out": false, "cancelled": false}),
        ),
        finished: true,
        ..bash(ToolState::Done)
    };
    let error = ToolSample {
        purpose: "Read the parser",
        input: json!({"path": "src/lib"}),
        raw_input: r#"{"purpose":"Read the parser","path":"src/lib"}"#.into(),
        output: Some("Path not found: src/lib\nDid you mean src/?".into()),
        finished: true,
        ..ToolSample::new("read", ToolState::Error)
    };
    let denied = ToolSample {
        output: Some("The user denied this tool call.".into()),
        finished: true,
        ..edit(ToolState::Denied)
    };
    let cancelled = ToolSample {
        output: Some("Cancelled by user.".into()),
        finished: true,
        ..bash(ToolState::Cancelled)
    };
    let with_image = ToolSample {
        purpose: "Look at the screenshot",
        input: json!({"path": "screenshot.png"}),
        raw_input: r#"{"purpose":"Look at the screenshot","path":"screenshot.png"}"#.into(),
        output: Some("Image: screenshot.png (image/png, 47.1 KB)".into()),
        meta: Some(json!({"path": "/home/user/project/screenshot.png", "image": true})),
        images: vec![ImageData {
            id: Some(format!("{MESSAGE_ID}:3:0")),
            ..image_stub()
        }],
        finished: true,
        ..ToolSample::new("read", ToolState::Done)
    };
    let todo = ToolSample {
        purpose: "Track the plan",
        input: json!({"todos": todos().iter().map(|t| json!({"content": t.content, "status": t.status})).collect::<Vec<_>>()}),
        output: Some("Todo list updated.".into()),
        finished: true,
        ..ToolSample::new("todo", ToolState::Done)
    };
    vec![
        ("tool", edit(ToolState::Running).part()),
        ("tool_streaming", streaming.part()),
        ("tool_awaiting_permission", awaiting.part()),
        ("tool_delayed", delayed.part()),
        ("tool_running", running.part()),
        ("tool_done_edit_diff", done_edit.part()),
        ("tool_done_bash", done_bash.part()),
        ("tool_error", error.part()),
        ("tool_denied", denied.part()),
        ("tool_cancelled", cancelled.part()),
        ("tool_with_image", with_image.part()),
        ("tool_todo", todo.part()),
    ]
}

pub fn permission_request() -> PermissionRequest {
    PermissionRequest {
        session_id: SESSION_ID.into(),
        message_id: MESSAGE_ID.into(),
        call_id: CALL_ID.into(),
        tool: "edit".into(),
        purpose: EDIT_PURPOSE.into(),
        input: edit_input(),
        time: T0 + 21 * SECOND,
    }
}

fn message(id: &str, role: Role, source: MessageSource, parts: Vec<Part>) -> Message {
    Message {
        id: id.into(),
        role,
        time: T0 + 2 * SECOND,
        time_end: None,
        source,
        parts,
        model: None,
        agent: None,
        usage: None,
        error: None,
    }
}

/// A prompt typed by the user, with a pasted image (as a stub).
pub fn user_message() -> Message {
    let text = Part::Text {
        text: "The parser test fails, please fix it. This is what I see:".into(),
        time: T0,
    };
    message(
        "msg_user_01",
        Role::User,
        MessageSource::Human,
        vec![text, image_part()],
    )
}

pub fn peer_message() -> Message {
    let source = MessageSource::Peer {
        from_id: "ses_e5f6a7b8".into(),
        from_name: "reviewer".into(),
        msg_id: "peer_77aa".into(),
    };
    let text = Part::Text {
        text: "Please also check the benchmark in benches/parse.rs.".into(),
        time: T0 + 30 * SECOND,
    };
    message("msg_peer_01", Role::User, source, vec![text])
}

/// A finished assistant message: reasoning, text and a tool call that is still running.
pub fn assistant_message() -> Message {
    Message {
        time_end: Some(T0 + 22 * SECOND),
        model: Some(model()),
        agent: Some("build".into()),
        usage: Some(Usage {
            input: 1200,
            output: 340,
            cache_read: 38_000,
            cache_write: 2500,
            reasoning: 120,
        }),
        ..message(
            MESSAGE_ID,
            Role::Assistant,
            MessageSource::Human,
            vec![reasoning_part(), text_part(), tool_parts().remove(0).1],
        )
    }
}

/// Assistant message whose turn ended with a final error.
pub fn failed_message() -> Message {
    Message {
        time_end: Some(T0 + 40 * SECOND),
        error: Some("429 Too Many Requests: rate limit exceeded".into()),
        model: Some(model()),
        agent: Some("build".into()),
        ..message(
            "msg_failed_01",
            Role::Assistant,
            MessageSource::Human,
            vec![],
        )
    }
}

pub fn snapshot() -> SessionSnapshot {
    SessionSnapshot {
        info: info(),
        messages: vec![user_message(), assistant_message()],
        more_before: true,
        pending_permissions: vec![permission_request()],
        queued: vec![peer_message()],
    }
}

pub fn retrying_info() -> SessionInfo {
    SessionInfo {
        status: SessionStatus::Retrying {
            attempt: 2,
            next_at: T0 + 100 * SECOND,
            error: "529 Overloaded".into(),
        },
        ..info()
    }
}

pub fn failed_info() -> SessionInfo {
    SessionInfo {
        status: SessionStatus::Idle,
        busy_since: None,
        last_error: Some("429 Too Many Requests: rate limit exceeded".into()),
        ..info()
    }
}

pub fn server_config() -> ServerConfigInfo {
    let agent = |name: &str, description: &str, color: Option<&str>| AgentInfo {
        name: name.into(),
        description: description.into(),
        model: None,
        creatable: true,
        can_spawn: name == "build",
        color: color.map(str::to_string),
    };
    ServerConfigInfo {
        agents: vec![
            agent("build", "Writes and changes code", Some("#3b82f6")),
            agent("plan", "Reads and plans, never edits", None),
        ],
        default_agent: "build".into(),
        models: vec![ModelInfo {
            provider: "anthropic".into(),
            provider_name: "Anthropic".into(),
            id: "claude-sonnet-4".into(),
            name: "Claude Sonnet 4".into(),
            context: 200_000,
            output: 64_000,
            images: true,
            reasoning: true,
            variants: vec!["low".into(), "medium".into(), "high".into()],
        }],
        commands: vec![CommandInfo {
            name: "review".into(),
            description: "Review the current changes".into(),
            source: "config".into(),
        }],
        mcp: vec![("github".into(), "connected (12 tools)".into())],
        tui: Value::Null,
    }
}

pub fn mcp_servers() -> Vec<McpServerStatus> {
    vec![
        McpServerStatus {
            name: "github".into(),
            status: "connected".into(),
            error: None,
            tools: vec!["github_search_issues".into(), "github_get_pr".into()],
        },
        McpServerStatus {
            name: "docs".into(),
            status: "failed".into(),
            error: Some("spawn docs-mcp: No such file or directory".into()),
            tools: vec![],
        },
    ]
}
