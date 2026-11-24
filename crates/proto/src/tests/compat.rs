//! Frames that older clients send must keep parsing.

use crate::*;
use serde_json::json;

/// Protocol 1 clients know no notifications: their `hello` has no `notify` field.
#[test]
fn hello_without_notify_still_parses() {
    let old = json!({
        "id": 1,
        "op": "hello",
        "client": "ref",
        "version": 1,
        "cwd": "/home/user/project",
        "token": "secret",
    });
    let frame: ClientFrame = serde_json::from_value(old).expect("old hello parses");
    match frame.req {
        Request::Hello {
            version, notify, ..
        } => {
            assert_eq!(version, 1);
            assert!(notify.is_empty(), "no notification kinds without the field");
        }
        other => panic!("not a hello: {other:?}"),
    }
}

/// A snapshot from a protocol 2 server has no `pending_questions`.
#[test]
fn snapshot_without_questions_still_parses() {
    let mut value = serde_json::to_value(super::samples::snapshot()).unwrap();
    value.as_object_mut().unwrap().remove("pending_questions");
    let snapshot: SessionSnapshot = serde_json::from_value(value).expect("old snapshot parses");
    assert!(snapshot.pending_questions.is_empty());
}

/// `custom` defaults to true and options are not preselected unless they say so.
#[test]
fn question_defaults() {
    let q: Question = serde_json::from_value(json!({
        "question": "Which?",
        "header": "Pick",
        "options": [{"label": "A", "description": "first"}],
    }))
    .unwrap();
    assert!(q.custom && !q.multiple && !q.options[0].default);
}

/// A response from a 0.2.0 server has no `code` field; a coded response round-trips.
#[test]
fn response_code_is_optional_and_round_trips() {
    let legacy: ServerFrame = serde_json::from_value(json!({
        "kind": "response",
        "id": 1,
        "ok": false,
        "result": null,
        "error": NO_PENDING_QUESTION,
    }))
    .unwrap();
    match legacy {
        ServerFrame::Response {
            ok, error, code, ..
        } => {
            assert!(!ok);
            assert_eq!(error.as_deref(), Some(NO_PENDING_QUESTION));
            assert_eq!(code, None);
        }
        other => panic!("not a response: {other:?}"),
    }

    let coded = ServerFrame::Response {
        id: 2,
        ok: false,
        result: serde_json::Value::Null,
        error: Some(NO_PENDING_QUESTION.into()),
        code: Some(ERR_NO_PENDING_QUESTION.into()),
    };
    let json = serde_json::to_string(&coded).unwrap();
    assert!(json.contains("\"code\":\"no_pending_question\""), "{json}");
    let back: ServerFrame = serde_json::from_str(&json).unwrap();
    match back {
        ServerFrame::Response { code, .. } => {
            assert_eq!(code.as_deref(), Some(ERR_NO_PENDING_QUESTION));
        }
        other => panic!("not a response: {other:?}"),
    }
}

/// A `PermissionRequest` from a 0.2.0 server has no `preview`; with `preview` it round-trips.
#[test]
fn permission_request_preview_is_optional_and_round_trips() {
    let legacy: PermissionRequest = serde_json::from_value(json!({
        "session_id": "s1",
        "message_id": "m1",
        "call_id": "c1",
        "tool": "edit",
        "purpose": "Fix bug",
        "input": {"path": "a.rs"},
        "time": 123,
    }))
    .unwrap();
    assert_eq!(legacy.preview, None);
    let legacy_json = serde_json::to_string(&legacy).unwrap();
    assert!(!legacy_json.contains("preview"), "{legacy_json}");

    let with_preview = PermissionRequest {
        preview: Some(EditPreview {
            diff: "--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n".into(),
            added: 1,
            removed: 1,
            truncated: true,
        }),
        ..legacy
    };
    let json = serde_json::to_string(&with_preview).unwrap();
    let back: PermissionRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.preview, with_preview.preview);
}

/// Bare `get_config` and `list_dirs` without v5 optional fields still parse.
#[test]
fn bare_get_config_and_list_dirs_still_parse() {
    let gc: ClientFrame = serde_json::from_value(json!({"id": 2, "op": "get_config"})).unwrap();
    match &gc.req {
        Request::GetConfig { cwd } => assert_eq!(cwd, &None),
        other => panic!("not get_config: {other:?}"),
    }
    let gc_json = serde_json::to_value(&gc).unwrap();
    assert_eq!(gc_json, json!({"id": 2, "op": "get_config"}));

    let ld: ClientFrame =
        serde_json::from_value(json!({"id": 3, "op": "list_dirs", "path": "."})).unwrap();
    match ld.req {
        Request::ListDirs {
            path,
            cwd,
            include_files,
        } => {
            assert_eq!(path, ".");
            assert_eq!(cwd, None);
            assert!(!include_files);
        }
        other => panic!("not list_dirs: {other:?}"),
    }
}

/// `complete_shell` parses with and without optional `cursor`/`limit`, and `ShellCompletionItem` round-trips.
#[test]
fn complete_shell_optional_fields_and_roundtrip() {
    let minimal: ClientFrame = serde_json::from_value(json!({
        "id": 44,
        "op": "complete_shell",
        "cwd": "/home/user/project",
        "line": "git che",
    }))
    .expect("minimal complete_shell parses");
    match &minimal.req {
        Request::CompleteShell {
            cwd,
            line,
            cursor,
            limit,
        } => {
            assert_eq!(cwd, "/home/user/project");
            assert_eq!(line, "git che");
            assert_eq!(*cursor, None);
            assert_eq!(*limit, None);
        }
        other => panic!("not complete_shell: {other:?}"),
    }
    let min_val = serde_json::to_value(&minimal).unwrap();
    assert_eq!(
        min_val,
        json!({
            "id": 44,
            "op": "complete_shell",
            "cwd": "/home/user/project",
            "line": "git che",
        })
    );

    let full = ClientFrame {
        id: Some(45),
        req: Request::CompleteShell {
            cwd: "/home/user/project".into(),
            line: "git che".into(),
            cursor: Some(7),
            limit: Some(50),
        },
    };
    let json = serde_json::to_string(&full).unwrap();
    let back: ClientFrame = serde_json::from_str(&json).unwrap();
    match back.req {
        Request::CompleteShell { cursor, limit, .. } => {
            assert_eq!(cursor, Some(7));
            assert_eq!(limit, Some(50));
        }
        other => panic!("not complete_shell: {other:?}"),
    }

    let bare_item: ShellCompletionItem =
        serde_json::from_value(json!({"value": "status"})).unwrap();
    assert_eq!(bare_item.description, "");
    let bare_json = serde_json::to_value(&bare_item).unwrap();
    assert_eq!(bare_json, json!({"value": "status"}));
}

#[test]
fn messages_before_and_range_roundtrip() {
    let before_req: ClientFrame = serde_json::from_value(json!({
        "id": 7,
        "op": "messages",
        "session_id": "ses_1",
        "before": "msg_1",
        "limit": 50
    }))
    .unwrap();
    match &before_req.req {
        Request::Messages {
            session_id,
            before,
            start_seq,
            end_seq,
            limit,
        } => {
            assert_eq!(session_id, "ses_1");
            assert_eq!(before.as_deref(), Some("msg_1"));
            assert_eq!(*start_seq, None);
            assert_eq!(*end_seq, None);
            assert_eq!(*limit, Some(50));
        }
        other => panic!("not messages: {other:?}"),
    }

    let range_req: ClientFrame = serde_json::from_value(json!({
        "id": 8,
        "op": "messages",
        "session_id": "ses_1",
        "start_seq": 10,
        "end_seq": 60
    }))
    .unwrap();
    match &range_req.req {
        Request::Messages {
            before,
            start_seq,
            end_seq,
            limit,
            ..
        } => {
            assert_eq!(*before, None);
            assert_eq!(*start_seq, Some(10));
            assert_eq!(*end_seq, Some(60));
            assert_eq!(*limit, None);
        }
        other => panic!("not messages: {other:?}"),
    }
}

#[test]
fn snapshot_and_reset_without_layout_metas_still_parse() {
    let mut value = serde_json::to_value(super::samples::snapshot()).unwrap();
    let obj = value.as_object_mut().unwrap();
    obj.remove("total_messages");
    obj.remove("start_seq");
    obj.remove("layout_metas");
    let snapshot: SessionSnapshot = serde_json::from_value(value).unwrap();
    assert_eq!(snapshot.total_messages, 0);
    assert_eq!(snapshot.start_seq, 0);
    assert!(snapshot.layout_metas.is_empty());

    let reset: SessionEvent = serde_json::from_value(json!({
        "type": "reset",
        "messages": [],
        "more_before": false
    }))
    .unwrap();
    match reset {
        SessionEvent::Reset {
            total_messages,
            start_seq,
            layout_metas,
            ..
        } => {
            assert_eq!(total_messages, 0);
            assert_eq!(start_seq, 0);
            assert!(layout_metas.is_empty());
        }
        other => panic!("not reset: {other:?}"),
    }
}

#[test]
fn retry_now_and_stash_defaults_roundtrip() {
    let retry: ClientFrame = serde_json::from_value(json!({
        "id": 46,
        "op": "retry_now",
        "session_id": "ses_1"
    }))
    .unwrap();
    assert!(matches!(retry.req, Request::RetryNow { .. }));

    let add: ClientFrame = serde_json::from_value(json!({
        "id": 53,
        "op": "add_to_group",
        "group_id": "grp_1",
        "session_id": "ses_1"
    }))
    .unwrap();
    match add.req {
        Request::AddToGroup {
            position,
            stash_live,
            ..
        } => {
            assert_eq!(position, None);
            assert!(stash_live);
        }
        other => panic!("not add_to_group: {other:?}"),
    }
}
