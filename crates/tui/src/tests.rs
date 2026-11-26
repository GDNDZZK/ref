//! Headless rendering tests (ratatui TestBackend). Run with `--nocapture` to see the screens.

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ref_proto::*;
use serde_json::json;
use tokio::sync::mpsc;

use crate::app::{App, InitPlan, Mode};
use crate::net::Client;
use crate::state::SessionView;
use crate::text;

pub fn dump(term: &Terminal<TestBackend>) -> String {
    let buf = term.backend().buffer();
    let mut out = String::new();
    for y in 0..buf.area.height {
        let mut line = String::new();
        let mut skip = 0;
        for x in 0..buf.area.width {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            let sym = buf[(x, y)].symbol();
            line.push_str(sym);
            let w = text::width(sym);
            if w > 1 {
                skip = w - 1;
            }
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

fn ts(h: u32, m: u32, s: u32) -> i64 {
    use chrono::{Local, TimeZone};
    let today = Local::now().date_naive();
    Local
        .from_local_datetime(&today.and_hms_opt(h, m, s).unwrap())
        .single()
        .unwrap()
        .timestamp_millis()
}

/// Id and working directory of the fixture session.
pub const TEST_SESSION: &str = "ses_test";
pub const TEST_CWD: &str = "/home/u/proj";

/// Safely past the input grace period of a freshly opened modal.
pub const PAST_GRACE: std::time::Duration =
    crate::input::MODAL_GRACE.saturating_add(std::time::Duration::from_millis(50));

// Pump durations (ms) for the in-process mock backend.

/// A full mock turn, or startup with an init plan.
const PUMP_TURN_MS: u64 = 2500;
/// Tiles of background sessions to appear.
const PUMP_TILES_MS: u64 = 1500;
/// The mock backend to connect and send its config.
const PUMP_CONNECT_MS: u64 = 600;
/// A reply to a command, such as a session switch.
const PUMP_REPLY_MS: u64 = 300;
/// One polling step while waiting for a condition.
const PUMP_POLL_MS: u64 = 250;
/// A single request/response round trip.
const PUMP_ROUNDTRIP_MS: u64 = 200;
/// One housekeeping tick.
const PUMP_TICK_MS: u64 = 30;

/// Size of a frame above the transport's usual limits.
const OVERSIZED_FRAME_BYTES: usize = 20 << 20;

fn info() -> SessionInfo {
    SessionInfo {
        name: "ref-a1b2".into(),
        title: "Fix the parser".into(),
        agent: "build".into(),
        model: ModelRef {
            provider: "dgn".into(),
            model: "claude-opus-5.5".into(),
            variant: Some("high".into()),
        },
        attached: 1,
        context: ContextUsage {
            used: 42_100,
            max: 1_000_000,
            recommend: 600_000,
            auto: 850_000,
        },
        pinned_at: None,
        ..crate::util::placeholder_info(TEST_SESSION, TEST_CWD)
    }
}

#[allow(clippy::too_many_arguments)]
fn tool(
    call: &str,
    name: &str,
    purpose: &str,
    input: serde_json::Value,
    state: ToolState,
    out: Option<&str>,
    t: i64,
    te: Option<i64>,
) -> Part {
    Part::Tool {
        call_id: call.into(),
        name: name.into(),
        purpose: purpose.into(),
        input,
        raw_input: String::new(),
        state,
        output: out.map(String::from),
        meta: None,
        images: vec![],
        time: t,
        time_end: te,
        run_at: None,
        provider_data: None,
    }
}

pub fn transcript(streaming: bool) -> Vec<Message> {
    let user = Message {
        id: "m1".into(),
        role: Role::User,
        time: ts(14, 2, 11),
        time_end: None,
        source: MessageSource::Human,
        parts: vec![Part::Text {
            text: "Please fix the off-by-one in the parser and run the tests. 顺便检查中文。"
                .into(),
            time: 0,
        }],
        model: None,
        agent: Some("build".into()),
        usage: None,
        error: None,
    };
    let reasoning = "First I need to read the parser. The loop uses an inclusive range which reads one past the end. Then run cargo test to confirm the fix works";
    let mut parts = vec![
        Part::Reasoning {
            text: reasoning.into(),
            time: ts(14, 2, 12),
            time_end: Some(ts(14, 2, 15)),
            provider_data: None,
        },
        Part::Text {
            text: "I'll look at the parser.\n\n```rust\nfor i in 0..items.len() {}\n```".into(),
            time: 0,
        },
        tool(
            "c1",
            "bash",
            "Run the unit tests",
            json!({"command": "cargo test -p parser"}),
            ToolState::Done,
            Some("test result: ok"),
            ts(14, 2, 15),
            Some(ts(14, 2, 15) + 1400),
        ),
        tool(
            "c2",
            "edit",
            "Fix the off-by-one in parser",
            json!({"path": "src/parser.rs"}),
            ToolState::Error,
            Some("tag mismatch: file changed"),
            ts(14, 2, 17),
            Some(ts(14, 2, 18)),
        ),
        tool(
            "c3",
            "read",
            "Read the config",
            json!({"path": "Cargo.toml"}),
            ToolState::Denied,
            Some("denied"),
            ts(14, 2, 18),
            Some(ts(14, 2, 18)),
        ),
    ];
    if streaming {
        parts.push(tool(
            "c4",
            "grep",
            "Find other inclusive ranges",
            json!({"pattern": "0..="}),
            ToolState::Running,
            None,
            ref_proto::now_ms() - 3000,
            None,
        ));
        parts.push(Part::Reasoning { text: "Now thinking about whether the grep results include test files as well, which would be noise for this particular".into(), time: ref_proto::now_ms() - 2000, time_end: None, provider_data: None });
    }
    let asst = Message {
        id: "m2".into(),
        role: Role::Assistant,
        time: ts(14, 2, 13),
        time_end: if streaming { None } else { Some(ts(14, 2, 20)) },
        source: MessageSource::Human,
        parts,
        model: Some(ModelRef {
            provider: "dgn".into(),
            model: "claude-opus-5.5".into(),
            variant: None,
        }),
        agent: Some("build".into()),
        usage: Some(Usage {
            input: 1000,
            output: 300,
            cache_read: 11_000,
            cache_write: 0,
            reasoning: 0,
        }),
        error: None,
    };
    vec![user, asst]
}

pub fn test_app(streaming: bool) -> App {
    let (app, rx) = test_app_with_requests(streaming);
    // Keep the receiver alive so requests are accepted (and recorded as pending).
    std::mem::forget(rx);
    app
}

/// Like `test_app`, with the receiver that sees every request the app sends.
pub fn test_app_with_requests(streaming: bool) -> (App, mpsc::UnboundedReceiver<ClientFrame>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut app = App::new(Client::new(tx), TEST_CWD.into(), InitPlan::default());
    app.connected = true;
    app.ever_connected = true;
    app.config = Some(ServerConfigInfo {
        agents: vec![AgentInfo {
            name: "build".into(),
            description: String::new(),
            model: None,
            creatable: true,
            can_spawn: true,
            color: None,
        }],
        default_agent: "build".into(),
        models: vec![ModelInfo {
            provider: "dgn".into(),
            provider_name: "dgn".into(),
            id: "claude-opus-5.5".into(),
            name: "claude-opus-5.5".into(),
            context: 1_000_000,
            output: 0,
            images: true,
            reasoning: true,
            variants: vec!["high".into()],
        }],
        commands: vec![],
        mcp: vec![],
        tui: serde_json::Value::Null,
        warnings: vec![],
    });
    let mut i = info();
    if streaming {
        i.status = SessionStatus::Running;
    }
    let mut v = SessionView::new(i.clone(), true);
    let msgs = transcript(streaming);
    let total = msgs.len() as u64;
    v.load_snapshot(SessionSnapshot {
        info: i,
        messages: msgs,
        more_before: false,
        total_messages: total,
        start_seq: 0,
        layout_metas: vec![],
        pending_permissions: vec![],
        pending_questions: vec![],
        queued: vec![],
    });
    app.views.insert(TEST_SESSION.into(), v);
    app.current = Some(TEST_SESSION.into());
    (app, rx)
}

pub fn render(app: &mut App, w: u16, h: u16) -> (Terminal<TestBackend>, String) {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| crate::render::draw(f, app)).unwrap();
    // Settle scroll animations like the main loop would, then draw the final frame.
    for v in app.views.values_mut() {
        for _ in 0..100 {
            if !v.chat.step_anim() {
                break;
            }
        }
    }
    term.draw(|f| crate::render::draw(f, app)).unwrap();
    let s = dump(&term);
    (term, s)
}

#[test]
fn chat_view_snapshot() {
    let mut app = test_app(false);
    let (_, s) = render(&mut app, 100, 30);
    println!("{s}");
    // Times on every message / tool.
    assert!(s.contains("14:02:11"), "user time");
    assert!(
        s.contains("14:02:13 → 14:02:20 (7.0s)"),
        "assistant start/end"
    );
    assert!(
        s.contains("1.4s") && s.contains("14:02:15"),
        "tool duration + time"
    );
    // Tool name + purpose.
    assert!(s.contains("bash  Run the unit tests"));
    assert!(s.contains("edit  Fix the off-by-one in parser"));
    assert!(
        s.contains("✗") && s.contains("tag mismatch"),
        "error first line"
    );
    assert!(s.contains("⊘"), "denied icon");
    // Reasoning collapsed: one line ending with the reasoning tail.
    let rline = s
        .lines()
        .find(|l| l.contains("Thought"))
        .expect("reasoning line");
    assert!(rline.contains("· 3.0s"));
    assert!(
        rline.trim_end().ends_with("confirm the fix works"),
        "{rline}"
    );
    assert!(rline.contains('…'));
    // Copy buttons (user + assistant).
    assert_eq!(s.matches("⧉ copy").count(), 2);
    // Footer and prompt.
    assert!(s.contains("Build · claude-opus-5.5"));
    assert!(s.contains("42.1k/1.0M (4%)"));
    assert!(s.contains("ctrl+p commands"));
}

#[test]
fn streaming_snapshot() {
    let mut app = test_app(true);
    let (_, s) = render(&mut app, 100, 30);
    println!("{s}");
    assert!(
        s.contains("running 3s") || s.contains("running 2s") || s.contains("running 4s"),
        "running tool counter"
    );
    let rline = s
        .lines()
        .find(|l| l.contains("Thinking"))
        .expect("live reasoning line");
    assert!(rline.trim_end().ends_with("for this particular"), "{rline}");
    assert!(s.contains("esc interrupt"));
}

#[test]
fn reasoning_tail_fits_line_cjk() {
    let mut app = test_app(false);
    if let Some(v) = app.views.get_mut(TEST_SESSION)
        && let Some(m) = v.message_at_mut(1)
        && let Part::Reasoning { text, .. } = &mut m.parts[0]
    {
        *text = "思考过程：首先读取解析器，然后检查循环边界是否包含末尾元素，最后运行测试确认修复有效果".into();
        v.versions[1] += 1000;
    }
    for w in [60u16, 81, 100] {
        let (term, s) = render(&mut app, w, 30);
        let line = s.lines().find(|l| l.contains("Thought")).expect("line");
        assert!(text::width(line) <= w as usize);
        assert!(line.trim_end().ends_with("修复有效果"), "{line}");
        let _ = term;
    }
}

#[test]
fn expanded_tool_shows_raw_io() {
    let mut app = test_app(false);
    if let Some(v) = app.views.get_mut(TEST_SESSION) {
        v.chat.toggle("m2", 2);
    }
    let (_, s) = render(&mut app, 100, 40);
    println!("{s}");
    assert!(s.contains("╭─ input"));
    assert!(s.contains("\"command\": \"cargo test -p parser\""));
    assert!(s.contains("├─ output"));
    assert!(s.contains("test result: ok"));
}

#[test]
fn click_copy_and_toggle_hits() {
    let mut app = test_app(false);
    let (term, s) = render(&mut app, 100, 30);
    let buf = term.backend().buffer().clone();
    // Find the assistant footer copy button.
    let lines: Vec<&str> = s.lines().collect();
    let y = lines
        .iter()
        .position(|l| l.contains("Build · claude") && l.contains("⧉ copy"))
        .expect("footer") as u16;
    let mut x = 0u16;
    while x < 100 && buf[(x, y)].symbol() != "⧉" {
        x += 1;
    }
    let v = app.views.get_mut(TEST_SESSION).unwrap();
    assert_eq!(
        v.chat.hit_at(x, y),
        Some(crate::ui::Hit::Copy { msg: "m2".into() })
    );
    let uy = lines
        .iter()
        .position(|l| l.contains("14:02:11") && l.contains("⧉ copy"))
        .expect("user meta") as u16;
    let mut ux = 0u16;
    while ux < 100 && buf[(ux, uy)].symbol() != "⧉" {
        ux += 1;
    }
    assert_eq!(
        v.chat.hit_at(ux + 3, uy),
        Some(crate::ui::Hit::Copy { msg: "m1".into() })
    );
    assert_eq!(
        v.chat.hit_at(ux.saturating_sub(1), uy),
        None,
        "gap before copy"
    );
    assert_eq!(
        v.chat.hit_at(ux.saturating_sub(4), uy),
        Some(crate::ui::Hit::Undo { msg: "m1".into() })
    );
    // Tool line toggles.
    let ty = lines
        .iter()
        .position(|l| l.contains("bash  Run the unit tests"))
        .unwrap() as u16;
    assert_eq!(
        v.chat.hit_at(10, ty),
        Some(crate::ui::Hit::Toggle {
            msg: "m2".into(),
            part: 2
        })
    );
}

#[test]
fn selection_copy_text() {
    let mut app = test_app(false);
    let (_, s) = render(&mut app, 100, 30);
    let lines: Vec<&str> = s.lines().collect();
    let y = lines
        .iter()
        .position(|l| l.contains("Please fix the off-by-one"))
        .unwrap() as u16;
    let v = app.views.get_mut(TEST_SESSION).unwrap();
    v.chat.mouse_down(0, y, 3);
    let t = v.chat.selected_text().unwrap();
    assert_eq!(
        t,
        "Please fix the off-by-one in the parser and run the tests. 顺便检查中文。"
    );
}

#[test]
fn split_view_snapshot() {
    let mut app = test_app(false);
    app.mode = Mode::Watch;
    let mut live = Vec::new();
    for (i, n) in ["worker-1", "reviewer", "docs"].iter().enumerate() {
        let mut inf = info();
        inf.id = format!("ses_bg{i}");
        inf.name = n.to_string();
        inf.attached = 0;
        inf.status = if i == 0 {
            SessionStatus::Busy
        } else {
            SessionStatus::Idle
        };
        let mut v = SessionView::new(inf.clone(), false);
        let msgs = transcript(i == 0);
        let total = msgs.len() as u64;
        v.load_snapshot(SessionSnapshot {
            info: inf.clone(),
            messages: msgs,
            more_before: false,
            total_messages: total,
            start_seq: 0,
            layout_metas: vec![],
            pending_permissions: vec![],
            pending_questions: vec![],
            queued: vec![],
        });
        app.views.insert(inf.id.clone(), v);
        live.push(inf);
    }
    app.live = live;
    app.watch.focus = Some("ses_bg1".into());
    let (_, s) = render(&mut app, 120, 30);
    println!("{s}");
    assert!(s.contains("worker-1") && s.contains("reviewer") && s.contains("docs"));
    assert!(s.contains("background sessions"));
    assert!(s.contains("Run the unit tests"));
}

#[test]
fn empty_and_weird_data_no_panic() {
    let mut app = test_app(false);
    let v = app.views.get_mut(TEST_SESSION).unwrap();
    v.apply(SessionEvent::Message {
        message: Message {
            id: "e".into(),
            role: Role::Assistant,
            time: 0,
            time_end: Some(0),
            source: MessageSource::Human,
            parts: vec![],
            model: None,
            agent: None,
            usage: None,
            error: Some("boom\x1b[31m\nsecond".into()),
        },
    });
    v.apply(SessionEvent::Message {
        message: Message {
            id: "e2".into(),
            role: Role::User,
            time: -5,
            time_end: None,
            source: MessageSource::Compaction,
            parts: vec![Part::Text {
                text: String::new(),
                time: 0,
            }],
            model: None,
            agent: None,
            usage: None,
            error: None,
        },
    });
    for (w, h) in [(100u16, 30u16), (20, 8), (5, 3), (1, 1), (300, 80)] {
        render(&mut app, w, h);
    }
}

/// Drives the app against the in-process mock backend for `ms` milliseconds.
async fn pump(app: &mut App, rx: &mut mpsc::UnboundedReceiver<crate::net::NetEvent>, ms: u64) {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(ms);
    loop {
        tokio::select! {
            ev = rx.recv() => match ev {
                Some(e) => app.on_net(e),
                None => return,
            },
            _ = tokio::time::sleep_until(deadline) => return,
        }
    }
}

pub fn key(
    code: crossterm::event::KeyCode,
    mods: crossterm::event::KeyModifiers,
) -> crossterm::event::Event {
    crossterm::event::Event::Key(crossterm::event::KeyEvent::new(code, mods))
}

/// Presses `code` without modifiers.
pub fn press(app: &mut App, code: KeyCode) {
    press_with(app, code, KeyModifiers::NONE);
}

pub fn press_with(app: &mut App, code: KeyCode, mods: KeyModifiers) {
    crate::input::handle(app, key(code, mods));
}

/// Types `s` one character at a time.
pub fn type_text(app: &mut App, s: &str) {
    for c in s.chars() {
        press(app, KeyCode::Char(c));
    }
}

/// An app wired to the in-process mock backend, with its foreground session.
struct MockApp {
    app: App,
    rx: mpsc::UnboundedReceiver<crate::net::NetEvent>,
    sid: String,
}

impl MockApp {
    /// Starts the app with the demo prompt and waits for its first reply.
    async fn start() -> Self {
        let (out_tx, out_rx) = mpsc::unbounded_channel();
        let (ev_tx, mut rx) = mpsc::unbounded_channel();
        tokio::spawn(crate::mock::run(TEST_CWD.into(), out_rx, ev_tx));
        let plan = InitPlan {
            prompt: Some("hello, run the demo".into()),
            ..Default::default()
        };
        let mut app = App::new(Client::new(out_tx), TEST_CWD.into(), plan);
        pump(&mut app, &mut rx, PUMP_TURN_MS).await;
        let sid = app.current.clone().expect("session created");
        Self { app, rx, sid }
    }

    async fn pump(&mut self, ms: u64) {
        pump(&mut self.app, &mut self.rx, ms).await;
    }

    /// Runs the demo until the permission request appears (drawn once, as keys need the layout).
    async fn wait_for_permission(&mut self) {
        for _ in 0..40 {
            if self.app.active_permission().is_some() {
                break;
            }
            self.pump(PUMP_POLL_MS).await;
        }
        assert!(
            self.app.active_permission().is_some(),
            "permission requested"
        );
        render(&mut self.app, 100, 30);
    }

    /// Keys right after the modal opens are ignored; wait then answer it.
    async fn answer_permission(&mut self, key: char) {
        tokio::time::sleep(PAST_GRACE).await;
        press(&mut self.app, KeyCode::Char(key));
        assert!(self.app.active_permission().is_none());
    }

    /// Runs the demo until its permission request is allowed.
    async fn start_allowed() -> Self {
        let mut m = Self::start().await;
        m.wait_for_permission().await;
        m.answer_permission('a').await;
        m
    }

    /// Waits for the reply to the queued prompt and the end of the turn.
    async fn wait_for_turn_end(&mut self) {
        for _ in 0..120 {
            let v = &self.app.views[&self.sid];
            if v.materialized_iter()
                .any(|m| m.raw_text().contains("Got it"))
                && !v.is_busy()
            {
                break;
            }
            self.pump(PUMP_POLL_MS).await;
        }
    }

    /// Queues one prompt, lets the turn finish and leaves the chat idle.
    async fn start_finished() -> Self {
        let mut m = Self::start_allowed().await;
        type_text(&mut m.app, "queued question");
        press(&mut m.app, KeyCode::Enter);
        m.wait_for_turn_end().await;
        m
    }

    fn queued_texts(&self) -> Vec<&str> {
        self.app.views[&self.sid]
            .queued
            .iter()
            .map(|q| q.text.as_str())
            .collect()
    }

    /// Opens the Ctrl+X leader menu and presses `key`.
    fn leader(&mut self, key: char) {
        press_with(&mut self.app, KeyCode::Char('x'), KeyModifiers::CONTROL);
        press(&mut self.app, KeyCode::Char(key));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mock_first_turn_streams_and_asks_permission() {
    let mut m = MockApp::start().await;
    {
        let v = &m.app.views[&m.sid];
        assert!(v.loaded, "snapshot loaded");
        assert!(v.slots.len() >= 2, "user + assistant");
    }
    let (_, s) = render(&mut m.app, 100, 30);
    println!("--- streaming ---\n{s}");

    m.wait_for_permission().await;
    let (_, s) = render(&mut m.app, 100, 30);
    println!("--- permission ---\n{s}");
    assert!(s.contains("Permission required"));
    assert!(s.contains("Allow once"));

    m.answer_permission('a').await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mock_queued_prompt_survives_reattach_and_can_be_removed() {
    let mut m = MockApp::start_allowed().await;
    let sid = m.sid.clone();

    // Queue a prompt while busy.
    type_text(&mut m.app, "queued question");
    press(&mut m.app, KeyCode::Enter);
    assert_eq!(m.app.views[&sid].queued.len(), 1);

    // Queue another one and remove it with its ✕ button while the turn is still running.
    type_text(&mut m.app, "never send this");
    press(&mut m.app, KeyCode::Enter);
    m.pump(PUMP_ROUNDTRIP_MS).await;
    assert!(
        m.app.views[&sid].queued.iter().all(|q| q.id.is_some()),
        "queued rows bound to server ids"
    );
    assert!(m.app.views[&sid].is_busy());

    // Switch to another session and back: the rows come back from the snapshot (still removable).
    let other = m
        .app
        .live
        .iter()
        .find(|s| s.id != sid)
        .map(|s| s.id.clone())
        .expect("another live session");
    m.app.attach_fg(&other);
    m.pump(PUMP_ROUNDTRIP_MS).await;
    m.app.attach_fg(&sid);
    m.pump(PUMP_ROUNDTRIP_MS).await;
    assert_eq!(
        m.queued_texts(),
        ["queued question", "never send this"],
        "queued rows survive re-attach"
    );
    assert!(m.app.views[&sid].queued.iter().all(|q| q.id.is_some()));

    let (term, s) = render(&mut m.app, 100, 30);
    let qy = s
        .lines()
        .position(|l| l.contains("never send this") && l.contains("✕ remove"))
        .expect("queued row with remove button") as u16;
    let qx = find_col(&term, qy, "✕").unwrap();
    m.app.screen = ratatui::layout::Rect::new(0, 0, 100, 30);
    {
        use crossterm::event::{MouseButton, MouseEventKind};
        crate::input::handle(
            &mut m.app,
            mouse(MouseEventKind::Down(MouseButton::Left), qx, qy),
        );
        crate::input::handle(
            &mut m.app,
            mouse(MouseEventKind::Up(MouseButton::Left), qx, qy),
        );
    }
    m.pump(PUMP_ROUNDTRIP_MS).await;
    assert_eq!(
        m.queued_texts(),
        ["queued question"],
        "removed from the queue"
    );
    assert_eq!(
        m.app.editor.text, "never send this",
        "removed prompt back in the input box"
    );
    m.app.editor.clear();

    // Let the turn finish.
    m.wait_for_turn_end().await;
    let v = &m.app.views[&sid];
    assert!(!v.is_busy(), "turn finished");
    assert!(v.queued.is_empty(), "queued prompt delivered");
    assert!(
        !v.materialized_iter()
            .any(|m| m.raw_text().contains("never send this")),
        "removed prompt never sent"
    );
    let (_, s) = render(&mut m.app, 100, 50);
    println!("--- done ---\n{s}");
    assert!(s.contains("Got it"), "reply to queued prompt");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mock_slash_command_via_autocomplete_toggles_details() {
    let mut m = MockApp::start_finished().await;

    // /details toggles tool expansion.
    type_text(&mut m.app, "/detai");
    assert!(m.app.ac.is_some(), "slash autocomplete");
    press(&mut m.app, KeyCode::Enter);
    assert!(m.app.details);
    assert!(m.app.editor.text.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mock_session_picker_lists_live_and_history() {
    let mut m = MockApp::start_finished().await;

    m.leader('l');
    m.pump(PUMP_REPLY_MS).await;
    let (_, s) = render(&mut m.app, 120, 34);
    println!("--- sessions ---\n{s}");
    assert!(s.contains("Live") && s.contains("History"));
    assert!(s.contains("worker-1") && s.contains(" bg ") && s.contains(" fg "));
    press(&mut m.app, KeyCode::Esc);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mock_watch_mode_tiles_focus_and_back_to_chat() {
    let mut m = MockApp::start_finished().await;
    let sid = m.sid.clone();

    // Tiles for the background sessions.
    m.leader('w');
    m.pump(PUMP_TILES_MS).await;
    let (_, s) = render(&mut m.app, 140, 36);
    println!("--- watch ---\n{s}");
    assert_eq!(m.app.mode, Mode::Watch);
    assert!(s.contains("worker-1") && s.contains("reviewer") && s.contains("docs-写手"));

    // The session that was foreground in this frontend stays foreground (hidden from tiles).
    let attached = |app: &App| app.live.iter().find(|i| i.id == sid).map(|i| i.attached);
    assert_eq!(attached(&m.app), Some(1));
    assert!(m.app.views[&sid].foreground);
    assert!(!m.app.tile_ids().contains(&sid));
    assert_eq!(m.app.current.as_deref(), Some(sid.as_str()));

    // A background tile asks for permission: the modal names the session; deny it.
    if m.app.active_permission().is_none() {
        let docs_id = m
            .app
            .live
            .iter()
            .find(|i| i.name == "docs-写手")
            .expect("docs-写手 session")
            .id
            .clone();
        m.app
            .on_net(crate::net::NetEvent::Frame(ServerFrame::Event {
                session_id: docs_id.clone(),
                event: SessionEvent::Permission {
                    request: PermissionRequest {
                        session_id: docs_id,
                        message_id: "m_perm_watch".into(),
                        call_id: "c_perm_watch".into(),
                        tool: "write".into(),
                        purpose: "Create the design document".into(),
                        input: json!({"path": "docs/design.md"}),
                        time: 0,
                        preview: None,
                    },
                },
            }));
    }
    let (_, s_perm) = render(&mut m.app, 140, 36);
    assert!(
        s_perm.contains("Permission required · docs-写手"),
        "{s_perm}"
    );
    m.answer_permission('d').await;
    assert!(m.app.watch.focus.is_none(), "no auto-focus");
    type_text(&mut m.app, "x");
    assert!(m.app.watch.inputs.values().all(|e| e.is_empty()));
    assert!(
        m.app
            .hint
            .as_ref()
            .is_some_and(|h| h.0.contains("click or tab to focus"))
    );

    // Tab focuses a tile; message it directly.
    press(&mut m.app, KeyCode::Tab);
    let focus = m.app.watch.focus.clone().expect("focus");
    type_text(&mut m.app, "ping");
    press(&mut m.app, KeyCode::Enter);
    m.pump(PUMP_ROUNDTRIP_MS).await;
    assert!(m.app.watch.inputs.get(&focus).is_none_or(|e| e.is_empty()));

    // Back to chat.
    m.leader('w');
    assert_eq!(m.app.mode, Mode::Chat);
    m.pump(PUMP_REPLY_MS).await;
    assert_eq!(attached(&m.app), Some(1), "stays foreground");
    assert!(m.app.views[&sid].foreground);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn permission_modal_keys_then_quit() {
    use crossterm::event::{KeyCode, KeyModifiers};
    let (out_tx, out_rx) = mpsc::unbounded_channel();
    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel();
    tokio::spawn(crate::mock::run(TEST_CWD.into(), out_rx, ev_tx));
    let mut app = App::new(
        Client::new(out_tx),
        TEST_CWD.into(),
        InitPlan {
            watch: true,
            ..Default::default()
        },
    );
    for _ in 0..60 {
        pump(&mut app, &mut ev_rx, PUMP_POLL_MS).await;
        render(&mut app, 100, 30);
        if app.active_permission().is_some() {
            break;
        }
    }
    assert!(app.active_permission().is_some());
    tokio::time::sleep(PAST_GRACE).await;
    for k in [
        key(KeyCode::Char('x'), KeyModifiers::CONTROL),
        key(KeyCode::Char('w'), KeyModifiers::NONE),
    ] {
        crate::input::handle(&mut app, k);
    }
    println!(
        "after ctrl+x w: perm={} mode={:?}",
        app.active_permission().is_some(),
        app.mode
    );
    press_with(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
    println!(
        "after ctrl+c: perm={} quit={}",
        app.active_permission().is_some(),
        app.quit
    );
    press_with(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
    println!(
        "after ctrl+c 2: perm={} quit={}",
        app.active_permission().is_some(),
        app.quit
    );
    assert!(app.quit);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn file_mention_autocomplete() {
    let (out_tx, out_rx) = mpsc::unbounded_channel();
    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel();
    tokio::spawn(crate::mock::run(TEST_CWD.into(), out_rx, ev_tx));
    let mut app = App::new(Client::new(out_tx), TEST_CWD.into(), InitPlan::default());
    pump(&mut app, &mut ev_rx, PUMP_CONNECT_MS).await;
    type_text(&mut app, "look at @src/ma");
    assert!(app.files_due.is_some(), "debounced query pending");
    for _ in 0..10 {
        tokio::time::sleep(std::time::Duration::from_millis(PUMP_TICK_MS)).await;
        app.housekeeping();
        pump(&mut app, &mut ev_rx, PUMP_TICK_MS).await;
    }
    let ac = app.ac.as_ref().expect("file popup");
    assert_eq!(ac.kind, crate::app::AcKind::File);
    assert!(
        ac.items.iter().any(|i| i.1 == "src/main.rs"),
        "{:?}",
        ac.items
    );
    let (_, s) = render(&mut app, 100, 30);
    println!("{s}");
    assert!(s.contains("files") && s.contains("src/main.rs"));
    press(&mut app, KeyCode::Tab);
    assert!(
        app.editor.text.starts_with("look at @src/"),
        "{}",
        app.editor.text
    );
    assert!(app.editor.text.ends_with(' '));
    assert!(app.ac.is_none());
    // Slash list with empty query: canonical names in declaration order.
    app.editor.clear();
    type_text(&mut app, "/");
    let ac = app.ac.as_ref().expect("slash popup");
    assert_eq!(ac.items[0].1, "/new");
    assert_eq!(ac.items[1].1, "/sessions");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dialogs_open_and_render() {
    use crossterm::event::{KeyCode, KeyModifiers};
    let (out_tx, out_rx) = mpsc::unbounded_channel();
    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel();
    tokio::spawn(crate::mock::run(TEST_CWD.into(), out_rx, ev_tx));
    let mut app = App::new(Client::new(out_tx), TEST_CWD.into(), InitPlan::default());
    pump(&mut app, &mut ev_rx, PUMP_CONNECT_MS).await;
    assert!(app.config.is_some());
    app.create_session(None);
    pump(&mut app, &mut ev_rx, PUMP_ROUNDTRIP_MS).await;
    press_with(&mut app, KeyCode::Char('x'), KeyModifiers::CONTROL);
    press(&mut app, KeyCode::Char('m'));
    assert!(app.dialog.is_some(), "models dialog");
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let (_, s) = render(&mut app, 120, 30);
    println!("{s}");
    assert!(s.contains("Switch model") && s.contains("GPT-6") && s.contains("DGN Gateway"));
    type_text(&mut app, "gpt");
    press(&mut app, KeyCode::Enter);
    pump(&mut app, &mut ev_rx, PUMP_ROUNDTRIP_MS).await;
    assert_eq!(app.cur().unwrap().info.model.model, "gpt-6");
    // Help dialog.
    press_with(&mut app, KeyCode::Char('x'), KeyModifiers::CONTROL);
    press(&mut app, KeyCode::Char('h'));
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let (_, s) = render(&mut app, 120, 30);
    println!("{s}");
    assert!(s.contains("Help") && s.contains("command palette"));
}

#[test]
fn drag_select_does_not_copy_until_ctrl_c() {
    use crossterm::event::{Event, KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    let mut app = test_app(false);
    let (_, s) = render(&mut app, 100, 30);
    let lines: Vec<&str> = s.lines().collect();
    let y = lines
        .iter()
        .position(|l| l.contains("I'll look at the parser."))
        .unwrap() as u16;
    let me = |kind, x, y| {
        Event::Mouse(MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        })
    };
    app.screen = ratatui::layout::Rect::new(0, 0, 100, 30);
    crate::input::handle(&mut app, me(MouseEventKind::Down(MouseButton::Left), 3, y));
    crate::input::handle(&mut app, me(MouseEventKind::Drag(MouseButton::Left), 10, y));
    crate::input::handle(&mut app, me(MouseEventKind::Up(MouseButton::Left), 10, y));
    assert!(app.has_selection());
    assert!(app.toasts.items.is_empty(), "no auto copy");
    let (_, _) = render(&mut app, 100, 30);
    crate::input::handle(
        &mut app,
        crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        )),
    );
    assert!(!app.has_selection());
    assert!(
        app.toasts
            .items
            .iter()
            .any(|t| t.text.contains("Copied selection (8 chars)")),
        "{:?}",
        app.toasts.items.iter().map(|t| &t.text).collect::<Vec<_>>()
    );
    // A plain click on a tool line toggles it.
    let ty = lines
        .iter()
        .position(|l| l.contains("bash  Run the unit tests"))
        .unwrap() as u16;
    crate::input::handle(
        &mut app,
        me(MouseEventKind::Down(MouseButton::Left), 20, ty),
    );
    crate::input::handle(&mut app, me(MouseEventKind::Up(MouseButton::Left), 20, ty));
    let (_, s) = render(&mut app, 100, 30);
    assert!(s.contains("╭─ input"));
}

#[test]
fn retry_status_and_reconnect() {
    let mut app = test_app(false);
    let v = app.views.get_mut(TEST_SESSION).unwrap();
    let mut i = v.info.clone();
    i.status = SessionStatus::Retrying {
        attempt: 3,
        next_at: ref_proto::now_ms() + 11_500,
        error: "rate limited (429)".into(),
    };
    v.apply(SessionEvent::Info { info: i });
    let (_, s) = render(&mut app, 100, 30);
    assert!(
        s.contains("Retrying in 12s (attempt 3)") && s.contains("rate limited (429)"),
        "{s}"
    );
    assert!(s.contains("retrying in 12s (#3)"));
    // Disconnect → banner; reconnect → re-attach current session as foreground.
    app.on_net(crate::net::NetEvent::Disconnected(
        "connection closed".into(),
    ));
    let (_, s) = render(&mut app, 100, 30);
    assert!(s.contains("Disconnected — reconnecting…"));
    app.on_net(crate::net::NetEvent::Connected);
    assert!(app.pending.values().any(
        |p| matches!(p, crate::app::Pending::Attach { sid, fg: true } if sid == TEST_SESSION)
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn net_hello_and_frames() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message as M;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (seen_tx, mut seen_rx) = mpsc::unbounded_channel::<String>();
    tokio::spawn(async move {
        let (sock, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(sock).await.unwrap();
        while let Some(Ok(M::Text(t))) = ws.next().await {
            let f: ClientFrame = serde_json::from_str(t.as_str()).unwrap();
            seen_tx.send(t.to_string()).unwrap();
            let id = f.id.unwrap_or(0);
            let reply = match f.req {
                Request::Hello { .. } => ServerFrame::Response {
                    id,
                    ok: true,
                    result: json!({ "protocol": PROTOCOL_VERSION }),
                    error: None,
                    code: None,
                },
                // Bigger than tungstenite's 16 MiB default frame limit (snapshots with many images).
                _ => ServerFrame::Response {
                    id,
                    ok: true,
                    result: json!({"sessions": [], "pad": "x".repeat(OVERSIZED_FRAME_BYTES)}),
                    error: None,
                    code: None,
                },
            };
            ws.send(M::text(serde_json::to_string(&reply).unwrap()))
                .await
                .unwrap();
            let toast = ServerFrame::Toast {
                level: ToastLevel::Info,
                text: "hi".into(),
            };
            ws.send(M::text(serde_json::to_string(&toast).unwrap()))
                .await
                .unwrap();
            ws.send(M::text("{\"kind\":\"future_frame\"}".to_string()))
                .await
                .unwrap();
        }
    });
    let (out_tx, out_rx) = mpsc::unbounded_channel();
    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel();
    let target = ref_client::Target::Explicit {
        url: format!("ws://127.0.0.1:{port}/"),
        token: Some("tok".into()),
    };
    tokio::spawn(crate::net::run(target, "/my/cwd".into(), out_rx, ev_tx));
    let first = tokio::time::timeout(std::time::Duration::from_secs(3), ev_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(first, crate::net::NetEvent::Connected),
        "{first:?}"
    );
    let hello = seen_rx.recv().await.unwrap();
    assert!(
        hello.contains("\"op\":\"hello\"")
            && hello.contains("/my/cwd")
            && hello.contains("\"token\":\"tok\""),
        "{hello}"
    );
    // Toast after hello arrives; unknown frame kinds are ignored.
    let t = ev_rx.recv().await.unwrap();
    assert!(matches!(
        t,
        crate::net::NetEvent::Frame(ServerFrame::Toast { .. })
    ));
    out_tx
        .send(ClientFrame {
            id: Some(7),
            req: Request::ListSessions,
        })
        .unwrap();
    let r = tokio::time::timeout(std::time::Duration::from_secs(10), ev_rx.recv())
        .await
        .unwrap()
        .unwrap();
    let ok = matches!(&r, crate::net::NetEvent::Frame(ServerFrame::Response { id: 7, ok: true, result, .. }) if result["pad"].as_str().map(str::len) == Some(OVERSIZED_FRAME_BYTES));
    assert!(
        ok,
        "{}",
        if let crate::net::NetEvent::Disconnected(e) = &r {
            e.clone()
        } else {
            "unexpected event".into()
        }
    );
}

#[test]
fn long_transcript_render_perf() {
    let mut app = test_app(false);
    {
        let v = app.views.get_mut(TEST_SESSION).unwrap();
        let base = transcript(false);
        let mut msgs = Vec::new();
        for i in 0..400 {
            for m in &base {
                let mut m = m.clone();
                m.id = format!("{}-{i}", m.id);
                msgs.push(m);
            }
        }
        let total = msgs.len() as u64;
        v.apply(SessionEvent::Reset {
            more_before: false,
            messages: msgs,
            total_messages: total,
            start_seq: 0,
            layout_metas: vec![],
        });
    }
    let total = app.views[TEST_SESSION].slots.len();
    let laid_out = |app: &App| app.views[TEST_SESSION].chat.laid_out;
    let t0 = std::time::Instant::now();
    render(&mut app, 120, 40);
    let first = t0.elapsed();
    assert_eq!(
        laid_out(&app),
        total,
        "the first frame lays out every message once"
    );
    // Streaming into the last message: only that message is laid out again.
    let t1 = std::time::Instant::now();
    for k in 0..30 {
        let before = laid_out(&app);
        let v = app.views.get_mut(TEST_SESSION).unwrap();
        let id = v.last_message().unwrap().id.clone();
        v.apply(SessionEvent::Delta {
            message_id: id,
            index: 1,
            delta: Delta::Text {
                text: format!(" more text {k}"),
            },
        });
        render(&mut app, 120, 40);
        assert_eq!(
            laid_out(&app) - before,
            1,
            "frame {k} re-laid-out more than the last message"
        );
    }
    // Timing is informational only: wall-clock ratios are unreliable on shared runners.
    println!(
        "first full layout {first:?}, per streaming frame {:?}",
        t1.elapsed() / 30
    );
}

#[test]
fn shell_mode_and_image_token() {
    let mut app = test_app(false);
    type_text(&mut app, "!");
    assert!(app.shell_mode);
    type_text(&mut app, "ls -la");
    let (_, s) = render(&mut app, 100, 30);
    assert!(s.contains("┃  Shell"), "{s}");
    press(&mut app, KeyCode::Esc);
    assert!(!app.shell_mode);
    app.editor.clear();
    app.editor.attach_image(
        ImageData {
            mime: "image/png".into(),
            data: "AAAA".into(),
            ..Default::default()
        },
        Some("clipboard".into()),
    );
    type_text(&mut app, "what is this?");
    let (_, s) = render(&mut app, 100, 30);
    assert!(s.contains("[Image 1] what is this?"));
    assert!(s.contains("1 image"));
}

pub fn mouse(kind: crossterm::event::MouseEventKind, x: u16, y: u16) -> crossterm::event::Event {
    crossterm::event::Event::Mouse(crossterm::event::MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: crossterm::event::KeyModifiers::NONE,
    })
}

/// Column of the first cell on row `y` whose symbol is `sym`.
fn find_col(term: &Terminal<TestBackend>, y: u16, sym: &str) -> Option<u16> {
    let buf = term.backend().buffer();
    (0..buf.area.width).find(|&x| buf[(x, y)].symbol() == sym)
}

#[test]
fn drag_selection_across_wrapped_cjk_lines() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = test_app(false);
    {
        let v = app.views.get_mut(TEST_SESSION).unwrap();
        if let Some(m) = v.message_at_mut(0)
            && let Part::Text { text, .. } = &mut m.parts[0]
        {
            *text =
                "alpha beta gamma delta 中文字符测试中文字符测试中文字符测试 epsilon zeta".into();
        }
        v.versions[0] += 1000;
    }
    let (w, h) = (40u16, 30u16);
    let (_, s) = render(&mut app, w, h);
    app.screen = ratatui::layout::Rect::new(0, 0, w, h);
    let lines: Vec<&str> = s.lines().collect();
    let y0 = lines
        .iter()
        .position(|l| l.contains("alpha beta"))
        .expect("first row") as u16;
    let y1 = lines
        .iter()
        .position(|l| l.contains("zeta"))
        .expect("last row") as u16;
    assert!(y1 >= y0 + 2, "text must wrap over several rows:\n{s}");
    for l in &lines {
        assert!(text::width(l) <= w as usize, "row overflows: {l:?}");
    }
    let row = lines[y0 as usize];
    let x0 = text::width(&row[..row.find("alpha").unwrap()]) as u16;
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), x0, y0),
    );
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Drag(MouseButton::Left), 20, y0 + 1),
    );
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Drag(MouseButton::Left), w - 2, y1),
    );
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Up(MouseButton::Left), w - 2, y1),
    );
    // Mouse up never copies.
    assert_eq!(crate::clipboard::take_last_copy(), None);
    assert!(app.has_selection());
    // `y` copies (empty prompt); soft-wrapped rows are joined without inserted breaks.
    press(&mut app, KeyCode::Char('y'));
    let got = crate::clipboard::take_last_copy().expect("copied");
    assert_eq!(
        got,
        "alpha beta gamma delta 中文字符测试中文字符测试中文字符测试 epsilon zeta"
    );
    assert!(!app.has_selection());
    assert!(
        app.editor.is_empty(),
        "y must not be typed into the prompt when copying"
    );
}

#[test]
fn selection_starting_mid_cjk_glyph() {
    use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};
    let mut app = test_app(false);
    let (term, s) = render(&mut app, 100, 30);
    app.screen = ratatui::layout::Rect::new(0, 0, 100, 30);
    let y = s.lines().position(|l| l.contains("顺便检查中文")).unwrap() as u16;
    let x = find_col(&term, y, "顺").unwrap();
    // Press on the right half of a wide glyph, drag to the right half of another.
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), x + 1, y),
    );
    let x2 = find_col(&term, y, "查").unwrap();
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Drag(MouseButton::Left), x2 + 1, y),
    );
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Up(MouseButton::Left), x2 + 1, y),
    );
    press_with(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(
        crate::clipboard::take_last_copy().as_deref(),
        Some("顺便检查")
    );
}

#[test]
fn copy_button_copies_raw_markdown() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = test_app(false);
    let (term, s) = render(&mut app, 100, 30);
    app.screen = ratatui::layout::Rect::new(0, 0, 100, 30);
    let y = s
        .lines()
        .position(|l| l.contains("Build · claude") && l.contains("⧉ copy"))
        .unwrap() as u16;
    let x = find_col(&term, y, "⧉").unwrap();
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), x + 2, y),
    );
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Up(MouseButton::Left), x + 2, y),
    );
    let raw = app.views[TEST_SESSION].message_at(1).unwrap().raw_text();
    assert!(raw.contains("```rust"), "raw markdown, not rendered text");
    assert_eq!(crate::clipboard::take_last_copy(), Some(raw));
    assert!(!app.has_selection());
}

#[test]
fn click_without_drag_does_not_select_or_copy() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = test_app(false);
    let (_, s) = render(&mut app, 100, 30);
    app.screen = ratatui::layout::Rect::new(0, 0, 100, 30);
    let y = s
        .lines()
        .position(|l| l.contains("I'll look at the parser."))
        .unwrap() as u16;
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), 5, y),
    );
    crate::input::handle(&mut app, mouse(MouseEventKind::Up(MouseButton::Left), 5, y));
    assert!(!app.has_selection());
    assert_eq!(crate::clipboard::take_last_copy(), None);
    // Double click selects a word; still nothing copied.
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), 5, y),
    );
    crate::input::handle(&mut app, mouse(MouseEventKind::Up(MouseButton::Left), 5, y));
    assert!(app.has_selection());
    assert_eq!(crate::clipboard::take_last_copy(), None);
}

fn fail_pending(app: &mut App, pred: impl Fn(&crate::app::Pending) -> bool, err: &str) {
    let id = *app
        .pending
        .iter()
        .find(|(_, p)| pred(p))
        .map(|(k, _)| k)
        .expect("pending request");
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id,
        ok: false,
        result: serde_json::Value::Null,
        error: Some(err.into()),
        code: None,
    }));
}

#[test]
fn reattach_failure_of_empty_session_deselects_without_creating() {
    use crate::app::Pending;
    let mut app = test_app(false);
    app.views
        .get_mut(TEST_SESSION)
        .unwrap()
        .apply(SessionEvent::Reset {
            more_before: false,
            messages: vec![],
            total_messages: 0,
            start_seq: 0,
            layout_metas: vec![],
        });
    app.on_net(crate::net::NetEvent::Disconnected(
        "connection closed".into(),
    ));
    app.on_net(crate::net::NetEvent::Connected);
    fail_pending(
        &mut app,
        |p| matches!(p, Pending::Attach { fg: true, .. }),
        "session not found",
    );
    assert!(app.current.is_none());
    assert!(
        !app.pending
            .values()
            .any(|p| matches!(p, Pending::Created { .. })),
        "no session auto-created"
    );
}

#[test]
fn failed_prompt_text_returns_to_editor() {
    use crate::app::Pending;
    let mut app = test_app(false);
    type_text(&mut app, "do the thing");
    press(&mut app, KeyCode::Enter);
    assert!(app.editor.is_empty());
    fail_pending(
        &mut app,
        |p| matches!(p, Pending::Prompt { .. }),
        "Session ses_test is not live",
    );
    assert_eq!(app.editor.text, "do the thing");
    assert!(
        app.toasts
            .items
            .iter()
            .any(|t| t.text.contains("Prompt not sent"))
    );
}

#[test]
fn keys_typed_as_permission_opens_go_to_editor() {
    let mut app = test_app(false);
    let req = PermissionRequest {
        session_id: TEST_SESSION.into(),
        message_id: "m2".into(),
        call_id: "c9".into(),
        tool: "bash".into(),
        purpose: "List files".into(),
        input: json!({"command": "ls -la"}),
        time: 0,
        preview: None,
    };
    app.views
        .get_mut(TEST_SESSION)
        .unwrap()
        .apply(SessionEvent::Permission { request: req });
    let (_, s) = render(&mut app, 100, 30);
    assert!(
        s.contains("Permission required") && s.contains("$ ls -la"),
        "{s}"
    );
    // Within the grace period: 'a' / 'd' must not answer the modal; they are typed text.
    type_text(&mut app, "ad");
    press(&mut app, KeyCode::Enter);
    assert!(
        app.active_permission().is_some(),
        "modal not answered by early keys"
    );
    assert_eq!(app.editor.text, "ad");
    // After the grace period the modal reacts.
    app.perm.as_mut().unwrap().opened -= PAST_GRACE;
    press(&mut app, KeyCode::Char('a'));
    assert!(app.active_permission().is_none());
    assert_eq!(app.editor.text, "ad");
    // The modal is sized to its content, not the full screen height.
    let mut app = test_app(false);
    let req = PermissionRequest {
        session_id: TEST_SESSION.into(),
        message_id: "m2".into(),
        call_id: "c10".into(),
        tool: "bash".into(),
        purpose: "x".into(),
        input: json!({"command": "ls"}),
        time: 0,
        preview: None,
    };
    app.views
        .get_mut(TEST_SESSION)
        .unwrap()
        .apply(SessionEvent::Permission { request: req });
    render(&mut app, 100, 40);
    assert!(
        app.perm.as_ref().unwrap().rect.height <= 14,
        "{:?}",
        app.perm.as_ref().unwrap().rect
    );
}

#[test]
fn model_filter_shows_provider() {
    let mut app = test_app(false);
    if let Some(c) = app.config.as_mut() {
        let mut m = c.models[0].clone();
        m.provider = "other".into();
        m.provider_name = "OtherCo".into();
        c.models.push(m);
    }
    app.open_models();
    type_text(&mut app, "opus");
    let (_, s) = render(&mut app, 100, 30);
    assert!(s.contains("claude-opus-5.5  OtherCo"), "{s}");
    assert!(s.contains("claude-opus-5.5  dgn"), "{s}");
}

// ---------------------------------------------------------------------------
// Tool box alignment, undo button, closing sessions, animation, title, exit, todos
// ---------------------------------------------------------------------------

fn ctx<'a>(details: bool) -> crate::ui::message::Ctx<'a> {
    crate::ui::message::Ctx {
        agents: &[],
        models: &[],
        toggled: None,
        show_all: None,
        details,
        thinking: false,
        now: ref_proto::now_ms(),
        tick: 0,
        compact: false,
        session_agent: "build",
        stale: false,
        context_start: false,
    }
}

/// Draws rows into a buffer (like the chat view does) and returns, per row, (symbols, right border column).
fn box_rows(rows: &[crate::ui::Row], w: u16) -> Vec<(String, Option<u16>)> {
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    let mut buf = Buffer::empty(Rect::new(0, 0, w, rows.len() as u16));
    for (y, r) in rows.iter().enumerate() {
        buf.set_line(0, y as u16, &r.line, w);
    }
    (0..rows.len() as u16)
        .map(|y| {
            let syms: String = (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect();
            let col = (0..w)
                .rev()
                .find(|&x| matches!(buf[(x, y)].symbol(), "│" | "╮" | "┤" | "╯"));
            (syms, col)
        })
        .collect()
}

#[test]
fn expanded_tool_box_right_borders_align() {
    let cjk_out = "测试结果：全部通过。中文输出行很长很长很长很长很长很长很长很长很长很长很长很长很长很长很长很长很长很长很长\nplain ascii line\n😀 emoji 宽字符 mixed 中a文b字c符d";
    let mut part = tool(
        "c1",
        "bash",
        "运行测试 Run the unit tests",
        json!({"command": "cargo test -p 解析器 && echo 完成完成完成完成完成完成完成完成完成完成完成完成完成完成"}),
        ToolState::Done,
        Some(cjk_out),
        ts(14, 2, 15),
        Some(ts(14, 2, 16)),
    );
    if let Part::Tool { meta, .. } = &mut part {
        *meta = Some(
            json!({"exit_code": 0, "diff": "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-旧的一行中文内容旧的一行中文内容旧的一行中文内容\n+新的一行中文内容新的一行中文内容新的一行中文内容新的", "note": "备注 note"}),
        );
    }
    let msg = Message {
        id: "m".into(),
        role: Role::Assistant,
        time: 0,
        time_end: Some(1),
        source: MessageSource::Human,
        parts: vec![part],
        model: None,
        agent: None,
        usage: None,
        error: None,
    };
    for w in [40usize, 41, 80, 117] {
        let rows = crate::ui::message::render(&msg, &ctx(true), w);
        let top = rows
            .iter()
            .position(|r| text::line_text(&r.line).contains('╭'))
            .expect("box top");
        let bot = rows
            .iter()
            .position(|r| text::line_text(&r.line).contains('╯'))
            .expect("box bottom");
        let blk = &rows[top..=bot];
        let drawn = box_rows(blk, w as u16 + 4);
        let width0 = text::line_width(&blk[0].line);
        let col0 = drawn[0].1.expect("border");
        for (r, (syms, col)) in blk.iter().zip(&drawn) {
            assert_eq!(
                text::line_width(&r.line),
                width0,
                "w={w}: row width differs: {syms:?}"
            );
            assert_eq!(
                *col,
                Some(col0),
                "w={w}: right border column differs: {syms:?}"
            );
        }
        // Sections and the bottom edge use the matching corner glyphs.
        let texts: Vec<String> = blk.iter().map(|r| text::line_text(&r.line)).collect();
        assert!(
            texts[0].trim_end().ends_with('╮')
                && texts
                    .iter()
                    .any(|t| t.contains("├─ output") && t.trim_end().ends_with('┤'))
        );
        assert!(
            texts.iter().any(|t| t.contains("├─ diff"))
                && texts.iter().any(|t| t.contains("├─ meta"))
        );
        assert!(texts.last().unwrap().trim_end().ends_with('╯'));
        // The box (with its indent) fits the content width.
        assert!(width0 <= w, "w={w} width={width0}");
    }
}

#[test]
fn write_and_edit_show_collapsed_diff_preview_and_expanded_input_output() {
    let diff: String = (1..=12)
        .map(|i| format!("+line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut part = tool(
        "c_edit",
        "edit",
        "Update parser",
        json!({"path": "src/parser.rs", "edits": []}),
        ToolState::Done,
        Some("Edited src/parser.rs"),
        ts(14, 2, 15),
        Some(ts(14, 2, 16)),
    );
    if let Part::Tool { meta, .. } = &mut part {
        *meta = Some(json!({
            "path": "src/parser.rs",
            "diff": diff,
            "added": 12,
            "removed": 2,
            "warnings": ["Recovered: file changed since read"],
        }));
    }
    let msg = Message {
        id: "m_edit".into(),
        role: Role::Assistant,
        time: 0,
        time_end: Some(1),
        source: MessageSource::Human,
        parts: vec![part],
        model: None,
        agent: None,
        usage: None,
        error: None,
    };
    // Collapsed: +12 -2, first 8 diff lines, "… 4 more", and warnings underneath.
    let collapsed: Vec<String> = crate::ui::message::render(&msg, &ctx(false), 80)
        .iter()
        .map(|r| text::line_text(&r.line))
        .collect();
    let c_text = collapsed.join("\n");
    assert!(c_text.contains("+12 -2"), "{c_text}");
    assert!(c_text.contains("+line 8"), "{c_text}");
    assert!(!c_text.contains("+line 9"), "capped at 8 lines: {c_text}");
    assert!(c_text.contains("… 4 more"), "{c_text}");
    assert!(
        c_text.contains("Recovered: file changed since read"),
        "{c_text}"
    );
    assert!(!c_text.contains("╭─ input"), "{c_text}");
    assert!(!c_text.contains("Edited src/parser.rs"), "{c_text}");
    // Warnings appear under the diff preview.
    let more_pos = collapsed
        .iter()
        .position(|l| l.contains("… 4 more"))
        .unwrap();
    let warn_pos = collapsed
        .iter()
        .position(|l| l.contains("Recovered: file changed since read"))
        .unwrap();
    assert!(warn_pos > more_pos, "{c_text}");

    // Compact (split tile) collapsed: includes path alongside +/- counts.
    let mut compact_cx = ctx(false);
    compact_cx.compact = true;
    let compact_text: String = crate::ui::message::render(&msg, &compact_cx, 60)
        .iter()
        .map(|r| text::line_text(&r.line))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(compact_text.contains("src/parser.rs"), "{compact_text}");
    assert!(compact_text.contains("+12 -2"), "{compact_text}");
    assert!(compact_text.contains("… 4 more"), "{compact_text}");

    // Expanded: Input + full Diff (all 12 lines) + Warnings + Output.
    let expanded: String = crate::ui::message::render(&msg, &ctx(true), 80)
        .iter()
        .map(|r| text::line_text(&r.line))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(expanded.contains("╭─ input"), "{expanded}");
    assert!(
        expanded.contains("\"path\": \"src/parser.rs\""),
        "{expanded}"
    );
    assert!(expanded.contains("├─ diff"), "{expanded}");
    assert!(expanded.contains("+line 12"), "{expanded}");
    assert!(!expanded.contains("… 4 more"), "{expanded}");
    assert!(expanded.contains("├─ warnings"), "{expanded}");
    assert!(expanded.contains("├─ output"), "{expanded}");
    assert!(expanded.contains("Edited src/parser.rs"), "{expanded}");
}

#[test]
fn box_section_with_cjk_label_matches_body_width() {
    let msg = Message {
        id: "m".into(),
        role: Role::Assistant,
        time: 0,
        time_end: Some(1),
        source: MessageSource::Human,
        parts: vec![tool(
            "c",
            "x",
            "",
            json!({"标签": "值"}),
            ToolState::Done,
            Some("ok"),
            0,
            Some(1),
        )],
        model: None,
        agent: None,
        usage: None,
        error: None,
    };
    let rows = crate::ui::message::render(&msg, &ctx(true), 60);
    let ws: Vec<usize> = rows
        .iter()
        .filter(|r| text::line_text(&r.line).contains(['│', '╮', '┤', '╯']))
        .map(|r| text::line_width(&r.line))
        .collect();
    assert!(ws.len() >= 4 && ws.iter().all(|&w| w == ws[0]), "{ws:?}");
}

/// Left click (press and release) at `x`, `y`.
fn click_at(app: &mut App, x: u16, y: u16) {
    use crossterm::event::{MouseButton, MouseEventKind};
    crate::input::handle(app, mouse(MouseEventKind::Down(MouseButton::Left), x, y));
    crate::input::handle(app, mouse(MouseEventKind::Up(MouseButton::Left), x, y));
}

fn is_undo(p: &crate::app::Pending) -> bool {
    matches!(p, crate::app::Pending::Undo { .. })
}

/// Renders the chat of `app` and returns the position of the first undo button.
fn undo_button(app: &mut App) -> (u16, u16) {
    app.screen = ratatui::layout::Rect::new(0, 0, 100, 30);
    let (term, s) = render(app, 100, 30);
    let y = s.lines().position(|l| l.contains("↶ undo")).unwrap() as u16;
    (find_col(&term, y, "↶").unwrap(), y)
}

#[test]
fn undo_button_hit_zone() {
    use crate::ui::Hit;
    let mut app = test_app(false);
    let (term, s) = render(&mut app, 100, 30);
    let lines: Vec<&str> = s.lines().collect();
    let uy = lines
        .iter()
        .position(|l| l.contains("14:02:11") && l.contains("↶ undo") && l.contains("⧉ copy"))
        .expect("user meta row") as u16;
    let ux = find_col(&term, uy, "↶").unwrap();
    let cx = find_col(&term, uy, "⧉").unwrap();
    assert_eq!(cx, ux + 8, "undo then two spaces then copy");

    // Only the human prompt has an undo button.
    assert_eq!(s.matches("↶ undo").count(), 1);
    let v = app.views.get_mut(TEST_SESSION).unwrap();
    for dx in 0..6 {
        assert_eq!(
            v.chat.hit_at(ux + dx, uy),
            Some(Hit::Undo { msg: "m1".into() }),
            "dx={dx}"
        );
    }
    assert_eq!(v.chat.hit_at(ux + 6, uy), None, "gap between buttons");
    assert_eq!(v.chat.hit_at(ux.saturating_sub(1), uy), None);
    assert_eq!(v.chat.hit_at(cx, uy), Some(Hit::Copy { msg: "m1".into() }));
    assert_eq!(
        v.chat.hit_at(cx + 5, uy),
        Some(Hit::Copy { msg: "m1".into() })
    );
}

#[test]
fn undo_click_on_idle_session_sends_request_and_fills_editor() {
    use crate::app::Pending;
    let mut app = test_app(false);
    let (ux, uy) = undo_button(&mut app);

    // Click (idle): request sent right away, removing 2 messages.
    click_at(&mut app, ux + 2, uy);
    assert!(
        app.pending
            .values()
            .any(|p| matches!(p, Pending::Undo { count: Some(2), .. })),
        "undo request"
    );
    respond(&mut app, is_undo, json!({"text": "Please fix it"}));
    assert_eq!(app.editor.text, "Please fix it");
    assert!(
        app.toasts
            .items
            .iter()
            .any(|t| t.text == "Undid 2 messages")
    );
}

#[test]
fn undo_appends_prompt_to_a_non_empty_editor() {
    let mut app = test_app(false);
    let (ux, uy) = undo_button(&mut app);

    // Non-empty editor is kept; the prompt is appended after a blank line.
    type_text(&mut app, "draft");
    click_at(&mut app, ux + 2, uy);
    respond(&mut app, is_undo, json!({"text": "old prompt"}));
    assert_eq!(app.editor.text, "draft\n\nold prompt");
    assert_eq!(app.editor.cursor, app.editor.text.len());
}

#[test]
fn undo_on_busy_session_needs_a_second_click() {
    let mut app = test_app(true);
    let (ux, uy) = undo_button(&mut app);

    // Busy session: the first click only arms a confirmation.
    click_at(&mut app, ux, uy);
    assert!(!app.pending.values().any(is_undo));
    assert!(
        app.hint
            .as_ref()
            .is_some_and(|h| h.0.contains("aborts the running turn"))
    );
    click_at(&mut app, ux, uy);
    assert!(app.pending.values().any(is_undo));
}

#[test]
fn undo_puts_prompt_and_images_back_into_editor() {
    use crate::app::Pending;
    let mut app = test_app(false);
    // The transcript only has a stub; the bytes come with the undo response.
    let stub = ImageData {
        mime: "image/png".into(),
        data: String::new(),
        ..Default::default()
    };
    let mut m3 = msg(
        "m3",
        Role::User,
        MessageSource::Human,
        0,
        "look at [Image 1]",
    );
    m3.parts.push(Part::Image {
        image: stub,
        name: Some("shot.png".into()),
    });
    let v = app.views.get_mut(TEST_SESSION).unwrap();
    v.apply(SessionEvent::Message { message: m3 });
    v.apply(SessionEvent::Message {
        message: msg("m4", Role::Assistant, MessageSource::Human, 0, "ok"),
    });
    v.queued
        .push(crate::state::Queued::new("stale".into(), vec![]));
    let respond = |app: &mut App, result: serde_json::Value| {
        let id = *app
            .pending
            .iter()
            .find(|(_, p)| matches!(p, Pending::Undo { .. }))
            .map(|(k, _)| k)
            .expect("undo request");
        app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
            id,
            ok: true,
            result,
            error: None,
            code: None,
        }));
    };
    // /undo (= ctrl+x u): the last human prompt, with its image.
    type_text(&mut app, "/undo");
    press(&mut app, KeyCode::Enter);
    respond(
        &mut app,
        json!({"text": "look at [Image 1]", "images": [{"mime": "image/png", "data": "iVBO", "name": "shot.png"}]}),
    );
    assert_eq!(app.editor.text, "look at [Image 1]");
    let imgs: Vec<(&str, Option<&str>)> = app
        .editor
        .images
        .iter()
        .map(|a| (a.image.data.as_str(), a.name.as_deref()))
        .collect();
    assert_eq!(imgs, [("iVBO", Some("shot.png"))]);
    assert!(
        app.toasts
            .items
            .iter()
            .any(|t| t.text == "Undid last prompt")
    );
    assert!(
        app.views[TEST_SESSION].queued.is_empty(),
        "undo clears the server queue"
    );
    // Undo from an earlier message: that prompt comes back (appended), not the last one.
    app.undo_message(TEST_SESSION, "m1");
    assert!(
        app.pending
            .values()
            .any(|p| matches!(p, Pending::Undo { count: Some(4), .. }))
    );
    respond(
        &mut app,
        json!({"text": "Please fix the off-by-one", "images": []}),
    );
    assert_eq!(
        app.editor.text,
        "look at [Image 1]\n\nPlease fix the off-by-one"
    );
    assert_eq!(app.editor.images.len(), 1);
}

/// A test app with the layout area set, as clicks need it.
fn queue_test_app() -> App {
    let mut app = test_app(true);
    app.screen = ratatui::layout::Rect::new(0, 0, 100, 30);
    app
}

/// Clicks the ✕ remove button of the queued row showing `text`.
fn click_remove(app: &mut App, text: &str) {
    use crate::ui::Hit;
    let (term, s) = render(app, 100, 30);
    let y = s
        .lines()
        .position(|l| l.contains(text) && l.contains("✕ remove"))
        .expect("queued row") as u16;
    let x = find_col(&term, y, "✕").unwrap();
    let key = app.views[TEST_SESSION]
        .queued
        .iter()
        .find(|q| q.text == text)
        .unwrap()
        .key;
    assert_eq!(
        app.views[TEST_SESSION].chat.hit_at(x, y),
        Some(Hit::Unqueue { key })
    );
    click_at(app, x, y);
}

fn is_prompt(p: &crate::app::Pending) -> bool {
    matches!(p, crate::app::Pending::Prompt { .. })
}

fn is_unqueue(p: &crate::app::Pending) -> bool {
    matches!(p, crate::app::Pending::Unqueue { .. })
}

#[test]
fn queued_prompt_removed_before_prompt_response_waits_for_message_id() {
    let mut app = queue_test_app();
    let sid = TEST_SESSION.to_string();

    // Clicked before the Prompt response: the delete waits for the message id.
    type_text(&mut app, "first queued");
    press(&mut app, KeyCode::Enter);
    click_remove(&mut app, "first queued");
    assert!(app.views[&sid].queued[0].removing);
    assert!(!app.pending.values().any(is_unqueue));
    let (_, s) = render(&mut app, 100, 30);
    assert!(s.contains("removing…"));
    respond(
        &mut app,
        is_prompt,
        json!({"queued": true, "message_id": "q1"}),
    );
    assert!(
        app.pending.values().any(is_unqueue),
        "RemoveQueued sent once the id is known"
    );

    // Typed meanwhile: the removed prompt is appended after a blank line.
    type_text(&mut app, "draft");
    respond(&mut app, is_unqueue, json!({"removed": true}));
    assert!(app.views[&sid].queued.is_empty());
    assert_eq!(app.editor.text, "draft\n\nfirst queued");
    assert_eq!(app.editor.cursor, app.editor.text.len());
    let (_, s) = render(&mut app, 100, 30);
    assert!(!s.contains("queued first queued") && !s.contains("✕ remove"));
}

#[test]
fn removed_queued_prompt_brings_its_images_back() {
    let mut app = queue_test_app();
    let sid = TEST_SESSION.to_string();

    // Images come back with it (empty editor: filled).
    let png = ImageData {
        mime: "image/png".into(),
        data: "iVBO".into(),
        ..Default::default()
    };
    app.send_prompt(&sid, "see [Image 1]".into(), vec![png]);
    respond(
        &mut app,
        is_prompt,
        json!({"queued": true, "message_id": "q0"}),
    );
    click_remove(&mut app, "see [Image 1]");
    respond(&mut app, is_unqueue, json!({"removed": true}));
    assert_eq!(app.editor.text, "see [Image 1]");
    assert_eq!(
        app.editor
            .images
            .iter()
            .map(|a| a.image.data.as_str())
            .collect::<Vec<_>>(),
        ["iVBO"]
    );
}

#[test]
fn queued_prompt_already_sent_when_removed_stays_until_its_message_shows() {
    let mut app = queue_test_app();
    let sid = TEST_SESSION.to_string();

    // Already delivered when the delete arrives: toast, row stays until the message shows up.
    type_text(&mut app, "second queued");
    press(&mut app, KeyCode::Enter);
    respond(
        &mut app,
        is_prompt,
        json!({"queued": true, "message_id": "q2"}),
    );
    click_remove(&mut app, "second queued");
    respond(&mut app, is_unqueue, json!({"removed": false}));
    assert!(
        app.toasts
            .items
            .iter()
            .any(|t| t.text.contains("Already sent"))
    );
    assert_eq!(app.views[&sid].queued.len(), 1);
    assert!(!app.views[&sid].queued[0].removing);

    // A user message with the same text but another id doesn't match; the delivered one does.
    let v = app.views.get_mut(&sid).unwrap();
    v.apply(SessionEvent::Message {
        message: msg(
            "other",
            Role::User,
            MessageSource::Human,
            0,
            "second queued",
        ),
    });
    assert_eq!(v.queued.len(), 1);
    v.apply(SessionEvent::Message {
        message: msg("q2", Role::User, MessageSource::Human, 0, "second queued"),
    });
    assert!(v.queued.is_empty());
}

#[test]
fn prompt_queued_by_server_while_thought_idle_gets_a_removable_row() {
    let mut app = queue_test_app();
    let sid = TEST_SESSION.to_string();

    // Sent while we thought it idle but the server queued it: a removable row appears.
    app.views.get_mut(&sid).unwrap().set_info(info());
    type_text(&mut app, "late");
    press(&mut app, KeyCode::Enter);
    assert!(app.views[&sid].queued.is_empty());
    respond(
        &mut app,
        is_prompt,
        json!({"queued": true, "message_id": "q3"}),
    );
    assert_eq!(
        app.views[&sid]
            .queued
            .iter()
            .map(|q| q.id.as_deref())
            .collect::<Vec<_>>(),
        [Some("q3")]
    );
}

fn bg_session(id: &str, name: &str, status: SessionStatus, attached: u32) -> SessionInfo {
    let mut i = info();
    i.id = id.into();
    i.name = name.into();
    i.status = status;
    i.attached = attached;
    i.created_at = 5;
    i
}

#[test]
fn working_time_comes_from_server_run_start_and_survives_switching() {
    let mut app = test_app(true);
    let mut i = app.views[TEST_SESSION].info.clone();
    i.busy_since = Some(ref_proto::now_ms() - 65_000);
    app.views.get_mut(TEST_SESSION).unwrap().set_info(i.clone());
    app.live = vec![
        i.clone(),
        bg_session("ses_other", "other", SessionStatus::Idle, 0),
    ];
    let (_, s) = render(&mut app, 100, 30);
    assert!(s.contains("running tools 1m 0"), "{s}");
    // Switch away and back: the view is rebuilt, the time is not reset.
    app.attach_fg("ses_other");
    assert!(!app.views.contains_key(TEST_SESSION));
    app.attach_fg(TEST_SESSION);
    assert!(
        app.views[TEST_SESSION]
            .busy_ms(ref_proto::now_ms())
            .is_some_and(|ms| ms >= 65_000)
    );
    let (_, s) = render(&mut app, 100, 30);
    assert!(s.contains("running tools 1m 0"), "{s}");
    // Older backend without busy_since: client-side fallback; idle: none.
    let mut i = info();
    i.status = SessionStatus::Busy;
    let v = SessionView::new(i, true);
    assert!(v.busy_ms(ref_proto::now_ms()).is_some_and(|ms| ms < 5_000));
    let v = SessionView::new(info(), true);
    assert_eq!(v.busy_ms(ref_proto::now_ms()), None);
}

#[test]
fn close_session_matching() {
    use crate::app::match_session;
    let live = vec![
        bg_session("ses_1", "Worker", SessionStatus::Idle, 0),
        bg_session("ses_2", "worker", SessionStatus::Idle, 0),
        bg_session("ses_3", "Docs", SessionStatus::Idle, 0),
    ];
    assert_eq!(
        match_session(&live, "Worker").unwrap().id,
        "ses_1",
        "exact wins"
    );
    assert_eq!(match_session(&live, "ses_2").unwrap().id, "ses_2");
    assert_eq!(
        match_session(&live, "docs").unwrap().id,
        "ses_3",
        "case-insensitive unique"
    );
    assert_eq!(
        match_session(&live, "\"Docs\"").unwrap().id,
        "ses_3",
        "quotes stripped"
    );
    assert!(
        match_session(&live, "WORKER")
            .unwrap_err()
            .contains("matches 2")
    );
    assert!(
        match_session(&live, "nope")
            .unwrap_err()
            .contains("No live session")
    );
    let mut gone = live.clone();
    gone[2].live = false;
    assert!(match_session(&gone, "Docs").is_err(), "only live sessions");
    // Command parsing: alias and argument.
    assert_eq!(
        crate::app::find_cmd("kill").map(|c| c.name),
        Some("close-session")
    );
}

/// An app with the current session, a background one and one foreground in another frontend.
fn close_session_app() -> App {
    let mut app = test_app(false);
    let cur = app.views[TEST_SESSION].info.clone();
    app.live = vec![
        cur,
        bg_session("ses_bg", "worker-1", SessionStatus::Busy, 0),
        bg_session("ses_fg2", "other-fe", SessionStatus::Idle, 1),
    ];
    app
}

fn is_closed(p: &crate::app::Pending) -> bool {
    matches!(p, crate::app::Pending::Closed { .. })
}

/// Opens the close-session picker, with extra live sessions `x1` and `y1` and a history entry.
fn open_close_picker(app: &mut App) {
    app.live
        .push(bg_session("ses_x", "x1", SessionStatus::Idle, 0));
    app.live
        .push(bg_session("ses_y", "y1", SessionStatus::Idle, 0));
    app.history_list = vec![bg_session("ses_h", "old-history", SessionStatus::Idle, 0)];
    app.run_command("close-session", "");
}

fn select_in_list_dialog(app: &mut App, key: &str) {
    if let Some(crate::ui::dialog::Dialog::List(d)) = app.dialog.as_mut() {
        d.select_key(key);
    }
}

#[test]
fn close_session_by_name_closes_a_background_session_at_once() {
    use crate::app::Pending;
    let mut app = close_session_app();
    type_text(&mut app, "/kill worker-1");
    press(&mut app, KeyCode::Enter);
    assert!(
        app.pending
            .values()
            .any(|p| matches!(p, Pending::Closed { name } if name == "worker-1"))
    );
    assert!(
        !app.live.iter().any(|s| s.id == "ses_bg"),
        "list updated right away"
    );
    assert_eq!(app.current.as_deref(), Some(TEST_SESSION));
}

#[test]
fn close_session_foreground_elsewhere_needs_the_exact_name() {
    let mut app = close_session_app();
    app.run_command("close-session", "OTHER-FE");
    assert!(app.pending.is_empty());
    assert!(
        app.hint
            .as_ref()
            .is_some_and(|h| h.0.contains("another frontend"))
    );
    app.run_command("close-session", "other-fe");
    assert!(app.pending.values().any(is_closed));
}

#[test]
fn close_session_picker_lists_live_sessions_and_stays_open() {
    use crate::app::Pending;
    use crate::ui::dialog::{Dialog, Kind};
    let mut app = close_session_app();
    open_close_picker(&mut app);
    let (_, s) = render(&mut app, 120, 34);
    assert!(
        s.contains("Close sessions")
            && s.contains("x1")
            && s.contains("y1")
            && !s.contains("old-history"),
        "{s}"
    );

    select_in_list_dialog(&mut app, "live:ses_x");
    press(&mut app, KeyCode::Enter);
    assert!(
        app.pending
            .values()
            .any(|p| matches!(p, Pending::Closed { name } if name == "x1"))
    );
    assert!(
        matches!(&app.dialog, Some(Dialog::List(d)) if d.kind == Kind::CloseSessions),
        "dialog stays open"
    );
    let (_, s) = render(&mut app, 120, 34);
    assert!(!s.contains("x1") && s.contains("y1"), "{s}");
}

#[test]
fn close_session_picker_closing_the_current_one_deselects() {
    use crate::app::Pending;
    let mut app = close_session_app();
    app.live.retain(|s| s.id == TEST_SESSION);
    open_close_picker(&mut app);
    app.live.retain(|s| s.id != "ses_x");
    render(&mut app, 120, 34);

    select_in_list_dialog(&mut app, "live:ses_test");
    press(&mut app, KeyCode::Enter);
    assert!(app.current.is_none());
    assert!(
        !app.pending
            .values()
            .any(|p| matches!(p, Pending::Created { .. }))
    );
    assert!(!app.quit);
}

#[test]
fn sessions_dialog_ctrl_d_closes_with_confirm() {
    use crate::app::Pending;
    use crossterm::event::{KeyCode, KeyModifiers};
    let mut app = test_app(false);
    let cur = app.views[TEST_SESSION].info.clone();
    app.live = vec![
        cur,
        bg_session("ses_bg", "worker-1", SessionStatus::Idle, 0),
    ];
    app.open_sessions();
    let (_, s) = render(&mut app, 120, 34);
    assert!(s.contains("ctrl+k pin · ctrl+d close/delete"), "{s}");
    if let Some(crate::ui::dialog::Dialog::List(d)) = app.dialog.as_mut() {
        d.select_key("live:ses_bg");
    }
    app.pending.clear();
    press_with(&mut app, KeyCode::Char('d'), KeyModifiers::CONTROL);
    let (_, s) = render(&mut app, 120, 34);
    assert!(s.contains("ctrl+d again to close"), "{s}");
    assert!(
        !app.pending
            .values()
            .any(|p| matches!(p, Pending::Closed { .. }))
    );
    press_with(&mut app, KeyCode::Char('d'), KeyModifiers::CONTROL);
    assert!(
        app.pending
            .values()
            .any(|p| matches!(p, Pending::Closed { .. }))
    );
    let (_, s) = render(&mut app, 120, 34);
    assert!(!s.contains("worker-1"), "refreshed immediately: {s}");
    assert!(app.dialog.is_some());
}

#[test]
fn sessions_dialog_creator_filter_keywords_and_badge() {
    let mut app = test_app(false);
    let mut main = bg_session("ses_main", "main", SessionStatus::Idle, 1);
    main.updated_at = 400;
    let mut w1 = bg_session("ses_w1", "worker-1", SessionStatus::Idle, 0);
    w1.parent = Some("ses_main".into());
    w1.updated_at = 300;
    let mut w2 = bg_session("ses_w2", "worker-2", SessionStatus::Idle, 0);
    w2.parent = Some("ses_other".into());
    w2.updated_at = 200;
    let mut hc = bg_session("ses_hc", "hist-child", SessionStatus::Idle, 0);
    hc.live = false;
    hc.parent = Some("ses_main".into());
    hc.title = "Closed subtask".into();
    hc.updated_at = 100;
    app.current = Some("ses_main".into());
    app.live = vec![main, w1, w2];
    app.history_list = vec![hc];
    app.open_sessions();

    let (_, s) = render(&mut app, 120, 34);
    assert!(s.contains("tab filter"), "{s}");
    assert!(s.contains(" All "), "{s}");
    assert!(s.contains("⟲ main"), "creator badge shown: {s}");
    assert!(
        s.contains("⟲ ses_other"),
        "fallback creator id badge shown: {s}"
    );

    // Search matches creator name and creator id via keywords.
    type_text(&mut app, "ses_main");
    let (_, s_kw) = render(&mut app, 120, 34);
    assert!(
        s_kw.contains("worker-1") && s_kw.contains("Closed subtask"),
        "{s_kw}"
    );
    assert!(!s_kw.contains("worker-2"), "{s_kw}");
    // Clear search and select worker-1 (created by ses_main).
    press_with(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
    if let Some(crate::ui::dialog::Dialog::List(d)) = app.dialog.as_mut() {
        d.select_key("live:ses_w1");
    }
    // Tab 1: User-created only.
    press(&mut app, KeyCode::Tab);
    let (_, s_user) = render(&mut app, 120, 34);
    assert!(s_user.contains(" User-created "), "{s_user}");
    assert!(
        s_user.contains("main")
            && !s_user.contains("worker-1")
            && !s_user.contains("worker-2")
            && !s_user.contains("Closed subtask"),
        "{s_user}"
    );
    // Tab 2: By creator of previously selected row (ses_main).
    press(&mut app, KeyCode::Tab);
    let (_, s_by) = render(&mut app, 120, 34);
    assert!(s_by.contains(" By main "), "{s_by}");
    assert!(
        s_by.contains("worker-1") && s_by.contains("Closed subtask"),
        "{s_by}"
    );
    assert!(
        !s_by.contains("worker-2") && !s_by.contains("● main"),
        "{s_by}"
    );
    assert!(s_by.contains("2/4"), "{s_by}");
    press(&mut app, KeyCode::Tab);
    let (_, s_all) = render(&mut app, 120, 34);
    assert!(s_all.contains(" All "), "{s_all}");
    assert!(
        s_all.contains("worker-1") && s_all.contains("worker-2"),
        "{s_all}"
    );
}

#[test]
fn leader_k_closes_current_and_focused_tile() {
    use crate::app::Pending;
    use crossterm::event::{KeyCode, KeyModifiers};
    let lk = |app: &mut App| {
        press_with(app, KeyCode::Char('x'), KeyModifiers::CONTROL);
        press(app, KeyCode::Char('k'));
    };
    let mut app = test_app(false);
    lk(&mut app);
    assert!(app.pending.is_empty());
    assert!(
        app.hint
            .as_ref()
            .is_some_and(|h| h.0.contains("press ctrl+x k again to close \"ref-a1b2\""))
    );
    lk(&mut app);
    assert!(
        app.pending
            .values()
            .any(|p| matches!(p, Pending::Closed { .. }))
    );
    assert!(
        app.current.is_none() && !app.quit,
        "last session: deselects without quitting"
    );
    // Watch mode: closes the focused tile.
    let mut app = test_app(false);
    app.mode = Mode::Watch;
    let bg = bg_session("ses_bg", "worker-1", SessionStatus::Idle, 0);
    app.views
        .insert("ses_bg".into(), SessionView::new(bg.clone(), false));
    app.live = vec![app.views[TEST_SESSION].info.clone(), bg];
    app.watch.focus = Some("ses_bg".into());
    app.pending.clear();
    lk(&mut app);
    lk(&mut app);
    assert!(
        app.pending
            .values()
            .any(|p| matches!(p, Pending::Closed { name } if name == "worker-1"))
    );
    assert!(!app.views.contains_key("ses_bg"));
    assert_eq!(
        app.current.as_deref(),
        Some(TEST_SESSION),
        "current untouched"
    );
    assert_eq!(app.mode, Mode::Watch);
}

#[test]
fn frame_interval_animates_background_sessions_in_sidebar() {
    let mut app = test_app(false);
    let cur = app.views[TEST_SESSION].info.clone();
    app.live = vec![
        cur,
        bg_session("ses_bg", "worker-1", SessionStatus::Busy, 0),
    ];
    let (_, s) = render(&mut app, 150, 40);
    assert!(s.contains("Live sessions") && s.contains("worker-1"), "{s}");
    app.toasts.items.clear();
    assert_eq!(
        app.frame_interval(),
        Some(std::time::Duration::from_millis(80)),
        "sidebar spinner of an unattached busy session"
    );
    // Spinner advances between ticks.
    let line_at = |app: &mut App| {
        render(app, 150, 40)
            .1
            .lines()
            .find(|l| l.contains("worker-1"))
            .unwrap()
            .to_string()
    };
    let a = line_at(&mut app);
    app.start -= std::time::Duration::from_millis(80);
    let b = line_at(&mut app);
    assert_ne!(a, b, "spinner frame changes");
    // Sidebar hidden (narrow screen) and nothing else animating: idle, no wakeups.
    render(&mut app, 100, 40);
    assert_eq!(app.frame_interval(), None);
    // Session dialog lists it too.
    app.open_sessions();
    if let Some(crate::ui::dialog::Dialog::List(l)) = app.dialog.as_mut() {
        l.opened -= std::time::Duration::from_secs(1)
    }
    assert!(app.frame_interval().is_some());
    app.dialog = None;
    // Split view: an unattached busy tile animates.
    app.mode = Mode::Watch;
    assert!(app.frame_interval().is_some());
}

#[test]
fn sidebar_live_sessions_show_working_time() {
    let mut app = test_app(false);
    let cur = app.views[TEST_SESSION].info.clone();
    let mut busy = bg_session("ses_bg", "worker-1", SessionStatus::Running, 0);
    busy.busy_since = Some(ref_proto::now_ms() - 125_000);
    // Busy without busy_since (older backend): spinner only.
    let legacy = bg_session("ses_old", "legacy", SessionStatus::Busy, 0);
    app.live = vec![
        cur,
        busy,
        legacy,
        bg_session("ses_idle", "idler", SessionStatus::Idle, 0),
    ];
    let rows = |app: &mut App| {
        let (term, _) = render(app, 150, 40);
        sidebar_text(&term, 150)
    };
    let row = |rows: &[String], name: &str| {
        rows.iter()
            .find(|l| l.contains(name))
            .unwrap()
            .trim_end()
            .to_string()
    };
    let r = rows(&mut app);
    let w1 = row(&r, "worker-1");
    assert!(w1.ends_with("2m 05s") || w1.ends_with("2m 06s"), "{w1:?}");
    assert!(w1.contains("bg  "), "right-aligned: {w1:?}");
    assert!(row(&r, "idler").ends_with("bg"), "idle: no time");
    assert!(row(&r, "legacy").ends_with("bg"));
    // Ticks with the clock.
    app.live[1].busy_since = app.live[1].busy_since.map(|t| t - 60_000);
    let r = rows(&mut app);
    assert!(row(&r, "worker-1").contains("3m 0"));
    // Idle again: the time disappears.
    app.live[1].status = SessionStatus::Idle;
    let r = rows(&mut app);
    assert!(row(&r, "worker-1").ends_with("bg"));
}

#[test]
fn running_tool_spinner_animates_in_tiles() {
    let mut app = test_app(false);
    app.mode = Mode::Watch;
    let mut inf = bg_session("ses_bg", "worker-1", SessionStatus::Running, 0);
    inf.created_at = 1;
    let mut v = SessionView::new(inf.clone(), false);
    let msgs = transcript(true);
    let total = msgs.len() as u64;
    v.load_snapshot(SessionSnapshot {
        info: inf.clone(),
        messages: msgs,
        more_before: false,
        total_messages: total,
        start_seq: 0,
        layout_metas: vec![],
        pending_permissions: vec![],
        pending_questions: vec![],
        queued: vec![],
    });
    app.views.insert("ses_bg".into(), v);
    app.live = vec![inf];
    let row = |app: &mut App| {
        render(app, 120, 30)
            .1
            .lines()
            .find(|l| l.contains("grep"))
            .unwrap()
            .to_string()
    };
    let a = row(&mut app);
    app.start -= std::time::Duration::from_millis(80);
    let b = row(&mut app);
    assert_ne!(a, b, "tool spinner in tile advances:\n{a}\n{b}");
}

#[test]
fn split_tile_shows_queue_and_expands_tools() {
    use crate::app::Pending;
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = test_app(false);
    app.mode = Mode::Watch;
    let inf = bg_session("ses_bg", "worker-1", SessionStatus::Busy, 0);
    let mut v = SessionView::new(inf.clone(), false);
    let queued = vec![msg(
        "q1",
        Role::User,
        MessageSource::Human,
        0,
        "then update the docs",
    )];
    let msgs = transcript(false);
    let total = msgs.len() as u64;
    v.load_snapshot(SessionSnapshot {
        info: inf.clone(),
        messages: msgs,
        more_before: false,
        total_messages: total,
        start_seq: 0,
        layout_metas: vec![],
        pending_permissions: vec![],
        pending_questions: vec![],
        queued,
    });
    app.views.insert("ses_bg".into(), v);
    app.live = vec![inf];
    let (_, s) = render(&mut app, 120, 40);
    let label = crate::ui::message::UNQUEUE_LABEL;
    assert!(
        s.lines()
            .any(|l| l.contains("queued then update the docs") && l.contains(label)),
        "{s}"
    );
    // Position of `pat` on screen (display columns).
    let at = |s: &str, pat: &str| {
        let (y, l) = s
            .lines()
            .enumerate()
            .find(|(_, l)| l.contains(pat))
            .expect(pat);
        (text::width(&l[..l.find(pat).unwrap()]) as u16, y as u16)
    };
    let click = |app: &mut App, (x, y): (u16, u16)| {
        crate::input::handle(app, mouse(MouseEventKind::Down(MouseButton::Left), x, y))
    };
    // Clicking a tool row opens its details in the tile; clicking again closes them (no zoom).
    click(&mut app, at(&s, "Run the unit tests"));
    assert_eq!(app.watch.focus.as_deref(), Some("ses_bg"));
    let (_, s) = render(&mut app, 120, 40);
    assert!(s.contains("▾ ✓ bash") && s.contains("╭─ input"), "{s}");
    click(&mut app, at(&s, "Run the unit tests"));
    assert_eq!(app.mode, Mode::Watch);
    let (_, s) = render(&mut app, 120, 40);
    assert!(s.contains("▸ ✓ bash") && !s.contains("╭─ input"), "{s}");
    // `✕ remove` withdraws the queued prompt.
    app.pending.clear();
    click(&mut app, at(&s, label));
    assert!(
        app.pending
            .values()
            .any(|p| matches!(p, Pending::Unqueue { .. }))
    );
}

#[test]
fn split_view_expand_collapse_releases_hold_in_busy_tile() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = test_app(false);
    app.mode = Mode::Watch;
    let inf = bg_session("ses_bg", "worker-1", SessionStatus::Busy, 0);
    let mut v = SessionView::new(inf.clone(), false);
    let msgs = transcript(false);
    let total = msgs.len() as u64;
    v.load_snapshot(SessionSnapshot {
        info: inf.clone(),
        messages: msgs,
        more_before: false,
        total_messages: total,
        start_seq: 0,
        layout_metas: vec![],
        pending_permissions: vec![],
        pending_questions: vec![],
        queued: vec![],
    });
    app.views.insert("ses_bg".into(), v);
    app.live = vec![inf];
    app.watch.focus = Some("ses_bg".into());
    let (_, s) = render(&mut app, 100, 22);
    let base_max = app.views["ses_bg"].chat.max_scroll();
    let base_top = app.views["ses_bg"].chat.top();
    let at = |scr: &str, pat: &str| {
        let (y, line) = scr
            .lines()
            .enumerate()
            .find(|(_, l)| l.contains(pat))
            .expect(pat);
        (
            text::width(&line[..line.find(pat).unwrap()]) as u16,
            y as u16,
        )
    };
    // Click to expand while following at the bottom of the busy tile.
    let p = at(&s, "Run the unit tests");
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), p.0, p.1),
    );
    let (_, expanded_s) = render(&mut app, 100, 22);
    assert!(
        expanded_s.contains("▾ ✓ bash") && expanded_s.contains("╭─ input"),
        "{expanded_s}"
    );
    assert!(
        app.views["ses_bg"].chat.max_scroll() > base_max,
        "{expanded_s}"
    );
    // Click again to collapse while still following: no blank hold rows linger.
    let p2 = at(&expanded_s, "Run the unit tests");
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), p2.0, p2.1),
    );
    let (_, collapsed_s) = render(&mut app, 100, 22);
    assert!(!collapsed_s.contains("╭─ input"), "{collapsed_s}");
    assert_eq!(
        app.views["ses_bg"].chat.max_scroll(),
        base_max,
        "no blank hold rows:\n{collapsed_s}"
    );
    assert_eq!(app.views["ses_bg"].chat.top(), base_top);
    assert_eq!(collapsed_s, s);
}

#[test]
fn terminal_title_text() {
    let mut app = test_app(false);
    assert_eq!(app.terminal_title(), "R|Fix the parser");
    app.views.get_mut(TEST_SESSION).unwrap().info.title = "  ".into();
    assert_eq!(app.terminal_title(), "R|ref-a1b2");
    app.mode = Mode::Watch;
    app.live = vec![
        bg_session("a", "a", SessionStatus::Idle, 0),
        bg_session("b", "b", SessionStatus::Idle, 0),
    ];
    assert_eq!(app.terminal_title(), "R|watch (2)");
    app.mode = Mode::Chat;
    app.current = None;
    assert_eq!(app.terminal_title(), "R|ref");
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\x1b' {
            for d in it.by_ref() {
                if d == 'm' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[test]
fn exit_screen_text() {
    let app = test_app(false);
    let raw = app.epilogue().unwrap();
    assert!(raw.contains("\x1b[38;2;250;178;131m"), "primary truecolor");
    let t = strip_ansi(&raw);
    let lines: Vec<&str> = t.lines().collect();
    assert_eq!(lines[0], "  █▀▀▄ █▀▀▀ █▀▀▀");
    assert_eq!(lines[1], "  █▄▄▀ █▀▀  █▀▀");
    assert_eq!(lines[2], "  █  █ █▄▄▄ █");
    assert_eq!(lines[3], "");
    assert_eq!(lines[4], "  Session   Fix the parser  (ref-a1b2)");
    assert_eq!(lines[5], "  Continue  ref -s ses_test");
    // Empty session: logo only, no resume line.
    let mut app = test_app(false);
    app.views
        .get_mut(TEST_SESSION)
        .unwrap()
        .apply(SessionEvent::Reset {
            more_before: false,
            messages: vec![],
            total_messages: 0,
            start_seq: 0,
            layout_metas: vec![],
        });
    let t = strip_ansi(&app.epilogue().unwrap());
    assert!(t.contains("█▀▀▄") && !t.contains("Continue") && !t.contains("Session"));
    // No session at all (e.g. --watch): nothing printed.
    let mut app = test_app(false);
    app.current = None;
    assert!(app.epilogue().is_none());
}

fn todo_input() -> serde_json::Value {
    json!({"todos": [
        {"content": "Read the parser", "status": "completed"},
        {"content": "Fix the off-by-one", "status": "in_progress"},
        {"content": "Run the tests", "status": "pending"}
    ]})
}

#[test]
fn todo_rendering_everywhere() {
    let mut app = test_app(false);
    {
        let v = app.views.get_mut(TEST_SESSION).unwrap();
        v.message_at_mut(1).unwrap().parts.push(tool(
            "c9",
            "todo",
            "Plan the work",
            todo_input(),
            ToolState::Done,
            Some("ok"),
            ts(14, 2, 19),
            Some(ts(14, 2, 19)),
        ));
        v.versions[1] += 1000;
        v.info.todos = crate::ui::todo::parse_todos(&todo_input());
    }
    let (term, s) = render(&mut app, 150, 50);
    println!("{s}");
    // Transcript: collapsed todo row shows the list inline.
    let i = s
        .lines()
        .position(|l| l.contains("todo  Plan the work"))
        .expect("todo row");
    let lines: Vec<&str> = s.lines().collect();
    assert!(lines[i].contains("1/3 done"));
    assert!(lines[i + 1].contains("☑ Read the parser"));
    assert!(lines[i + 2].contains("◐ Fix the off-by-one"));
    assert!(lines[i + 3].contains("☐ Run the tests"));
    // Styles: in-progress bold primary, completed struck through.
    let y = (i + 2) as u16;
    let x = find_col(&term, y, "F").unwrap();
    let c = &term.backend().buffer()[(x, y)];
    assert_eq!(c.fg, crate::ui::theme::PRIMARY);
    assert!(c.modifier.contains(ratatui::style::Modifier::BOLD));
    let y = (i + 1) as u16;
    let x = find_col(&term, y, "R").unwrap();
    assert!(
        term.backend().buffer()[(x, y)]
            .modifier
            .contains(ratatui::style::Modifier::CROSSED_OUT)
    );
    // Sidebar section with progress.
    assert!(s.contains("Todo  1/3"), "{s}");
    // Prompt indicator.
    assert!(s.contains("◐ Fix the off-by-one · 1/3"), "{s}");
    // All done: indicator disappears.
    app.views
        .get_mut(TEST_SESSION)
        .unwrap()
        .info
        .todos
        .iter_mut()
        .for_each(|t| t.status = TodoStatus::Completed);
    let (_, s) = render(&mut app, 150, 50);
    assert!(!s.contains("· 3/3"), "{s}");
    // Split tile: in-progress item in the title.
    let mut app = test_app(false);
    app.mode = Mode::Watch;
    let mut inf = bg_session("ses_bg", "worker-1", SessionStatus::Busy, 0);
    inf.todos = crate::ui::todo::parse_todos(&todo_input());
    let mut v = SessionView::new(inf.clone(), false);
    let msgs = transcript(false);
    let total = msgs.len() as u64;
    v.load_snapshot(SessionSnapshot {
        info: inf.clone(),
        messages: msgs,
        more_before: false,
        total_messages: total,
        start_seq: 0,
        layout_metas: vec![],
        pending_permissions: vec![],
        pending_questions: vec![],
        queued: vec![],
    });
    app.views.insert("ses_bg".into(), v);
    app.live = vec![inf];
    let (_, s) = render(&mut app, 120, 30);
    let title = s.lines().find(|l| l.contains("worker-1")).unwrap();
    assert!(title.contains("◐ Fix the off-by-one · 1/3"), "{title}");
}

// ---------------------------------------------------------------------------
// Sidebar MCP / todo / live sections, /mcp dialog, compaction divider
// ---------------------------------------------------------------------------

fn mcp_servers() -> Vec<McpServerStatus> {
    vec![
        McpServerStatus { name: "context7".into(), status: "connected".into(), error: None, tools: vec!["context7_resolve-library-id".into(), "context7_get-library-docs".into()] },
        McpServerStatus { name: "github".into(), status: "failed".into(), error: Some("HTTP 401 Unauthorized: missing or invalid bearer token; set GITHUB_TOKEN in the environment and restart the backend".into()), tools: vec![] },
        McpServerStatus { name: "docs-搜索".into(), status: "connecting".into(), error: None, tools: vec![] },
        McpServerStatus { name: "sqlite".into(), status: "disabled".into(), error: None, tools: vec![] },
    ]
}

/// Answers the pending McpStatus request with `servers`.
fn answer_mcp(app: &mut App, servers: &[McpServerStatus]) {
    let id = *app
        .pending
        .iter()
        .find(|(_, p)| matches!(p, crate::app::Pending::Mcp))
        .map(|(k, _)| k)
        .expect("mcp_status request");
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id,
        ok: true,
        result: json!({ "servers": servers }),
        error: None,
        code: None,
    }));
}

fn sidebar_app() -> App {
    let mut app = test_app(false);
    app.refresh_mcp();
    answer_mcp(&mut app, &mcp_servers());
    let v = app.views.get_mut(TEST_SESSION).unwrap();
    v.info.todos = crate::ui::todo::parse_todos(&json!({"todos": [
        {"content": "Read the parser", "status": "completed"},
        {"content": "Fix the off-by-one", "status": "in_progress"},
        {"content": "Run the tests", "status": "pending"},
        {"content": "Update the changelog", "status": "pending"},
        {"content": "检查中文输出的宽度", "status": "pending"}
    ]}));
    let cur = v.info.clone();
    app.live = vec![
        cur,
        bg_session("ses_b1", "worker-1", SessionStatus::Busy, 0),
        bg_session("ses_b2", "docs-writer", SessionStatus::Idle, 0),
        bg_session("ses_b3", "reviewer", SessionStatus::Idle, 1),
    ];
    app
}

/// Sidebar text (columns right of the separator) per screen row.
fn sidebar_text(term: &Terminal<TestBackend>, w: u16) -> Vec<String> {
    let buf = term.backend().buffer();
    let x0 = w - crate::render::SIDEBAR_W;
    (0..buf.area.height)
        .map(|y| {
            let mut s = String::new();
            let mut skip = 0;
            for x in x0..w {
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                let sym = buf[(x, y)].symbol();
                s.push_str(sym);
                skip = text::width(sym).saturating_sub(1);
            }
            s.trim().to_string()
        })
        .collect()
}

fn pos(lines: &[String], pat: &str) -> Option<usize> {
    lines.iter().position(|l| l.contains(pat))
}

#[test]
fn sidebar_mcp_todo_live_tall() {
    let mut app = sidebar_app();
    let (term, s) = render(&mut app, 160, 40);
    println!("{s}");
    let sb = sidebar_text(&term, 160);
    // Session info.
    assert!(
        pos(&sb, "Fix the parser").is_some()
            && pos(&sb, "ref-a1b2").is_some()
            && pos(&sb, TEST_SESSION).is_some()
    );
    assert!(
        pos(&sb, "agent build").is_some() && pos(&sb, "model claude-opus-5.5 high").is_some(),
        "{sb:#?}"
    );
    assert!(sb.iter().any(|l| l.contains("━")), "context bar");
    // MCP section: summary, each server with a status dot and detail.
    let m = pos(&sb, "MCP  1/3").expect("mcp header");
    let ctx7 = &sb[pos(&sb, "context7").unwrap()];
    assert!(
        ctx7.starts_with("● context7") && ctx7.ends_with("2 tools"),
        "{ctx7}"
    );
    assert!(
        sb[pos(&sb, "github").unwrap()].contains("HTTP 401"),
        "short error"
    );
    assert!(
        sb[pos(&sb, "docs-搜索").unwrap()].starts_with("◌ docs-搜索")
            && sb[pos(&sb, "docs-搜索").unwrap()].contains("connecting")
    );
    assert!(
        sb[pos(&sb, "sqlite").unwrap()].starts_with("○ sqlite")
            && sb[pos(&sb, "sqlite").unwrap()].ends_with("disabled")
    );
    // Dot colours.
    let buf = term.backend().buffer();
    let x0 = 160 - crate::render::SIDEBAR_W + 1;
    let dot_fg = |name: &str| buf[(x0, pos(&sb, name).unwrap() as u16)].fg;
    assert_eq!(dot_fg("context7"), crate::ui::theme::SUCCESS);
    assert_eq!(dot_fg("github"), crate::ui::theme::ERROR);
    // Todo (all items) and live sessions (all), in display order MCP, Todo, Live.
    let t = pos(&sb, "Todo  1/5").expect("todo header");
    for item in [
        "☑ Read the parser",
        "◐ Fix the off-by-one",
        "☐ Run the tests",
        "☐ Update the changelog",
        "☐ 检查中文输出的宽度",
    ] {
        assert!(pos(&sb, item).is_some(), "{item}: {sb:#?}");
    }
    let l = pos(&sb, "Live sessions  4").expect("live header");
    assert!(m < t && t < l, "order: {sb:#?}");
    for n in ["worker-1", "docs-writer", "reviewer"] {
        assert!(pos(&sb, n).is_some(), "{n}");
    }
    // Nothing spills past the screen edge / into the separator column.
    for (y, line) in sb.iter().enumerate() {
        assert!(
            text::width(line) <= crate::render::SIDEBAR_W as usize,
            "row {y}: {line}"
        );
    }
    // Click targets: MCP row opens the dialog on that server, session row switches.
    let gy = pos(&sb, "github").unwrap() as u16;
    assert!(
        app.sidebar_mcp_rows
            .iter()
            .any(|(y, n)| *y == gy && n == "github")
    );
    let wy = pos(&sb, "worker-1").unwrap() as u16;
    assert!(
        app.sidebar_rows
            .iter()
            .any(|(y, id)| *y == wy && id == "ses_b1")
    );
    app.screen = ratatui::layout::Rect::new(0, 0, 160, 40);
    use crossterm::event::{MouseButton, MouseEventKind};
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), 150, gy),
    );
    match &app.dialog {
        Some(crate::ui::dialog::Dialog::Mcp(d)) => assert!(d.expanded.contains("github")),
        _ => panic!("mcp dialog not opened"),
    }
}

#[test]
fn sidebar_short_screen_priority() {
    let mut app = sidebar_app();
    let (term, s) = render(&mut app, 160, 20);
    println!("{s}");
    let sb = sidebar_text(&term, 160);
    // Compact session info (title + agent/model + context) still on top.
    assert!(pos(&sb, "Fix the parser").is_some());
    assert!(pos(&sb, "build · claude-opus-5.5").is_some(), "{sb:#?}");
    // Todo has priority over MCP, MCP over live sessions; all sections still present in some form.
    let t = pos(&sb, "Todo").expect("todo");
    assert!(
        pos(&sb, "◐ Fix the off-by-one").is_some(),
        "in-progress item kept: {sb:#?}"
    );
    assert!(pos(&sb, "MCP").is_some(), "mcp header/summary: {sb:#?}");
    let todo_rows = sb
        .iter()
        .filter(|l| l.starts_with('☐') || l.starts_with('◐') || l.starts_with('☑'))
        .count();
    let mcp_rows = ["context7", "github", "sqlite"]
        .iter()
        .filter(|n| pos(&sb, n).is_some())
        .count();
    let live_rows = ["worker-1", "docs-writer", "reviewer"]
        .iter()
        .filter(|n| pos(&sb, n).is_some())
        .count();
    assert!(
        todo_rows >= mcp_rows && mcp_rows >= live_rows,
        "priority: todo {todo_rows} mcp {mcp_rows} live {live_rows}\n{sb:#?}"
    );
    assert!(t > 0);
    // Nothing drawn beyond the screen; sidebar rows within width.
    assert_eq!(sb.len(), 20);
    for line in &sb {
        assert!(
            text::width(line) <= crate::render::SIDEBAR_W as usize,
            "{line}"
        );
    }
    // Very short: no panic, session title first.
    for h in [6u16, 10, 14] {
        let (term, _) = render(&mut app, 160, h);
        let sb = sidebar_text(&term, 160);
        assert!(sb[1].contains("Fix the parser"), "h={h}: {sb:#?}");
    }
}

#[test]
fn mcp_status_fallback_and_polling() {
    use crate::app::Pending;
    let mut app = test_app(false);
    app.config.as_mut().unwrap().mcp = vec![
        ("ctx".into(), "connected (3 tools)".into()),
        ("gh".into(), "failed: HTTP 401".into()),
        ("slow".into(), "connecting".into()),
    ];
    app.refresh_mcp();
    // Older backend: McpStatus fails -> GetConfig lines are used.
    let id = *app
        .pending
        .iter()
        .find(|(_, p)| matches!(p, Pending::Mcp))
        .map(|(k, _)| k)
        .unwrap();
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id,
        ok: false,
        result: json!(null),
        error: Some("unknown op".into()),
        code: None,
    }));
    assert!(
        app.mcp_fallback && app.toasts.items.is_empty(),
        "no error toast"
    );
    assert_eq!(
        app.mcp.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        ["ctx", "gh", "slow"]
    );
    assert_eq!(app.mcp[0].n_tools, 3);
    assert_eq!(app.mcp[1].error.as_deref(), Some("HTTP 401"));
    // Something is connecting: a poll is scheduled (via GetConfig in fallback mode).
    assert!(app.mcp_poll_at.is_some());
    app.mcp_poll_at = Some(std::time::Instant::now());
    app.housekeeping();
    let id = *app
        .pending
        .iter()
        .find(|(_, p)| matches!(p, Pending::ConfigMcp))
        .map(|(k, _)| k)
        .expect("fallback poll");
    let mut cfg = app.config.clone().unwrap();
    cfg.mcp[2].1 = "connected (1 tools)".into();
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id,
        ok: true,
        result: json!({ "config": cfg }),
        error: None,
        code: None,
    }));
    assert!(app.mcp_poll_at.is_none(), "all settled: no more polling");
    assert_eq!(app.mcp[2].state, crate::ui::mcp::McpState::Connected);
    // New backend: McpStatus answered; polls while connecting, stops when settled.
    let mut app = test_app(false);
    app.refresh_mcp();
    answer_mcp(&mut app, &mcp_servers());
    assert!(app.mcp_poll_at.is_some());
    assert_eq!(
        app.mcp[0].tools,
        ["resolve-library-id", "get-library-docs"],
        "prefix stripped"
    );
    app.refresh_mcp();
    let settled: Vec<McpServerStatus> = mcp_servers()
        .into_iter()
        .map(|mut s| {
            if s.status == "connecting" {
                s.status = "connected".into();
            }
            s
        })
        .collect();
    answer_mcp(&mut app, &settled);
    assert!(app.mcp_poll_at.is_none());
    app.toasts.items.clear();
    render(&mut app, 100, 30);
    assert_eq!(app.frame_interval(), None, "idle: no wakeups once settled");
}

#[test]
fn mcp_dialog_render_wrap_cjk_and_tools() {
    let mut app = sidebar_app();
    type_text(&mut app, "/mcp");
    press(&mut app, KeyCode::Enter);
    assert!(
        matches!(app.dialog, Some(crate::ui::dialog::Dialog::Mcp(_))),
        "/mcp opens the dialog"
    );
    assert!(
        app.pending
            .values()
            .any(|p| matches!(p, crate::app::Pending::Mcp)),
        "refreshed on open"
    );
    if let Some(crate::ui::dialog::Dialog::Mcp(d)) = app.dialog.as_mut() {
        d.opened -= std::time::Duration::from_secs(1);
    }
    for (w, h) in [(100u16, 30u16), (60, 30)] {
        let (_, s) = render(&mut app, w, h);
        println!("{s}");
        assert!(s.contains("MCP servers"));
        assert!(
            s.contains("context7") && s.contains("connected · 2 tools"),
            "{s}"
        );
        assert!(s.contains("docs-搜索") && s.contains("connecting"));
        assert!(s.contains("sqlite") && s.contains("disabled"));
        // The long error wraps inside the dialog (both the start and the end are shown).
        assert!(s.contains("error: HTTP 401 Unauthorized"), "{s}");
        assert!(
            s.contains("restart the backend") || s.contains("more lines"),
            "{s}"
        );
        for l in s.lines() {
            assert!(text::width(l) <= w as usize);
        }
        // Frame borders line up on CJK rows.
        let row = s.lines().find(|l| l.contains("docs-搜索")).unwrap();
        assert!(row.contains("connecting… │"), "{row}");
    }
    // Enter expands the selected (first) server's tools.
    let (_, s) = render(&mut app, 100, 30);
    assert!(!s.contains("resolve-library-id"));
    press(&mut app, KeyCode::Enter);
    let (_, s) = render(&mut app, 100, 30);
    assert!(
        s.contains("resolve-library-id") && s.contains("get-library-docs"),
        "{s}"
    );
    assert!(
        s.contains("▾") && s.contains("1/3 connected · 2 tools"),
        "{s}"
    );
    // ← collapses, ↓ moves, esc closes. Read-only: no requests besides refreshes.
    press(&mut app, KeyCode::Left);
    let (_, s) = render(&mut app, 100, 30);
    assert!(!s.contains("resolve-library-id"));
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Esc);
    assert!(app.dialog.is_none());
    // Autocomplete + palette + help list it.
    type_text(&mut app, "/mc");
    assert!(
        app.ac
            .as_ref()
            .is_some_and(|a| a.items.iter().any(|i| i.1 == "/mcp"))
    );
    app.editor.clear();
    app.ac = None;
    app.open_help();
    render(&mut app, 120, 80);
    if let Some(crate::ui::dialog::Dialog::Help(h)) = app.dialog.as_mut() {
        h.scroll_by(1000);
    }
    let (_, s) = render(&mut app, 120, 80);
    assert!(
        s.contains("/mcp") && s.contains("MCP servers: status"),
        "{s}"
    );
    // Empty / not loaded.
    let mut app = test_app(false);
    app.open_mcp(None);
    // Past the grow-in animation (a 6-row dialog has no content rows while it grows).
    if let Some(crate::ui::dialog::Dialog::Mcp(d)) = app.dialog.as_mut() {
        d.opened -= std::time::Duration::from_secs(1);
    }
    let (_, s) = render(&mut app, 100, 30);
    assert!(s.contains("Loading MCP servers"), "{s}");
}

fn msg(id: &str, role: Role, source: MessageSource, t: i64, text: &str) -> Message {
    Message {
        id: id.into(),
        role,
        time: t,
        time_end: (role == Role::Assistant).then_some(t + 1000),
        source,
        parts: vec![Part::Text {
            text: text.into(),
            time: t,
        }],
        model: None,
        agent: Some("build".into()),
        usage: None,
        error: None,
    }
}

/// Old conversation, compaction, new conversation.
fn compacted_app() -> App {
    let mut app = test_app(false);
    let mut msgs = transcript(false);
    msgs.push(msg(
        "c1",
        Role::User,
        MessageSource::Compaction,
        ts(14, 5, 0),
        "## Goal\nFix the parser off-by-one.\n\n## Done\n- Fixed loop bound",
    ));
    msgs.push(msg(
        "m3",
        Role::User,
        MessageSource::Human,
        ts(14, 6, 0),
        "Now update the changelog",
    ));
    msgs.push(msg(
        "m4",
        Role::Assistant,
        MessageSource::Human,
        ts(14, 6, 1),
        "Changelog updated.",
    ));
    let v = app.views.get_mut(TEST_SESSION).unwrap();
    let info = v.info.clone();
    let total = msgs.len() as u64;
    v.load_snapshot(SessionSnapshot {
        info,
        messages: msgs,
        more_before: false,
        total_messages: total,
        start_seq: 0,
        layout_metas: vec![],
        pending_permissions: vec![],
        pending_questions: vec![],
        queued: vec![],
    });
    app
}

/// Row index of the compaction divider in a rendered screen.
fn divider_row(lines: &[&str]) -> usize {
    lines
        .iter()
        .position(|l| l.contains("◇ Context compacted"))
        .expect("divider")
}

#[test]
fn compaction_divider_spans_the_width_and_dims_old_messages() {
    let mut app = compacted_app();
    let (term, s) = render(&mut app, 100, 60);
    println!("{s}");
    let lines: Vec<&str> = s.lines().collect();

    // Old messages are all still there.
    assert!(
        s.contains("Please fix the off-by-one")
            && s.contains("bash  Run the unit tests")
            && s.contains("Now update the changelog")
            && s.contains("Changelog updated.")
    );

    // Divider row across the width.
    let dy = divider_row(&lines);
    let dl = lines[dy];
    assert!(
        dl.contains("──── ◇ Context compacted · 14:05:00 · click to view summary"),
        "{dl}"
    );
    assert!(
        dl.contains("──────") && dl.trim_end().ends_with("⧉ copy"),
        "{dl}"
    );
    assert!(text::width(dl.trim_end()) >= 95, "spans the width: {dl}");
    assert!(
        lines[dy + 1].contains(crate::ui::message::CONTEXT_HINT),
        "hint row"
    );
    let old_user = lines.iter().position(|l| l.contains("Please fix")).unwrap();
    let new_user = lines.iter().position(|l| l.contains("Now update")).unwrap();
    assert!(old_user < dy && dy < new_user);

    // Old messages are dimmed, new ones are not.
    let buf = term.backend().buffer();
    let fg_at = |y: usize, sym: &str| buf[(find_col(&term, y as u16, sym).unwrap(), y as u16)].fg;
    assert_ne!(
        fg_at(old_user, "P"),
        crate::ui::theme::TEXT,
        "old prompt dimmed"
    );
    assert_eq!(
        fg_at(new_user, "N"),
        crate::ui::theme::TEXT,
        "new prompt normal"
    );
}

#[test]
fn compaction_divider_keeps_hit_zones_and_undo_of_old_messages() {
    use crate::ui::Hit;
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = compacted_app();
    let (term, s) = render(&mut app, 100, 60);
    let lines: Vec<&str> = s.lines().collect();
    let dy = divider_row(&lines);

    // Copy/undo on old messages still work (hit zones unchanged).
    let uy = lines
        .iter()
        .position(|l| l.contains("14:02:11") && l.contains("↶ undo"))
        .expect("old user meta") as u16;
    let ux = find_col(&term, uy, "↶").unwrap();
    let cx = find_col(&term, uy, "⧉").unwrap();
    let ay = lines
        .iter()
        .position(|l| l.contains("Build · claude") && l.contains("14:02:13"))
        .expect("old assistant footer") as u16;
    let ax = find_col(&term, ay, "⧉").unwrap();
    let cdx = find_col(&term, dy as u16, "⧉").unwrap();
    {
        let v = app.views.get_mut(TEST_SESSION).unwrap();
        assert_eq!(v.chat.hit_at(ux, uy), Some(Hit::Undo { msg: "m1".into() }));
        assert_eq!(v.chat.hit_at(cx, uy), Some(Hit::Copy { msg: "m1".into() }));
        assert_eq!(v.chat.hit_at(ax, ay), Some(Hit::Copy { msg: "m2".into() }));
        assert_eq!(
            v.chat.hit_at(cdx, dy as u16),
            Some(Hit::Copy { msg: "c1".into() })
        );
        assert_eq!(
            v.chat.hit_at(10, dy as u16),
            Some(Hit::Toggle {
                msg: "c1".into(),
                part: 0
            })
        );
    }

    // Undo before the divider removes 5 messages (incl. the compaction; the server handles it).
    app.screen = ratatui::layout::Rect::new(0, 0, 100, 60);
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), ux + 1, uy),
    );
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Up(MouseButton::Left), ux + 1, uy),
    );
    assert!(
        app.pending
            .values()
            .any(|p| matches!(p, crate::app::Pending::Undo { count: Some(5), .. }))
    );
}

#[test]
fn compaction_divider_expands_summary_and_old_tool_and_fits_narrow() {
    let mut app = compacted_app();
    render(&mut app, 100, 60);

    // Old tool still expands; the summary expands on click.
    let v = app.views.get_mut(TEST_SESSION).unwrap();
    v.chat.toggle("m2", 2);
    v.chat.toggle("c1", 0);
    let (_, s) = render(&mut app, 100, 80);
    assert!(
        s.contains("╭─ input") && s.contains("test result: ok"),
        "old tool expanded: {s}"
    );
    assert!(
        s.contains("click to hide summary") && s.contains("Fix the parser off-by-one."),
        "{s}"
    );

    // Narrow: the divider fits.
    let (_, s) = render(&mut app, 40, 30);
    for l in s.lines() {
        assert!(text::width(l) <= 40);
    }
}

#[test]
fn transcript_starting_with_a_compaction_has_no_older_messages_hint() {
    // An old backend's transcript STARTS with a compaction.
    let mut app = test_app(false);
    let v = app.views.get_mut(TEST_SESSION).unwrap();
    let info = v.info.clone();
    v.load_snapshot(SessionSnapshot {
        info,
        messages: vec![msg(
            "c1",
            Role::User,
            MessageSource::Compaction,
            ts(14, 5, 0),
            "sum",
        )],
        more_before: false,
        total_messages: 1,
        start_seq: 0,
        layout_metas: vec![],
        pending_permissions: vec![],
        pending_questions: vec![],
        queued: vec![],
    });
    let (_, s) = render(&mut app, 100, 30);
    assert!(s.contains("◇ Context compacted") && !s.contains(crate::ui::message::CONTEXT_HINT));
}

#[test]
fn compaction_divider_in_split_tile() {
    let mut app = test_app(false);
    app.mode = Mode::Watch;
    let inf = bg_session("ses_bg", "worker-1", SessionStatus::Idle, 0);
    let mut v = SessionView::new(inf.clone(), false);
    let src = compacted_app();
    let msgs: Vec<Message> = src.views[TEST_SESSION]
        .materialized_iter()
        .cloned()
        .collect();
    let total = msgs.len() as u64;
    v.load_snapshot(SessionSnapshot {
        info: inf.clone(),
        messages: msgs,
        more_before: false,
        total_messages: total,
        start_seq: 0,
        layout_metas: vec![],
        pending_permissions: vec![],
        pending_questions: vec![],
        queued: vec![],
    });
    app.views.insert("ses_bg".into(), v);
    app.live = vec![inf];
    let (_, s) = render(&mut app, 120, 40);
    println!("{s}");
    let l = s
        .lines()
        .find(|l| l.contains("◇ compacted"))
        .expect("compact divider");
    assert!(l.contains("── ◇ compacted 14:05:00 ───"), "{l}");
    assert!(s.contains("Changelog updated."));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mock_compact_keeps_history() {
    let (out_tx, out_rx) = mpsc::unbounded_channel();
    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel();
    tokio::spawn(crate::mock::run(TEST_CWD.into(), out_rx, ev_tx));
    let plan = InitPlan {
        prompt: Some("hello".into()),
        ..Default::default()
    };
    let mut app = App::new(Client::new(out_tx), TEST_CWD.into(), plan);
    pump(&mut app, &mut ev_rx, PUMP_TILES_MS).await;
    assert!(
        app.mcp
            .iter()
            .any(|e| e.name == "github" && e.error.is_some()),
        "mcp status fetched on connect"
    );
    for _ in 0..60 {
        if let Some((sid, req, _)) = app.active_permission() {
            app.reply_permission(&sid, &req.call_id, true, false);
        }
        if app
            .cur()
            .is_some_and(|v| v.loaded && !v.is_busy() && v.slots.len() >= 2)
        {
            break;
        }
        pump(&mut app, &mut ev_rx, PUMP_POLL_MS).await;
    }
    let before = app.cur().unwrap().slots.len();
    assert!(before >= 2);
    app.run_command("compact", "");
    pump(&mut app, &mut ev_rx, PUMP_TURN_MS).await;
    let v = app.cur().unwrap();
    assert_eq!(
        v.slots.len(),
        before + 1,
        "old messages kept, compaction appended"
    );
    assert_eq!(v.last_message().unwrap().source, MessageSource::Compaction);
    let (_, s) = render(&mut app, 100, 40);
    assert!(
        s.contains("◇ Context compacted") && s.contains(crate::ui::message::CONTEXT_HINT),
        "{s}"
    );
}

// ---------------------------------------------------------------------------
// Split view scrolling / focus, sidebar scrolling, expand without layout shift
// ---------------------------------------------------------------------------

/// A long transcript: `n` user/assistant exchanges.
fn long_transcript(tag: &str, n: usize) -> Vec<Message> {
    let mut out = Vec::new();
    for i in 0..n {
        out.push(msg(
            &format!("{tag}u{i}"),
            Role::User,
            MessageSource::Human,
            ts(10, 0, 0) + i as i64 * 1000,
            &format!("{tag} question {i}"),
        ));
        out.push(msg(
            &format!("{tag}a{i}"),
            Role::Assistant,
            MessageSource::Human,
            ts(10, 0, 0) + i as i64 * 1000 + 500,
            &format!("{tag} answer {i}\nsecond line of answer {i}"),
        ));
    }
    out
}

/// Split view with two idle background sessions holding long transcripts. The current session
/// stays foreground (hidden), so it is not tiled.
fn watch_app() -> App {
    let mut app = test_app(false);
    app.toggle_watch();
    let mut live = vec![app.views[TEST_SESSION].info.clone()];
    for (i, (id, name)) in [("ses_w1", "alpha"), ("ses_w2", "beta")]
        .into_iter()
        .enumerate()
    {
        let mut inf = bg_session(id, name, SessionStatus::Idle, 0);
        inf.created_at = 10 + i as i64;
        let mut v = SessionView::new(inf.clone(), false);
        let msgs = long_transcript(name, 30);
        let total = msgs.len() as u64;
        v.load_snapshot(SessionSnapshot {
            info: inf.clone(),
            messages: msgs,
            more_before: false,
            total_messages: total,
            start_seq: 0,
            layout_metas: vec![],
            pending_permissions: vec![],
            pending_questions: vec![],
            queued: vec![],
        });
        app.views.insert(id.into(), v);
        live.push(inf);
    }
    app.live = live;
    app.screen = ratatui::layout::Rect::new(0, 0, 120, 30);
    app
}

fn tile_rect(app: &App, sid: &str) -> ratatui::layout::Rect {
    app.watch
        .tiles
        .iter()
        .find(|t| t.0 == sid)
        .map(|t| t.1)
        .expect("tile")
}

#[test]
fn split_tile_wheel_scrolls_tile_under_pointer() {
    use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};
    let mut app = watch_app();
    let (_, s) = render(&mut app, 120, 30);
    assert!(app.watch.focus.is_none());
    assert!(
        s.contains("alpha answer 29") && s.contains("beta answer 29"),
        "tiles start at the bottom:\n{s}"
    );
    let (r1, r2) = (tile_rect(&app, "ses_w1"), tile_rect(&app, "ses_w2"));
    let top2 = app.views["ses_w2"].chat.top();
    let top1 = app.views["ses_w1"].chat.top();
    // Wheel over the unfocused second tile scrolls only that tile, and doesn't focus it.
    for _ in 0..4 {
        crate::input::handle(
            &mut app,
            mouse(MouseEventKind::ScrollUp, r2.x + 5, r2.y + 3),
        );
    }
    assert!(app.watch.focus.is_none());
    assert_eq!(app.views["ses_w2"].chat.top(), top2 - 12);
    assert!(!app.views["ses_w2"].chat.follow);
    assert_eq!(app.views["ses_w1"].chat.top(), top1, "other tile untouched");
    let (term, s) = render(&mut app, 120, 30);
    assert!(
        s.contains("alpha answer 29") && !s.contains("beta answer 29"),
        "{s}"
    );
    // Scroll thumb drawn on the scrolled tile's right border only.
    let buf = term.backend().buffer();
    let thumb = |r: ratatui::layout::Rect| {
        (r.y + 1..r.y + r.height - 1).any(|y| buf[(r.x + r.width - 1, y)].symbol() == "┃")
    };
    assert!(thumb(r2) && !thumb(r1));
    // New content while scrolled up: position kept, "↓ N new lines" pill appears.
    let top = app.views["ses_w2"].chat.top();
    let v = app.views.get_mut("ses_w2").unwrap();
    v.apply(SessionEvent::Message {
        message: msg(
            "late",
            Role::Assistant,
            MessageSource::Human,
            ts(11, 0, 0),
            "late arrival",
        ),
    });
    let (_, s) = render(&mut app, 120, 30);
    assert_eq!(app.views["ses_w2"].chat.top(), top, "position kept");
    assert!(s.contains("new lines"), "{s}");
    assert!(!s.contains("late arrival"));
    // Click on the pill: back to the bottom (follow again); click doesn't need focus first.
    let p = app.views["ses_w2"].chat.pill.expect("pill rect");
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), p.x + 1, p.y),
    );
    let (_, s) = render(&mut app, 120, 30);
    assert!(app.views["ses_w2"].chat.follow);
    assert!(
        s.contains("late arrival") && !s.contains("new lines"),
        "{s}"
    );
    // Streaming at the bottom keeps following.
    let v = app.views.get_mut("ses_w2").unwrap();
    v.apply(SessionEvent::Message {
        message: msg(
            "later",
            Role::Assistant,
            MessageSource::Human,
            ts(11, 1, 0),
            "even later",
        ),
    });
    let (_, s) = render(&mut app, 120, 30);
    assert!(s.contains("even later"));
    // Focused tile: PgUp / shift+↑ scroll it, End returns to the bottom.
    app.watch.focus = Some("ses_w1".into());
    render(&mut app, 120, 30);
    let bottom = app.views["ses_w1"].chat.top();
    press(&mut app, KeyCode::PageUp);
    render(&mut app, 120, 30);
    assert!(app.views["ses_w1"].chat.top() < bottom);
    let t = app.views["ses_w1"].chat.top();
    press_with(&mut app, KeyCode::Up, KeyModifiers::SHIFT);
    assert_eq!(app.views["ses_w1"].chat.top(), t - 1);
    press(&mut app, KeyCode::End);
    render(&mut app, 120, 30);
    assert_eq!(app.views["ses_w1"].chat.top(), bottom);
    assert!(app.views["ses_w1"].chat.follow);
    // Scrolled to the very top: the first message of the session is reachable.
    press(&mut app, KeyCode::Home);
    let (_, s) = render(&mut app, 120, 30);
    assert!(s.contains("alpha question 0"), "{s}");
}

#[test]
fn split_view_no_default_focus_and_esc_unfocus() {
    use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};
    let mut app = watch_app();
    app.sync_watch();
    let (term, s) = render(&mut app, 120, 30);
    assert!(app.watch.focus.is_none(), "no focus by default");
    assert!(
        s.contains("click or tab to focus · f filter · wheel scrolls"),
        "{s}"
    );
    // No tile border in the focus colour.
    let buf = term.backend().buffer();
    let r = tile_rect(&app, "ses_w1");
    assert_ne!(buf[(r.x, r.y)].fg, crate::ui::theme::PRIMARY);
    // Typing / enter with nothing focused: no input, no prompt sent.
    app.pending.clear();
    type_text(&mut app, "hi");
    press(&mut app, KeyCode::Enter);
    assert!(app.watch.inputs.values().all(|e| e.is_empty()) && app.pending.is_empty());
    // Sessions appearing/disappearing don't auto-focus.
    app.sync_watch();
    assert!(app.watch.focus.is_none());
    // Tab focuses the first tile; esc with text clears it, the next esc unfocuses.
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.watch.focus.as_deref(), Some("ses_w1"));
    type_text(&mut app, "hi");
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.watch.focus.as_deref(), Some("ses_w1"));
    assert!(app.watch.inputs["ses_w1"].is_empty());
    press(&mut app, KeyCode::Esc);
    assert!(
        app.watch.focus.is_none(),
        "esc on idle empty tile unfocuses"
    );
    // Shift+tab from nothing: last tile. Busy tile: esc arms, esc aborts, focus kept.
    press_with(&mut app, KeyCode::BackTab, KeyModifiers::SHIFT);
    assert_eq!(app.watch.focus.as_deref(), Some("ses_w2"));
    app.views.get_mut("ses_w2").unwrap().info.status = SessionStatus::Busy;
    app.pending.clear();
    press(&mut app, KeyCode::Esc);
    assert!(app.esc_armed.is_some() && app.watch.focus.is_some());
    press(&mut app, KeyCode::Esc);
    assert_eq!(
        app.watch.focus.as_deref(),
        Some("ses_w2"),
        "abort keeps focus"
    );
    assert!(
        app.toasts
            .items
            .iter()
            .any(|t| t.text.contains("Interrupted"))
    );
    // Click focuses; click on the footer unfocuses.
    render(&mut app, 120, 30);
    let r = tile_rect(&app, "ses_w1");
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), r.x + 3, r.y + 3),
    );
    assert_eq!(app.watch.focus.as_deref(), Some("ses_w1"));
    let (_, s) = render(&mut app, 120, 30);
    assert!(s.contains("esc unfocus"), "{s}");
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), 10, 29),
    );
    assert!(app.watch.focus.is_none());
    // The focused session leaving the split view clears the focus.
    app.watch.focus = Some("ses_w1".into());
    app.live.retain(|s| s.id != "ses_w1");
    app.sync_watch();
    assert!(app.watch.focus.is_none());
}

#[test]
fn split_view_click_focused_tile_deselects_and_double_click_zooms() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = watch_app();
    app.sync_watch();
    render(&mut app, 120, 30);
    let r = tile_rect(&app, "ses_w1");
    // First click focuses the tile.
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), r.x + 3, r.y + 3),
    );
    assert_eq!(app.watch.focus.as_deref(), Some("ses_w1"));
    // Clicking inside the focused tile's body keeps focus.
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), r.x + 6, r.y + 4),
    );
    assert_eq!(app.watch.focus.as_deref(), Some("ses_w1"));
    // Single-clicking the tile's border/header unfocuses it.
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), r.x + 4, r.y),
    );
    assert!(app.watch.focus.is_none());
    // Double-clicking a tile still zooms into it.
    let click = |app: &mut App, x, y| {
        crate::input::handle(app, mouse(MouseEventKind::Down(MouseButton::Left), x, y));
    };
    click(&mut app, r.x + 3, r.y + 3);
    assert_eq!(app.watch.focus.as_deref(), Some("ses_w1"));
    click(&mut app, r.x + 3, r.y + 3);
    assert_eq!(app.mode, Mode::Chat);
    assert_eq!(app.current.as_deref(), Some("ses_w1"));
}

#[test]
fn split_view_root_task_chip_and_border_style() {
    let mut app = test_app(false);
    app.mode = Mode::Watch;
    let mut r1 = bg_session("ses_r1", "alpha-root", SessionStatus::Idle, 1);
    r1.created_at = 1;
    let mut u1 = bg_session("ses_u1", "user-tile", SessionStatus::Idle, 0);
    u1.created_at = 2;
    let mut c1 = bg_session("ses_c1", "child-1", SessionStatus::Idle, 0);
    c1.parent = Some("ses_r1".into());
    c1.created_at = 3;
    let mut gc1 = bg_session("ses_gc1", "grand-1", SessionStatus::Busy, 0);
    gc1.parent = Some("ses_c1".into());
    gc1.created_at = 4;
    app.current = Some("ses_r1".into());
    app.live = vec![r1, u1.clone(), c1.clone(), gc1.clone()];
    app.views
        .insert("ses_u1".into(), SessionView::new(u1, false));
    app.views
        .insert("ses_c1".into(), SessionView::new(c1, false));
    app.views
        .insert("ses_gc1".into(), SessionView::new(gc1, false));
    // Focus gc1 to confirm focus colour wins over root-task border colour.
    app.watch.focus = Some("ses_gc1".into());
    let (term, s) = render(&mut app, 140, 36);
    let buf = term.backend().buffer();
    let ru = tile_rect(&app, "ses_u1");
    let rc = tile_rect(&app, "ses_c1");
    let rg = tile_rect(&app, "ses_gc1");
    // All tiles use solid rounded borders; user-created tile has no ⟲ chip.
    assert_eq!(buf[(ru.x, ru.y)].symbol(), "╭");
    assert_eq!(buf[(rc.x, rc.y)].symbol(), "╭");
    assert_eq!(buf[(rg.x, rg.y)].symbol(), "╭");
    assert!(!s.contains("user-tile ⟲"), "{s}");
    // Both child and grandchild resolve to alpha-root; unfocused child gets root_task_color, focused gets PRIMARY.
    assert!(s.contains("child-1 ⟲ alpha-root"), "{s}");
    assert!(s.contains("grand-1 ⟲ alpha-root"), "{s}");
    assert_eq!(buf[(ru.x, ru.y)].fg, crate::ui::theme::BORDER);
    assert_eq!(
        buf[(rc.x, rc.y)].fg,
        crate::ui::theme::root_task_color("ses_r1")
    );
    assert_eq!(buf[(rg.x, rg.y)].fg, crate::ui::theme::PRIMARY);
}

#[test]
fn sidebar_scrolls_and_clicks_map_after_scroll() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = sidebar_app();
    for i in 0..12 {
        let mut s = bg_session(
            &format!("ses_x{i}"),
            &format!("extra-{i}"),
            SessionStatus::Idle,
            0,
        );
        s.created_at = 100 + i;
        app.live.push(s);
    }
    app.screen = ratatui::layout::Rect::new(0, 0, 160, 24);
    let (_, _) = render(&mut app, 160, 24);
    assert!(
        app.sidebar_max_scroll > 0,
        "content taller than the sidebar"
    );
    let chat_top = app.views[TEST_SESSION].chat.top();
    // Wheel over the sidebar scrolls the sidebar, not the chat (also on the separator column).
    for _ in 0..50 {
        crate::input::handle(&mut app, mouse(MouseEventKind::ScrollDown, 150, 10));
    }
    assert_eq!(
        app.sidebar_scroll, app.sidebar_max_scroll,
        "clamped at the end"
    );
    crate::input::handle(
        &mut app,
        mouse(
            MouseEventKind::ScrollUp,
            160 - crate::render::SIDEBAR_W - 1,
            10,
        ),
    );
    crate::input::handle(
        &mut app,
        mouse(
            MouseEventKind::ScrollDown,
            160 - crate::render::SIDEBAR_W - 1,
            10,
        ),
    );
    assert_eq!(
        app.views[TEST_SESSION].chat.top(),
        chat_top,
        "chat not scrolled"
    );
    let (term, s) = render(&mut app, 160, 24);
    println!("{s}");
    let sb = sidebar_text(&term, 160);
    // The end of the list (last live session) is reachable.
    let y = pos(&sb, "extra-11").expect("last session visible after scrolling") as u16;
    assert!(
        app.sidebar_rows
            .iter()
            .any(|(ry, id)| *ry == y && id == "ses_x11")
    );
    // Scrollbar thumb in the rightmost column.
    let buf = term.backend().buffer();
    assert!((0..24).any(|y| buf[(159, y)].symbol() == "┃"));
    // Click on the row as drawn after scrolling: switches to that session.
    app.pending.clear();
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), 150, y),
    );
    assert_eq!(app.current.as_deref(), Some("ses_x11"));
    // Back at the top: session info first again.
    let mut app = sidebar_app();
    app.screen = ratatui::layout::Rect::new(0, 0, 160, 24);
    render(&mut app, 160, 24);
    crate::input::handle(&mut app, mouse(MouseEventKind::ScrollDown, 150, 10));
    crate::input::handle(&mut app, mouse(MouseEventKind::ScrollUp, 150, 10));
    let (term, _) = render(&mut app, 160, 24);
    assert!(sidebar_text(&term, 160)[1].contains("Fix the parser"));
}

#[test]
fn expanding_tool_keeps_header_row() {
    // Like the screenshot: `▸ Thought` directly followed by tool rows.
    let mut m = transcript(false)[1].clone();
    m.parts.remove(1); // text part between reasoning and tools
    let row_text = |r: &crate::ui::Row| {
        r.line
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect::<String>()
    };
    let find = |rows: &[crate::ui::Row], pat: &str| {
        rows.iter()
            .position(|r| row_text(r).contains(pat))
            .expect(pat)
    };
    let render_with = |toggled: &[usize], details: bool| {
        let set: std::collections::BTreeSet<usize> = toggled.iter().copied().collect();
        let mut cx = ctx(details);
        cx.toggled = Some(&set);
        crate::ui::message::render(&m, &cx, 100)
    };
    let collapsed = render_with(&[], false);
    let bash = find(&collapsed, "bash  Run the unit tests");
    assert!(
        row_text(&collapsed[bash - 1]).contains("Thought"),
        "collapsed rows stack"
    );
    // Expand the bash tool (part 1): its header row doesn't move; no blank line above it.
    let expanded = render_with(&[1], false);
    assert!(expanded.len() > collapsed.len());
    assert_eq!(
        find(&expanded, "bash  Run the unit tests"),
        bash,
        "header row index unchanged"
    );
    assert!(row_text(&expanded[bash - 1]).contains("Thought"));
    assert!(
        row_text(&expanded[bash + 1]).contains("╭─ input"),
        "box right under the header"
    );
    // The next tool row is separated from the box by one blank line.
    let edit = find(&expanded, "edit  Fix the off-by-one");
    assert!(
        row_text(&expanded[edit - 1]).trim().is_empty()
            && row_text(&expanded[edit - 2]).contains('╰')
    );
    // Expanding the reasoning keeps its header (row 0) and pushes only what follows.
    let r_exp = render_with(&[0], false);
    assert!(row_text(&r_exp[0]).contains("Thought") && row_text(&collapsed[0]).contains("Thought"));
    // All tools expanded by default (/details): the first tool header is still in the same row.
    assert_eq!(
        find(&render_with(&[], true), "bash  Run the unit tests"),
        bash
    );
}

fn respond(app: &mut App, pred: impl Fn(&crate::app::Pending) -> bool, result: serde_json::Value) {
    let id = *app
        .pending
        .iter()
        .find(|(_, p)| pred(p))
        .map(|(k, _)| k)
        .expect("pending request");
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id,
        ok: true,
        result,
        error: None,
        code: None,
    }));
}

#[test]
fn current_session_stays_foreground_in_split_view() {
    use crate::app::Pending;
    let mut app = test_app(false);
    app.live = vec![app.views[TEST_SESSION].info.clone()];
    app.toggle_watch();
    assert_eq!(app.mode, Mode::Watch);
    let v = &app.views[TEST_SESSION];
    assert!(v.foreground);
    assert_eq!(app.live[0].attached, 1);
    assert!(app.tile_ids().is_empty());
    let (_, empty_screen) = render(&mut app, 100, 30);
    assert!(
        empty_screen.contains("No background sessions"),
        "{empty_screen}"
    );
    assert!(
        empty_screen.contains("ctrl+x w back to chat"),
        "{empty_screen}"
    );
    assert!(!app.attached_elsewhere(TEST_SESSION));
    app.sync_watch();
    assert!(app.views.contains_key(TEST_SESSION), "current view kept");
    app.pending.clear();
    app.toggle_watch();
    assert_eq!(app.mode, Mode::Chat);
    assert!(app.views[TEST_SESSION].foreground);
    assert_eq!(app.live[0].attached, 1);
    assert!(
        !app.pending
            .values()
            .any(|p| matches!(p, Pending::Attach { sid, fg: true } if sid == TEST_SESSION))
    );
    // An in-flight foreground attach answered after entering split view keeps it foreground.
    app.attach_fg(TEST_SESSION);
    app.toggle_watch();
    assert_eq!(app.mode, Mode::Watch);
    let snap = SessionSnapshot {
        info: app.views[TEST_SESSION].info.clone(),
        messages: vec![],
        more_before: false,
        total_messages: 0,
        start_seq: 0,
        layout_metas: vec![],
        pending_permissions: vec![],
        pending_questions: vec![],
        queued: vec![],
    };
    respond(
        &mut app,
        |p| matches!(p, Pending::Attach { sid, fg: true } if sid == TEST_SESSION),
        json!({ "snapshot": snap }),
    );
    assert!(app.views[TEST_SESSION].foreground);
}

#[test]
fn close_current_deselects_without_auto_switch_or_quit() {
    use crate::app::Pending;
    let enter = |app: &mut App| press(app, KeyCode::Enter);
    // Other live sessions present: closing current deselects (no auto-switch, no new session, no quit).
    let mut app = test_app(false);
    let cur = app.views[TEST_SESSION].info.clone();
    let mut newer = bg_session("ses_new", "newer", SessionStatus::Idle, 0);
    newer.created_at = 50;
    let mut older = bg_session("ses_old", "older", SessionStatus::Idle, 0);
    older.created_at = 10;
    app.live = vec![cur.clone(), newer, older];
    type_text(&mut app, "/close");
    enter(&mut app);
    assert!(
        app.pending
            .values()
            .any(|p| matches!(p, Pending::Closed { .. }))
    );
    assert!(app.current.is_none());
    assert!(!app.views.contains_key(TEST_SESSION));
    assert!(
        !app.pending
            .values()
            .any(|p| matches!(p, Pending::Created { .. }))
    );
    assert!(!app.quit);

    // Last session: deselects and stays running after Close confirms (never auto-quits).
    let mut app = test_app(false);
    app.live = vec![cur.clone()];
    type_text(&mut app, "/close");
    enter(&mut app);
    assert!(app.current.is_none());
    assert!(!app.quit);
    respond(&mut app, |p| matches!(p, Pending::Closed { .. }), json!({}));
    assert!(!app.quit);
    assert!(app.current.is_none());

    // A failed close or a dropped connection does not quit either.
    let mut app = test_app(false);
    app.run_command("close", "");
    fail_pending(&mut app, |p| matches!(p, Pending::Closed { .. }), "boom");
    assert!(!app.quit && app.current.is_none());
    let mut app = test_app(false);
    app.run_command("close", "");
    app.on_net(crate::net::NetEvent::Disconnected("gone".into()));
    assert!(!app.quit && app.current.is_none());

    // Session already gone on the server: deselects right away without Close request.
    let mut app = test_app(false);
    app.views.get_mut(TEST_SESSION).unwrap().closed = true;
    app.run_command("close", "");
    assert!(app.current.is_none() && !app.quit);
    assert!(
        !app.pending
            .values()
            .any(|p| matches!(p, Pending::Closed { .. }))
    );
}

#[test]
fn cd_command_changes_session_directory() {
    use crate::app::Pending;
    let mut app = test_app(false);
    app.run_command("cd", "");
    assert!(
        matches!(app.dialog, Some(crate::ui::dialog::Dialog::List(ref d)) if d.kind == crate::ui::dialog::Kind::Cd)
    );
    app.dialog = None;
    app.pending.clear();
    type_text(&mut app, "/cwd \"../other dir\"");
    press(&mut app, KeyCode::Enter);
    assert!(
        app.pending
            .values()
            .any(|p| matches!(p, Pending::Cwd { sid } if sid == TEST_SESSION))
    );
    let mut info = app.views[TEST_SESSION].info.clone();
    info.cwd = "/home/u/other dir".into();
    respond(
        &mut app,
        |p| matches!(p, Pending::Cwd { .. }),
        json!({ "session": info }),
    );
    assert_eq!(app.views[TEST_SESSION].info.cwd, "/home/u/other dir");
    assert_eq!(app.cwd, "/home/u/other dir", "new sessions follow");
    assert!(
        app.toasts
            .items
            .iter()
            .any(|t| t.text.contains("other dir"))
    );
    // Errors are shown, nothing changes.
    type_text(&mut app, "/cd /nope");
    press(&mut app, KeyCode::Enter);
    fail_pending(
        &mut app,
        |p| matches!(p, Pending::Cwd { .. }),
        "Directory does not exist: /nope",
    );
    assert!(
        app.toasts
            .items
            .iter()
            .any(|t| t.text.contains("Directory does not exist"))
    );
    assert_eq!(app.cwd, "/home/u/other dir");
}

#[test]
fn split_tile_commands_act_on_that_session() {
    use crate::app::Pending;
    let mut app = test_app(false);
    app.mode = Mode::Watch;
    let bg = bg_session("ses_bg", "worker-1", SessionStatus::Idle, 0);
    app.views
        .insert("ses_bg".into(), SessionView::new(bg.clone(), false));
    app.live = vec![app.views[TEST_SESSION].info.clone(), bg];
    app.watch.focus = Some("ses_bg".into());
    let typed = |app: &mut App, t: &str| {
        for c in t.chars() {
            press(app, KeyCode::Char(c));
        }
        press(app, KeyCode::Enter);
    };
    typed(&mut app, "/cd sub");
    assert!(
        app.pending
            .values()
            .any(|p| matches!(p, Pending::Cwd { sid } if sid == "ses_bg"))
    );
    assert!(
        !app.pending
            .values()
            .any(|p| matches!(p, Pending::Prompt { .. })),
        "not sent to the model"
    );
    typed(&mut app, "/compact");
    assert!(app.watch.inputs["ses_bg"].text.is_empty());
    assert!(
        app.pending
            .values()
            .any(|p| matches!(p, Pending::Simple { .. }))
    );
    typed(&mut app, "/close");
    assert!(
        app.pending
            .values()
            .any(|p| matches!(p, Pending::Closed { name } if name == "worker-1"))
    );
    assert!(!app.views.contains_key("ses_bg"));
    assert_eq!(app.current.as_deref(), Some(TEST_SESSION));
    assert_eq!(app.mode, Mode::Watch);
    assert!(!app.quit);
}

/// An app whose live sessions all have todo lists of different shapes.
fn live_sessions_app() -> App {
    let todo = |c: &str, st: TodoStatus| Todo {
        content: c.into(),
        status: st,
    };
    let mut app = test_app(false);
    let mut cur = app.views[TEST_SESSION].info.clone();
    cur.todos = vec![
        todo("a", TodoStatus::Completed),
        todo("b", TodoStatus::InProgress),
        todo("c", TodoStatus::Pending),
        todo("x", TodoStatus::Pending),
        todo("y", TodoStatus::Pending),
    ];
    app.views.get_mut(TEST_SESSION).unwrap().info.todos = cur.todos.clone();
    let mut busy = bg_session("ses_busy", "worker-1", SessionStatus::Running, 0);
    busy.busy_since = Some(ref_proto::now_ms() - 65_000);
    busy.todos = vec![
        todo("a", TodoStatus::Completed),
        todo("b", TodoStatus::Completed),
        todo("c", TodoStatus::Completed),
    ];
    let mut pending = bg_session("ses_pend", "pender", SessionStatus::Idle, 0);
    pending.todos = vec![
        todo("a", TodoStatus::Pending),
        todo("b", TodoStatus::Pending),
        todo("c", TodoStatus::Cancelled),
    ];
    let mut done = bg_session("ses_done", "reviewer", SessionStatus::Idle, 0);
    done.todos = vec![
        todo("a", TodoStatus::Completed),
        todo("b", TodoStatus::Cancelled),
    ];
    let mut gone = bg_session("ses_cxl", "cancelled-only", SessionStatus::Idle, 0);
    gone.todos = vec![todo("a", TodoStatus::Cancelled)];
    let mut long = bg_session(
        "ses_long",
        "解析器重构和一个很长很长的名字",
        SessionStatus::Running,
        0,
    );
    long.busy_since = Some(ref_proto::now_ms() - 723_000);
    long.todos = vec![
        todo("a", TodoStatus::Completed),
        todo("b", TodoStatus::InProgress),
        todo("c", TodoStatus::Pending),
        todo("d", TodoStatus::Pending),
    ];
    app.live = vec![
        cur,
        busy,
        pending,
        done,
        gone,
        bg_session("ses_none", "scratch", SessionStatus::Idle, 0),
        long,
    ];
    app
}

/// All sidebar rows, and those from the live sessions list on.
fn live_sessions_sidebar(app: &mut App) -> (Vec<String>, Vec<String>) {
    let (term, _) = render(app, 150, 50);
    let all = sidebar_text(&term, 150);
    let rows = all
        .iter()
        .skip_while(|l| !l.contains("Live sessions"))
        .cloned()
        .collect();
    (all, rows)
}

/// The trimmed live sessions row containing `name`.
fn live_row(rows: &[String], name: &str) -> String {
    rows.iter()
        .find(|l| l.contains(name))
        .unwrap_or_else(|| panic!("{name}: {rows:#?}"))
        .trim_end()
        .to_string()
}

#[test]
fn sidebar_live_sessions_show_todo_count_and_busy_time() {
    let (_, rows) = live_sessions_sidebar(&mut live_sessions_app());
    assert!(
        live_row(&rows, "ref-a1b2").ends_with("1/5"),
        "current session too: {:?}",
        live_row(&rows, "ref-a1b2")
    );
    let w = live_row(&rows, "worker-1");
    assert!(
        w.ends_with("✓3/3 1m 05s") || w.ends_with("✓3/3 1m 06s"),
        "{w:?}"
    );
    assert!(
        live_row(&rows, "pender").ends_with(" 0/2"),
        "cancelled not counted"
    );
    assert!(live_row(&rows, "reviewer").ends_with("✓1/1"));
}

#[test]
fn sidebar_live_sessions_without_countable_todos_have_no_badge() {
    let (_, rows) = live_sessions_sidebar(&mut live_sessions_app());
    assert!(
        live_row(&rows, "cancelled-only").ends_with("bg"),
        "no countable items: no badge"
    );
    assert!(live_row(&rows, "scratch").ends_with("bg"));
}

#[test]
fn sidebar_live_sessions_truncate_long_names_and_fit_the_width() {
    let (all, rows) = live_sessions_sidebar(&mut live_sessions_app());
    let l = live_row(&rows, "解析器");
    assert!(
        l.contains('…') && (l.ends_with("1/4 12m 03s") || l.ends_with("1/4 12m 04s")),
        "name truncated, count and time whole: {l:?}"
    );
    for r in &all {
        assert!(
            crate::text::width(r.trim_end()) <= crate::render::SIDEBAR_W as usize,
            "{r:?}"
        );
    }
}

#[test]
fn queue_event_shows_messages_from_other_sessions() {
    use crate::app::Pending;
    let mut app = test_app(true);
    let user = |id: &str, text: &str, source: MessageSource| msg(id, Role::User, source, 1, text);
    let peer = MessageSource::Peer {
        from_id: "ses_w".into(),
        from_name: "worker-1".into(),
        msg_id: "pm1".into(),
    };
    let ev = |app: &mut App, messages: Vec<Message>| {
        app.on_net(crate::net::NetEvent::Frame(ServerFrame::Event {
            session_id: TEST_SESSION.into(),
            event: SessionEvent::Queue { messages },
        }))
    };

    // Our prompt is queued; the Queue event arrives before the Prompt response.
    type_text(&mut app, "my follow-up");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.views[TEST_SESSION].queued.len(), 1);
    ev(
        &mut app,
        vec![
            user("pq1", "please check the parser", peer.clone()),
            user("q1", "my follow-up", MessageSource::Human),
        ],
    );
    let q = &app.views[TEST_SESSION].queued;
    assert_eq!(q.len(), 2, "no duplicate of our own row");
    assert_eq!(
        (q[0].from.as_deref(), q[0].id.as_deref()),
        (Some("worker-1"), Some("pq1"))
    );
    assert_eq!(
        (q[1].from.as_deref(), q[1].id.as_deref()),
        (None, Some("q1"))
    );
    respond(
        &mut app,
        |p| matches!(p, Pending::Prompt { .. }),
        json!({"queued": true, "message_id": "q1"}),
    );
    assert_eq!(app.views[TEST_SESSION].queued.len(), 2);

    // Rendering: the sender is shown and only our own prompt can be removed.
    let (_, s) = render(&mut app, 100, 30);
    let peer_row = s
        .lines()
        .find(|l| l.contains("please check the parser"))
        .unwrap();
    assert!(
        peer_row.contains("queued from worker-1") && !peer_row.contains("✕ remove"),
        "{peer_row}"
    );
    assert!(
        s.lines()
            .find(|l| l.contains("my follow-up"))
            .unwrap()
            .contains("✕ remove")
    );
    let pkey = app.views[TEST_SESSION].queued[0].key;
    app.unqueue(TEST_SESSION, pkey);
    assert!(
        !app.pending
            .values()
            .any(|p| matches!(p, Pending::Unqueue { .. }))
    );

    // Delivered: the server announces the empty queue.
    ev(&mut app, vec![]);
    assert!(app.views[TEST_SESSION].queued.is_empty());
}

fn history_app_with_count(
    old_pairs: usize,
    newest_pairs: usize,
) -> (App, Vec<Message>, Vec<Message>) {
    let mut app = test_app(false);
    let all = long_transcript("h", old_pairs + newest_pairs);
    let old_count = old_pairs * 2;
    let total = all.len() as u64;
    let (old, newest) = all.split_at(old_count);
    let metas: Vec<_> = old
        .iter()
        .enumerate()
        .map(|(i, m)| ref_proto::compute_layout_meta(i as u64, m))
        .collect();
    let inf = app.views[TEST_SESSION].info.clone();
    app.views
        .get_mut(TEST_SESSION)
        .unwrap()
        .load_snapshot(SessionSnapshot {
            info: inf,
            messages: newest.to_vec(),
            more_before: !old.is_empty(),
            total_messages: total,
            start_seq: old_count as u64,
            layout_metas: metas,
            pending_permissions: vec![],
            pending_questions: vec![],
            queued: vec![],
        });
    (app, old.to_vec(), newest.to_vec())
}

/// A session with the newest 40 of 120 messages loaded and placeholders before them.
fn history_app() -> (App, Vec<Message>, Vec<Message>) {
    history_app_with_count(40, 20)
}

/// The request id and `[start_seq, end_seq)` of the pending range request, if any.
fn range_request(app: &App) -> Option<(u64, usize, usize)> {
    app.pending.iter().find_map(|(id, p)| match p {
        crate::app::Pending::MessagesRange {
            start_seq, end_seq, ..
        } => Some((*id, *start_seq, *end_seq)),
        _ => None,
    })
}

/// Scrolls to the top of the transcript and asks the app to load missing visible ranges.
fn scroll_to_top_and_load_older(app: &mut App) {
    render(app, 100, 30);
    app.views
        .get_mut(TEST_SESSION)
        .unwrap()
        .chat
        .scroll_by(-1_000_000, false);
    render(app, 100, 30);
    app.load_older();
}

fn respond_to_range(app: &mut App, id: u64, result: serde_json::Value) {
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id,
        ok: true,
        result,
        error: None,
        code: None,
    }));
}

#[test]
fn history_range_is_requested_once_when_scrolled_to_placeholders() {
    use crate::app::Pending;
    let (mut app, ..) = history_app();
    render(&mut app, 100, 30);
    app.load_older();
    assert!(
        range_request(&app).is_none(),
        "at the bottom nothing is requested"
    );

    // Scrolled to the top: the placeholder range [0, 80) is requested once.
    scroll_to_top_and_load_older(&mut app);
    let (_, start_seq, end_seq) = range_request(&app).expect("range requested");
    assert_eq!((start_seq, end_seq), (0, 80));
    app.load_older();
    assert_eq!(
        app.pending
            .values()
            .filter(|p| matches!(p, Pending::MessagesRange { .. }))
            .count(),
        1
    );
}

#[test]
fn history_range_materializes_slots_and_preserves_anchor() {
    let (mut app, old, _) = history_app_with_count(300, 20);
    let sid = TEST_SESSION;
    scroll_to_top_and_load_older(&mut app);
    let (id, start_seq, end_seq) = range_request(&app).expect("range requested");
    assert_eq!((start_seq, end_seq), (0, 500));

    respond_to_range(
        &mut app,
        id,
        json!({
            "messages": old[0..500].to_vec(),
            "more_before": false,
            "start_seq": 0,
            "end_seq": 500,
        }),
    );
    assert_eq!(app.views[sid].slots.len(), 640);
    assert_eq!(app.views[sid].resident_count(), 540);
    assert_eq!(app.views[sid].slots[0].id(), "hu0");
    assert!(!app.views[sid].slots[500].is_materialized());

    // Rendering materializes the top and leaves no pending range while looking at 0..100.
    let (_, s) = render(&mut app, 100, 30);
    assert!(s.contains("h question 0"), "{s}");
    app.load_older();
    assert!(range_request(&app).is_none());
}

#[test]
fn history_range_answered_after_a_reset_is_dropped() {
    let (mut app, old, newest) = history_app();
    let sid = TEST_SESSION;
    scroll_to_top_and_load_older(&mut app);
    let (id, ..) = range_request(&app).expect("range requested");

    // A reset in between makes the answer stale: it is dropped.
    let total = newest.len() as u64;
    let v = app.views.get_mut(sid).unwrap();
    v.apply(SessionEvent::Reset {
        messages: newest,
        more_before: false,
        total_messages: total,
        start_seq: 0,
        layout_metas: vec![],
    });
    respond_to_range(
        &mut app,
        id,
        json!({
            "messages": old[..40].to_vec(),
            "more_before": false,
            "start_seq": 0,
            "end_seq": 40,
        }),
    );
    assert_eq!(app.views[sid].slots.len(), 40);
    assert_eq!(app.views[sid].resident_count(), 40);
}

#[test]
fn history_range_error_halts_further_requests() {
    let (mut app, ..) = history_app();
    scroll_to_top_and_load_older(&mut app);
    let (id, ..) = range_request(&app).expect("range requested");

    // An error halts further requests until next snapshot/reset (no retry loop).
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id,
        ok: false,
        result: json!(null),
        error: Some("gone".into()),
        code: None,
    }));
    render(&mut app, 100, 30);
    app.load_older();
    assert!(range_request(&app).is_none());
    assert!(app.views[TEST_SESSION].range_halted);
}

const EDITOR_TEXT: &str = "hello brave new world";

/// An app with `EDITOR_TEXT` in the editor, drawn; returns it with the text's row and a
/// function giving the screen column of a text column.
fn editor_mouse_app() -> (App, u16, impl Fn(u16) -> u16) {
    let mut app = test_app(false);
    type_text(&mut app, EDITOR_TEXT);
    let (term, screen) = render(&mut app, 100, 30);
    let y = screen
        .lines()
        .position(|l| l.contains(EDITOR_TEXT))
        .expect("editor row") as u16;
    let x0 = (0..100)
        .find(|&x| term.backend().buffer()[(x, y)].symbol() == "h")
        .unwrap();
    (app, y, move |col| x0 + col)
}

fn send_mouse(app: &mut App, kind: crossterm::event::MouseEventKind, x: u16, y: u16) {
    crate::input::handle(app, mouse(kind, x, y));
}

/// Presses at `from`, drags to `to` and releases there.
fn drag_select(app: &mut App, y: u16, from: u16, to: u16) {
    use crossterm::event::{MouseButton, MouseEventKind::*};
    send_mouse(app, Down(MouseButton::Left), from, y);
    send_mouse(app, Drag(MouseButton::Left), to, y);
    send_mouse(app, Up(MouseButton::Left), to, y);
}

#[test]
fn editor_mouse_drag_selects_forward_and_backward() {
    use crossterm::event::{MouseButton, MouseEventKind::*};
    let (mut app, y, at) = editor_mouse_app();

    // Press on "brave", drag to the end of "new": that range is selected.
    send_mouse(&mut app, Down(MouseButton::Left), at(6), y);
    assert_eq!(app.editor.selected_text(), None);
    send_mouse(&mut app, Drag(MouseButton::Left), at(10), y);
    send_mouse(&mut app, Drag(MouseButton::Left), at(15), y);
    send_mouse(&mut app, Up(MouseButton::Left), at(15), y);
    assert_eq!(app.editor.selected_text().as_deref(), Some("brave new"));
    assert_eq!(app.editor.cursor, 15);

    // Dragging backwards works too.
    drag_select(&mut app, y, at(11), at(0));
    assert_eq!(app.editor.selected_text().as_deref(), Some("hello brave"));
}

#[test]
fn editor_right_click_copies_the_selection() {
    use crossterm::event::{MouseButton, MouseEventKind::Down};
    let (mut app, y, at) = editor_mouse_app();
    drag_select(&mut app, y, at(11), at(0));

    send_mouse(&mut app, Down(MouseButton::Right), at(3), y);
    assert!(
        app.toasts
            .items
            .iter()
            .any(|t| t.text == "Copied selection (11 chars)")
    );
    assert_eq!(app.editor.selected_text(), None);
}

#[test]
fn editor_double_click_selects_word_triple_click_line_plain_click_clears() {
    use crossterm::event::{MouseButton, MouseEventKind::*};
    let (mut app, y, at) = editor_mouse_app();

    // Double click: the word; triple click: the line. A plain click clears.
    send_mouse(&mut app, Down(MouseButton::Left), at(17), y);
    send_mouse(&mut app, Up(MouseButton::Left), at(17), y);
    send_mouse(&mut app, Down(MouseButton::Left), at(17), y);
    assert_eq!(app.editor.selected_text().as_deref(), Some("world"));
    send_mouse(&mut app, Up(MouseButton::Left), at(17), y);
    send_mouse(&mut app, Down(MouseButton::Left), at(17), y);
    assert_eq!(app.editor.selected_text().as_deref(), Some(EDITOR_TEXT));
    send_mouse(&mut app, Up(MouseButton::Left), at(17), y);
    app.last_click = None;
    send_mouse(&mut app, Down(MouseButton::Left), at(2), y);
    send_mouse(&mut app, Up(MouseButton::Left), at(2), y);
    assert_eq!(app.editor.selected_text(), None);
    assert_eq!(app.editor.cursor, 2);
}

#[test]
fn typing_replaces_the_editor_mouse_selection() {
    let (mut app, y, at) = editor_mouse_app();
    drag_select(&mut app, y, at(0), at(5));
    type_text(&mut app, "bye");
    assert_eq!(app.editor.text, "bye brave new world");
}

// --- Hold: the transcript end does not move up while a turn runs ---

fn set_status(app: &mut App, sid: &str, status: SessionStatus) {
    let v = app.views.get_mut(sid).unwrap();
    let mut i = v.info.clone();
    i.status = status;
    v.set_info(i);
}

fn retrying() -> SessionStatus {
    SessionStatus::Retrying {
        attempt: 2,
        next_at: ref_proto::now_ms() + 60_000,
        error: "rate limited".into(),
    }
}

/// Screen row of the first line containing `needle`.
fn row_of(screen: &str, needle: &str) -> usize {
    screen
        .lines()
        .position(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("{needle:?} not on screen:\n{screen}"))
}

fn long_session_app(pairs: usize) -> App {
    let mut app = test_app(false);
    let msgs = long_transcript("a", pairs);
    let total = msgs.len() as u64;
    app.views
        .get_mut(TEST_SESSION)
        .unwrap()
        .apply(SessionEvent::Reset {
            more_before: false,
            messages: msgs,
            total_messages: total,
            start_seq: 0,
            layout_metas: vec![],
        });
    app
}

#[test]
fn retry_notice_disappearing_snaps_back_without_blank_hold_space() {
    let last = "second line of answer 29";
    let mut app = long_session_app(30);
    set_status(&mut app, TEST_SESSION, SessionStatus::Busy);
    let (_, s) = render(&mut app, 100, 30);
    let at_rest = row_of(&s, last);

    set_status(&mut app, TEST_SESSION, retrying());
    let (_, s) = render(&mut app, 100, 30);
    let with_notice = row_of(&s, last);
    assert!(with_notice < at_rest, "the notice pushes the transcript up");

    set_status(&mut app, TEST_SESSION, SessionStatus::Busy);
    let (_, s) = render(&mut app, 100, 30);
    assert_eq!(
        row_of(&s, last),
        at_rest,
        "no artificial hold space during busy turn\n{s}"
    );
}

#[test]
fn removing_unfinished_message_leaves_no_blank_hold_space() {
    let last_answer = "second line of answer 29";
    let mut app = long_session_app(30);
    set_status(&mut app, TEST_SESSION, SessionStatus::Busy);
    let (_, s0) = render(&mut app, 100, 30);
    let at_rest = row_of(&s0, last_answer);
    let streaming = msg(
        "streaming",
        Role::Assistant,
        MessageSource::Human,
        ts(10, 5, 0),
        &"partial output\n".repeat(8),
    );
    app.views
        .get_mut(TEST_SESSION)
        .unwrap()
        .apply(SessionEvent::Message { message: streaming });
    let (_, s) = render(&mut app, 100, 30);
    assert!(row_of(&s, last_answer) < at_rest);

    app.views
        .get_mut(TEST_SESSION)
        .unwrap()
        .apply(SessionEvent::Removed {
            message_id: "streaming".into(),
        });
    let (_, s) = render(&mut app, 100, 30);
    assert_eq!(row_of(&s, last_answer), at_rest, "no blank hold space\n{s}");
}

#[test]
fn split_tile_shrink_and_reflow_preserve_scroll_invariants() {
    let mut app = watch_app();
    set_status(&mut app, "ses_w2", SessionStatus::Busy);
    let (_, s) = render(&mut app, 120, 30);
    let at_rest = row_of(&s, "beta answer 29");

    let queued = crate::state::Queued {
        key: 1,
        id: None,
        text: "later".into(),
        images: vec![],
        removing: false,
        from: Some("alpha".into()),
    };
    app.views.get_mut("ses_w2").unwrap().queued.push(queued);
    let (_, s) = render(&mut app, 120, 30);
    assert!(row_of(&s, "beta answer 29") < at_rest);

    app.views.get_mut("ses_w2").unwrap().queued.clear();
    let (_, s) = render(&mut app, 120, 30);
    assert_eq!(
        row_of(&s, "beta answer 29"),
        at_rest,
        "no blank hold space in split tile"
    );
}

#[test]
fn reflow_preserves_reading_anchor_when_scrolled_up() {
    let mut app = long_session_app(30);
    let _ = render(&mut app, 120, 24);
    let chat = &mut app.views.get_mut(TEST_SESSION).unwrap().chat;
    chat.scroll_by(-40, false);
    assert!(!chat.follow);
    let (_, wide) = render(&mut app, 120, 24);
    let top_line = wide
        .lines()
        .map(str::trim)
        .find(|l| l.contains("a question ") || l.contains("answer "))
        .expect("visible message line")
        .to_string();
    let marker = if let Some(idx) = top_line.find("a question ") {
        top_line[idx..]
            .trim_end_matches(['│', '┃', ' '])
            .to_string()
    } else {
        top_line
            .split(':')
            .next()
            .unwrap_or(&top_line)
            .trim_end_matches(['│', '┃', ' '])
            .to_string()
    };
    let row_wide = row_of(&wide, &marker);
    assert!(
        row_wide <= 4,
        "marker near top in wide view: row={row_wide}\n{wide}"
    );

    // Narrowing wraps earlier messages into many more rows, yet keeps the same message anchored at top.
    let (_, narrow) = render(&mut app, 44, 24);
    assert!(!app.views[TEST_SESSION].chat.follow);
    let row_narrow = row_of(&narrow, &marker);
    assert!(
        row_narrow <= 4,
        "marker stays anchored near top after narrow reflow: row={row_narrow}\n{narrow}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn net_protocol_mismatch_stops_retrying() {
    use futures_util::{SinkExt, StreamExt};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio_tungstenite::tungstenite::Message as M;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let accepts = Arc::new(AtomicUsize::new(0));
    let accepts_srv = accepts.clone();
    tokio::spawn(async move {
        while let Ok((sock, _)) = listener.accept().await {
            accepts_srv.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut ws = tokio_tungstenite::accept_async(sock).await.unwrap();
                if let Some(Ok(M::Text(t))) = ws.next().await {
                    let f: ClientFrame = serde_json::from_str(t.as_str()).unwrap();
                    let reply = ServerFrame::Response {
                        id: f.id.unwrap_or(0),
                        ok: false,
                        result: json!(null),
                        error: Some("ref-server 0.2.1 speaks protocol 5, mismatch test".into()),
                        code: Some(ERR_PROTOCOL_MISMATCH.into()),
                    };
                    let _ = ws
                        .send(M::text(serde_json::to_string(&reply).unwrap()))
                        .await;
                }
            });
        }
    });

    let (out_tx, out_rx) = mpsc::unbounded_channel();
    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel();
    let target = ref_client::Target::Explicit {
        url: format!("ws://127.0.0.1:{port}/"),
        token: None,
    };
    let task = tokio::spawn(crate::net::run(target, "/my/cwd".into(), out_rx, ev_tx));
    let ev = tokio::time::timeout(std::time::Duration::from_secs(3), ev_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(&ev, crate::net::NetEvent::Refused(msg) if msg.contains("mismatch test")),
        "{ev:?}"
    );
    // Past RECONNECT_DELAY (1s): must not reconnect, and must keep the channel open.
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    assert_eq!(accepts.load(Ordering::SeqCst), 1);
    assert!(ev_rx.try_recv().is_err());

    drop(out_tx);
    tokio::time::timeout(std::time::Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn net_old_backend_emits_backend_too_old() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message as M;

    for (hello_result, expect_too_old) in [
        (
            json!({ "protocol": PROTOCOL_VERSION - 1 }),
            Some(PROTOCOL_VERSION - 1),
        ),
        (json!({}), Some(1)),
        (json!({ "protocol": PROTOCOL_VERSION }), None),
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(sock).await.unwrap();
            while let Some(Ok(M::Text(t))) = ws.next().await {
                let f: ClientFrame = serde_json::from_str(t.as_str()).unwrap();
                let reply = ServerFrame::Response {
                    id: f.id.unwrap_or(0),
                    ok: true,
                    result: hello_result.clone(),
                    error: None,
                    code: None,
                };
                let _ = ws
                    .send(M::text(serde_json::to_string(&reply).unwrap()))
                    .await;
                let toast = ServerFrame::Toast {
                    level: ToastLevel::Info,
                    text: "after".into(),
                };
                let _ = ws
                    .send(M::text(serde_json::to_string(&toast).unwrap()))
                    .await;
            }
        });
        let (out_tx, out_rx) = mpsc::unbounded_channel();
        let (ev_tx, mut ev_rx) = mpsc::unbounded_channel();
        let target = ref_client::Target::Explicit {
            url: format!("ws://127.0.0.1:{port}/"),
            token: None,
        };
        tokio::spawn(crate::net::run(target, "/my/cwd".into(), out_rx, ev_tx));
        let first = tokio::time::timeout(std::time::Duration::from_secs(3), ev_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(first, crate::net::NetEvent::Connected),
            "{first:?}"
        );
        let second = tokio::time::timeout(std::time::Duration::from_secs(3), ev_rx.recv())
            .await
            .unwrap()
            .unwrap();
        match expect_too_old {
            Some(p) => assert!(
                matches!(second, crate::net::NetEvent::BackendTooOld { protocol } if protocol == p),
                "{second:?}"
            ),
            None => assert!(
                matches!(
                    second,
                    crate::net::NetEvent::Frame(ServerFrame::Toast { .. })
                ),
                "{second:?}"
            ),
        }
        drop(out_tx);
    }
}

#[test]
fn protocol_refusal_and_old_backend_banners_render() {
    let mut app = test_app(false);
    app.on_net(crate::net::NetEvent::BackendTooOld {
        protocol: PROTOCOL_VERSION - 1,
    });
    let (_, s) = render(&mut app, 110, 30);
    let expected = format!(
        "The backend speaks protocol {} but this ref needs {}. Run ref-server --stop and start ref again.",
        PROTOCOL_VERSION - 1,
        PROTOCOL_VERSION
    );
    assert!(s.contains(&expected), "{s}");
    // Stays connected and unblocked.
    assert!(app.connected);

    // Refusal after a session was connected shows the message and drops the spinner.
    let refusal = "ref-server 0.2.1 speaks protocol 4, mismatch detail";
    app.on_net(crate::net::NetEvent::Refused(refusal.into()));
    let (_, s) = render(&mut app, 110, 30);
    assert!(s.contains(refusal), "{s}");
    assert!(!s.contains("reconnecting…"), "{s}");

    // Refusal on initial connect also shows the text without "connecting…".
    let (tx, _) = mpsc::unbounded_channel();
    let mut fresh = App::new(Client::new(tx), "/work".into(), InitPlan::default());
    fresh.on_net(crate::net::NetEvent::Refused(refusal.into()));
    let (_, s) = render(&mut fresh, 100, 30);
    assert!(s.contains(refusal), "{s}");
    assert!(!s.contains("connecting…"), "{s}");
    assert!(!s.contains("reconnecting…"), "{s}");
}

#[test]
fn edit_warnings_render_under_diff_from_shared_fixture() {
    let part: Part = serde_json::from_str(include_str!(
        "../../proto/fixtures/part/tool_done_edit_warnings.json"
    ))
    .unwrap();
    let msg = Message {
        id: "m_warn".into(),
        role: Role::Assistant,
        time: 0,
        time_end: Some(1),
        source: MessageSource::Human,
        parts: vec![part],
        model: None,
        agent: None,
        usage: None,
        error: None,
    };
    let rows = crate::ui::message::render(&msg, &ctx(true), 100);
    let rendered: Vec<String> = rows.iter().map(|r| text::line_text(&r.line)).collect();
    let diff_idx = rendered
        .iter()
        .position(|l| l.contains("diff ─"))
        .expect("diff section");
    let warn_hdr_idx = rendered
        .iter()
        .position(|l| l.contains("warnings ─"))
        .expect("warnings section");
    let warn_row_idx = rendered
        .iter()
        .position(|l| l.contains("Recovered: file changed since read; line numbers remapped."))
        .expect("warning line");
    let out_idx = rendered
        .iter()
        .position(|l| l.contains("output ─"))
        .expect("output section");
    assert!(diff_idx < warn_hdr_idx && warn_hdr_idx < warn_row_idx && warn_row_idx < out_idx);
    let has_warning_color = rows[warn_row_idx]
        .line
        .spans
        .iter()
        .any(|s| s.style.fg == Some(crate::ui::theme::WARNING));
    assert!(has_warning_color);

    // Without warnings, no warnings section is drawn.
    let plain: Part = serde_json::from_str(include_str!(
        "../../proto/fixtures/part/tool_done_edit_diff.json"
    ))
    .unwrap();
    let msg_plain = Message {
        parts: vec![plain],
        ..msg
    };
    let rows_plain = crate::ui::message::render(&msg_plain, &ctx(true), 100);
    assert!(
        !rows_plain
            .iter()
            .any(|r| text::line_text(&r.line).contains("warnings ─"))
    );
}

#[test]
fn permission_modal_renders_preview_and_truncation_marker() {
    let mut app = test_app(false);
    // Absent preview: no preview section.
    let no_prev = PermissionRequest {
        session_id: TEST_SESSION.into(),
        message_id: "m_p".into(),
        call_id: "c_p0".into(),
        tool: "edit".into(),
        purpose: "Fix parser".into(),
        input: json!({"path": "src/parser.rs"}),
        time: 0,
        preview: None,
    };
    app.views
        .get_mut(TEST_SESSION)
        .unwrap()
        .apply(SessionEvent::Permission { request: no_prev });
    let (_, s) = render(&mut app, 100, 34);
    assert!(!s.contains("preview"), "{s}");

    app.views
        .get_mut(TEST_SESSION)
        .unwrap()
        .apply(SessionEvent::PermissionResolved {
            call_id: "c_p0".into(),
        });

    // With preview (> 12 diff lines + truncated = true): shows diff, counts, marker, and (preview cut).
    let mut diff = String::from("--- a/src/parser.rs\n+++ b/src/parser.rs\n@@ -1,15 +1,15 @@\n");
    for i in 1..=15 {
        diff.push_str(&format!("-old line {i}\n+new line {i}\n"));
    }
    let with_prev = PermissionRequest {
        session_id: TEST_SESSION.into(),
        message_id: "m_p".into(),
        call_id: "c_p1".into(),
        tool: "edit".into(),
        purpose: "Fix parser".into(),
        input: json!({"path": "src/parser.rs"}),
        time: 0,
        preview: Some(EditPreview {
            diff,
            added: 15,
            removed: 15,
            truncated: true,
        }),
    };
    app.views
        .get_mut(TEST_SESSION)
        .unwrap()
        .apply(SessionEvent::Permission { request: with_prev });
    let (_, s) = render(&mut app, 100, 34);
    assert!(s.contains("preview  +15 -15 (preview cut)"), "{s}");
    assert!(s.contains("--- a/src/parser.rs"), "{s}");
    assert!(s.contains("-old line 1"), "{s}");
    assert!(s.contains("more lines (preview cut)"), "{s}");
}

#[test]
fn malformed_responses_toast_errors_without_fallback() {
    use crate::app::Pending;
    let mut app = test_app(false);
    // Malformed Continue response must not start a new session.
    app.pending.insert(1, Pending::Continue);
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: 1,
        ok: true,
        result: json!({ "sessions": "bad" }),
        error: None,
        code: None,
    }));
    assert!(app.pending.is_empty(), "did not create a new session");
    assert!(
        app.toasts
            .items
            .iter()
            .any(|t| t.text.contains("Malformed continue"))
    );

    // Malformed MessagesRange response clears range_pending and preserves more_before.
    app.toasts.items.clear();
    let v = app.views.get_mut(TEST_SESSION).unwrap();
    v.more_before = true;
    let hist_gen = v.history_gen;
    v.range_pending = Some((hist_gen, 0, 10));
    app.pending.insert(
        2,
        Pending::MessagesRange {
            sid: TEST_SESSION.into(),
            generation: hist_gen,
            start_seq: 0,
            end_seq: 10,
        },
    );
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: 2,
        ok: true,
        result: json!({ "messages": [] }),
        error: None,
        code: None,
    }));
    let v = &app.views[TEST_SESSION];
    assert!(v.more_before && v.range_pending.is_none());
    assert!(
        app.toasts
            .items
            .iter()
            .any(|t| t.text.contains("Malformed history page"))
    );
    // Malformed ConfigMcp and Text responses toast errors and do not open empty dialog.
    app.toasts.items.clear();
    app.pending.insert(3, Pending::ConfigMcp);
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: 3,
        ok: true,
        result: json!({}),
        error: None,
        code: None,
    }));
    assert!(
        app.toasts
            .items
            .iter()
            .any(|t| t.text.contains("Malformed config"))
    );

    app.toasts.items.clear();
    app.pending.insert(
        4,
        Pending::Text {
            title: "Peers".into(),
        },
    );
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: 4,
        ok: true,
        result: json!({}),
        error: None,
        code: None,
    }));
    assert!(app.dialog.is_none());
    assert!(
        app.toasts
            .items
            .iter()
            .any(|t| t.text.contains("Malformed response"))
    );
}

#[test]
fn copy_text_toasts_osc52_warn_and_clipboard_error() {
    use ref_proto::ToastLevel;
    let mut app = test_app(false);

    app.copy_text("hello", "selection");
    assert_eq!(crate::clipboard::take_last_copy().as_deref(), Some("hello"));
    assert_eq!(app.toasts.items.last().unwrap().level, ToastLevel::Info);

    crate::clipboard::set_next_copy_result(Ok(crate::clipboard::CopyOutcome::Osc52Only));
    app.copy_text("hello", "selection");
    let t = app.toasts.items.last().unwrap();
    assert_eq!(t.level, ToastLevel::Info);
    assert_eq!(t.text, "Copied via terminal (OSC 52)");
    crate::clipboard::set_next_copy_result(Err("no clipboard tool".into()));
    app.copy_text("hello", "selection");
    let t = app.toasts.items.last().unwrap();
    assert_eq!(t.level, ToastLevel::Error);
    assert!(
        t.text.contains("Copy failed: no clipboard tool"),
        "{}",
        t.text
    );
}

#[test]
fn pasting_oversized_image_path_toasts_error() {
    let mut app = test_app(false);
    let dir = std::env::temp_dir().join(format!("ref-tui-paste-err-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("huge.png");
    let f = std::fs::File::create(&path).unwrap();
    f.set_len(21 * 1024 * 1024).unwrap();
    drop(f);
    crate::input::handle(
        &mut app,
        crossterm::event::Event::Paste(path.to_string_lossy().into_owned()),
    );
    assert!(app.editor.text.is_empty());
    assert!(app.editor.images.is_empty());
    assert!(
        app.toasts
            .items
            .iter()
            .any(|t| t.text.contains("too large"))
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn push_history_toasts_once_on_failure() {
    let mut app = test_app(false);
    let dir = std::env::temp_dir().join(format!("ref-tui-hist-app-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    app.history = crate::ui::editor::History::with_path(dir.clone());
    app.push_history("first prompt");
    app.push_history("second prompt");
    let warns: Vec<_> = app
        .toasts
        .items
        .iter()
        .filter(|t| t.text.contains("Cannot save prompt history"))
        .collect();
    assert_eq!(warns.len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn load_prompt_history_from_warns_on_corrupt_file_and_missing_state_dir() {
    let dir = std::env::temp_dir().join(format!("ref-tui-hist-load-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("prompt-history.jsonl"), "{corrupt\n").unwrap();

    let mut toasts = crate::ui::toast::Toasts::default();
    let h = crate::app::load_prompt_history_from(Ok(dir.clone()), &mut toasts);
    assert!(h.items.is_empty());
    assert_eq!(toasts.items.len(), 1);
    assert_eq!(toasts.items[0].level, ToastLevel::Warn);
    assert!(toasts.items[0].text.contains("Cannot load prompt history"));

    let mut toasts = crate::ui::toast::Toasts::default();
    let h = crate::app::load_prompt_history_from(Err("no home".into()), &mut toasts);
    assert!(h.items.is_empty());
    assert_eq!(toasts.items.len(), 1);
    assert_eq!(toasts.items[0].level, ToastLevel::Warn);
    assert!(toasts.items[0].text.contains("Cannot locate state dir"));

    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// 0.2.3: /reload, slash UX, shell mode, tile borders, and 12 UX fixes
// ---------------------------------------------------------------------------

fn drain_requests(rx: &mut mpsc::UnboundedReceiver<ClientFrame>) -> Vec<ClientFrame> {
    std::iter::from_fn(|| rx.try_recv().ok()).collect()
}

#[test]
fn reload_config_deduplicates_config_changed_and_toasts_warnings() {
    let (mut app, mut rx) = test_app_with_requests(false);
    type_text(&mut app, "/RELOAD");
    press(&mut app, KeyCode::Enter);
    let frames = drain_requests(&mut rx);
    let reload_frame = frames
        .iter()
        .find(|f| matches!(f.req, Request::ReloadConfig { .. }))
        .expect("reload_config sent");
    assert!(matches!(
        &reload_frame.req,
        Request::ReloadConfig { cwd: Some(c) } if c == TEST_CWD
    ));
    let reload_id = reload_frame.id.unwrap();

    // While ReloadConfig is in flight, a broadcast ConfigChanged must not trigger GetConfig or duplicate toasts.
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::ConfigChanged {
        warnings: vec!["bad key in ref.jsonc".into()],
    }));
    assert!(
        drain_requests(&mut rx).is_empty(),
        "deduped while ReloadConfig in flight"
    );
    assert!(app.toasts.items.is_empty());

    // ReloadConfig response updates config, refreshes MCP, and toasts both info and warnings.
    let mut updated = app.config.clone().unwrap();
    updated.commands.push(CommandInfo {
        name: "review-pr".into(),
        description: "Review pull request".into(),
        source: "config".into(),
    });
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: reload_id,
        ok: true,
        result: json!({
            "config": updated,
            "warnings": ["bad key in ref.jsonc"]
        }),
        error: None,
        code: None,
    }));
    assert!(
        app.config
            .as_ref()
            .unwrap()
            .commands
            .iter()
            .any(|c| c.name == "review-pr")
    );
    let after_frames = drain_requests(&mut rx);
    assert!(
        after_frames
            .iter()
            .any(|f| matches!(f.req, Request::McpStatus { cwd: Some(ref c) } if c == TEST_CWD))
    );
    assert!(
        app.toasts
            .items
            .iter()
            .any(|t| t.level == ToastLevel::Info && t.text == "Configuration reloaded")
    );
    assert!(
        app.toasts
            .items
            .iter()
            .any(|t| t.level == ToastLevel::Warn && t.text.contains("bad key in ref.jsonc"))
    );

    // External ConfigChanged (no ReloadConfig in flight) fetches GetConfig + McpStatus scoped to cwd.
    app.toasts.items.clear();
    answer_mcp(&mut app, &mcp_servers());
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::ConfigChanged {
        warnings: vec!["unknown field foo".into()],
    }));
    let ext_frames = drain_requests(&mut rx);
    assert!(
        ext_frames
            .iter()
            .any(|f| matches!(f.req, Request::GetConfig { cwd: Some(ref c) } if c == TEST_CWD))
    );
    assert!(
        ext_frames
            .iter()
            .any(|f| matches!(f.req, Request::McpStatus { cwd: Some(ref c) } if c == TEST_CWD))
    );
    assert!(
        app.toasts
            .items
            .iter()
            .any(|t| t.level == ToastLevel::Warn && t.text.contains("unknown field foo"))
    );

    // /cd response refreshes GetConfig + McpStatus for the new directory.
    type_text(&mut app, "/cd /home/u/other");
    press(&mut app, KeyCode::Enter);
    let cd_frame = drain_requests(&mut rx)
        .into_iter()
        .find(|f| matches!(f.req, Request::SetCwd { .. }))
        .expect("set_cwd sent");
    let mut new_info = app.views[TEST_SESSION].info.clone();
    new_info.cwd = "/home/u/other".into();
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: cd_frame.id.unwrap(),
        ok: true,
        result: json!({ "session": new_info }),
        error: None,
        code: None,
    }));
    let cd_after = drain_requests(&mut rx);
    assert!(
        cd_after.iter().any(
            |f| matches!(f.req, Request::GetConfig { cwd: Some(ref c) } if c == "/home/u/other")
        )
    );
    assert!(
        cd_after.iter().any(
            |f| matches!(f.req, Request::McpStatus { cwd: Some(ref c) } if c == "/home/u/other")
        )
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mock_reload_listdirs_listbinaries_and_variant_clear() {
    let (out_tx, out_rx) = mpsc::unbounded_channel();
    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel();
    tokio::spawn(crate::mock::run(TEST_CWD.into(), out_rx, ev_tx));
    let mut app = App::new(
        Client::new(out_tx.clone()),
        TEST_CWD.into(),
        InitPlan {
            prompt: Some("hello".into()),
            ..Default::default()
        },
    );
    pump(&mut app, &mut ev_rx, PUMP_TURN_MS).await;
    let sid = app.current.clone().expect("session");

    // /reload against mock backend succeeds and toasts Configuration reloaded without duplicate warning.
    app.toasts.items.clear();
    type_text(&mut app, "/reload");
    press(&mut app, KeyCode::Enter);
    pump(&mut app, &mut ev_rx, PUMP_ROUNDTRIP_MS).await;
    assert!(
        app.toasts
            .items
            .iter()
            .any(|t| t.text == "Configuration reloaded")
    );

    // /variant default sends Some("") and mock clears session.model.variant to None.
    type_text(&mut app, "/variant high");
    press(&mut app, KeyCode::Enter);
    pump(&mut app, &mut ev_rx, PUMP_ROUNDTRIP_MS).await;
    assert_eq!(app.views[&sid].info.model.variant.as_deref(), Some("high"));
    type_text(&mut app, "/variant default");
    press(&mut app, KeyCode::Enter);
    pump(&mut app, &mut ev_rx, PUMP_ROUNDTRIP_MS).await;
    assert_eq!(app.views[&sid].info.model.variant, None);
    assert!(
        app.toasts
            .items
            .iter()
            .any(|t| t.text == "Variant: default")
    );

    // Direct ListDirs (existing + nonexistent ERR_DIR_NOT_FOUND) and ListBinaries on mock.
    let _ = out_tx.send(ClientFrame {
        id: Some(9001),
        req: Request::ListDirs {
            cwd: Some(TEST_CWD.into()),
            path: ".".into(),
            include_files: true,
        },
    });
    let _ = out_tx.send(ClientFrame {
        id: Some(9002),
        req: Request::ListDirs {
            cwd: Some(TEST_CWD.into()),
            path: "no_such_dir".into(),
            include_files: false,
        },
    });
    let _ = out_tx.send(ClientFrame {
        id: Some(9003),
        req: Request::ListBinaries {
            query: "car".into(),
            limit: Some(10),
        },
    });
    tokio::time::sleep(std::time::Duration::from_millis(120)).await;
    let mut got_dirs = false;
    let mut got_not_found = false;
    let mut got_bins = false;
    while let Ok(ev) = ev_rx.try_recv() {
        if let crate::net::NetEvent::Frame(ServerFrame::Response {
            id,
            ok,
            result,
            code,
            ..
        }) = ev
        {
            match id {
                9001 => {
                    assert!(ok);
                    let dirs: Vec<String> = serde_json::from_value(result["dirs"].clone()).unwrap();
                    let files: Vec<String> =
                        serde_json::from_value(result["files"].clone()).unwrap();
                    assert!(dirs.contains(&"src".to_string()), "{dirs:?}");
                    assert!(files.contains(&"Cargo.toml".to_string()), "{files:?}");
                    got_dirs = true;
                }
                9002 => {
                    assert!(!ok);
                    assert_eq!(code.as_deref(), Some(ERR_DIR_NOT_FOUND));
                    got_not_found = true;
                }
                9003 => {
                    assert!(ok);
                    let bins: Vec<String> =
                        serde_json::from_value(result["binaries"].clone()).unwrap();
                    assert_eq!(bins.first().map(String::as_str), Some("cargo"));
                    got_bins = true;
                }
                _ => {}
            }
        }
    }
    assert!(got_dirs && got_not_found && got_bins);
}

#[test]
fn slash_case_insensitivity_and_unknown_typo_guard() {
    let (mut app, mut rx) = test_app_with_requests(false);
    app.config.as_mut().unwrap().commands.push(CommandInfo {
        name: "review".into(),
        description: "Review code".into(),
        source: "config".into(),
    });

    // Case-insensitive builtin (/HELP) and custom (/REVIEW).
    type_text(&mut app, "/HELP");
    press(&mut app, KeyCode::Enter);
    assert!(matches!(
        app.dialog,
        Some(crate::ui::dialog::Dialog::Help(_))
    ));
    app.dialog = None;

    type_text(&mut app, "/REVIEW src/lib.rs");
    press(&mut app, KeyCode::Enter);
    let frames = drain_requests(&mut rx);
    assert!(frames.iter().any(|f| matches!(
        &f.req,
        Request::ExpandCommand { name, args, .. } if name == "review" && args == "src/lib.rs"
    )));

    // Single-token transposition typo `/relaod`: not a fuzzy subsequence, intercepted locally,
    // keeps draft in editor, and suggests `/reload` via Damerau-Levenshtein!
    app.toasts.items.clear();
    type_text(&mut app, "/relaod");
    press(&mut app, KeyCode::Enter);
    assert!(
        drain_requests(&mut rx).is_empty(),
        "typo must not send prompt"
    );
    assert_eq!(app.editor.text, "/relaod", "draft preserved for quick edit");
    let warn = &app.toasts.items.last().expect("warn toast").text;
    assert!(
        warn.contains("Unknown command /relaod") && warn.contains("/reload"),
        "{warn}"
    );
    app.editor.clear();

    // Distant single-token `/zzzzzzzz`: intercepted locally without bogus suggestion.
    app.toasts.items.clear();
    type_text(&mut app, "/zzzzzzzz");
    press(&mut app, KeyCode::Enter);
    assert!(drain_requests(&mut rx).is_empty());
    assert_eq!(app.editor.text, "/zzzzzzzz");
    assert_eq!(
        app.toasts.items.last().unwrap().text,
        "Unknown command /zzzzzzzz"
    );
    app.editor.clear();

    // Multi-word prose starting with a Unix path is sent as a normal prompt.
    type_text(&mut app, "/etc/passwd is readable by all users");
    press(&mut app, KeyCode::Enter);
    let frames = drain_requests(&mut rx);
    assert!(frames.iter().any(|f| matches!(
        &f.req,
        Request::Prompt { text, .. } if text == "/etc/passwd is readable by all users"
    )));
}

#[test]
fn direct_slash_args_models_agents_variant_sessions_close_mcp() {
    let (mut app, mut rx) = test_app_with_requests(false);
    let cfg = app.config.as_mut().unwrap();
    cfg.agents.push(AgentInfo {
        name: "plan".into(),
        description: "Planner".into(),
        model: None,
        creatable: true,
        can_spawn: false,
        color: None,
    });
    cfg.models.push(ModelInfo {
        provider: "oai".into(),
        provider_name: "OpenAI".into(),
        id: "gpt-6".into(),
        name: "GPT-6".into(),
        context: 400_000,
        output: 0,
        images: true,
        reasoning: true,
        variants: vec!["low".into(), "high".into()],
    });
    app.refresh_mcp();
    answer_mcp(&mut app, &mcp_servers());
    let _ = drain_requests(&mut rx);

    // Direct `/model gpt-6` switches without opening a modal.
    type_text(&mut app, "/model gpt-6");
    press(&mut app, KeyCode::Enter);
    assert!(app.dialog.is_none());
    let frames = drain_requests(&mut rx);
    assert!(frames.iter().any(|f| matches!(
        &f.req,
        Request::SetModel { model, .. } if model.provider == "oai" && model.model == "gpt-6"
    )));

    // Direct `/agent plan` switches without opening a modal.
    type_text(&mut app, "/agent plan");
    press(&mut app, KeyCode::Enter);
    assert!(app.dialog.is_none());
    let frames = drain_requests(&mut rx);
    assert!(frames.iter().any(|f| matches!(
        &f.req,
        Request::SetAgent { agent, .. } if agent == "plan"
    )));

    // Direct `/variant default` sends `Some("")` instead of `None` (which would cycle).
    type_text(&mut app, "/variant default");
    press(&mut app, KeyCode::Enter);
    let frames = drain_requests(&mut rx);
    assert!(frames.iter().any(|f| matches!(
        &f.req,
        Request::SetVariant { variant, .. } if variant.as_deref() == Some("")
    )));

    // Multi-token `/close-session "worker one" worker-2` closes both live sessions.
    let s1 = bg_session("ses_w1", "worker one", SessionStatus::Idle, 0);
    let s2 = bg_session("ses_w2", "worker-2", SessionStatus::Idle, 0);
    app.live = vec![app.views[TEST_SESSION].info.clone(), s1, s2];
    type_text(&mut app, "/close-session \"worker one\" worker-2");
    press(&mut app, KeyCode::Enter);
    let closed: Vec<String> = drain_requests(&mut rx)
        .into_iter()
        .filter_map(|f| match f.req {
            Request::Close { session_id } => Some(session_id),
            _ => None,
        })
        .collect();
    assert_eq!(closed, vec!["ses_w1".to_string(), "ses_w2".to_string()]);

    // `/sessions archived-parser` fetches history when not in live and attaches the closed session.
    type_text(&mut app, "/sessions archived-parser");
    press(&mut app, KeyCode::Enter);
    let hist_req = drain_requests(&mut rx)
        .into_iter()
        .find(|f| matches!(f.req, Request::ListHistory { .. }))
        .expect("list_history sent");
    let mut hist_sess = bg_session("ses_hist1", "archived-parser", SessionStatus::Idle, 0);
    hist_sess.live = false;
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: hist_req.id.unwrap(),
        ok: true,
        result: json!({ "sessions": [hist_sess] }),
        error: None,
        code: None,
    }));
    assert_eq!(app.current.as_deref(), Some("ses_hist1"));
    let _ = drain_requests(&mut rx);
    app.current = Some(TEST_SESSION.into());

    // Case-insensitive `/mcp GITHUB` opens MCP dialog focused & expanded on `github`.
    type_text(&mut app, "/mcp GITHUB");
    press(&mut app, KeyCode::Enter);
    let (_, s) = render(&mut app, 100, 30);
    assert!(s.contains("MCP servers") && s.contains("r refresh"), "{s}");
    if let Some(crate::ui::dialog::Dialog::Mcp(d)) = &app.dialog {
        let idx = app.mcp.iter().position(|e| e.name == "github").unwrap();
        assert_eq!(d.sel, idx);
    } else {
        panic!("expected Mcp dialog");
    }
}

#[test]
fn interactive_cd_picker_and_palette_arg_prompt() {
    let (mut app, mut rx) = test_app_with_requests(false);
    app.config.as_mut().unwrap().commands.push(CommandInfo {
        name: "deploy".into(),
        description: "Deploy target environment".into(),
        source: "config".into(),
    });
    let _ = drain_requests(&mut rx);

    // `/cd` with no argument opens interactive Kind::Cd picker and requests ListDirs.
    type_text(&mut app, "/cd");
    press(&mut app, KeyCode::Enter);
    assert!(
        matches!(app.dialog, Some(crate::ui::dialog::Dialog::List(ref d)) if d.kind == crate::ui::dialog::Kind::Cd)
    );
    let dir_frame = drain_requests(&mut rx)
        .into_iter()
        .find(|f| matches!(f.req, Request::ListDirs { .. }))
        .expect("list_dirs sent");
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: dir_frame.id.unwrap(),
        ok: true,
        result: json!({ "path": TEST_CWD, "dirs": ["crates", "src"], "files": [] }),
        error: None,
        code: None,
    }));
    let (_, s) = render(&mut app, 100, 28);
    assert!(
        s.contains("Change working directory") && s.contains("crates/"),
        "{s}"
    );

    // Down x2 selects `crates/` (after `Use this directory` and `../`); Tab drills into `crates/`,
    // Backspace at `/` pops back up, and typing `cr` + Enter confirms `crates`!
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Tab);
    if let Some(crate::ui::dialog::Dialog::List(d)) = &app.dialog {
        assert!(
            d.filter.text.ends_with("crates/"),
            "drilled into crates/: {}",
            d.filter.text
        );
    }
    let _ = drain_requests(&mut rx);
    press(&mut app, KeyCode::Backspace);
    if let Some(crate::ui::dialog::Dialog::List(d)) = &app.dialog {
        assert!(
            !d.filter.text.ends_with("crates/"),
            "popped up: {}",
            d.filter.text
        );
    }
    type_text(&mut app, "cr");
    // Enter confirms the filtered directory (`crates`) and sends SetCwd.
    press(&mut app, KeyCode::Enter);
    assert!(app.dialog.is_none());
    let frames = drain_requests(&mut rx);
    assert!(
        frames
            .iter()
            .any(|f| matches!(&f.req, Request::SetCwd { path, .. } if path.ends_with("crates")))
    );

    // Ctrl+P Palette -> selecting custom command with `args: true` enters inline `asking_arg`; Esc returns to Palette!
    press_with(&mut app, KeyCode::Char('p'), KeyModifiers::CONTROL);
    type_text(&mut app, "deploy");
    press(&mut app, KeyCode::Enter);
    if let Some(crate::ui::dialog::Dialog::List(d)) = &app.dialog {
        assert!(d.asking_arg.is_some(), "entered inline arg prompt");
    } else {
        panic!("palette closed unexpectedly");
    }
    let (_, s) = render(&mut app, 100, 26);
    assert!(s.contains("esc back to commands"), "{s}");
    // Esc cancels arg prompt back to the filtered palette list instead of closing the dialog.
    press(&mut app, KeyCode::Esc);
    if let Some(crate::ui::dialog::Dialog::List(d)) = &app.dialog {
        assert!(d.asking_arg.is_none());
        assert_eq!(d.filter.text, "deploy");
    } else {
        panic!("Esc should return to palette list");
    }
    // Re-enter arg prompt and submit argument.
    press(&mut app, KeyCode::Enter);
    type_text(&mut app, "staging");
    press(&mut app, KeyCode::Enter);
    assert!(app.dialog.is_none());
    let frames = drain_requests(&mut rx);
    assert!(frames.iter().any(|f| matches!(
        &f.req,
        Request::ExpandCommand { name, args, .. } if name == "deploy" && args == "staging"
    )));
}

#[test]
fn slash_arg_completions_zero_flicker_dir_cache_and_custom_cmd_mentions() {
    let (mut app, mut rx) = test_app_with_requests(false);
    app.config.as_mut().unwrap().commands.push(CommandInfo {
        name: "review".into(),
        description: "Review files".into(),
        source: "config".into(),
    });
    let _ = drain_requests(&mut rx);

    // `/variant ` shows `default` + model variants.
    type_text(&mut app, "/variant ");
    let ac = app.ac.as_ref().expect("variant arg ac");
    assert_eq!(ac.kind, crate::app::AcKind::SlashArg);
    assert!(ac.items.iter().any(|(_, l, _)| l == "default"));
    assert!(ac.items.iter().any(|(_, l, _)| l == "high"));
    app.editor.clear();

    // `/export ` shows default session filename suggestion; `/models ` & `/agents ` show choices.
    type_text(&mut app, "/export ");
    let ac = app.ac.as_ref().expect("export arg ac");
    assert_eq!(ac.kind, crate::app::AcKind::SlashArg);
    assert!(ac.items.iter().any(|(_, _, d)| d == "default filename"));
    let _ = drain_requests(&mut rx);
    app.editor.clear();
    type_text(&mut app, "/models ");
    assert!(
        app.ac
            .as_ref()
            .is_some_and(|a| a.kind == crate::app::AcKind::SlashArg)
    );
    app.editor.clear();
    type_text(&mut app, "/agents ");
    assert!(
        app.ac
            .as_ref()
            .is_some_and(|a| a.kind == crate::app::AcKind::SlashArg)
    );
    app.editor.clear();

    // `/cd ` fetches ListDirs once, then typing `sr` filters synchronously from DirCache with 0 flicker!
    type_text(&mut app, "/cd ");
    let dir_frame = drain_requests(&mut rx)
        .into_iter()
        .find(|f| matches!(f.req, Request::ListDirs { .. }))
        .expect("list_dirs for /cd");
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: dir_frame.id.unwrap(),
        ok: true,
        result: json!({ "path": TEST_CWD, "dirs": ["crates", "docs", "src"], "files": [] }),
        error: None,
        code: None,
    }));
    assert_eq!(app.ac.as_ref().unwrap().items.len(), 3);
    type_text(&mut app, "sr");
    // Must stay populated synchronously without sending another ListDirs!
    assert!(
        drain_requests(&mut rx).is_empty(),
        "served from parent DirCache"
    );
    let ac = app.ac.as_ref().expect("0-flicker ac");
    assert_eq!(ac.items.len(), 1);
    assert_eq!(ac.items[0].1, "src/");
    app.editor.clear();

    // `@file` mention inside custom command arguments (`/review @src/ma`) triggers file completion!
    type_text(&mut app, "/review @src/ma");
    assert!(
        app.files_due.is_some(),
        "@mention inside custom command schedules ListFiles"
    );
}

#[test]
fn shell_mode_completions_freeform_enter_history_and_paste() {
    let (mut app, mut rx) = test_app_with_requests(false);
    app.config.as_mut().unwrap().agents.push(AgentInfo {
        name: "plan".into(),
        description: "Planner".into(),
        model: None,
        creatable: true,
        can_spawn: false,
        color: None,
    });
    app.history = crate::ui::editor::History::default();
    app.push_history("explain parser");
    app.push_history("!cargo check --workspace");
    app.push_history("write unit tests");
    let _ = drain_requests(&mut rx);

    // Enter shell mode with `!`.
    type_text(&mut app, "!");
    assert!(app.shell_mode);
    assert_eq!(app.editor.text, "");
    // Tab in shell_mode must NEVER cycle agent (even when popup is empty/pending).
    app.ac = None;
    press(&mut app, KeyCode::Tab);
    assert!(
        drain_requests(&mut rx)
            .iter()
            .all(|f| !matches!(f.req, Request::SetAgent { .. }))
    );
    assert_eq!(app.views[TEST_SESSION].info.agent, "build");

    // Up/Down in shell_mode recalls ONLY `!`-prefixed history entries, keeping `shell_mode == true`.
    app.editor.clear();
    press(&mut app, KeyCode::Up);
    assert!(app.shell_mode);
    assert_eq!(app.editor.text, "cargo check --workspace");
    press(&mut app, KeyCode::Up);
    assert_eq!(
        app.editor.text, "cargo check --workspace",
        "skips non-shell history"
    );
    press(&mut app, KeyCode::Down);
    assert!(app.shell_mode);
    assert_eq!(app.editor.text, "");

    // Typing `car` triggers ListBinaries + merges `!` history into ShellCmd with `freeform_enter = true`.
    type_text(&mut app, "car");
    let bin_frame = drain_requests(&mut rx)
        .into_iter()
        .find(|f| matches!(&f.req, Request::ListBinaries { query, .. } if query == "car"))
        .expect("list_binaries for car sent");
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: bin_frame.id.unwrap(),
        ok: true,
        result: json!({ "binaries": ["cargo", "cargo-clippy"] }),
        error: None,
        code: None,
    }));
    let ac = app.ac.as_ref().expect("shell cmd popup");
    assert_eq!(ac.kind, crate::app::AcKind::ShellCmd);
    assert!(ac.freeform_enter && !ac.explicitly_selected);
    // History item `cargo check --workspace` + binary `cargo` both appear.
    assert!(
        ac.items
            .iter()
            .any(|(v, l, d)| v == "cargo check --workspace"
                && l == "!cargo check --workspace"
                && d == "history")
    );
    assert!(ac.items.iter().any(|(_, l, _)| l == "cargo"));

    // Plain Enter without explicit selection submits `!car` directly instead of hijacking to completion!
    press(&mut app, KeyCode::Enter);
    assert!(!app.shell_mode);
    let frames = drain_requests(&mut rx);
    assert!(
        frames
            .iter()
            .any(|f| matches!(&f.req, Request::Shell { command, .. } if command == "car"))
    );

    // Explicit Down + Enter accepts completion; pipe `| gr` switches back from ShellPath to ShellCmd.
    type_text(&mut app, "!car");
    assert!(app.ac.is_some(), "cached binaries populate immediately");
    press(&mut app, KeyCode::Down);
    assert!(app.ac.as_ref().unwrap().explicitly_selected);
    press(&mut app, KeyCode::Enter);
    assert!(app.shell_mode);
    assert!(
        app.editor.text.starts_with("cargo"),
        "accepted completion: {}",
        app.editor.text
    );
    type_text(&mut app, " Cargo.toml | gr");
    let pipe_bin = drain_requests(&mut rx)
        .into_iter()
        .find(|f| matches!(&f.req, Request::ListBinaries { query, .. } if query == "gr"))
        .expect("pipe segment switches back to ListBinaries");
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: pipe_bin.id.unwrap(),
        ok: true,
        result: json!({ "binaries": ["grep"] }),
        error: None,
        code: None,
    }));
    assert_eq!(app.ac.as_ref().unwrap().kind, crate::app::AcKind::ShellCmd);

    // Pasting an existing `.png` path in shell_mode pastes literal path text instead of attaching [Image 1].
    app.editor.clear();
    let dir = std::env::temp_dir().join(format!("ref-tui-shell-paste-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let img_path = dir.join("shot.png");
    std::fs::write(&img_path, [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]).unwrap();
    let path_str = img_path.to_string_lossy().into_owned();
    crate::input::handle(&mut app, crossterm::event::Event::Paste(path_str.clone()));
    assert!(
        app.editor.images.is_empty(),
        "must not attach image in shell mode"
    );
    assert_eq!(app.editor.text, path_str);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tiny_terminal_bounds_safety_across_all_views_and_overlays() {
    let sizes = [(0, 0), (1, 1), (2, 2), (3, 2), (5, 3), (6, 4), (12, 6)];
    let mut app = sidebar_app();
    app.toasts.warn("Warning toast on a tiny viewport");
    type_text(&mut app, "/");
    assert!(app.ac.is_some());
    for &(w, h) in &sizes {
        render(&mut app, w, h);
    }
    app.editor.clear();

    // Dialogs: Palette, Sessions, Cd, Help, Mcp.
    app.open_palette();
    for &(w, h) in &sizes {
        render(&mut app, w, h);
    }
    app.open_sessions();
    for &(w, h) in &sizes {
        render(&mut app, w, h);
    }
    app.open_cd(Some(TEST_SESSION));
    for &(w, h) in &sizes {
        render(&mut app, w, h);
    }
    app.open_help();
    for &(w, h) in &sizes {
        render(&mut app, w, h);
    }
    app.open_mcp(None);
    for &(w, h) in &sizes {
        render(&mut app, w, h);
    }
    app.dialog = None;

    // Split Watch mode with focused tile input.
    app.mode = Mode::Watch;
    let bg1 = bg_session("ses_t1", "worker-1", SessionStatus::Busy, 0);
    let bg2 = bg_session("ses_t2", "worker-2", SessionStatus::WaitingPermission, 0);
    for inf in [&bg1, &bg2] {
        let mut v = SessionView::new(inf.clone(), false);
        let msgs = transcript(true);
        let total = msgs.len() as u64;
        v.load_snapshot(SessionSnapshot {
            info: inf.clone(),
            messages: msgs,
            more_before: false,
            total_messages: total,
            start_seq: 0,
            layout_metas: vec![],
            pending_permissions: vec![],
            pending_questions: vec![],
            queued: vec![],
        });
        app.views.insert(inf.id.clone(), v);
    }
    app.live = vec![bg1, bg2];
    app.watch.focus = Some("ses_t1".into());
    for &(w, h) in &sizes {
        render(&mut app, w, h);
    }

    // Permission modal with diff preview on tiny screens.
    app.mode = Mode::Chat;
    app.views
        .get_mut(TEST_SESSION)
        .unwrap()
        .apply(SessionEvent::Permission {
            request: PermissionRequest {
                session_id: TEST_SESSION.into(),
                message_id: "m_p".into(),
                call_id: "c_tiny".into(),
                tool: "edit".into(),
                purpose: "Edit src/lib.rs".into(),
                input: json!({"path": "src/very/deeply/nested/path/to/module.rs"}),
                time: 0,
                preview: Some(EditPreview {
                    diff: "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-old\n+new\n".into(),
                    added: 1,
                    removed: 1,
                    truncated: false,
                }),
            },
        });
    for &(w, h) in &sizes {
        render(&mut app, w, h);
    }
}

#[test]
fn split_view_border_priority_root_color_and_closed_history_root() {
    use crate::ui::theme;
    // Palette colors must never collide with reserved semantic border colors.
    for &c in &theme::ROOT_TASK_PALETTE {
        assert_ne!(c, theme::PRIMARY);
        assert_ne!(c, theme::WARNING);
        assert_ne!(c, theme::ERROR);
    }

    let mut app = test_app(false);
    app.mode = Mode::Watch;
    let mut closed_root = bg_session("ses_closed_root", "epic-refactor", SessionStatus::Idle, 0);
    closed_root.live = false;
    app.history_list = vec![closed_root];

    let mut t_focus = bg_session(
        "s_focus",
        "focused-tile",
        SessionStatus::WaitingPermission,
        0,
    );
    t_focus.last_error = Some("boom".into());
    t_focus.parent = Some("ses_closed_root".into());
    let mut t_warn = bg_session("s_warn", "warn-tile", SessionStatus::WaitingQuestion, 0);
    t_warn.last_error = Some("boom".into());
    t_warn.parent = Some("ses_closed_root".into());
    let mut t_err = bg_session("s_err", "err-tile", SessionStatus::Busy, 0);
    t_err.last_error = Some("boom".into());
    t_err.parent = Some("ses_closed_root".into());
    let mut t_child = bg_session("s_child", "child-tile", SessionStatus::Busy, 0);
    t_child.parent = Some("ses_closed_root".into());
    let t_busy = bg_session("s_busy", "busy-root", SessionStatus::Busy, 0);
    let t_idle = bg_session("s_idle", "idle-root", SessionStatus::Idle, 0);

    let tiles = [t_focus, t_warn, t_err, t_child, t_busy, t_idle];
    for inf in &tiles {
        let mut v = SessionView::new(inf.clone(), false);
        v.load_snapshot(SessionSnapshot {
            info: inf.clone(),
            messages: vec![],
            more_before: false,
            total_messages: 0,
            start_seq: 0,
            layout_metas: vec![],
            pending_permissions: vec![],
            pending_questions: vec![],
            queued: vec![],
        });
        app.views.insert(inf.id.clone(), v);
    }
    app.live = tiles.to_vec();
    app.watch.focus = Some("s_focus".into());
    let (term, s) = render(&mut app, 180, 40);
    // Closed root in history_list resolves `⟲ epic-refactor` chip!
    assert!(s.contains("⟲ epic-refactor"), "{s}");
    let buf = term.backend().buffer();
    let border_fg = |sid: &str| {
        let r = app.watch.tiles.iter().find(|(id, _)| id == sid).unwrap().1;
        assert_eq!(
            buf[(r.x, r.y)].symbol(),
            "╭",
            "solid rounded border on {sid}"
        );
        buf[(r.x, r.y)].fg
    };
    assert_eq!(border_fg("s_focus"), theme::PRIMARY);
    assert_eq!(border_fg("s_warn"), theme::WARNING);
    assert_eq!(border_fg("s_err"), theme::ERROR);
    assert_eq!(
        border_fg("s_child"),
        theme::root_task_color("ses_closed_root")
    );
    assert_eq!(border_fg("s_busy"), theme::BORDER_ACTIVE);
    assert_eq!(border_fg("s_idle"), theme::BORDER);
}

#[test]
fn focused_tile_mouse_click_and_tile_slash_and_shell_commands() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let (mut app, mut rx) = test_app_with_requests(false);
    app.config.as_mut().unwrap().agents.push(AgentInfo {
        name: "plan".into(),
        description: "Planner".into(),
        model: None,
        creatable: true,
        can_spawn: false,
        color: None,
    });
    app.mode = Mode::Watch;
    let bg = bg_session("ses_tile", "tile-worker", SessionStatus::Idle, 0);
    let mut v = SessionView::new(bg.clone(), false);
    let msgs = transcript(false);
    let total = msgs.len() as u64;
    v.load_snapshot(SessionSnapshot {
        info: bg.clone(),
        messages: msgs,
        more_before: false,
        total_messages: total,
        start_seq: 0,
        layout_metas: vec![],
        pending_permissions: vec![],
        pending_questions: vec![],
        queued: vec![],
    });
    app.views.insert("ses_tile".into(), v);
    app.live = vec![bg];
    app.watch.focus = Some("ses_tile".into());
    render(&mut app, 100, 24);
    let _ = drain_requests(&mut rx);

    // Typing in focused tile and clicking inside its input repositions cursor without unfocusing!
    type_text(&mut app, "hello world");
    render(&mut app, 100, 24);
    let tile_rect = app.watch.tiles[0].1;
    let ir = crate::ui::split::input_rect(tile_rect).unwrap();
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), ir.x + 2, ir.y),
    );
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Up(MouseButton::Left), ir.x + 2, ir.y),
    );
    assert_eq!(
        app.watch.focus.as_deref(),
        Some("ses_tile"),
        "input click keeps focus"
    );
    assert_eq!(
        app.watch.inputs["ses_tile"].cursor, 2,
        "click sets tile input cursor"
    );

    // Clicking inside tile body also keeps focus; clicking on the tile border unfocuses!
    crate::input::handle(
        &mut app,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            tile_rect.x + 4,
            tile_rect.y + 2,
        ),
    );
    crate::input::handle(
        &mut app,
        mouse(
            MouseEventKind::Up(MouseButton::Left),
            tile_rect.x + 4,
            tile_rect.y + 2,
        ),
    );
    assert_eq!(
        app.watch.focus.as_deref(),
        Some("ses_tile"),
        "body click keeps focus"
    );
    crate::input::handle(
        &mut app,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            tile_rect.x,
            tile_rect.y,
        ),
    );
    crate::input::handle(
        &mut app,
        mouse(
            MouseEventKind::Up(MouseButton::Left),
            tile_rect.x,
            tile_rect.y,
        ),
    );
    assert!(app.watch.focus.is_none(), "border click unfocuses");

    // Refocus and run `/agent plan`, `!pwd`, and typo `/badcmd` targeting `ses_tile` (NOT TEST_SESSION).
    app.watch.focus = Some("ses_tile".into());
    app.watch.inputs.get_mut("ses_tile").unwrap().clear();
    type_text(&mut app, "/agent plan");
    press(&mut app, KeyCode::Enter);
    let frames = drain_requests(&mut rx);
    assert!(frames.iter().any(|f| matches!(
        &f.req,
        Request::SetAgent { session_id, agent } if session_id == "ses_tile" && agent == "plan"
    )));
    type_text(&mut app, "!pwd");
    press(&mut app, KeyCode::Enter);
    let frames = drain_requests(&mut rx);
    assert!(frames.iter().any(|f| matches!(
        &f.req,
        Request::Shell { session_id, command } if session_id == "ses_tile" && command == "pwd"
    )));
    type_text(&mut app, "/badcmd");
    press(&mut app, KeyCode::Enter);
    assert!(
        drain_requests(&mut rx).is_empty(),
        "tile typo must not send prompt"
    );
    assert_eq!(app.watch.inputs["ses_tile"].text, "/badcmd");
}

#[test]
fn autocomplete_hover_scroll_stability_and_stationary_mouse_dedup() {
    use crossterm::event::MouseEventKind;
    let mut app = test_app(false);
    // Typing `/` opens >10 commands; scroll down to item 12 (`scroll > 0`).
    type_text(&mut app, "/");
    render(&mut app, 100, 28);
    for _ in 0..12 {
        press(&mut app, KeyCode::Down);
    }
    render(&mut app, 100, 28);
    let saved_scroll = app.ac.as_ref().unwrap().scroll;
    let r = app.ac.as_ref().unwrap().rect;
    assert!(saved_scroll > 0);
    // Hovering on visible row 2 selects `saved_scroll + 2` without jumping `scroll`!
    crate::input::handle(&mut app, mouse(MouseEventKind::Moved, r.x + 4, r.y + 3));
    assert_eq!(app.ac.as_ref().unwrap().sel, saved_scroll + 2);
    assert_eq!(app.ac.as_ref().unwrap().scroll, saved_scroll);
    // Hovering on bottom pad row `r.y + r.height - 1` does nothing (not off-by-one)!
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Moved, r.x + 4, r.y + r.height - 1),
    );
    assert_eq!(app.ac.as_ref().unwrap().sel, saved_scroll + 2);
    app.editor.clear();
    app.ac = None;

    // Stationary MouseEventKind::Moved dedup: keyboard selection & Ctrl+D confirm in ListDialog survive!
    let s1 = bg_session("ses_a", "alpha", SessionStatus::Idle, 0);
    let s2 = bg_session("ses_b", "beta", SessionStatus::Idle, 0);
    app.live = vec![s1, s2];
    app.open_sessions();
    let (_, s) = render(&mut app, 100, 28);
    let alpha_y = s.lines().position(|l| l.contains("alpha")).unwrap() as u16;
    let hover_x = 40;
    crate::input::handle(&mut app, mouse(MouseEventKind::Moved, hover_x, alpha_y));
    if let Some(crate::ui::dialog::Dialog::List(d)) = &app.dialog {
        assert_eq!(d.sel, 0);
    }
    // Navigate down with keyboard and arm Ctrl+D confirm.
    press(&mut app, KeyCode::Down);
    press_with(&mut app, KeyCode::Char('d'), KeyModifiers::CONTROL);
    // Emit stationary Moved at the SAME (hover_x, alpha_y): must be ignored!
    crate::input::handle(&mut app, mouse(MouseEventKind::Moved, hover_x, alpha_y));
    if let Some(crate::ui::dialog::Dialog::List(d)) = &app.dialog {
        assert_eq!(d.sel, 1, "stationary Moved must not snap selection back");
        assert!(
            d.confirm.is_some(),
            "stationary Moved must not clear Ctrl+D confirm"
        );
    }
    // Actually moving the mouse to `(hover_x + 1, alpha_y)` updates hover normally.
    crate::input::handle(&mut app, mouse(MouseEventKind::Moved, hover_x + 1, alpha_y));
    if let Some(crate::ui::dialog::Dialog::List(d)) = &app.dialog {
        assert_eq!(d.sel, 0);
    }
}

#[test]
fn image_and_paste_token_cursor_atomicity() {
    let mut ed = crate::ui::editor::Editor::default();
    ed.insert("ab ");
    ed.attach_image(
        ImageData {
            mime: "image/png".into(),
            data: "AAAA".into(),
            ..Default::default()
        },
        Some("a.png".into()),
    );
    ed.insert("cd");
    // Text is `"ab [Image 1] cd"`; `[Image 1]` spans bytes 3..12.
    assert_eq!(ed.text, "ab [Image 1] cd");
    let tok_start = 3usize;
    let tok_end = 12usize;

    // Left from `tok_end` jumps straight to `tok_start` in one press.
    ed.cursor = tok_end;
    ed.left();
    assert_eq!(ed.cursor, tok_start);
    // Right from `tok_start` jumps straight to `tok_end` in one press.
    ed.right();
    assert_eq!(ed.cursor, tok_end);
    // Word-left and word-right never stop inside `tok_start..tok_end`.
    ed.cursor = ed.text.len();
    while ed.cursor > 0 {
        ed.word_left();
        assert!(
            !(ed.cursor > tok_start && ed.cursor < tok_end),
            "word_left inside token: {}",
            ed.cursor
        );
    }
    while ed.cursor < ed.text.len() {
        ed.word_right();
        assert!(
            !(ed.cursor > tok_start && ed.cursor < tok_end),
            "word_right inside token: {}",
            ed.cursor
        );
    }
    // Single-click inside `[Image 1]` snaps cursor to nearest boundary (`tok_start` or `tok_end`).
    ed.mouse_down(tok_start + 2, 1);
    assert_eq!(ed.cursor, tok_start);
    ed.mouse_down(tok_end - 2, 1);
    assert_eq!(ed.cursor, tok_end);
    // Multi-line `up` into a row containing `[Image 1]` snaps outside the token.
    ed.set_text("ab [Image 1] cd\n0123456");
    ed.images.push(crate::ui::editor::Attachment {
        token: "[Image 1]".into(),
        image: ImageData {
            mime: "image/png".into(),
            data: "AAAA".into(),
            ..Default::default()
        },
        name: None,
    });
    ed.last_width = 40;
    ed.cursor = ed.text.len(); // column 7 on row 2 -> middle of `[Image 1]` on row 1!
    assert!(ed.up());
    assert!(
        !(ed.cursor > tok_start && ed.cursor < tok_end),
        "up() landed inside token: {}",
        ed.cursor
    );
}

#[test]
fn single_line_editor_horizontal_scroll_and_help_and_narrow_permission() {
    use crossterm::event::{MouseButton, MouseEventKind};
    // 1. Single-line horizontal scroll in ListDialog filter keeps tail around cursor visible.
    let mut app = test_app(false);
    app.open_palette();
    let long_q = "abcdefghijklmnopqrstuvwxyz0123456789_TAIL_MARKER";
    type_text(&mut app, long_q);
    let (_, s) = render(&mut app, 44, 18);
    assert!(
        s.contains("TAIL_MARKER"),
        "single-line hscroll keeps tail visible:\n{s}"
    );
    if let Some(crate::ui::dialog::Dialog::List(d)) = &app.dialog {
        assert!(d.filter.hscroll > 0);
    }
    app.dialog = None;

    // 2. HelpDialog Home/End/g/G scrolling, non-closing random keys & inside click, outside click closes.
    app.open_help();
    if let Some(crate::ui::dialog::Dialog::Help(h)) = &mut app.dialog {
        h.opened -= crate::ui::dialog::OPEN_SETTLE;
    }
    render(&mut app, 90, 18);
    press(&mut app, KeyCode::End);
    if let Some(crate::ui::dialog::Dialog::Help(h)) = &app.dialog {
        assert!(h.scroll > 0, "End scrolls help to bottom");
    }
    press(&mut app, KeyCode::Home);
    if let Some(crate::ui::dialog::Dialog::Help(h)) = &app.dialog {
        assert_eq!(h.scroll, 0, "Home scrolls help to top");
    }
    // Random key (`a`, `Enter`) and click inside `h.rect` must NOT dismiss HelpDialog.
    press(&mut app, KeyCode::Char('a'));
    press(&mut app, KeyCode::Enter);
    let hr = match &app.dialog {
        Some(crate::ui::dialog::Dialog::Help(h)) => h.rect,
        _ => panic!("HelpDialog closed on non-close key"),
    };
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), hr.x + 2, hr.y + 2),
    );
    assert!(
        matches!(app.dialog, Some(crate::ui::dialog::Dialog::Help(_))),
        "inside click keeps help open"
    );
    // Outside click dismisses HelpDialog.
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), 0, 0),
    );
    assert!(app.dialog.is_none(), "outside click closes help");

    // 3. Narrow PermissionView wraps headline/path and steps down through 3 compact button tiers.
    app.views.get_mut(TEST_SESSION).unwrap().apply(SessionEvent::Permission {
        request: PermissionRequest {
            session_id: TEST_SESSION.into(),
            message_id: "m_p".into(),
            call_id: "c_narrow".into(),
            tool: "edit".into(),
            purpose: "Refactor long deeply nested configuration loader".into(),
            input: json!({"path": "crates/tui/src/very_long_directory_name/deeply_nested_file.rs"}),
            time: 0,
            preview: None,
        },
    });
    render(&mut app, 80, 22);
    app.perm.as_mut().unwrap().opened -= PAST_GRACE;
    let (_, wide_s) = render(&mut app, 80, 22);
    assert!(
        wide_s.contains("Allow once") && wide_s.contains("Always allow"),
        "{wide_s}"
    );
    let (_, mid_s) = render(&mut app, 40, 22);
    assert!(
        mid_s.contains("Once") && mid_s.contains("Always") && mid_s.contains("Deny"),
        "{mid_s}"
    );
    let (_, narrow_s) = render(&mut app, 22, 22);
    assert!(
        narrow_s.contains(" a ") && narrow_s.contains(" s ") && narrow_s.contains(" d "),
        "{narrow_s}"
    );
}

#[test]
fn cd_dialog_windows_roots_parent_nav_and_backspace_edges() {
    let (mut app, mut rx) = test_app_with_requests(false);
    // 1. Windows drive root `C:/` in `open_cd`, `cur_target`, `at_root`, and `dialog_submit`.
    app.views.get_mut(TEST_SESSION).unwrap().info.cwd = r"C:\".into();
    app.open_cd(Some(TEST_SESSION));
    if let Some(crate::ui::dialog::Dialog::List(d)) = &app.dialog {
        assert_eq!(d.filter.text, "C:/");
    }
    let f = drain_requests(&mut rx)
        .into_iter()
        .find(|f| matches!(f.req, Request::ListDirs { .. }))
        .expect("list_dirs for C:/");
    if let Request::ListDirs { path, .. } = &f.req {
        assert_eq!(path, "C:/", "drive root keeps trailing slash in ListDirs");
    }
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: f.id.unwrap(),
        ok: true,
        result: json!({ "path": "C:/", "dirs": ["Users"], "files": [] }),
        error: None,
        code: None,
    }));
    if let Some(crate::ui::dialog::Dialog::List(d)) = &app.dialog {
        assert!(d.items.iter().all(|i| i.key != "nav:.."), "C:/ is at_root");
        assert!(d.items.iter().any(|i| i.key == "use:C:/"));
    }
    press(&mut app, KeyCode::Enter);
    let sent = drain_requests(&mut rx);
    assert!(
        sent.iter()
            .any(|f| matches!(&f.req, Request::SetCwd { path, .. } if path == "C:/"))
    );

    //Restore TEST_CWD and verify `nav:..` vs `Backspace` at `~/`, `""`, `./`, `../`, and `../../`.
    app.views.get_mut(TEST_SESSION).unwrap().info.cwd = TEST_CWD.into();
    app.open_cd(Some(TEST_SESSION));
    let _ = drain_requests(&mut rx);
    if let Some(crate::ui::dialog::Dialog::List(d)) = &mut app.dialog {
        d.filter.set_text("~/");
    }
    app.on_cd_filter_edited();
    let home_req = drain_requests(&mut rx)
        .into_iter()
        .find(|f| matches!(f.req, Request::ListDirs { .. }))
        .unwrap();
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: home_req.id.unwrap(),
        ok: true,
        result: json!({ "path": "/home/u", "dirs": ["proj"], "files": [] }),
        error: None,
        code: None,
    }));
    // Select `nav:..` at `~/` (when session cwd is child `/home/u/proj`): Tab steps UP to `/home/`, NEVER `""`!
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Tab);
    if let Some(crate::ui::dialog::Dialog::List(d)) = &app.dialog {
        assert_eq!(d.filter.text, "/home/");
    }
    // Backspace at `~/` deletes `/` -> `"~"` instead of teleporting to `""`!
    if let Some(crate::ui::dialog::Dialog::List(d)) = &mut app.dialog {
        d.filter.set_text("~/");
    }
    press(&mut app, KeyCode::Backspace);
    if let Some(crate::ui::dialog::Dialog::List(d)) = &app.dialog {
        assert_eq!(d.filter.text, "~");
    }
    // Unresolved `""` -> `nav:..` gives `"../"` (never `"/"`), then `"../../"`, and Backspace never loops!
    let _ = drain_requests(&mut rx);
    if let Some(crate::ui::dialog::Dialog::List(d)) = &mut app.dialog {
        d.filter.set_text("");
        d.cd_resolved_dir.clear();
        d.items = vec![crate::ui::dialog::Item {
            key: "nav:..".into(),
            title: "../".into(),
            ..Default::default()
        }];
        d.refilter();
        d.sel = 0;
    }
    press(&mut app, KeyCode::Enter);
    if let Some(crate::ui::dialog::Dialog::List(d)) = &mut app.dialog {
        assert_eq!(d.filter.text, "../");
        d.sel = 0;
    }
    press(&mut app, KeyCode::Tab);
    if let Some(crate::ui::dialog::Dialog::List(d)) = &app.dialog {
        assert_eq!(d.filter.text, "../../");
    }
    // Backspace from `"../../"` -> `"../"` -> `".."` -> `"."` -> `""`!
    for expected in ["../", "..", ".", ""] {
        press(&mut app, KeyCode::Backspace);
        if let Some(crate::ui::dialog::Dialog::List(d)) = &app.dialog {
            assert_eq!(d.filter.text, expected);
        }
    }
}

#[test]
fn autocomplete_cursor_moves_export_tilde_typing_reset_and_shell_ctrl_v() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let (mut app, mut rx) = test_app_with_requests(false);
    let _ = drain_requests(&mut rx);

    // 1. Cursor-only movement (keyboard + mouse) updates or closes `app.ac`.
    type_text(&mut app, "!echo src/");
    let dir_req = drain_requests(&mut rx)
        .into_iter()
        .find(|f| matches!(&f.req, Request::ListDirs { path, .. } if path == "src"))
        .unwrap();
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: dir_req.id.unwrap(),
        ok: true,
        result: json!({ "path": "/home/u/proj/src", "dirs": ["ui"], "files": ["main.rs"] }),
        error: None,
        code: None,
    }));
    assert_eq!(app.ac.as_ref().unwrap().kind, crate::app::AcKind::ShellPath);
    assert_eq!(app.ac.as_ref().unwrap().start, 5);
    // Pressing Home moves cursor to byte 0 (`""` before cursor -> `ac` closes instead of stale `start=5`).
    press(&mut app, KeyCode::Home);
    assert!(
        app.ac.is_none(),
        "Home to start of shell line closes ShellPath ac"
    );
    app.shell_mode = false;
    app.editor.clear();

    // Mouse click inside editor away from `@mention` closes `AcKind::File`.
    type_text(&mut app, "check @src/ma");
    app.files_due = None;
    let fid = 900;
    app.pending.insert(
        fid,
        crate::app::Pending::Files {
            query: "src/ma".into(),
        },
    );
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: fid,
        ok: true,
        result: json!({ "files": ["src/main.rs"] }),
        error: None,
        code: None,
    }));
    assert_eq!(app.ac.as_ref().unwrap().kind, crate::app::AcKind::File);
    render(&mut app, 100, 24);
    let lay = crate::render::chat_layout(&app, app.screen);
    crate::input::handle(
        &mut app,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            lay.editor.x,
            lay.editor.y,
        ),
    );
    crate::input::handle(
        &mut app,
        mouse(
            MouseEventKind::Up(MouseButton::Left),
            lay.editor.x,
            lay.editor.y,
        ),
    );
    assert!(
        app.ac.is_none(),
        "clicking away from @mention closes File ac"
    );
    app.editor.clear();

    // 2. `/export ~/out.md`, `/export ~\out.md`, and `/export C:\out.md` do NOT prepend session cwd!
    for (cmd_str, want) in [
        ("/export ~/out.md", "~/out.md"),
        (r"/export ~\out.md", r"~\out.md"),
        (r"/export C:\out.md", r"C:\out.md"),
    ] {
        type_text(&mut app, cmd_str);
        press(&mut app, KeyCode::Enter);
        let frames = drain_requests(&mut rx);
        assert!(
            frames.iter().any(|f| matches!(
                &f.req,
                Request::Export { path, .. } if path == want
            )),
            "cmd={cmd_str}"
        );
    }

    // 3. `explicitly_selected` resets on subsequent typing in free-form popup, while async reply preserves it.
    type_text(&mut app, "!ca");
    let b1 = drain_requests(&mut rx)
        .into_iter()
        .find(|f| matches!(&f.req, Request::ListBinaries { query, .. } if query == "ca"))
        .unwrap();
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: b1.id.unwrap(),
        ok: true,
        result: json!({ "binaries": ["cargo", "cat", "cal"] }),
        error: None,
        code: None,
    }));
    press(&mut app, KeyCode::Down);
    assert!(app.ac.as_ref().unwrap().explicitly_selected);
    assert_eq!(app.ac.as_ref().unwrap().sel, 1);
    // User types `t` (`!cat`): `explicitly_selected` resets to false and `sel` to 0!
    type_text(&mut app, "t");
    let b2 = drain_requests(&mut rx)
        .into_iter()
        .find(|f| matches!(&f.req, Request::ListBinaries { query, .. } if query == "cat"))
        .unwrap();
    assert!(!app.ac.as_ref().unwrap().explicitly_selected);
    assert_eq!(app.ac.as_ref().unwrap().sel, 0);
    // If user presses Down while `b2` is in flight, `b2`'s arrival preserves `explicitly_selected`!
    press(&mut app, KeyCode::Down);
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: b2.id.unwrap(),
        ok: true,
        result: json!({ "binaries": ["cat", "catchsegv"] }),
        error: None,
        code: None,
    }));
    assert!(app.ac.as_ref().unwrap().explicitly_selected);
    // Typing `" f"` resets again so plain Enter runs `!cat f` as-is.
    type_text(&mut app, " f");
    let _ = drain_requests(&mut rx);
    press(&mut app, KeyCode::Enter);
    let frames = drain_requests(&mut rx);
    assert!(frames.iter().any(|f| matches!(
        &f.req,
        Request::Shell { command, .. } if command == "cat f"
    )));

    // 4. `clipboard_paste_with` skips image paste in `shell_mode`, and clears `app.ac` when attaching in chat.
    type_text(&mut app, "!");
    assert!(app.shell_mode);
    crate::input::clipboard_paste_with(
        &mut app,
        || panic!("must not read clipboard image in shell_mode"),
        || Ok(Some("echo from_clip".into())),
    );
    assert!(app.editor.images.is_empty());
    assert_eq!(app.editor.text, "echo from_clip");
    app.shell_mode = false;
    app.editor.clear();
    type_text(&mut app, "/");
    assert!(app.ac.is_some());
    crate::input::clipboard_paste_with(
        &mut app,
        || Ok(Some(ImageData::new("image/png", "AAAA"))),
        || Ok(None),
    );
    assert!(app.ac.is_none(), "attach_image clears app.ac");
    assert_eq!(app.editor.images.len(), 1);

    // 5. Closing `current` while in `Mode::Watch` cleans up `watch.inputs` and `attaching`.
    app.toggle_watch();
    app.watch
        .inputs
        .entry(TEST_SESSION.into())
        .or_default()
        .set_text("draft");
    app.close_session(TEST_SESSION);
    assert!(!app.watch.inputs.contains_key(TEST_SESSION));
}

#[test]
fn standalone_slash_command_completion_tab_and_enter() {
    use crate::ui::dialog::{Dialog, Kind};
    use crossterm::event::{MouseButton, MouseEventKind};
    let (mut app, mut rx) = test_app_with_requests(false);
    app.config.as_mut().unwrap().commands.push(CommandInfo {
        name: "deploy".into(),
        description: "Deploy app".into(),
        source: "config".into(),
    });
    let _ = drain_requests(&mut rx);

    // 1. Tab on a standalone command completes WITHOUT trailing space and closes popup.
    type_text(&mut app, "/ses");
    let ac = app.ac.as_ref().expect("slash popup for /ses");
    assert_eq!(ac.items[0].0, "/sessions");
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.editor.text, "/sessions");
    assert!(app.ac.is_none());
    assert!(app.dialog.is_none());
    press(&mut app, KeyCode::Enter);
    assert!(matches!(
        &app.dialog,
        Some(Dialog::List(d)) if d.kind == Kind::Sessions
    ));
    app.dialog = None;

    // 2. Enter on partial standalone optional-arg commands executes directly!
    for (prefix, check) in [
        ("/ses", "sessions"),
        ("/mod", "models"),
        ("/ag", "agents"),
        ("/var", "variant"),
        ("/cd", "cd"),
        ("/close-s", "close-session"),
        ("/mcp", "mcp"),
    ] {
        type_text(&mut app, prefix);
        assert!(app.ac.is_some(), "popup for {prefix}");
        assert!(
            !app.ac.as_ref().unwrap().items[0].0.ends_with(' '),
            "no trailing space for {prefix}"
        );
        press(&mut app, KeyCode::Enter);
        assert!(app.editor.text.is_empty(), "{prefix} cleared editor");
        assert!(app.ac.is_none(), "{prefix} closed ac");
        assert!(
            app.dialog.is_some(),
            "{check} opened picker directly on Enter"
        );
        app.dialog = None;
    }

    // `/exp`, `/peers-n`, and custom `/dep` also execute directly on Enter!
    type_text(&mut app, "/exp");
    press(&mut app, KeyCode::Enter);
    assert!(app.editor.text.is_empty());
    let frames = drain_requests(&mut rx);
    assert!(frames.iter().any(|f| matches!(
        &f.req,
        Request::Export { path, .. } if path.ends_with(&format!("ref-session-{TEST_SESSION}.md"))
    )));

    type_text(&mut app, "/peers-n");
    press(&mut app, KeyCode::Enter);
    assert!(app.editor.text.is_empty());
    let frames = drain_requests(&mut rx);
    assert!(frames.iter().any(|f| matches!(
        &f.req,
        Request::PeerCommand { command, args, .. } if command == "peers-name" && args.is_empty()
    )));

    type_text(&mut app, "/dep");
    assert_eq!(app.ac.as_ref().unwrap().items[0].0, "/deploy");
    press(&mut app, KeyCode::Enter);
    assert!(app.editor.text.is_empty());
    let frames = drain_requests(&mut rx);
    assert!(frames.iter().any(|f| matches!(
        &f.req,
        Request::ExpandCommand { name, args, .. } if name == "deploy" && args.is_empty()
    )));

    // 3. `/rename` still requires an argument: keeps trailing space on both Tab and Enter.
    type_text(&mut app, "/ren");
    assert_eq!(app.ac.as_ref().unwrap().items[0].0, "/rename ");
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.editor.text, "/rename ");
    assert!(app.ac.is_none());
    app.editor.clear();
    type_text(&mut app, "/ren");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.editor.text, "/rename ");
    assert!(app.ac.is_none());
    assert!(drain_requests(&mut rx).is_empty());
    app.editor.clear();

    // 4. Mouse click on `/sessions` in autocomplete popup executes directly.
    type_text(&mut app, "/ses");
    render(&mut app, 100, 28);
    let r = app.ac.as_ref().unwrap().rect;
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), r.x + 3, r.y + 1),
    );
    assert!(matches!(
        &app.dialog,
        Some(Dialog::List(d)) if d.kind == Kind::Sessions
    ));
}

#[test]
fn watch_mode_autocomplete_and_shell_mode() {
    use crate::ui::dialog::{Dialog, Kind};
    use crossterm::event::{MouseButton, MouseEventKind};
    let (mut app, mut rx) = test_app_with_requests(false);
    app.config.as_mut().unwrap().agents.push(AgentInfo {
        name: "plan".into(),
        description: "Planner".into(),
        model: None,
        creatable: true,
        can_spawn: false,
        color: None,
    });
    app.mode = Mode::Watch;
    let mut bg1 = bg_session("ses_t1", "worker-1", SessionStatus::Idle, 0);
    bg1.cwd = "/repo/worker1".into();
    let mut bg2 = bg_session("ses_t2", "worker-2", SessionStatus::Idle, 0);
    bg2.cwd = "/repo/worker2".into();
    for inf in [&bg1, &bg2] {
        let mut v = SessionView::new(inf.clone(), false);
        let msgs = transcript(false);
        let total = msgs.len() as u64;
        v.load_snapshot(SessionSnapshot {
            info: inf.clone(),
            messages: msgs,
            more_before: false,
            total_messages: total,
            start_seq: 0,
            layout_metas: vec![],
            pending_permissions: vec![],
            pending_questions: vec![],
            queued: vec![],
        });
        app.views.insert(inf.id.clone(), v);
    }
    app.live = vec![bg1, bg2];
    render(&mut app, 120, 30);
    let _ = drain_requests(&mut rx);

    // Focus first tile with Tab.
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.watch.focus.as_deref(), Some("ses_t1"));

    // 1. Slash completion in Watch mode renders popup and Tab / Enter work.
    type_text(&mut app, "/");
    let ac = app.ac.as_ref().expect("slash ac in watch mode");
    assert_eq!(ac.kind, crate::app::AcKind::Slash);
    let (_, scr) = render(&mut app, 120, 30);
    assert!(scr.contains("commands") && scr.contains("/new"), "{scr}");
    assert!(app.ac.as_ref().unwrap().rect.width > 0);
    // Home moves cursor away from end -> closes Slash ac.
    press(&mut app, KeyCode::Home);
    assert!(app.ac.is_none());
    press(&mut app, KeyCode::End);
    type_text(&mut app, "ses");
    assert_eq!(app.ac.as_ref().unwrap().items[0].0, "/sessions");
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.watch.inputs["ses_t1"].text, "/sessions");
    assert!(app.ac.is_none());

    // Focus change clears ac / suppress_ac; `/ag` + Enter opens Agents targeting focused tile.
    app.watch.inputs.get_mut("ses_t1").unwrap().clear();
    type_text(&mut app, "/ag");
    assert!(app.ac.is_some());
    press_with(&mut app, KeyCode::Right, KeyModifiers::CONTROL);
    assert_eq!(app.watch.focus.as_deref(), Some("ses_t2"));
    assert!(app.ac.is_none(), "focus change clears ac");
    type_text(&mut app, "/export ");
    assert!(
        app.ac
            .as_ref()
            .unwrap()
            .items
            .iter()
            .any(|(v, _, _)| v == "ref-session-ses_t2.md"),
        "export completion uses focused tile session id"
    );
    app.watch.inputs.get_mut("ses_t2").unwrap().clear();
    app.clear_autocomplete();

    // 2. `@file` mention in Watch mode uses focused tile cwd and renders popup.
    type_text(&mut app, "check @src/ma");
    assert!(app.files_due.is_some());
    app.files_due = Some((
        std::time::Instant::now() - std::time::Duration::from_millis(1),
        "src/ma".into(),
    ));
    app.housekeeping();
    let file_req = drain_requests(&mut rx)
        .into_iter()
        .find(|f| matches!(&f.req, Request::ListFiles { cwd, .. } if cwd == "/repo/worker2"))
        .expect("ListFiles uses focused tile cwd");
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: file_req.id.unwrap(),
        ok: true,
        result: json!({ "files": ["src/main.rs"] }),
        error: None,
        code: None,
    }));
    assert_eq!(
        app.ac.as_ref().map(|a| &a.kind),
        Some(&crate::app::AcKind::File)
    );
    render(&mut app, 120, 30);
    let r = app.ac.as_ref().unwrap().rect;
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), r.x + 3, r.y + 1),
    );
    assert_eq!(app.watch.inputs["ses_t2"].text, "check @src/main.rs ");
    app.watch.inputs.get_mut("ses_t2").unwrap().clear();

    // Mid-text `/` does not trigger slash autocomplete.
    type_text(&mut app, "hi /ses");
    assert!(app.ac.is_none());
    app.watch.inputs.get_mut("ses_t2").unwrap().clear();

    // Paste updates autocomplete in Watch mode.
    crate::input::handle(&mut app, crossterm::event::Event::Paste("/ag".into()));
    assert_eq!(
        app.ac.as_ref().map(|a| &a.kind),
        Some(&crate::app::AcKind::Slash)
    );
    press(&mut app, KeyCode::Enter);
    assert!(matches!(
        &app.dialog,
        Some(Dialog::List(d)) if d.kind == Kind::Agents && d.target_sid.as_deref() == Some("ses_t2")
    ));
    app.dialog = None;

    // 3. Per-tile `!` shell mode, badge, status line, Tab binary trigger, Esc layering, Backspace exit.
    type_text(&mut app, "!");
    assert!(app.watch.shell_modes.contains("ses_t2"));
    assert!(app.watch.inputs["ses_t2"].text.is_empty());
    let (_, scr) = render(&mut app, 120, 30);
    assert!(scr.contains("esc exit shell mode"), "{scr}");
    assert!(scr.contains("! Run a shell command"), "{scr}");

    // Tab in empty tile shell mode triggers ListBinaries instead of switching tiles.
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.watch.focus.as_deref(), Some("ses_t2"));
    let bin_empty = drain_requests(&mut rx)
        .into_iter()
        .find(|f| matches!(&f.req, Request::ListBinaries { query, .. } if query.is_empty()))
        .expect("Tab in tile shell mode requests ListBinaries");
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: bin_empty.id.unwrap(),
        ok: true,
        result: json!({ "binaries": ["cargo", "cat"] }),
        error: None,
        code: None,
    }));
    assert_eq!(
        app.ac.as_ref().map(|a| &a.kind),
        Some(&crate::app::AcKind::ShellCmd)
    );

    // Esc layering: 1) closes ac, 2) exits shell mode, 3) clears text, 4) unfocuses.
    type_text(&mut app, "ca");
    assert!(app.ac.is_some());
    press(&mut app, KeyCode::Esc);
    assert!(app.ac.is_none());
    assert!(app.watch.shell_modes.contains("ses_t2"));
    assert_eq!(app.watch.inputs["ses_t2"].text, "ca");
    press(&mut app, KeyCode::Esc);
    assert!(!app.watch.shell_modes.contains("ses_t2"));
    assert_eq!(app.watch.inputs["ses_t2"].text, "ca");
    press(&mut app, KeyCode::Esc);
    assert!(app.watch.inputs["ses_t2"].text.is_empty());
    press(&mut app, KeyCode::Esc);
    assert!(app.watch.focus.is_none());

    // Backspace on empty tile shell input exits shell mode; Down + Enter accepts completion and runs.
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.watch.focus.as_deref(), Some("ses_t1"));
    app.watch.inputs.get_mut("ses_t1").unwrap().clear();
    type_text(&mut app, "!");
    assert!(app.watch.shell_modes.contains("ses_t1"));
    press(&mut app, KeyCode::Backspace);
    assert!(!app.watch.shell_modes.contains("ses_t1"));

    type_text(&mut app, "!ca");
    let _ = drain_requests(&mut rx);
    assert_eq!(
        app.ac.as_ref().map(|a| &a.kind),
        Some(&crate::app::AcKind::ShellCmd)
    );
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.watch.inputs["ses_t1"].text, "cat ");
    type_text(&mut app, "README.md");
    press(&mut app, KeyCode::Enter);
    assert!(!app.watch.shell_modes.contains("ses_t1"));
    let frames = drain_requests(&mut rx);
    assert!(frames.iter().any(|f| matches!(
        &f.req,
        Request::Shell { session_id, command } if session_id == "ses_t1" && command == "cat README.md"
    )));
}

#[test]
fn watch_mode_text_selection_drag_copy_and_double_click() {
    use crossterm::event::{Event, KeyCode, KeyModifiers, MouseButton, MouseEventKind};
    let mut app = watch_app();
    app.sync_watch();
    let (_, s) = render(&mut app, 120, 30);
    let line_y = s
        .lines()
        .position(|l| l.contains("alpha answer 29"))
        .expect("visible tile message line") as u16;
    let r1 = tile_rect(&app, "ses_w1");
    let r2 = tile_rect(&app, "ses_w2");

    // 1. Drag inside tile 1 transcript (ending slightly outside tile border) creates active selection.
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), r1.x + 3, line_y),
    );
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Drag(MouseButton::Left), r1.x + 15, line_y),
    );
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Up(MouseButton::Left), r2.x + 2, line_y),
    );
    assert!(app.has_selection());
    assert_eq!(app.sel_owner.as_deref(), Some("ses_w1"));
    assert_eq!(app.watch.focus.as_deref(), Some("ses_w1"));
    assert!(app.watch.drag_tile.is_none());

    // Render paints SELECTION background on selected cells in draw_tile.
    // Note: focusing tile 1 shrinks its viewport by 2 rows for the input bar, shifting the line up.
    let (term, s_focused) = render(&mut app, 120, 30);
    let focused_line_y = s_focused
        .lines()
        .position(|l| l.contains("alpha answer 29"))
        .expect("focused tile message line") as u16;
    let buf = term.backend().buffer();
    assert!(
        (r1.x + 1..r1.x + r1.width - 1)
            .any(|x| buf[(x, focused_line_y)].bg == crate::ui::theme::SELECTION),
        "draw_tile must paint selection background"
    );

    // Right-click copies selection and clears it.
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Right), r1.x + 5, line_y),
    );
    assert!(!app.has_selection());
    assert!(
        app.toasts
            .items
            .iter()
            .any(|t| t.text.contains("Copied selection")),
        "{:?}",
        app.toasts.items.iter().map(|t| &t.text).collect::<Vec<_>>()
    );
    app.toasts.items.clear();

    // 2. Drag selection + Ctrl+C copies selection without clearing input or arming exit.
    type_text(&mut app, "draft");
    render(&mut app, 120, 30);
    crate::input::handle(
        &mut app,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            r1.x + 3,
            focused_line_y,
        ),
    );
    crate::input::handle(
        &mut app,
        mouse(
            MouseEventKind::Drag(MouseButton::Left),
            r1.x + 12,
            focused_line_y,
        ),
    );
    crate::input::handle(
        &mut app,
        mouse(
            MouseEventKind::Up(MouseButton::Left),
            r1.x + 12,
            focused_line_y,
        ),
    );
    assert!(app.has_selection());
    crate::input::handle(
        &mut app,
        Event::Key(crossterm::event::KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        )),
    );
    assert!(!app.has_selection());
    assert_eq!(app.watch.inputs["ses_w1"].text, "draft");
    assert!(app.ctrlc_armed.is_none());
    assert!(
        app.toasts
            .items
            .iter()
            .any(|t| t.text.contains("Copied selection"))
    );

    // 3. Esc clears chat selection first without unfocusing tile.
    app.watch.inputs.get_mut("ses_w1").unwrap().clear();
    crate::input::handle(
        &mut app,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            r1.x + 4,
            focused_line_y,
        ),
    );
    crate::input::handle(
        &mut app,
        mouse(
            MouseEventKind::Drag(MouseButton::Left),
            r1.x + 12,
            focused_line_y,
        ),
    );
    crate::input::handle(
        &mut app,
        mouse(
            MouseEventKind::Up(MouseButton::Left),
            r1.x + 12,
            focused_line_y,
        ),
    );
    assert!(app.has_selection());
    press(&mut app, KeyCode::Esc);
    assert!(!app.has_selection());
    assert_eq!(app.watch.focus.as_deref(), Some("ses_w1"));

    // 4. Double-clicking tile 2 zooms into tile 2.
    let click = |app: &mut App, x, y| {
        crate::input::handle(app, mouse(MouseEventKind::Down(MouseButton::Left), x, y));
    };
    click(&mut app, r2.x + 3, r2.y + 3);
    click(&mut app, r2.x + 3, r2.y + 3);
    assert_eq!(app.mode, Mode::Chat);
    assert_eq!(app.current.as_deref(), Some("ses_w2"));
}

#[test]
fn shell_wrapper_completion_end_to_end() {
    let (mut app, mut rx) = test_app_with_requests(false);
    app.history = crate::ui::editor::History::default();
    app.push_history("!cargo check --workspace");
    let _ = drain_requests(&mut rx);

    // Typing `sudo -u ` (wrapper flag arg) must NOT send ListBinaries for command completion.
    type_text(&mut app, "!sudo ");
    let _ = drain_requests(&mut rx);
    type_text(&mut app, "-u ro");
    assert!(
        drain_requests(&mut rx)
            .iter()
            .all(|f| !matches!(&f.req, Request::ListBinaries { .. })),
        "sudo -u ro must not prematurely request binaries"
    );
    type_text(&mut app, "root ");
    app.editor.clear();
    let _ = drain_requests(&mut rx);

    // Typing `!timeout 5s car` sends ListBinaries for `car` without history items, and completes `cargo `.
    type_text(&mut app, "timeout 5s car");
    let bin_req = drain_requests(&mut rx)
        .into_iter()
        .find(|f| matches!(&f.req, Request::ListBinaries { query, .. } if query == "car"))
        .expect("wrapped command requests ListBinaries");
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: bin_req.id.unwrap(),
        ok: true,
        result: json!({ "binaries": ["cargo"] }),
        error: None,
        code: None,
    }));
    let ac = app.ac.as_ref().expect("ShellCmd popup for wrapped command");
    assert_eq!(ac.kind, crate::app::AcKind::ShellCmd);
    assert!(
        ac.items.iter().all(|(_, _, d)| d != "history"),
        "wrapped commands must not suggest whole-line history items"
    );
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.editor.text, "timeout 5s cargo ");
}

#[test]
fn review_fixes_watch_selection_cleanup_drag_edge_and_shell_tab_reset() {
    use crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};

    // 1. Watch selection, drag_edge, drag_tile, and editor anchor cleanup across transitions.
    let mut app = watch_app();
    app.sync_watch();
    let (_, s) = render(&mut app, 120, 30);
    let _ = s;
    let r1 = tile_rect(&app, "ses_w1");
    let line_y = r1.y + 5;

    // Dragging above tile top sets `drag_edge = -1`, `sel_owner`, `watch.drag_tile`.
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), r1.x + 3, line_y),
    );
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Drag(MouseButton::Left), r1.x + 8, 0),
    );
    assert_eq!(app.views["ses_w1"].chat.drag_edge, -1);
    assert!(app.has_selection());
    assert!(app.watch.drag_tile.is_some());

    // `Esc` clears selection, resets `drag_edge = 0`, and clears `watch.drag_tile`.
    press(&mut app, KeyCode::Esc);
    assert!(!app.has_selection());
    assert!(app.watch.drag_tile.is_none());
    assert_eq!(app.views["ses_w1"].chat.drag_edge, 0);

    // Clicking inside `v.chat` clears focused tile editor `anchor` (Shift+Left selection).
    app.watch
        .inputs
        .entry("ses_w1".into())
        .or_default()
        .set_text("hello");
    press_with(&mut app, KeyCode::Left, KeyModifiers::SHIFT);
    assert!(app.watch.inputs["ses_w1"].anchor.is_some());
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), r1.x + 6, line_y),
    );
    assert!(app.watch.inputs["ses_w1"].anchor.is_none());

    // Drag + `Ctrl+N` page switch clears selection, `drag_edge`, and `watch.drag_tile`.
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Drag(MouseButton::Left), r1.x + 12, line_y),
    );
    app.views.get_mut("ses_w1").unwrap().chat.drag_edge = 1;
    assert!(app.has_selection());
    press_with(&mut app, KeyCode::Char('n'), KeyModifiers::CONTROL);
    assert!(!app.has_selection());
    assert!(app.watch.drag_tile.is_none());
    assert_eq!(app.views["ses_w1"].chat.drag_edge, 0);

    // Drag + `resume_current()` (`ctrl+x w` back to chat) clears selection & `drag_edge`.
    app.connected = false;
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), r1.x + 4, line_y),
    );
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Drag(MouseButton::Left), r1.x + 12, line_y),
    );
    app.views.get_mut("ses_w1").unwrap().chat.drag_edge = -1;
    app.toggle_watch();
    assert_eq!(app.mode, Mode::Chat);
    assert!(!app.has_selection());
    assert!(app.watch.drag_tile.is_none());
    assert_eq!(app.views["ses_w1"].chat.drag_edge, 0);
    assert!(app.views["ses_w1"].chat.sel.is_none());

    // Drag + `zoom` clears selection & `drag_edge`.
    app.toggle_watch();
    render(&mut app, 120, 30);
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), r1.x + 5, line_y),
    );
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Drag(MouseButton::Left), r1.x + 14, line_y),
    );
    app.views.get_mut("ses_w1").unwrap().chat.drag_edge = 1;
    app.zoom("ses_w2");
    assert_eq!(app.mode, Mode::Chat);
    assert_eq!(app.current.as_deref(), Some("ses_w2"));
    assert!(!app.has_selection());
    assert!(app.watch.drag_tile.is_none());
    assert_eq!(app.views["ses_w1"].chat.drag_edge, 0);
    assert!(app.views["ses_w1"].chat.sel.is_none());

    // 2. `shell_tab_requested` resets on accept_completion and Esc so delayed binaries don't reopen popup.
    let (mut app, mut rx) = test_app_with_requests(false);
    let _ = drain_requests(&mut rx);
    type_text(&mut app, "!");
    press(&mut app, KeyCode::Tab);
    let bin_req = drain_requests(&mut rx)
        .into_iter()
        .find(|f| matches!(&f.req, Request::ListBinaries { query, .. } if query.is_empty()))
        .expect("ListBinaries empty");
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: bin_req.id.unwrap(),
        ok: true,
        result: json!({ "binaries": ["sudo"] }),
        error: None,
        code: None,
    }));
    assert!(app.ac.is_some());
    // Accept `"sudo "` via Tab -> `shell_tab_requested` must be cleared.
    app.pending.insert(
        9999,
        crate::app::Pending::Binaries {
            query: String::new(),
        },
    );
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.editor.text, "sudo ");
    assert!(app.ac.is_none());
    // Simulate a late `ListBinaries` response for `""` arriving after acceptance: must NOT reopen popup!
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: 9999,
        ok: true,
        result: json!({ "binaries": ["sudo", "ls"] }),
        error: None,
        code: None,
    }));
    assert!(
        app.ac.is_none(),
        "stale binary response must not reopen popup after accept_completion"
    );

    // Also verify `Esc` dismissal + leaving/re-entering `!` resets `shell_tab_requested`.
    app.editor.clear();
    press(&mut app, KeyCode::Tab);
    assert!(app.ac.is_some());
    press(&mut app, KeyCode::Esc); // dismisses ac
    assert!(app.ac.is_none());
    press(&mut app, KeyCode::Esc); // leaves shell mode
    app.pending.insert(
        9998,
        crate::app::Pending::Binaries {
            query: String::new(),
        },
    );
    assert!(!app.shell_mode);
    type_text(&mut app, "!");
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: 9998,
        ok: true,
        result: json!({ "binaries": ["cargo"] }),
        error: None,
        code: None,
    }));
    assert!(
        app.ac.is_none(),
        "stale binary response must not reopen popup after Esc reset"
    );
}

#[test]
fn maybe_image_path_ctrl_z_restores_path_without_extra_empty_snapshot_or_reattach() {
    let dir = std::env::temp_dir().join(format!("ref-tui-img-undo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let img_path = dir.join("shot.png");
    std::fs::write(&img_path, b"\x89PNG\r\n\x1a\npayload").unwrap();
    let path_str = img_path.to_str().unwrap();

    let mut app = test_app(false);
    type_text(&mut app, path_str);
    assert_eq!(app.editor.text, "[Image 1] ");
    assert_eq!(app.editor.images.len(), 1);
    assert_eq!(
        app.toasts.items.last().map(|t| (t.level, t.text.as_str())),
        Some((ToastLevel::Info, "Attached image shot.png"))
    );

    // First Ctrl+Z restores the typed path (no extra empty snapshot, no immediate re-attach).
    press_with(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(app.editor.text, path_str);
    assert!(app.editor.images.is_empty());

    // Second Ctrl+Z restores the empty editor.
    press_with(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert!(app.editor.is_empty());

    // Ctrl+Y steps cleanly back through the path to the attached image.
    press_with(&mut app, KeyCode::Char('y'), KeyModifiers::CONTROL);
    assert_eq!(app.editor.text, path_str);
    assert!(app.editor.images.is_empty());

    press_with(&mut app, KeyCode::Char('y'), KeyModifiers::CONTROL);
    assert_eq!(app.editor.text, "[Image 1] ");
    assert_eq!(app.editor.images.len(), 1);

    // Typing after maybe_image_path does not coalesce into the attachment undo step.
    type_text(&mut app, "fix");
    assert_eq!(app.editor.text, "[Image 1] fix");
    press_with(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(app.editor.text, "[Image 1] ");
    assert_eq!(app.editor.images.len(), 1);
    press_with(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(app.editor.text, path_str);
    assert!(app.editor.images.is_empty());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn typing_after_attach_image_does_not_coalesce_with_attachment() {
    let mut app = test_app(false);
    app.editor
        .attach_image(ref_proto::ImageData::new("image/png", "AA"), None);
    type_text(&mut app, "fix");
    assert_eq!(app.editor.text, "[Image 1] fix");
    assert_eq!(app.editor.images.len(), 1);

    // First Ctrl+Z reverts only "fix", keeping [Image 1] and its attachment.
    press_with(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(app.editor.text, "[Image 1] ");
    assert_eq!(app.editor.images.len(), 1);

    // Second Ctrl+Z reverts [Image 1] cleanly.
    press_with(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert!(app.editor.is_empty());
}

#[test]
fn sessions_dialog_requests_filtered_history_without_clobbering_unfiltered_cache() {
    let (mut app, mut rx) = test_app_with_requests(false);
    let mut main = bg_session("ses_main", "main", SessionStatus::Idle, 1);
    main.updated_at = 400;
    let mut w1 = bg_session("ses_w1", "worker-1", SessionStatus::Idle, 0);
    w1.parent = Some("ses_main".into());
    w1.updated_at = 300;
    let mut h_recent = bg_session("ses_hr", "recent-hist", SessionStatus::Idle, 0);
    h_recent.live = false;
    h_recent.updated_at = 200;
    let mut h_deep = bg_session("ses_hd", "deep-child", SessionStatus::Idle, 0);
    h_deep.live = false;
    h_deep.parent = Some("ses_main".into());
    h_deep.title = "Deep history match".into();
    h_deep.updated_at = 50;

    app.current = Some("ses_main".into());
    app.live = vec![main, w1];
    app.history_list = vec![h_recent.clone()];
    let _ = drain_requests(&mut rx);

    // Opening dialog sends unfiltered ListHistory.
    app.open_sessions();
    let reqs = drain_requests(&mut rx);
    assert!(reqs.iter().any(|f| matches!(
        &f.req,
        Request::ListHistory {
            cwd: None,
            creator: None,
            query: None,
            limit: Some(_),
        }
    )));

    // Tab 1 -> UserCreated sends creator = Some("user").
    press(&mut app, KeyCode::Tab);
    let reqs = drain_requests(&mut rx);
    assert!(reqs.iter().any(|f| matches!(
        &f.req,
        Request::ListHistory {
            creator: Some(c),
            query: None,
            ..
        } if c == "user"
    )));

    // Tab 2 -> ByCreator selects worker-1's creator (ses_main) or cycles back if none selected;
    // select worker-1 first so Tab 2 picks ses_main.
    press(&mut app, KeyCode::Tab); // back to All
    let _ = drain_requests(&mut rx);
    if let Some(crate::ui::dialog::Dialog::List(d)) = app.dialog.as_mut() {
        d.select_key("live:ses_w1");
    }
    press(&mut app, KeyCode::Tab); // UserCreated (remembers ses_main)
    let _ = drain_requests(&mut rx);
    press(&mut app, KeyCode::Tab); // ByCreator (ses_main)
    let by_req = drain_requests(&mut rx)
        .into_iter()
        .find(|f| matches!(f.req, Request::ListHistory { .. }))
        .expect("filtered list_history sent");
    assert!(matches!(
        &by_req.req,
        Request::ListHistory {
            creator: Some(c),
            query: None,
            ..
        } if c == "ses_main"
    ));

    // Server returns a deep match outside `app.history_list`: dialog shows it, while
    // `app.history_list` stays intact for unfiltered fallback / autocomplete.
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: by_req.id.unwrap(),
        ok: true,
        result: json!({ "sessions": [h_deep] }),
        error: None,
        code: None,
    }));
    let (_, s_filtered) = render(&mut app, 120, 34);
    assert!(s_filtered.contains("Deep history match"), "{s_filtered}");
    assert_eq!(app.history_list.len(), 1);
    assert_eq!(app.history_list[0].id, h_recent.id);

    // Tab back to All re-fetches full history and immediately shows `recent-hist` from cache.
    press(&mut app, KeyCode::Tab);
    let reset_reqs = drain_requests(&mut rx);
    assert!(reset_reqs.iter().any(|f| matches!(
        &f.req,
        Request::ListHistory {
            creator: None,
            query: None,
            ..
        }
    )));
    let (_, s_all) = render(&mut app, 120, 34);
    assert!(s_all.contains("recent-hist"), "{s_all}");

    // Typing a query schedules debounced ListHistory with `query = Some(...)`.
    type_text(&mut app, "deep");
    assert!(app.sessions_due.is_some());
    assert!(
        !drain_requests(&mut rx)
            .iter()
            .any(|f| matches!(f.req, Request::ListHistory { .. }))
    );
    app.sessions_due = Some(std::time::Instant::now() - std::time::Duration::from_millis(1));
    app.housekeeping();
    let q_req = drain_requests(&mut rx)
        .into_iter()
        .find(|f| matches!(f.req, Request::ListHistory { .. }))
        .expect("debounced query sent");
    assert!(matches!(
        &q_req.req,
        Request::ListHistory {
            creator: None,
            query: Some(q),
            ..
        } if q == "deep"
    ));

    // Closing the dialog via Esc before the response arrives prevents stale overwrite;
    // reopening with `open_sessions_filtered` sends the initial query immediately.
    press(&mut app, KeyCode::Esc);
    assert!(app.dialog.is_none());
    app.open_sessions_filtered(Some("parser"));
    let open_q_req = drain_requests(&mut rx)
        .into_iter()
        .find(|f| matches!(f.req, Request::ListHistory { .. }))
        .expect("open_sessions_filtered sends list_history");
    assert!(matches!(
        &open_q_req.req,
        Request::ListHistory {
            creator: None,
            query: Some(q),
            ..
        } if q == "parser"
    ));
}

#[test]
fn shell_arg_complete_shell_cache_accept_flag_suppression_and_listdirs_fallback() {
    let (mut app, mut rx) = test_app_with_requests(false);
    let _ = drain_requests(&mut rx);

    // 1. Typing `!docker ` schedules debounced CompleteShell; housekeeping sends it.
    type_text(&mut app, "!docker ");
    assert!(app.shell_complete_due.is_some());
    assert!(
        !drain_requests(&mut rx)
            .iter()
            .any(|f| matches!(f.req, Request::CompleteShell { .. }))
    );
    app.shell_complete_due = Some((
        std::time::Instant::now() - std::time::Duration::from_millis(1),
        TEST_CWD.into(),
        "docker ".into(),
    ));
    app.housekeeping();
    let req = drain_requests(&mut rx)
        .into_iter()
        .find(|f| matches!(&f.req, Request::CompleteShell { line, .. } if line == "docker "))
        .expect("CompleteShell for `docker `");
    assert!(matches!(
        &req.req,
        Request::CompleteShell {
            cwd,
            cursor: Some(7),
            limit: Some(50),
            ..
        } if cwd == TEST_CWD
    ));
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: req.id.unwrap(),
        ok: true,
        result: json!({
            "items": [
                {"value": "compose", "description": "Define and run multi-container applications"},
                {"value": "build", "description": "Build an image"},
                {"value": "--help", "description": "Print usage"}
            ]
        }),
        error: None,
        code: None,
    }));
    let ac = app.ac.as_ref().expect("ShellArg popup");
    assert_eq!(ac.kind, crate::app::AcKind::ShellArg);
    assert!(ac.freeform_enter && !ac.explicitly_selected);
    assert_eq!(ac.items[0].0, "compose ");
    assert_eq!(ac.items[0].1, "compose");

    // 2. Typing `co` filters immediately from cache (0 flicker) while scheduling debounced query.
    type_text(&mut app, "co");
    let ac = app.ac.as_ref().expect("0-flicker cached ShellArg");
    assert_eq!(ac.kind, crate::app::AcKind::ShellArg);
    assert_eq!(ac.items.len(), 1);
    assert_eq!(ac.items[0].1, "compose");
    assert!(app.shell_complete_due.is_some());

    // Accepting `compose` appends trailing space and closes popup.
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.editor.text, "docker compose ");
    assert!(app.ac.is_none());

    // 3. Item ending with `=` inserts without trailing space and closes popup.
    app.editor.clear();
    type_text(&mut app, "git --git");
    press(&mut app, KeyCode::Tab); // 0ms debounce on Tab
    let git_req = drain_requests(&mut rx)
        .into_iter()
        .find(|f| matches!(&f.req, Request::CompleteShell { line, .. } if line == "git --git"))
        .expect("immediate CompleteShell on Tab");
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: git_req.id.unwrap(),
        ok: true,
        result: json!({ "items": [{"value": "--git-dir=", "description": "Set path to repository"}] }),
        error: None,
        code: None,
    }));
    assert_eq!(app.ac.as_ref().unwrap().kind, crate::app::AcKind::ShellArg);
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.editor.text, "git --git-dir=");
    assert!(app.ac.is_none());

    // 4. Item ending with `/` inserts without space and keeps open (refreshing into ShellPath).
    app.editor.clear();
    type_text(&mut app, "git checkout sr");
    press(&mut app, KeyCode::Tab);
    let step4_reqs = drain_requests(&mut rx);
    if let Some(pending_dir) = step4_reqs
        .iter()
        .find(|f| matches!(&f.req, Request::ListDirs { path, .. } if path == "."))
    {
        app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
            id: pending_dir.id.unwrap(),
            ok: false,
            result: serde_json::Value::Null,
            error: Some("cancelled".into()),
            code: None,
        }));
    }
    let branch_req = step4_reqs
        .into_iter()
        .find(
            |f| matches!(&f.req, Request::CompleteShell { line, .. } if line == "git checkout sr"),
        )
        .expect("CompleteShell for git checkout sr");
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: branch_req.id.unwrap(),
        ok: true,
        result: json!({ "items": [{"value": "src/"}] }),
        error: None,
        code: None,
    }));
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.editor.text, "git checkout src/");
    let src_dir_req = drain_requests(&mut rx)
        .into_iter()
        .find(|f| matches!(&f.req, Request::ListDirs { path, .. } if path == "src"))
        .expect("accepting trailing `/` ShellArg keeps open and queries ListDirs");
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: src_dir_req.id.unwrap(),
        ok: false,
        result: serde_json::Value::Null,
        error: Some("cancelled".into()),
        code: None,
    }));

    // 5. `-`-prefixed flag with empty CompleteShell closes popup (NO ListDirs fallback).
    app.editor.clear();
    type_text(&mut app, "docker --zzz");
    press(&mut app, KeyCode::Tab);
    let flag_req = drain_requests(&mut rx)
        .into_iter()
        .find(|f| matches!(&f.req, Request::CompleteShell { line, .. } if line == "docker --zzz"))
        .expect("CompleteShell for docker --zzz");
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: flag_req.id.unwrap(),
        ok: true,
        result: json!({ "items": [] }),
        error: None,
        code: None,
    }));
    assert!(app.ac.is_none());
    assert!(
        drain_requests(&mut rx)
            .iter()
            .all(|f| !matches!(&f.req, Request::ListDirs { .. })),
        "empty `-` flag completion must NOT fall back to ListDirs"
    );

    // 6. Non-flag arg shows ListDirs fallback while CompleteShell is pending (0-flicker) and on empty reply.
    app.editor.clear();
    type_text(&mut app, "ls foo");
    press(&mut app, KeyCode::Tab);
    let reqs = drain_requests(&mut rx);
    let ls_req = reqs
        .iter()
        .find(|f| matches!(&f.req, Request::CompleteShell { line, .. } if line == "ls foo"))
        .expect("CompleteShell for ls foo");
    let dir_req = reqs
        .iter()
        .find(|f| matches!(&f.req, Request::ListDirs { path, .. } if path == "."))
        .expect("ListDirs fallback sent while CompleteShell is pending");
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: dir_req.id.unwrap(),
        ok: true,
        result: json!({ "path": TEST_CWD, "dirs": [], "files": ["foobar.txt"] }),
        error: None,
        code: None,
    }));
    assert_eq!(app.ac.as_ref().unwrap().kind, crate::app::AcKind::ShellPath);
    assert_eq!(app.ac.as_ref().unwrap().items[0].1, "foobar.txt");
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: ls_req.id.unwrap(),
        ok: true,
        result: json!({ "items": [] }),
        error: None,
        code: None,
    }));
    assert_eq!(app.ac.as_ref().unwrap().kind, crate::app::AcKind::ShellPath);
    assert_eq!(app.ac.as_ref().unwrap().items[0].1, "foobar.txt");

    // 7. Path arg token (`ls src/`) skips CompleteShell and goes directly to ListDirs.
    app.editor.clear();
    type_text(&mut app, "ls src/");
    let reqs = drain_requests(&mut rx);
    assert!(
        reqs.iter()
            .all(|f| !matches!(&f.req, Request::CompleteShell { .. }))
    );
    let step7_dir_req = reqs
        .into_iter()
        .find(|f| matches!(&f.req, Request::ListDirs { path, .. } if path == "src"))
        .expect("path arg token queries ListDirs directly");
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: step7_dir_req.id.unwrap(),
        ok: false,
        result: serde_json::Value::Null,
        error: Some("cancelled".into()),
        code: None,
    }));

    // 8. Flags containing `/` or `.` (`--manifest-path=sub/`, `-I./inc`) route through CompleteShell, NOT ListDirs.
    for flag_cmd in ["cargo --manifest-path=sub/", "cc -I./inc"] {
        app.editor.clear();
        type_text(&mut app, flag_cmd);
        press(&mut app, KeyCode::Tab);
        let reqs = drain_requests(&mut rx);
        assert!(
            reqs.iter()
                .all(|f| !matches!(&f.req, Request::ListDirs { .. })),
            "{flag_cmd} must not query ListDirs"
        );
        let f_req = reqs
            .into_iter()
            .find(|f| matches!(&f.req, Request::CompleteShell { line, .. } if line == flag_cmd))
            .expect("flag with path chars queries CompleteShell");
        app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
            id: f_req.id.unwrap(),
            ok: true,
            result: json!({ "items": [] }),
            error: None,
            code: None,
        }));
        assert!(app.ac.is_none());
    }

    // 9a. CompleteShell error on prefix matching cached `foobar.txt` falls back to ListDirs (ShellPath).
    app.editor.clear();
    type_text(&mut app, "git foo");
    assert_eq!(app.ac.as_ref().unwrap().kind, crate::app::AcKind::ShellPath);
    assert!(app.shell_complete_due.is_some());
    app.shell_complete_due = Some((
        std::time::Instant::now() - std::time::Duration::from_millis(1),
        TEST_CWD.into(),
        "git foo".into(),
    ));
    app.housekeeping();
    let err_match_req = drain_requests(&mut rx)
        .into_iter()
        .find(|f| matches!(&f.req, Request::CompleteShell { line, .. } if line == "git foo"))
        .unwrap();
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: err_match_req.id.unwrap(),
        ok: false,
        result: serde_json::Value::Null,
        error: Some("timeout".into()),
        code: None,
    }));
    assert_eq!(app.ac.as_ref().unwrap().kind, crate::app::AcKind::ShellPath);
    assert_eq!(app.ac.as_ref().unwrap().items[0].1, "foobar.txt");

    // 9b. CompleteShell error on prefix with no matching cached files leaves ac None and Tab retries CompleteShell.
    app.editor.clear();
    type_text(&mut app, "git ch");
    press(&mut app, KeyCode::Tab);
    let err_req = drain_requests(&mut rx)
        .into_iter()
        .find(|f| matches!(&f.req, Request::CompleteShell { line, .. } if line == "git ch"))
        .unwrap();
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: err_req.id.unwrap(),
        ok: false,
        result: serde_json::Value::Null,
        error: Some("timeout".into()),
        code: None,
    }));
    assert!(app.ac.is_none());
    press(&mut app, KeyCode::Tab);
    let retry_req = drain_requests(&mut rx)
        .into_iter()
        .find(|f| matches!(&f.req, Request::CompleteShell { line, .. } if line == "git ch"))
        .expect("Tab retries CompleteShell after prior error");
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: retry_req.id.unwrap(),
        ok: true,
        result: json!({ "items": [{"value": "checkout", "description": "Switch branches"}] }),
        error: None,
        code: None,
    }));
    assert_eq!(app.ac.as_ref().unwrap().kind, crate::app::AcKind::ShellArg);
    // Leaving shell mode clears cached CompleteShell entries.
    app.editor.clear();
    app.exit_shell_mode();
    type_text(&mut app, "!git ch");
    press(&mut app, KeyCode::Tab);
    assert!(
        drain_requests(&mut rx)
            .iter()
            .any(|f| matches!(&f.req, Request::CompleteShell { line, .. } if line == "git ch")),
        "cache cleared after exiting shell mode"
    );
    assert!(
        app.dirs_inflight.is_empty(),
        "stale dirs_inflight: {:?}",
        app.dirs_inflight
    );
}

#[test]
fn empty_startup_welcome_draft_config_and_lazy_create() {
    use crate::app::Pending;
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut app = App::new(Client::new(tx), TEST_CWD.into(), InitPlan::default());
    app.on_net(crate::net::NetEvent::Connected);
    // Empty startup: no CreateSession sent on connect, current stays None.
    let frames = drain_requests(&mut rx);
    assert!(
        !frames
            .iter()
            .any(|f| matches!(f.req, Request::CreateSession { .. })),
        "no eager CreateSession on connect"
    );
    assert!(app.current.is_none());

    // Provide config and edit draft agent / model / variant while current == None.
    respond(
        &mut app,
        |p| matches!(p, Pending::Config { .. }),
        json!({
            "config": {
                "agents": [{ "name": "plan", "description": "Planner", "model": null, "creatable": true, "can_spawn": false, "color": null }],
                "default_agent": "plan",
                "models": [{ "provider": "dgn", "provider_name": "DGN", "id": "opus", "name": "Opus", "context": 100_000, "output": 0, "images": true, "reasoning": true, "variants": ["low", "high"] }],
                "commands": [],
                "mcp": [],
                "tui": null,
                "warnings": []
            }
        }),
    );
    type_text(&mut app, "/agent plan");
    press(&mut app, KeyCode::Enter);
    type_text(&mut app, "/model opus");
    press(&mut app, KeyCode::Enter);
    type_text(&mut app, "/variant high");
    press(&mut app, KeyCode::Enter);
    assert!(app.current.is_none());

    let (_, s) = render(&mut app, 110, 30);
    assert!(s.contains("Switch Backend…"), "{s}");
    assert!(s.contains("Plan · Opus DGN · high"), "{s}");
    assert!(!app.switch_backend_rects.is_empty());

    // Submitting first prompt lazily creates session with draft config.
    type_text(&mut app, "hello draft");
    press(&mut app, KeyCode::Enter);
    let frames = drain_requests(&mut rx);
    assert!(frames.iter().any(|f| matches!(
        &f.req,
        Request::CreateSession { agent, model: Some(m), .. }
            if agent.as_deref() == Some("plan") && m.model == "opus" && m.variant.as_deref() == Some("high")
    )));
}

#[test]
fn continue_without_history_warns_and_stays_empty() {
    use crate::app::Pending;
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut app = App::new(
        Client::new(tx),
        TEST_CWD.into(),
        InitPlan {
            continue_last: true,
            ..Default::default()
        },
    );
    app.on_net(crate::net::NetEvent::Connected);
    let _ = drain_requests(&mut rx);
    respond(
        &mut app,
        |p| matches!(p, Pending::Continue),
        json!({ "sessions": [] }),
    );
    assert!(app.current.is_none());
    assert!(
        !app.pending
            .values()
            .any(|p| matches!(p, Pending::Created { .. }))
    );
    assert!(
        app.toasts
            .items
            .iter()
            .any(|t| t.text.contains("No previous session"))
    );
}

#[test]
fn retry_now_banner_click_sends_retry_now_request() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let (mut app, mut rx) = test_app_with_requests(false);
    set_status(&mut app, TEST_SESSION, retrying());
    let (term, s) = render(&mut app, 100, 30);
    assert!(s.contains("↻ retry now"), "{s}");
    let y = row_of(&s, "↻ retry now") as u16;
    let x = find_col(&term, y, "↻").unwrap();
    app.screen = ratatui::layout::Rect::new(0, 0, 100, 30);
    let _ = drain_requests(&mut rx);
    send_mouse(&mut app, MouseEventKind::Down(MouseButton::Left), x + 1, y);
    send_mouse(&mut app, MouseEventKind::Up(MouseButton::Left), x + 1, y);
    assert!(
        drain_requests(&mut rx).iter().any(
            |f| matches!(&f.req, Request::RetryNow { session_id } if session_id == TEST_SESSION)
        )
    );
}

#[test]
fn watch_direct_parent_filter_pill_cycles_and_filters_tiles() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = test_app(false);
    app.mode = Mode::Watch;
    let mut parent = bg_session("ses_p", "root-agent", SessionStatus::Idle, 0);
    parent.created_at = 1;
    let mut child_direct = bg_session("ses_c1", "sub-direct", SessionStatus::Idle, 0);
    child_direct.parent = Some("ses_p".into());
    child_direct.created_at = 2;
    let mut grandchild = bg_session("ses_gc", "sub-grandchild", SessionStatus::Idle, 0);
    grandchild.parent = Some("ses_c1".into());
    grandchild.created_at = 3;
    app.live = vec![parent, child_direct, grandchild];
    for s in &app.live {
        app.views
            .insert(s.id.clone(), SessionView::new(s.clone(), false));
    }

    let (_, s0) = render(&mut app, 120, 30);
    assert!(s0.contains(" All "), "{s0}");
    assert_eq!(app.tile_ids(), vec!["ses_p", "ses_c1", "ses_gc"]);
    let pill = app.watch.filter_pill_rect.expect("filter pill rect");

    // Click pill: All -> User-created (only ses_p).
    send_mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        pill.x + 1,
        pill.y,
    );
    let (_, s1) = render(&mut app, 120, 30);
    assert!(s1.contains(" User-created "), "{s1}");
    assert_eq!(app.tile_ids(), vec!["ses_p"]);

    // Press `f` unfocused: User-created -> By root-agent (only direct child ses_c1, not grandchild).
    press(&mut app, KeyCode::Char('f'));
    let (_, s2) = render(&mut app, 120, 30);
    assert!(s2.contains(" By root-agent "), "{s2}");
    assert_eq!(app.tile_ids(), vec!["ses_c1"]);
}

#[test]
fn stash_dialog_and_backend_switch_reset_state() {
    let (mut app, mut rx) = test_app_with_requests(false);
    let info = app.views[TEST_SESSION].info.clone();
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::GroupsChanged {
        groups: vec![StashGroup {
            id: "grp_1".into(),
            name: "work".into(),
            locked: false,
            created_at: 10,
            updated_at: 20,
            members: vec![StashMember {
                session_id: TEST_SESSION.into(),
                position: 0,
                info,
            }],
        }],
    }));
    app.run_command("stashed", "");
    let (_, s) = render(&mut app, 110, 30);
    assert!(s.contains("Stashed groups") && s.contains("work"), "{s}");
    press(&mut app, KeyCode::Char('R'));
    assert!(
        drain_requests(&mut rx)
            .iter()
            .any(|f| matches!(&f.req, Request::RestoreGroup { group_id } if group_id == "grp_1"))
    );
    press(&mut app, KeyCode::Esc);
    assert!(app.dialog.is_none());

    // Backend switch clears current/views/groups and disconnects cleanly.
    app.switch_backend(
        ref_client::BackendTarget::Explicit {
            url: "ws://127.0.0.1:9999".into(),
            token: None,
        },
        crate::prefs::BackendSource::Temporary,
    );
    assert!(app.current.is_none());
    assert!(app.views.is_empty());
    assert!(app.groups.is_empty());
    assert!(!app.connected);
}

#[test]
fn window_zero_auto_loads_all_placeholder_chunks_and_skips_eviction() {
    let (mut app, old, _) = history_app_with_count(650, 20); // 1300 old placeholders + 40 newest = 1340
    app.connected = true;
    assert_eq!(app.views[TEST_SESSION].resident_count(), 40);

    app.run_command("window", "0");
    assert_eq!(app.message_window_size, 0);
    assert!(app.views[TEST_SESSION].is_window_unlimited());

    // First chunk: [300, 1300) (1000 messages = PAGE_MAX).
    let (id1, start1, end1) = range_request(&app).expect("first unlimited chunk requested");
    assert_eq!((start1, end1), (300, 1300));
    respond_to_range(
        &mut app,
        id1,
        json!({
            "messages": old[300..1300].to_vec(),
            "more_before": true,
            "start_seq": 300,
            "end_seq": 1300,
        }),
    );
    assert_eq!(app.views[TEST_SESSION].resident_count(), 1040);

    // Second chunk: [0, 300).
    let (id2, start2, end2) = range_request(&app).expect("second unlimited chunk requested");
    assert_eq!((start2, end2), (0, 300));
    respond_to_range(
        &mut app,
        id2,
        json!({
            "messages": old[0..300].to_vec(),
            "more_before": false,
            "start_seq": 0,
            "end_seq": 300,
        }),
    );
    assert_eq!(app.views[TEST_SESSION].resident_count(), 1340);
    assert!(range_request(&app).is_none());

    // Rendering while following must not evict when unlimited.
    let _ = render(&mut app, 100, 30);
    assert_eq!(app.views[TEST_SESSION].resident_count(), 1340);
}

#[test]
fn window_command_sets_resets_and_preserves_backend_prefs() {
    use crate::prefs::{TuiPrefs, load_prefs_from, save_prefs_to};
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("ref-tui-window-{}-{nanos}", std::process::id()));
    let prefs_path = dir.join("tui.json");
    save_prefs_to(
        &prefs_path,
        &TuiPrefs {
            backend_url: Some("wss://remote.example/ws".into()),
            backend_token: Some("tok-123".into()),
            message_window_size: None,
        },
    )
    .unwrap();

    let (mut app, ..) = history_app_with_count(0, 160); // 320 resident messages
    app.prefs_path = Some(prefs_path.clone());

    app.run_command("window", "");
    assert!(
        app.toasts
            .items
            .iter()
            .any(|t| t.text.contains("Message window: 1000"))
    );

    app.run_command("window", "250");
    assert_eq!(app.message_window_size, 250);
    assert_eq!(app.views[TEST_SESSION].message_window_size, 250);
    assert_eq!(app.views[TEST_SESSION].resident_count(), 250);
    let saved = load_prefs_from(&prefs_path).unwrap();
    assert_eq!(
        saved.backend_url.as_deref(),
        Some("wss://remote.example/ws")
    );
    assert_eq!(saved.backend_token.as_deref(), Some("tok-123"));
    assert_eq!(saved.message_window_size, Some(250));

    app.run_command("window", "reset");
    assert_eq!(app.message_window_size, crate::state::MAX_RESIDENT_MESSAGES);
    let after_reset = load_prefs_from(&prefs_path).unwrap();
    assert_eq!(
        after_reset.backend_url.as_deref(),
        Some("wss://remote.example/ws")
    );
    assert_eq!(after_reset.backend_token.as_deref(), Some("tok-123"));
    assert_eq!(after_reset.message_window_size, None);

    app.run_command("window", "not-a-number");
    assert!(
        app.toasts
            .items
            .iter()
            .any(|t| t.text.contains("Usage: /window <count|0|reset>"))
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn handle_backend_action_preserves_message_window_size() {
    use crate::prefs::load_prefs_from;
    use crate::ui::dialog::BackendAction;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "ref-tui-backend-prefs-{}-{nanos}",
        std::process::id()
    ));
    let prefs_path = dir.join("tui.json");

    let mut app = test_app(false).with_message_window_size(2500);
    app.prefs_path = Some(prefs_path.clone());

    app.handle_backend_action(BackendAction::SaveDefault {
        url: "wss://saved.example/ws".into(),
        token: Some("secret".into()),
    });
    let saved = load_prefs_from(&prefs_path).unwrap();
    assert_eq!(saved.backend_url.as_deref(), Some("wss://saved.example/ws"));
    assert_eq!(saved.backend_token.as_deref(), Some("secret"));
    assert_eq!(saved.message_window_size, Some(2500));
    assert_eq!(app.message_window_size, 2500);

    app.handle_backend_action(BackendAction::RestoreLocal);
    let restored = load_prefs_from(&prefs_path).unwrap();
    assert_eq!(restored.backend_url, None);
    assert_eq!(restored.backend_token, None);
    assert_eq!(restored.message_window_size, Some(2500));
    assert_eq!(app.message_window_size, 2500);

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn welcome_logo_rows_align_vertically() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut app = App::new(Client::new(tx), TEST_CWD.into(), InitPlan::default());
    app.on_net(crate::net::NetEvent::Connected);
    assert!(app.current.is_none());

    for w in [100, 101] {
        let (term, s) = render(&mut app, w, 30);
        let y0 = row_of(&s, crate::app::LOGO[0].trim_end()) as u16;
        let cols: Vec<u16> = (0..3)
            .map(|i| find_col(&term, y0 + i, "█").expect("logo row"))
            .collect();
        assert_eq!(cols, vec![cols[0]; 3], "width {w} logo misaligned:\n{s}");
    }
}

#[test]
fn theme_assign_root_colors_handles_collisions_balance_empty_and_single() {
    use crate::ui::theme;
    use std::collections::HashSet;

    // Construction `prefix + cc` has identical `hash % 8` for all ASCII `c` because 31*31 ≡ 1 and 31*c+c = 32*c ≡ 0 (mod 8).
    let colliding_8 = [
        "ses_col_aa",
        "ses_col_bb",
        "ses_col_cc",
        "ses_col_dd",
        "ses_col_ee",
        "ses_col_ff",
        "ses_col_gg",
        "ses_col_hh",
    ];
    for id in &colliding_8 {
        assert_eq!(
            theme::root_task_color(id),
            theme::root_task_color("ses_col_aa")
        );
    }
    let assigned_8 = theme::assign_root_colors(colliding_8.iter().copied());
    assert_eq!(assigned_8.len(), 8);
    let distinct: HashSet<_> = assigned_8.values().copied().collect();
    assert_eq!(distinct.len(), 8);

    let many = [
        "ses_col_aa",
        "ses_col_bb",
        "ses_col_cc",
        "ses_col_dd",
        "ses_col_ee",
        "ses_col_ff",
        "ses_col_gg",
        "ses_col_hh",
        "ses_col_ii",
        "ses_col_jj",
    ];
    let assigned_many = theme::assign_root_colors(many.iter().copied());
    assert_eq!(assigned_many.len(), 10);
    let counts: Vec<usize> = theme::ROOT_TASK_PALETTE
        .iter()
        .map(|c| assigned_many.values().filter(|v| *v == c).count())
        .collect();
    let max_c = *counts.iter().max().unwrap();
    let min_c = *counts.iter().min().unwrap();
    assert!(max_c - min_c <= 1, "unbalanced slots: {counts:?}");

    let deduped = theme::assign_root_colors(["", "ses_col_aa", "ses_col_aa", "", "ses_col_bb"]);
    assert_eq!(deduped.len(), 2);
    assert!(!deduped.contains_key(""));
    assert_ne!(deduped["ses_col_aa"], deduped["ses_col_bb"]);

    let single = theme::assign_root_colors(["ses_single"]);
    assert_eq!(
        single.get("ses_single").copied(),
        Some(theme::root_task_color("ses_single"))
    );
}

#[test]
fn split_view_colliding_roots_render_distinct_border_and_chip_colors() {
    use crate::ui::theme;
    assert_eq!(
        theme::root_task_color("ses_col_aa"),
        theme::root_task_color("ses_col_bb")
    );
    let mut app = test_app(false);
    app.mode = Mode::Watch;
    let mut r1 = bg_session("ses_col_aa", "root-one", SessionStatus::Idle, 0);
    r1.live = false;
    let mut r2 = bg_session("ses_col_bb", "root-two", SessionStatus::Idle, 0);
    r2.live = false;
    app.history_list = vec![r1, r2];

    let mut c1 = bg_session("ses_c1", "child-one", SessionStatus::Idle, 0);
    c1.parent = Some("ses_col_aa".into());
    c1.created_at = 1;
    let mut c2 = bg_session("ses_c2", "child-two", SessionStatus::Idle, 0);
    c2.parent = Some("ses_col_bb".into());
    c2.created_at = 2;
    app.views
        .insert("ses_c1".into(), SessionView::new(c1.clone(), false));
    app.views
        .insert("ses_c2".into(), SessionView::new(c2.clone(), false));
    app.live = vec![c1, c2];
    app.watch.focus = None;

    let (term, s) = render(&mut app, 140, 36);
    assert!(s.contains("child-one ⟲ root-one"), "{s}");
    assert!(s.contains("child-two ⟲ root-two"), "{s}");
    let buf = term.backend().buffer();
    let rc1 = tile_rect(&app, "ses_c1");
    let rc2 = tile_rect(&app, "ses_c2");
    let b1 = buf[(rc1.x, rc1.y)].fg;
    let b2 = buf[(rc2.x, rc2.y)].fg;
    assert_ne!(b1, b2);
    let chip_fg = |r: ratatui::layout::Rect| {
        (r.x..r.x + r.width)
            .find(|&x| buf[(x, r.y)].symbol() == "⟲")
            .map(|x| buf[(x, r.y)].fg)
            .expect("⟲ chip present")
    };
    let chip1 = chip_fg(rc1);
    let chip2 = chip_fg(rc2);
    assert_eq!(chip1, b1);
    assert_eq!(chip2, b2);
    assert_ne!(chip1, chip2);
}

#[test]
fn stash_dialog_member_accent_bars_resolve_creator_collisions() {
    use crate::ui::dialog::Dialog;
    use crate::ui::theme;
    assert_eq!(
        theme::root_task_color("ses_col_aa"),
        theme::root_task_color("ses_col_bb")
    );
    let mut app = test_app(false);
    let mut m1 = bg_session("ses_m1", "worker-1", SessionStatus::Idle, 0);
    m1.parent = Some("ses_col_aa".into());
    let mut m2 = bg_session("ses_m2", "worker-2", SessionStatus::Idle, 0);
    m2.parent = Some("ses_col_bb".into());
    let mut m3 = bg_session("ses_m3", "worker-3", SessionStatus::Idle, 0);
    m3.parent = Some("ses_col_aa".into());
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::GroupsChanged {
        groups: vec![StashGroup {
            id: "grp_1".into(),
            name: "work".into(),
            locked: false,
            created_at: 10,
            updated_at: 20,
            members: vec![
                StashMember {
                    session_id: "ses_m1".into(),
                    position: 0,
                    info: m1,
                },
                StashMember {
                    session_id: "ses_m2".into(),
                    position: 1,
                    info: m2,
                },
                StashMember {
                    session_id: "ses_m3".into(),
                    position: 2,
                    info: m3,
                },
            ],
        }],
    }));
    app.run_command("stashed", "");
    if let Some(Dialog::Stash(d)) = app.dialog.as_mut() {
        d.opened = std::time::Instant::now() - std::time::Duration::from_secs(1);
    }
    let (term, s) = render(&mut app, 110, 30);
    assert!(
        s.contains("worker-1") && s.contains("worker-2") && s.contains("worker-3"),
        "{s}"
    );
    let members_rect = match app.dialog.as_ref() {
        Some(Dialog::Stash(d)) => d.members_rect,
        _ => panic!("expected stash dialog"),
    };
    let buf = term.backend().buffer();
    let x = members_rect.x;
    let (y0, y1, y2) = (members_rect.y + 1, members_rect.y + 2, members_rect.y + 3);
    assert_eq!(buf[(x, y0)].symbol(), "▍");
    assert_eq!(buf[(x, y1)].symbol(), "▍");
    assert_eq!(buf[(x, y2)].symbol(), "▍");
    let (c0, c1, c2) = (buf[(x, y0)].fg, buf[(x, y1)].fg, buf[(x, y2)].fg);
    assert_ne!(c0, c1);
    assert_eq!(c0, c2);

    press(&mut app, KeyCode::Tab);
    let (term_active, _) = render(&mut app, 110, 30);
    let buf_active = term_active.backend().buffer();
    assert_eq!(buf_active[(x, y0)].fg, theme::PANEL);
    assert_eq!(buf_active[(x, y1)].fg, c1);
    assert_eq!(buf_active[(x, y2)].fg, c2);
}

#[test]
fn sidebar_live_list_sorts_pinned_first_and_renders_pin_marker() {
    let mut app = test_app(true);
    let mut u0 = bg_session("ses_u0", "unpinned-early", SessionStatus::Idle, 0);
    u0.created_at = 5;
    let mut u1 = bg_session("ses_u1", "unpinned-late", SessionStatus::Idle, 0);
    u1.created_at = 10;
    let mut p1 = bg_session("ses_p1", "pinned-older", SessionStatus::Idle, 0);
    p1.created_at = 20;
    p1.pinned_at = Some(100);
    let mut p2 = bg_session("ses_p2", "pinned-newer", SessionStatus::Idle, 0);
    p2.created_at = 30;
    p2.pinned_at = Some(200);
    app.sidebar = true;
    app.live = vec![u1, p1, u0, p2];

    let (_, s) = render(&mut app, 160, 30);
    let ids: Vec<&str> = app.sidebar_rows.iter().map(|(_, id)| id.as_str()).collect();
    assert_eq!(ids, vec!["ses_p2", "ses_p1", "ses_u0", "ses_u1"]);
    assert!(s.contains("✦ pinned-newer"), "{s}");
    assert!(s.contains("✦ pinned-older"), "{s}");
    assert!(!s.contains("✦ unpinned-early"), "{s}");
    assert!(!s.contains("✦ unpinned-late"), "{s}");
}

#[test]
fn sessions_dialog_sorts_pinned_first_renders_marker_and_ctrl_k_pins_live_and_history() {
    use crate::ui::dialog::Dialog;
    let (mut app, mut rx) = test_app_with_requests(false);
    let mut lu = bg_session("ses_lu", "live-unpinned", SessionStatus::Idle, 0);
    lu.updated_at = 500;
    let mut lp1 = bg_session("ses_lp1", "live-pin-old", SessionStatus::Idle, 0);
    lp1.updated_at = 100;
    lp1.pinned_at = Some(100);
    let mut lp2 = bg_session("ses_lp2", "live-pin-new", SessionStatus::Idle, 0);
    lp2.updated_at = 200;
    lp2.pinned_at = Some(200);
    let mut hu = bg_session("ses_hu", "hist-u", SessionStatus::Idle, 0);
    hu.live = false;
    hu.title = "Hist unpinned".into();
    hu.updated_at = 900;
    let mut hp = bg_session("ses_hp", "hist-p", SessionStatus::Idle, 0);
    hp.live = false;
    hp.title = "Hist pinned".into();
    hp.updated_at = 50;
    hp.pinned_at = Some(300);
    app.live = vec![lu.clone(), lp1, lp2];
    app.history_list = vec![hu, hp.clone()];
    app.open_sessions();
    if let Some(Dialog::List(d)) = app.dialog.as_mut() {
        d.opened = std::time::Instant::now() - std::time::Duration::from_secs(1);
        let keys: Vec<&str> = d.items.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "live:ses_lp2",
                "live:ses_lp1",
                "live:ses_lu",
                "hist:ses_hp",
                "hist:ses_hu",
            ]
        );
        let pinned_flags: Vec<bool> = d.items.iter().map(|i| i.pinned).collect();
        assert_eq!(pinned_flags, vec![true, true, false, true, false]);
    } else {
        panic!("expected sessions dialog");
    }
    let (_, s) = render(&mut app, 120, 34);
    assert!(s.contains("✦ live-pin-new"), "{s}");
    assert!(s.contains("✦ Hist pinned"), "{s}");
    assert!(!s.contains("✦ live-unpinned"), "{s}");
    assert!(!s.contains("✦ Hist unpinned"), "{s}");
    let _ = drain_requests(&mut rx);

    // Ctrl+K on an unpinned live row sends SetPinned { pinned: true }.
    if let Some(Dialog::List(d)) = app.dialog.as_mut() {
        d.select_key("live:ses_lu");
    }
    press_with(&mut app, KeyCode::Char('k'), KeyModifiers::CONTROL);
    let live_pin_req = drain_requests(&mut rx)
        .into_iter()
        .find(|f| matches!(f.req, Request::SetPinned { .. }))
        .expect("ctrl+k on live row sends SetPinned");
    assert!(matches!(
        &live_pin_req.req,
        Request::SetPinned { session_id, pinned: true } if session_id == "ses_lu"
    ));
    let mut lu_pinned = lu;
    lu_pinned.pinned_at = Some(999);
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: live_pin_req.id.unwrap(),
        ok: true,
        result: json!({ "session": lu_pinned }),
        error: None,
        code: None,
    }));
    if let Some(Dialog::List(d)) = app.dialog.as_ref() {
        assert_eq!(d.items[0].key, "live:ses_lu");
        assert!(d.items[0].pinned);
    }

    // Ctrl+K on a pinned history row sends SetPinned { pinned: false }.
    if let Some(Dialog::List(d)) = app.dialog.as_mut() {
        d.select_key("hist:ses_hp");
    }
    press_with(&mut app, KeyCode::Char('k'), KeyModifiers::CONTROL);
    let hist_unpin_req = drain_requests(&mut rx)
        .into_iter()
        .find(|f| matches!(f.req, Request::SetPinned { .. }))
        .expect("ctrl+k on history row sends SetPinned");
    assert!(matches!(
        &hist_unpin_req.req,
        Request::SetPinned { session_id, pinned: false } if session_id == "ses_hp"
    ));
    let mut hp_unpinned = hp;
    hp_unpinned.pinned_at = None;
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: hist_unpin_req.id.unwrap(),
        ok: true,
        result: json!({ "session": hp_unpinned }),
        error: None,
        code: None,
    }));
    if let Some(Dialog::List(d)) = app.dialog.as_ref() {
        let hist_keys: Vec<&str> = d
            .items
            .iter()
            .filter(|i| i.key.starts_with("hist:"))
            .map(|i| i.key.as_str())
            .collect();
        assert_eq!(hist_keys, vec!["hist:ses_hu", "hist:ses_hp"]);
    }
}

#[test]
fn pin_command_toggles_current_and_named_sessions() {
    let (mut app, mut rx) = test_app_with_requests(false);
    let _ = drain_requests(&mut rx);

    app.run_command("pin", "");
    let pin_req = drain_requests(&mut rx)
        .into_iter()
        .find(|f| matches!(f.req, Request::SetPinned { .. }))
        .expect("/pin sends SetPinned");
    assert!(matches!(
        &pin_req.req,
        Request::SetPinned { session_id, pinned: true } if session_id == TEST_SESSION
    ));
    let mut cur_pinned = app.views[TEST_SESSION].info.clone();
    cur_pinned.pinned_at = Some(500);
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id: pin_req.id.unwrap(),
        ok: true,
        result: json!({ "session": cur_pinned }),
        error: None,
        code: None,
    }));
    assert_eq!(app.views[TEST_SESSION].info.pinned_at, Some(500));

    app.run_command("pin", "");
    let unpin_req = drain_requests(&mut rx)
        .into_iter()
        .find(|f| matches!(f.req, Request::SetPinned { .. }))
        .expect("second /pin unpins current session");
    assert!(matches!(
        &unpin_req.req,
        Request::SetPinned { session_id, pinned: false } if session_id == TEST_SESSION
    ));

    let mut hist = bg_session("ses_hist_named", "arch-notes", SessionStatus::Idle, 0);
    hist.live = false;
    app.history_list = vec![hist];
    app.run_command("pin", "arch-notes");
    let named_req = drain_requests(&mut rx)
        .into_iter()
        .find(|f| matches!(f.req, Request::SetPinned { .. }))
        .expect("/pin <name> sends SetPinned");
    assert!(matches!(
        &named_req.req,
        Request::SetPinned { session_id, pinned: true } if session_id == "ses_hist_named"
    ));

    app.current = None;
    app.run_command("pin", "");
    assert!(
        !drain_requests(&mut rx)
            .iter()
            .any(|f| matches!(f.req, Request::SetPinned { .. }))
    );
    let (_, s) = render(&mut app, 100, 30);
    assert!(s.contains("No active session to pin"), "{s}");
}

#[test]
fn history_changed_frame_triggers_list_history_refresh() {
    let (mut app, mut rx) = test_app_with_requests(false);
    app.history_list = vec![bg_session("ses_h1", "h1", SessionStatus::Idle, 0)];
    let _ = drain_requests(&mut rx);

    app.on_net(crate::net::NetEvent::Frame(ServerFrame::HistoryChanged));
    assert!(
        drain_requests(&mut rx)
            .iter()
            .any(|f| matches!(f.req, Request::ListHistory { .. })),
        "HistoryChanged refreshes cached history"
    );

    app.history_list.clear();
    app.open_sessions();
    let _ = drain_requests(&mut rx);
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::HistoryChanged));
    assert!(
        drain_requests(&mut rx)
            .iter()
            .any(|f| matches!(f.req, Request::ListHistory { .. })),
        "HistoryChanged refreshes open /sessions dialog"
    );
}
