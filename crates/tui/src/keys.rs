//! Keyboard and mouse handling.

use crate::app::{App, Mode, UiHit, ESC_WINDOW};
use crate::dialog::Outcome;
use crate::render::message::{selection_text, tool_input_text, Hit, Selection, Target};
use crate::screen;
use crate::ui;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use hn_proto::{Part, PermissionDecision, ToastLevel};
use ratatui::layout::Rect;
use std::time::Instant;

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
    pub fn on_key(&mut self, k: KeyEvent, area: Rect) {
        // Dialog first.
        if let Some(d) = self.dialog.as_mut() {
            let out = d.key(k);
            self.dialog_outcome(out);
            return;
        }
        // Leader sequences.
        if self.leader.take().is_some() {
            self.on_leader(k, area);
            return;
        }
        if !(k.code == KeyCode::Char('c') && ctrl(&k)) {
            self.ctrlc_armed = None;
        }
        // Global keys.
        match k.code {
            KeyCode::Char('x') if ctrl(&k) => {
                self.leader = Some(Instant::now());
                return;
            }
            KeyCode::Char('p') if ctrl(&k) => {
                self.open_palette();
                return;
            }
            KeyCode::F(2) => {
                self.toggle_mouse();
                return;
            }
            KeyCode::Char('c') | KeyCode::Char('C') if ctrl(&k) && shift(&k) => {
                self.copy_selection();
                return;
            }
            KeyCode::Char('y') if ctrl(&k) => {
                self.copy_selection_or_focus();
                return;
            }
            KeyCode::Char('c') if ctrl(&k) => {
                self.on_ctrl_c();
                return;
            }
            KeyCode::Char('d') if ctrl(&k) && self.active_editor().is_empty() => {
                self.quit = true;
                return;
            }
            KeyCode::PageUp => {
                self.scroll_active(-(self.viewport_of_active() as i64 - 2).max(1));
                return;
            }
            KeyCode::PageDown => {
                self.scroll_active((self.viewport_of_active() as i64 - 2).max(1));
                return;
            }
            KeyCode::Char('g') if ctrl(&k) && alt(&k) => {
                self.with_active_view(|v| v.scroll_to_bottom());
                return;
            }
            KeyCode::Char('g') if ctrl(&k) => {
                self.with_active_view(|v| v.scroll_to_top());
                return;
            }
            KeyCode::Home if k.modifiers.is_empty() && self.active_editor().is_empty() => {
                self.with_active_view(|v| v.scroll_to_top());
                return;
            }
            KeyCode::End if k.modifiers.is_empty() && self.active_editor().is_empty() => {
                self.with_active_view(|v| v.scroll_to_bottom());
                return;
            }
            KeyCode::End if ctrl(&k) => {
                self.with_active_view(|v| v.scroll_to_bottom());
                return;
            }
            KeyCode::Home if ctrl(&k) => {
                self.with_active_view(|v| v.scroll_to_top());
                return;
            }
            _ => {}
        }
        if self.mode == Mode::Monitor && alt(&k) {
            let dir = match k.code {
                KeyCode::Left => Some((-1, 0)),
                KeyCode::Right => Some((1, 0)),
                KeyCode::Up => Some((0, -1)),
                KeyCode::Down => Some((0, 1)),
                _ => None,
            };
            if let Some(d) = dir {
                self.move_pane_focus(d, area);
                return;
            }
        }
        // Permission prompt captures most keys.
        if self.on_permission_key(k) {
            return;
        }
        // Part focus navigation.
        let focus_nav = (self.mode == Mode::Normal && alt(&k)) || ctrl(&k);
        match k.code {
            KeyCode::Up if focus_nav => return self.move_focus(-1),
            KeyCode::Down if focus_nav => return self.move_focus(1),
            _ => {}
        }
        if let Some(t) = self.active().and_then(|id| self.views.get(&id)).and_then(|v| v.focus.clone()) {
            match k.code {
                KeyCode::Enter | KeyCode::Char(' ') if k.modifiers.is_empty() || k.modifiers == KeyModifiers::SHIFT => {
                    let id = self.active().unwrap();
                    let v = self.views.entry(id).or_default();
                    v.toggle(&t);
                    return;
                }
                KeyCode::Esc => {
                    self.with_active_view(|v| v.focus = None);
                    return;
                }
                _ => self.with_active_view(|v| v.focus = None),
            }
        }
        // Selection shortcuts.
        let has_sel = self.active_selection().is_some();
        if has_sel {
            match k.code {
                KeyCode::Char('y') if k.modifiers.is_empty() => {
                    self.copy_selection();
                    return;
                }
                KeyCode::Esc => {
                    self.with_active_view(|v| v.selection = None);
                    return;
                }
                _ => {}
            }
        }
        // Autocomplete popup.
        if self.on_autocomplete_key(k) {
            return;
        }
        match k.code {
            KeyCode::Esc => {
                self.on_esc();
                return;
            }
            KeyCode::Tab if k.modifiers.is_empty() => {
                self.cycle_agent(1);
                return;
            }
            KeyCode::BackTab => {
                self.cycle_agent(-1);
                return;
            }
            KeyCode::Char('z') if self.mode == Mode::Monitor && alt(&k) => {
                self.zoom = !self.zoom;
                return;
            }
            KeyCode::Char('v') if ctrl(&k) => {
                self.paste_clipboard();
                return;
            }
            _ => {}
        }
        self.on_editor_key(k);
    }

    fn on_ctrl_c(&mut self) {
        if !self.active_editor().is_empty() {
            self.active_editor().clear();
            self.hist_pos = None;
            self.ctrlc_armed = None;
            return;
        }
        if self.active_selection().is_some() {
            self.copy_selection();
            return;
        }
        if self.ctrlc_armed.take().is_some() {
            self.quit = true;
        } else {
            self.ctrlc_armed = Some(Instant::now());
        }
    }

    fn on_esc(&mut self) {
        let Some(id) = self.active() else { return };
        let busy = self
            .sessions
            .get(&id)
            .is_some_and(|s| s.info.status.is_active() || s.delayed_tool().is_some() || s.is_animating());
        if !busy {
            self.esc_armed = None;
            return;
        }
        match self.esc_armed.take() {
            Some((sid, at)) if sid == id && at.elapsed() < ESC_WINDOW => self.abort(&id),
            _ => self.esc_armed = Some((id, Instant::now())),
        }
    }

    fn on_leader(&mut self, k: KeyEvent, area: Rect) {
        let c = match k.code {
            KeyCode::Char(c) => c,
            KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down if self.mode == Mode::Monitor => {
                let d = match k.code {
                    KeyCode::Left => (-1, 0),
                    KeyCode::Right => (1, 0),
                    KeyCode::Up => (0, -1),
                    _ => (0, 1),
                };
                self.move_pane_focus(d, area);
                return;
            }
            _ => return,
        };
        let c = if ctrl(&k) { c.to_ascii_lowercase() } else { c };
        if self.mode == Mode::Monitor {
            match c {
                '1'..='9' => {
                    let n = c as usize - '1' as usize;
                    let cap = screen::pane_capacity(area);
                    let panes = self.panes();
                    if let Some(id) = panes.get(self.page * cap + n) {
                        self.focus_pane = Some(id.clone());
                    }
                    return;
                }
                'z' => {
                    self.zoom = !self.zoom;
                    return;
                }
                '[' | ']' => {
                    let cap = screen::pane_capacity(area);
                    let panes = self.panes();
                    let pages = panes.len().div_ceil(cap).max(1);
                    let p = if c == ']' { (self.page + 1) % pages } else { (self.page + pages - 1) % pages };
                    self.page = p;
                    self.focus_pane = panes.get(p * cap).cloned();
                    self.zoom = false;
                    return;
                }
                _ => {}
            }
        }
        let cmd = match c {
            'n' => "new",
            'l' => "sessions",
            'o' => "open",
            'm' => "models",
            'a' => "agents",
            'c' => "compact",
            'u' => "undo",
            'r' => "rename",
            'x' => "export",
            'y' => "copy",
            'e' => "editor",
            't' => "thinking",
            'd' => "details",
            'b' => "sidebar",
            's' => "select",
            'k' => "close",
            'h' | '?' => "help",
            'q' => "exit",
            _ => return,
        };
        self.run_builtin(cmd, "");
    }

    fn on_permission_key(&mut self, k: KeyEvent) -> bool {
        let Some(id) = self.active() else { return false };
        let Some(call_id) =
            self.sessions.get(&id).and_then(|s| s.pending_permission()).map(|(_, _, tp)| tp.call_id.clone())
        else {
            return false;
        };
        let sel = self.perm_sel.entry(call_id.clone()).or_insert(0);
        match k.code {
            KeyCode::Left | KeyCode::BackTab => *sel = (*sel + 2) % 3,
            KeyCode::Right | KeyCode::Tab => *sel = (*sel + 1) % 3,
            KeyCode::Char('h') if !ctrl(&k) => *sel = (*sel + 2) % 3,
            KeyCode::Char('l') if !ctrl(&k) => *sel = (*sel + 1) % 3,
            KeyCode::Up | KeyCode::Char('k') => {
                let s = self.perm_scroll.entry(call_id).or_insert(0);
                *s = s.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                *self.perm_scroll.entry(call_id).or_insert(0) += 1;
            }
            KeyCode::Enter => {
                let d = ui::PERM_CHOICES[*sel].0;
                self.permission(&id, &call_id, d);
            }
            KeyCode::Char('a') | KeyCode::Char('y') | KeyCode::Char('1') => {
                self.permission(&id, &call_id, PermissionDecision::Once)
            }
            KeyCode::Char('A') | KeyCode::Char('2') => self.permission(&id, &call_id, PermissionDecision::Always),
            KeyCode::Char('r') | KeyCode::Char('n') | KeyCode::Char('3') | KeyCode::Esc => {
                self.permission(&id, &call_id, PermissionDecision::Reject)
            }
            _ => {}
        }
        true
    }

    fn on_autocomplete_key(&mut self, k: KeyEvent) -> bool {
        if self.mode == Mode::Monitor {
            return false; // no popup in panes; `/cmd` + enter still runs commands
        }
        let ed = match (self.mode, self.focus_pane.clone()) {
            (Mode::Monitor, Some(id)) => self.pane_editors.entry(id).or_default().clone(),
            _ => self.editor.clone(),
        };
        let items = ui::autocomplete(self, &ed);
        if items.is_empty() {
            if ed.slash_query().is_none() {
                self.ac_hidden = false;
            }
            return false;
        }
        let n = items.len();
        match k.code {
            KeyCode::Up => self.ac_sel = (self.ac_sel + n - 1) % n,
            KeyCode::Down => self.ac_sel = (self.ac_sel + 1) % n,
            KeyCode::Char('p') if ctrl(&k) => self.ac_sel = (self.ac_sel + n - 1) % n,
            KeyCode::Char('n') if ctrl(&k) => self.ac_sel = (self.ac_sel + 1) % n,
            KeyCode::Esc => self.ac_hidden = true,
            KeyCode::Tab => {
                let it = &items[self.ac_sel.min(n - 1)];
                let t = format!("/{} ", it.name);
                self.active_editor().set_text(&t);
            }
            KeyCode::Enter if k.modifiers.is_empty() => {
                let it = &items[self.ac_sel.min(n - 1)];
                let (name, builtin) = (it.name.clone(), it.builtin);
                if builtin {
                    self.active_editor().clear();
                    self.run_builtin(&name, "");
                } else {
                    self.active_editor().set_text(&format!("/{name} "));
                }
                self.ac_sel = 0;
            }
            _ => return false,
        }
        true
    }

    fn on_editor_key(&mut self, k: KeyEvent) {
        let width = match self.mode {
            Mode::Normal => self.prompt_width,
            Mode::Monitor => usize::MAX / 4,
        };
        let c = ctrl(&k);
        let a = alt(&k);
        let mut changed_text = true;
        match k.code {
            KeyCode::Enter if a || shift(&k) || c => self.active_editor().newline(),
            KeyCode::Enter => {
                self.submit();
                self.ac_sel = 0;
                return;
            }
            KeyCode::Char('j') if c => self.active_editor().newline(),
            KeyCode::Backspace if a || c => self.active_editor().delete_word_back(),
            KeyCode::Char('h') if c => self.active_editor().delete_word_back(),
            KeyCode::Char('w') if c => self.active_editor().delete_word_back(),
            KeyCode::Backspace => self.active_editor().backspace(),
            KeyCode::Delete if a || c => self.active_editor().delete_word_forward(),
            KeyCode::Char('d') if a => self.active_editor().delete_word_forward(),
            KeyCode::Delete => self.active_editor().delete(),
            KeyCode::Char('d') if c => self.active_editor().delete(),
            KeyCode::Char('k') if c => self.active_editor().kill_to_end(),
            KeyCode::Char('u') if c => self.active_editor().kill_to_start(),
            KeyCode::Char('-') | KeyCode::Char('_') if c => self.active_editor().undo(),
            KeyCode::Char('z') if c => self.active_editor().undo(),
            KeyCode::Left if a || c => {
                self.active_editor().word_left();
                changed_text = false;
            }
            KeyCode::Right if a || c => {
                self.active_editor().word_right();
                changed_text = false;
            }
            KeyCode::Char('b') if a => {
                self.active_editor().word_left();
                changed_text = false;
            }
            KeyCode::Char('f') if a => {
                self.active_editor().word_right();
                changed_text = false;
            }
            KeyCode::Left => {
                self.active_editor().left();
                changed_text = false;
            }
            KeyCode::Right => {
                self.active_editor().right();
                changed_text = false;
            }
            KeyCode::Char('b') if c => {
                self.active_editor().left();
                changed_text = false;
            }
            KeyCode::Char('f') if c => {
                self.active_editor().right();
                changed_text = false;
            }
            KeyCode::Home => {
                self.active_editor().home();
                changed_text = false;
            }
            KeyCode::Char('a') if c => {
                self.active_editor().home();
                changed_text = false;
            }
            KeyCode::End => {
                self.active_editor().end();
                changed_text = false;
            }
            KeyCode::Char('e') if c => {
                self.active_editor().end();
                changed_text = false;
            }
            KeyCode::Up => {
                if !self.active_editor().up(width) {
                    self.history_move(-1);
                }
                return;
            }
            KeyCode::Down => {
                if !self.active_editor().down(width) {
                    self.history_move(1);
                }
                return;
            }
            KeyCode::Char(ch) if !c && !a => {
                self.active_editor().insert_char(ch);
                self.with_active_view(|v| v.selection = None);
            }
            _ => changed_text = false,
        }
        if changed_text {
            self.ac_sel = 0;
            self.hist_pos = None;
        }
    }

    fn history_move(&mut self, dir: i64) {
        if self.history.is_empty() {
            return;
        }
        let ed_text = self.active_editor().text().to_string();
        let pos = match self.hist_pos {
            None => {
                if dir > 0 {
                    return;
                }
                self.hist_draft = ed_text;
                self.history.len() - 1
            }
            Some(p) => {
                let np = p as i64 + dir;
                if np < 0 {
                    return;
                }
                if np as usize >= self.history.len() {
                    self.hist_pos = None;
                    let d = std::mem::take(&mut self.hist_draft);
                    self.active_editor().set_text(&d);
                    return;
                }
                np as usize
            }
        };
        self.hist_pos = Some(pos);
        let t = self.history[pos].clone();
        self.active_editor().set_text(&t);
    }

    // --- views / focus / selection ---------------------------------------------

    fn with_active_view(&mut self, f: impl FnOnce(&mut crate::render::message::ViewState)) {
        if let Some(id) = self.active() {
            f(self.views.entry(id).or_default());
        }
    }

    fn viewport_of_active(&self) -> usize {
        self.active().and_then(|id| self.views.get(&id)).map(|v| v.viewport).unwrap_or(20)
    }

    fn scroll_active(&mut self, d: i64) {
        self.with_active_view(|v| v.scroll_by(d));
    }

    fn active_selection(&self) -> Option<Selection> {
        let id = self.active()?;
        self.views.get(&id)?.selection.filter(|s| !s.is_empty())
    }

    fn copy_selection(&mut self) {
        let Some(id) = self.active() else { return };
        let Some(sel) = self.active_selection() else {
            self.toast(ToastLevel::Info, "Nothing selected · drag with the mouse to select");
            return;
        };
        let text = self.docs.get(&id).map(|d| selection_text(d, &sel)).unwrap_or_default();
        self.copy_text(&text, "selection");
    }

    fn copy_selection_or_focus(&mut self) {
        if self.active_selection().is_some() {
            return self.copy_selection();
        }
        let Some(id) = self.active() else { return };
        if let Some(t) = self.views.get(&id).and_then(|v| v.focus.clone()) {
            return self.copy_target(&id, &t, true);
        }
        self.run_builtin("copy", "");
    }

    /// Copy a part: tool → output (or input if `output` is false), reasoning → text.
    fn copy_target(&mut self, sid: &str, t: &Target, output: bool) {
        let Some(m) = self.sessions.get(sid).and_then(|s| s.get(&t.msg)) else { return };
        let (text, what) = match m.msg.parts.get(t.idx) {
            Some(Part::Tool(tp)) if output => (tp.output.clone(), "tool output"),
            Some(Part::Tool(tp)) => (tool_input_text(tp), "tool input"),
            Some(Part::Reasoning { text, .. }) => (text.clone(), "reasoning"),
            Some(Part::Text { text, .. }) => (text.clone(), "text"),
            _ => (m.msg.raw_text(), "message"),
        };
        self.copy_text(&text, what);
    }

    fn move_focus(&mut self, dir: i64) {
        let Some(id) = self.active() else { return };
        let Some(doc) = self.docs.get(&id) else { return };
        let targets = doc.targets();
        if targets.is_empty() {
            return;
        }
        let v = self.views.entry(id).or_default();
        let cur = v.focus.as_ref().and_then(|f| targets.iter().position(|(t, _)| t == f));
        let next = match cur {
            None => {
                if dir < 0 {
                    // Last target visible above the bottom of the viewport.
                    let bottom = v.scroll + v.viewport;
                    targets.iter().rposition(|(_, r)| *r < bottom).unwrap_or(targets.len() - 1)
                } else {
                    targets.iter().position(|(_, r)| *r >= v.scroll).unwrap_or(0)
                }
            }
            Some(i) => (i as i64 + dir).clamp(0, targets.len() as i64 - 1) as usize,
        };
        let (t, _) = targets[next].clone();
        if let Some((a, b)) = doc.target_rows(&t) {
            v.reveal(a, b.min(a + v.viewport.max(1)));
        }
        v.focus = Some(t);
    }

    fn move_pane_focus(&mut self, (dx, dy): (i32, i32), area: Rect) {
        let rects = screen::pane_rects(self, area);
        if rects.is_empty() {
            return;
        }
        let cur = self.focus_pane.as_ref().and_then(|f| rects.iter().find(|(id, _)| id == f)).map(|x| x.1);
        let Some(c) = cur else {
            self.focus_pane = Some(rects[0].0.clone());
            return;
        };
        let (cx, cy) = (c.x as i32 + c.width as i32 / 2, c.y as i32 + c.height as i32 / 2);
        let mut best: Option<(i64, String)> = None;
        for (id, r) in &rects {
            let (x, y) = (r.x as i32 + r.width as i32 / 2, r.y as i32 + r.height as i32 / 2);
            let (ddx, ddy) = (x - cx, y - cy);
            let ok = (dx != 0 && ddx.signum() == dx && ddx != 0) || (dy != 0 && ddy.signum() == dy && ddy != 0);
            if !ok {
                continue;
            }
            let (along, across) = if dx != 0 { (ddx.abs(), ddy.abs()) } else { (ddy.abs(), ddx.abs()) };
            let score = along as i64 + across as i64 * 3;
            if best.as_ref().is_none_or(|b| score < b.0) {
                best = Some((score, id.clone()));
            }
        }
        if let Some((_, id)) = best {
            self.focus_pane = Some(id);
        }
    }

    // --- paste ------------------------------------------------------------------

    pub fn on_paste(&mut self, s: String) {
        if let Some(d) = self.dialog.as_mut() {
            d.paste(&s);
            return;
        }
        if let Some((mime, bytes, _name)) = crate::clipboard::image_from_path(&s) {
            let data = crate::clipboard::b64(&bytes);
            self.active_editor().add_image(hn_proto::ImageData { mime, data });
            return;
        }
        let s = s.replace("\r\n", "\n").replace('\r', "\n");
        let lines = s.lines().count();
        if lines >= 3 || s.chars().count() > 150 {
            self.active_editor().add_paste(s);
        } else {
            self.active_editor().insert_str(&s);
        }
    }

    fn paste_clipboard(&mut self) {
        if let Some((mime, bytes)) = crate::clipboard::paste_image() {
            let data = crate::clipboard::b64(&bytes);
            self.active_editor().add_image(hn_proto::ImageData { mime, data });
            return;
        }
        match crate::clipboard::paste_text() {
            Some(t) if !t.is_empty() => self.on_paste(t),
            _ => self.toast(ToastLevel::Info, "Clipboard is empty"),
        }
    }

    // --- mouse ------------------------------------------------------------------

    fn hit_at(&self, x: u16, y: u16) -> Option<(Rect, UiHit)> {
        self.hits.iter().rev().find(|(r, _)| r.contains((x, y).into())).cloned()
    }

    pub fn on_mouse(&mut self, m: MouseEvent) {
        let (x, y) = (m.column, m.row);
        if let Some(d) = self.dialog.as_mut() {
            match m.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    if !d.rect.contains((x, y).into()) {
                        self.dialog = None;
                    } else if let Some(v) = d.click(y) {
                        self.dialog_outcome(Outcome::Select(v));
                    }
                }
                MouseEventKind::ScrollUp => d.move_sel(-1),
                MouseEventKind::ScrollDown => d.move_sel(1),
                _ => {}
            }
            return;
        }
        match m.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let d = if m.kind == MouseEventKind::ScrollUp { -3 } else { 3 };
                let target = match self.hit_at(x, y) {
                    Some((_, UiHit::Messages(id))) | Some((_, UiHit::Pane(id))) => Some(id),
                    _ => self.active(),
                };
                // Permission JSON scrolling when hovering the panel bottom area is handled by keys.
                if let Some(id) = target {
                    self.views.entry(id).or_default().scroll_by(d);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => self.on_click(x, y),
            MouseEventKind::Drag(MouseButton::Left) => self.on_drag(x, y),
            MouseEventKind::Up(MouseButton::Left) => {
                if let Some(d) = self.drag.take() {
                    if !d.moved {
                        if let Some(v) = self.views.get_mut(&d.session) {
                            v.selection = None;
                        }
                    }
                    if let Some(v) = self.views.get_mut(&d.session) {
                        v.selecting = false;
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Right) if self.active_selection().is_some() => self.copy_selection(),
            _ => {}
        }
    }

    fn on_click(&mut self, x: u16, y: u16) {
        let Some((r, hit)) = self.hit_at(x, y) else { return };
        // In monitor mode clicking anywhere in a pane focuses it.
        if self.mode == Mode::Monitor {
            let pane = match &hit {
                UiHit::Pane(id) | UiHit::Messages(id) | UiHit::NewMessages(id) => Some(id.clone()),
                UiHit::PermButton(id, ..) => Some(id.clone()),
                _ => None,
            };
            if let Some(p) = pane {
                if self.focus_pane.as_ref() != Some(&p) {
                    self.focus_pane = Some(p);
                }
            }
        }
        match hit {
            UiHit::Messages(sid) => {
                let Some(v) = self.views.get(&sid) else { return };
                let row = v.scroll + (y - r.y) as usize;
                let col = x - r.x;
                let h = self.docs.get(&sid).and_then(|d| d.hit_at(row, col).cloned());
                match h {
                    Some(Hit::Toggle(t)) => {
                        let v = self.views.entry(sid.clone()).or_default();
                        v.toggle(&t);
                        v.focus = Some(t);
                        v.selection = None;
                    }
                    Some(Hit::CopyMessage(mid)) => {
                        let text = self.sessions.get(&sid).and_then(|s| s.get(&mid)).map(|e| e.msg.raw_text());
                        self.copy_text(&text.unwrap_or_default(), "message");
                    }
                    Some(Hit::CopyToolInput(t)) => self.copy_target(&sid, &t, false),
                    Some(Hit::CopyToolOutput(t)) => self.copy_target(&sid, &t, true),
                    None => {
                        let v = self.views.entry(sid.clone()).or_default();
                        v.focus = None;
                        v.selection = Some(Selection { anchor: (row, col), head: (row, col) });
                        v.selecting = true;
                        self.drag = Some(Drag { session: sid, rect: r, start: (x, y), moved: false });
                    }
                }
            }
            UiHit::Prompt(_) => {
                if self.mode == Mode::Normal {
                    let w = self.prompt_width;
                    let (_, (cr, _)) = ui::editor_view(&self.editor, w, 8);
                    let (abs_r, _) = self.editor.cursor_pos(w);
                    let start = abs_r - cr;
                    self.editor.click(w, start + (y - r.y) as usize, (x - r.x) as usize);
                }
            }
            UiHit::PermButton(sid, call, d) => self.permission(&sid, &call, d),
            UiHit::Autocomplete(i) => {
                self.ac_sel = i;
                let items = ui::autocomplete(self, &self.editor.clone());
                if let Some(it) = items.get(i) {
                    let (name, builtin) = (it.name.clone(), it.builtin);
                    if builtin {
                        self.active_editor().clear();
                        self.run_builtin(&name, "");
                    } else {
                        self.active_editor().set_text(&format!("/{name} "));
                    }
                }
            }
            UiHit::NewMessages(sid) => self.views.entry(sid).or_default().scroll_to_bottom(),
            UiHit::SidebarSession(sid) => self.switch_to(&sid),
            UiHit::Pane(_) => {}
        }
    }

    fn on_drag(&mut self, x: u16, y: u16) {
        let Some(d) = self.drag.as_mut() else { return };
        if (x, y) != d.start {
            d.moved = true;
        }
        let r = d.rect;
        let sid = d.session.clone();
        let Some(v) = self.views.get_mut(&sid) else { return };
        // Auto-scroll when dragging past the edges.
        if y < r.y {
            v.scroll_by(-1);
        } else if y >= r.bottom() {
            v.scroll_by(1);
        }
        let yy = y.clamp(r.y, r.bottom().saturating_sub(1));
        let xx = x.clamp(r.x, r.right());
        let row = v.scroll + (yy - r.y) as usize;
        let col = xx - r.x;
        if let Some(sel) = v.selection.as_mut() {
            sel.head = (row, col);
        }
    }
}

use crate::app::Drag;
