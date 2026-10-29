//! Modal dialogs: generic filterable list (commands, sessions, models, variants, agents), help text.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Widget};

use super::layout::centered;
use super::text::{pad_line, truncate, width};
use super::theme::*;

#[derive(Debug, Clone, PartialEq)]
pub enum DialogKind {
    Commands,
    Sessions,
    Models,
    Variants,
    Agents,
    Help,
}

#[derive(Debug, Clone, Default)]
pub struct Item {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub section: String,
    pub badges: Vec<(String, Color)>,
    /// Shown right-aligned (e.g. key binding, date).
    pub right: String,
    pub current: bool,
    pub color: Option<Color>,
}

pub struct ListDialog {
    pub kind: DialogKind,
    pub title: String,
    pub items: Vec<Item>,
    pub filter: String,
    pub selected: usize,
    pub scroll: usize,
    pub hint: String,
    /// Screen rows of visible items from the last draw: (y, filtered index).
    pub rows: Vec<(u16, usize)>,
    pub rect: Rect,
    /// For Help: static lines.
    pub help: Vec<Line<'static>>,
}

impl ListDialog {
    pub fn new(kind: DialogKind, title: &str, items: Vec<Item>, hint: &str) -> Self {
        let selected = items.iter().position(|i| i.current).unwrap_or(0);
        ListDialog {
            kind,
            title: title.into(),
            items,
            filter: String::new(),
            selected,
            scroll: 0,
            hint: hint.into(),
            rows: vec![],
            rect: Rect::default(),
            help: vec![],
        }
    }

    pub fn filtered(&self) -> Vec<usize> {
        let words: Vec<String> = self.filter.to_lowercase().split_whitespace().map(String::from).collect();
        self.items
            .iter()
            .enumerate()
            .filter(|(_, it)| {
                if words.is_empty() {
                    return true;
                }
                let hay = format!("{} {} {} {}", it.label, it.detail, it.section, it.id).to_lowercase();
                words.iter().all(|w| hay.contains(w.as_str()))
            })
            .map(|(i, _)| i)
            .collect()
    }

    pub fn selected_item(&self) -> Option<&Item> {
        let f = self.filtered();
        f.get(self.selected).map(|&i| &self.items[i])
    }

    pub fn move_sel(&mut self, d: isize) {
        let n = self.filtered().len();
        if n == 0 {
            self.selected = 0;
            return;
        }
        self.selected = (self.selected as isize + d).rem_euclid(n as isize) as usize;
    }

    pub fn page(&mut self, d: isize) {
        let n = self.filtered().len();
        if n == 0 {
            return;
        }
        let page = self.rows.len().max(1) as isize;
        self.selected = (self.selected as isize + d * page).clamp(0, n as isize - 1) as usize;
    }

    pub fn set_filter(&mut self, f: String) {
        self.filter = f;
        self.selected = 0;
        self.scroll = 0;
    }

    pub fn set_items(&mut self, items: Vec<Item>) {
        let cur = self.selected_item().map(|i| i.id.clone());
        self.items = items;
        let f = self.filtered();
        self.selected = cur.and_then(|id| f.iter().position(|&i| self.items[i].id == id)).unwrap_or(0).min(f.len().saturating_sub(1));
    }

    /// Index (into filtered) at a screen row.
    pub fn item_at(&self, y: u16) -> Option<usize> {
        self.rows.iter().find(|(ry, _)| *ry == y).map(|(_, i)| *i)
    }

    pub fn draw(&mut self, buf: &mut Buffer, area: Rect) {
        let w = (area.width.saturating_sub(4)).clamp(30, 90);
        let h = (area.height.saturating_sub(4)).clamp(8, 30);
        let rect = centered(area, w, h);
        self.rect = rect;
        Clear.render(rect, buf);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(fg(BORDER_ACTIVE))
            .style(Style::new().bg(PANEL))
            .title(Span::styled(format!(" {} ", self.title), bold(TEXT)))
            .title_bottom(Line::from(Span::styled(format!(" {} ", self.hint), muted())).right_aligned());
        let inner = block.inner(rect);
        block.render(rect, buf);
        if inner.height < 3 {
            return;
        }
        let iw = inner.width as usize;
        let bg = Style::new().bg(PANEL);
        if self.kind == DialogKind::Help {
            let rows = inner.height as usize;
            self.scroll = self.scroll.min(self.help.len().saturating_sub(rows));
            for (i, l) in self.help.iter().skip(self.scroll).take(rows).enumerate() {
                buf.set_line(inner.x + 1, inner.y + i as u16, l, inner.width.saturating_sub(2));
            }
            return;
        }
        // Filter input.
        let ph = if self.filter.is_empty() { Span::styled("Search…", muted()) } else { Span::styled(self.filter.clone(), text()) };
        let fl = Line::from(vec![Span::styled(" ⌕ ", fg(PRIMARY)), ph]);
        buf.set_line(inner.x, inner.y, &pad_line(fl, iw, Style::new().bg(ELEMENT)), inner.width);
        buf.set_style(Rect { height: 1, ..inner }, Style::new().bg(ELEMENT));
        let fx = inner.x + 3 + width(&self.filter) as u16;
        if let Some(c) = buf.cell_mut((fx.min(inner.x + inner.width - 1), inner.y)) {
            c.set_style(Style::new().bg(TEXT).fg(PANEL));
        }
        let list_area = Rect { y: inner.y + 2, height: inner.height - 2, ..inner };
        let filtered = self.filtered();
        // Build visual rows: section headers + items.
        let mut vis: Vec<(Option<usize>, Line<'static>)> = Vec::new();
        let mut last_section = String::new();
        let mut sel_row = 0;
        for (fi, &ii) in filtered.iter().enumerate() {
            let it = &self.items[ii];
            if it.section != last_section && !it.section.is_empty() {
                if !vis.is_empty() {
                    vis.push((None, Line::default()));
                }
                vis.push((None, Line::from(Span::styled(format!(" {}", it.section), bold(ACCENT)))));
                last_section = it.section.clone();
            }
            if fi == self.selected {
                sel_row = vis.len();
            }
            vis.push((Some(fi), self.item_line(it, iw, fi == self.selected)));
        }
        let rows = list_area.height as usize;
        if sel_row < self.scroll {
            self.scroll = sel_row.saturating_sub(1);
        } else if sel_row >= self.scroll + rows {
            self.scroll = sel_row + 1 - rows;
        }
        if self.selected == 0 {
            self.scroll = 0;
        }
        self.scroll = self.scroll.min(vis.len().saturating_sub(rows));
        self.rows.clear();
        for (i, (fi, l)) in vis.iter().skip(self.scroll).take(rows).enumerate() {
            let y = list_area.y + i as u16;
            buf.set_line(list_area.x, y, l, list_area.width);
            if let Some(fi) = fi {
                self.rows.push((y, *fi));
            }
        }
        if filtered.is_empty() {
            buf.set_line(list_area.x + 1, list_area.y, &Line::from(Span::styled("No results", muted())), list_area.width);
        }
        let _ = bg;
    }

    fn item_line(&self, it: &Item, w: usize, sel: bool) -> Line<'static> {
        let base = if sel { Style::new().bg(PRIMARY).fg(PANEL) } else { Style::new().bg(PANEL).fg(TEXT) };
        let sub = if sel { base } else { base.fg(MUTED) };
        let mut left: Vec<Span<'static>> = vec![Span::styled(if it.current { " ● " } else { "   " }, base.fg(if sel { PANEL } else { it.color.unwrap_or(SUCCESS) }))];
        left.push(Span::styled(it.label.clone(), if sel { base.add_modifier(Modifier::BOLD) } else { base.fg(it.color.unwrap_or(TEXT)) }));
        for (b, c) in &it.badges {
            left.push(Span::styled(" ", base));
            left.push(Span::styled(format!(" {b} "), if sel { Style::new().bg(PANEL).fg(PRIMARY) } else { Style::new().bg(ELEMENT).fg(*c) }));
        }
        if !it.detail.is_empty() {
            left.push(Span::styled(format!("  {}", it.detail), sub));
        }
        let right = if it.right.is_empty() { vec![] } else { vec![Span::styled(format!("{} ", it.right), sub)] };
        let rw: usize = right.iter().map(|s| width(&s.content)).sum();
        let avail = w.saturating_sub(rw + 1);
        let mut out = Vec::new();
        let mut used = 0;
        for s in left {
            let sw = width(&s.content);
            if used + sw <= avail {
                used += sw;
                out.push(s);
            } else {
                let t = truncate(&s.content, avail - used);
                used += width(&t);
                out.push(Span::styled(t, s.style));
                break;
            }
        }
        out.push(Span::styled(" ".repeat(w.saturating_sub(used + rw)), base));
        out.extend(right);
        Line::from(out)
    }
}

/// Slash-command autocomplete popup (drawn above the input).
pub struct Popup {
    pub items: Vec<Item>,
    pub selected: usize,
    pub rect: Rect,
    pub max_rows: usize,
}

impl Popup {
    /// Rows of items shown (limited by the space above the input).
    fn shown(&self) -> usize {
        self.items.len().min(10).min(self.max_rows.max(1))
    }

    pub fn draw(&mut self, buf: &mut Buffer, input_area: Rect) {
        self.max_rows = input_area.y.saturating_sub(2) as usize;
        let n = self.shown();
        if n == 0 || input_area.y < 3 {
            self.rect = Rect::default();
            return;
        }
        let h = n as u16 + 2;
        let rect = Rect { x: input_area.x, y: input_area.y.saturating_sub(h), width: input_area.width, height: h };
        self.rect = rect;
        Clear.render(rect, buf);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(fg(BORDER))
            .style(Style::new().bg(PANEL));
        let inner = block.inner(rect);
        block.render(rect, buf);
        let start = self.selected.saturating_sub(n - 1);
        for (i, it) in self.items.iter().enumerate().skip(start).take(n) {
            let sel = i == self.selected;
            let st = if sel { Style::new().bg(PRIMARY).fg(PANEL) } else { Style::new().bg(PANEL).fg(TEXT) };
            let sub = if sel { st } else { st.fg(MUTED) };
            let name = format!(" {:<16}", it.label);
            let l = Line::from(vec![Span::styled(name, st.add_modifier(Modifier::BOLD)), Span::styled(format!(" {}", it.detail), sub)]);
            let l = pad_line(l, inner.width as usize, st);
            buf.set_line(inner.x, inner.y + (i - start) as u16, &l, inner.width);
        }
    }

    pub fn item_at(&self, x: u16, y: u16) -> Option<usize> {
        let inner_y = self.rect.y + 1;
        if !super::layout::contains(self.rect, x, y) || y < inner_y || y >= self.rect.y + self.rect.height - 1 {
            return None;
        }
        let n = self.shown();
        let start = self.selected.saturating_sub(n - 1);
        Some(start + (y - inner_y) as usize)
    }
}
