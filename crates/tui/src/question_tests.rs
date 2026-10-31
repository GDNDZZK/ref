//! The question dialog: keys, rendering and the replies it sends.

use crossterm::event::{Event, KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use ref_proto::*;
use serde_json::Value;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::app::App;
use crate::net::NetEvent;
use crate::tests::{key, mouse, render, test_app_with_requests};

const SESSION: &str = "ses_test";
const PAST_GRACE: std::time::Duration = std::time::Duration::from_millis(500);

fn option(label: &str, description: &str, default: bool) -> QuestionOption {
    QuestionOption {
        label: label.into(),
        description: description.into(),
        default,
    }
}

/// The first question of the `question` mock script: three options, the first is the default.
fn loop_bound() -> Question {
    Question {
        question: "How should the loop bound in `parse_items` be fixed?".into(),
        header: "Loop bound".into(),
        options: vec![
            option("Exclusive range", "Use `0..items.len()`", true),
            option("Iterator", "Use `items.iter()` instead of indexing", false),
            option(
                "Guard the index",
                "Keep the loop and check the index",
                false,
            ),
        ],
        multiple: false,
        custom: true,
    }
}

/// The second question of that script: a multiple choice with two defaults.
fn checks() -> Question {
    Question {
        question: "Which checks should run afterwards?".into(),
        header: "Checks".into(),
        options: vec![
            option("Unit tests", "cargo test", true),
            option("Clippy", "cargo clippy", true),
            option("Formatting", "cargo fmt --check", false),
        ],
        multiple: true,
        custom: true,
    }
}

/// Like the `question-one` script, but with the default in the middle.
fn first_file() -> Question {
    Question {
        question: "Which file should I change first?".into(),
        header: "First file".into(),
        options: vec![
            option("src/lib.rs", "The public API", false),
            option("src/parser.rs", "Where the bug is", true),
            option("README.md", "The documentation", false),
        ],
        multiple: false,
        custom: true,
    }
}

fn request(questions: Vec<Question>) -> QuestionRequest {
    QuestionRequest {
        session_id: SESSION.into(),
        message_id: "m2".into(),
        call_id: "q1".into(),
        questions,
        time: 0,
    }
}

/// An app with a rendered question dialog; the grace period has passed.
fn open(questions: Vec<Question>) -> (App, UnboundedReceiver<ClientFrame>) {
    let (mut app, rx) = test_app_with_requests(false);
    app.views
        .get_mut(SESSION)
        .unwrap()
        .apply(SessionEvent::Question {
            request: request(questions),
        });
    render(&mut app, 100, 30);
    app.question.as_mut().unwrap().opened -= PAST_GRACE;
    (app, rx)
}

fn press(app: &mut App, code: KeyCode) {
    crate::input::handle(app, key(code, KeyModifiers::NONE));
}

fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        press(app, KeyCode::Char(c));
    }
}

/// Requests sent since the last call.
fn sent(rx: &mut UnboundedReceiver<ClientFrame>) -> Vec<ClientFrame> {
    std::iter::from_fn(|| rx.try_recv().ok()).collect()
}

/// The one `QuestionReply` that was sent: its request id and answers.
fn reply(rx: &mut UnboundedReceiver<ClientFrame>) -> (Option<u64>, Option<Vec<Vec<String>>>) {
    let mut frames = sent(rx);
    assert_eq!(
        frames.len(),
        1,
        "{:?}",
        frames.iter().map(|f| &f.req).collect::<Vec<_>>()
    );
    let frame = frames.remove(0);
    match frame.req {
        Request::QuestionReply {
            session_id,
            call_id,
            answers,
        } => {
            assert_eq!((session_id.as_str(), call_id.as_str()), (SESSION, "q1"));
            (frame.id, answers)
        }
        other => panic!("expected a question reply, got {other:?}"),
    }
}

fn answers(lists: &[&[&str]]) -> Option<Vec<Vec<String>>> {
    Some(
        lists
            .iter()
            .map(|l| l.iter().map(|s| s.to_string()).collect())
            .collect(),
    )
}

fn screen(app: &mut App) -> String {
    let (_, s) = render(app, 100, 30);
    println!("{s}");
    s
}

/// The backend's answer to request `id`.
fn respond(app: &mut App, id: Option<u64>, error: Option<&str>) {
    app.on_net(NetEvent::Frame(ServerFrame::Response {
        id: id.unwrap(),
        ok: error.is_none(),
        result: Value::Null,
        error: error.map(String::from),
    }));
}

#[test]
fn keys_right_after_the_dialog_opens_are_typed_not_taken() {
    let (mut app, mut rx) = open(vec![first_file()]);
    app.question.as_mut().unwrap().opened = std::time::Instant::now();
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Char('2'));
    press(&mut app, KeyCode::Esc);
    assert!(sent(&mut rx).is_empty(), "nothing answered");
    assert!(app.active_question().is_some());
    // Printable keys went to the prompt, which is not lost.
    assert_eq!(app.editor.text, "2");
    app.question.as_mut().unwrap().opened -= PAST_GRACE;
    press(&mut app, KeyCode::Char('2'));
    assert_eq!(reply(&mut rx).1, answers(&[&["src/parser.rs"]]));
    assert_eq!(app.editor.text, "2", "answering does not type");
}

#[test]
fn a_single_question_starts_on_its_default_and_enter_takes_it() {
    let (mut app, mut rx) = open(vec![first_file()]);
    let s = screen(&mut app);
    assert!(s.contains("? First file"), "{s}");
    assert!(s.contains("Which file should I change first?"), "{s}");
    assert!(s.contains("1. src/lib.rs"), "{s}");
    assert!(
        s.contains("› 2. src/parser.rs (default)"),
        "cursor on the default: {s}"
    );
    assert!(s.contains("Where the bug is"), "description: {s}");
    assert!(s.contains("4. Type your own answer"), "{s}");
    assert!(s.contains("↑↓ select  enter submit  esc dismiss"), "{s}");
    assert!(!s.contains("tab"), "no tabs for one question: {s}");
    press(&mut app, KeyCode::Enter);
    assert_eq!(reply(&mut rx).1, answers(&[&["src/parser.rs"]]));
}

#[test]
fn a_question_without_a_default_starts_on_the_first_option() {
    let mut q = first_file();
    q.options[1].default = false;
    let (mut app, mut rx) = open(vec![q]);
    assert!(screen(&mut app).contains("› 1. src/lib.rs"));
    press(&mut app, KeyCode::Enter);
    assert_eq!(reply(&mut rx).1, answers(&[&["src/lib.rs"]]));
}

#[test]
fn arrows_and_jk_move_the_cursor() {
    let (mut app, mut rx) = open(vec![first_file()]);
    press(&mut app, KeyCode::Down);
    assert!(screen(&mut app).contains("› 3. README.md"));
    press(&mut app, KeyCode::Char('j'));
    assert!(screen(&mut app).contains("› 4. Type your own answer"));
    press(&mut app, KeyCode::Down);
    assert!(
        screen(&mut app).contains("› 4. Type your own answer"),
        "stops at the end"
    );
    press(&mut app, KeyCode::Up);
    press(&mut app, KeyCode::Char('k'));
    assert!(screen(&mut app).contains("› 2. src/parser.rs"));
    press(&mut app, KeyCode::Up);
    press(&mut app, KeyCode::Up);
    assert!(
        screen(&mut app).contains("› 1. src/lib.rs"),
        "stops at the start"
    );
    assert!(sent(&mut rx).is_empty());
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    assert_eq!(reply(&mut rx).1, answers(&[&["README.md"]]));
}

#[test]
fn number_keys_pick_and_submit() {
    let (mut app, mut rx) = open(vec![first_file()]);
    press(&mut app, KeyCode::Char('1'));
    assert_eq!(reply(&mut rx).1, answers(&[&["src/lib.rs"]]));
    let (mut app, mut rx) = open(vec![first_file()]);
    press(&mut app, KeyCode::Char('9'));
    assert!(sent(&mut rx).is_empty(), "no such row");
    press(&mut app, KeyCode::Char('3'));
    assert_eq!(reply(&mut rx).1, answers(&[&["README.md"]]));
}

#[test]
fn esc_dismisses() {
    let (mut app, mut rx) = open(vec![first_file()]);
    press(&mut app, KeyCode::Esc);
    assert_eq!(reply(&mut rx).1, None);
}

#[test]
fn several_questions_get_tabs_and_a_confirm_page() {
    let (mut app, mut rx) = open(vec![loop_bound(), checks()]);
    let s = screen(&mut app);
    assert!(s.contains("? Questions"), "{s}");
    assert!(
        s.contains("Loop bound") && s.contains("Checks") && s.contains("Confirm"),
        "{s}"
    );
    assert!(s.contains("› 1. Exclusive range (default)"), "{s}");
    assert!(
        s.contains("⇆ tab  ↑↓ select  enter select  esc dismiss"),
        "{s}"
    );
    // A single choice moves on to the next tab.
    press(&mut app, KeyCode::Enter);
    let s = screen(&mut app);
    assert!(s.contains("Which checks should run afterwards?"), "{s}");
    assert!(
        s.contains("› [✓] Unit tests (default)"),
        "defaults are ticked: {s}"
    );
    assert!(s.contains("[✓] Clippy (default)"), "{s}");
    assert!(s.contains("[ ] Formatting"), "{s}");
    assert!(s.contains("enter toggle"), "{s}");
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    assert!(screen(&mut app).contains("› [✓] Formatting"));
    press(&mut app, KeyCode::Char(' '));
    assert!(
        screen(&mut app).contains("› [ ] Formatting"),
        "space toggles back"
    );
    press(&mut app, KeyCode::Char('2'));
    assert!(
        screen(&mut app).contains("› [ ] Clippy"),
        "a number toggles, never submits"
    );
    press(&mut app, KeyCode::Char('2'));
    assert!(sent(&mut rx).is_empty());
    press(&mut app, KeyCode::Right);
    let s = screen(&mut app);
    assert!(s.contains("Review your answers"), "{s}");
    assert!(s.contains("✓ Exclusive range"), "{s}");
    assert!(s.contains("✓ Unit tests") && s.contains("✓ Clippy"), "{s}");
    assert!(!s.contains("not answered"), "{s}");
    assert!(s.contains("⇆ tab  enter submit  esc dismiss"), "{s}");
    press(&mut app, KeyCode::Enter);
    assert_eq!(
        reply(&mut rx).1,
        answers(&[&["Exclusive range"], &["Unit tests", "Clippy"]])
    );
}

#[test]
fn the_confirm_page_flags_unanswered_questions() {
    let (mut app, mut rx) = open(vec![loop_bound(), checks()]);
    press(&mut app, KeyCode::BackTab);
    let s = screen(&mut app);
    assert!(
        s.contains("Review your answers"),
        "BackTab wraps to Confirm: {s}"
    );
    assert!(s.contains("(not answered)"), "{s}");
    assert!(s.contains("✓ Unit tests"), "ticked defaults count: {s}");
    // The red marker is the error colour.
    let (term, _) = render(&mut app, 100, 30);
    let red = term
        .backend()
        .buffer()
        .content
        .iter()
        .any(|c| c.fg == crate::ui::theme::ERROR && c.symbol() == "(");
    assert!(red, "unanswered is red");
    // Submitting with a gap is allowed: the server accepts an empty list.
    press(&mut app, KeyCode::Enter);
    assert_eq!(reply(&mut rx).1, answers(&[&[], &["Unit tests", "Clippy"]]));
}

#[test]
fn tabs_cycle_and_keep_their_choices() {
    let (mut app, _rx) = open(vec![loop_bound(), checks()]);
    press(&mut app, KeyCode::Char('2'));
    assert!(screen(&mut app).contains("Which checks"), "moved on");
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Tab);
    let s = screen(&mut app);
    assert!(s.contains("› 2. Iterator ✓"), "the choice is marked: {s}");
    assert!(s.contains("Iterator"), "{s}");
    press(&mut app, KeyCode::Left);
    assert!(screen(&mut app).contains("Review your answers"));
    press(&mut app, KeyCode::Right);
    assert!(screen(&mut app).contains("How should the loop bound"));
}

#[test]
fn a_custom_answer_is_typed_in_the_last_row_and_submits_a_lone_question() {
    let (mut app, mut rx) = open(vec![first_file()]);
    press(&mut app, KeyCode::Char('4'));
    let s = screen(&mut app);
    assert!(s.contains("› 4. your answer"), "empty field: {s}");
    assert!(s.contains("enter submit  esc back"), "{s}");
    assert!(
        app.question.as_ref().unwrap().cursor_at.is_some(),
        "terminal cursor in the field"
    );
    // Digits and letters are text now, not shortcuts.
    type_text(&mut app, "42 j");
    let s = screen(&mut app);
    assert!(s.contains("› 4. 42 j"), "{s}");
    assert!(sent(&mut rx).is_empty());
    press(&mut app, KeyCode::Enter);
    assert_eq!(reply(&mut rx).1, answers(&[&["42 j"]]));
}

#[test]
fn an_empty_custom_answer_is_cancelled_and_esc_only_leaves_the_field() {
    let (mut app, mut rx) = open(vec![first_file()]);
    press(&mut app, KeyCode::Char('4'));
    press(&mut app, KeyCode::Enter);
    assert!(sent(&mut rx).is_empty(), "nothing to send");
    assert!(app.question.as_ref().unwrap().edit.is_none());
    assert!(screen(&mut app).contains("› 4. Type your own answer"));
    press(&mut app, KeyCode::Enter);
    type_text(&mut app, "   ");
    press(&mut app, KeyCode::Enter);
    assert!(sent(&mut rx).is_empty(), "blank counts as empty");
    press(&mut app, KeyCode::Enter);
    type_text(&mut app, "later");
    press(&mut app, KeyCode::Esc);
    assert!(
        sent(&mut rx).is_empty(),
        "esc does not dismiss while typing"
    );
    assert!(app.active_question().is_some());
    assert!(screen(&mut app).contains("↑↓ select  enter submit  esc dismiss"));
    press(&mut app, KeyCode::Esc);
    assert_eq!(reply(&mut rx).1, None);
}

#[test]
fn a_custom_answer_to_one_of_several_questions_moves_on() {
    let (mut app, mut rx) = open(vec![loop_bound(), checks()]);
    press(&mut app, KeyCode::Char('4'));
    type_text(&mut app, "Use a while loop");
    press(&mut app, KeyCode::Enter);
    let s = screen(&mut app);
    assert!(
        s.contains("Which checks should run afterwards?"),
        "next tab: {s}"
    );
    assert!(sent(&mut rx).is_empty());
    // On a multiple choice the typed text is added to the ticked options.
    press(&mut app, KeyCode::Char('4'));
    type_text(&mut app, "Docs build");
    press(&mut app, KeyCode::Enter);
    let s = screen(&mut app);
    assert!(s.contains("[✓] Type your own answer: Docs build"), "{s}");
    assert!(s.contains("Which checks"), "stays on the tab: {s}");
    press(&mut app, KeyCode::Tab);
    let s = screen(&mut app);
    assert!(s.contains("✓ Use a while loop"), "{s}");
    assert!(s.contains("✓ Docs build"), "{s}");
    press(&mut app, KeyCode::Enter);
    assert_eq!(
        reply(&mut rx).1,
        answers(&[
            &["Use a while loop"],
            &["Unit tests", "Clippy", "Docs build"]
        ])
    );
}

#[test]
fn choosing_the_custom_row_of_a_multiple_choice_again_takes_the_text_back() {
    let (mut app, mut rx) = open(vec![checks()]);
    assert!(screen(&mut app).contains("⇆ tab  ↑↓ select  enter toggle"));
    press(&mut app, KeyCode::Char('4'));
    type_text(&mut app, "x");
    press(&mut app, KeyCode::Enter);
    assert!(screen(&mut app).contains("[✓] Type your own answer: x"));
    press(&mut app, KeyCode::Enter);
    let s = screen(&mut app);
    assert!(s.contains("› [ ] Type your own answer"), "{s}");
    assert!(!s.contains(": x"), "{s}");
    // Typing it again starts from an empty field.
    press(&mut app, KeyCode::Enter);
    type_text(&mut app, "y");
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Right);
    press(&mut app, KeyCode::Enter);
    assert_eq!(reply(&mut rx).1, answers(&[&["Unit tests", "Clippy", "y"]]));
}

#[test]
fn a_single_choice_replaces_a_typed_answer_and_the_other_way_round() {
    let (mut app, _rx) = open(vec![loop_bound(), checks()]);
    press(&mut app, KeyCode::Char('4'));
    type_text(&mut app, "mine");
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::BackTab);
    let s = screen(&mut app);
    assert!(s.contains("4. Type your own answer: mine ✓"), "{s}");
    press(&mut app, KeyCode::Char('1'));
    press(&mut app, KeyCode::BackTab);
    let s = screen(&mut app);
    assert!(s.contains("1. Exclusive range (default) ✓"), "{s}");
    assert!(s.contains("4. Type your own answer"), "{s}");
    assert!(!s.contains("mine"), "{s}");
}

#[test]
fn the_dialog_waits_for_the_backend_before_closing() {
    let (mut app, mut rx) = open(vec![first_file()]);
    press(&mut app, KeyCode::Enter);
    let (id, _) = reply(&mut rx);
    let s = screen(&mut app);
    assert!(s.contains("sending…"), "{s}");
    press(&mut app, KeyCode::Char('1'));
    press(&mut app, KeyCode::Esc);
    assert!(sent(&mut rx).is_empty(), "keys are ignored while sending");
    respond(&mut app, id, None);
    assert!(app.active_question().is_none());
    assert!(!screen(&mut app).contains("First file"));
}

#[test]
fn an_answer_given_elsewhere_closes_the_dialog_quietly() {
    let (mut app, mut rx) = open(vec![first_file()]);
    press(&mut app, KeyCode::Enter);
    let (id, _) = reply(&mut rx);
    respond(&mut app, id, Some("No pending question with that id."));
    assert!(app.active_question().is_none());
    assert!(app.question.is_none());
    let s = screen(&mut app);
    assert!(!s.contains("No pending question"), "no error: {s}");
    assert!(app.toasts.items.is_empty(), "no toast either");
}

#[test]
fn a_refused_answer_keeps_the_dialog_with_the_reason() {
    let (mut app, mut rx) = open(vec![loop_bound(), checks()]);
    press(&mut app, KeyCode::BackTab);
    press(&mut app, KeyCode::Enter);
    let (id, _) = reply(&mut rx);
    respond(&mut app, id, Some("Expected 2 answers, got 3."));
    let s = screen(&mut app);
    assert!(s.contains("Expected 2 answers, got 3."), "{s}");
    assert!(s.contains("Review your answers"), "dialog stays: {s}");
    assert!(app.toasts.items.is_empty());
    // The next key clears the message, and the answer can be sent again.
    press(&mut app, KeyCode::Enter);
    let (id, answers) = reply(&mut rx);
    assert_eq!(answers, self::answers(&[&[], &["Unit tests", "Clippy"]]));
    respond(&mut app, id, None);
    assert!(app.active_question().is_none());
}

#[test]
fn the_server_resolving_the_question_closes_the_dialog() {
    let (mut app, mut rx) = open(vec![first_file()]);
    app.views
        .get_mut(SESSION)
        .unwrap()
        .apply(SessionEvent::QuestionResolved {
            call_id: "q1".into(),
        });
    screen(&mut app);
    assert!(app.question.is_none());
    assert!(app.active_question().is_none());
    press(&mut app, KeyCode::Enter);
    assert!(sent(&mut rx).is_empty(), "the key is no longer a reply");
}

#[test]
fn a_question_is_shown_once_and_restored_from_a_snapshot() {
    let (mut app, _rx) = test_app_with_requests(false);
    let v = app.views.get_mut(SESSION).unwrap();
    v.apply(SessionEvent::Question {
        request: request(vec![first_file()]),
    });
    v.apply(SessionEvent::Question {
        request: request(vec![first_file()]),
    });
    assert_eq!(v.questions.len(), 1, "the same call id is not queued twice");
    let mut snap = SessionSnapshot {
        info: v.info.clone(),
        messages: vec![],
        more_before: false,
        pending_permissions: vec![],
        pending_questions: vec![request(vec![loop_bound(), checks()])],
        queued: vec![],
    };
    v.load_snapshot(snap.clone());
    assert_eq!(v.questions.len(), 1);
    render(&mut app, 100, 30);
    assert!(screen(&mut app).contains("? Questions"));
    snap.pending_questions.clear();
    app.views.get_mut(SESSION).unwrap().load_snapshot(snap);
    screen(&mut app);
    assert!(app.question.is_none(), "answered while we were away");
}

#[test]
fn a_permission_request_goes_first() {
    let (mut app, mut rx) = open(vec![first_file()]);
    app.views
        .get_mut(SESSION)
        .unwrap()
        .apply(SessionEvent::Permission {
            request: PermissionRequest {
                session_id: SESSION.into(),
                message_id: "m2".into(),
                call_id: "c9".into(),
                tool: "bash".into(),
                purpose: "List files".into(),
                input: serde_json::json!({"command": "ls -la"}),
                time: 0,
            },
        });
    let s = screen(&mut app);
    assert!(s.contains("Permission required"), "{s}");
    assert!(!s.contains("First file"), "{s}");
    app.perm.as_mut().unwrap().opened -= PAST_GRACE;
    press(&mut app, KeyCode::Char('d'));
    assert!(app.active_permission().is_none());
    let _ = sent(&mut rx);
    let s = screen(&mut app);
    assert!(s.contains("? First file"), "the question follows: {s}");
}

#[test]
fn the_title_names_the_session_when_several_are_shown() {
    let (mut app, _rx) = open(vec![first_file()]);
    let mut view = crate::ui::question::QuestionView::new(&request(vec![first_file()]));
    let area = ratatui::layout::Rect::new(0, 0, 100, 30);
    let mut buf = ratatui::buffer::Buffer::empty(area);
    view.draw(&mut buf, area, Some("docs-writer"));
    let top = (0..30)
        .map(|y| {
            (0..100)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .find(|l| l.contains('?'))
        .unwrap();
    println!("{top}");
    assert!(top.contains("? First file · docs-writer"), "{top}");
    assert!(
        !screen(&mut app).contains("First file ·"),
        "no suffix for the session on screen"
    );
}

fn click(app: &mut App, x: u16, y: u16) {
    crate::input::handle(app, mouse(MouseEventKind::Down(MouseButton::Left), x, y));
    crate::input::handle(app, mouse(MouseEventKind::Up(MouseButton::Left), x, y));
}

/// Column and row of `needle` on the screen dump (the dialog area holds only narrow characters).
fn find(s: &str, needle: &str) -> (u16, u16) {
    s.lines()
        .enumerate()
        .find_map(|(y, l)| {
            let byte = l.find(needle)?;
            Some((l[..byte].chars().count() as u16, y as u16))
        })
        .unwrap_or_else(|| panic!("{needle:?} not on screen:\n{s}"))
}

#[test]
fn clicking_a_row_chooses_it_and_a_tab_switches_page() {
    let (mut app, mut rx) = open(vec![first_file()]);
    let (x, y) = find(&screen(&mut app), "3. README.md");
    click(&mut app, x, y);
    assert_eq!(reply(&mut rx).1, answers(&[&["README.md"]]));

    let (mut app, mut rx) = open(vec![loop_bound(), checks()]);
    let (x, y) = find(&screen(&mut app), "Checks");
    click(&mut app, x, y);
    let s = screen(&mut app);
    assert!(s.contains("Which checks"), "tab clicked: {s}");
    let (x, y) = find(&s, "Formatting");
    click(&mut app, x, y);
    assert!(screen(&mut app).contains("[✓] Formatting"), "row clicked");
    // The description line belongs to its row.
    let (x, y) = find(&screen(&mut app), "cargo fmt --check");
    click(&mut app, x, y);
    assert!(screen(&mut app).contains("[ ] Formatting"), "toggled back");
    assert!(sent(&mut rx).is_empty());
    // A click outside any row does nothing.
    click(&mut app, 0, 0);
    assert!(sent(&mut rx).is_empty());
    assert!(app.active_question().is_some());
}

#[test]
fn pasting_fills_the_custom_field_and_is_ignored_otherwise() {
    let (mut app, mut rx) = open(vec![first_file()]);
    crate::input::handle(&mut app, Event::Paste("ignored".into()));
    assert_eq!(app.editor.text, "", "the prompt does not get it");
    press(&mut app, KeyCode::Char('4'));
    crate::input::handle(&mut app, Event::Paste("two\nlines".into()));
    press(&mut app, KeyCode::Enter);
    assert_eq!(reply(&mut rx).1, answers(&[&["two lines"]]));
}

#[test]
fn small_terminals_and_long_texts_do_not_break_the_layout() {
    let long = "word ".repeat(80);
    let mut q = loop_bound();
    q.question = long.clone();
    q.options[0].description = long.clone();
    q.options[1].label = "L".repeat(120);
    let (mut app, _rx) = open(vec![q, checks()]);
    for (w, h) in [(100, 30), (60, 14), (40, 12), (24, 8), (10, 4), (1, 1)] {
        let (_, s) = render(&mut app, w, h);
        if w == 40 {
            println!("{s}");
        }
    }
    // The cursor row stays on screen when the page is taller than the dialog.
    for _ in 0..4 {
        press(&mut app, KeyCode::Down);
    }
    let (_, s) = render(&mut app, 60, 14);
    println!("{s}");
    assert!(s.contains("› 4. Type your own"), "cursor in view: {s}");
    press(&mut app, KeyCode::Char('4'));
    type_text(&mut app, &"x".repeat(200));
    for (w, h) in [(60, 14), (24, 8), (10, 4)] {
        render(&mut app, w, h);
    }
}
