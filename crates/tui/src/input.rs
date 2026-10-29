//! Keyboard / mouse / paste routing.

use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::app::{AcKind, App, Mode};
use crate::render;
use crate::ui::dialog::{Dialog, Kind};
use crate::ui::editor::Editor;
use crate::ui::{Hit, chat, split, theme};

const CTRL: KeyModifiers = KeyModifiers::CONTROL;
const ALT: KeyModifiers = KeyModifiers::ALT;
const SHIFT: KeyModifiers = KeyModifiers::SHIFT;

/// Appends a line to `$REF_TUI_LOG` (debugging aid; no-op when unset).
fn log_path() -> Option<&'static std::path::PathBuf> {
    static PATH: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    PATH.get_or_init(|| std::env::var_os("REF_TUI_LOG").map(Into::into)).as_ref()
}

pub fn debug_log(s: &str) {
    use std::io::Write;
    if let Some(p) = log_path()
        && let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p)
    {
        let _ = writeln!(f, "{} {s}", ref_proto::now_ms());
    }
}

pub fn handle(app: &mut App, ev: Event) {
    if log_path().is_some() && !matches!(ev, Event::Mouse(MouseEvent { kind: MouseEventKind::Moved, .. })) {
        debug_log(&format!("input {ev:?} mode={:?}", app.mode));
    }
    app.dirty = true;
    match ev {
        Event::Key(k) if k.kind != KeyEventKind::Release => key(app, k),
        Event::Mouse(m) => mouse(app, m),
        Event::Paste(s) => paste(app, s),
        Event::Resize(..) => {}
        _ => {}
    }
}

fn is(k: &KeyEvent, code: KeyCode, mods: KeyModifiers) -> bool {
    k.code == code && k.modifiers == mods
}

fn ctrl_char(k: &KeyEvent, c: char) -> bool {
    k.modifiers.contains(CTRL) && !k.modifiers.contains(ALT) && matches!(k.code, KeyCode::Char(x) if x.eq_ignore_ascii_case(&c))
}

fn key(app: &mut App, k: KeyEvent) {
    // Copy selection: ctrl+c / ctrl+shift+c / y (when a transcript selection exists).
    if app.has_selection() && (ctrl_char(&k, 'c') || (is(&k, KeyCode::Char('y'), KeyModifiers::NONE) && app.editor.is_empty())) {
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
        if app.perm.as_ref().is_none_or(|p| p.opened.elapsed() < Duration::from_millis(350)) {
            if matches!(k.code, KeyCode::Char(_) | KeyCode::Backspace) && k.modifiers.difference(SHIFT).is_empty() {
                let ed = target_editor(app);
                if edit_key(ed, &k, false) {
                    app.history.reset();
                }
            }
            return;
        }
        match k.code {
            KeyCode::Char('a') | KeyCode::Enter => app.reply_permission(&sid, &req.call_id, true, false),
            KeyCode::Char('s') => app.reply_permission(&sid, &req.call_id, true, true),
            KeyCode::Char('d') | KeyCode::Esc => app.reply_permission(&sid, &req.call_id, false, false),
            KeyCode::Up | KeyCode::Char('k') => scroll_perm(app, -1),
            KeyCode::Down | KeyCode::Char('j') => scroll_perm(app, 1),
            KeyCode::PageUp => scroll_perm(app, -10),
            KeyCode::PageDown => scroll_perm(app, 10),
            _ => {}
        }
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

fn exit_or_arm(app: &mut App) {
    if app.ctrlc_armed.is_some_and(|t| t.elapsed() < Duration::from_millis(1500)) {
        app.quit = true;
    } else {
        app.ctrlc_armed = Some(Instant::now());
        app.set_hint("ctrl+c again to exit", theme::WARNING);
    }
}

fn leader(app: &mut App, k: KeyEvent) {
    let KeyCode::Char(c) = k.code else {
        return;
    };
    if c.eq_ignore_ascii_case(&'k') {
        app.close_key();
        return;
    }
    let cmd = match c.to_ascii_lowercase() {
        'n' => "new",
        'l' => "sessions",
        'm' => "models",
        'a' => "agents",
        't' => "thinking",
        'd' => "details",
        'c' => "compact",
        'u' => "undo",
        'e' => "editor",
        'x' => "export",
        'y' => "copy",
        'w' => "watch",
        'b' => "sidebar",
        'h' | '?' => "help",
        'q' => "exit",
        _ => return,
    };
    app.run_command(cmd, "");
}

fn dialog_key(app: &mut App, k: KeyEvent) {
    match app.dialog.as_mut() {
        Some(Dialog::Help(h)) => match k.code {
            KeyCode::Up | KeyCode::Char('k') => h.scroll_by(-1),
            KeyCode::Down | KeyCode::Char('j') => h.scroll_by(1),
            KeyCode::PageUp => h.scroll_by(-10),
            KeyCode::PageDown | KeyCode::Char(' ') => h.scroll_by(10),
            _ => app.dialog = None,
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
                KeyCode::PageUp => d.scroll_by(-10),
                KeyCode::PageDown => d.scroll_by(10),
                KeyCode::Char('r') if k.modifiers.is_empty() => app.refresh_mcp(),
                KeyCode::Esc | KeyCode::Char('q') => app.dialog = None,
                KeyCode::Char('c') if k.modifiers == CTRL => app.dialog = None,
                _ => app.dirty = false,
            }
        }
        Some(Dialog::List(d)) => {
            match k.code {
                KeyCode::Esc => {
                    if d.confirm.is_some() {
                        d.confirm = None;
                    } else {
                        app.dialog = None;
                    }
                }
                KeyCode::Up => d.move_sel(-1),
                KeyCode::Down => d.move_sel(1),
                KeyCode::Char('p') if k.modifiers == CTRL => d.move_sel(-1),
                KeyCode::Char('n') if k.modifiers == CTRL => d.move_sel(1),
                KeyCode::PageUp => d.page(-1),
                KeyCode::PageDown => d.page(1),
                KeyCode::Enter => app.dialog_submit(),
                KeyCode::Char('d') if k.modifiers == CTRL => {
                    if matches!(d.kind, Kind::Sessions | Kind::CloseSessions) {
                        app.dialog_delete();
                    }
                }
                KeyCode::Char('c') if k.modifiers == CTRL => {
                    if d.filter.text.is_empty() {
                        app.dialog = None;
                    } else {
                        d.filter.clear();
                        d.refilter();
                    }
                }
                _ => {
                    if edit_key(&mut d.filter, &k, false) {
                        d.refilter();
                    }
                }
            }
        }
        None => {}
    }
}

/// Generic editing keys. Returns true if the text changed (or cursor moved).
pub fn edit_key(e: &mut Editor, k: &KeyEvent, multiline: bool) -> bool {
    let m = k.modifiers;
    let shift = m.contains(SHIFT);
    match k.code {
        KeyCode::Char(c) if m.contains(CTRL) && !m.contains(ALT) => match c.to_ascii_lowercase() {
            'a' => {
                e.select_mode(false);
                e.home()
            }
            'e' => {
                e.select_mode(false);
                e.end()
            }
            'b' => {
                e.select_mode(false);
                e.left()
            }
            'f' => {
                e.select_mode(false);
                e.right()
            }
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
            e.select_mode(shift);
            if m.intersects(CTRL | ALT) { e.word_left() } else { e.left() }
        }
        KeyCode::Right => {
            e.select_mode(shift);
            if m.intersects(CTRL | ALT) { e.word_right() } else { e.right() }
        }
        KeyCode::Home => {
            e.select_mode(shift);
            if m.contains(CTRL) { e.buf_start() } else { e.home() }
        }
        KeyCode::End => {
            e.select_mode(shift);
            if m.contains(CTRL) { e.buf_end() } else { e.end() }
        }
        KeyCode::Enter if multiline && m.intersects(SHIFT | ALT | CTRL) => e.newline(),
        _ => return false,
    }
    true
}

fn chat_key(app: &mut App, k: KeyEvent) {
    let m = k.modifiers;
    // ctrl+c: clear editor / exit twice.
    if ctrl_char(&k, 'c') {
        if let Some(t) = app.editor.selected_text() {
            app.copy_text(&t, "selection");
            app.editor.select_mode(false);
        } else if !app.editor.text.is_empty() || app.shell_mode {
            if app.editor.text.trim().chars().count() >= 20 {
                let t = app.editor.text.clone();
                app.history.push(&t);
            }
            app.editor.clear();
            app.shell_mode = false;
            app.ac = None;
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
    if app.ac.is_some() {
        match k.code {
            KeyCode::Up => return ac_move(app, -1),
            KeyCode::Down => return ac_move(app, 1),
            KeyCode::Char('p') if m == CTRL => return ac_move(app, -1),
            KeyCode::Char('n') if m == CTRL => return ac_move(app, 1),
            KeyCode::Tab => {
                app.accept_completion(false);
                app.update_autocomplete_after_accept();
                return;
            }
            KeyCode::Enter if m.is_empty() => {
                let is_slash = app.ac.as_ref().is_some_and(|a| a.kind == AcKind::Slash);
                let ran = app.accept_completion(true);
                if !ran && is_slash {
                    // Command with args: completed text stays for the user to type args.
                }
                app.update_autocomplete_after_accept();
                return;
            }
            KeyCode::Esc => {
                app.ac = None;
                app.suppress_ac = true;
                return;
            }
            _ => {}
        }
    }
    match k.code {
        KeyCode::Esc => {
            if app.has_selection() {
                app.clear_selection();
            } else if app.editor.selection().is_some() {
                app.editor.select_mode(false);
            } else if app.shell_mode {
                app.shell_mode = false;
            } else if app.cur().is_some_and(has_delayed_tool) {
                // "runs in Ns · esc to cancel": a single esc stops the turn before the tool runs.
                if let Some(sid) = app.current.clone() {
                    app.abort(&sid);
                    app.toasts.info("Cancelled delayed tool call");
                }
            } else if app.cur().is_some_and(|v| v.is_busy()) {
                if app.esc_armed.is_some_and(|t| t.elapsed() < Duration::from_millis(1500)) {
                    app.esc_armed = None;
                    app.hint = None;
                    if let Some(sid) = app.current.clone() {
                        app.abort(&sid);
                    }
                } else {
                    app.esc_armed = Some(Instant::now());
                    app.set_hint("esc again to interrupt", theme::WARNING);
                }
            }
        }
        KeyCode::Enter if m.is_empty() => app.submit(),
        KeyCode::Tab if m.is_empty() => app.cycle_agent(1),
        KeyCode::BackTab => app.cycle_agent(-1),
        KeyCode::PageUp => scroll(app, |c| c.page(-1, false)),
        KeyCode::PageDown => scroll(app, |c| c.page(1, false)),
        KeyCode::Char('u') if m == CTRL | ALT => scroll(app, |c| c.page(-1, true)),
        KeyCode::Char('d') if m == CTRL | ALT => scroll(app, |c| c.page(1, true)),
        KeyCode::Char('y') if m == CTRL | ALT => scroll(app, |c| c.scroll_by(-1, false)),
        KeyCode::Char('e') if m == CTRL | ALT => scroll(app, |c| c.scroll_by(1, false)),
        KeyCode::Char('g') if m == CTRL => scroll(app, |c| c.scroll_to_top()),
        KeyCode::Char('g') if m == CTRL | ALT => scroll(app, |c| c.scroll_to_bottom(true)),
        KeyCode::Up if m == SHIFT && app.editor.selection().is_none() && app.editor.height(app.editor.last_width.max(1)) <= 1 => {
            scroll(app, |c| c.scroll_by(-1, false))
        }
        KeyCode::Down if m == SHIFT && app.editor.selection().is_none() && app.editor.height(app.editor.last_width.max(1)) <= 1 => {
            scroll(app, |c| c.scroll_by(1, false))
        }
        KeyCode::Home if app.editor.is_empty() => scroll(app, |c| c.scroll_to_top()),
        KeyCode::End if app.editor.is_empty() => scroll(app, |c| c.scroll_to_bottom(true)),
        KeyCode::Up if !m.contains(SHIFT) => {
            if app.editor.on_first_row() {
                if app.editor.text.is_empty() || app.history.browsing() || app.editor.cursor == 0 {
                    let cur = app.editor.text.clone();
                    if let Some(h) = app.history.prev(&cur) {
                        set_from_history(app, &h);
                    }
                } else {
                    app.editor.buf_start();
                }
            } else {
                app.editor.select_mode(false);
                app.editor.up();
            }
        }
        KeyCode::Down if !m.contains(SHIFT) => {
            if app.editor.on_last_row() {
                if app.history.browsing() {
                    if let Some(h) = app.history.next() {
                        set_from_history(app, &h);
                    }
                } else {
                    app.editor.buf_end();
                }
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
        KeyCode::Char('!') if app.editor.text.is_empty() && !app.shell_mode && m.difference(SHIFT).is_empty() => {
            app.shell_mode = true;
            app.ac = None;
        }
        KeyCode::Backspace if app.shell_mode && app.editor.text.is_empty() => {
            app.shell_mode = false;
        }
        _ => {
            let before = app.editor.text.clone();
            if edit_key(&mut app.editor, &k, true) {
                if app.editor.text != before {
                    app.history.reset();
                    app.suppress_ac = false;
                    app.update_autocomplete();
                    // Typed / pasted path to an image → attachment.
                    maybe_image_path(app);
                }
                if app.has_selection() {
                    app.clear_selection();
                }
            }
        }
    }
}

fn has_delayed_tool(v: &crate::state::SessionView) -> bool {
    v.messages.last().is_some_and(|m| m.parts.iter().any(|p| matches!(p, ref_proto::Part::Tool { state: ref_proto::ToolState::Delayed, .. })))
}

fn set_from_history(app: &mut App, h: &str) {
    if let Some(rest) = h.strip_prefix('!') {
        app.shell_mode = true;
        app.editor.set_text(rest);
    } else {
        app.shell_mode = false;
        app.editor.set_text(h);
    }
    app.ac = None;
}

fn ac_move(app: &mut App, d: i32) {
    if let Some(ac) = app.ac.as_mut() {
        let n = ac.items.len() as i32;
        if n > 0 {
            ac.sel = ((ac.sel as i32 + d).rem_euclid(n)) as usize;
        }
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
    if t.len() < 5 || !t.contains('.') || t.contains('\n') {
        return;
    }
    if let Some((img, name)) = crate::clipboard::image_from_path(t) {
        app.editor.clear();
        app.editor.attach_image(img, Some(name.clone()));
        app.toasts.info(format!("Attached image {name}"));
    }
}

fn clipboard_paste(app: &mut App) {
    if let Some(img) = crate::clipboard::paste_image() {
        let target = target_editor(app);
        target.attach_image(img, Some("clipboard".into()));
        app.toasts.info("Pasted image from clipboard");
        return;
    }
    match crate::clipboard::paste_text() {
        Some(t) if !t.is_empty() => paste(app, t),
        _ => app.toasts.warn("Clipboard is empty"),
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

fn paste(app: &mut App, s: String) {
    if let Some(Dialog::List(d)) = app.dialog.as_mut() {
        d.filter.insert(&crate::ui::text::one_line(&s));
        d.refilter();
        return;
    }
    if app.active_permission().is_some() {
        return;
    }
    if s.trim().is_empty() {
        // Empty bracketed paste: often an image-only clipboard.
        if let Some(img) = crate::clipboard::paste_image() {
            target_editor(app).attach_image(img, Some("clipboard".into()));
            app.toasts.info("Pasted image from clipboard");
        }
        return;
    }
    // One or more image paths.
    let paths: Vec<&str> = s.split_whitespace().collect();
    if !paths.is_empty() && paths.len() <= 32 {
        let imgs: Vec<_> = paths.iter().filter_map(|p| crate::clipboard::image_from_path(p)).collect();
        if imgs.len() == paths.len() || (imgs.len() == 1 && crate::clipboard::image_from_path(s.trim()).is_some()) {
            let imgs = if imgs.len() == paths.len() { imgs } else { crate::clipboard::image_from_path(s.trim()).into_iter().collect() };
            let n = imgs.len();
            let ed = target_editor(app);
            for (img, name) in imgs {
                ed.attach_image(img, Some(name));
            }
            app.toasts.info(format!("Attached {n} image{}", if n > 1 { "s" } else { "" }));
            return;
        }
    }
    if app.mode == Mode::Watch {
        let one = crate::ui::text::one_line(&s);
        target_editor(app).insert(&one);
        return;
    }
    app.editor.paste(&s);
    app.history.reset();
    app.update_autocomplete();
}

// ---------------------------------------------------------------------------
// Watch mode
// ---------------------------------------------------------------------------

fn watch_key(app: &mut App, k: KeyEvent) {
    let m = k.modifiers;
    let tiles: Vec<String> = app.watch.tiles.iter().map(|t| t.0.clone()).collect();
    let all = app.tile_ids();
    let focus_idx = app.watch.focus.as_ref().and_then(|f| tiles.iter().position(|t| t == f));
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
            app.watch.focus = Some(tiles[i].clone());
        } else if focus_idx.is_none() && !tiles.is_empty() {
            app.watch.focus = Some(tiles[0].clone());
        }
        return;
    }
    if ctrl_char(&k, 'c') {
        let empty = app.watch.focus.as_ref().and_then(|f| app.watch.inputs.get(f)).is_none_or(|e| e.is_empty());
        if empty {
            exit_or_arm(app);
        } else if let Some(f) = app.watch.focus.clone() {
            app.watch.inputs.entry(f).or_default().clear();
        }
        return;
    }
    if ctrl_char(&k, 'd') {
        let empty = app.watch.focus.as_ref().and_then(|f| app.watch.inputs.get(f)).is_none_or(|e| e.is_empty());
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
        let p = if ctrl_char(&k, 'n') { (app.watch.page + 1) % pages } else { (app.watch.page + pages - 1) % pages };
        app.watch.page = p;
        app.watch.focus = all.get(p * per).cloned();
        return;
    }
    if ctrl_char(&k, 'o') {
        if let Some(f) = app.watch.focus.clone() {
            app.zoom(&f);
        }
        return;
    }
    match k.code {
        KeyCode::Tab | KeyCode::BackTab => {
            if all.is_empty() {
                return;
            }
            let d = if k.code == KeyCode::BackTab { -1 } else { 1 };
            let i = app.watch.focus.as_ref().and_then(|f| all.iter().position(|t| t == f)).map(|i| i as i32).unwrap_or(-1);
            let n = all.len() as i32;
            app.watch.focus = Some(all[((i + d).rem_euclid(n)) as usize].clone());
        }
        KeyCode::Esc => {
            let Some(f) = app.watch.focus.clone() else { return };
            let busy = app.views.get(&f).is_some_and(|v| v.is_busy());
            if busy {
                if app.esc_armed.is_some_and(|t| t.elapsed() < Duration::from_millis(1500)) {
                    app.esc_armed = None;
                    app.hint = None;
                    app.abort(&f);
                    let name = app.views.get(&f).map(|v| v.info.name.clone()).unwrap_or_default();
                    app.toasts.info(format!("Interrupted \"{name}\""));
                } else {
                    app.esc_armed = Some(Instant::now());
                    let name = app.views.get(&f).map(|v| v.info.name.clone()).unwrap_or_default();
                    app.set_hint(format!("esc again to interrupt \"{name}\""), theme::WARNING);
                }
            } else if let Some(e) = app.watch.inputs.get_mut(&f) {
                e.clear();
            }
        }
        KeyCode::Enter if m.is_empty() => {
            let Some(f) = app.watch.focus.clone() else { return };
            let ed = app.watch.inputs.entry(f.clone()).or_default();
            if ed.is_empty() {
                app.zoom(&f);
                return;
            }
            let sub = ed.submit();
            let images = sub.images.into_iter().map(|(i, _)| i).collect();
            app.send_prompt(&f, sub.text, images);
        }
        _ => {
            if let Some(f) = app.watch.focus.clone() {
                let ed = app.watch.inputs.entry(f).or_default();
                edit_key(ed, &k, false);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Mouse
// ---------------------------------------------------------------------------

fn click_count(app: &mut App, x: u16, y: u16) -> u8 {
    let n = match app.last_click {
        Some((t, lx, ly, n)) if t.elapsed() < Duration::from_millis(450) && lx == x && ly == y => (n % 3) + 1,
        _ => 1,
    };
    app.last_click = Some((Instant::now(), x, y, n));
    n
}

fn mouse(app: &mut App, me: MouseEvent) {
    let (x, y) = (me.column, me.row);
    // Permission modal.
    if let Some((sid, req, _)) = app.active_permission() {
        match me.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(a) = app.perm.as_ref().and_then(|p| p.button_at(x, y)) {
                    match a {
                        0 => app.reply_permission(&sid, &req.call_id, true, false),
                        1 => app.reply_permission(&sid, &req.call_id, true, true),
                        _ => app.reply_permission(&sid, &req.call_id, false, false),
                    }
                }
            }
            MouseEventKind::ScrollUp => scroll_perm(app, -3),
            MouseEventKind::ScrollDown => scroll_perm(app, 3),
            _ => app.dirty = false,
        }
        return;
    }
    if let Some(d) = app.dialog.as_mut() {
        match (d, me.kind) {
            (Dialog::List(d), MouseEventKind::Down(MouseButton::Left)) => {
                let r = d.rect;
                let inside = x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height;
                if !inside {
                    app.dialog = None;
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
                    d.sel = i;
                } else {
                    app.dirty = false;
                }
            }
            (Dialog::Help(h), MouseEventKind::ScrollUp) => h.scroll_by(-3),
            (Dialog::Help(h), MouseEventKind::ScrollDown) => h.scroll_by(3),
            (Dialog::Help(_), MouseEventKind::Down(_)) => app.dialog = None,
            (Dialog::Mcp(d), MouseEventKind::Down(MouseButton::Left)) => {
                if !d.contains(x, y) {
                    app.dialog = None;
                } else if let Some(i) = d.server_at(x, y) {
                    d.sel = i;
                    d.set_expanded(&app.mcp, None);
                }
            }
            (Dialog::Mcp(d), MouseEventKind::ScrollUp) => d.scroll_by(-3),
            (Dialog::Mcp(d), MouseEventKind::ScrollDown) => d.scroll_by(3),
            _ => app.dirty = false,
        }
        return;
    }
    if app.mode == Mode::Watch {
        if let MouseEventKind::Down(MouseButton::Left) = me.kind {
            if let Some((sid, _)) = app.watch.tiles.iter().find(|(_, r)| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height) {
                let sid = sid.clone();
                if app.watch.focus.as_deref() == Some(sid.as_str()) && click_count(app, x, y) == 2 {
                    app.zoom(&sid);
                } else {
                    app.watch.focus = Some(sid);
                    let _ = click_count(app, x, y);
                }
            }
        } else {
            app.dirty = false;
        }
        return;
    }
    // Chat mode.
    let Some(sid) = app.current.clone() else { return };
    let lay = render::chat_layout(app, app.screen);
    // Autocomplete popup.
    if let Some(ac) = app.ac.as_mut() {
        let r = ac.rect;
        if x >= r.x && x < r.x + r.width && y > r.y && y < r.y + r.height {
            let n = ac.items.len().min(r.height as usize - 2);
            let start = ac.sel.saturating_sub(n.saturating_sub(1));
            let idx = start + (y - r.y - 1) as usize;
            match me.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    if idx < ac.items.len() {
                        ac.sel = idx;
                        app.accept_completion(true);
                        app.update_autocomplete_after_accept();
                    }
                }
                MouseEventKind::Moved => {
                    if idx < ac.items.len() {
                        ac.sel = idx;
                    }
                }
                MouseEventKind::ScrollUp => ac_move(app, -1),
                MouseEventKind::ScrollDown => ac_move(app, 1),
                _ => {}
            }
            return;
        }
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
        } else if let Some((_, name)) = app.sidebar_mcp_rows.iter().find(|(ry, _)| *ry == y).cloned() {
            app.open_mcp(Some(&name));
        }
        return;
    }
    let Some(v) = app.views.get_mut(&sid) else { return };
    let c = &mut v.chat;
    match me.kind {
        MouseEventKind::ScrollUp => {
            if c.contains(x, y) || y < lay.prompt.y {
                c.scroll_by(-3, false);
            }
        }
        MouseEventKind::ScrollDown => {
            if c.contains(x, y) || y < lay.prompt.y {
                c.scroll_by(3, false);
            }
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if chat::pill_hit(c, x, y) {
                c.scroll_to_bottom(true);
                return;
            }
            if c.contains(x, y) {
                // Repeated clicks on a button (e.g. confirming undo) are clicks, not word selection.
                let on_button = c.hit_at(x, y).is_some_and(|h| h.is_button());
                let n = click_count(app, x, y);
                let n = if on_button { 1 } else { n };
                let Some(v) = app.views.get_mut(&sid) else { return };
                v.chat.mouse_down(x, y, n);
                if v.chat.has_selection() {
                    app.sel_owner = Some(sid);
                }
            } else if y >= lay.prompt.y && y < lay.prompt.y + lay.prompt.height {
                app.clear_selection();
                // Click in the editor: move the cursor.
                let e = lay.editor;
                if y >= e.y && y < e.y + e.height {
                    place_cursor(&mut app.editor, e, x, y);
                }
            }
        }
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

fn place_cursor(e: &mut Editor, area: ratatui::layout::Rect, x: u16, y: u16) {
    use unicode_segmentation::UnicodeSegmentation;
    let rows = e.layout(area.width as usize);
    let ri = e.scroll + (y - area.y) as usize;
    let Some(r) = rows.get(ri.min(rows.len().saturating_sub(1))).copied() else { return };
    let want = x.saturating_sub(area.x) as usize;
    let mut col = 0;
    let mut pos = r.start;
    for (i, g) in e.text[r.start..r.end].grapheme_indices(true) {
        let w = crate::ui::text::gwidth(g);
        if g == "\n" || col + w > want {
            break;
        }
        col += w;
        pos = r.start + i + g.len();
    }
    e.anchor = None;
    e.cursor = pos;
}

impl App {
    pub fn update_autocomplete_after_accept(&mut self) {
        // After accepting a slash command needing args, or a file, close the popup.
        if self.ac.is_none() && self.editor.mention().is_none() && !self.editor.text.starts_with('/') {
            return;
        }
        if self.editor.text.ends_with(' ') {
            self.ac = None;
        } else {
            self.update_autocomplete();
        }
    }
}

