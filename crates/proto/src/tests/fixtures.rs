//! JSON fixtures in `crates/proto/fixtures`, one directory per protocol type:
//!
//! | directory       | type            | file content                                  |
//! |-----------------|-----------------|-----------------------------------------------|
//! | `request`       | [`ClientFrame`] | a whole client frame (with `id`)              |
//! | `server_frame`  | [`ServerFrame`] | a whole server frame                          |
//! | `session_event` | [`SessionEvent`]| the event inside an `event` frame             |
//! | `part`          | [`Part`]        |                                               |
//! | `tool_state`    | [`ToolState`]   | a bare string                                 |
//! | `delta`         | [`Delta`]       |                                               |
//!
//! `<variant>.json` is the main sample of a variant; more samples of the same variant are
//! `<variant>_<scenario>.json`. The `response_*` files of `server_frame` show the `result` of
//! the answer to each request that has one.

use super::bindings::{declared_literals, declared_variants};
use super::samples::*;
use super::sync::{Tree, read_tree, sync};
use crate::*;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::PathBuf;

/// Version shown in the `response_hello` sample, fixed so version bumps do not touch fixtures.
const SAMPLE_SERVER_VERSION: &str = "0.2.0";
/// A 1x1 PNG.
const PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/q842iQAAAABJRU5ErkJggg==";

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

type Samples<T> = Vec<(&'static str, T)>;

fn frame(id: u64, req: Request) -> ClientFrame {
    ClientFrame { id: Some(id), req }
}

fn requests() -> Samples<ClientFrame> {
    let sid = || SESSION_ID.to_string();
    let image = ImageData::new("image/png", PNG_BASE64);
    vec![
        (
            "hello",
            frame(
                1,
                Request::Hello {
                    client: "ref-web".into(),
                    version: PROTOCOL_VERSION,
                    cwd: "/home/user/project".into(),
                    token: Some("3f9c1d2e".into()),
                    notify: vec![
                        NotifyKind::Permission,
                        NotifyKind::TurnDone,
                        NotifyKind::TurnError,
                    ],
                },
            ),
        ),
        ("get_config", frame(2, Request::GetConfig)),
        ("list_sessions", frame(3, Request::ListSessions)),
        (
            "list_history",
            frame(
                4,
                Request::ListHistory {
                    cwd: Some("/home/user/project".into()),
                    limit: Some(50),
                },
            ),
        ),
        (
            "create_session",
            frame(
                5,
                Request::CreateSession {
                    cwd: "/home/user/project".into(),
                    agent: Some("build".into()),
                    model: Some(model()),
                    name: Some("parser-fix".into()),
                },
            ),
        ),
        (
            "attach",
            frame(
                6,
                Request::Attach {
                    session_id: sid(),
                    foreground: true,
                    limit: Some(100),
                },
            ),
        ),
        (
            "messages",
            frame(
                7,
                Request::Messages {
                    session_id: sid(),
                    before: "msg_user_01".into(),
                    limit: Some(100),
                },
            ),
        ),
        (
            "get_image",
            frame(
                8,
                Request::GetImage {
                    session_id: sid(),
                    image_id: format!("{MESSAGE_ID}:1:0"),
                },
            ),
        ),
        ("detach", frame(9, Request::Detach { session_id: sid() })),
        ("watch_all", frame(10, Request::WatchAll { enabled: true })),
        (
            "prompt",
            frame(
                11,
                Request::Prompt {
                    session_id: sid(),
                    text: "Fix the failing parser test".into(),
                    images: vec![],
                },
            ),
        ),
        (
            "prompt_with_image",
            frame(
                12,
                Request::Prompt {
                    session_id: sid(),
                    text: "This is what I see".into(),
                    images: vec![image],
                },
            ),
        ),
        (
            "remove_queued",
            frame(
                13,
                Request::RemoveQueued {
                    session_id: sid(),
                    message_id: "msg_user_02".into(),
                },
            ),
        ),
        (
            "shell",
            frame(
                14,
                Request::Shell {
                    session_id: sid(),
                    command: "git status --short".into(),
                },
            ),
        ),
        ("abort", frame(15, Request::Abort { session_id: sid() })),
        (
            "permission_reply",
            frame(
                16,
                Request::PermissionReply {
                    session_id: sid(),
                    call_id: CALL_ID.into(),
                    allow: true,
                    always: false,
                },
            ),
        ),
        (
            "set_model",
            frame(
                17,
                Request::SetModel {
                    session_id: sid(),
                    model: model(),
                },
            ),
        ),
        (
            "set_agent",
            frame(
                18,
                Request::SetAgent {
                    session_id: sid(),
                    agent: "plan".into(),
                },
            ),
        ),
        (
            "rename",
            frame(
                19,
                Request::Rename {
                    session_id: sid(),
                    name: "parser-fix-2".into(),
                },
            ),
        ),
        (
            "set_cwd",
            frame(
                20,
                Request::SetCwd {
                    session_id: sid(),
                    path: "~/project/crates/parser".into(),
                },
            ),
        ),
        ("compact", frame(21, Request::Compact { session_id: sid() })),
        (
            "undo",
            frame(
                22,
                Request::Undo {
                    session_id: sid(),
                    message_id: None,
                },
            ),
        ),
        (
            "undo_message",
            frame(
                23,
                Request::Undo {
                    session_id: sid(),
                    message_id: Some("msg_user_01".into()),
                },
            ),
        ),
        ("close", frame(24, Request::Close { session_id: sid() })),
        ("delete", frame(25, Request::Delete { session_id: sid() })),
        (
            "expand_command",
            frame(
                26,
                Request::ExpandCommand {
                    name: "review".into(),
                    args: "src/parser.rs".into(),
                    cwd: "/home/user/project".into(),
                },
            ),
        ),
        (
            "list_files",
            frame(
                27,
                Request::ListFiles {
                    cwd: "/home/user/project".into(),
                    query: "pars".into(),
                    limit: Some(20),
                },
            ),
        ),
        (
            "set_variant",
            frame(
                28,
                Request::SetVariant {
                    session_id: sid(),
                    variant: Some("low".into()),
                },
            ),
        ),
        (
            "export",
            frame(
                29,
                Request::Export {
                    session_id: sid(),
                    path: "/tmp/parser-fix.md".into(),
                },
            ),
        ),
        (
            "peer_command",
            frame(
                30,
                Request::PeerCommand {
                    session_id: sid(),
                    command: "peers".into(),
                    args: String::new(),
                },
            ),
        ),
        (
            "mcp_status",
            frame(
                31,
                Request::McpStatus {
                    cwd: Some("/home/user/project".into()),
                },
            ),
        ),
        (
            "set_notify",
            frame(
                32,
                Request::SetNotify {
                    kinds: vec![NotifyKind::Permission, NotifyKind::TurnError],
                },
            ),
        ),
        (
            "set_focus",
            frame(
                33,
                Request::SetFocus {
                    focused: true,
                    sessions: vec![sid(), "ses_e5f6a7b8".into()],
                },
            ),
        ),
        (
            "list_dirs",
            frame(
                34,
                Request::ListDirs {
                    path: "~/pro".into(),
                },
            ),
        ),
        ("shutdown", frame(35, Request::Shutdown)),
        ("ping", frame(36, Request::Ping)),
    ]
}

fn response(id: u64, result: Value) -> ServerFrame {
    ServerFrame::Response {
        id,
        ok: true,
        result,
        error: None,
    }
}

fn hello_result() -> Value {
    let instance = InstanceInfo {
        pid: 4242,
        port: 41873,
        bind: "127.0.0.1".into(),
        allow_hosts: vec!["127.0.0.1".into(), "::1".into(), "localhost".into()],
        idle_timeout: 0,
        started_at: 1_760_000_000_000,
        version: SAMPLE_SERVER_VERSION.into(),
        token: String::new(),
    };
    let mut instance = serde_json::to_value(instance).expect("instance");
    // The server leaves the token out of the answer.
    instance.as_object_mut().expect("object").remove("token");
    json!({
        "server_version": SAMPLE_SERVER_VERSION,
        "protocol": PROTOCOL_VERSION,
        "instance": instance,
        "cwd": "/home/user/project",
    })
}

fn server_frames() -> Samples<ServerFrame> {
    let image = ImageData::new("image/png", PNG_BASE64);
    vec![
        ("response", response(2, json!({}))),
        (
            "response_error",
            ServerFrame::Response {
                id: 5,
                ok: false,
                result: Value::Null,
                error: Some("Directory does not exist: /home/user/nowhere".into()),
            },
        ),
        ("response_hello", response(1, hello_result())),
        (
            "response_get_config",
            response(2, json!({"config": server_config()})),
        ),
        (
            "response_list_sessions",
            response(3, json!({"sessions": [info()]})),
        ),
        (
            "response_list_history",
            response(4, json!({"sessions": [failed_info()]})),
        ),
        (
            "response_create_session",
            response(5, json!({"session": info()})),
        ),
        (
            "response_attach",
            response(6, json!({"snapshot": snapshot()})),
        ),
        (
            "response_messages",
            response(
                7,
                json!({"messages": [user_message()], "more_before": false}),
            ),
        ),
        ("response_get_image", response(8, json!({"image": image}))),
        (
            "response_watch_all",
            response(10, json!({"sessions": [info(), retrying_info()]})),
        ),
        (
            "response_prompt",
            response(11, json!({"queued": true, "message_id": "msg_user_02"})),
        ),
        (
            "response_remove_queued",
            response(
                13,
                json!({"removed": true, "text": "Also check the docs", "images": [NamedImage {
                    image: ImageData::new("image/png", PNG_BASE64),
                    name: Some("screenshot.png".into()),
                }]}),
            ),
        ),
        (
            "response_set_model",
            response(17, json!({"session": info()})),
        ),
        (
            "response_undo",
            response(
                22,
                json!({"text": "Fix the failing parser test", "images": []}),
            ),
        ),
        (
            "response_expand_command",
            response(26, json!({"text": "Review src/parser.rs"})),
        ),
        (
            "response_list_files",
            response(
                27,
                json!({"files": ["src/parser.rs", "src/parser/", "tests/parser.rs"]}),
            ),
        ),
        (
            "response_export",
            response(29, json!({"path": "/tmp/parser-fix.md"})),
        ),
        (
            "response_peer_command",
            response(30, json!({"text": "reviewer  ses_e5f6a7b8  idle"})),
        ),
        (
            "response_mcp_status",
            response(31, json!({"servers": mcp_servers()})),
        ),
        (
            "response_list_dirs",
            response(
                34,
                json!({"path": "/home/user/project", "dirs": ["benches", "crates", "docs"]}),
            ),
        ),
        (
            "event",
            ServerFrame::Event {
                session_id: SESSION_ID.into(),
                event: SessionEvent::Delta {
                    message_id: MESSAGE_ID.into(),
                    index: 1,
                    delta: Delta::Text {
                        text: " The loop".into(),
                    },
                },
            },
        ),
        (
            "sessions",
            ServerFrame::Sessions {
                sessions: vec![info(), retrying_info(), failed_info()],
            },
        ),
        (
            "toast",
            ServerFrame::Toast {
                level: ToastLevel::Warn,
                text: "MCP server docs failed to start".into(),
            },
        ),
        (
            "notify",
            ServerFrame::Notify {
                session_id: SESSION_ID.into(),
                notify_kind: NotifyKind::Permission,
                session_title: "Fix the failing parser test".into(),
                detail: Some("edit".into()),
            },
        ),
        (
            "notify_turn_done",
            ServerFrame::Notify {
                session_id: SESSION_ID.into(),
                notify_kind: NotifyKind::TurnDone,
                session_title: "Fix the failing parser test".into(),
                detail: None,
            },
        ),
        (
            "notify_turn_error",
            ServerFrame::Notify {
                session_id: SESSION_ID.into(),
                notify_kind: NotifyKind::TurnError,
                session_title: "Fix the failing parser test".into(),
                detail: Some("429 Too Many Requests: rate limit exceeded".into()),
            },
        ),
    ]
}

fn session_events() -> Samples<SessionEvent> {
    let streaming = tool_parts().remove(1).1;
    let done = tool_parts().remove(5).1;
    vec![
        ("info", SessionEvent::Info { info: info() }),
        (
            "info_retrying",
            SessionEvent::Info {
                info: retrying_info(),
            },
        ),
        (
            "info_failed",
            SessionEvent::Info {
                info: failed_info(),
            },
        ),
        (
            "message",
            SessionEvent::Message {
                message: assistant_message(),
            },
        ),
        (
            "message_user",
            SessionEvent::Message {
                message: user_message(),
            },
        ),
        (
            "message_peer",
            SessionEvent::Message {
                message: peer_message(),
            },
        ),
        (
            "message_error",
            SessionEvent::Message {
                message: failed_message(),
            },
        ),
        (
            "part",
            SessionEvent::Part {
                message_id: MESSAGE_ID.into(),
                index: 2,
                part: streaming,
            },
        ),
        (
            "part_tool_done",
            SessionEvent::Part {
                message_id: MESSAGE_ID.into(),
                index: 2,
                part: done,
            },
        ),
        (
            "delta",
            SessionEvent::Delta {
                message_id: MESSAGE_ID.into(),
                index: 1,
                delta: Delta::Text {
                    text: " runs one step".into(),
                },
            },
        ),
        (
            "delta_tool_input",
            SessionEvent::Delta {
                message_id: MESSAGE_ID.into(),
                index: 2,
                delta: Delta::ToolInput {
                    text: r#"s/parser.rs", "tag": "a1"#.into(),
                },
            },
        ),
        (
            "removed",
            SessionEvent::Removed {
                message_id: "msg_failed_01".into(),
            },
        ),
        (
            "reset",
            SessionEvent::Reset {
                messages: vec![user_message(), assistant_message()],
                more_before: true,
            },
        ),
        (
            "permission",
            SessionEvent::Permission {
                request: permission_request(),
            },
        ),
        (
            "permission_resolved",
            SessionEvent::PermissionResolved {
                call_id: CALL_ID.into(),
            },
        ),
        (
            "queue",
            SessionEvent::Queue {
                messages: vec![peer_message()],
            },
        ),
        (
            "error",
            SessionEvent::Error {
                text: "Automatic compaction failed: request timed out".into(),
            },
        ),
        ("closed", SessionEvent::Closed),
    ]
}

fn parts() -> Samples<Part> {
    let mut all = vec![
        ("text", text_part()),
        ("reasoning", reasoning_part()),
        ("image", image_part()),
    ];
    all.extend(tool_parts());
    all
}

fn tool_states() -> Samples<ToolState> {
    vec![
        ("streaming", ToolState::Streaming),
        ("awaiting_permission", ToolState::AwaitingPermission),
        ("delayed", ToolState::Delayed),
        ("running", ToolState::Running),
        ("done", ToolState::Done),
        ("error", ToolState::Error),
        ("denied", ToolState::Denied),
        ("cancelled", ToolState::Cancelled),
    ]
}

fn deltas() -> Samples<Delta> {
    vec![
        (
            "text",
            Delta::Text {
                text: " runs one step".into(),
            },
        ),
        (
            "reasoning",
            Delta::Reasoning {
                text: " so the range is".into(),
            },
        ),
        (
            "tool_input",
            Delta::ToolInput {
                text: r#"s/parser.rs", "tag": "a1"#.into(),
            },
        ),
    ]
}

fn add<T: Serialize>(tree: &mut Tree, dir: &str, samples: Samples<T>) {
    for (name, value) in samples {
        let mut text = serde_json::to_string_pretty(&value).expect("serialize sample");
        text.push('\n');
        tree.insert(format!("{dir}/{name}.json"), text);
    }
}

fn expected() -> Tree {
    let mut tree = Tree::new();
    add(&mut tree, "request", requests());
    add(&mut tree, "server_frame", server_frames());
    add(&mut tree, "session_event", session_events());
    add(&mut tree, "part", parts());
    add(&mut tree, "tool_state", tool_states());
    add(&mut tree, "delta", deltas());
    tree
}

#[test]
fn fixtures_are_up_to_date() {
    sync(&fixtures_dir(), &expected(), "Fixtures");
}

/// Every fixture of `dir` parses as `T` and serializes back to the same JSON.
fn check_roundtrip<T: Serialize + DeserializeOwned>(dir: &str) {
    let tree = read_tree(&fixtures_dir().join(dir));
    assert!(!tree.is_empty(), "no fixtures in {dir}");
    for (name, text) in tree {
        let value: Value = serde_json::from_str(&text).expect("valid JSON");
        let typed: T = serde_json::from_value(value.clone())
            .unwrap_or_else(|e| panic!("{dir}/{name} does not parse: {e}"));
        let back = serde_json::to_value(&typed).expect("serialize");
        assert_eq!(back, value, "{dir}/{name} changes when parsed and written");
    }
}

#[test]
fn fixtures_parse_as_their_types() {
    check_roundtrip::<ClientFrame>("request");
    check_roundtrip::<ServerFrame>("server_frame");
    check_roundtrip::<SessionEvent>("session_event");
    check_roundtrip::<Part>("part");
    check_roundtrip::<ToolState>("tool_state");
    check_roundtrip::<Delta>("delta");
}

/// Every declared variant has a fixture, and each file is named after its variant.
/// `tag` is the JSON field that names the variant; `None` for a plain string enum.
fn check_variants(dir: &str, tag: Option<&str>, declared: Vec<String>) {
    let mut seen = BTreeSet::new();
    for (path, text) in read_tree(&fixtures_dir().join(dir)) {
        let value: Value = serde_json::from_str(&text).expect("valid JSON");
        let variant = match tag {
            Some(tag) => value[tag].as_str().expect("tag field").to_string(),
            None => value.as_str().expect("string enum").to_string(),
        };
        let stem = path.trim_end_matches(".json");
        let named_after_variant = stem == variant || stem.starts_with(&format!("{variant}_"));
        assert!(named_after_variant, "{dir}/{path} holds variant {variant}");
        seen.insert(variant);
    }
    let declared: BTreeSet<String> = declared.into_iter().collect();
    assert!(!declared.is_empty(), "no variants declared for {dir}");
    assert_eq!(
        seen, declared,
        "variants without fixtures (right) or unknown (left) in {dir}"
    );
}

#[test]
fn every_variant_has_a_fixture() {
    check_variants("request", Some("op"), declared_variants::<Request>("op"));
    check_variants(
        "server_frame",
        Some("kind"),
        declared_variants::<ServerFrame>("kind"),
    );
    check_variants(
        "session_event",
        Some("type"),
        declared_variants::<SessionEvent>("type"),
    );
    check_variants("part", Some("type"), declared_variants::<Part>("type"));
    check_variants("tool_state", None, declared_literals::<ToolState>());
    check_variants("delta", Some("type"), declared_variants::<Delta>("type"));
}
