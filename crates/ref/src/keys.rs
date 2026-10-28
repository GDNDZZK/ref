//! Keyboard, mouse and paste handling.

use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ref_proto::*;

use crate::app::{App, Drag, Mode};
use crate::ui::chat::{content_col, Action, Sel};
use crate::ui::dialogs::DialogKind;
use crate::ui::layout::contains;

fn ctrl(k: &KeyEvent) -> bool {
    k.modifiers.contains(KeyModifiers::CONTROL)
}
fn alt(k: &KeyEvent) -> bool {
    k.modifiers.contains(KeyModifiers::ALT)
}
fn shift(k: &KeyEvent) -> bool {
    k.modifiers.contains(KeyModifiers::SHIFT)
}

impl App {
    pub fn on_event(&mut self, ev: Event) {
        match ev {
            Event::Key(k) if k.kind != KeyEventKind::Release => self.on_key(k),
            Event::Paste(s) => self.on_paste(s),
            Event::Mouse(m) => self.on_mouse(m),
            _ => {}
        }
    }

    fn on_paste(&mut self, s: String) {
        if let Some(d) = self.dialog.as_mut() {
            let f = format!("{}{}", d.filter, s.replace('\n', " "));
            d.set_filter(f);
            return;
        }
        if s.is_empty() {
            // Terminals send an empty paste when the clipboard holds an image.
            self.request_clipboard_image(false);
            return;
        }
        self.paste_text(&s);
        self.update_popup();
    }

    fn on_key(&mut self, k: KeyEvent) {
        // ── leader sequences ──
        if self.leader.take().is_some() {
            self.on_leader(k);
            return;
        }
        if ctrl(&k) && k.code == KeyCode::Char('x') && !matches!(self.dialog.as_ref().map(|d| &d.kind), Some(DialogKind::Sessions)) {
            self.leader = Some(Instant::now());
            return;
        }
        if self.dialog.is_some() {
            self.on_dialog_key(k);
            return;
        }
        if self.pending_permission().is_some() && self.on_permission_key(&k) {
            return;
        }
        if self.mode == Mode::Normal && self.on_msg_select_key(&k) {
            return;
        }
        // ── selection copy ──
        if self.selection_session().is_some() {
            let is_copy = (ctrl(&k) && matches!(k.code, KeyCode::Char('c') | KeyCode::Char('C')))
                || (k.code == KeyCode::Char('y') && k.modifiers.is_empty() && self.target_editor_ref_empty());
            if is_copy {
                self.copy_selection();
                return;
            }
            if k.code == KeyCode::Esc {
                self.clear_selection();
                return;
            }
        }
        // ── slash popup ──
        if self.popup.is_some() {
            match k.code {
                KeyCode::Up => {
                    let p = self.popup.as_mut().unwrap();
                    p.selected = (p.selected + p.items.len() - 1) % p.items.len();
                    return;
                }
                KeyCode::Down => {
                    let p = self.popup.as_mut().unwrap();
                    p.selected = (p.selected + 1) % p.items.len();
                    return;
                }
                KeyCode::Char('p') | KeyCode::Char('n') if ctrl(&k) => {
                    let p = self.popup.as_mut().unwrap();
                    let n = p.items.len();
                    p.selected = if k.code == KeyCode::Char('p') { (p.selected + n - 1) % n } else { (p.selected + 1) % n };
                    return;
                }
                KeyCode::Tab => {
                    self.popup_accept(false);
                    return;
                }
                KeyCode::Enter if k.modifiers.is_empty() => {
                    self.popup_accept(true);
                    return;
                }
                KeyCode::Esc => {
                    self.popup = None;
                    self.popup_dismissed = true;
                    return;
                }
                _ => {}
            }
        }
        if self.on_global_key(&k) {
            return;
        }
        if self.mode == Mode::Watch {
            self.on_watch_key(k);
        } else {
            self.on_input_key(k);
        }
    }

    fn target_editor_ref_empty(&self) -> bool {
        match (&self.mode, &self.watch.focus) {
            (Mode::Watch, Some(f)) => self.pane_editors.get(f).map(|e| e.is_empty()).unwrap_or(true),
            _ => self.editor.is_empty(),
        }
    }

    fn on_leader(&mut self, k: KeyEvent) {
        let c = match k.code {
            KeyCode::Char(c) => Some(c),
            _ => None,
        };
        match (k.code, c) {
            (_, Some('n')) => self.run_command("new", ""),
            (_, Some('l')) => self.run_command("sessions", ""),
            (_, Some('m')) => self.run_command("models", ""),
            (_, Some('a')) => self.run_command("agents", ""),
            (_, Some('c')) => self.run_command("compact", ""),
            (_, Some('u')) => self.run_command("undo", ""),
            (_, Some('e')) => self.run_command("editor", ""),
            (_, Some('b')) => self.run_command("sidebar", ""),
            (_, Some('y')) => self.copy_last_assistant(),
            (_, Some('d')) => self.run_command("details", ""),
            (_, Some('w')) => self.run_command("watch", ""),
            (_, Some('s')) => self.run_command("mouse", ""),
            (_, Some('q')) => self.quit = true,
            (_, Some('o')) => self.focus_cycle(1),
            (_, Some('z')) => self.watch.zoom = !self.watch.zoom,
            (_, Some('[')) => self.watch.page = self.watch.page.saturating_sub(1),
            (_, Some(']')) => self.watch.page = (self.watch.page + 1).min(self.watch.pages.saturating_sub(1)),
            (_, Some('h')) => self.run_command("help", ""),
            (KeyCode::Enter, _) if self.mode == Mode::Watch => {
                if let Some(f) = self.watch.focus.clone() {
                    self.attach(&f);
                    self.sync_watch();
                }
            }
            (KeyCode::Up, _) => self.enter_msg_select(),
            (KeyCode::Esc, _) => {}
            _ => {}
        }
    }

    fn on_dialog_key(&mut self, k: KeyEvent) {
        let Some(d) = self.dialog.as_mut() else { return };
        if d.kind == DialogKind::Help {
            match k.code {
                KeyCode::Up | KeyCode::Char('k') => d.scroll = d.scroll.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => d.scroll += 1,
                KeyCode::PageUp => d.scroll = d.scroll.saturating_sub(10),
                KeyCode::PageDown => d.scroll += 10,
                _ => self.dialog = None,
            }
            return;
        }
        match k.code {
            KeyCode::Esc => self.dialog = None,
            KeyCode::Char('c') if ctrl(&k) => self.dialog = None,
            KeyCode::Up => d.move_sel(-1),
            KeyCode::Down => d.move_sel(1),
            KeyCode::Char('p') if ctrl(&k) => d.move_sel(-1),
            KeyCode::Char('n') if ctrl(&k) => d.move_sel(1),
            KeyCode::PageUp => d.page(-1),
            KeyCode::PageDown => d.page(1),
            KeyCode::Enter => self.dialog_activate(),
            KeyCode::Char('d') if ctrl(&k) && d.kind == DialogKind::Sessions => {
                if let Some(it) = d.selected_item().cloned() {
                    let info = self.store.list.iter().find(|s| s.id == it.id).cloned();
                    match info {
                        Some(i) if i.open => self.toast(NoticeLevel::Warning, "Close the session first (ctrl+x), then delete"),
                        Some(_) => {
                            self.send(ClientMsg::Delete { session_id: it.id.clone() });
                            self.store.list.retain(|s| s.id != it.id);
                            self.refresh_dialog();
                            self.toast(NoticeLevel::Info, format!("Deleted \"{}\"", it.label));
                        }
                        None => {}
                    }
                }
            }
            KeyCode::Char('x') if ctrl(&k) && d.kind == DialogKind::Sessions => {
                if let Some(it) = d.selected_item().cloned() {
                    if self.store.list.iter().any(|s| s.id == it.id && s.open) {
                        self.send(ClientMsg::Close { session_id: it.id.clone() });
                        self.toast(NoticeLevel::Info, format!("Closed \"{}\"", it.label));
                        if self.current.as_deref() == Some(&it.id) {
                            self.current = None;
                            self.store.sessions.remove(&it.id);
                        }
                    }
                }
            }
            KeyCode::Tab if d.kind == DialogKind::Agents => d.move_sel(1),
            KeyCode::Backspace => {
                let mut f = d.filter.clone();
                f.pop();
                d.set_filter(f);
            }
            KeyCode::Char('u') if ctrl(&k) => d.set_filter(String::new()),
            KeyCode::Char(c) if !ctrl(&k) && !alt(&k) => {
                let f = format!("{}{}", d.filter, c);
                d.set_filter(f);
            }
            _ => {}
        }
    }

    /// Returns true if the key was consumed by the permission prompt.
    fn on_permission_key(&mut self, k: &KeyEvent) -> bool {
        if ctrl(k) || alt(k) {
            return false;
        }
        let decisions = [PermissionDecision::Once, PermissionDecision::Always, PermissionDecision::Deny];
        if matches!(k.code, KeyCode::Char(_) | KeyCode::Enter) && self.perm_guard() {
            return true;
        }
        match k.code {
            KeyCode::Char('a') | KeyCode::Char('y') => self.decide(PermissionDecision::Once),
            KeyCode::Char('A') => self.decide(PermissionDecision::Always),
            KeyCode::Char('d') | KeyCode::Char('n') => self.decide(PermissionDecision::Deny),
            KeyCode::Left => self.perm_sel = (self.perm_sel + 2) % 3,
            KeyCode::Right | KeyCode::Tab => self.perm_sel = (self.perm_sel + 1) % 3,
            KeyCode::Enter => self.decide(decisions[self.perm_sel.min(2)]),
            KeyCode::Up => self.perm_scroll = self.perm_scroll.saturating_sub(1),
            KeyCode::Down => self.perm_scroll += 1,
            KeyCode::PageUp => self.perm_scroll = self.perm_scroll.saturating_sub(10),
            KeyCode::PageDown => self.perm_scroll += 10,
            // The prompt is modal: swallow other printable keys so typing can't leak into a decision.
            KeyCode::Char(_) => {}
            _ => return false,
        }
        true
    }

    pub fn enter_msg_select(&mut self) {
        if self.mode != Mode::Normal {
            return;
        }
        let Some(sid) = self.current.clone() else { return };
        let n = self.store.get(&sid).map(|v| v.messages.len()).unwrap_or(0);
        if n == 0 {
            return;
        }
        let v = self.view_mut(&sid);
        v.msg_select = Some(n - 1);
        v.ensure_visible(n - 1);
    }

    fn on_msg_select_key(&mut self, k: &KeyEvent) -> bool {
        let Some(sid) = self.current.clone() else { return false };
        let Some(i) = self.views.get(&sid).and_then(|v| v.msg_select) else { return false };
        let n = self.store.get(&sid).map(|v| v.messages.len()).unwrap_or(0);
        if n == 0 {
            self.view_mut(&sid).msg_select = None;
            return false;
        }
        let i = i.min(n - 1);
        match k.code {
            KeyCode::Up | KeyCode::Char('k') => {
                let v = self.view_mut(&sid);
                v.msg_select = Some(i.saturating_sub(1));
                v.ensure_visible(i.saturating_sub(1));
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let v = self.view_mut(&sid);
                v.msg_select = Some((i + 1).min(n - 1));
                v.ensure_visible((i + 1).min(n - 1));
            }
            KeyCode::Home | KeyCode::Char('g') => {
                let v = self.view_mut(&sid);
                v.msg_select = Some(0);
                v.ensure_visible(0);
            }
            KeyCode::End | KeyCode::Char('G') => {
                let v = self.view_mut(&sid);
                v.msg_select = Some(n - 1);
                v.ensure_visible(n - 1);
            }
            KeyCode::Char('c') if ctrl(k) => self.view_mut(&sid).msg_select = None,
            KeyCode::Char('y') | KeyCode::Char('c') => {
                let mid = self.store.get(&sid).map(|v| v.messages[i].msg.id.clone()).unwrap_or_default();
                self.copy_message(&sid, &mid);
            }
            KeyCode::Enter | KeyCode::Char(' ') => self.toggle_message(&sid, i),
            KeyCode::Esc | KeyCode::Char('q') => self.view_mut(&sid).msg_select = None,
            _ => {
                // Any other key leaves select mode and is handled normally.
                self.view_mut(&sid).msg_select = None;
                return false;
            }
        }
        true
    }

    /// Expand/collapse all collapsible parts of message `i`.
    fn toggle_message(&mut self, sid: &str, i: usize) {
        let Some(m) = self.store.get(sid).map(|v| v.messages[i].msg.clone()) else { return };
        let (details, thinking) = (self.details, self.thinking);
        let v = self.view_mut(sid);
        if m.origin == MessageOrigin::Compaction {
            v.toggle(&m.id);
            return;
        }
        let ids: Vec<(String, bool)> = m
            .parts
            .iter()
            .filter_map(|p| match p {
                Part::Tool(t) => Some((t.id.clone(), details)),
                Part::Reasoning(r) => Some((r.id.clone(), thinking)),
                _ => None,
            })
            .collect();
        let any_collapsed = ids.iter().any(|(id, d)| !v.is_expanded(id, *d));
        for (id, d) in ids {
            if v.is_expanded(&id, d) != any_collapsed {
                v.toggle(&id);
            }
        }
    }

    /// Keys valid in both modes. Returns true if consumed.
    fn on_global_key(&mut self, k: &KeyEvent) -> bool {
        let target = self.target_session();
        match k.code {
            KeyCode::Char('p') if ctrl(k) && !alt(k) => self.open_palette(),
            KeyCode::Char('c') | KeyCode::Char('C') if ctrl(k) => {
                if shift(k) || k.code == KeyCode::Char('C') {
                    if !self.copy_selection() {
                        self.toast(NoticeLevel::Info, "Nothing selected");
                    }
                } else if !self.target_editor_ref_empty() {
                    self.target_editor().clear();
                    self.history.pos = None;
                    self.popup = None;
                } else {
                    self.quit = true;
                }
            }
            KeyCode::Char('d') if ctrl(k) => {
                if self.target_editor_ref_empty() {
                    self.quit = true;
                } else {
                    self.target_editor().delete();
                }
            }
            KeyCode::Esc => {
                let busy = target.as_ref().and_then(|s| self.store.get(s)).map(|v| v.busy()).unwrap_or(false);
                if busy {
                    match self.esc_at {
                        Some(t) if t.elapsed() < Duration::from_secs(1) => {
                            self.esc_at = None;
                            if let Some(s) = target {
                                self.send(ClientMsg::Abort { session_id: s });
                                self.toast(NoticeLevel::Warning, "Interrupted");
                            }
                        }
                        _ => self.esc_at = Some(Instant::now()),
                    }
                } else if let Some(sid) = &self.current {
                    if let Some(v) = self.views.get_mut(sid) {
                        v.msg_select = None;
                    }
                }
            }
            KeyCode::Tab if !ctrl(k) && !alt(k) => self.cycle_agent(false),
            KeyCode::BackTab => self.cycle_agent(true),
            KeyCode::Char('t') if ctrl(k) => self.cycle_variant(),
            KeyCode::Char('v') if ctrl(k) && !shift(k) => self.request_clipboard_image(true),
            KeyCode::PageUp | KeyCode::PageDown => {
                if let Some(s) = self.scroll_target() {
                    let v = self.view_mut(&s);
                    let page = v.height.saturating_sub(2).max(1) as isize;
                    v.scroll_by(if k.code == KeyCode::PageUp { -page } else { page });
                }
            }
            KeyCode::Char('g') if ctrl(k) && alt(k) => {
                if let Some(s) = self.scroll_target() {
                    self.view_mut(&s).scroll_to_bottom();
                }
            }
            KeyCode::Char('g') if ctrl(k) => {
                if let Some(s) = self.scroll_target() {
                    self.view_mut(&s).scroll_to_top();
                }
            }
            KeyCode::Up if alt(k) && self.mode == Mode::Normal => self.enter_msg_select(),
            KeyCode::F(6) => self.focus_cycle(if shift(k) { -1 } else { 1 }),
            _ => return false,
        }
        true
    }

    fn scroll_target(&self) -> Option<String> {
        self.target_session()
    }

    fn on_input_key(&mut self, k: KeyEvent) {
        let w = self.regions.input_text_w.max(4) as usize;
        let ed = &mut self.editor;
        let newline = (k.code == KeyCode::Enter && (shift(&k) || alt(&k) || ctrl(&k))) || (ctrl(&k) && k.code == KeyCode::Char('j'));
        if newline {
            ed.insert_char('\n');
            self.after_edit();
            return;
        }
        match k.code {
            KeyCode::Enter => {
                self.submit();
                return;
            }
            KeyCode::Up => {
                if !ed.up(w) {
                    let cur = ed.text().to_string();
                    if let Some(t) = self.history.prev(&cur) {
                        self.editor.set_text(&t);
                        self.editor.buffer_start();
                    }
                }
                return;
            }
            KeyCode::Down => {
                if !ed.down(w) {
                    if let Some(t) = self.history.next() {
                        self.editor.set_text(&t);
                    }
                }
                return;
            }
            _ => {}
        }
        if edit_key(ed, &k, w) {
            self.after_edit();
        }
    }

    fn after_edit(&mut self) {
        self.history.pos = None;
        self.update_popup();
    }

    fn on_watch_key(&mut self, k: KeyEvent) {
        let dir = match k.code {
            KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down if ctrl(&k) || alt(&k) => Some(k.code),
            _ => None,
        };
        if let Some(d) = dir {
            self.focus_dir(d);
            return;
        }
        let Some(f) = self.watch.focus.clone() else { return };
        if k.code == KeyCode::Enter && k.modifiers.is_empty() {
            self.submit_pane();
            return;
        }
        if matches!(k.code, KeyCode::Up | KeyCode::Down) && k.modifiers.is_empty() {
            let v = self.view_mut(&f);
            v.scroll_by(if k.code == KeyCode::Up { -1 } else { 1 });
            return;
        }
        let ed = self.pane_editors.entry(f).or_default();
        edit_key(ed, &k, 1000);
    }

    pub fn focus_cycle(&mut self, d: isize) {
        let ids = self.visible_pane_ids();
        if ids.is_empty() {
            return;
        }
        let i = self.watch.focus.as_ref().and_then(|f| ids.iter().position(|x| x == f)).unwrap_or(0) as isize;
        let j = (i + d).rem_euclid(ids.len() as isize) as usize;
        self.watch.focus = Some(ids[j].clone());
    }

    fn visible_pane_ids(&self) -> Vec<String> {
        if self.watch.panes.is_empty() {
            self.watch_ids()
        } else {
            self.watch.panes.iter().map(|p| p.0.clone()).collect()
        }
    }

    /// Move focus to the nearest pane in direction `d` (geometrically).
    fn focus_dir(&mut self, d: KeyCode) {
        let panes = self.watch.panes.clone();
        let Some(cur) = self.watch.focus.as_ref().and_then(|f| panes.iter().find(|p| &p.0 == f)).map(|p| p.1) else {
            self.focus_cycle(1);
            return;
        };
        let (cx, cy) = (cur.x as i32 + cur.width as i32 / 2, cur.y as i32 + cur.height as i32 / 2);
        let mut best: Option<(i32, String)> = None;
        for (id, r, _) in &panes {
            let (x, y) = (r.x as i32 + r.width as i32 / 2, r.y as i32 + r.height as i32 / 2);
            let ok = match d {
                KeyCode::Left => r.x + r.width <= cur.x,
                KeyCode::Right => r.x >= cur.x + cur.width,
                KeyCode::Up => r.y + r.height <= cur.y,
                KeyCode::Down => r.y >= cur.y + cur.height,
                _ => false,
            };
            if !ok {
                continue;
            }
            let dist = (x - cx).abs() + (y - cy).abs() * 2;
            if best.as_ref().map(|b| dist < b.0).unwrap_or(true) {
                best = Some((dist, id.clone()));
            }
        }
        match best {
            Some((_, id)) => self.watch.focus = Some(id),
            None => {
                // Wrap to the next/previous page at the edges.
                if matches!(d, KeyCode::Right | KeyCode::Down) && self.watch.page + 1 < self.watch.pages {
                    self.watch.page += 1;
                    self.watch.focus = None;
                } else if matches!(d, KeyCode::Left | KeyCode::Up) && self.watch.page > 0 {
                    self.watch.page -= 1;
                    self.watch.focus = None;
                }
            }
        }
    }

    // ───────────────────────── mouse ─────────────────────────

    fn on_mouse(&mut self, m: MouseEvent) {
        let (x, y) = (m.column, m.row);
        match m.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let d: isize = if m.kind == MouseEventKind::ScrollUp { -3 } else { 3 };
                if let Some(dl) = self.dialog.as_mut() {
                    if dl.kind == DialogKind::Help {
                        dl.scroll = (dl.scroll as isize + d).max(0) as usize;
                    } else {
                        dl.move_sel(d.signum());
                    }
                    return;
                }
                if contains(self.regions.perm, x, y) {
                    self.perm_scroll = (self.perm_scroll as isize + d).max(0) as usize;
                    return;
                }
                if let Some(sid) = self.session_at(x, y) {
                    self.view_mut(&sid).scroll_by(d);
                    if let Some(dr) = self.drag.clone() {
                        self.update_drag(&dr.session, dr.last.0, dr.last.1);
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Left) => self.mouse_down(x, y),
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(dr) = self.drag.as_mut() {
                    dr.last = (x, y);
                    dr.moved = true;
                    let s = dr.session.clone();
                    self.update_drag(&s, x, y);
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if let Some(dr) = self.drag.take() {
                    if !dr.moved {
                        self.view_mut(&dr.session).selection = None;
                    }
                }
            }
            _ => {}
        }
    }

    /// Session whose chat area contains (x, y).
    fn session_at(&self, x: u16, y: u16) -> Option<String> {
        match self.mode {
            Mode::Normal => contains(self.regions.chat, x, y).then(|| self.current.clone()).flatten(),
            Mode::Watch => self.watch.panes.iter().find(|p| contains(p.1, x, y)).map(|p| p.0.clone()),
        }
    }

    fn chat_rect(&self, sid: &str) -> Option<ratatui::layout::Rect> {
        match self.mode {
            Mode::Normal => Some(self.regions.chat),
            Mode::Watch => self.watch.panes.iter().find(|p| p.0 == sid).map(|p| p.2),
        }
    }

    fn mouse_down(&mut self, x: u16, y: u16) {
        if let Some(d) = self.dialog.as_mut() {
            if !contains(d.rect, x, y) {
                self.dialog = None;
            } else if let Some(i) = d.item_at(y) {
                if d.selected == i {
                    self.dialog_activate();
                } else {
                    d.selected = i;
                }
            }
            return;
        }
        if let Some(p) = self.popup.as_mut() {
            if let Some(i) = p.item_at(x, y) {
                if i < p.items.len() {
                    p.selected = i;
                    self.popup_accept(true);
                }
                return;
            }
        }
        for (r, d) in self.regions.perm_buttons.clone() {
            if contains(r, x, y) {
                self.decide(d);
                return;
            }
        }
        // Clicking anywhere clears an old selection.
        self.clear_selection();
        if self.mode == Mode::Normal && contains(self.regions.input, x, y) {
            let row = (y.saturating_sub(self.regions.input.y + 1)) as usize + self.regions.input_scroll;
            let col = x.saturating_sub(self.regions.input_text_x) as usize;
            let w = self.regions.input_text_w as usize;
            self.editor.click(w, row, col);
            return;
        }
        if self.mode == Mode::Watch {
            if let Some(p) = self.watch.panes.iter().find(|p| contains(p.1, x, y)) {
                self.watch.focus = Some(p.0.clone());
            }
        }
        let Some(sid) = self.session_at(x, y) else { return };
        let Some(rect) = self.chat_rect(&sid) else { return };
        if !contains(rect, x, y) {
            return;
        }
        let v = self.view_mut(&sid);
        if v.unseen && !v.follow && y + 1 == rect.y + rect.height {
            v.scroll_to_bottom();
            return;
        }
        let abs = v.scroll_top + (y - rect.y) as usize;
        let col = content_col(v, x);
        if let Some(a) = v.hit_at(abs, col) {
            match a {
                Action::Copy(mid) => self.copy_message(&sid, &mid),
                Action::TogglePart(pid) | Action::ToggleMsg(pid) => {
                    let v = self.view_mut(&sid);
                    v.toggle(&pid);
                    v.follow = false;
                }
            }
            return;
        }
        if let Some(i) = v.msg_select.and(v.msg_index_at(abs)) {
            v.msg_select = Some(i);
        }
        v.selection = Some(Sel { anchor: (abs, col), head: (abs, col) });
        self.drag = Some(Drag { session: sid, last: (x, y), moved: false });
    }

    fn update_drag(&mut self, sid: &str, x: u16, y: u16) {
        let Some(rect) = self.chat_rect(sid) else { return };
        let v = self.view_mut(sid);
        let yy = y.clamp(rect.y, rect.y + rect.height.saturating_sub(1));
        let abs = (v.scroll_top + (yy - rect.y) as usize).min(v.total.saturating_sub(1));
        let col = if y < rect.y {
            0
        } else if y >= rect.y + rect.height {
            usize::MAX / 2
        } else {
            content_col(v, x)
        };
        if let Some(s) = v.selection.as_mut() {
            s.head = (abs, col);
        }
    }

    /// While dragging past the chat edge, scroll and extend the selection.
    pub fn drag_autoscroll(&mut self) -> bool {
        let Some(dr) = self.drag.clone() else { return false };
        let Some(rect) = self.chat_rect(&dr.session) else { return false };
        let (x, y) = dr.last;
        let d = if y < rect.y {
            -((rect.y - y) as isize).min(5)
        } else if y >= rect.y + rect.height {
            ((y + 1 - rect.y - rect.height) as isize).min(5)
        } else {
            return false;
        };
        self.view_mut(&dr.session).scroll_by(d);
        self.update_drag(&dr.session, x, y);
        true
    }
}

/// Common editing keys. Returns true if the text/cursor changed.
pub fn edit_key(ed: &mut crate::editor::Editor, k: &KeyEvent, w: usize) -> bool {
    let c = ctrl(k);
    let a = alt(k);
    match k.code {
        KeyCode::Char('a') if c => ed.home(),
        KeyCode::Char('e') if c => ed.end(),
        KeyCode::Char('b') if c => ed.left(),
        KeyCode::Char('f') if c => ed.right(),
        KeyCode::Char('b') if a => ed.word_left(),
        KeyCode::Char('f') if a => ed.word_right(),
        KeyCode::Char('d') if a => ed.delete_word_fwd(),
        KeyCode::Char('w') if c => ed.delete_word_back(),
        KeyCode::Char('h') if c => ed.backspace(),
        KeyCode::Char('u') if c => ed.kill_to_start(),
        KeyCode::Char('k') if c => ed.kill_to_end(),
        KeyCode::Backspace if a || c => ed.delete_word_back(),
        KeyCode::Backspace => ed.backspace(),
        KeyCode::Delete if c || a => ed.delete_word_fwd(),
        KeyCode::Delete => ed.delete(),
        KeyCode::Left if c || a => ed.word_left(),
        KeyCode::Right if c || a => ed.word_right(),
        KeyCode::Left => ed.left(),
        KeyCode::Right => ed.right(),
        KeyCode::Home if c => ed.buffer_start(),
        KeyCode::End if c => ed.buffer_end(),
        KeyCode::Home => ed.home(),
        KeyCode::End => ed.end(),
        KeyCode::Up => return ed.up(w),
        KeyCode::Down => return ed.down(w),
        KeyCode::Char(ch) if !c && !a => ed.insert_char(ch),
        _ => return false,
    }
    true
}
