//! Keyboard / mouse / paste routing.

use std::time::{Duration, Instant};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Position, Rect};
use ref_proto::ImageData;

use crate::app::{ARMED_KEY_TIMEOUT, App, COMMANDS, Mode};
use crate::render;
use crate::ui::dialog::{Dialog, Kind, PAGE_ROWS};
use crate::ui::editor::{Editor, MIN_EDITOR_COLS};
use crate::ui::permission::PermissionAction;
use crate::ui::question::Outcome;
use crate::ui::{Hit, chat, split, theme};
use crate::util::cmd;

const CTRL: KeyModifiers = KeyModifiers::CONTROL;
const ALT: KeyModifiers = KeyModifiers::ALT;
const SHIFT: KeyModifiers = KeyModifiers::SHIFT;
/// Rows of one mouse wheel notch.
const WHEEL_SCROLL_ROWS: i32 = 3;
/// A cleared draft goes to the prompt history only from this many characters.
const MIN_DRAFT_CHARS_FOR_HISTORY: usize = 20;
/// Shortest text that can be an image path (`a.png`).
const MIN_IMAGE_PATH_LEN: usize = 5;
const MAX_PASTED_PATHS: usize = 32;
/// Clicks closer together than this count as a double / triple click.
const MULTI_CLICK_WINDOW: Duration = Duration::from_millis(450);
/// Click counts cycle through single, double and triple click.
const CLICK_COUNT_CYCLE: u8 = 3;
/// Keys right after a modal opens were meant for the prompt: they are not taken as answers.
pub(crate) const MODAL_GRACE: Duration = Duration::from_millis(350);

/// Path from `$REF_TUI_LOG`, if set.
fn log_path() -> Option<&'static std::path::PathBuf> {
    static PATH: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    PATH.get_or_init(|| std::env::var_os("REF_TUI_LOG").map(Into::into))
        .as_ref()
}

/// Appends a line to `$REF_TUI_LOG` (debugging aid; no-op when unset).
fn debug_log(s: &str) {
    use std::io::Write;
    if let Some(p) = log_path()
        && let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
    {
        // A failed write must not disturb the UI it is debugging.
        let _ = writeln!(f, "{} {s}", ref_proto::now_ms());
    }
}

pub fn handle(app: &mut App, ev: Event) {
    if log_path().is_some()
        && !matches!(
            ev,
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Moved,
                ..
            })
        )
    {
        debug_log(&format!("input {ev:?} mode={:?}", app.mode));
    }
    app.dirty = true;
    match ev {
        Event::Key(k) if k.kind != KeyEventKind::Release => key(app, k),
        Event::Mouse(m) => mouse(app, m),
        Event::Paste(s) => paste(app, s),
        _ => {}
    }
}

fn is(k: &KeyEvent, code: KeyCode, mods: KeyModifiers) -> bool {
    k.code == code && k.modifiers == mods
}

fn ctrl_char(k: &KeyEvent, c: char) -> bool {
    k.modifiers.contains(CTRL)
        && !k.modifiers.contains(ALT)
        && matches!(k.code, KeyCode::Char(x) if x.eq_ignore_ascii_case(&c))
}

fn key(app: &mut App, k: KeyEvent) {
    // Copy selection: ctrl+c / ctrl+shift+c / y (when a transcript selection exists).
    let editor_empty = app.active_editor().is_none_or(Editor::is_empty);
    let copy_key =
        ctrl_char(&k, 'c') || (is(&k, KeyCode::Char('y'), KeyModifiers::NONE) && editor_empty);
    if app.has_selection() && copy_key {
        app.copy_selection();
        return;
    }
    // Leader second key.
    if app.leader.take().is_some() {
        leader(app, k);
        return;
    }
    if ctrl_char(&k, 'x') {
        app.leader = Some(Instant::now());
        return;
    }
    // Permission modal has priority.
    if let Some((sid, req, _)) = app.active_permission() {
        if ctrl_char(&k, 'c') {
            exit_or_arm(app);
            return;
        }
        // Ignore keys right after the modal appears (they were meant for the prompt).
        // Plain typing still lands in the editor instead of being lost (or answering the modal).
        if app
            .perm
            .as_ref()
            .is_none_or(|p| p.opened.elapsed() < MODAL_GRACE)
        {
            if matches!(k.code, KeyCode::Char(_) | KeyCode::Backspace)
                && k.modifiers.difference(SHIFT).is_empty()
            {
                let ed = target_editor(app);
                if edit_key(ed, &k, false) {
                    app.history.reset();
                }
            }
            return;
        }
        match k.code {
            KeyCode::Char('a') | KeyCode::Enter => {
                app.reply_permission(&sid, &req.call_id, true, false)
            }
            KeyCode::Char('s') => app.reply_permission(&sid, &req.call_id, true, true),
            KeyCode::Char('d') | KeyCode::Esc => {
                app.reply_permission(&sid, &req.call_id, false, false)
            }
            KeyCode::Up | KeyCode::Char('k') => scroll_perm(app, -1),
            KeyCode::Down | KeyCode::Char('j') => scroll_perm(app, 1),
            KeyCode::PageUp => scroll_perm(app, -PAGE_ROWS),
            KeyCode::PageDown => scroll_perm(app, PAGE_ROWS),
            _ => {}
        }
        return;
    }
    if let Some((sid, req)) = app.active_question() {
        question_key(app, &sid, &req.call_id, k);
        return;
    }
    if app.dialog.is_some() {
        dialog_key(app, k);
        return;
    }
    if ctrl_char(&k, 'p') && app.ac.is_none() {
        app.open_palette();
        return;
    }
    match app.mode {
        Mode::Chat => chat_key(app, k),
        Mode::Watch => watch_key(app, k),
    }
}

fn scroll_perm(app: &mut App, d: i32) {
    if let Some(p) = app.perm.as_mut() {
        p.scroll_by(d);
    }
}

/// Prefix of the leader key bindings in `COMMANDS` (`ctrl+x n`, ...).
const LEADER_PREFIX: &str = "ctrl+x ";

/// Second press of a key within `ARMED_KEY_TIMEOUT` confirms (true); otherwise arms it (false).
fn confirm_or_arm(armed: &mut Option<Instant>) -> bool {
    if armed.is_some_and(|t| t.elapsed() < ARMED_KEY_TIMEOUT) {
        *armed = None;
        return true;
    }
    *armed = Some(Instant::now());
    false
}

fn exit_or_arm(app: &mut App) {
    if confirm_or_arm(&mut app.ctrlc_armed) {
        app.quit = true;
        return;
    }
    app.set_hint("ctrl+c again to exit", theme::WARNING);
}

/// Esc on a busy chat session: the second press within the window aborts the turn.
fn interrupt_current(app: &mut App) {
    if !confirm_or_arm(&mut app.esc_armed) {
        app.set_hint("esc again to interrupt", theme::WARNING);
        return;
    }
    app.hint = None;
    if let Some(sid) = app.current.clone() {
        app.abort(&sid);
    }
}

/// The command bound to `ctrl+x <c>` in the `COMMANDS` table (`?` also opens help).
fn leader_command(c: char) -> Option<&'static str> {
    if c == '?' {
        return Some(cmd::HELP);
    }
    let key = format!("{LEADER_PREFIX}{c}");
    COMMANDS.iter().find(|x| x.key == key).map(|x| x.name)
}

fn leader(app: &mut App, k: KeyEvent) {
    let KeyCode::Char(c) = k.code else {
        return;
    };
    if c.eq_ignore_ascii_case(&'k') {
        app.close_key();
        return;
    }
    let Some(cmd) = leader_command(c.to_ascii_lowercase()) else {
        return;
    };
    app.run_command(cmd, "");
}

fn dialog_key(app: &mut App, k: KeyEvent) {
    match app.dialog.as_mut() {
        Some(Dialog::Help(h)) => match k.code {
            KeyCode::Up | KeyCode::Char('k') => h.scroll_by(-1),
            KeyCode::Down | KeyCode::Char('j') => h.scroll_by(1),
            KeyCode::PageUp => h.scroll_by(-PAGE_ROWS),
            KeyCode::PageDown | KeyCode::Char(' ') => h.scroll_by(PAGE_ROWS),
            KeyCode::Home => h.scroll_to_top(),
            KeyCode::Char('g') if k.modifiers.is_empty() => h.scroll_to_top(),
            KeyCode::End | KeyCode::Char('G') => h.scroll_to_bottom(),
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => app.dialog = None,
            KeyCode::Char('c') if k.modifiers == CTRL => app.dialog = None,
            _ => app.dirty = false,
        },
        Some(Dialog::Mcp(d)) => {
            let n = app.mcp.len();
            match k.code {
                KeyCode::Up | KeyCode::Char('k') => d.move_sel(-1, n),
                KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => d.move_sel(1, n),
                KeyCode::Char('p') if k.modifiers == CTRL => d.move_sel(-1, n),
                KeyCode::Char('n') if k.modifiers == CTRL => d.move_sel(1, n),
                KeyCode::Enter | KeyCode::Char(' ') => d.set_expanded(&app.mcp, None),
                KeyCode::Right | KeyCode::Char('l') => d.set_expanded(&app.mcp, Some(true)),
                KeyCode::Left | KeyCode::Char('h') => d.set_expanded(&app.mcp, Some(false)),
                KeyCode::PageUp => d.scroll_by(-PAGE_ROWS),
                KeyCode::PageDown => d.scroll_by(PAGE_ROWS),
                KeyCode::Char('r') if k.modifiers.is_empty() => app.refresh_mcp(),
                KeyCode::Esc | KeyCode::Char('q') => app.dialog = None,
                KeyCode::Char('c') if k.modifiers == CTRL => app.dialog = None,
                _ => app.dirty = false,
            }
        }
        Some(Dialog::List(d)) => match k.code {
            KeyCode::Esc => {
                if d.confirm.is_some() {
                    d.confirm = None;
                } else if !d.cancel_arg_prompt() {
                    app.dialog = None;
                }
            }
            KeyCode::Up => d.move_sel(-1),
            KeyCode::Down => d.move_sel(1),
            KeyCode::Char('p') if k.modifiers == CTRL => d.move_sel(-1),
            KeyCode::Char('n') if k.modifiers == CTRL => d.move_sel(1),
            KeyCode::PageUp => d.page(-1),
            KeyCode::PageDown => d.page(1),
            KeyCode::Enter if k.modifiers.intersects(CTRL | ALT) && d.kind == Kind::Cd => {
                app.cd_dialog_confirm_typed();
            }
            KeyCode::Enter => app.dialog_submit(),
            KeyCode::Tab => {
                if d.kind == Kind::Cd {
                    app.cd_dialog_tab();
                } else if d.asking_arg.is_none() {
                    app.cycle_dialog_creator_filter();
                }
            }
            KeyCode::Backspace
                if d.kind == Kind::Cd
                    && k.modifiers.is_empty()
                    && d.filter.selection().is_none()
                    && d.filter.cursor == d.filter.text.len() =>
            {
                if let Some(up) = crate::ui::autocomplete::parent_dir_input(&d.filter.text) {
                    d.filter.set_text(&up);
                    app.on_cd_filter_edited();
                } else if edit_key(&mut d.filter, &k, false) {
                    app.on_cd_filter_edited();
                }
            }
            KeyCode::Char('d') if k.modifiers == CTRL => {
                if d.asking_arg.is_none() && matches!(d.kind, Kind::Sessions | Kind::CloseSessions)
                {
                    app.dialog_delete();
                }
            }
            KeyCode::Char('c') if k.modifiers == CTRL => {
                if d.filter.text.is_empty() {
                    if !d.cancel_arg_prompt() {
                        app.dialog = None;
                    }
                } else {
                    d.filter.clear();
                    if d.kind == Kind::Cd {
                        app.on_cd_filter_edited();
                    } else if d.kind == Kind::Sessions {
                        app.on_sessions_filter_edited(true);
                    } else {
                        d.refilter();
                    }
                }
            }
            _ => {
                let is_cd = d.kind == Kind::Cd;
                let is_sessions = d.kind == Kind::Sessions;
                let before = is_sessions.then(|| d.filter.text.clone());
                if edit_key(&mut d.filter, &k, false) {
                    if is_cd {
                        app.on_cd_filter_edited();
                    } else if is_sessions {
                        let changed = before.as_deref() != Some(d.filter.text.as_str());
                        app.on_sessions_filter_edited(changed);
                    } else {
                        d.refilter();
                    }
                }
            }
        },
        None => {}
    }
}

/// Sets the selection mode, then moves the cursor with `step`.
fn move_cursor(e: &mut Editor, extend: bool, step: fn(&mut Editor)) {
    e.select_mode(extend);
    step(e);
}

/// Like `move_cursor`, with a coarser step when `coarse` is set (word or buffer).
fn move_by(
    e: &mut Editor,
    extend: bool,
    coarse: bool,
    coarse_step: fn(&mut Editor),
    step: fn(&mut Editor),
) {
    move_cursor(e, extend, if coarse { coarse_step } else { step });
}

/// Generic editing keys. Returns true if the text changed (or cursor moved).
pub(crate) fn edit_key(e: &mut Editor, k: &KeyEvent, multiline: bool) -> bool {
    let m = k.modifiers;
    let shift = m.contains(SHIFT);
    match k.code {
        KeyCode::Char(c) if m.contains(CTRL) && !m.contains(ALT) => match c.to_ascii_lowercase() {
            'a' => move_cursor(e, false, Editor::home),
            'e' => move_cursor(e, false, Editor::end),
            'b' => move_cursor(e, false, Editor::left),
            'f' => move_cursor(e, false, Editor::right),
            'w' | 'h' => e.delete_word_back(),
            'u' => e.kill_to_start(),
            'k' => e.kill_to_end(),
            'z' | '-' => e.undo(),
            'y' | '.' => e.redo(),
            'j' if multiline => e.newline(),
            _ => return false,
        },
        KeyCode::Char(c) if m.contains(ALT) && !m.contains(CTRL) => match c {
            'b' => e.word_left(),
            'f' => e.word_right(),
            'd' => e.delete_word_forward(),
            _ => return false,
        },
        KeyCode::Char(c) => {
            let mut buf = [0u8; 4];
            e.insert(c.encode_utf8(&mut buf));
        }
        KeyCode::Backspace if m.intersects(CTRL | ALT) => e.delete_word_back(),
        KeyCode::Backspace => e.backspace(),
        KeyCode::Delete if m.intersects(CTRL | ALT) => e.delete_word_forward(),
        KeyCode::Delete => e.delete(),
        KeyCode::Left => {
            let by_word = m.intersects(CTRL | ALT);
            move_by(e, shift, by_word, Editor::word_left, Editor::left)
        }
        KeyCode::Right => {
            let by_word = m.intersects(CTRL | ALT);
            move_by(e, shift, by_word, Editor::word_right, Editor::right)
        }
        KeyCode::Home => move_by(e, shift, m.contains(CTRL), Editor::buf_start, Editor::home),
        KeyCode::End => move_by(e, shift, m.contains(CTRL), Editor::buf_end, Editor::end),
        KeyCode::Enter if multiline && m.intersects(SHIFT | ALT | CTRL) => e.newline(),
        _ => return false,
    }
    true
}

/// Shift+Up/Down scroll the transcript only while the prompt is one row with no selection.
fn single_row_editor_without_selection(app: &App) -> bool {
    app.editor.selection().is_none()
        && app
            .editor
            .height(app.editor.last_width.max(MIN_EDITOR_COLS))
            <= 1
}

/// `!` at the start of an empty prompt switches to shell mode.
fn can_enter_shell_mode(app: &App, m: KeyModifiers) -> bool {
    app.editor.text.is_empty() && !app.shell_mode && m.difference(SHIFT).is_empty()
}

/// Up on the first prompt row: older history at its boundary, else to the start of the text.
fn history_prev(app: &mut App) {
    let at_history_boundary =
        app.editor.text.is_empty() || app.history.browsing() || app.editor.cursor == 0;
    if !at_history_boundary {
        app.editor.buf_start();
        return;
    }
    let cur = app.editor.text.clone();
    if let Some(h) = app.history.prev_mode(&cur, app.shell_mode) {
        set_from_history(app, &h);
    }
}

/// Down on the last prompt row: newer history while browsing, else to the end of the text.
fn history_next(app: &mut App) {
    if !app.history.browsing() {
        app.editor.buf_end();
        return;
    }
    let Some((h, restore_shell)) = app.history.next_mode() else {
        return;
    };
    if restore_shell {
        app.shell_mode = true;
        app.editor.set_text(h.strip_prefix('!').unwrap_or(&h));
        app.ac = None;
    } else {
        set_from_history(app, &h);
    }
}

fn is_undo_or_redo(k: &KeyEvent) -> bool {
    k.modifiers.contains(CTRL)
        && !k.modifiers.contains(ALT)
        && matches!(
            k.code,
            KeyCode::Char(c) if matches!(c.to_ascii_lowercase(), 'z' | '-' | 'y' | '.')
        )
}

fn edit_chat_input(app: &mut App, k: &KeyEvent, before: &str) {
    if !edit_key(&mut app.editor, k, true) {
        return;
    }
    if app.editor.text != before {
        app.history.reset();
        app.suppress_ac = false;
        app.update_autocomplete_on_edit();
        // Typed / pasted path to an image → attachment (disabled in `!` shell mode and on undo/redo).
        if !app.shell_mode && !is_undo_or_redo(k) {
            maybe_image_path(app);
        }
    }
    if app.has_selection() {
        app.clear_selection();
    }
}
fn chat_key(app: &mut App, k: KeyEvent) {
    let m = k.modifiers;
    // ctrl+c: clear editor / exit twice.
    if ctrl_char(&k, 'c') {
        if let Some(t) = app.editor.selected_text() {
            app.copy_text(&t, "selection");
            app.editor.select_mode(false);
        } else if !app.editor.text.is_empty() || app.shell_mode {
            if app.editor.text.trim().chars().count() >= MIN_DRAFT_CHARS_FOR_HISTORY {
                let t = app.editor.text.clone();
                app.push_history(&t);
            }
            app.editor.clear();
            app.exit_shell_mode();
        } else {
            exit_or_arm(app);
        }
        return;
    }
    if ctrl_char(&k, 'd') && app.editor.is_empty() {
        app.quit = true;
        return;
    }
    if ctrl_char(&k, 't') {
        app.cycle_variant();
        return;
    }
    if ctrl_char(&k, 'v') {
        clipboard_paste(app);
        return;
    }
    if ctrl_char(&k, 'l') {
        app.force_clear = true;
        return;
    }
    // Autocomplete navigation.
    if app.ac.is_some() && handle_ac_key(app, &k) {
        return;
    }
    let before = app.editor.text.clone();
    let before_cursor = app.editor.cursor;
    match k.code {
        KeyCode::Esc => {
            if app.has_selection() {
                app.clear_selection();
            } else if app.editor.selection().is_some() {
                app.editor.select_mode(false);
            } else if app.shell_mode {
                app.exit_shell_mode();
            } else if app.cur().is_some_and(has_delayed_tool) {
                // "runs in Ns · esc to cancel": a single esc stops the turn before the tool runs.
                if let Some(sid) = app.current.clone() {
                    app.abort(&sid);
                    app.toasts.info("Cancelled delayed tool call");
                }
            } else if app.cur().is_some_and(|v| v.is_busy()) {
                interrupt_current(app);
            }
        }
        KeyCode::Enter if m.is_empty() => app.submit(),
        KeyCode::Tab if m.is_empty() => {
            if app.shell_mode {
                app.trigger_tab_completion();
            } else if !app.trigger_tab_completion() {
                app.cycle_agent(1);
            }
        }
        KeyCode::BackTab => {
            if !app.shell_mode {
                app.cycle_agent(-1);
            }
        }
        KeyCode::PageUp => scroll(app, |c| c.page(-1, false)),
        KeyCode::PageDown => scroll(app, |c| c.page(1, false)),
        KeyCode::Char('u') if m == CTRL | ALT => scroll(app, |c| c.page(-1, true)),
        KeyCode::Char('d') if m == CTRL | ALT => scroll(app, |c| c.page(1, true)),
        KeyCode::Char('y') if m == CTRL | ALT => scroll(app, |c| c.scroll_by(-1, false)),
        KeyCode::Char('e') if m == CTRL | ALT => scroll(app, |c| c.scroll_by(1, false)),
        KeyCode::Char('g') if m == CTRL => scroll(app, |c| c.scroll_to_top()),
        KeyCode::Char('g') if m == CTRL | ALT => scroll(app, |c| c.scroll_to_bottom(true)),
        KeyCode::Up if m == SHIFT && single_row_editor_without_selection(app) => {
            scroll(app, |c| c.scroll_by(-1, false))
        }
        KeyCode::Down if m == SHIFT && single_row_editor_without_selection(app) => {
            scroll(app, |c| c.scroll_by(1, false))
        }
        KeyCode::Home if app.editor.is_empty() => scroll(app, |c| c.scroll_to_top()),
        KeyCode::End if app.editor.is_empty() => scroll(app, |c| c.scroll_to_bottom(true)),
        KeyCode::Up if !m.contains(SHIFT) => {
            if app.editor.on_first_row() {
                history_prev(app);
            } else {
                app.editor.select_mode(false);
                app.editor.up();
            }
        }
        KeyCode::Down if !m.contains(SHIFT) => {
            if app.editor.on_last_row() {
                history_next(app);
            } else {
                app.editor.select_mode(false);
                app.editor.down();
            }
        }
        KeyCode::Up => {
            app.editor.select_mode(true);
            if !app.editor.up() {
                app.editor.buf_start();
            }
        }
        KeyCode::Down => {
            app.editor.select_mode(true);
            if !app.editor.down() {
                app.editor.buf_end();
            }
        }
        KeyCode::Char('!') if can_enter_shell_mode(app, m) => {
            app.shell_mode = true;
            app.clear_autocomplete();
        }
        KeyCode::Backspace if app.shell_mode && app.editor.text.is_empty() => {
            app.exit_shell_mode();
        }
        _ => edit_chat_input(app, &k, &before),
    }
    if app.editor.text == before && app.editor.cursor != before_cursor && app.ac.is_some() {
        app.update_autocomplete();
    }
}

fn has_delayed_tool(v: &crate::state::SessionView) -> bool {
    v.messages.last().is_some_and(|m| {
        m.parts.iter().any(|p| {
            matches!(
                p,
                ref_proto::Part::Tool {
                    state: ref_proto::ToolState::Delayed,
                    ..
                }
            )
        })
    })
}

fn set_from_history(app: &mut App, h: &str) {
    if let Some(rest) = h.strip_prefix('!') {
        app.shell_mode = true;
        app.editor.set_text(rest);
    } else {
        app.shell_mode = false;
        app.clear_shell_complete_cache();
        app.editor.set_text(h);
    }
    app.clear_autocomplete();
}

fn handle_ac_key(app: &mut App, k: &KeyEvent) -> bool {
    let m = k.modifiers;
    match k.code {
        KeyCode::Up => ac_move(app, -1),
        KeyCode::Down => ac_move(app, 1),
        KeyCode::Char('p') if m == CTRL => ac_move(app, -1),
        KeyCode::Char('n') if m == CTRL => ac_move(app, 1),
        KeyCode::Tab => accept_completion_step(app, false),
        KeyCode::Enter if m.is_empty() => {
            let freeform_submit = app
                .ac
                .as_ref()
                .is_some_and(|ac| ac.freeform_enter && !ac.explicitly_selected);
            if freeform_submit {
                app.ac = None;
                submit_active(app);
            } else {
                accept_completion_step(app, true);
            }
        }
        KeyCode::Esc => {
            app.dismiss_autocomplete();
        }
        _ => return false,
    }
    true
}

fn accept_completion_step(app: &mut App, run: bool) {
    let keep_open = app
        .ac
        .as_ref()
        .and_then(|ac| ac.items.get(ac.sel))
        .is_some_and(|(v, _, _)| ac_keeps_open_on_tab(app.ac.as_ref().map(|a| &a.kind), v));
    let executed = app.accept_completion(run);
    if executed {
        return;
    }
    if keep_open {
        refresh_autocomplete_after_accept(app);
    } else {
        app.ac = None;
    }
}

fn submit_active(app: &mut App) {
    match app.mode {
        Mode::Chat => app.submit(),
        Mode::Watch => {
            if let Some(f) = app.watch.focus.clone() {
                submit_watch_tile(app, &f);
            }
        }
    }
}

fn submit_watch_tile(app: &mut App, f: &str) {
    let in_shell = app.watch.shell_modes.contains(f);
    let empty = app.watch.inputs.get(f).is_none_or(Editor::is_empty);
    if empty {
        if !in_shell {
            app.zoom(f);
        }
        return;
    }
    if app.submit_tile_command(f) {
        return;
    }
    let sub = app.watch.inputs.entry(f.to_string()).or_default().submit();
    let images = sub.images.into_iter().map(|(i, _)| i).collect();
    app.send_prompt(f, sub.text, images);
}
fn ac_move(app: &mut App, d: i32) {
    if let Some(ac) = app.ac.as_mut() {
        ac.move_sel(d);
    }
}

fn ac_keeps_open_on_tab(kind: Option<&crate::app::AcKind>, value: &str) -> bool {
    use crate::app::AcKind;
    match kind {
        Some(AcKind::SlashArg) => value.ends_with(['/', '\\']) || value.ends_with(' '),
        Some(AcKind::ShellPath | AcKind::ShellArg) => value.ends_with(['/', '\\']),
        Some(AcKind::Slash | AcKind::File | AcKind::ShellCmd) | None => false,
    }
}

fn scroll(app: &mut App, f: impl FnOnce(&mut chat::ChatState)) {
    if let Some(v) = app.cur_mut() {
        f(&mut v.chat);
    }
}

/// If the editor text is exactly a path to an image file, convert it to an attachment.
fn maybe_image_path(app: &mut App) {
    let t = app.editor.text.trim();
    if t.len() < MIN_IMAGE_PATH_LEN || !t.contains('.') || t.contains('\n') {
        return;
    }
    match crate::clipboard::image_from_path(t) {
        Ok(Some((img, name))) => {
            app.editor.replace_with_image(img, Some(name.clone()));
            app.ac = None;
            app.toasts.info(format!("Attached image {name}"));
        }
        Ok(None) => {}
        Err(e) => app.toasts.error(e),
    }
}

fn clipboard_paste(app: &mut App) {
    clipboard_paste_with(
        app,
        crate::clipboard::paste_image,
        crate::clipboard::paste_text,
    );
}

pub(crate) fn clipboard_paste_with(
    app: &mut App,
    read_image: impl FnOnce() -> Result<Option<ImageData>, String>,
    read_text: impl FnOnce() -> Result<Option<String>, String>,
) {
    let in_shell = app.active_shell_mode();
    let img_err = if in_shell {
        None
    } else {
        match read_image() {
            Ok(Some(img)) => {
                let target = target_editor(app);
                target.attach_image(img, Some("clipboard".into()));
                app.ac = None;
                app.toasts.info("Pasted image from clipboard");
                return;
            }
            Ok(None) => None,
            Err(e) => Some(e),
        }
    };
    match read_text() {
        Ok(Some(t)) => paste(app, t),
        Ok(None) => match img_err {
            Some(e) => app.toasts.error(format!("Clipboard image error: {e}")),
            None => app.toasts.warn("Clipboard is empty"),
        },
        Err(e) => app.toasts.error(format!("Clipboard paste failed: {e}")),
    }
}

fn target_editor(app: &mut App) -> &mut Editor {
    if app.mode == Mode::Watch
        && let Some(f) = app.watch.focus.clone()
    {
        return app.watch.inputs.entry(f).or_default();
    }
    &mut app.editor
}

/// The question modal. Like the permission modal it ignores Enter, digits and Esc right after
/// it opens (those keys were meant for the prompt), and lets plain typing land in the editor.
fn question_key(app: &mut App, sid: &str, call_id: &str, k: KeyEvent) {
    if ctrl_char(&k, 'c') {
        exit_or_arm(app);
        return;
    }
    let ready = app
        .question
        .as_ref()
        .is_some_and(|q| q.opened.elapsed() >= MODAL_GRACE);
    if !ready {
        if matches!(k.code, KeyCode::Char(_) | KeyCode::Backspace)
            && k.modifiers.difference(SHIFT).is_empty()
        {
            let ed = target_editor(app);
            if edit_key(ed, &k, false) {
                app.history.reset();
            }
        }
        return;
    }
    let Some(outcome) = app.question.as_mut().map(|q| q.key(&k)) else {
        return;
    };
    finish_question(app, sid, call_id, outcome);
}

fn finish_question(app: &mut App, sid: &str, call_id: &str, outcome: Outcome) {
    match outcome {
        Outcome::Open => {}
        Outcome::Submit(answers) => app.reply_question(sid, call_id, Some(answers)),
        Outcome::Dismiss => app.reply_question(sid, call_id, None),
    }
}

/// Images named by a pasted text: every whitespace-separated path, or the whole text as one
/// path (names with spaces). Empty when the text is not an image paste.
fn pasted_images(s: &str) -> Result<Vec<(ImageData, String)>, String> {
    let paths: Vec<&str> = s.split_whitespace().collect();
    if paths.is_empty() || paths.len() > MAX_PASTED_PATHS {
        return Ok(Vec::new());
    }
    let mut imgs = Vec::new();
    for p in &paths {
        if let Some(img) = crate::clipboard::image_from_path(p)? {
            imgs.push(img);
        }
    }
    let all_paths_are_images = imgs.len() == paths.len();
    if all_paths_are_images {
        return Ok(imgs);
    }
    if !imgs.is_empty() {
        return Ok(Vec::new());
    }
    Ok(crate::clipboard::image_from_path(s.trim())?
        .into_iter()
        .collect())
}

fn paste(app: &mut App, s: String) {
    if let Some(Dialog::List(d)) = app.dialog.as_mut() {
        d.filter.insert(&crate::text::one_line(&s));
        if d.kind == Kind::Cd {
            app.on_cd_filter_edited();
        } else if d.kind == Kind::Sessions {
            app.on_sessions_filter_edited(true);
        } else {
            d.refilter();
        }
        return;
    }
    if app.active_permission().is_some() {
        return;
    }
    if app.active_question().is_some() {
        if let Some(editor) = app.question.as_mut().and_then(|q| q.edit.as_mut()) {
            editor.insert(&crate::text::one_line(&s));
        }
        return;
    }
    let in_shell = app.active_shell_mode();
    if s.trim().is_empty() {
        if !in_shell {
            // Empty bracketed paste: often an image-only clipboard.
            match crate::clipboard::paste_image() {
                Ok(Some(img)) => {
                    target_editor(app).attach_image(img, Some("clipboard".into()));
                    app.ac = None;
                    app.toasts.info("Pasted image from clipboard");
                }
                Ok(None) => {}
                Err(e) => app.toasts.error(format!("Clipboard image error: {e}")),
            }
        }
        return;
    }
    // One or more image paths (skipped in `!` shell mode so shell commands get literal paths).
    if !in_shell {
        match pasted_images(&s) {
            Ok(imgs) if !imgs.is_empty() => {
                let n = imgs.len();
                let ed = target_editor(app);
                for (img, name) in imgs {
                    ed.attach_image(img, Some(name));
                }
                app.ac = None;
                app.toasts.info(format!(
                    "Attached {n} image{}",
                    if n > 1 { "s" } else { "" }
                ));
                return;
            }
            Ok(_) => {}
            Err(e) => {
                app.toasts.error(e);
                return;
            }
        }
    }
    if app.mode == Mode::Watch {
        let one = crate::text::one_line(&s);
        target_editor(app).insert(&one);
        app.history.reset();
        app.suppress_ac = false;
        app.update_autocomplete_on_edit();
        return;
    }
    app.editor.paste(&s);
    app.history.reset();
    app.suppress_ac = false;
    app.update_autocomplete_on_edit();
}

// ---------------------------------------------------------------------------
// Watch mode
// ---------------------------------------------------------------------------

fn can_enter_watch_shell_mode(app: &App, m: KeyModifiers) -> bool {
    let Some(f) = &app.watch.focus else {
        return false;
    };
    !app.watch.shell_modes.contains(f)
        && m.difference(SHIFT).is_empty()
        && app.watch.inputs.get(f).is_none_or(|ed| ed.text.is_empty())
}

fn can_leave_watch_shell_mode(app: &App) -> bool {
    let Some(f) = &app.watch.focus else {
        return false;
    };
    app.watch.shell_modes.contains(f) && app.watch.inputs.get(f).is_none_or(|ed| ed.text.is_empty())
}

fn edit_watch_input(app: &mut App, f: String, k: &KeyEvent) {
    let (changed, cursor_moved) = {
        let ed = app.watch.inputs.entry(f).or_default();
        let before = ed.text.clone();
        let before_cursor = ed.cursor;
        if !edit_key(ed, k, false) {
            return;
        }
        (ed.text != before, ed.cursor != before_cursor)
    };
    if changed {
        app.history.reset();
        app.suppress_ac = false;
        app.update_autocomplete_on_edit();
        if app.has_selection() {
            app.clear_selection();
        }
    } else if cursor_moved && app.ac.is_some() {
        app.update_autocomplete();
    }
}

fn watch_key(app: &mut App, k: KeyEvent) {
    let m = k.modifiers;
    let tiles: Vec<String> = app.watch.tiles.iter().map(|t| t.0.clone()).collect();
    let all = app.tile_ids();
    let focus_idx = app
        .watch
        .focus
        .as_ref()
        .and_then(|f| tiles.iter().position(|t| t == f));
    let dir = match k.code {
        KeyCode::Left if m.intersects(CTRL | ALT) => Some((-1, 0)),
        KeyCode::Right if m.intersects(CTRL | ALT) => Some((1, 0)),
        KeyCode::Up if m.intersects(CTRL | ALT) => Some((0, -1)),
        KeyCode::Down if m.intersects(CTRL | ALT) => Some((0, 1)),
        _ => None,
    };
    if let Some((dx, dy)) = dir {
        let rects: Vec<_> = app.watch.tiles.iter().map(|t| t.1).collect();
        if let Some(i) = focus_idx.and_then(|i| split::neighbor(&rects, i, dx, dy)) {
            app.set_watch_focus(Some(tiles[i].clone()));
        } else if focus_idx.is_none() && !tiles.is_empty() {
            app.set_watch_focus(Some(tiles[0].clone()));
        } else {
            app.dirty = false;
        }
        return;
    }
    if ctrl_char(&k, 'c') {
        if let Some(f) = app.watch.focus.clone() {
            let sel = app.watch.inputs.get(&f).and_then(|e| e.selected_text());
            if let Some(t) = sel {
                app.copy_text(&t, "selection");
                if let Some(e) = app.watch.inputs.get_mut(&f) {
                    e.select_mode(false);
                }
                return;
            }
            let has_input = app.watch.inputs.get(&f).is_some_and(|e| !e.is_empty())
                || app.watch.shell_modes.contains(&f);
            if has_input {
                if let Some(e) = app.watch.inputs.get_mut(&f) {
                    e.clear();
                }
                app.exit_watch_shell_mode(&f);
                return;
            }
        }
        exit_or_arm(app);
        return;
    }
    if ctrl_char(&k, 'd') {
        let empty = app
            .watch
            .focus
            .as_ref()
            .and_then(|f| app.watch.inputs.get(f))
            .is_none_or(|e| e.is_empty());
        if empty {
            app.quit = true;
            return;
        }
    }
    if ctrl_char(&k, 'v') {
        clipboard_paste(app);
        return;
    }
    if ctrl_char(&k, 'n') || ctrl_char(&k, 'b') {
        let pages = app.watch.pages.max(1);
        let per = split::per_page(app.screen.width, app.screen.height.saturating_sub(1)).max(1);
        let p = if ctrl_char(&k, 'n') {
            (app.watch.page + 1) % pages
        } else {
            (app.watch.page + pages - 1) % pages
        };
        app.watch.page = p;
        app.clear_selection();
        // The focused tile pins its page: move focus along (if any) instead of clearing it.
        if app.watch.focus.is_some() {
            app.set_watch_focus(all.get(p * per).cloned());
        }
        return;
    }
    if ctrl_char(&k, 'o') {
        if let Some(f) = app.watch.focus.clone() {
            app.zoom(&f);
        }
        return;
    }
    if app.ac.is_some() && handle_ac_key(app, &k) {
        return;
    }
    match k.code {
        KeyCode::Tab if m.is_empty() && app.active_shell_mode() => {
            app.trigger_tab_completion();
        }
        KeyCode::Tab | KeyCode::BackTab => {
            if all.is_empty() {
                return;
            }
            let back = k.code == KeyCode::BackTab;
            let n = all.len() as i32;

            let i = match app
                .watch
                .focus
                .as_ref()
                .and_then(|f| all.iter().position(|t| t == f))
            {
                Some(i) => (i as i32 + if back { -1 } else { 1 }).rem_euclid(n),
                // Nothing focused: start with the first (shift+tab: last) tile of this page.
                None => {
                    let page: Vec<usize> = tiles
                        .iter()
                        .filter_map(|t| all.iter().position(|a| a == t))
                        .collect();
                    let pick = if back { page.last() } else { page.first() };
                    pick.map(|&p| p as i32)
                        .unwrap_or(if back { n - 1 } else { 0 })
                }
            };

            app.set_watch_focus(Some(all[i as usize].clone()));
        }
        // Scroll the focused tile's transcript.
        KeyCode::PageUp | KeyCode::PageDown | KeyCode::Home | KeyCode::End
            if tile_scroll_key(app, &k) => {}
        KeyCode::Up | KeyCode::Down if m == SHIFT && tile_scroll_key(app, &k) => {}
        KeyCode::Esc => {
            if app.has_selection() {
                app.clear_selection();
                return;
            }
            let Some(f) = app.watch.focus.clone() else {
                app.dirty = false;
                return;
            };
            if app
                .watch
                .inputs
                .get(&f)
                .is_some_and(|e| e.selection().is_some())
            {
                if let Some(e) = app.watch.inputs.get_mut(&f) {
                    e.select_mode(false);
                }
                return;
            }
            if app.exit_watch_shell_mode(&f) {
                return;
            }
            let busy = app.views.get(&f).is_some_and(|v| v.is_busy());
            let has_text = app.watch.inputs.get(&f).is_some_and(|e| !e.is_empty());
            if has_text {
                // First esc clears the input; the next one unfocuses (or arms the abort).
                if let Some(e) = app.watch.inputs.get_mut(&f) {
                    e.clear();
                }
                app.clear_autocomplete();
            } else if busy {
                let name = app
                    .views
                    .get(&f)
                    .map(|v| v.info.name.clone())
                    .unwrap_or_default();

                if confirm_or_arm(&mut app.esc_armed) {
                    app.hint = None;
                    app.abort(&f);
                    app.toasts.info(format!("Interrupted \"{name}\""));
                } else {
                    app.set_hint(format!("esc again to interrupt \"{name}\""), theme::WARNING);
                }
            } else {
                app.set_watch_focus(None);
            }
        }
        KeyCode::Enter if m.is_empty() && app.watch.focus.is_some() => {
            let f = app.watch.focus.clone().expect("guarded by arm condition");
            submit_watch_tile(app, &f);
        }
        KeyCode::Char('!') if can_enter_watch_shell_mode(app, m) => {
            if let Some(f) = app.watch.focus.clone() {
                app.watch.shell_modes.insert(f);
                app.clear_autocomplete();
            }
        }
        KeyCode::Backspace if can_leave_watch_shell_mode(app) => {
            if let Some(f) = app.watch.focus.clone() {
                app.exit_watch_shell_mode(&f);
            }
        }
        _ => match app.watch.focus.clone() {
            Some(f) => edit_watch_input(app, f, &k),
            None => {
                if matches!(
                    k.code,
                    KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Enter
                ) && !app.watch.tiles.is_empty()
                {
                    app.set_hint("click or tab to focus a session", theme::MUTED);
                } else {
                    app.dirty = false;
                }
            }
        },
    }
}

/// PgUp/PgDn/Home/End/shift+↑↓ on the focused tile. Home/End only while its input is empty.
fn tile_scroll_key(app: &mut App, k: &KeyEvent) -> bool {
    let Some(f) = app.watch.focus.clone() else {
        return false;
    };
    let empty = app.watch.inputs.get(&f).is_none_or(|e| e.is_empty());
    let Some(v) = app.views.get_mut(&f) else {
        return false;
    };
    let c = &mut v.chat;
    match k.code {
        KeyCode::PageUp => c.page(-1, false),
        KeyCode::PageDown => c.page(1, false),
        KeyCode::Up => c.scroll_by(-1, false),
        KeyCode::Down => c.scroll_by(1, false),
        KeyCode::Home if empty => c.scroll_to_top(),
        KeyCode::End if empty => c.scroll_to_bottom(true),
        _ => return false,
    }
    true
}

// ---------------------------------------------------------------------------
// Mouse
// ---------------------------------------------------------------------------

fn click_count(app: &mut App, x: u16, y: u16) -> u8 {
    let n = match app.last_click {
        Some((t, lx, ly, n)) if t.elapsed() < MULTI_CLICK_WINDOW && lx == x && ly == y => {
            (n % CLICK_COUNT_CYCLE) + 1
        }
        _ => 1,
    };
    app.last_click = Some((Instant::now(), x, y, n));
    n
}

fn mouse(app: &mut App, me: MouseEvent) {
    let (x, y) = (me.column, me.row);
    // Deduplicate stationary Moved events so keypresses/redraws never steal keyboard selection or clear confirm.
    if matches!(me.kind, MouseEventKind::Moved) {
        if app.last_mouse_pos == Some((x, y)) {
            app.dirty = false;
            return;
        }
        app.last_mouse_pos = Some((x, y));
    } else {
        app.last_mouse_pos = Some((x, y));
    }
    let pos = Position::new(x, y);
    // Permission modal.
    if let Some((sid, req, _)) = app.active_permission() {
        match me.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(a) = app.perm.as_ref().and_then(|p| p.button_at(x, y)) {
                    match a {
                        PermissionAction::AllowOnce => {
                            app.reply_permission(&sid, &req.call_id, true, false)
                        }
                        PermissionAction::AllowAlways => {
                            app.reply_permission(&sid, &req.call_id, true, true)
                        }
                        PermissionAction::Deny => {
                            app.reply_permission(&sid, &req.call_id, false, false)
                        }
                    }
                }
            }
            MouseEventKind::ScrollUp => scroll_perm(app, -WHEEL_SCROLL_ROWS),
            MouseEventKind::ScrollDown => scroll_perm(app, WHEEL_SCROLL_ROWS),
            _ => app.dirty = false,
        }
        return;
    }
    if let Some((sid, req)) = app.active_question() {
        match me.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(outcome) = app.question.as_mut().map(|q| q.click(x, y)) {
                    finish_question(app, &sid, &req.call_id, outcome);
                }
            }
            MouseEventKind::ScrollUp => {
                if let Some(q) = app.question.as_mut() {
                    q.wheel(-WHEEL_SCROLL_ROWS);
                }
            }
            MouseEventKind::ScrollDown => {
                if let Some(q) = app.question.as_mut() {
                    q.wheel(WHEEL_SCROLL_ROWS);
                }
            }
            _ => app.dirty = false,
        }
        return;
    }
    if let Some(d) = app.dialog.as_mut() {
        match (d, me.kind) {
            (Dialog::List(d), MouseEventKind::Down(MouseButton::Left)) => {
                let r = d.rect;
                if !r.contains(pos) {
                    app.dialog = None;
                } else if d.click_filter(x, y) {
                    // Positioned cursor in search / argument bar.
                } else if d.kind == Kind::CloseSessions {
                    // Destructive: a click only selects; enter closes.
                    if d.item_at(x, y).is_some_and(|i| i != d.sel) {
                        d.click(x, y);
                        d.confirm = None;
                    }
                } else if d.click(x, y) {
                    app.dialog_submit();
                }
            }
            (Dialog::List(d), MouseEventKind::ScrollUp) => d.scroll(-1),
            (Dialog::List(d), MouseEventKind::ScrollDown) => d.scroll(1),
            (Dialog::List(d), MouseEventKind::Moved) => {
                if let Some(i) = d.item_at(x, y) {
                    if d.sel != i {
                        d.sel = i;
                        d.confirm = None;
                    } else {
                        app.dirty = false;
                    }
                } else {
                    app.dirty = false;
                }
            }
            (Dialog::Help(h), MouseEventKind::ScrollUp) => h.scroll_by(-WHEEL_SCROLL_ROWS),
            (Dialog::Help(h), MouseEventKind::ScrollDown) => h.scroll_by(WHEEL_SCROLL_ROWS),
            (Dialog::Help(h), MouseEventKind::Down(MouseButton::Left)) => {
                if !h.rect.contains(pos) {
                    app.dialog = None;
                } else {
                    app.dirty = false;
                }
            }
            (Dialog::Mcp(d), MouseEventKind::Down(MouseButton::Left)) => {
                if !d.rect.contains(pos) {
                    app.dialog = None;
                } else if let Some(i) = d.server_at(x, y) {
                    d.sel = i;
                    d.set_expanded(&app.mcp, None);
                }
            }
            (Dialog::Mcp(d), MouseEventKind::ScrollUp) => d.scroll_by(-WHEEL_SCROLL_ROWS),
            (Dialog::Mcp(d), MouseEventKind::ScrollDown) => d.scroll_by(WHEEL_SCROLL_ROWS),
            _ => app.dirty = false,
        }
        return;
    }
    if app.mode == Mode::Watch {
        watch_mouse(app, me);
        return;
    }
    // Chat mode.
    let Some(sid) = app.current.clone() else {
        return;
    };
    let lay = render::chat_layout(app, app.screen);
    if autocomplete_mouse(app, me) {
        return;
    }
    // Wheel over the sidebar scrolls the sidebar (never the chat).
    if let MouseEventKind::ScrollUp | MouseEventKind::ScrollDown = me.kind
        && let Some(sb) = lay.sidebar
        && x + 1 >= sb.x
    {
        let before = app.sidebar_scroll;
        app.sidebar_scroll = if me.kind == MouseEventKind::ScrollUp {
            before.saturating_sub(WHEEL_SCROLL_ROWS as usize)
        } else {
            (before + WHEEL_SCROLL_ROWS as usize).min(app.sidebar_max_scroll)
        };
        app.dirty = app.sidebar_scroll != before;
        return;
    }
    // Sidebar clicks: switch session.
    if let MouseEventKind::Down(MouseButton::Left) = me.kind
        && let Some(sb) = lay.sidebar
        && x >= sb.x
    {
        if let Some((_, id)) = app.sidebar_rows.iter().find(|(ry, _)| *ry == y).cloned()
            && id != sid
        {
            app.attach_fg(&id);
        } else if let Some((_, name)) = app
            .sidebar_mcp_rows
            .iter()
            .find(|(ry, _)| *ry == y)
            .cloned()
        {
            app.open_mcp(Some(&name));
        }
        return;
    }
    let Some(v) = app.views.get_mut(&sid) else {
        return;
    };
    let c = &mut v.chat;
    match me.kind {
        MouseEventKind::ScrollUp => {
            if c.contains(x, y) || y < lay.prompt.y {
                c.scroll_by(-WHEEL_SCROLL_ROWS, false);
            }
        }
        MouseEventKind::ScrollDown => {
            if c.contains(x, y) || y < lay.prompt.y {
                c.scroll_by(WHEEL_SCROLL_ROWS, false);
            }
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if chat::pill_hit(c, x, y) {
                c.scroll_to_bottom(true);
                return;
            }
            if c.contains(x, y) {
                app.editor.anchor = None;
                // Repeated clicks on a button (e.g. confirming undo) are clicks, not word selection.
                let on_button = c.hit_at(x, y).is_some_and(|h| h.is_button());
                let n = click_count(app, x, y);
                let n = if on_button { 1 } else { n };
                let Some(v) = app.views.get_mut(&sid) else {
                    return;
                };
                v.chat.mouse_down(x, y, n);
                if v.chat.has_selection() {
                    app.sel_owner = Some(sid);
                }
            } else if y >= lay.prompt.y && y < lay.prompt.y + lay.prompt.height {
                app.clear_selection();
                // Click in the editor: move the cursor; drag / double / triple click select.
                let e = lay.editor;
                if y >= e.y && y < e.y + e.height {
                    let before_cursor = app.editor.cursor;
                    let off = app.editor.offset_at(e, x, y);
                    let n = click_count(app, x, y);
                    app.editor.mouse_down(off, n);
                    if app.editor.cursor != before_cursor && app.ac.is_some() {
                        app.update_autocomplete();
                    }
                }
            }
        }
        MouseEventKind::Drag(MouseButton::Left) if app.editor.dragging => {
            let before_cursor = app.editor.cursor;
            let off = app.editor.offset_at(lay.editor, x, y);
            app.editor.mouse_drag(off);
            if app.editor.cursor != before_cursor && app.ac.is_some() {
                app.update_autocomplete();
            }
        }
        MouseEventKind::Up(MouseButton::Left) if app.editor.dragging => app.editor.mouse_up(),
        MouseEventKind::Drag(MouseButton::Left) => {
            c.mouse_drag(x, y);
            if c.has_selection() {
                app.sel_owner = Some(sid);
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            let hit = c.mouse_up(x, y);
            if c.has_selection() {
                app.sel_owner = Some(sid.clone());
            }
            match hit {
                Some(Hit::Toggle { msg, part }) => {
                    if let Some(v) = app.views.get_mut(&sid) {
                        v.chat.toggle(&msg, part);
                    }
                }
                Some(Hit::ShowAll { msg, part }) => {
                    if let Some(v) = app.views.get_mut(&sid) {
                        v.chat.show_all(&msg, part);
                    }
                }
                Some(Hit::Copy { msg }) => app.copy_message(&sid, &msg),
                Some(Hit::Undo { msg }) => app.undo_message(&sid, &msg),
                Some(Hit::Unqueue { key }) => app.unqueue(&sid, key),
                None => {}
            }
        }
        MouseEventKind::Down(MouseButton::Right) => {
            if !app.copy_selection() {
                app.dirty = false;
            }
        }
        MouseEventKind::Moved => {
            let old = c.hover;
            let new = Some((x, y));
            let before = old.and_then(|(ox, oy)| c.hit_at(ox, oy));
            c.hover = new;
            let after = c.hit_at(x, y);
            if before == after {
                app.dirty = false;
            }
        }
        _ => app.dirty = false,
    }
}

/// Mouse over the autocomplete popup: click accepts, hover selects, wheel moves.
/// Returns true if the popup took the event.
fn autocomplete_mouse(app: &mut App, me: MouseEvent) -> bool {
    let Some(ac) = app.ac.as_mut() else {
        return false;
    };
    let (x, y) = (me.column, me.row);
    let r = ac.rect;
    if !r.contains(Position::new(x, y)) || y <= r.y {
        return false;
    }
    let idx = ac.item_at(x, y);
    match me.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            let Some(i) = idx else {
                return true;
            };
            ac.sel = i;
            ac.explicitly_selected = true;
            let keep_open = ac
                .items
                .get(i)
                .is_some_and(|(v, _, _)| ac_keeps_open_on_tab(Some(&ac.kind), v));
            let executed = app.accept_completion(true);
            if !executed {
                if keep_open {
                    refresh_autocomplete_after_accept(app);
                } else {
                    app.ac = None;
                }
            }
        }
        MouseEventKind::Moved => {
            if let Some(i) = idx {
                if ac.sel != i {
                    ac.sel = i;
                    ac.explicitly_selected = true;
                } else {
                    app.dirty = false;
                }
            } else {
                app.dirty = false;
            }
        }
        MouseEventKind::ScrollUp => ac_move(app, -1),
        MouseEventKind::ScrollDown => ac_move(app, 1),
        _ => app.dirty = false,
    }
    true
}

fn watch_mouse(app: &mut App, me: MouseEvent) {
    if autocomplete_mouse(app, me) {
        return;
    }
    let (x, y) = (me.column, me.row);
    let pos = Position::new(x, y);
    let inside = |r: &Rect| r.contains(pos);
    // Handle active drag/release in the focused tile's single-line input first.
    if let Some(f) = app.watch.focus.clone()
        && let Some(tr) = app
            .watch
            .tiles
            .iter()
            .find(|(s, _)| s == &f)
            .map(|(_, r)| *r)
        && let Some(ir) = split::input_rect(tr)
        && app.watch.inputs.get(&f).is_some_and(|e| e.dragging)
    {
        match me.kind {
            MouseEventKind::Drag(MouseButton::Left) => {
                let cursor_moved = if let Some(ed) = app.watch.inputs.get_mut(&f) {
                    let before = ed.cursor;
                    let off = ed.offset_at_single_line(ir, x);
                    ed.mouse_drag(off);
                    ed.cursor != before
                } else {
                    false
                };
                if cursor_moved && app.ac.is_some() {
                    app.update_autocomplete();
                }
                return;
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if let Some(ed) = app.watch.inputs.get_mut(&f) {
                    ed.mouse_up();
                }
                return;
            }
            _ => {}
        }
    }
    // Handle active drag/release of a tile's transcript text selection.
    if let Some(sid) = app.watch.drag_tile.clone() {
        match me.kind {
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(v) = app.views.get_mut(&sid) {
                    v.chat.mouse_drag(x, y);
                    if v.chat.has_selection() {
                        app.sel_owner = Some(sid);
                    }
                }
                return;
            }
            MouseEventKind::Up(MouseButton::Left) => {
                app.watch.drag_tile = None;
                if let Some(v) = app.views.get_mut(&sid) {
                    v.chat.mouse_up(x, y);
                    if v.chat.has_selection() {
                        app.sel_owner = Some(sid);
                    }
                }
                return;
            }
            _ => {}
        }
    }
    let tile = app.watch.tiles.iter().find(|(_, r)| inside(r)).cloned();
    match me.kind {
        // The wheel scrolls the tile under the pointer, focused or not.
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            let d = if me.kind == MouseEventKind::ScrollUp {
                -WHEEL_SCROLL_ROWS
            } else {
                WHEEL_SCROLL_ROWS
            };
            match tile.and_then(|(sid, _)| app.views.get_mut(&sid)) {
                Some(v) => v.chat.scroll_by(d, false),
                None => app.dirty = false,
            }
        }
        MouseEventKind::Down(MouseButton::Right) => {
            if !app.copy_selection() {
                app.dirty = false;
            }
        }
        MouseEventKind::Down(MouseButton::Left) => watch_mouse_down(app, tile, x, y),
        _ => app.dirty = false,
    }
}

fn click_watch_input(app: &mut App, sid: &str, tr: Rect, x: u16, y: u16) {
    app.clear_selection();
    let Some(ir) = split::input_rect(tr) else {
        return;
    };
    let n = click_count(app, x, y);
    let cursor_moved = {
        let ed = app.watch.inputs.entry(sid.to_string()).or_default();
        let before = ed.cursor;
        let off = ed.offset_at_single_line(ir, x);
        ed.mouse_down(off, n);
        ed.cursor != before
    };
    if cursor_moved && app.ac.is_some() {
        app.update_autocomplete();
    }
}

fn watch_mouse_down(app: &mut App, tile: Option<(String, Rect)>, x: u16, y: u16) {
    let Some((sid, tr)) = tile else {
        // Empty space / footer: clear selection and unfocus.
        app.clear_selection();
        app.set_watch_focus(None);
        return;
    };
    let already_focused = app.watch.focus.as_deref() == Some(sid.as_str());
    let in_focused_input = already_focused
        && tr.height >= 5
        && y == tr.y + tr.height - 2
        && x > tr.x
        && x + 1 < tr.x + tr.width;
    if in_focused_input {
        click_watch_input(app, &sid, tr, x, y);
        return;
    }
    if let Some(v) = app.views.get_mut(&sid)
        && chat::pill_hit(&v.chat, x, y)
    {
        v.chat.scroll_to_bottom(true);
        return;
    }
    // Click targets: expand a tool call, show all output, withdraw a queued prompt.
    let hit = app.views.get(&sid).and_then(|v| v.chat.hit_at(x, y));
    let is_tool = |app: &App, msg: &str, part: usize| {
        app.views
            .get(&sid)
            .and_then(|v| v.find(msg).and_then(|i| v.messages[i].parts.get(part)))
            .is_some_and(|p| matches!(p, ref_proto::Part::Tool { .. }))
    };
    let handled = match hit {
        Some(Hit::Toggle { msg, part }) if is_tool(app, &msg, part) => {
            if let Some(v) = app.views.get_mut(&sid) {
                v.chat.toggle(&msg, part);
            }
            true
        }
        Some(Hit::ShowAll { msg, part }) => {
            if let Some(v) = app.views.get_mut(&sid) {
                v.chat.show_all(&msg, part);
            }
            true
        }
        Some(Hit::Unqueue { key }) => {
            app.unqueue(&sid, key);
            true
        }
        _ => false,
    };
    if handled {
        app.clear_selection();
        app.set_watch_focus(Some(sid));
        return;
    }
    app.clear_selection();
    let clicks = click_count(app, x, y);
    if clicks == 2 {
        app.zoom(&sid);
        return;
    }
    let in_chat = app.views.get(&sid).is_some_and(|v| v.chat.contains(x, y));
    if in_chat {
        if let Some(f) = app.watch.focus.as_ref()
            && let Some(ed) = app.watch.inputs.get_mut(f)
        {
            ed.anchor = None;
        }
        if let Some(ed) = app.watch.inputs.get_mut(&sid) {
            ed.anchor = None;
        }
        app.set_watch_focus(Some(sid.clone()));
        if let Some(v) = app.views.get_mut(&sid) {
            v.chat.mouse_down(x, y, 1);
            app.watch.drag_tile = Some(sid);
        }
        return;
    }
    if already_focused {
        // Only clicking the tile border/header (or outside) unfocuses; body clicks keep focus.
        let on_border =
            x == tr.x || x + 1 == tr.x + tr.width || y == tr.y || y + 1 == tr.y + tr.height;
        if on_border {
            app.set_watch_focus(None);
        }
    } else {
        app.set_watch_focus(Some(sid));
    }
}

/// Updates autocomplete after accepting a completion that drills down or expects arguments.
fn refresh_autocomplete_after_accept(app: &mut App) {
    app.update_autocomplete();
}
