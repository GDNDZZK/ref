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
    Local.from_local_datetime(&today.and_hms_opt(h, m, s).unwrap()).single().unwrap().timestamp_millis()
}

fn info() -> SessionInfo {
    SessionInfo {
        id: "ses_test".into(),
        name: "ref-a1b2".into(),
        title: "Fix the parser".into(),
        agent: "build".into(),
        model: ModelRef { provider: "dgn".into(), model: "claude-opus-5.5".into(), variant: Some("high".into()) },
        cwd: "/home/u/proj".into(),
        created_at: 0,
        updated_at: 0,
        status: SessionStatus::Idle,
        attached: 1,
        live: true,
        parent: None,
        must_report: false,
        context: ContextUsage { used: 42_100, max: 1_000_000, recommend: 600_000, auto: 850_000 },
        todos: Vec::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn tool(call: &str, name: &str, purpose: &str, input: serde_json::Value, state: ToolState, out: Option<&str>, t: i64, te: Option<i64>) -> Part {
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
        parts: vec![Part::Text { text: "Please fix the off-by-one in the parser and run the tests. 顺便检查中文。".into(), time: 0 }],
        model: None,
        agent: Some("build".into()),
        usage: None,
        error: None,
    };
    let reasoning = "First I need to read the parser. The loop uses an inclusive range which reads one past the end. Then run cargo test to confirm the fix works";
    let mut parts = vec![
        Part::Reasoning { text: reasoning.into(), time: ts(14, 2, 12), time_end: Some(ts(14, 2, 15)), provider_data: None },
        Part::Text { text: "I'll look at the parser.\n\n```rust\nfor i in 0..items.len() {}\n```".into(), time: 0 },
        tool("c1", "bash", "Run the unit tests", json!({"command": "cargo test -p parser"}), ToolState::Done, Some("test result: ok"), ts(14, 2, 15), Some(ts(14, 2, 15) + 1400)),
        tool("c2", "edit", "Fix the off-by-one in parser", json!({"path": "src/parser.rs"}), ToolState::Error, Some("tag mismatch: file changed"), ts(14, 2, 17), Some(ts(14, 2, 18))),
        tool("c3", "read", "Read the config", json!({"path": "Cargo.toml"}), ToolState::Denied, Some("denied"), ts(14, 2, 18), Some(ts(14, 2, 18))),
    ];
    if streaming {
        parts.push(tool("c4", "grep", "Find other inclusive ranges", json!({"pattern": "0..="}), ToolState::Running, None, ref_proto::now_ms() - 3000, None));
        parts.push(Part::Reasoning { text: "Now thinking about whether the grep results include test files as well, which would be noise for this particular".into(), time: ref_proto::now_ms() - 2000, time_end: None, provider_data: None });
    }
    let asst = Message {
        id: "m2".into(),
        role: Role::Assistant,
        time: ts(14, 2, 13),
        time_end: if streaming { None } else { Some(ts(14, 2, 20)) },
        source: MessageSource::Human,
        parts,
        model: Some(ModelRef { provider: "dgn".into(), model: "claude-opus-5.5".into(), variant: None }),
        agent: Some("build".into()),
        usage: Some(Usage { input: 1000, output: 300, cache_read: 11_000, cache_write: 0, reasoning: 0 }),
        error: None,
    };
    vec![user, asst]
}

pub fn test_app(streaming: bool) -> App {
    let (tx, rx) = mpsc::unbounded_channel();
    // Keep the receiver alive so requests are accepted (and recorded as pending).
    std::mem::forget(rx);
    let mut app = App::new(Client::new(tx), "/home/u/proj".into(), InitPlan::default());
    app.connected = true;
    app.ever_connected = true;
    app.config = Some(ServerConfigInfo {
        agents: vec![AgentInfo { name: "build".into(), description: String::new(), model: None, creatable: true, can_spawn: true, color: None }],
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
    v.load_snapshot(SessionSnapshot { info: i, messages: transcript(streaming), pending_permissions: vec![] });
    app.views.insert("ses_test".into(), v);
    app.current = Some("ses_test".into());
    app
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
    assert!(s.contains("14:02:13 → 14:02:20 (7.0s)"), "assistant start/end");
    assert!(s.contains("1.4s") && s.contains("14:02:15"), "tool duration + time");
    // Tool name + purpose.
    assert!(s.contains("bash  Run the unit tests"));
    assert!(s.contains("edit  Fix the off-by-one in parser"));
    assert!(s.contains("✗") && s.contains("tag mismatch"), "error first line");
    assert!(s.contains("⊘"), "denied icon");
    // Reasoning collapsed: one line ending with the reasoning tail.
    let rline = s.lines().find(|l| l.contains("Thought")).expect("reasoning line");
    assert!(rline.contains("· 3.0s"));
    assert!(rline.trim_end().ends_with("confirm the fix works"), "{rline}");
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
    assert!(s.contains("running 3s") || s.contains("running 2s") || s.contains("running 4s"), "running tool counter");
    let rline = s.lines().find(|l| l.contains("Thinking")).expect("live reasoning line");
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
    assert!(s.contains("╭─input"));
    assert!(s.contains("\"command\": \"cargo test -p parser\""));
    assert!(s.contains("├─output"));
    assert!(s.contains("test result: ok"));
}

#[test]
fn click_copy_and_toggle_hits() {
    let mut app = test_app(false);
    let (term, s) = render(&mut app, 100, 30);
    let buf = term.backend().buffer().clone();
    // Find the assistant footer copy button.
    let lines: Vec<&str> = s.lines().collect();
    let y = lines.iter().position(|l| l.contains("Build · claude") && l.contains("⧉ copy")).expect("footer") as u16;
    let mut x = 0u16;
    while x < 100 && buf[(x, y)].symbol() != "⧉" {
        x += 1;
    }
    let v = app.views.get_mut("ses_test").unwrap();
    assert_eq!(v.chat.hit_at(x, y), Some(crate::ui::Hit::Copy { msg: "m2".into() }));
    let uy = lines.iter().position(|l| l.contains("14:02:11") && l.contains("⧉ copy")).expect("user meta") as u16;
    let mut ux = 0u16;
    while ux < 100 && buf[(ux, uy)].symbol() != "⧉" {
        ux += 1;
    }
    assert_eq!(v.chat.hit_at(ux + 3, uy), Some(crate::ui::Hit::Copy { msg: "m1".into() }));
    assert_eq!(v.chat.hit_at(ux.saturating_sub(4), uy), None);
    // Tool line toggles.
    let ty = lines.iter().position(|l| l.contains("bash  Run the unit tests")).unwrap() as u16;
    assert_eq!(v.chat.hit_at(10, ty), Some(crate::ui::Hit::Toggle { msg: "m2".into(), part: 2 }));
}

#[test]
fn selection_copy_text() {
    let mut app = test_app(false);
    let (_, s) = render(&mut app, 100, 30);
    let lines: Vec<&str> = s.lines().collect();
    let y = lines.iter().position(|l| l.contains("Please fix the off-by-one")).unwrap() as u16;
    let v = app.views.get_mut("ses_test").unwrap();
    v.chat.mouse_down(0, y, 3);
    let t = v.chat.selected_text().unwrap();
    assert_eq!(t, "Please fix the off-by-one in the parser and run the tests. 顺便检查中文。");
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
        inf.status = if i == 0 { SessionStatus::Busy } else { SessionStatus::Idle };
        let mut v = SessionView::new(inf.clone(), false);
        v.load_snapshot(SessionSnapshot { info: inf.clone(), messages: transcript(i == 0), pending_permissions: vec![] });
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
    v.messages.push(Message { id: "e".into(), role: Role::Assistant, time: 0, time_end: Some(0), source: MessageSource::Human, parts: vec![], model: None, agent: None, usage: None, error: Some("boom\x1b[31m\nsecond".into()) });
    v.messages.push(Message {
        id: "e2".into(),
        role: Role::User,
        time: -5,
        time_end: None,
        source: MessageSource::Compaction,
        parts: vec![Part::Text { text: String::new(), time: 0 }],
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

fn key(code: crossterm::event::KeyCode, mods: crossterm::event::KeyModifiers) -> crossterm::event::Event {
    crossterm::event::Event::Key(crossterm::event::KeyEvent::new(code, mods))
}

fn type_str(app: &mut App, s: &str) {
    for c in s.chars() {
        crate::input::handle(app, key(crossterm::event::KeyCode::Char(c), crossterm::event::KeyModifiers::NONE));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mock_end_to_end() {
    use crossterm::event::{KeyCode, KeyModifiers};
    let (out_tx, out_rx) = mpsc::unbounded_channel();
    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel();
    tokio::spawn(crate::mock::run("/home/u/proj".into(), out_rx, ev_tx));
    let plan = InitPlan { prompt: Some("hello, run the demo".into()), ..Default::default() };
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
    // Let the turn finish.
    for _ in 0..120 {
        if app.views[&sid].messages.iter().any(|m| m.raw_text().contains("Got it")) && !app.views[&sid].is_busy() {
            break;
        }
        pump(&mut app, &mut ev_rx, 250).await;
    }
    let v = &app.views[&sid];
    assert!(!v.is_busy(), "turn finished");
    assert!(v.queued.is_empty(), "queued prompt delivered");
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
    // A background tile asks for permission: the modal names the session; deny it.
    if app.active_permission().is_some() {
        assert!(s.contains("Permission required · docs-写手"));
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        crate::input::handle(&mut app, key(KeyCode::Char('d'), KeyModifiers::NONE));
        assert!(app.active_permission().is_none());
    }
    // Message the focused tile directly.
    let focus = app.watch.focus.clone().expect("focus");
    type_str(&mut app, "ping");
    crate::input::handle(&mut app, key(KeyCode::Enter, KeyModifiers::NONE));
    pump(&mut app, &mut ev_rx, 200).await;
    assert!(app.watch.inputs.get(&focus).is_none_or(|e| e.is_empty()));
    // Back to chat.
    crate::input::handle(&mut app, key(KeyCode::Char('x'), KeyModifiers::CONTROL));
    crate::input::handle(&mut app, key(KeyCode::Char('w'), KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Chat);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn permission_modal_keys_then_quit() {
    use crossterm::event::{KeyCode, KeyModifiers};
    let (out_tx, out_rx) = mpsc::unbounded_channel();
    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel();
    tokio::spawn(crate::mock::run("/home/u/proj".into(), out_rx, ev_tx));
    let mut app = App::new(Client::new(out_tx), "/home/u/proj".into(), InitPlan { watch: true, ..Default::default() });
    for _ in 0..60 {
        pump(&mut app, &mut ev_rx, 250).await;
        render(&mut app, 100, 30);
        if app.active_permission().is_some() {
            break;
        }
    }
    assert!(app.active_permission().is_some());
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    for k in [key(KeyCode::Char('x'), KeyModifiers::CONTROL), key(KeyCode::Char('w'), KeyModifiers::NONE)] {
        crate::input::handle(&mut app, k);
    }
    println!("after ctrl+x w: perm={} mode={:?}", app.active_permission().is_some(), app.mode);
    crate::input::handle(&mut app, key(KeyCode::Char('c'), KeyModifiers::CONTROL));
    println!("after ctrl+c: perm={} quit={}", app.active_permission().is_some(), app.quit);
    crate::input::handle(&mut app, key(KeyCode::Char('c'), KeyModifiers::CONTROL));
    println!("after ctrl+c 2: perm={} quit={}", app.active_permission().is_some(), app.quit);
    assert!(app.quit);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn file_mention_autocomplete() {
    let (out_tx, out_rx) = mpsc::unbounded_channel();
    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel();
    tokio::spawn(crate::mock::run("/home/u/proj".into(), out_rx, ev_tx));
    let mut app = App::new(Client::new(out_tx), "/home/u/proj".into(), InitPlan::default());
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
    assert!(ac.items.iter().any(|i| i.1 == "src/main.rs"), "{:?}", ac.items);
    let (_, s) = render(&mut app, 100, 30);
    println!("{s}");
    assert!(s.contains("files") && s.contains("src/main.rs"));
    crate::input::handle(&mut app, key(crossterm::event::KeyCode::Tab, crossterm::event::KeyModifiers::NONE));
    assert!(app.editor.text.starts_with("look at @src/"), "{}", app.editor.text);
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
    let mut app = App::new(Client::new(out_tx), "/home/u/proj".into(), InitPlan::default());
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
    let y = lines.iter().position(|l| l.contains("I'll look at the parser.")).unwrap() as u16;
    let me = |kind, x, y| Event::Mouse(MouseEvent { kind, column: x, row: y, modifiers: KeyModifiers::NONE });
    app.screen = ratatui::layout::Rect::new(0, 0, 100, 30);
    crate::input::handle(&mut app, me(MouseEventKind::Down(MouseButton::Left), 3, y));
    crate::input::handle(&mut app, me(MouseEventKind::Drag(MouseButton::Left), 10, y));
    crate::input::handle(&mut app, me(MouseEventKind::Up(MouseButton::Left), 10, y));
    assert!(app.has_selection());
    assert!(app.toasts.items.is_empty(), "no auto copy");
    let (_, _) = render(&mut app, 100, 30);
    crate::input::handle(&mut app, crossterm::event::Event::Key(crossterm::event::KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
    assert!(!app.has_selection());
    assert!(app.toasts.items.iter().any(|t| t.text.contains("Copied selection (8 chars)")), "{:?}", app.toasts.items.iter().map(|t| &t.text).collect::<Vec<_>>());
    // A plain click on a tool line toggles it.
    let ty = lines.iter().position(|l| l.contains("bash  Run the unit tests")).unwrap() as u16;
    crate::input::handle(&mut app, me(MouseEventKind::Down(MouseButton::Left), 20, ty));
    crate::input::handle(&mut app, me(MouseEventKind::Up(MouseButton::Left), 20, ty));
    let (_, s) = render(&mut app, 100, 30);
    assert!(s.contains("╭─input"));
}

#[test]
fn retry_status_and_reconnect() {
    let mut app = test_app(false);
    let v = app.views.get_mut("ses_test").unwrap();
    let mut i = v.info.clone();
    i.status = SessionStatus::Retrying { attempt: 3, next_at: ref_proto::now_ms() + 11_500, error: "rate limited (429)".into() };
    v.apply(SessionEvent::Info { info: i });
    let (_, s) = render(&mut app, 100, 30);
    assert!(s.contains("Retrying in 12s (attempt 3): rate limited (429)"), "{s}");
    assert!(s.contains("retrying in 12s (#3)"));
    // Disconnect → banner; reconnect → re-attach current session as foreground.
    app.on_net(crate::net::NetEvent::Disconnected("connection closed".into()));
    let (_, s) = render(&mut app, 100, 30);
    assert!(s.contains("Disconnected — reconnecting…"));
    app.on_net(crate::net::NetEvent::Connected);
    assert!(app.pending.values().any(|p| matches!(p, crate::app::Pending::Attach { sid, fg: true } if sid == "ses_test")));
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
                Request::Hello { .. } => ServerFrame::Response { id, ok: true, result: json!({}), error: None },
                _ => ServerFrame::Response { id, ok: true, result: json!({"sessions": []}), error: None },
            };
            ws.send(M::text(serde_json::to_string(&reply).unwrap())).await.unwrap();
            let toast = ServerFrame::Toast { level: ToastLevel::Info, text: "hi".into() };
            ws.send(M::text(serde_json::to_string(&toast).unwrap())).await.unwrap();
            ws.send(M::text("{\"kind\":\"future_frame\"}".to_string())).await.unwrap();
        }
    });
    let (out_tx, out_rx) = mpsc::unbounded_channel();
    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel();
    let target = crate::launcher::Target::Explicit { url: format!("ws://127.0.0.1:{port}/"), token: Some("tok".into()) };
    tokio::spawn(crate::net::run(target, "/my/cwd".into(), out_rx, ev_tx));
    let first = tokio::time::timeout(std::time::Duration::from_secs(3), ev_rx.recv()).await.unwrap().unwrap();
    assert!(matches!(first, crate::net::NetEvent::Connected), "{first:?}");
    let hello = seen_rx.recv().await.unwrap();
    assert!(hello.contains("\"op\":\"hello\"") && hello.contains("/my/cwd") && hello.contains("\"token\":\"tok\""), "{hello}");
    // Toast after hello arrives; unknown frame kinds are ignored.
    let t = ev_rx.recv().await.unwrap();
    assert!(matches!(t, crate::net::NetEvent::Frame(ServerFrame::Toast { .. })));
    out_tx.send(ClientFrame { id: Some(7), req: Request::ListSessions }).unwrap();
    let r = ev_rx.recv().await.unwrap();
    assert!(matches!(r, crate::net::NetEvent::Frame(ServerFrame::Response { id: 7, ok: true, .. })), "{r:?}");
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
        v.apply(SessionEvent::Reset { messages: msgs });
    }
    let t0 = std::time::Instant::now();
    render(&mut app, 120, 40);
    let first = t0.elapsed();
    // Streaming into the last message: only that message is re-rendered.
    let t1 = std::time::Instant::now();
    for k in 0..30 {
        let v = app.views.get_mut("ses_test").unwrap();
        let id = v.messages.last().unwrap().id.clone();
        v.apply(SessionEvent::Delta { message_id: id, index: 1, delta: Delta::Text { text: format!(" more text {k}") } });
        render(&mut app, 120, 40);
    }
    let per = t1.elapsed() / 30;
    println!("first full layout {first:?}, per streaming frame {per:?}");
    assert!(per * 4 < first, "incremental frames must be much cheaper than the first layout ({per:?} vs {first:?})");
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
    app.editor.attach_image(ImageData { mime: "image/png".into(), data: "AAAA".into() }, Some("clipboard".into()));
    type_str(&mut app, "what is this?");
    let (_, s) = render(&mut app, 100, 30);
    assert!(s.contains("[Image 1] what is this?"));
    assert!(s.contains("1 image"));
}

fn mouse(kind: crossterm::event::MouseEventKind, x: u16, y: u16) -> crossterm::event::Event {
    crossterm::event::Event::Mouse(crossterm::event::MouseEvent { kind, column: x, row: y, modifiers: crossterm::event::KeyModifiers::NONE })
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
            *text = "alpha beta gamma delta 中文字符测试中文字符测试中文字符测试 epsilon zeta".into();
        }
        v.versions[0] += 1000;
    }
    let (w, h) = (40u16, 30u16);
    let (_, s) = render(&mut app, w, h);
    app.screen = ratatui::layout::Rect::new(0, 0, w, h);
    let lines: Vec<&str> = s.lines().collect();
    let y0 = lines.iter().position(|l| l.contains("alpha beta")).expect("first row") as u16;
    let y1 = lines.iter().position(|l| l.contains("zeta")).expect("last row") as u16;
    assert!(y1 >= y0 + 2, "text must wrap over several rows:\n{s}");
    for l in &lines {
        assert!(text::width(l) <= w as usize, "row overflows: {l:?}");
    }
    let row = lines[y0 as usize];
    let x0 = text::width(&row[..row.find("alpha").unwrap()]) as u16;
    crate::input::handle(&mut app, mouse(MouseEventKind::Down(MouseButton::Left), x0, y0));
    crate::input::handle(&mut app, mouse(MouseEventKind::Drag(MouseButton::Left), 20, y0 + 1));
    crate::input::handle(&mut app, mouse(MouseEventKind::Drag(MouseButton::Left), w - 2, y1));
    crate::input::handle(&mut app, mouse(MouseEventKind::Up(MouseButton::Left), w - 2, y1));
    // Mouse up never copies.
    assert_eq!(crate::clipboard::take_last_copy(), None);
    assert!(app.has_selection());
    // `y` copies (empty prompt); soft-wrapped rows are joined without inserted breaks.
    crate::input::handle(&mut app, key(KeyCode::Char('y'), KeyModifiers::NONE));
    let got = crate::clipboard::take_last_copy().expect("copied");
    assert_eq!(got, "alpha beta gamma delta 中文字符测试中文字符测试中文字符测试 epsilon zeta");
    assert!(!app.has_selection());
    assert!(app.editor.is_empty(), "y must not be typed into the prompt when copying");
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
    crate::input::handle(&mut app, mouse(MouseEventKind::Down(MouseButton::Left), x + 1, y));
    let x2 = find_col(&term, y, "查").unwrap();
    crate::input::handle(&mut app, mouse(MouseEventKind::Drag(MouseButton::Left), x2 + 1, y));
    crate::input::handle(&mut app, mouse(MouseEventKind::Up(MouseButton::Left), x2 + 1, y));
    crate::input::handle(&mut app, key(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert_eq!(crate::clipboard::take_last_copy().as_deref(), Some("顺便检查"));
}

#[test]
fn copy_button_copies_raw_markdown() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = test_app(false);
    let (term, s) = render(&mut app, 100, 30);
    app.screen = ratatui::layout::Rect::new(0, 0, 100, 30);
    let y = s.lines().position(|l| l.contains("Build · claude") && l.contains("⧉ copy")).unwrap() as u16;
    let x = find_col(&term, y, "⧉").unwrap();
    crate::input::handle(&mut app, mouse(MouseEventKind::Down(MouseButton::Left), x + 2, y));
    crate::input::handle(&mut app, mouse(MouseEventKind::Up(MouseButton::Left), x + 2, y));
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
    let y = s.lines().position(|l| l.contains("I'll look at the parser.")).unwrap() as u16;
    crate::input::handle(&mut app, mouse(MouseEventKind::Down(MouseButton::Left), 5, y));
    crate::input::handle(&mut app, mouse(MouseEventKind::Up(MouseButton::Left), 5, y));
    assert!(!app.has_selection());
    assert_eq!(crate::clipboard::take_last_copy(), None);
    // Double click selects a word; still nothing copied.
    crate::input::handle(&mut app, mouse(MouseEventKind::Down(MouseButton::Left), 5, y));
    crate::input::handle(&mut app, mouse(MouseEventKind::Up(MouseButton::Left), 5, y));
    assert!(app.has_selection());
    assert_eq!(crate::clipboard::take_last_copy(), None);
}

fn fail_pending(app: &mut App, pred: impl Fn(&crate::app::Pending) -> bool, err: &str) {
    let id = *app.pending.iter().find(|(_, p)| pred(p)).map(|(k, _)| k).expect("pending request");
    app.on_net(crate::net::NetEvent::Frame(ServerFrame::Response { id, ok: false, result: serde_json::Value::Null, error: Some(err.into()) }));
}

#[test]
fn reattach_failure_of_empty_session_starts_new_one() {
    use crate::app::Pending;
    let mut app = test_app(false);
    app.views.get_mut("ses_test").unwrap().apply(SessionEvent::Reset { messages: vec![] });
    app.on_net(crate::net::NetEvent::Disconnected("connection closed".into()));
    app.on_net(crate::net::NetEvent::Connected);
    fail_pending(&mut app, |p| matches!(p, Pending::Attach { fg: true, .. }), "session not found");
    assert!(app.current.is_none());
    assert!(app.pending.values().any(|p| matches!(p, Pending::Created { .. })), "a fresh session is requested");
}

#[test]
fn failed_prompt_text_returns_to_editor() {
    use crate::app::Pending;
    let mut app = test_app(false);
    type_str(&mut app, "do the thing");
    crate::input::handle(&mut app, key(crossterm::event::KeyCode::Enter, crossterm::event::KeyModifiers::NONE));
    assert!(app.editor.is_empty());
    fail_pending(&mut app, |p| matches!(p, Pending::Prompt { .. }), "Session ses_test is not live");
    assert_eq!(app.editor.text, "do the thing");
    assert!(app.toasts.items.iter().any(|t| t.text.contains("Prompt not sent")));
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
    app.views.get_mut("ses_test").unwrap().apply(SessionEvent::Permission { request: req });
    let (_, s) = render(&mut app, 100, 30);
    assert!(s.contains("Permission required") && s.contains("$ ls -la"), "{s}");
    // Within the grace period: 'a' / 'd' must not answer the modal; they are typed text.
    type_str(&mut app, "ad");
    crate::input::handle(&mut app, key(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.active_permission().is_some(), "modal not answered by early keys");
    assert_eq!(app.editor.text, "ad");
    // After the grace period the modal reacts.
    app.perm.as_mut().unwrap().opened -= std::time::Duration::from_millis(500);
    crate::input::handle(&mut app, key(KeyCode::Char('a'), KeyModifiers::NONE));
    assert!(app.active_permission().is_none());
    assert_eq!(app.editor.text, "ad");
    // The modal is sized to its content, not the full screen height.
    let mut app = test_app(false);
    let req = PermissionRequest { session_id: "ses_test".into(), message_id: "m2".into(), call_id: "c10".into(), tool: "bash".into(), purpose: "x".into(), input: json!({"command": "ls"}), time: 0 };
    app.views.get_mut("ses_test").unwrap().apply(SessionEvent::Permission { request: req });
    render(&mut app, 100, 40);
    assert!(app.perm.as_ref().unwrap().rect.height <= 14, "{:?}", app.perm.as_ref().unwrap().rect);
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
