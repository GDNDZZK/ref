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
