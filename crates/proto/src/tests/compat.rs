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
