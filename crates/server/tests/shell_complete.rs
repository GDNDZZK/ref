//! Integration tests for `Request::CompleteShell` via `ref_server::api::handle`.

mod common;

use common::*;
use ref_config::Config;
use ref_proto::{ERR_DIR_NOT_FOUND, Request, ShellCompletionItem};
use ref_server::{api, ops};

fn parse_items(val: serde_json::Value) -> Vec<ShellCompletionItem> {
    serde_json::from_value(val["items"].clone()).expect("items array")
}

#[tokio::test]
async fn complete_shell_sandbox_mock_mode_and_limit() {
    let mock = ref_server::mock::start(0, "token".into()).await.unwrap();
    let hub = mock.running.hub.clone();
    let conn = hello_conn(&hub);

    // 1. `docker ` returns all docker mock completions, respecting `limit`.
    let res = call(
        &hub,
        &conn,
        Request::CompleteShell {
            cwd: "/nonexistent-elsewhere".into(),
            line: "docker ".into(),
            cursor: Some(7),
            limit: Some(3),
        },
    )
    .await;
    let items = parse_items(res);
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].value, "compose");

    // 2. `docker comp` filters by prefix and preserves descriptions.
    let res = call(
        &hub,
        &conn,
        Request::CompleteShell {
            cwd: String::new(),
            line: "docker comp".into(),
            cursor: None,
            limit: None,
        },
    )
    .await;
    let items = parse_items(res);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].value, "compose");
    assert!(!items[0].description.is_empty());

    // 3. Piped + wrapped `echo hi | sudo git che` resolves inner command `git`.
    let res = call(
        &hub,
        &conn,
        Request::CompleteShell {
            cwd: ".".into(),
            line: "echo hi | sudo git che".into(),
            cursor: Some(22),
            limit: Some(50),
        },
    )
    .await;
    let items = parse_items(res);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].value, "checkout");

    // 4. Flags like `cargo test --work` work case-insensitively.
    let res = call(
        &hub,
        &conn,
        Request::CompleteShell {
            cwd: ".".into(),
            line: "cargo test --WORK".into(),
            cursor: None,
            limit: Some(50),
        },
    )
    .await;
    let items = parse_items(res);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].value, "--workspace");

    // 5. Unknown commands (`ls`, `cat`, `echo`) return empty `items` so TUI falls back to ListDirs.
    for line in ["ls ", "cat foo", "echo hi"] {
        let res = call(
            &hub,
            &conn,
            Request::CompleteShell {
                cwd: ".".into(),
                line: line.into(),
                cursor: None,
                limit: Some(50),
            },
        )
        .await;
        assert!(parse_items(res).is_empty(), "line={line}");
    }

    // 6. Still on command token returns empty `items`.
    let res = call(
        &hub,
        &conn,
        Request::CompleteShell {
            cwd: ".".into(),
            line: "git".into(),
            cursor: None,
            limit: Some(50),
        },
    )
    .await;
    assert!(parse_items(res).is_empty());

    ops::shutdown_all(&hub).await;
    drop(mock);
}

#[tokio::test]
async fn complete_shell_validates_and_resolves_cwd_relative_to_conn() {
    let dir = temp_dir("shell-cwd");
    let sub = dir.join("sub");
    std::fs::create_dir_all(&sub).unwrap();

    let (hub, _work) = hub(Config::default(), "shell-cwd-hub");
    let conn = hello_conn(&hub);
    conn.lock().cwd = dir.to_string_lossy().into_owned();

    // Missing directory returns ERR_DIR_NOT_FOUND.
    let err = api::handle(
        &hub,
        &conn,
        Request::CompleteShell {
            cwd: "missing_subdir".into(),
            line: "git ".into(),
            cursor: None,
            limit: Some(10),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err.code, Some(ERR_DIR_NOT_FOUND));

    // Relative valid `cwd` resolves against `conn.cwd`; command token returns empty cleanly.
    let ok = call(
        &hub,
        &conn,
        Request::CompleteShell {
            cwd: "sub".into(),
            line: "git".into(),
            cursor: None,
            limit: Some(10),
        },
    )
    .await;
    assert!(parse_items(ok).is_empty());

    ops::shutdown_all(&hub).await;
}
