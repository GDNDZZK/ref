//! Render tests (ratatui TestBackend) and app-level behavior tests.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use ref_proto::*;
use serde_json::json;
use tokio::sync::mpsc;

use crate::app::{App, Mode, StartOpts};
use crate::conn::{Conn, ConnEvent};
use crate::state::tests::{info, msg};

fn app() -> (App, mpsc::UnboundedReceiver<ClientMsg>) {
    let (conn, rx) = Conn::capture();
    let (tx, _arx) = mpsc::unbounded_channel();
    let mut a = App::new(conn, StartOpts { cwd: "/tmp".into(), ..Default::default() }, tx);
    a.history = crate::history::History::default();
    a.connected = true;
    a.startup_done = true;
    a.highlight = false;
    (a, rx)
}

fn catalog() -> Catalog {
    Catalog {
        models: vec![ModelEntry {
            id: "anthropic/claude".into(),
            provider_name: "Anthropic".into(),
            name: "Claude".into(),
            variants: vec!["high".into(), "max".into()],
            context: 200_000,
            image_input: true,
            reasoning: true,
        }],
        agents: vec![
            AgentEntry { name: "build".into(), description: "".into(), model: "anthropic/claude".into(), variant: None, color: None, spawnable: false, can_spawn: true },
            AgentEntry { name: "plan".into(), description: "".into(), model: "anthropic/claude".into(), variant: None, color: None, spawnable: true, can_spawn: false },
        ],
        default_agent: "build".into(),
        mcp: vec![McpStatus { name: "github".into(), status: "connected".into(), tools: 12, error: None }],
        commands: vec![CommandEntry { name: "review".into(), description: "Review changes".into() }],
    }
}

fn tool(id: &str, name: &str, purpose: &str, state: ToolState, t: i64) -> Part {
    Part::Tool(ToolPart {
        id: id.into(),
        call_id: format!("call-{id}"),
        name: name.into(),
        purpose: purpose.into(),
        input: json!({"command": "ls -la", "purpose": purpose}),
        input_raw: String::new(),
        state,
        output: if state == ToolState::Done { "total 0\nfile.txt".into() } else { String::new() },
        images: vec![],
        time: t,
        time_start: Some(t),
        time_end: if state.is_final() { Some(t + 1500) } else { None },
        run_at: None,
        meta: None,
    })
}

/// A transcript: user question, assistant with reasoning + tool + markdown answer.
fn load_transcript(a: &mut App) -> (i64, i64) {
    let now = now_ms();
    let t0 = now - 60_000;
    let user = msg("m1", Role::User, "列出文件 please", t0);
    let mut asst = msg("m2", Role::Assistant, "", t0 + 1000);
    asst.agent = Some("build".into());
    asst.model = Some("anthropic/claude".into());
    asst.parts.push(Part::Reasoning(ReasoningPart {
        id: "r1".into(),
        text: "The user wants a listing.\nI should run ls to see the files in the directory".into(),
        time: t0 + 1000,
        time_end: Some(t0 + 4000),
        meta: None,
    }));
    asst.parts.push(tool("t1", "bash", "List directory contents", ToolState::Done, t0 + 4000));
    asst.parts.push(Part::Text(TextPart { id: "x1".into(), text: "Here are the **files**:\n\n- `file.txt`\n".into(), time: t0 + 6000, meta: None }));
    asst.time_end = Some(t0 + 13_300);
    asst.finish = Some("stop".into());
    asst.usage = Some(Usage { input: 1200, output: 340, ..Default::default() });
    a.on_server(ServerMsg::Welcome {
        instance: InstanceInfo { pid: 1, version: "0".into(), bind: "127.0.0.1".into(), port: 1, allow_hosts: vec![], idle_timeout: 0, started: 0, data_dir: String::new() },
        catalog: catalog(),
        protocol: 1,
        client_id: "c".into(),
    });
    let r = a.req(crate::app::Req::Create);
    a.on_server(ServerMsg::SessionSnapshot { req: r, info: info("s1"), messages: vec![user, asst], permissions: vec![] });
    (t0, now)
}

fn render(a: &mut App, w: u16, h: u16) -> Vec<String> {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| super::draw(f, a)).unwrap();
    let buf = t.backend().buffer().clone();
    (0..h)
        .map(|y| {
            let mut s = String::new();
            let mut skip = 0;
            for x in 0..w {
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                let sym = buf[(x, y)].symbol();
                s.push_str(sym);
                skip = unicode_width::UnicodeWidthStr::width(sym).saturating_sub(1);
            }
            s
        })
        .collect()
}

fn screen(lines: &[String]) -> String {
    lines.join("\n")
}

#[test]
fn renders_transcript() {
    let (mut a, _rx) = app();
    let (t0, _) = load_transcript(&mut a);
    let lines = render(&mut a, 100, 40);
    let s = screen(&lines);
    // Header.
    assert!(lines[0].contains("name-s1"), "{s}");
    assert!(lines[0].contains("build"));
    assert!(lines[0].contains("claude"));
    assert!(lines[0].contains("1.0k/200k (1%)"), "{}", lines[0]);
    assert!(lines[0].contains("$0.00"));
    // Times on user message, tool line and assistant footer.
    let t_user = crate::util::fmt_time(t0);
    let t_tool = crate::util::fmt_time(t0 + 4000);
    assert!(s.contains(&t_user), "{s}");
    assert!(s.contains(&t_tool));
    assert!(s.contains("列出文件 please"));
    // Collapsed reasoning shows duration + tail.
    let think = lines.iter().find(|l| l.contains("Thought for 3s")).expect(&s);
    assert!(think.contains("files in the directory"), "{think}");
    // Tool line: icon, name, purpose, duration.
    let tl = lines.iter().find(|l| l.contains("bash")).expect(&s);
    assert!(tl.contains("✓ bash  List directory contents"), "{tl}");
    assert!(tl.contains("1.5s"));
    // Markdown.
    assert!(s.contains("Here are the files:"));
    assert!(s.contains("• file.txt"));
    // Footer with copy button.
    let foot = lines.iter().find(|l| l.contains("▣ build")).expect(&s);
    assert!(foot.contains("12.3s"), "{foot}");
    assert!(foot.contains("↑1.2k ↓340"));
    assert!(foot.contains("⧉ copy"));
    assert!(s.contains("Ask anything"));
    // Hint line.
    assert!(lines[39].contains("ctrl+p"), "{}", lines[39]);
}

#[test]
fn renders_cjk_without_overflow() {
    let (mut a, _rx) = app();
    load_transcript(&mut a);
    let long = "这是一段很长的中文回答，用来测试在窄终端中是否正确换行并且不会超出边界。".repeat(3);
    a.on_server(ServerMsg::MessageUpsert { session_id: "s1".into(), message: msg("m3", Role::Assistant, &long, now_ms()) });
    let lines = render(&mut a, 50, 40);
    for l in &lines {
        assert!(unicode_width::UnicodeWidthStr::width(l.as_str()) <= 50, "{l}");
    }
    assert!(screen(&lines).contains("这是一段很长的"));
}

fn click(a: &mut App, x: u16, y: u16) {
    a.on_event(Event::Mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE }));
    a.on_event(Event::Mouse(MouseEvent { kind: MouseEventKind::Up(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE }));
}

fn find(lines: &[String], pat: &str) -> Option<(u16, u16)> {
    for (y, l) in lines.iter().enumerate() {
        if let Some(bi) = l.find(pat) {
            let x = unicode_width::UnicodeWidthStr::width(&l[..bi]);
            return Some((x as u16, y as u16));
        }
    }
    None
}

#[tokio::test]
async fn click_expands_tool_and_copies() {
    let (mut a, _rx) = app();
    load_transcript(&mut a);
    let lines = render(&mut a, 100, 40);
    let (x, y) = find(&lines, "✓ bash").unwrap();
    click(&mut a, x + 2, y);
    let lines = render(&mut a, 100, 40);
    let s = screen(&lines);
    assert!(s.contains("┌ input"), "{s}");
    assert!(s.contains("\"command\": \"ls -la\""));
    assert!(s.contains("┌ output"));
    assert!(s.contains("file.txt"));
    // Copy button on the assistant footer.
    let foot_y = lines.iter().position(|l| l.contains("▣ build")).unwrap() as u16;
    let (cx, _) = find(&lines[foot_y as usize..], "⧉ copy").unwrap();
    click(&mut a, cx + 1, foot_y);
    assert!(a.toasts.items.iter().any(|t| t.text.starts_with("Copied")), "{:?}", a.toasts.items.iter().map(|t| &t.text).collect::<Vec<_>>());
    let raw = a.store.get("s1").unwrap().message("m2").unwrap().raw_text();
    assert!(a.toasts.items.iter().any(|t| t.text == format!("Copied {} chars", raw.chars().count())));
    assert_eq!(last_copy().as_deref(), Some(raw.as_str()));
    // Copy on the user message header.
    let lines = render(&mut a, 100, 40);
    let (ux, uy) = find(&lines, "⧉ copy").unwrap();
    click(&mut a, ux + 2, uy);
    assert_eq!(last_copy().as_deref(), Some("列出文件 please"));
}

fn last_copy() -> Option<String> {
    crate::clipboard::LAST_COPY.with(|c| c.borrow_mut().take())
}

#[tokio::test]
async fn copy_button_on_every_message() {
    let (mut a, _rx) = app();
    load_transcript(&mut a);
    // Tool-only assistant message.
    let mut m = msg("m3", Role::Assistant, "", now_ms());
    m.parts.push(tool("t3", "bash", "Show date", ToolState::Done, now_ms()));
    m.time_end = Some(now_ms());
    m.finish = Some("tool_use".into());
    a.on_server(ServerMsg::MessageUpsert { session_id: "s1".into(), message: m });
    // Error-only assistant message.
    let mut e = msg("m4", Role::Assistant, "", now_ms() + 1);
    e.error = Some("provider exploded".into());
    e.time_end = Some(now_ms());
    e.finish = Some("error".into());
    a.on_server(ServerMsg::MessageUpsert { session_id: "s1".into(), message: e });
    let lines = render(&mut a, 100, 50);
    let copies: Vec<(u16, u16)> = lines
        .iter()
        .enumerate()
        .filter_map(|(y, l)| l.find("⧉ copy").map(|bi| (unicode_width::UnicodeWidthStr::width(&l[..bi]) as u16, y as u16)))
        .collect();
    assert_eq!(copies.len(), 4, "{}", screen(&lines));
    let (x, y) = copies[2];
    click(&mut a, x + 1, y);
    let c = last_copy().unwrap();
    assert!(c.starts_with("bash {"), "{c}");
    assert!(c.contains("\"command\": \"ls -la\""));
    let (x, y) = copies[3];
    click(&mut a, x + 1, y);
    assert_eq!(last_copy().as_deref(), Some("provider exploded"));
}

#[test]
fn reasoning_expand_and_streaming_tail() {
    let (mut a, _rx) = app();
    load_transcript(&mut a);
    let mut m = msg("m3", Role::Assistant, "", now_ms());
    m.parts.push(Part::Reasoning(ReasoningPart { id: "r2".into(), text: "step one. ".repeat(30) + "LATEST_TOKENS", time: now_ms(), time_end: None, meta: None }));
    a.on_server(ServerMsg::MessageUpsert { session_id: "s1".into(), message: m });
    let lines = render(&mut a, 80, 40);
    let l = lines.iter().find(|l| l.contains("Thinking")).unwrap();
    assert!(l.trim_end().ends_with("LATEST_TOKENS"), "{l}");
    assert!(unicode_width::UnicodeWidthStr::width(l.as_str()) <= 80);
    a.on_server(ServerMsg::PartDelta { session_id: "s1".into(), message_id: "m3".into(), part_id: "r2".into(), delta: " MORE".into() });
    let lines = render(&mut a, 80, 40);
    assert!(lines.iter().any(|l| l.trim_end().ends_with("LATEST_TOKENS MORE")));
}

#[test]
fn permission_prompt_and_decision() {
    let (mut a, mut rx) = app();
    load_transcript(&mut a);
    a.on_server(ServerMsg::PermissionAsk {
        request: PermissionRequest {
            session_id: "s1".into(),
            message_id: "m2".into(),
            part_id: "t9".into(),
            call_id: "c9".into(),
            tool: "write".into(),
            purpose: "Create config".into(),
            input: json!({"path": "/tmp/x.toml", "content": "a = 1\nb = 2", "purpose": "Create config"}),
            time: now_ms(),
        },
    });
    let lines = render(&mut a, 100, 40);
    let s = screen(&lines);
    assert!(s.contains("Permission required"), "{s}");
    assert!(s.contains("write"));
    assert!(s.contains("\"path\": \"/tmp/x.toml\""));
    assert!(s.contains("[a] allow once"));
    assert!(s.contains("[A] always (session)"));
    assert!(s.contains("[d] deny"));
    while rx.try_recv().is_ok() {}
    // Keys right after the prompt appears are ignored (typing guard).
    a.on_event(Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)));
    assert!(rx.try_recv().is_err());
    assert!(a.editor.is_empty());
    a.perm_since = Some(("c9".into(), std::time::Instant::now() - std::time::Duration::from_secs(1)));
    a.on_event(Event::Key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT)));
    match rx.try_recv().unwrap() {
        ClientMsg::Permission { call_id, decision, .. } => {
            assert_eq!(call_id, "c9");
            assert_eq!(decision, PermissionDecision::Always);
        }
        other => panic!("{other:?}"),
    }
    assert!(a.pending_permission().is_none());
}

fn typ(a: &mut App, s: &str) {
    for c in s.chars() {
        a.on_event(Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)));
    }
}

#[test]
fn submit_prompt_and_slash_popup() {
    let (mut a, mut rx) = app();
    load_transcript(&mut a);
    while rx.try_recv().is_ok() {}
    typ(&mut a, "hello 世界");
    a.on_event(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
    match rx.try_recv().unwrap() {
        ClientMsg::Prompt { session_id, text, .. } => {
            assert_eq!(session_id, "s1");
            assert_eq!(text, "hello 世界");
        }
        other => panic!("{other:?}"),
    }
    assert!(a.editor.is_empty());
    typ(&mut a, "/mo");
    assert!(a.popup.is_some());
    let lines = render(&mut a, 100, 40);
    assert!(screen(&lines).contains("/models"));
    a.on_event(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
    assert!(a.dialog.is_some());
    let lines = render(&mut a, 100, 40);
    let s = screen(&lines);
    assert!(s.contains("Anthropic"), "{s}");
    assert!(s.contains("Claude"));
    assert!(s.contains("reasoning"));
    a.on_event(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
    assert!(matches!(rx.try_recv().unwrap(), ClientMsg::SetModel { .. }));
}

#[test]
fn esc_twice_interrupts() {
    let (mut a, mut rx) = app();
    load_transcript(&mut a);
    let mut i = info("s1");
    i.status = SessionStatus::Busy;
    a.on_server(ServerMsg::SessionUpdated { info: i });
    while rx.try_recv().is_ok() {}
    a.on_event(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
    assert!(rx.try_recv().is_err());
    let lines = render(&mut a, 100, 40);
    assert!(screen(&lines).contains("press esc again to interrupt"));
    a.on_event(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
    assert!(matches!(rx.try_recv().unwrap(), ClientMsg::Abort { .. }));
}

#[test]
fn retry_status_header_and_bar() {
    let (mut a, _rx) = app();
    load_transcript(&mut a);
    let mut i = info("s1");
    let err = "HTTP 429 Too Many Requests: {\"error\":{\"message\":\"Rate limit reached for requests per minute. Please try again later.\",\"type\":\"rate_limit\"}}";
    i.status = SessionStatus::Retrying { attempt: 3, at: now_ms() + 4000, error: err.into() };
    a.on_server(ServerMsg::SessionUpdated { info: i });
    for w in [60u16, 80, 130] {
        let lines = render(&mut a, w, 30);
        let s = screen(&lines);
        // Header keeps the short status readable at every width.
        assert!(lines[0].contains("retry #3 in 4s"), "w={w}: {}", lines[0]);
        assert!(lines[0].contains("name-s1"), "w={w}: {}", lines[0]);
        // The full error is shown in the status bar above the input.
        assert!(s.contains("Retrying in 4s (attempt 3)"), "{s}");
        assert!(s.contains("HTTP 429 Too Many Requests"), "{s}");
        for l in &lines {
            assert!(unicode_width::UnicodeWidthStr::width(l.as_str()) <= w as usize);
        }
    }
}

#[test]
fn header_fits_long_and_cjk_names() {
    let (mut a, _rx) = app();
    load_transcript(&mut a);
    let mut i = info("s1");
    i.name = "一个非常非常长的中文会话名称用于测试标题栏溢出的情况以及截断是否正确".into();
    i.variant = Some("high".into());
    i.status = SessionStatus::Busy;
    a.on_server(ServerMsg::SessionUpdated { info: i });
    for w in [40u16, 80, 120, 200] {
        let lines = render(&mut a, w, 24);
        assert!(unicode_width::UnicodeWidthStr::width(lines[0].as_str()) <= w as usize, "{}", lines[0]);
        assert!(lines[0].contains("working"), "w={w}: {}", lines[0]);
        assert!(lines[0].contains("一个非常"), "w={w}: {}", lines[0]);
    }
    let lines = render(&mut a, 200, 24);
    assert!(lines[0].contains("claude high"), "{}", lines[0]);
}

#[test]
fn empty_state_logo() {
    let (mut a, _rx) = app();
    a.on_server(ServerMsg::Welcome {
        instance: InstanceInfo { pid: 1, version: "0".into(), bind: "127.0.0.1".into(), port: 1, allow_hosts: vec![], idle_timeout: 0, started: 0, data_dir: String::new() },
        catalog: catalog(),
        protocol: 1,
        client_id: "c".into(),
    });
    let r = a.req(crate::app::Req::Create);
    a.on_server(ServerMsg::SessionSnapshot { req: r, info: info("s1"), messages: vec![], permissions: vec![] });
    for (w, h) in [(80u16, 24u16), (120, 40), (60, 16), (30, 10)] {
        let lines = render(&mut a, w, h);
        let s = screen(&lines);
        if std::env::var("SHOW").is_ok() {
            println!("{s}");
        }
        for l in &lines {
            assert!(unicode_width::UnicodeWidthStr::width(l.as_str()) <= w as usize);
        }
        if h >= 24 {
            assert!(s.contains("█▀▀█"), "{s}");
            assert!(s.contains("commands"), "{s}");
        }
    }
}

#[tokio::test]
async fn selection_copy() {
    let (mut a, _rx) = app();
    load_transcript(&mut a);
    let lines = render(&mut a, 100, 40);
    let (x, y) = find(&lines, "Here are the files").unwrap();
    let m = |kind, col, row| Event::Mouse(MouseEvent { kind, column: col, row, modifiers: KeyModifiers::NONE });
    a.on_event(m(MouseEventKind::Down(MouseButton::Left), x, y));
    a.on_event(m(MouseEventKind::Drag(MouseButton::Left), x + 8, y));
    a.on_event(m(MouseEventKind::Up(MouseButton::Left), x + 8, y));
    assert_eq!(a.views.get("s1").unwrap().selected_text(), "Here are");
    // No auto-copy on mouse up.
    assert_eq!(last_copy(), None);
    // Typing text in the input then ctrl+c copies instead of clearing.
    typ(&mut a, "draft");
    a.on_event(Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
    assert_eq!(a.editor.text(), "draft");
    assert!(a.toasts.items.iter().any(|t| t.text == "Copied 8 chars"));
    assert_eq!(last_copy().as_deref(), Some("Here are"));
    // Without selection ctrl+c clears input.
    a.on_event(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
    a.on_event(Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
    assert!(a.editor.is_empty());
    assert!(!a.quit);
}

#[test]
fn watch_mode_panes() {
    let (mut a, mut rx) = app();
    a.opts.watch = true;
    a.mode = Mode::Watch;
    a.on_conn(ConnEvent::Connected);
    let mut sessions = vec![];
    for k in 0..3 {
        let mut i = info(&format!("b{k}"));
        i.attached = 0;
        i.created = k;
        i.name = format!("worker-{k}");
        sessions.push(i);
    }
    while rx.try_recv().is_ok() {}
    a.on_server(ServerMsg::SessionList { req: 0, sessions: sessions.clone() });
    let mut watched = vec![];
    while let Ok(m) = rx.try_recv() {
        if let ClientMsg::Watch { session_id } = m {
            watched.push(session_id);
        }
    }
    watched.sort();
    assert_eq!(watched, vec!["b0", "b1", "b2"]);
    for s in &sessions {
        a.on_server(ServerMsg::SessionSnapshot { req: 0, info: s.clone(), messages: vec![msg(&format!("{}-m", s.id), Role::User, &format!("task for {}", s.name), 1)], permissions: vec![] });
    }
    let lines = render(&mut a, 160, 50);
    let s = screen(&lines);
    assert!(s.contains("worker-0") && s.contains("worker-1") && s.contains("worker-2"), "{s}");
    assert!(s.contains("task for worker-2"));
    assert_eq!(a.watch.panes.len(), 3);
    // Focus first, send a prompt to it.
    assert_eq!(a.watch.focus.as_deref(), Some("b0"));
    a.on_event(Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL)));
    assert_eq!(a.watch.focus.as_deref(), Some("b1"));
    typ(&mut a, "go");
    a.on_event(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
    let mut got = false;
    while let Ok(m) = rx.try_recv() {
        if let ClientMsg::Prompt { session_id, text, .. } = m {
            assert_eq!(session_id, "b1");
            assert_eq!(text, "go");
            got = true;
        }
    }
    assert!(got);
    // A session becoming foreground elsewhere disappears from the panes.
    sessions[2].attached = 1;
    a.on_server(ServerMsg::SessionList { req: 0, sessions: sessions.clone() });
    assert!(matches!(rx.try_recv(), Ok(ClientMsg::Unwatch { session_id }) if session_id == "b2"));
    let _ = render(&mut a, 160, 50);
    assert_eq!(a.watch.panes.len(), 2);
}

#[test]
fn startup_continue_attaches_latest_same_cwd() {
    let (mut a, mut rx) = app();
    a.startup_done = false;
    a.opts.cont = true;
    a.on_conn(ConnEvent::Connected);
    let req = match rx.try_recv().unwrap() {
        ClientMsg::ListSessions { req } => req,
        other => panic!("{other:?}"),
    };
    let mut s1 = info("old");
    s1.updated = 10;
    let mut s2 = info("new");
    s2.updated = 20;
    let mut s3 = info("other");
    s3.updated = 30;
    s3.cwd = "/elsewhere".into();
    a.on_server(ServerMsg::SessionList { req, sessions: vec![s1, s2, s3] });
    assert!(matches!(rx.try_recv().unwrap(), ClientMsg::Attach { session_id, .. } if session_id == "new"));
}

#[test]
fn model_flag_only_for_first_session() {
    let (mut a, mut rx) = app();
    a.startup_done = false;
    a.opts.model = Some("x/y".into());
    a.on_conn(ConnEvent::Connected);
    let _ = rx.try_recv(); // ListSessions
    assert!(matches!(rx.try_recv().unwrap(), ClientMsg::CreateSession { model: Some(m), .. } if m == "x/y"));
    a.run_command("new", "");
    let mut saw = false;
    while let Ok(m) = rx.try_recv() {
        if let ClientMsg::CreateSession { model, .. } = m {
            assert!(model.is_none());
            saw = true;
        }
    }
    assert!(saw);
}

#[test]
fn undo_restores_input() {
    let (mut a, _rx) = app();
    load_transcript(&mut a);
    a.on_server(ServerMsg::Undone { session_id: "s1".into(), text: "my old prompt".into() });
    assert_eq!(a.editor.text(), "my old prompt");
}

#[test]
fn sidebar_shows_context_and_mcp() {
    let (mut a, _rx) = app();
    load_transcript(&mut a);
    let lines = render(&mut a, 150, 40);
    let s = screen(&lines);
    assert!(s.contains("Context"), "{s}");
    assert!(s.contains("github"));
    assert!(s.contains("Connected · 12 tools"));
}

/// Rendering cost with a long transcript: the first frame builds caches, later frames (typing, spinner
/// ticks, streaming into the last message) must be cheap. Run with `--ignored --nocapture` for timings.
#[test]
fn perf_long_transcript() {
    let (mut a, _rx) = app();
    load_transcript(&mut a);
    let mut msgs = vec![];
    let md = "## Section\n\nSome **bold** text with `code` and a [link](https://example.com).\n\n- item one\n- item two\n  - nested\n\n```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n";
    for k in 0..2000 {
        let role = if k % 2 == 0 { Role::User } else { Role::Assistant };
        let mut m = msg(&format!("p{k}"), role, if k % 2 == 0 { "question about the code base" } else { md }, 1000 + k as i64);
        if role == Role::Assistant {
            m.time_end = Some(2000 + k as i64);
            m.finish = Some("stop".into());
            m.parts.push(tool(&format!("pt{k}"), "bash", "Run tests", ToolState::Done, 1000 + k as i64));
        }
        msgs.push(m);
    }
    let r = a.req(crate::app::Req::Attach);
    a.on_server(ServerMsg::SessionSnapshot { req: r, info: info("s1"), messages: msgs, permissions: vec![] });
    let mut t = Terminal::new(TestBackend::new(140, 50)).unwrap();
    let t0 = std::time::Instant::now();
    t.draw(|f| super::draw(f, &mut a)).unwrap();
    let first = t0.elapsed();
    let t1 = std::time::Instant::now();
    for _ in 0..20 {
        a.editor.insert_char('x');
        t.draw(|f| super::draw(f, &mut a)).unwrap();
    }
    let typing = t1.elapsed() / 20;
    // Streaming: a live assistant message growing to a long markdown document.
    let mut live = msg("live", Role::Assistant, "", now_ms());
    live.parts.push(Part::Text(TextPart { id: "lt".into(), text: String::new(), time: now_ms(), meta: None }));
    a.on_server(ServerMsg::MessageUpsert { session_id: "s1".into(), message: live });
    let t2 = std::time::Instant::now();
    let mut n = 0;
    for _ in 0..40 {
        a.on_server(ServerMsg::PartDelta { session_id: "s1".into(), message_id: "live".into(), part_id: "lt".into(), delta: md.repeat(3) });
        t.draw(|f| super::draw(f, &mut a)).unwrap();
        n += 1;
    }
    let streaming = t2.elapsed() / n;
    let total = a.views.get("s1").unwrap().total;
    eprintln!("lines={total} first={first:?} typing/frame={typing:?} streaming/frame={streaming:?}");
    // Generous bounds (debug build, shared CI machines).
    assert!(typing < std::time::Duration::from_millis(40), "typing frame {typing:?}");
    assert!(streaming < std::time::Duration::from_millis(150), "streaming frame {streaming:?}");
}

/// Every rendered line of a rich transcript fits the terminal width at common sizes.
#[test]
fn no_overflow_at_common_sizes() {
    let (mut a, _rx) = app();
    load_transcript(&mut a);
    let mut c = msg("mc", Role::User, "summary of everything so far", now_ms());
    c.origin = MessageOrigin::Compaction;
    a.on_server(ServerMsg::MessageUpsert { session_id: "s1".into(), message: c });
    let mut p = msg("mp", Role::User, "hello from a peer", now_ms() + 1);
    p.origin = MessageOrigin::Peer { from_session: "x".into(), from_name: "工作者-1".into() };
    a.on_server(ServerMsg::MessageUpsert { session_id: "s1".into(), message: p });
    let mut d = msg("md", Role::Assistant, "", now_ms() + 2);
    let mut tp = tool("td", "bash", "Run the full test-suite with coverage enabled and upload results", ToolState::Delayed, now_ms());
    if let Part::Tool(t) = &mut tp {
        t.run_at = Some(now_ms() + 3000);
    }
    d.parts.push(tp);
    a.on_server(ServerMsg::MessageUpsert { session_id: "s1".into(), message: d });
    for (w, h) in [(80u16, 20u16), (80, 40), (120, 30), (200, 60), (60, 24), (140, 40)] {
        let lines = render(&mut a, w, h);
        for l in &lines {
            assert!(unicode_width::UnicodeWidthStr::width(l.as_str()) <= w as usize, "w={w}: {l}");
        }
        let s = screen(&lines);
        if h >= 40 {
            assert!(s.contains("context compacted"), "{s}");
            assert!(s.contains("runs in 3s"), "w={w}\n{s}");
            // The compaction line keeps its copy button fully visible.
            let cl = lines.iter().find(|l| l.contains("context compacted")).unwrap();
            assert!(cl.contains("⧉ copy"), "{cl}");
        }
    }
}

#[test]
fn scrolling_follow_and_new_messages_indicator() {
    let (mut a, _rx) = app();
    load_transcript(&mut a);
    for k in 0..30 {
        a.on_server(ServerMsg::MessageUpsert { session_id: "s1".into(), message: msg(&format!("f{k}"), Role::User, &format!("filler {k}"), now_ms() + k) });
    }
    let lines = render(&mut a, 100, 30);
    assert!(screen(&lines).contains("filler 29"));
    let wheel = |a: &mut App, kind| a.on_event(Event::Mouse(MouseEvent { kind, column: 10, row: 10, modifiers: KeyModifiers::NONE }));
    wheel(&mut a, MouseEventKind::ScrollUp);
    wheel(&mut a, MouseEventKind::ScrollUp);
    let v = a.views.get("s1").unwrap();
    assert!(!v.follow);
    let top = v.scroll_top;
    // New content while scrolled up: position kept, indicator shown.
    a.on_server(ServerMsg::MessageUpsert { session_id: "s1".into(), message: msg("new", Role::User, "brand new", now_ms() + 100) });
    let lines = render(&mut a, 100, 30);
    let s = screen(&lines);
    assert_eq!(a.views.get("s1").unwrap().scroll_top, top);
    assert!(s.contains("↓ new messages"), "{s}");
    assert!(!s.contains("brand new"));
    // PageUp / PageDown.
    a.on_event(Event::Key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE)));
    assert!(a.views.get("s1").unwrap().scroll_top < top);
    // Clicking the indicator jumps to the bottom and follows again.
    let (x, y) = find(&lines, "↓ new messages").unwrap();
    let _ = render(&mut a, 100, 30);
    click(&mut a, x + 3, y);
    let lines = render(&mut a, 100, 30);
    assert!(a.views.get("s1").unwrap().follow);
    assert!(screen(&lines).contains("brand new"));
    // Resize keeps follow at the bottom.
    let lines = render(&mut a, 70, 22);
    assert!(screen(&lines).contains("brand new"));
}

/// No panics / overflow at any size with every overlay active.
#[test]
fn size_sweep_no_panic() {
    let (mut a, _rx) = app();
    load_transcript(&mut a);
    let mut i = info("s1");
    i.status = SessionStatus::Retrying { attempt: 2, at: now_ms() + 3000, error: "overloaded ".repeat(20) };
    a.on_server(ServerMsg::SessionUpdated { info: i });
    a.on_server(ServerMsg::PermissionAsk {
        request: PermissionRequest {
            session_id: "s1".into(),
            message_id: "m2".into(),
            part_id: "t9".into(),
            call_id: "c9".into(),
            tool: "bash".into(),
            purpose: "Do a thing".into(),
            input: json!({"command": "echo 1\n".repeat(40)}),
            time: now_ms(),
        },
    });
    a.toast(NoticeLevel::Error, "something failed ".repeat(10));
    for pass in 0..3 {
        match pass {
            1 => a.open_sessions(false),
            2 => {
                a.dialog = None;
                a.editor.set_text("/");
                a.update_popup();
            }
            _ => {}
        }
        for w in (1u16..=200).step_by(7) {
            for h in (1u16..=60).step_by(4) {
                let lines = render(&mut a, w, h);
                for l in &lines {
                    assert!(unicode_width::UnicodeWidthStr::width(l.as_str()) <= w as usize, "w={w} h={h}: {l}");
                }
            }
        }
    }
    // Watch mode too.
    a.dialog = None;
    a.popup = None;
    a.mode = Mode::Watch;
    a.watched.insert("s1".into());
    for w in (1u16..=200).step_by(9) {
        for h in (1u16..=60).step_by(5) {
            let _ = render(&mut a, w, h);
        }
    }
}

#[test]
fn watch_pane_esc_esc_aborts_focused() {
    let (mut a, mut rx) = app();
    a.opts.watch = true;
    a.mode = Mode::Watch;
    a.on_conn(ConnEvent::Connected);
    let mut sessions = vec![];
    for k in 0..2 {
        let mut i = info(&format!("b{k}"));
        i.attached = 0;
        i.created = k;
        i.status = SessionStatus::Busy;
        sessions.push(i);
    }
    a.on_server(ServerMsg::SessionList { req: 0, sessions: sessions.clone() });
    for s in &sessions {
        a.on_server(ServerMsg::SessionSnapshot { req: 0, info: s.clone(), messages: vec![], permissions: vec![] });
    }
    let _ = render(&mut a, 160, 40);
    a.on_event(Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL)));
    while rx.try_recv().is_ok() {}
    a.on_event(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
    a.on_event(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
    assert!(matches!(rx.try_recv(), Ok(ClientMsg::Abort { session_id }) if session_id == "b1"));
    // 0 background sessions: friendly empty state.
    a.on_server(ServerMsg::SessionList { req: 0, sessions: vec![] });
    let s = screen(&render(&mut a, 100, 30));
    assert!(s.contains("No background sessions"), "{s}");
}
