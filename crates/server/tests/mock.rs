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
    rig_on(mock).await
}

async fn rig_on(mock: Mock) -> Rig {
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
                    output,
                    ..
                } => Some(Tool {
                    call_id,
                    name,
                    state,
                    output: output.unwrap_or_default(),
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
    output: String,
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
        "question",
        "question-one",
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

    /// The text of the message the session whose name starts with `name` sent to main.
    fn report_from(&self, name: &str) -> String {
        let messages = self.session.lock().messages.clone();
        messages
            .into_iter()
            .find(|m| matches!(&m.source, MessageSource::Peer { from_name, .. } if from_name.starts_with(name)))
            .map(|m| m.raw_text())
            .unwrap_or_default()
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

impl Rig {
    /// Waits for the model's question and answers it.
    async fn answer_question(&self, answers: Option<Vec<Vec<&str>>>) {
        let start = Instant::now();
        let call_id = loop {
            let pending = self
                .session
                .lock()
                .questions
                .first()
                .map(|q| q.request.call_id.clone());
            if let Some(id) = pending {
                break id;
            }
            assert!(start.elapsed() < WAIT, "no question was asked");
            tokio::time::sleep(POLL).await;
        };
        let answers = answers.map(|a| {
            a.into_iter()
                .map(|e| e.into_iter().map(String::from).collect())
                .collect()
        });
        let req = Request::QuestionReply {
            session_id: self.session.id.clone(),
            call_id,
            answers,
        };
        api::handle(&self.hub, &self.conn, req).await.unwrap();
    }
}

#[tokio::test]
async fn question_one_takes_the_default_answer() {
    let rig = rig().await;
    rig.prompt("question-one");
    rig.answer_question(Some(vec![vec!["src/parser.rs"]])).await;
    rig.run(true).await;
    assert!(
        rig.last_text().contains("You chose: "),
        "{}",
        rig.last_text()
    );
    assert!(rig.last_text().contains("src/parser.rs"));
    rig.finish().await;
}

#[tokio::test]
async fn question_asks_two_and_repeats_the_answers_or_the_dismissal() {
    let rig = rig().await;
    rig.prompt("question");
    rig.answer_question(Some(vec![
        vec!["Iterator"],
        vec!["Unit tests", "typed check"],
    ]))
    .await;
    rig.run(true).await;
    let text = rig.last_text();
    assert!(
        text.contains("Iterator") && text.contains("Unit tests, typed check"),
        "{text}"
    );

    rig.prompt("question-one");
    rig.answer_question(None).await;
    rig.run(true).await;
    assert!(rig.last_text().contains("dismissed"), "{}", rig.last_text());
    rig.finish().await;
}

/// A frontend joins and leaves; says whether the backend decided to shut down.
async fn left_alone(mock: &Mock, wait: Duration) -> bool {
    let hub = &mock.running.hub;
    let conn = hub.add_conn();
    {
        let mut c = conn.lock();
        c.hello = true;
        c.frontend = true;
    }
    hub.frontend_joined();
    hub.remove_conn(conn.id);
    tokio::time::sleep(wait).await;
    hub.shutdown.is_cancelled()
}

#[tokio::test]
async fn the_mock_exits_with_its_last_client_unless_an_idle_timeout_says_otherwise() {
    let default = ref_server::mock::start(0, "token".into()).await.unwrap();
    assert!(left_alone(&default, Duration::from_millis(100)).await);

    let staying = ref_server::mock::start_with(0, "token".into(), -1)
        .await
        .unwrap();
    assert!(!left_alone(&staying, Duration::from_millis(300)).await);

    let late = ref_server::mock::start_with(0, "token".into(), 1)
        .await
        .unwrap();
    assert!(
        !left_alone(&late, Duration::from_millis(300)).await,
        "not yet"
    );
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert!(late.running.hub.shutdown.is_cancelled());
}

#[tokio::test]
async fn the_tester_reports_a_message_typed_to_it() {
    let rig = rig().await;
    rig.prompt("orchestrate fix the parser bug, test it, update the docs");
    let start = Instant::now();
    let mut typed = false;
    while !rig.main_texts().contains(FINAL_SENTENCE) {
        assert!(start.elapsed() < ORCHESTRATE_WAIT, "no final summary");
        rig.approve_everywhere().await;
        let tester = rig
            .hub
            .sessions()
            .into_iter()
            .find(|s| s.info().name.starts_with("tester"));
        // Typed while the tester works: it waits in the queue for the next safe point.
        if let Some(tester) = tester.filter(|t| !typed && t.lock().messages.len() >= 2) {
            let msg = user_message(
                MessageSource::Human,
                "also cover empty input".into(),
                vec![],
            );
            assert!(ref_server::engine::submit(&rig.hub, &tester, msg), "queued");
            typed = true;
        }
        tokio::time::sleep(POLL).await;
    }
    assert!(typed);
    // The report is wrapped in the sender's envelope.
    let report = rig.report_from("tester");
    assert!(
        report.contains("\nTests added for the parser fix. Also covered: empty input.\n"),
        "{report}"
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

const STALE_PROMPT: &str = "stale rename parse_line to parse_row";
const RECOVERED: &str = "Recovered: file changed since read; line numbers remapped.";
const REJECTED: &str =
    "Edit rejected for scratch/src/lib.rs: file changed since it was read (tag #";

/// One run of `stale`: the shifted edit is moved, the changed one is rejected, the retry lands.
async fn check_stale_run(rig: &Rig, old: &str, new: &str) {
    let before = rig.tools().len();
    rig.prompt(STALE_PROMPT);
    // Denying every permission request would show up as a denied tool.
    rig.run(false).await;
    let tools: Vec<Tool> = rig.tools().into_iter().skip(before).collect();
    assert!(tools.iter().all(|t| t.state != ToolState::Denied));
    let edits: Vec<&Tool> = tools.iter().filter(|t| t.name == "edit").collect();
    assert_eq!(edits.len(), 3, "{:?}", rig.last_text());
    assert_eq!(edits[0].state, ToolState::Done);
    assert!(edits[0].output.contains(RECOVERED), "{}", edits[0].output);
    assert_eq!(edits[1].state, ToolState::Error);
    assert!(edits[1].output.starts_with(REJECTED), "{}", edits[1].output);
    assert!(
        edits[1]
            .output
            .contains("Current content around your anchors")
    );
    assert_eq!(edits[2].state, ToolState::Done);
    for file in ["scratch/src/parser.rs", "scratch/src/lib.rs"] {
        let text = rig.workspace_file(file);
        assert!(text.contains(new) && !text.contains(old), "{file}:\n{text}");
    }
}

#[tokio::test]
async fn stale_shows_a_remapped_edit_and_a_rejected_one_from_the_real_tool() {
    let rig = rig().await;
    check_stale_run(&rig, "parse_line", "parse_row").await;
    // The names are swapped back, so the demo can be run again in the same backend.
    check_stale_run(&rig, "parse_row", "parse_line").await;
    rig.finish().await;
}

/// Base delay of the retry backoff in the `outage` test (2 s in production).
const OUTAGE_RETRY_BASE: Duration = Duration::from_millis(100);

#[tokio::test]
async fn outage_retries_with_growing_waits_and_then_goes_on() {
    let mock = ref_server::mock::start_with_retry_base(OUTAGE_RETRY_BASE)
        .await
        .unwrap();
    let rig = rig_on(mock).await;
    rig.prompt("outage run the tests and fix the failures");
    let seen = rig.run(false).await;
    let retries: Vec<(u32, String)> = seen
        .iter()
        .filter_map(|s| match s {
            SessionStatus::Retrying { attempt, error, .. } => Some((*attempt, error.clone())),
            _ => None,
        })
        .collect();
    let attempts: Vec<u32> = retries.iter().map(|r| r.0).collect();
    assert_eq!(attempts, [1, 2, 3, 4], "{seen:?}");
    assert!(retries[0].1.contains("529") && retries[0].1.contains("Overloaded"));
    assert!(retries[1].1.contains("529"));
    // Windows words a reset as "An existing connection was forcibly closed" (os error 10054).
    let reset = retries[2].1.to_lowercase();
    assert!(
        reset.contains("connection reset") || reset.contains("forcibly closed"),
        "{}",
        retries[2].1
    );
    assert!(retries[3].1.contains("503"));

    let tools = rig.tools();
    assert!(
        tools.iter().all(|t| t.state == ToolState::Done),
        "no tool is lost to the outage"
    );
    assert_eq!(states(&tools, "bash").len(), 2);
    assert_eq!(states(&tools, "edit"), [ToolState::Done]);
    assert_eq!(rig.last_text(), "All 12 tests pass now.");
    assert!(rig.session.info().last_error.is_none());
    rig.finish().await;
}

/// Waits until some tool call has the given text in its live output.
async fn live_output_of(rig: &Rig, session: &Arc<Session>, text: &str) {
    let start = Instant::now();
    loop {
        let found = session
            .lock()
            .messages
            .iter()
            .flat_map(|m| m.parts.iter())
            .any(|p| {
                matches!(p, Part::Tool { meta: Some(meta), .. }
                if meta["live_output"].as_str().is_some_and(|o| o.contains(text)))
            });
        if found {
            return;
        }
        assert!(start.elapsed() < WAIT, "no output with {text:?}");
        rig.approve_everywhere().await;
        tokio::time::sleep(POLL).await;
    }
}

#[tokio::test]
async fn orchestrate_long_helpers_print_their_progress_while_they_work() {
    let rig = rig().await;
    rig.prompt("orchestrate-long fix the parser bug");
    let start = Instant::now();
    let mut sessions = Vec::new();
    while sessions.len() < 4 {
        assert!(start.elapsed() < WAIT, "the helpers did not start");
        sessions = rig
            .hub
            .sessions()
            .into_iter()
            .filter(|s| s.id != rig.session.id)
            .collect();
        tokio::time::sleep(POLL).await;
    }
    for (name, line) in [
        ("scout", "scout: read 2/40 files"),
        ("fixer", "fixer: patched 0/5 files"),
        ("tester", "tester: ran 1/30 tests"),
        ("docs", "docs: updated 0/9 pages"),
    ] {
        let session = sessions
            .iter()
            .find(|s| s.info().name.starts_with(name))
            .unwrap_or_else(|| panic!("no {name}"));
        live_output_of(&rig, session, line).await;
        let running = session.lock().run.is_some();
        assert!(running, "{name} is still working");
    }
    // The main session is idle while its helpers work, and the reports go to the long variant.
    assert!(rig.main_texts().contains("Four helpers are working"));
    rig.finish().await;
}

#[tokio::test]
async fn longrun_prints_a_numbered_test_and_can_be_cancelled() {
    let rig = rig().await;
    rig.prompt("longrun run every test in src/");
    live_output_of(&rig, &rig.session, "step 1/20: tests::parser_01 ... ok").await;
    let req = Request::Abort {
        session_id: rig.session.id.clone(),
    };
    api::handle(&rig.hub, &rig.conn, req).await.unwrap();
    rig.run(true).await;
    assert!(rig.tools().iter().all(|t| t.state == ToolState::Cancelled));
    rig.finish().await;
}
