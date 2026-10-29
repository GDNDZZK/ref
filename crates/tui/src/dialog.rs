//! Modal dialogs: generic searchable grouped list (commands, models, agents,
//! sessions, open sessions, help) and a single-line input dialog (rename).

use crate::input::Editor;
use crate::render::util::{fit_spans, left_right, spans_width};
use crate::text;
use crate::theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use hn_proto::ModelRef;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

#[derive(Debug, Clone, PartialEq)]
pub enum DValue {
    Command(String),
    Model(ModelRef),
    Agent(String),
    Session(String),
    None,
}

#[derive(Debug, Clone)]
pub struct ListItem {
    pub group: String,
    pub label: String,
    pub desc: String,
    pub right: String,
    pub dot: Option<(String, Color)>,
    pub current: bool,
    pub value: DValue,
    /// Extra searchable text (ids etc.), not displayed.
    pub keys: String,
    /// Sub-row of a parent item (e.g. a model variant): drawn indented under it,
    /// or as `parent · label` when the list is filtered.
    pub parent: Option<String>,
}

impl ListItem {
    pub fn new(group: impl Into<String>, label: impl Into<String>, value: DValue) -> Self {
        ListItem {
            group: group.into(),
            label: label.into(),
            desc: String::new(),
            right: String::new(),
            dot: None,
            current: false,
            value,
            keys: String::new(),
            parent: None,
        }
    }
    pub fn keys(mut self, k: impl Into<String>) -> Self {
        self.keys = k.into();
        self
    }
    pub fn parent(mut self, p: impl Into<String>) -> Self {
        self.parent = Some(p.into());
        self
    }
    pub fn desc(mut self, d: impl Into<String>) -> Self {
        self.desc = d.into();
        self
    }
    pub fn right(mut self, r: impl Into<String>) -> Self {
        self.right = r.into();
        self
    }
    pub fn dot(mut self, s: &str, c: Color) -> Self {
        self.dot = Some((s.into(), c));
        self
    }
    pub fn current(mut self, c: bool) -> Self {
        self.current = c;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DKind {
    Commands,
    Models,
    Agents,
    Sessions,
    /// Open sessions in the backend. `close` = enter closes instead of attaching.
    Open { close: bool },
    Help,
    Rename,
}

pub enum Outcome {
    None,
    Close,
    Select(DValue),
    /// ctrl+d confirmed on an item.
    Delete(DValue),
    /// Input dialog submitted.
    Submit(String),
}

pub struct Dialog {
    pub kind: DKind,
    pub title: String,
    pub query: Editor,
    pub items: Vec<ListItem>,
    filtered: Vec<usize>,
    pub sel: usize,
    scroll: usize,
    confirm: Option<usize>,
    pub hints: Vec<(&'static str, &'static str)>,
    pub placeholder: String,
    pub loading: bool,
    /// Rows of the list from the last draw: (screen rect row y, filtered index).
    pub row_hits: Vec<(u16, usize)>,
    pub list_rect: Rect,
    pub rect: Rect,
}

enum DRow {
    Header(String),
    Item(usize),
}

fn text_contains(hay: &str, needle: &str) -> bool {
    hay.to_lowercase().contains(&needle.to_lowercase())
}

/// Case-insensitive fuzzy subsequence score (higher is better).
pub fn fuzzy(query: &str, text: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let t: Vec<char> = text.chars().flat_map(|c| c.to_lowercase()).collect();
    let q: Vec<char> = query.chars().flat_map(|c| c.to_lowercase()).filter(|c| !c.is_whitespace()).collect();
    if q.is_empty() {
        return Some(0);
    }
    let tl: String = t.iter().collect();
    let ql: String = q.iter().collect();
    let mut score = 0;
    if tl.starts_with(&ql) {
        score += 100;
    } else if tl.contains(&ql) {
        score += 50;
    }
    let mut ti = 0;
    let mut prev: Option<usize> = None;
    for qc in &q {
        let mut found = false;
        while ti < t.len() {
            if t[ti] == *qc {
                if prev.is_some_and(|p| p + 1 == ti) {
                    score += 5;
                }
                if ti == 0 || !t[ti - 1].is_alphanumeric() {
                    score += 3;
                }
                score += 1;
                prev = Some(ti);
                ti += 1;
                found = true;
                break;
            }
            ti += 1;
        }
        if !found {
            return None;
        }
    }
    Some(score - (t.len() as i32 / 10))
}

impl Dialog {
    pub fn list(kind: DKind, title: &str, items: Vec<ListItem>) -> Self {
        let mut d = Dialog {
            kind,
            title: title.into(),
            query: Editor::new(),
            items,
            filtered: Vec::new(),
            sel: 0,
            scroll: 0,
            confirm: None,
            hints: vec![("↑↓", "select"), ("enter", "confirm")],
            placeholder: "Search".into(),
            loading: false,
            row_hits: Vec::new(),
            list_rect: Rect::default(),
            rect: Rect::default(),
        };
        d.refilter();
        if let Some(p) = d.filtered.iter().position(|&i| d.items[i].current) {
            d.sel = p;
        }
        d
    }

    pub fn input(kind: DKind, title: &str, initial: &str) -> Self {
        let mut d = Dialog::list(kind, title, Vec::new());
        d.query.set_text(initial);
        d.hints = vec![("enter", "save"), ("esc", "cancel")];
        d.placeholder = String::new();
        d
    }

    fn is_input(&self) -> bool {
        matches!(self.kind, DKind::Rename)
    }

    pub fn set_items(&mut self, items: Vec<ListItem>) {
        self.items = items;
        self.loading = false;
        self.refilter();
        if let Some(p) = self.filtered.iter().position(|&i| self.items[i].current) {
            self.sel = p;
        }
    }

    fn refilter(&mut self) {
        let q = self.query.text().trim().to_string();
        if q.is_empty() {
            self.filtered = (0..self.items.len()).collect();
        } else {
            let terms: Vec<&str> = q.split_whitespace().collect();
            let mut scored: Vec<(i32, usize)> = self
                .items
                .iter()
                .enumerate()
                .filter_map(|(i, it)| {
                    let label = match &it.parent {
                        Some(p) => format!("{p} {}", it.label),
                        None => it.label.clone(),
                    };
                    let hay = format!("{label} {} {} {}", it.group, it.desc, it.keys);
                    // Whole query against the label first (keeps "cop" → "claude-opus" style matches).
                    let whole = fuzzy(&q, &label).map(|s| s + 20);
                    // Every whitespace-separated term must match somewhere.
                    let mut sum = 0;
                    let mut all = true;
                    for t in &terms {
                        let best = [&label, &it.group, &it.desc, &it.keys]
                            .iter()
                            .filter_map(|h| fuzzy(t, h).map(|s| if text_contains(h, t) { s + 30 } else { s }))
                            .max()
                            .or_else(|| fuzzy(t, &hay));
                        match best {
                            Some(s) => sum += s,
                            None => {
                                all = false;
                                break;
                            }
                        }
                    }
                    let multi = all.then_some(sum);
                    whole.max(multi).map(|s| (s, i))
                })
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            self.filtered = scored.into_iter().map(|x| x.1).collect();
        }
        self.sel = self.sel.min(self.filtered.len().saturating_sub(1));
        self.confirm = None;
    }

    pub fn selected(&self) -> Option<&ListItem> {
        self.filtered.get(self.sel).map(|&i| &self.items[i])
    }

    fn grouped(&self) -> bool {
        self.query.text().trim().is_empty() && self.items.iter().any(|i| !i.group.is_empty())
    }

    fn rows(&self) -> Vec<DRow> {
        let mut rows = Vec::new();
        let grouped = self.grouped();
        let mut last: Option<&str> = None;
        for (fi, &i) in self.filtered.iter().enumerate() {
            let g = self.items[i].group.as_str();
            if grouped && last != Some(g) {
                if last.is_some() {
                    rows.push(DRow::Header(String::new()));
                }
                rows.push(DRow::Header(g.to_string()));
                last = Some(g);
            }
            rows.push(DRow::Item(fi));
        }
        rows
    }

    pub fn move_sel(&mut self, d: i64) {
        if self.filtered.is_empty() {
            return;
        }
        let n = self.filtered.len() as i64;
        self.sel = ((self.sel as i64 + d).rem_euclid(n)) as usize;
        self.confirm = None;
    }

    pub fn key(&mut self, k: KeyEvent) -> Outcome {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Esc => return Outcome::Close,
            KeyCode::Char('c') if ctrl => return Outcome::Close,
            KeyCode::Enter => {
                if self.is_input() {
                    return Outcome::Submit(self.query.text().trim().to_string());
                }
                return match self.selected() {
                    Some(it) => Outcome::Select(it.value.clone()),
                    None => Outcome::None,
                };
            }
            KeyCode::Up => self.move_sel(-1),
            KeyCode::Down => self.move_sel(1),
            KeyCode::Char('p') if ctrl => self.move_sel(-1),
            KeyCode::Char('n') if ctrl => self.move_sel(1),
            KeyCode::PageUp => self.move_sel(-10),
            KeyCode::PageDown => self.move_sel(10),
            KeyCode::Char('d') if ctrl && matches!(self.kind, DKind::Sessions | DKind::Open { .. }) => {
                if let Some(it) = self.selected() {
                    if self.confirm == Some(self.sel) {
                        let v = it.value.clone();
                        self.confirm = None;
                        return Outcome::Delete(v);
                    }
                    self.confirm = Some(self.sel);
                }
            }
            KeyCode::Backspace => {
                if ctrl || k.modifiers.contains(KeyModifiers::ALT) {
                    self.query.delete_word_back();
                } else {
                    self.query.backspace();
                }
                self.refilter();
            }
            KeyCode::Char('w') if ctrl => {
                self.query.delete_word_back();
                self.refilter();
            }
            KeyCode::Char('u') if ctrl => {
                self.query.clear();
                self.refilter();
            }
            KeyCode::Left => self.query.left(),
            KeyCode::Right => self.query.right(),
            KeyCode::Home => self.query.home(),
            KeyCode::End => self.query.end(),
            KeyCode::Char(c) if !ctrl => {
                self.query.insert_char(c);
                self.refilter();
            }
            _ => {}
        }
        Outcome::None
    }

    pub fn paste(&mut self, s: &str) {
        self.query.insert_str(&text::one_line(s));
        self.refilter();
    }

    /// Mouse click at screen row `y`: select and return the item if one was hit.
    pub fn click(&mut self, y: u16) -> Option<DValue> {
        let fi = self.row_hits.iter().find(|(ry, _)| *ry == y).map(|x| x.1)?;
        self.sel = fi;
        self.selected().map(|i| i.value.clone())
    }

    // --- drawing ------------------------------------------------------------

    pub fn draw(&mut self, area: Rect, buf: &mut Buffer) {
        dim(area, buf);
        let wide = matches!(self.kind, DKind::Models | DKind::Sessions | DKind::Open { .. } | DKind::Help);
        let w = if wide { 88 } else { 64 }.min(area.width.saturating_sub(4)).max(20.min(area.width));
        let list_h_wanted = if self.is_input() { 0 } else { self.rows().len().max(1) as u16 };
        let chrome = if self.is_input() { 7 } else { 8 };
        // opencode places dialogs at height/4 from the top; shrink that offset when tall.
        let top_off = area.height / 4;
        let max_h = area.height.saturating_sub(4).max(6);
        let h = (chrome + list_h_wanted).min(max_h).min(area.height);
        let top = area.y + top_off.min(area.height.saturating_sub(h + 2) / 2 + 1).min(area.height.saturating_sub(h));
        let x = area.x + (area.width.saturating_sub(w)) / 2;
        let rect = Rect { x, y: top, width: w, height: h };
        self.rect = rect;
        let bg = theme::PANEL;
        buf.set_style(rect, Style::new().bg(bg).fg(theme::TEXT));
        for yy in rect.y..rect.bottom() {
            for xx in rect.x..rect.right() {
                if let Some(c) = buf.cell_mut((xx, yy)) {
                    c.set_symbol(" ");
                    c.set_bg(bg);
                }
            }
        }
        let inner_x = rect.x + 2;
        let iw = rect.width.saturating_sub(4) as usize;
        let mut y = rect.y + 1;
        // Title row
        let (title, _) = left_right(
            vec![Span::styled(self.title.clone(), theme::bold(theme::TEXT))],
            vec![Span::styled("esc", theme::muted())],
            iw,
            Style::new(),
        );
        buf.set_line(inner_x, y, &title, iw as u16);
        y += 2;
        // Search / input row
        let q = self.query.text();
        let cursor_col = text::width(&q[..self.query.cursor()]);
        let line = if q.is_empty() {
            Line::from(Span::styled(self.placeholder.clone(), theme::faint()))
        } else {
            let shown = if cursor_col >= iw { text::tail_fit(q, iw.saturating_sub(1)) } else { q.to_string() };
            Line::from(Span::styled(shown, theme::text()))
        };
        buf.set_line(inner_x, y, &line, iw as u16);
        let cx = inner_x + (cursor_col.min(iw.saturating_sub(1))) as u16;
        if let Some(c) = buf.cell_mut((cx, y)) {
            c.set_style(Style::new().bg(theme::TEXT).fg(theme::BG));
        }
        y += 2;
        self.row_hits.clear();
        if !self.is_input() {
            let list_bottom = rect.bottom().saturating_sub(2);
            let list_h = list_bottom.saturating_sub(y) as usize;
            self.list_rect = Rect { x: rect.x, y, width: rect.width, height: list_h as u16 };
            let rows = self.rows();
            if self.loading {
                buf.set_line(inner_x, y, &Line::from(Span::styled("Loading…", theme::muted())), iw as u16);
            } else if rows.is_empty() {
                buf.set_line(inner_x, y, &Line::from(Span::styled("No results", theme::muted())), iw as u16);
            }
            // Keep selection visible.
            let sel_row = rows.iter().position(|r| matches!(r, DRow::Item(i) if *i == self.sel)).unwrap_or(0);
            if sel_row < self.scroll {
                self.scroll = sel_row.saturating_sub(1);
            } else if sel_row >= self.scroll + list_h {
                self.scroll = sel_row + 1 - list_h;
            }
            // Include the group header when selecting the first item of a group.
            if self.scroll > 0 && sel_row == self.scroll && matches!(rows.get(sel_row - 1), Some(DRow::Header(_))) {
                self.scroll -= 1;
            }
            for (ri, row) in rows.iter().enumerate().skip(self.scroll).take(list_h) {
                let yy = y + (ri - self.scroll) as u16;
                match row {
                    DRow::Header(g) => {
                        let l = Line::from(Span::styled(g.clone(), theme::bold(theme::ACCENT)));
                        buf.set_line(inner_x, yy, &l, iw as u16);
                    }
                    DRow::Item(fi) => {
                        let it = &self.items[self.filtered[*fi]];
                        let selected = *fi == self.sel;
                        let confirm = self.confirm == Some(*fi);
                        self.row_hits.push((yy, *fi));
                        draw_item(it, selected, confirm, !self.grouped(), rect.x + 1, yy, rect.width - 2, buf);
                    }
                }
            }
            if rows.len() > list_h && list_h > 0 {
                // Scrollbar
                let total = rows.len();
                let th = ((list_h * list_h) / total).max(1);
                let ty = (self.scroll * list_h) / total;
                for i in 0..list_h {
                    let ch = if i >= ty && i < ty + th { "┃" } else { "│" };
                    let col = if i >= ty && i < ty + th { theme::BORDER_ACTIVE } else { theme::BORDER_SUBTLE };
                    if let Some(c) = buf.cell_mut((rect.right() - 1, y + i as u16)) {
                        c.set_symbol(ch);
                        c.set_fg(col);
                    }
                }
            }
        }
        // Hints
        let mut hint = Vec::new();
        for (k, a) in &self.hints {
            if !hint.is_empty() {
                hint.push(Span::raw("  "));
            }
            hint.push(Span::styled(*k, theme::text()));
            hint.push(Span::styled(format!(" {a}"), theme::muted()));
        }
        if self.confirm.is_some() {
            hint = vec![Span::styled("press ctrl+d again to confirm", theme::fg(theme::ERROR))];
        }
        let hy = rect.bottom().saturating_sub(1).max(y);
        if hy < rect.bottom() {
            buf.set_line(inner_x, hy, &Line::from(fit_spans(hint, iw)), iw as u16);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_item(it: &ListItem, selected: bool, confirm: bool, flat: bool, x: u16, y: u16, w: u16, buf: &mut Buffer) {
    let (bg, fg) = if confirm {
        (theme::ERROR, theme::BG)
    } else if selected {
        (theme::PRIMARY, theme::BG)
    } else {
        (theme::PANEL, theme::TEXT)
    };
    let sub = if selected || confirm { fg } else { theme::MUTED };
    let mut left = vec![Span::raw(" ")];
    match &it.dot {
        Some((s, c)) => left.push(Span::styled(format!("{s} "), Style::new().fg(if selected { fg } else { *c }))),
        None => left.push(Span::raw("  ")),
    }
    let mut label_style = Style::new().fg(fg);
    if it.current {
        label_style = label_style.add_modifier(Modifier::BOLD);
    }
    match &it.parent {
        Some(p) if flat => {
            left.push(Span::styled(p.clone(), Style::new().fg(fg)));
            left.push(Span::styled(" · ", Style::new().fg(sub)));
            left.push(Span::styled(it.label.clone(), label_style));
        }
        Some(_) => {
            left.push(Span::styled("  · ", Style::new().fg(sub)));
            left.push(Span::styled(it.label.clone(), label_style));
        }
        None => left.push(Span::styled(it.label.clone(), label_style)),
    }
    if it.current {
        left.push(Span::styled(" ●", Style::new().fg(if selected { fg } else { theme::PRIMARY })));
    }
    let desc = if flat && !it.group.is_empty() && it.desc.is_empty() { it.group.clone() } else { it.desc.clone() };
    if !desc.is_empty() {
        left.push(Span::styled(format!("  {desc}"), Style::new().fg(sub)));
    }
    let right = if it.right.is_empty() { vec![] } else { vec![Span::styled(format!("{} ", it.right), Style::new().fg(sub))] };
    let rw = spans_width(&right);
    let _ = rw;
    let (line, _) = left_right(left, right, w as usize, Style::new());
    let line = crate::render::util::with_bg(line, w as usize, bg);
    buf.set_line(x, y, &line, w);
}

/// Dim everything in `area` (modal backdrop).
pub fn dim(area: Rect, buf: &mut Buffer) {
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            if let Some(c) = buf.cell_mut((x, y)) {
                let f = c.fg;
                let b = c.bg;
                c.set_fg(darken(f, theme::TEXT));
                c.set_bg(darken(b, theme::BG));
            }
        }
    }
}

fn darken(c: Color, default: Color) -> Color {
    let base = match c {
        Color::Rgb(..) => c,
        _ => default,
    };
    theme::blend(base, Color::Rgb(0, 0, 0), 0.6)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_scores() {
        assert!(fuzzy("opus", "claude-opus-5.5").is_some());
        assert!(fuzzy("cop", "claude-opus").is_some());
        assert!(fuzzy("xyz", "claude").is_none());
        assert!(fuzzy("new", "new").unwrap() > fuzzy("new", "rename window").unwrap_or(-100));
    }

    #[test]
    fn multi_term_filter() {
        let items = vec![
            ListItem::new("DGN", "Claude Sonnet 5.5", DValue::None).keys("dgn/claude-sonnet-5.5"),
            ListItem::new("DGN", "high", DValue::Command("x".into())).parent("Claude Sonnet 5.5").keys("dgn/claude-sonnet-5.5#high"),
            ListItem::new("DGN", "high", DValue::None).parent("Claude Opus 5.5").keys("dgn/claude-opus-5.5#high"),
            ListItem::new("Zai", "glm-5.3", DValue::None).keys("zai/glm-5.3"),
        ];
        let mut d = Dialog::list(DKind::Models, "Models", items);
        d.paste("dgn sonnet high");
        assert_eq!(d.filtered.len(), 1);
        assert_eq!(d.selected().unwrap().value, DValue::Command("x".into()));
        d.query.clear();
        d.paste("glm");
        assert_eq!(d.selected().unwrap().label, "glm-5.3");
    }

    #[test]
    fn filter_and_select() {
        let items = vec![
            ListItem::new("A", "alpha", DValue::Command("alpha".into())),
            ListItem::new("A", "beta", DValue::Command("beta".into())),
            ListItem::new("B", "gamma", DValue::Command("gamma".into())),
        ];
        let mut d = Dialog::list(DKind::Commands, "Commands", items);
        d.paste("gam");
        assert_eq!(d.selected().unwrap().label, "gamma");
        match d.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) {
            Outcome::Select(DValue::Command(c)) => assert_eq!(c, "gamma"),
            _ => panic!(),
        }
    }
}
