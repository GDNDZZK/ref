//! End-to-end checks of `ref-server --mock`: the scripts run through the real engine, tools
//! and LLM client, and nothing outside the temporary directory is touched.

use ref_proto::{MessageSource, Part, Request, Role, ServerFrame, SessionStatus, ToolState};
use ref_server::api;
use ref_server::hub::{Conn, Hub};
use ref_server::mock::Mock;
use ref_server::ops::{self, CreateOpts};
use ref_server::session::{Session, user_message};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(40);
const POLL: Duration = Duration::from_millis(10);
/// A working directory no client should be able to reach in mock mode.
const ELSEWHERE: &str = "/nonexistent-elsewhere";

struct Rig {
    mock: Mock,
    hub: Arc<Hub>,
    conn: Arc<Conn>,
    session: Arc<Session>,
}

async fn rig() -> Rig {
    let mock = ref_server::mock::start(0, "token".into()).await.unwrap();
    let hub = mock.running.hub.clone();
    let conn = hub.add_conn();
    let session = new_session(&hub, ELSEWHERE, "main").await;
    Rig {
        mock,
        hub,
        conn,
        session,
    }
}

async fn new_session(hub: &Arc<Hub>, cwd: &str, name: &str) -> Arc<Session> {
    let opts = CreateOpts {
        cwd: cwd.into(),
        agent: None,
        model: None,
        name: Some(name.into()),
        unique_suffix: false,
        parent: None,
        must_report: false,
    };
    ops::create_session(hub, opts).await.unwrap()
}

impl Rig {
    fn prompt(&self, text: &str) {
        let msg = user_message(MessageSource::Human, text.into(), vec![]);
        ref_server::engine::submit(&self.hub, &self.session, msg);
    }

    /// Runs until the session is idle, answering permission requests with `allow`.
    /// Returns the statuses seen on the way (consecutive duplicates removed).
    async fn run(&self, allow: bool) -> Vec<SessionStatus> {
        let start = Instant::now();
        let mut seen: Vec<SessionStatus> = Vec::new();
        while self.session.lock().run.is_some() {
            assert!(start.elapsed() < WAIT, "the script did not finish");
            let status = self.session.lock().info.status.clone();
            if seen.last() != Some(&status) {
                seen.push(status);
            }
            for call_id in self.awaiting_permission() {
                let req = Request::PermissionReply {
                    session_id: self.session.id.clone(),
                    call_id,
                    allow,
                    always: false,
                };
                api::handle(&self.hub, &self.conn, req).await.unwrap();
            }
            tokio::time::sleep(POLL).await;
        }
        seen
    }

    fn awaiting_permission(&self) -> Vec<String> {
        self.tools()
            .into_iter()
            .filter(|t| t.state == ToolState::AwaitingPermission)
            .map(|t| t.call_id)
            .collect()
    }

    fn tools(&self) -> Vec<Tool> {
        let messages = self.session.lock().messages.clone();
        messages
            .into_iter()
            .flat_map(|m| m.parts)
            .filter_map(|p| match p {
                Part::Tool {
                    call_id,
                    name,
                    state,
                    ..
                } => Some(Tool {
                    call_id,
                    name,
                    state,
                }),
                _ => None,
            })
            .collect()
    }

    fn last_text(&self) -> String {
        let messages = self.session.lock().messages.clone();
        messages
            .iter()
            .rev()
            .find(|m| m.role == Role::Assistant)
            .map(|m| m.raw_text())
            .unwrap_or_default()
    }

    fn workspace_file(&self, rel: &str) -> String {
        std::fs::read_to_string(self.mock.workspace().join(rel)).unwrap()
    }

    /// Shuts the backend down and deletes the temporary directory.
    async fn finish(self) -> PathBuf {
        ops::shutdown_all(&self.hub).await;
        let root = self.mock.root().to_path_buf();
        drop(self.mock);
        root
    }
}

struct Tool {
    call_id: String,
    name: String,
    state: ToolState,
}

fn states(tools: &[Tool], name: &str) -> Vec<ToolState> {
    tools
        .iter()
        .filter(|t| t.name == name)
        .map(|t| t.state)
        .collect()
}

#[tokio::test]
async fn sessions_run_in_the_sample_workspace_whatever_cwd_is_asked() {
    let rig = rig().await;
    let cwd = rig.session.info().cwd;
    let workspace = dunce::canonicalize(rig.mock.workspace()).unwrap();
    assert_eq!(Path::new(&cwd), workspace);
    rig.prompt("hello");
    rig.run(true).await;
    assert!(rig.last_text().contains("mock backend"));
    rig.finish().await;
}

#[tokio::test]
async fn help_lists_the_scripts() {
    let rig = rig().await;
    rig.prompt("help");
    rig.run(true).await;
    let text = rig.last_text();
    for name in [
        "hello",
        "tools",
        "permission",
        "retry",
        "peer",
        "everything",
    ] {
        assert!(text.contains(&format!("`{name}`")), "{name} is missing");
    }
    rig.finish().await;
}

#[tokio::test]
async fn tools_script_reads_edits_and_fixes_the_bug() {
    let rig = rig().await;
    rig.prompt("tools");
    rig.run(true).await;
    let tools = rig.tools();
    assert!(
        states(&tools, "read").contains(&ToolState::Error),
        "missing file"
    );
    assert_eq!(states(&tools, "edit"), [ToolState::Done]);
    assert!(
        rig.workspace_file("src/parser.rs")
            .contains("0..items.len()")
    );
    assert!(
        !rig.workspace_file("src/parser.rs")
            .contains("0..=items.len()")
    );
    rig.finish().await;
}

#[tokio::test]
async fn a_denied_permission_leaves_the_file_alone() {
    let rig = rig().await;
    let before = rig.workspace_file("src/parser.rs");
    rig.prompt("permission");
    rig.run(false).await;
    assert_eq!(states(&rig.tools(), "edit"), [ToolState::Denied]);
    assert_eq!(rig.workspace_file("src/parser.rs"), before);
    rig.finish().await;
}

#[tokio::test]
async fn retry_shows_the_retrying_status_then_succeeds() {
    let rig = rig().await;
    rig.prompt("retry");
    let seen = rig.run(true).await;
    let retries = seen
        .iter()
        .filter(|s| matches!(s, SessionStatus::Retrying { .. }))
        .count();
    assert!(retries >= 2, "{seen:?}");
    assert!(rig.last_text().contains("attempt 3"));
    assert!(rig.session.info().last_error.is_none());
    rig.finish().await;
}

#[tokio::test]
async fn fail_ends_with_an_error() {
    let rig = rig().await;
    rig.prompt("fail");
    rig.run(true).await;
    let error = rig.session.info().last_error.expect("last_error");
    assert!(error.contains("Invalid API key"), "{error}");
    rig.finish().await;
}

#[tokio::test]
async fn a_full_context_is_compacted_before_the_next_prompt() {
    let rig = rig().await;
    rig.prompt("fill");
    rig.run(true).await;
    let info = rig.session.info();
    assert!(info.context.used >= info.context.auto, "{:?}", info.context);
    rig.prompt("hello");
    rig.run(true).await;
    let messages = rig.session.lock().messages.clone();
    let summary = messages
        .iter()
        .find(|m| m.source == MessageSource::Compaction)
        .expect("a compaction message");
    assert!(summary.raw_text().contains("## Goal"));
    rig.finish().await;
}

#[tokio::test]
async fn the_model_can_compact_its_own_context() {
    let rig = rig().await;
    rig.prompt("compact-tool");
    rig.run(true).await;
    let messages = rig.session.lock().messages.clone();
    assert!(
        messages
            .iter()
            .any(|m| m.source == MessageSource::Compaction)
    );
    rig.finish().await;
}

#[tokio::test]
async fn the_created_session_reports_back() {
    let rig = rig().await;
    rig.prompt("peer");
    rig.run(true).await;
    let session = rig.session.clone();
    let reported = move || {
        let messages = session.lock().messages.clone();
        messages
            .iter()
            .any(|m| matches!(&m.source, MessageSource::Peer { from_name, .. } if from_name.starts_with("helper")))
    };
    let start = Instant::now();
    while !reported() {
        assert!(start.elapsed() < WAIT, "no report arrived");
        tokio::time::sleep(POLL).await;
    }
    rig.run(true).await;
    assert!(rig.last_text().contains("Got this message"));
    rig.finish().await;
}

/// Time allowed for the `orchestrate` crew; its script takes about 25 seconds.
const ORCHESTRATE_WAIT: Duration = Duration::from_secs(90);
const FINAL_SENTENCE: &str = "The parser bug is fixed.";
const REPORTERS: [&str; 5] = ["scout", "fixer", "tester", "docs", "reviewer"];

impl Rig {
    /// Allows every tool that waits for permission in any session.
    async fn approve_everywhere(&self) {
        for session in self.hub.sessions() {
            let waiting: Vec<String> = session
                .lock()
                .messages
                .iter()
                .flat_map(|m| m.parts.iter())
                .filter_map(|p| match p {
                    Part::Tool {
                        call_id,
                        state: ToolState::AwaitingPermission,
                        ..
                    } => Some(call_id.clone()),
                    _ => None,
                })
                .collect();
            for call_id in waiting {
                let req = Request::PermissionReply {
                    session_id: session.id.clone(),
                    call_id,
                    allow: true,
                    always: false,
                };
                api::handle(&self.hub, &self.conn, req).await.unwrap();
            }
        }
    }

    /// Names of the sessions that reported to the main session, in arrival order.
    fn reporters(&self) -> Vec<String> {
        let messages = self.session.lock().messages.clone();
        messages
            .into_iter()
            .filter_map(|m| match m.source {
                MessageSource::Peer { from_name, .. } => Some(from_name),
                _ => None,
            })
            .collect()
    }

    fn main_texts(&self) -> String {
        let messages = self.session.lock().messages.clone();
        messages
            .iter()
            .filter(|m| m.role == Role::Assistant)
            .map(|m| m.raw_text())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[tokio::test]
async fn orchestrate_directs_a_crew_and_collects_five_reports() {
    let rig = rig().await;
    let start = Instant::now();
    rig.prompt("orchestrate fix the parser bug, test it, update the docs");
    let mut seen_reports = 0;
    while !rig.main_texts().contains(FINAL_SENTENCE) {
        assert!(
            start.elapsed() < ORCHESTRATE_WAIT,
            "no final summary; reports so far: {:?}",
            rig.reporters()
        );
        rig.approve_everywhere().await;
        let reports = rig.reporters();
        if reports.len() > seen_reports {
            eprintln!(
                "orchestrate: report {} from {} at {:.1} s",
                reports.len(),
                reports[reports.len() - 1],
                start.elapsed().as_secs_f32()
            );
            seen_reports = reports.len();
        }
        tokio::time::sleep(POLL).await;
    }
    eprintln!(
        "orchestrate: done at {:.1} s",
        start.elapsed().as_secs_f32()
    );

    let reporters = rig.reporters();
    assert_eq!(reporters.len(), REPORTERS.len(), "{reporters:?}");
    for name in REPORTERS {
        let n = reporters.iter().filter(|r| r.starts_with(name)).count();
        assert_eq!(n, 1, "{name} reports once: {reporters:?}");
    }
    let text = rig.main_texts();
    for sentence in [
        "Four helpers are working",
        "All four have reported",
        "Waiting for the review",
    ] {
        assert!(text.contains(sentence), "{sentence}");
    }
    let todos = rig.session.info().todos;
    assert_eq!(todos.len(), 5);
    assert!(
        todos
            .iter()
            .all(|t| t.status == ref_proto::TodoStatus::Completed)
    );
    let children = rig
        .hub
        .infos()
        .into_iter()
        .filter(|i| i.parent.as_deref() == Some(rig.session.id.as_str()))
        .count();
    assert_eq!(children, REPORTERS.len());
    assert!(
        rig.workspace_file("src/parser.rs")
            .contains("0..items.len()")
    );
    assert!(
        rig.workspace_file("NOTES.txt")
            .contains("Fix off-by-one in parse_items")
    );
    rig.finish().await;
}

#[tokio::test]
async fn cancel_stops_a_command_that_never_ends() {
    let rig = rig().await;
    rig.prompt("cancel");
    let start = Instant::now();
    while !rig.tools().iter().any(|t| t.state == ToolState::Running) {
        assert!(start.elapsed() < WAIT, "the command never started");
        tokio::time::sleep(POLL).await;
    }
    let req = Request::Abort {
        session_id: rig.session.id.clone(),
    };
    api::handle(&rig.hub, &rig.conn, req).await.unwrap();
    rig.run(true).await;
    assert!(rig.tools().iter().all(|t| t.state == ToolState::Cancelled));
    rig.finish().await;
}

#[tokio::test]
async fn the_old_sessions_are_in_the_history() {
    let rig = rig().await;
    let req = Request::ListHistory {
        cwd: None,
        limit: None,
    };
    let res = api::handle(&rig.hub, &rig.conn, req).await.unwrap();
    let stored = res["sessions"].as_array().unwrap();
    let old = stored.iter().filter(|s| s["live"] == false).count();
    assert_eq!(old, 3);
    rig.finish().await;
}

/// Lists every file under `dir` with its size and modification time.
fn snapshot(dir: &Path) -> Vec<(PathBuf, u64, std::time::SystemTime)> {
    let mut out = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for entry in std::fs::read_dir(&d).unwrap() {
            let entry = entry.unwrap();
            // Not `entry.metadata()`: on Windows that is the directory index's copy, which
            // NTFS refreshes lazily, so a directory's time can lag behind its contents.
            let meta = std::fs::metadata(entry.path()).unwrap();
            if meta.is_dir() {
                todo.push(entry.path());
            }
            out.push((entry.path(), meta.len(), meta.modified().unwrap()));
        }
    }
    out.sort();
    out
}

#[tokio::test]
async fn nothing_outside_the_temporary_directory_changes() {
    let bait = std::env::temp_dir().join(format!("ref-mock-bait-{}", std::process::id()));
    std::fs::create_dir_all(bait.join("src")).unwrap();
    std::fs::write(bait.join("src/parser.rs"), "for i in 0..=items.len() {\n").unwrap();
    let before = snapshot(&bait);

    let rig = rig().await;
    let session = new_session(&rig.hub, &bait.to_string_lossy(), "bait").await;
    let rig = Rig { session, ..rig };
    rig.prompt("everything");
    rig.run(true).await;
    assert!(!rig.tools().is_empty());
    let root = rig.finish().await;

    assert_eq!(snapshot(&bait), before);
    assert!(!root.exists(), "temporary directory is still there");
    std::fs::remove_dir_all(&bait).unwrap();
}

#[tokio::test]
async fn every_frame_parses_as_a_server_frame() {
    let rig = rig().await;
    let conn = rig.hub.add_conn();
    conn.lock().hello = true;
    let req = Request::Attach {
        session_id: rig.session.id.clone(),
        foreground: true,
        limit: None,
    };
    api::handle(&rig.hub, &conn, req).await.unwrap();
    rig.prompt("markdown");
    rig.run(true).await;
    let frames = conn.out.drain();
    assert!(!frames.is_empty());
    for frame in frames {
        let text = serde_json::to_string(&frame).unwrap();
        serde_json::from_str::<ServerFrame>(&text).unwrap();
    }
    rig.finish().await;
}

#[tokio::test]
async fn the_remaining_tool_scripts_end_in_the_states_they_promise() {
    let rig = rig().await;
    let cases = [
        ("denied", ToolState::Denied),
        ("bash-fail", ToolState::Done),
        ("stream-args", ToolState::Done),
        ("image", ToolState::Done),
    ];
    for (script, state) in cases {
        let before = rig.tools().len();
        rig.prompt(script);
        rig.run(true).await;
        let tools = rig.tools();
        let last = tools
            .last()
            .unwrap_or_else(|| panic!("{script}: no tool call"));
        assert!(tools.len() > before, "{script}");
        assert_eq!(last.state, state, "{script}");
    }
    assert!(rig.workspace_file("scratch/draft.md").len() > 1000);
    rig.prompt("todo");
    rig.run(true).await;
    assert_eq!(rig.session.info().todos.len(), 3);
    rig.finish().await;
}

#[tokio::test]
async fn directory_listing_stays_inside_the_workspace() {
    let rig = rig().await;
    rig.conn.lock().cwd = rig.mock.workspace().to_string_lossy().into();
    let workspace = dunce::canonicalize(rig.mock.workspace()).unwrap();
    let list = |path: &str| {
        let req = Request::ListDirs { path: path.into() };
        api::handle(&rig.hub, &rig.conn, req)
    };

    let home = list("~").await.unwrap();
    assert_eq!(home["path"], workspace.to_string_lossy().as_ref());
    assert_eq!(home["dirs"], json!(["data", "docs", "scratch", "src"]));
    assert!(list("src/..").await.is_ok());
    // The temporary directory holds the mock's root, so it exists on every platform.
    let temp = std::env::temp_dir().to_string_lossy().into_owned();
    for outside in ["/", "..", temp.as_str(), "~/../.."] {
        let err = list(outside).await.unwrap_err();
        assert!(
            err.contains("outside the mock workspace"),
            "{outside}: {err}"
        );
    }
    rig.finish().await;
}

#[tokio::test]
async fn retry_can_be_demonstrated_again_after_an_undo() {
    let rig = rig().await;
    let retries = |seen: &[SessionStatus]| {
        seen.iter()
            .filter(|s| matches!(s, SessionStatus::Retrying { .. }))
            .count()
    };
    rig.prompt("retry");
    assert!(retries(&rig.run(true).await) >= 2);

    let undo = Request::Undo {
        session_id: rig.session.id.clone(),
        message_id: None,
    };
    api::handle(&rig.hub, &rig.conn, undo).await.unwrap();
    rig.prompt("retry");
    assert!(
        retries(&rig.run(true).await) >= 2,
        "failed again the second time"
    );
    rig.finish().await;
}
