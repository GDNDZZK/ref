//! Terminal setup/teardown (alternate screen, raw mode, paste, mouse, keyboard enhancement).

use std::io::{self, Stdout, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use crossterm::event::{
    DisableBracketedPaste, DisableFocusChange, EnableBracketedPaste, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

pub type Term = Terminal<CrosstermBackend<Stdout>>;

/// Mouse reporting: clicks (1000) + drag motion (1002) + SGR coordinates (1006). Unlike crossterm's
/// EnableMouseCapture this does not enable any-motion tracking (1003), so plain mouse moves cost nothing.
pub struct EnableMouseCapture;
pub struct DisableMouseCapture;

impl crossterm::Command for EnableMouseCapture {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str("\x1b[?1000h\x1b[?1002h\x1b[?1006h")
    }
}

impl crossterm::Command for DisableMouseCapture {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str("\x1b[?1006l\x1b[?1003l\x1b[?1002l\x1b[?1000l")
    }
}

static KB_ENHANCED: AtomicBool = AtomicBool::new(false);
static ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn setup() -> anyhow::Result<Term> {
    terminal::enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(out, EnterAlternateScreen, EnableBracketedPaste, EnableMouseCapture)?;
    if matches!(terminal::supports_keyboard_enhancement(), Ok(true)) {
        let flags = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS;
        if execute!(out, PushKeyboardEnhancementFlags(flags)).is_ok() {
            KB_ENHANCED.store(true, Ordering::SeqCst);
        }
    }
    ACTIVE.store(true, Ordering::SeqCst);
    let mut t = Terminal::new(CrosstermBackend::new(out))?;
    t.clear()?;
    Ok(t)
}

pub fn restore() {
    if !ACTIVE.swap(false, Ordering::SeqCst) {
        return;
    }
    let mut out = io::stdout();
    if KB_ENHANCED.load(Ordering::SeqCst) {
        let _ = execute!(out, PopKeyboardEnhancementFlags);
    }
    let _ = execute!(out, DisableMouseCapture, DisableBracketedPaste, DisableFocusChange, LeaveAlternateScreen, crossterm::cursor::Show);
    let _ = terminal::disable_raw_mode();
    let _ = out.flush();
}

pub fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        prev(info);
    }));
}

pub fn set_mouse_capture(on: bool) {
    let mut out = io::stdout();
    let _ = if on { execute!(out, EnableMouseCapture) } else { execute!(out, DisableMouseCapture) };
}

pub fn draw(t: &mut Term, app: &mut crate::app::App) -> io::Result<()> {
    t.draw(|f| crate::ui::draw(f, app))?;
    Ok(())
}

/// Temporarily leave the TUI (for $EDITOR), then restore.
pub fn with_suspended<T>(t: &mut Term, f: impl FnOnce() -> T) -> anyhow::Result<T> {
    restore();
    let r = f();
    let mut out = io::stdout();
    terminal::enable_raw_mode()?;
    execute!(out, EnterAlternateScreen, EnableBracketedPaste, EnableMouseCapture)?;
    if KB_ENHANCED.load(Ordering::SeqCst) {
        let flags = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS;
        let _ = execute!(out, PushKeyboardEnhancementFlags(flags));
    }
    ACTIVE.store(true, Ordering::SeqCst);
    t.clear()?;
    Ok(r)
}

/// Open `$VISUAL`/`$EDITOR` on a temp file with `text`; returns the edited text.
pub fn edit_in_editor(text: &str) -> Option<String> {
    let editor = std::env::var("VISUAL").ok().filter(|s| !s.is_empty()).or_else(|| std::env::var("EDITOR").ok().filter(|s| !s.is_empty())).unwrap_or_else(|| "vi".into());
    let path = std::env::temp_dir().join(format!("ref-prompt-{}.md", std::process::id()));
    std::fs::write(&path, text).ok()?;
    let mut parts = editor.split_whitespace();
    let prog = parts.next()?;
    let status = std::process::Command::new(prog).args(parts).arg(&path).status().ok()?;
    let out = if status.success() { std::fs::read_to_string(&path).ok() } else { None };
    let _ = std::fs::remove_file(&path);
    out
}
