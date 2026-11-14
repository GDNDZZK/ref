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
