//! Headless rendering tests (ratatui TestBackend). Run with `--nocapture` to see the screens.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ref_proto::*;
use serde_json::json;
use tokio::sync::mpsc;

use crate::app::{App, InitPlan, Mode};
use crate::net::Client;
use crate::state::SessionView;
use crate::ui::text;

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

fn info() -> SessionInfo {
    SessionInfo {
        id: "ses_test".into(),
        name: "ref-a1b2".into(),
        title: "Fix the parser".into(),
        agent: "build".into(),
        model: ModelRef {
            provider: "dgn".into(),
            model: "claude-opus-5.5".into(),
            variant: Some("high".into()),
        },
        cwd: "/home/u/proj".into(),
        created_at: 0,
        updated_at: 0,
        status: SessionStatus::Idle,
        attached: 1,
        live: true,
        parent: None,
        must_report: false,
        context: ContextUsage {
            used: 42_100,
            max: 1_000_000,
            recommend: 600_000,
            auto: 850_000,
        },
        todos: Vec::new(),
        busy_since: None,
        last_error: None,
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
    let mut app = App::new(Client::new(tx), "/home/u/proj".into(), InitPlan::default());
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
    });
    let mut i = info();
    if streaming {
        i.status = SessionStatus::Running;
    }
    let mut v = SessionView::new(i.clone(), true);
    v.load_snapshot(SessionSnapshot {
        info: i,
        messages: transcript(streaming),
        more_before: false,
        pending_permissions: vec![],
        pending_questions: vec![],
        queued: vec![],
    });
    app.views.insert("ses_test".into(), v);
    app.current = Some("ses_test".into());
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
    if let Some(v) = app.views.get_mut("ses_test")
        && let Part::Reasoning { text, .. } = &mut v.messages[1].parts[0]
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
    if let Some(v) = app.views.get_mut("ses_test") {
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
    let v = app.views.get_mut("ses_test").unwrap();
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
    let v = app.views.get_mut("ses_test").unwrap();
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
        v.load_snapshot(SessionSnapshot {
            info: inf.clone(),
            messages: transcript(i == 0),
            more_before: false,
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
    let v = app.views.get_mut("ses_test").unwrap();
    v.messages.push(Message {
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
    });
    v.messages.push(Message {
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
    });
    v.versions.push(1);
    v.versions.push(2);
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

fn type_str(app: &mut App, s: &str) {
    for c in s.chars() {
        crate::input::handle(
            app,
            key(
                crossterm::event::KeyCode::Char(c),
                crossterm::event::KeyModifiers::NONE,
            ),
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mock_end_to_end() {
    use crossterm::event::{KeyCode, KeyModifiers};
    let (out_tx, out_rx) = mpsc::unbounded_channel();
    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel();
    tokio::spawn(crate::mock::run("/home/u/proj".into(), out_rx, ev_tx));
    let plan = InitPlan {
        prompt: Some("hello, run the demo".into()),
        ..Default::default()
    };
    let mut app = App::new(Client::new(out_tx), "/home/u/proj".into(), plan);
    pump(&mut app, &mut ev_rx, 2500).await;
    let sid = app.current.clone().expect("session created");
    {
        let v = &app.views[&sid];
        assert!(v.loaded, "snapshot loaded");
        assert!(v.messages.len() >= 2, "user + assistant");
    }
    let (_, s) = render(&mut app, 100, 30);
    println!("--- streaming ---\n{s}");
    // Run until the permission request appears.
    for _ in 0..40 {
        if app.active_permission().is_some() {
            break;
        }
        pump(&mut app, &mut ev_rx, 250).await;
    }
    assert!(app.active_permission().is_some(), "permission requested");
    let (_, s) = render(&mut app, 100, 30);
    println!("--- permission ---\n{s}");
    assert!(s.contains("Permission required"));
    assert!(s.contains("Allow once"));
    // Keys within 350ms of the modal opening are ignored; wait then allow.
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    crate::input::handle(&mut app, key(KeyCode::Char('a'), KeyModifiers::NONE));
    assert!(app.active_permission().is_none());
    // Queue a prompt while busy.
    type_str(&mut app, "queued question");
    crate::input::handle(&mut app, key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.views[&sid].queued.len(), 1);
    // Queue another one and remove it with its ✕ button while the turn is still running.
    type_str(&mut app, "never send this");
    crate::input::handle(&mut app, key(KeyCode::Enter, KeyModifiers::NONE));
    pump(&mut app, &mut ev_rx, 200).await;
    assert!(
        app.views[&sid].queued.iter().all(|q| q.id.is_some()),
        "queued rows bound to server ids"
    );
    assert!(app.views[&sid].is_busy());
    // Switch to another session and back: the rows come back from the snapshot (still removable).
    let other = app
        .live
        .iter()
        .find(|s| s.id != sid)
        .map(|s| s.id.clone())
        .expect("another live session");
    app.attach_fg(&other);
    pump(&mut app, &mut ev_rx, 200).await;
    app.attach_fg(&sid);
    pump(&mut app, &mut ev_rx, 200).await;
    let q: Vec<&str> = app.views[&sid]
        .queued
        .iter()
        .map(|q| q.text.as_str())
        .collect();
    assert_eq!(
        q,
        ["queued question", "never send this"],
        "queued rows survive re-attach"
    );
    assert!(app.views[&sid].queued.iter().all(|q| q.id.is_some()));
    let (term, s) = render(&mut app, 100, 30);
    let qy = s
        .lines()
        .position(|l| l.contains("never send this") && l.contains("✕ remove"))
        .expect("queued row with remove button") as u16;
    let qx = find_col(&term, qy, "✕").unwrap();
    app.screen = ratatui::layout::Rect::new(0, 0, 100, 30);
    {
        use crossterm::event::{MouseButton, MouseEventKind};
        crate::input::handle(
            &mut app,
            mouse(MouseEventKind::Down(MouseButton::Left), qx, qy),
        );
        crate::input::handle(
            &mut app,
            mouse(MouseEventKind::Up(MouseButton::Left), qx, qy),
        );
    }
    pump(&mut app, &mut ev_rx, 200).await;
    let q: Vec<&str> = app.views[&sid]
        .queued
        .iter()
        .map(|q| q.text.as_str())
        .collect();
    assert_eq!(q, ["queued question"], "removed from the queue");
    assert_eq!(
        app.editor.text, "never send this",
        "removed prompt back in the input box"
    );
    app.editor.clear();
    // Let the turn finish.
    for _ in 0..120 {
        if app.views[&sid]
            .messages
            .iter()
            .any(|m| m.raw_text().contains("Got it"))
            && !app.views[&sid].is_busy()
        {
            break;
        }
        pump(&mut app, &mut ev_rx, 250).await;
    }
    let v = &app.views[&sid];
    assert!(!v.is_busy(), "turn finished");
    assert!(v.queued.is_empty(), "queued prompt delivered");
    assert!(
        !v.messages
            .iter()
            .any(|m| m.raw_text().contains("never send this")),
        "removed prompt never sent"
    );
    let (_, s) = render(&mut app, 100, 50);
    println!("--- done ---\n{s}");
    assert!(s.contains("Got it"), "reply to queued prompt");
    // Slash command via autocomplete: /details toggles tool expansion.
    type_str(&mut app, "/deta");
    assert!(app.ac.is_some(), "slash autocomplete");
    crate::input::handle(&mut app, key(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.details);
    assert!(app.editor.text.is_empty());
    // Session picker lists live + history.
    crate::input::handle(&mut app, key(KeyCode::Char('x'), KeyModifiers::CONTROL));
    crate::input::handle(&mut app, key(KeyCode::Char('l'), KeyModifiers::NONE));
    pump(&mut app, &mut ev_rx, 300).await;
    let (_, s) = render(&mut app, 120, 34);
    println!("--- sessions ---\n{s}");
    assert!(s.contains("Live") && s.contains("History"));
    assert!(s.contains("worker-1") && s.contains(" bg ") && s.contains(" fg "));
    crate::input::handle(&mut app, key(KeyCode::Esc, KeyModifiers::NONE));
    // Watch mode: tiles for the background sessions.
    crate::input::handle(&mut app, key(KeyCode::Char('x'), KeyModifiers::CONTROL));
    crate::input::handle(&mut app, key(KeyCode::Char('w'), KeyModifiers::NONE));
    pump(&mut app, &mut ev_rx, 1500).await;
    let (_, s) = render(&mut app, 140, 36);
    println!("--- watch ---\n{s}");
    assert_eq!(app.mode, Mode::Watch);
    assert!(s.contains("worker-1") && s.contains("reviewer") && s.contains("docs-写手"));
    // The session that was foreground in this frontend is background now, and tiled.
    let attached = |app: &App| app.live.iter().find(|i| i.id == sid).map(|i| i.attached);
    assert_eq!(attached(&app), Some(0));
    assert!(!app.views[&sid].foreground);
    assert!(app.tile_ids().contains(&sid));
    assert_eq!(app.current.as_deref(), Some(sid.as_str()));
    // A background tile asks for permission: the modal names the session; deny it.
    if app.active_permission().is_some() {
        assert!(s.contains("Permission required · docs-写手"));
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        crate::input::handle(&mut app, key(KeyCode::Char('d'), KeyModifiers::NONE));
        assert!(app.active_permission().is_none());
    }
    // Nothing is focused on entering the split view; typing only shows a hint.
    assert!(app.watch.focus.is_none(), "no auto-focus");
    type_str(&mut app, "x");
    assert!(app.watch.inputs.values().all(|e| e.is_empty()));
    assert!(
        app.hint
            .as_ref()
            .is_some_and(|h| h.0.contains("click or tab to focus"))
    );
    // Tab focuses a tile; message it directly.
    crate::input::handle(&mut app, key(KeyCode::Tab, KeyModifiers::NONE));
    let focus = app.watch.focus.clone().expect("focus");
    type_str(&mut app, "ping");
    crate::input::handle(&mut app, key(KeyCode::Enter, KeyModifiers::NONE));
    pump(&mut app, &mut ev_rx, 200).await;
    assert!(app.watch.inputs.get(&focus).is_none_or(|e| e.is_empty()));
    // Back to chat.
    crate::input::handle(&mut app, key(KeyCode::Char('x'), KeyModifiers::CONTROL));
    crate::input::handle(&mut app, key(KeyCode::Char('w'), KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Chat);
    pump(&mut app, &mut ev_rx, 300).await;
    assert_eq!(attached(&app), Some(1), "foreground again");
    assert!(app.views[&sid].foreground);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn permission_modal_keys_then_quit() {
    use crossterm::event::{KeyCode, KeyModifiers};
    let (out_tx, out_rx) = mpsc::unbounded_channel();
    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel();
    tokio::spawn(crate::mock::run("/home/u/proj".into(), out_rx, ev_tx));
    let mut app = App::new(
        Client::new(out_tx),
        "/home/u/proj".into(),
        InitPlan {
            watch: true,
            ..Default::default()
        },
    );
    for _ in 0..60 {
        pump(&mut app, &mut ev_rx, 250).await;
        render(&mut app, 100, 30);
        if app.active_permission().is_some() {
            break;
        }
    }
    assert!(app.active_permission().is_some());
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
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
    crate::input::handle(&mut app, key(KeyCode::Char('c'), KeyModifiers::CONTROL));
    println!(
        "after ctrl+c: perm={} quit={}",
        app.active_permission().is_some(),
        app.quit
    );
    crate::input::handle(&mut app, key(KeyCode::Char('c'), KeyModifiers::CONTROL));
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
    tokio::spawn(crate::mock::run("/home/u/proj".into(), out_rx, ev_tx));
    let mut app = App::new(
        Client::new(out_tx),
        "/home/u/proj".into(),
        InitPlan::default(),
    );
    pump(&mut app, &mut ev_rx, 600).await;
    type_str(&mut app, "look at @src/ma");
    assert!(app.files_due.is_some(), "debounced query pending");
    for _ in 0..10 {
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        app.housekeeping();
        pump(&mut app, &mut ev_rx, 30).await;
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
    crate::input::handle(
        &mut app,
        key(
            crossterm::event::KeyCode::Tab,
            crossterm::event::KeyModifiers::NONE,
        ),
    );
    assert!(
        app.editor.text.starts_with("look at @src/"),
        "{}",
        app.editor.text
    );
    assert!(app.editor.text.ends_with(' '));
    assert!(app.ac.is_none());
    // Slash list with empty query: canonical names in declaration order.
    app.editor.clear();
    type_str(&mut app, "/");
    let ac = app.ac.as_ref().expect("slash popup");
    assert_eq!(ac.items[0].1, "/new");
    assert_eq!(ac.items[1].1, "/sessions");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dialogs_open_and_render() {
    use crossterm::event::{KeyCode, KeyModifiers};
    let (out_tx, out_rx) = mpsc::unbounded_channel();
    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel();
    tokio::spawn(crate::mock::run("/home/u/proj".into(), out_rx, ev_tx));
    let mut app = App::new(
        Client::new(out_tx),
        "/home/u/proj".into(),
        InitPlan::default(),
    );
    pump(&mut app, &mut ev_rx, 600).await;
    assert!(app.config.is_some());
    crate::input::handle(&mut app, key(KeyCode::Char('x'), KeyModifiers::CONTROL));
    crate::input::handle(&mut app, key(KeyCode::Char('m'), KeyModifiers::NONE));
    assert!(app.dialog.is_some(), "models dialog");
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let (_, s) = render(&mut app, 120, 30);
    println!("{s}");
    assert!(s.contains("Switch model") && s.contains("GPT-6") && s.contains("DGN Gateway"));
    type_str(&mut app, "gpt");
    crate::input::handle(&mut app, key(KeyCode::Enter, KeyModifiers::NONE));
    pump(&mut app, &mut ev_rx, 200).await;
    assert_eq!(app.cur().unwrap().info.model.model, "gpt-6");
    // Help dialog.
    crate::input::handle(&mut app, key(KeyCode::Char('x'), KeyModifiers::CONTROL));
    crate::input::handle(&mut app, key(KeyCode::Char('h'), KeyModifiers::NONE));
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
    let v = app.views.get_mut("ses_test").unwrap();
    let mut i = v.info.clone();
    i.status = SessionStatus::Retrying {
        attempt: 3,
        next_at: ref_proto::now_ms() + 11_500,
        error: "rate limited (429)".into(),
    };
    v.apply(SessionEvent::Info { info: i });
    let (_, s) = render(&mut app, 100, 30);
    assert!(
        s.contains("Retrying in 12s (attempt 3): rate limited (429)"),
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
    assert!(
        app.pending.values().any(
            |p| matches!(p, crate::app::Pending::Attach { sid, fg: true } if sid == "ses_test")
        )
    );
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
                    result: json!({}),
                    error: None,
                },
                // Bigger than tungstenite's 16 MiB default frame limit (snapshots with many images).
                _ => ServerFrame::Response {
                    id,
                    ok: true,
                    result: json!({"sessions": [], "pad": "x".repeat(20 << 20)}),
                    error: None,
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
    let ok = matches!(&r, crate::net::NetEvent::Frame(ServerFrame::Response { id: 7, ok: true, result, .. }) if result["pad"].as_str().map(str::len) == Some(20 << 20));
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
        let v = app.views.get_mut("ses_test").unwrap();
        let base = transcript(false);
        let mut msgs = Vec::new();
        for i in 0..400 {
            for m in &base {
                let mut m = m.clone();
                m.id = format!("{}-{i}", m.id);
                msgs.push(m);
            }
        }
        v.apply(SessionEvent::Reset {
            more_before: false,
            messages: msgs,
        });
    }
    let total = app.views["ses_test"].messages.len();
    let laid_out = |app: &App| app.views["ses_test"].chat.laid_out;
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
        let v = app.views.get_mut("ses_test").unwrap();
        let id = v.messages.last().unwrap().id.clone();
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
    use crossterm::event::{KeyCode, KeyModifiers};
    let mut app = test_app(false);
    type_str(&mut app, "!");
    assert!(app.shell_mode);
    type_str(&mut app, "ls -la");
    let (_, s) = render(&mut app, 100, 30);
    assert!(s.contains("┃  Shell"), "{s}");
    crate::input::handle(&mut app, key(KeyCode::Esc, KeyModifiers::NONE));
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
    type_str(&mut app, "what is this?");
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
    use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};
    let mut app = test_app(false);
    {
        let v = app.views.get_mut("ses_test").unwrap();
        if let Part::Text { text, .. } = &mut v.messages[0].parts[0] {
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
    crate::input::handle(&mut app, key(KeyCode::Char('y'), KeyModifiers::NONE));
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
    crate::input::handle(&mut app, key(KeyCode::Char('c'), KeyModifiers::CONTROL));
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
    let raw = app.views["ses_test"].messages[1].raw_text();
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
    }));
}

#[test]
fn reattach_failure_of_empty_session_starts_new_one() {
    use crate::app::Pending;
    let mut app = test_app(false);
    app.views
        .get_mut("ses_test")
        .unwrap()
        .apply(SessionEvent::Reset {
            more_before: false,
            messages: vec![],
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
        app.pending
            .values()
            .any(|p| matches!(p, Pending::Created { .. })),
        "a fresh session is requested"
    );
}

#[test]
fn failed_prompt_text_returns_to_editor() {
    use crate::app::Pending;
    let mut app = test_app(false);
    type_str(&mut app, "do the thing");
    crate::input::handle(
        &mut app,
        key(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ),
    );
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
    use crossterm::event::{KeyCode, KeyModifiers};
    let mut app = test_app(false);
    let req = PermissionRequest {
        session_id: "ses_test".into(),
        message_id: "m2".into(),
        call_id: "c9".into(),
        tool: "bash".into(),
        purpose: "List files".into(),
        input: json!({"command": "ls -la"}),
        time: 0,
    };
    app.views
        .get_mut("ses_test")
        .unwrap()
        .apply(SessionEvent::Permission { request: req });
    let (_, s) = render(&mut app, 100, 30);
    assert!(
        s.contains("Permission required") && s.contains("$ ls -la"),
        "{s}"
    );
    // Within the grace period: 'a' / 'd' must not answer the modal; they are typed text.
    type_str(&mut app, "ad");
    crate::input::handle(&mut app, key(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        app.active_permission().is_some(),
        "modal not answered by early keys"
    );
    assert_eq!(app.editor.text, "ad");
    // After the grace period the modal reacts.
    app.perm.as_mut().unwrap().opened -= std::time::Duration::from_millis(500);
    crate::input::handle(&mut app, key(KeyCode::Char('a'), KeyModifiers::NONE));
    assert!(app.active_permission().is_none());
    assert_eq!(app.editor.text, "ad");
    // The modal is sized to its content, not the full screen height.
    let mut app = test_app(false);
    let req = PermissionRequest {
        session_id: "ses_test".into(),
        message_id: "m2".into(),
        call_id: "c10".into(),
        tool: "bash".into(),
        purpose: "x".into(),
        input: json!({"command": "ls"}),
        time: 0,
    };
    app.views
        .get_mut("ses_test")
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
    type_str(&mut app, "opus");
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

#[test]
fn undo_button_hit_zone_and_flow() {
    use crate::app::Pending;
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
    let v = app.views.get_mut("ses_test").unwrap();
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
    // Click (idle): request sent right away, removing 2 messages.
    app.screen = ratatui::layout::Rect::new(0, 0, 100, 30);
    let click = |app: &mut App| {
        use crossterm::event::{MouseButton, MouseEventKind};
        crate::input::handle(
            app,
            mouse(MouseEventKind::Down(MouseButton::Left), ux + 2, uy),
        );
        crate::input::handle(
            app,
            mouse(MouseEventKind::Up(MouseButton::Left), ux + 2, uy),
        );
    };
    click(&mut app);
    let id = *app
        .pending
        .iter()
        .find(|(_, p)| matches!(p, Pending::Undo { count: Some(2), .. }))
        .map(|(k, _)| k)
        .expect("undo request");
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id,
        ok: true,
        result: json!({"text": "Please fix it"}),
        error: None,
    }));
    assert_eq!(app.editor.text, "Please fix it");
    assert!(
        app.toasts
            .items
            .iter()
            .any(|t| t.text == "Undid 2 messages")
    );
    // Non-empty editor is kept; the prompt is appended after a blank line.
    let mut app = test_app(false);
    render(&mut app, 100, 30);
    app.screen = ratatui::layout::Rect::new(0, 0, 100, 30);
    type_str(&mut app, "draft");
    click(&mut app);
    let id = *app
        .pending
        .iter()
        .find(|(_, p)| matches!(p, Pending::Undo { .. }))
        .map(|(k, _)| k)
        .unwrap();
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id,
        ok: true,
        result: json!({"text": "old prompt"}),
        error: None,
    }));
    assert_eq!(app.editor.text, "draft\n\nold prompt");
    assert_eq!(app.editor.cursor, app.editor.text.len());
    // Busy session: the first click only arms a confirmation.
    let mut app = test_app(true);
    render(&mut app, 100, 30);
    app.screen = ratatui::layout::Rect::new(0, 0, 100, 30);
    let (term, s) = render(&mut app, 100, 30);
    let uy2 = s.lines().position(|l| l.contains("↶ undo")).unwrap() as u16;
    let ux2 = find_col(&term, uy2, "↶").unwrap();
    let click2 = |app: &mut App| {
        use crossterm::event::{MouseButton, MouseEventKind};
        crate::input::handle(
            app,
            mouse(MouseEventKind::Down(MouseButton::Left), ux2, uy2),
        );
        crate::input::handle(app, mouse(MouseEventKind::Up(MouseButton::Left), ux2, uy2));
    };
    click2(&mut app);
    assert!(
        !app.pending
            .values()
            .any(|p| matches!(p, Pending::Undo { .. }))
    );
    assert!(
        app.hint
            .as_ref()
            .is_some_and(|h| h.0.contains("aborts the running turn"))
    );
    click2(&mut app);
    assert!(
        app.pending
            .values()
            .any(|p| matches!(p, Pending::Undo { .. }))
    );
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
    let v = app.views.get_mut("ses_test").unwrap();
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
        }));
    };
    // /undo (= ctrl+x u): the last human prompt, with its image.
    type_str(&mut app, "/undo");
    crate::input::handle(
        &mut app,
        key(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ),
    );
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
        app.views["ses_test"].queued.is_empty(),
        "undo clears the server queue"
    );
    // Undo from an earlier message: that prompt comes back (appended), not the last one.
    app.undo_message("ses_test", "m1");
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

#[test]
fn queued_prompt_remove_button_and_already_sent_race() {
    use crate::app::Pending;
    use crate::ui::Hit;
    let mut app = test_app(true);
    let sid = "ses_test".to_string();
    app.screen = ratatui::layout::Rect::new(0, 0, 100, 30);
    let respond = |app: &mut App, pred: &dyn Fn(&Pending) -> bool, result: serde_json::Value| {
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
        }));
    };
    let click_remove = |app: &mut App, text: &str| {
        use crossterm::event::{MouseButton, MouseEventKind};
        let (term, s) = render(app, 100, 30);
        let y = s
            .lines()
            .position(|l| l.contains(text) && l.contains("✕ remove"))
            .expect("queued row") as u16;
        let x = find_col(&term, y, "✕").unwrap();
        let key = app.views["ses_test"]
            .queued
            .iter()
            .find(|q| q.text == text)
            .unwrap()
            .key;
        assert_eq!(
            app.views["ses_test"].chat.hit_at(x, y),
            Some(Hit::Unqueue { key })
        );
        crate::input::handle(app, mouse(MouseEventKind::Down(MouseButton::Left), x, y));
        crate::input::handle(app, mouse(MouseEventKind::Up(MouseButton::Left), x, y));
    };
    // Clicked before the Prompt response: the delete waits for the message id.
    type_str(&mut app, "first queued");
    crate::input::handle(
        &mut app,
        key(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ),
    );
    click_remove(&mut app, "first queued");
    assert!(app.views[&sid].queued[0].removing);
    assert!(
        !app.pending
            .values()
            .any(|p| matches!(p, Pending::Unqueue { .. }))
    );
    let (_, s) = render(&mut app, 100, 30);
    assert!(s.contains("removing…"));
    respond(
        &mut app,
        &|p| matches!(p, Pending::Prompt { .. }),
        json!({"queued": true, "message_id": "q1"}),
    );
    assert!(
        app.pending
            .values()
            .any(|p| matches!(p, Pending::Unqueue { .. })),
        "RemoveQueued sent once the id is known"
    );
    // Typed meanwhile: the removed prompt is appended after a blank line.
    type_str(&mut app, "draft");
    respond(
        &mut app,
        &|p| matches!(p, Pending::Unqueue { .. }),
        json!({"removed": true}),
    );
    assert!(app.views[&sid].queued.is_empty());
    assert_eq!(app.editor.text, "draft\n\nfirst queued");
    assert_eq!(app.editor.cursor, app.editor.text.len());
    let (_, s) = render(&mut app, 100, 30);
    assert!(!s.contains("queued first queued") && !s.contains("✕ remove"));
    app.editor.clear();
    // Images come back with it (empty editor: filled).
    let png = ImageData {
        mime: "image/png".into(),
        data: "iVBO".into(),
        ..Default::default()
    };
    app.send_prompt(&sid, "see [Image 1]".into(), vec![png]);
    respond(
        &mut app,
        &|p| matches!(p, Pending::Prompt { .. }),
        json!({"queued": true, "message_id": "q0"}),
    );
    click_remove(&mut app, "see [Image 1]");
    respond(
        &mut app,
        &|p| matches!(p, Pending::Unqueue { .. }),
        json!({"removed": true}),
    );
    assert_eq!(app.editor.text, "see [Image 1]");
    assert_eq!(
        app.editor
            .images
            .iter()
            .map(|a| a.image.data.as_str())
            .collect::<Vec<_>>(),
        ["iVBO"]
    );
    app.editor.clear();
    // Already delivered when the delete arrives: toast, row stays until the message shows up.
    type_str(&mut app, "second queued");
    crate::input::handle(
        &mut app,
        key(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ),
    );
    respond(
        &mut app,
        &|p| matches!(p, Pending::Prompt { .. }),
        json!({"queued": true, "message_id": "q2"}),
    );
    click_remove(&mut app, "second queued");
    respond(
        &mut app,
        &|p| matches!(p, Pending::Unqueue { .. }),
        json!({"removed": false}),
    );
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
    // Sent while we thought it idle but the server queued it: a removable row appears.
    v.set_info(info());
    type_str(&mut app, "late");
    crate::input::handle(
        &mut app,
        key(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ),
    );
    assert!(app.views[&sid].queued.is_empty());
    respond(
        &mut app,
        &|p| matches!(p, Pending::Prompt { .. }),
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
    let mut i = app.views["ses_test"].info.clone();
    i.busy_since = Some(ref_proto::now_ms() - 65_000);
    app.views.get_mut("ses_test").unwrap().set_info(i.clone());
    app.live = vec![
        i.clone(),
        bg_session("ses_other", "other", SessionStatus::Idle, 0),
    ];
    let (_, s) = render(&mut app, 100, 30);
    assert!(s.contains("running tools 1m 0"), "{s}");
    // Switch away and back: the view is rebuilt, the time is not reset.
    app.attach_fg("ses_other");
    assert!(!app.views.contains_key("ses_test"));
    app.attach_fg("ses_test");
    assert!(
        app.views["ses_test"]
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

#[test]
fn close_session_command_and_confirmation() {
    use crate::app::Pending;
    use crate::ui::dialog::{Dialog, Kind};
    let mut app = test_app(false);
    let cur = app.views["ses_test"].info.clone();
    app.live = vec![
        cur,
        bg_session("ses_bg", "worker-1", SessionStatus::Busy, 0),
        bg_session("ses_fg2", "other-fe", SessionStatus::Idle, 1),
    ];
    // By name: background session closes at once.
    type_str(&mut app, "/kill worker-1");
    crate::input::handle(
        &mut app,
        key(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ),
    );
    assert!(
        app.pending
            .values()
            .any(|p| matches!(p, Pending::Closed { name } if name == "worker-1"))
    );
    assert!(
        !app.live.iter().any(|s| s.id == "ses_bg"),
        "list updated right away"
    );
    assert_eq!(app.current.as_deref(), Some("ses_test"));
    // Foreground in another frontend: confirm first.
    app.pending.clear();
    app.run_command("close-session", "OTHER-FE");
    assert!(app.pending.is_empty());
    assert!(
        app.hint
            .as_ref()
            .is_some_and(|h| h.0.contains("another frontend"))
    );
    app.run_command("close-session", "other-fe");
    assert!(
        app.pending
            .values()
            .any(|p| matches!(p, Pending::Closed { .. }))
    );
    // Picker: live sessions only, stays open after closing, list refreshes.
    app.live
        .push(bg_session("ses_x", "x1", SessionStatus::Idle, 0));
    app.live
        .push(bg_session("ses_y", "y1", SessionStatus::Idle, 0));
    app.history_list = vec![bg_session("ses_h", "old-history", SessionStatus::Idle, 0)];
    app.run_command("close-session", "");
    let (_, s) = render(&mut app, 120, 34);
    assert!(
        s.contains("Close sessions")
            && s.contains("x1")
            && s.contains("y1")
            && !s.contains("old-history"),
        "{s}"
    );
    if let Some(Dialog::List(d)) = app.dialog.as_mut() {
        d.select_key("live:ses_x");
    }
    app.pending.clear();
    crate::input::handle(
        &mut app,
        key(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ),
    );
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
    // Closing the current session from the picker switches to the remaining live one.
    if let Some(Dialog::List(d)) = app.dialog.as_mut() {
        d.select_key("live:ses_test");
    }
    crate::input::handle(
        &mut app,
        key(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ),
    );
    assert_eq!(app.current.as_deref(), Some("ses_y"));
    assert!(
        !app.pending
            .values()
            .any(|p| matches!(p, Pending::Created { .. }))
    );
    assert!(!app.quit && !app.quit_after_close);
}

#[test]
fn sessions_dialog_ctrl_d_closes_with_confirm() {
    use crate::app::Pending;
    use crossterm::event::{KeyCode, KeyModifiers};
    let mut app = test_app(false);
    let cur = app.views["ses_test"].info.clone();
    app.live = vec![
        cur,
        bg_session("ses_bg", "worker-1", SessionStatus::Idle, 0),
    ];
    app.open_sessions();
    let (_, s) = render(&mut app, 120, 34);
    assert!(s.contains("ctrl+d close live / delete history"), "{s}");
    if let Some(crate::ui::dialog::Dialog::List(d)) = app.dialog.as_mut() {
        d.select_key("live:ses_bg");
    }
    app.pending.clear();
    crate::input::handle(&mut app, key(KeyCode::Char('d'), KeyModifiers::CONTROL));
    let (_, s) = render(&mut app, 120, 34);
    assert!(s.contains("ctrl+d again to close"), "{s}");
    assert!(
        !app.pending
            .values()
            .any(|p| matches!(p, Pending::Closed { .. }))
    );
    crate::input::handle(&mut app, key(KeyCode::Char('d'), KeyModifiers::CONTROL));
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
fn leader_k_closes_current_and_focused_tile() {
    use crate::app::Pending;
    use crossterm::event::{KeyCode, KeyModifiers};
    let lk = |app: &mut App| {
        crate::input::handle(app, key(KeyCode::Char('x'), KeyModifiers::CONTROL));
        crate::input::handle(app, key(KeyCode::Char('k'), KeyModifiers::NONE));
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
        app.quit_after_close && !app.quit,
        "last session: exit once the close is confirmed"
    );
    assert!(app.hint.is_none(), "confirmation hint cleared");
    // Watch mode: closes the focused tile.
    let mut app = test_app(false);
    app.mode = Mode::Watch;
    let bg = bg_session("ses_bg", "worker-1", SessionStatus::Idle, 0);
    app.views
        .insert("ses_bg".into(), SessionView::new(bg.clone(), false));
    app.live = vec![app.views["ses_test"].info.clone(), bg];
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
        Some("ses_test"),
        "current untouched"
    );
    assert_eq!(app.mode, Mode::Watch);
}

#[test]
fn frame_interval_animates_background_sessions_in_sidebar() {
    let mut app = test_app(false);
    let cur = app.views["ses_test"].info.clone();
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
    let cur = app.views["ses_test"].info.clone();
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
    v.load_snapshot(SessionSnapshot {
        info: inf.clone(),
        messages: transcript(true),
        more_before: false,
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
    v.load_snapshot(SessionSnapshot {
        info: inf.clone(),
        messages: transcript(false),
        more_before: false,
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
fn terminal_title_text() {
    let mut app = test_app(false);
    assert_eq!(app.terminal_title(), "R|Fix the parser");
    app.views.get_mut("ses_test").unwrap().info.title = "  ".into();
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
        .get_mut("ses_test")
        .unwrap()
        .apply(SessionEvent::Reset {
            more_before: false,
            messages: vec![],
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
        let v = app.views.get_mut("ses_test").unwrap();
        v.messages[1].parts.push(tool(
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
        .get_mut("ses_test")
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
    v.load_snapshot(SessionSnapshot {
        info: inf.clone(),
        messages: transcript(false),
        more_before: false,
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
    }));
}

fn sidebar_app() -> App {
    let mut app = test_app(false);
    app.refresh_mcp();
    answer_mcp(&mut app, &mcp_servers());
    let v = app.views.get_mut("ses_test").unwrap();
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
            && pos(&sb, "ses_test").is_some()
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
    use crossterm::event::{KeyCode, KeyModifiers};
    let mut app = sidebar_app();
    type_str(&mut app, "/mcp");
    crate::input::handle(&mut app, key(KeyCode::Enter, KeyModifiers::NONE));
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
    crate::input::handle(&mut app, key(KeyCode::Enter, KeyModifiers::NONE));
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
    crate::input::handle(&mut app, key(KeyCode::Left, KeyModifiers::NONE));
    let (_, s) = render(&mut app, 100, 30);
    assert!(!s.contains("resolve-library-id"));
    crate::input::handle(&mut app, key(KeyCode::Down, KeyModifiers::NONE));
    crate::input::handle(&mut app, key(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.dialog.is_none());
    // Autocomplete + palette + help list it.
    type_str(&mut app, "/mc");
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
    let v = app.views.get_mut("ses_test").unwrap();
    let info = v.info.clone();
    v.load_snapshot(SessionSnapshot {
        info,
        messages: msgs,
        more_before: false,
        pending_permissions: vec![],
        pending_questions: vec![],
        queued: vec![],
    });
    app
}

#[test]
fn compaction_divider_keeps_old_messages_interactive() {
    use crate::ui::Hit;
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
    let dy = lines
        .iter()
        .position(|l| l.contains("◇ Context compacted"))
        .expect("divider");
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
    let _ = buf;
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
        let v = app.views.get_mut("ses_test").unwrap();
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
    use crossterm::event::{MouseButton, MouseEventKind};
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
    // Old tool still expands; the summary expands on click.
    let v = app.views.get_mut("ses_test").unwrap();
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
    // A transcript that STARTS with a compaction (old backend) has no "older messages" hint.
    let mut app = test_app(false);
    let v = app.views.get_mut("ses_test").unwrap();
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
    let msgs = src.views["ses_test"].messages.clone();
    v.load_snapshot(SessionSnapshot {
        info: inf.clone(),
        messages: msgs,
        more_before: false,
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
    tokio::spawn(crate::mock::run("/home/u/proj".into(), out_rx, ev_tx));
    let plan = InitPlan {
        prompt: Some("hello".into()),
        ..Default::default()
    };
    let mut app = App::new(Client::new(out_tx), "/home/u/proj".into(), plan);
    pump(&mut app, &mut ev_rx, 1500).await;
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
            .is_some_and(|v| v.loaded && !v.is_busy() && v.messages.len() >= 2)
        {
            break;
        }
        pump(&mut app, &mut ev_rx, 250).await;
    }
    let before = app.cur().unwrap().messages.len();
    assert!(before >= 2);
    app.run_command("compact", "");
    pump(&mut app, &mut ev_rx, 2500).await;
    let v = app.cur().unwrap();
    assert_eq!(
        v.messages.len(),
        before + 1,
        "old messages kept, compaction appended"
    );
    assert_eq!(v.messages.last().unwrap().source, MessageSource::Compaction);
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
/// is foreground in another frontend, so it is not tiled.
fn watch_app() -> App {
    let mut app = test_app(false);
    app.toggle_watch();
    let mut cur = app.views["ses_test"].info.clone();
    cur.attached = 1;
    let mut live = vec![cur];
    for (i, (id, name)) in [("ses_w1", "alpha"), ("ses_w2", "beta")]
        .into_iter()
        .enumerate()
    {
        let mut inf = bg_session(id, name, SessionStatus::Idle, 0);
        inf.created_at = 10 + i as i64;
        let mut v = SessionView::new(inf.clone(), false);
        v.load_snapshot(SessionSnapshot {
            info: inf.clone(),
            messages: long_transcript(name, 30),
            more_before: false,
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
    crate::input::handle(&mut app, key(KeyCode::PageUp, KeyModifiers::NONE));
    render(&mut app, 120, 30);
    assert!(app.views["ses_w1"].chat.top() < bottom);
    let t = app.views["ses_w1"].chat.top();
    crate::input::handle(&mut app, key(KeyCode::Up, KeyModifiers::SHIFT));
    assert_eq!(app.views["ses_w1"].chat.top(), t - 1);
    crate::input::handle(&mut app, key(KeyCode::End, KeyModifiers::NONE));
    render(&mut app, 120, 30);
    assert_eq!(app.views["ses_w1"].chat.top(), bottom);
    assert!(app.views["ses_w1"].chat.follow);
    // Scrolled to the very top: the first message of the session is reachable.
    crate::input::handle(&mut app, key(KeyCode::Home, KeyModifiers::NONE));
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
    assert!(s.contains("click or tab to focus a session"), "{s}");
    // No tile border in the focus colour.
    let buf = term.backend().buffer();
    let r = tile_rect(&app, "ses_w1");
    assert_ne!(buf[(r.x, r.y)].fg, crate::ui::theme::PRIMARY);
    // Typing / enter with nothing focused: no input, no prompt sent.
    app.pending.clear();
    type_str(&mut app, "hi");
    crate::input::handle(&mut app, key(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.watch.inputs.values().all(|e| e.is_empty()) && app.pending.is_empty());
    // Sessions appearing/disappearing don't auto-focus.
    app.sync_watch();
    assert!(app.watch.focus.is_none());
    // Tab focuses the first tile; esc with text clears it, the next esc unfocuses.
    crate::input::handle(&mut app, key(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.watch.focus.as_deref(), Some("ses_w1"));
    type_str(&mut app, "hi");
    crate::input::handle(&mut app, key(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.watch.focus.as_deref(), Some("ses_w1"));
    assert!(app.watch.inputs["ses_w1"].is_empty());
    crate::input::handle(&mut app, key(KeyCode::Esc, KeyModifiers::NONE));
    assert!(
        app.watch.focus.is_none(),
        "esc on idle empty tile unfocuses"
    );
    // Shift+tab from nothing: last tile. Busy tile: esc arms, esc aborts, focus kept.
    crate::input::handle(&mut app, key(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(app.watch.focus.as_deref(), Some("ses_w2"));
    app.views.get_mut("ses_w2").unwrap().info.status = SessionStatus::Busy;
    app.pending.clear();
    crate::input::handle(&mut app, key(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.esc_armed.is_some() && app.watch.focus.is_some());
    crate::input::handle(&mut app, key(KeyCode::Esc, KeyModifiers::NONE));
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
    let chat_top = app.views["ses_test"].chat.top();
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
        app.views["ses_test"].chat.top(),
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
    }));
}

#[test]
fn current_session_goes_background_in_split_view() {
    use crate::app::Pending;
    let mut app = test_app(false);
    app.live = vec![app.views["ses_test"].info.clone()];
    app.toggle_watch();
    assert_eq!(app.mode, Mode::Watch);
    let v = &app.views["ses_test"];
    assert!(!v.foreground);
    assert_eq!(app.live[0].attached, 0);
    assert_eq!(app.tile_ids(), ["ses_test"]);
    assert!(!app.attached_elsewhere("ses_test"));
    app.sync_watch();
    assert!(app.views.contains_key("ses_test"), "current view kept");
    // Back to chat: attached as foreground again.
    app.pending.clear();
    app.toggle_watch();
    assert_eq!(app.mode, Mode::Chat);
    assert!(app.views["ses_test"].foreground);
    assert_eq!(app.live[0].attached, 1);
    assert!(
        app.pending
            .values()
            .any(|p| matches!(p, Pending::Attach { sid, fg: true } if sid == "ses_test"))
    );
    // Into the split view before that attach is answered: the late answer keeps it background.
    app.toggle_watch();
    let snap = SessionSnapshot {
        info: app.views["ses_test"].info.clone(),
        messages: vec![],
        more_before: false,
        pending_permissions: vec![],
        pending_questions: vec![],
        queued: vec![],
    };
    respond(
        &mut app,
        |p| matches!(p, Pending::Attach { fg: true, .. }),
        json!({ "snapshot": snap }),
    );
    assert!(!app.views["ses_test"].foreground);
}

#[test]
fn close_current_goes_to_first_live_session_or_exits() {
    use crate::app::Pending;
    let enter = |app: &mut App| {
        crate::input::handle(
            app,
            key(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE,
            ),
        )
    };
    // Other live sessions: switch to the oldest one (sidebar order), no new session.
    let mut app = test_app(false);
    let cur = app.views["ses_test"].info.clone();
    let mut newer = bg_session("ses_new", "newer", SessionStatus::Idle, 0);
    newer.created_at = 50;
    let mut older = bg_session("ses_old", "older", SessionStatus::Idle, 0);
    older.created_at = 10;
    app.live = vec![cur.clone(), newer, older];
    type_str(&mut app, "/close");
    enter(&mut app);
    assert!(
        app.pending
            .values()
            .any(|p| matches!(p, Pending::Closed { .. }))
    );
    assert_eq!(app.current.as_deref(), Some("ses_old"));
    assert!(
        app.pending
            .values()
            .any(|p| matches!(p, Pending::Attach { sid, fg: true } if sid == "ses_old"))
    );
    assert!(!app.views.contains_key("ses_test"));
    assert!(
        !app.pending
            .values()
            .any(|p| matches!(p, Pending::Created { .. }))
    );
    assert!(!app.quit_after_close);

    // Nothing else live: exit after the backend confirmed the close; the exit screen still
    // shows how to resume the closed session.
    let mut app = test_app(false);
    app.live = vec![cur.clone()];
    type_str(&mut app, "/close");
    enter(&mut app);
    assert!(!app.quit, "waits for the Close response");
    respond(&mut app, |p| matches!(p, Pending::Closed { .. }), json!({}));
    assert!(app.quit);
    assert!(app.epilogue().is_some_and(|t| t.contains("ses_test")));

    // A failed close or a dropped connection exits as well.
    let mut app = test_app(false);
    app.run_command("close", "");
    fail_pending(&mut app, |p| matches!(p, Pending::Closed { .. }), "boom");
    assert!(app.quit);
    let mut app = test_app(false);
    app.run_command("close", "");
    app.on_net(crate::net::NetEvent::Disconnected("gone".into()));
    assert!(app.quit);

    // Session already gone on the server: exit right away.
    let mut app = test_app(false);
    app.views.get_mut("ses_test").unwrap().closed = true;
    app.run_command("close", "");
    assert!(app.quit);
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
    assert!(app.pending.is_empty());
    assert!(
        app.toasts
            .items
            .iter()
            .any(|t| t.text.contains("Working directory:") && t.text.contains("/cd <dir>")),
        "no argument shows the directory"
    );
    type_str(&mut app, "/cwd \"../other dir\"");
    crate::input::handle(
        &mut app,
        key(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ),
    );
    assert!(
        app.pending
            .values()
            .any(|p| matches!(p, Pending::Cwd { sid } if sid == "ses_test"))
    );
    let mut info = app.views["ses_test"].info.clone();
    info.cwd = "/home/u/other dir".into();
    respond(
        &mut app,
        |p| matches!(p, Pending::Cwd { .. }),
        json!({ "session": info }),
    );
    assert_eq!(app.views["ses_test"].info.cwd, "/home/u/other dir");
    assert_eq!(app.cwd, "/home/u/other dir", "new sessions follow");
    assert!(
        app.toasts
            .items
            .iter()
            .any(|t| t.text.contains("other dir"))
    );
    // Errors are shown, nothing changes.
    type_str(&mut app, "/cd /nope");
    crate::input::handle(
        &mut app,
        key(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ),
    );
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
    use crossterm::event::{KeyCode, KeyModifiers};
    let mut app = test_app(false);
    app.mode = Mode::Watch;
    let bg = bg_session("ses_bg", "worker-1", SessionStatus::Idle, 0);
    app.views
        .insert("ses_bg".into(), SessionView::new(bg.clone(), false));
    app.live = vec![app.views["ses_test"].info.clone(), bg];
    app.watch.focus = Some("ses_bg".into());
    let typed = |app: &mut App, t: &str| {
        for c in t.chars() {
            crate::input::handle(app, key(KeyCode::Char(c), KeyModifiers::NONE));
        }
        crate::input::handle(app, key(KeyCode::Enter, KeyModifiers::NONE));
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
    assert!(
        app.hint
            .as_ref()
            .is_some_and(|h| h.0.contains("works in the chat view"))
    );
    assert_eq!(
        app.watch.inputs["ses_bg"].text, "/compact",
        "kept for editing"
    );
    app.watch.inputs.get_mut("ses_bg").unwrap().clear();
    typed(&mut app, "/close");
    assert!(
        app.pending
            .values()
            .any(|p| matches!(p, Pending::Closed { name } if name == "worker-1"))
    );
    assert!(!app.views.contains_key("ses_bg"));
    assert_eq!(app.current.as_deref(), Some("ses_test"));
    assert_eq!(app.mode, Mode::Watch);
    assert!(!app.quit_after_close);
}

#[test]
fn sidebar_live_sessions_show_todo_count() {
    let todo = |c: &str, st: TodoStatus| Todo {
        content: c.into(),
        status: st,
    };
    let mut app = test_app(false);
    let mut cur = app.views["ses_test"].info.clone();
    cur.todos = vec![
        todo("a", TodoStatus::Completed),
        todo("b", TodoStatus::InProgress),
        todo("c", TodoStatus::Pending),
        todo("x", TodoStatus::Pending),
        todo("y", TodoStatus::Pending),
    ];
    app.views.get_mut("ses_test").unwrap().info.todos = cur.todos.clone();
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
    let (term, _) = render(&mut app, 150, 50);
    let all = sidebar_text(&term, 150);
    let rows: Vec<String> = all
        .iter()
        .skip_while(|l| !l.contains("Live sessions"))
        .cloned()
        .collect();
    let row = |name: &str| {
        rows.iter()
            .find(|l| l.contains(name))
            .unwrap_or_else(|| panic!("{name}: {rows:#?}"))
            .trim_end()
            .to_string()
    };
    assert!(
        row("ref-a1b2").ends_with("1/5"),
        "current session too: {:?}",
        row("ref-a1b2")
    );
    let w = row("worker-1");
    assert!(
        w.ends_with("✓3/3 1m 05s") || w.ends_with("✓3/3 1m 06s"),
        "{w:?}"
    );
    assert!(row("pender").ends_with(" 0/2"), "cancelled not counted");
    assert!(row("reviewer").ends_with("✓1/1"));
    assert!(
        row("cancelled-only").ends_with("bg"),
        "no countable items: no badge"
    );
    assert!(row("scratch").ends_with("bg"));
    let l = rows
        .iter()
        .find(|l| l.contains("解析器"))
        .unwrap()
        .trim_end()
        .to_string();
    assert!(
        l.contains('…') && (l.ends_with("1/4 12m 03s") || l.ends_with("1/4 12m 04s")),
        "name truncated, count and time whole: {l:?}"
    );
    for r in &all {
        assert!(
            crate::ui::text::width(r.trim_end()) <= crate::render::SIDEBAR_W as usize,
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
            session_id: "ses_test".into(),
            event: SessionEvent::Queue { messages },
        }))
    };

    // Our prompt is queued; the Queue event arrives before the Prompt response.
    type_str(&mut app, "my follow-up");
    crate::input::handle(
        &mut app,
        key(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ),
    );
    assert_eq!(app.views["ses_test"].queued.len(), 1);
    ev(
        &mut app,
        vec![
            user("pq1", "please check the parser", peer.clone()),
            user("q1", "my follow-up", MessageSource::Human),
        ],
    );
    let q = &app.views["ses_test"].queued;
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
    assert_eq!(app.views["ses_test"].queued.len(), 2);

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
    let pkey = app.views["ses_test"].queued[0].key;
    app.unqueue("ses_test", pkey);
    assert!(
        !app.pending
            .values()
            .any(|p| matches!(p, Pending::Unqueue { .. }))
    );

    // Delivered: the server announces the empty queue.
    ev(&mut app, vec![]);
    assert!(app.views["ses_test"].queued.is_empty());
}

#[test]
fn history_pages_load_when_scrolled_to_the_top() {
    use crate::app::Pending;
    let mut app = test_app(false);
    let sid = "ses_test";
    let all = long_transcript("h", 60);
    let (old, newest) = all.split_at(80);
    let inf = app.views[sid].info.clone();
    app.views
        .get_mut(sid)
        .unwrap()
        .load_snapshot(SessionSnapshot {
            info: inf,
            messages: newest.to_vec(),
            more_before: true,
            pending_permissions: vec![],
            pending_questions: vec![],
            queued: vec![],
        });
    let older = |app: &App| {
        app.pending.iter().find_map(|(id, p)| match p {
            Pending::Older { before, .. } => Some((*id, before.clone())),
            _ => None,
        })
    };
    render(&mut app, 100, 30);
    app.load_older();
    assert!(older(&app).is_none(), "at the bottom nothing is requested");
    // Scrolled to the top: the page before the first loaded message is requested once.
    app.views
        .get_mut(sid)
        .unwrap()
        .chat
        .scroll_by(-1_000_000, false);
    render(&mut app, 100, 30);
    app.load_older();
    let (id, before) = older(&app).expect("older page requested");
    assert_eq!(before, "hu40");
    app.load_older();
    assert_eq!(
        app.pending
            .values()
            .filter(|p| matches!(p, Pending::Older { .. }))
            .count(),
        1
    );
    // Scrollbar column excluded: its thumb moves when content is added above.
    let shown = |app: &mut App| {
        render(app, 100, 30)
            .1
            .lines()
            .filter(|l| l.contains("h question") || l.contains("h answer"))
            .map(|l| l.trim_end().trim_end_matches(['│', '┃']).to_string())
            .collect::<Vec<_>>()
    };
    let before_screen = shown(&mut app);
    assert!(
        before_screen.iter().any(|l| l.contains("h question 40")),
        "{before_screen:?}"
    );
    let page = json!({ "messages": old[40..].to_vec(), "more_before": true });
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id,
        ok: true,
        result: page,
        error: None,
    }));
    assert_eq!(app.views[sid].messages.len(), 80);
    assert_eq!(app.views[sid].messages[0].id, "hu20");
    // The same content stays on screen; no new request until scrolled up again.
    assert_eq!(shown(&mut app), before_screen);
    app.load_older();
    assert!(older(&app).is_none());
    app.views
        .get_mut(sid)
        .unwrap()
        .chat
        .scroll_by(-1_000_000, false);
    render(&mut app, 100, 30);
    app.load_older();
    let (id, before) = older(&app).expect("next page requested");
    assert_eq!(before, "hu20");
    // A reset in between makes the answer stale: it is dropped.
    let v = app.views.get_mut(sid).unwrap();
    v.apply(SessionEvent::Reset {
        messages: newest.to_vec(),
        more_before: true,
    });
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id,
        ok: true,
        result: json!({ "messages": old[..40].to_vec(), "more_before": false }),
        error: None,
    }));
    assert_eq!(app.views[sid].messages.len(), 40);
    // An error stops further requests (no retry loop).
    app.views
        .get_mut(sid)
        .unwrap()
        .chat
        .scroll_by(-1_000_000, false);
    render(&mut app, 100, 30);
    app.load_older();
    let (id, _) = older(&app).expect("requested after the reset");
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response {
        id,
        ok: false,
        result: json!(null),
        error: Some("gone".into()),
    }));
    render(&mut app, 100, 30);
    app.load_older();
    assert!(older(&app).is_none());
    assert!(!app.views[sid].more_before);
}

#[test]
fn editor_mouse_drag_double_and_triple_click_select() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = test_app(false);
    type_str(&mut app, "hello brave new world");
    let (term, screen) = render(&mut app, 100, 30);
    let y = screen
        .lines()
        .position(|l| l.contains("hello brave new world"))
        .expect("editor row") as u16;
    let x0 = (0..100)
        .find(|&x| term.backend().buffer()[(x, y)].symbol() == "h")
        .unwrap();
    let at = |col: u16| x0 + col;
    // Press on "brave", drag to the end of "new": that range is selected.
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), at(6), y),
    );
    assert_eq!(app.editor.selected_text(), None);
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Drag(MouseButton::Left), at(10), y),
    );
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Drag(MouseButton::Left), at(15), y),
    );
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Up(MouseButton::Left), at(15), y),
    );
    assert_eq!(app.editor.selected_text().as_deref(), Some("brave new"));
    assert_eq!(app.editor.cursor, 15);
    // Dragging backwards works too.
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), at(11), y),
    );
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Drag(MouseButton::Left), at(0), y),
    );
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Up(MouseButton::Left), at(0), y),
    );
    assert_eq!(app.editor.selected_text().as_deref(), Some("hello brave"));
    // Right click copies the editor selection.
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Right), at(3), y),
    );
    assert!(
        app.toasts
            .items
            .iter()
            .any(|t| t.text == "Copied selection (11 chars)")
    );
    assert_eq!(app.editor.selected_text(), None);
    // Double click: the word; triple click: the line. A plain click clears.
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), at(17), y),
    );
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Up(MouseButton::Left), at(17), y),
    );
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), at(17), y),
    );
    assert_eq!(app.editor.selected_text().as_deref(), Some("world"));
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Up(MouseButton::Left), at(17), y),
    );
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), at(17), y),
    );
    assert_eq!(
        app.editor.selected_text().as_deref(),
        Some("hello brave new world")
    );
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Up(MouseButton::Left), at(17), y),
    );
    app.last_click = None;
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), at(2), y),
    );
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Up(MouseButton::Left), at(2), y),
    );
    assert_eq!(app.editor.selected_text(), None);
    assert_eq!(app.editor.cursor, 2);
    // Typing replaces the selection.
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), at(0), y),
    );
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Drag(MouseButton::Left), at(5), y),
    );
    crate::input::handle(
        &mut app,
        mouse(MouseEventKind::Up(MouseButton::Left), at(5), y),
    );
    type_str(&mut app, "bye");
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
    app.views
        .get_mut("ses_test")
        .unwrap()
        .apply(SessionEvent::Reset {
            more_before: false,
            messages: long_transcript("a", pairs),
        });
    app
}

#[test]
fn hold_keeps_transcript_still_when_the_retry_notice_goes_away() {
    let last = "second line of answer 29";
    let mut app = long_session_app(30);
    set_status(&mut app, "ses_test", SessionStatus::Busy);
    let (_, s) = render(&mut app, 100, 30);
    let at_rest = row_of(&s, last);

    set_status(&mut app, "ses_test", retrying());
    let (_, s) = render(&mut app, 100, 30);
    let with_notice = row_of(&s, last);
    assert!(with_notice < at_rest, "the notice pushes the transcript up");

    set_status(&mut app, "ses_test", SessionStatus::Busy);
    let (_, s) = render(&mut app, 100, 30);
    assert_eq!(row_of(&s, last), with_notice, "held: nothing moves\n{s}");

    set_status(&mut app, "ses_test", SessionStatus::Idle);
    let (_, s) = render(&mut app, 100, 30);
    assert_eq!(row_of(&s, last), at_rest, "released at the end of the turn");
}

#[test]
fn hold_absorbs_the_removal_of_an_unfinished_message() {
    // A retry makes the server remove the unfinished assistant message.
    let last_answer = "second line of answer 29";
    let mut app = long_session_app(30);
    set_status(&mut app, "ses_test", SessionStatus::Busy);
    let streaming = msg(
        "streaming",
        Role::Assistant,
        MessageSource::Human,
        ts(10, 5, 0),
        &"partial output\n".repeat(8),
    );
    app.views
        .get_mut("ses_test")
        .unwrap()
        .apply(SessionEvent::Message { message: streaming });
    let (_, s) = render(&mut app, 100, 30);
    let before = row_of(&s, last_answer);

    app.views
        .get_mut("ses_test")
        .unwrap()
        .apply(SessionEvent::Removed {
            message_id: "streaming".into(),
        });
    let (_, s) = render(&mut app, 100, 30);
    assert_eq!(row_of(&s, last_answer), before, "held\n{s}");

    set_status(&mut app, "ses_test", SessionStatus::Idle);
    let (_, s) = render(&mut app, 100, 30);
    assert!(row_of(&s, last_answer) > before, "released with the turn");
}

#[test]
fn hold_applies_to_split_tiles() {
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
    let with_queue = row_of(&s, "beta answer 29");
    assert!(with_queue < at_rest, "the queued row pushes the tile up");

    app.views.get_mut("ses_w2").unwrap().queued.clear();
    let (_, s) = render(&mut app, 120, 30);
    assert_eq!(row_of(&s, "beta answer 29"), with_queue, "held\n{s}");

    set_status(&mut app, "ses_w2", SessionStatus::Idle);
    let (_, s) = render(&mut app, 120, 30);
    assert_eq!(row_of(&s, "beta answer 29"), at_rest, "released");
}
