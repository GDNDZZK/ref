//! Centered overlay dialogs: fuzzy-filtered lists (palette, sessions, models, agents) and help.

use std::time::Instant;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use super::editor::Editor;
use super::text::{self, width};
use super::theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Palette,
    Sessions,
    /// Live sessions only; enter closes the selected one and the dialog stays open.
    CloseSessions,
    Models,
    Agents,
    Variants,
}

#[derive(Debug, Clone, Default)]
pub struct Item {
    /// Value returned on submit.
    pub key: String,
    pub title: String,
    /// Muted text after the title.
    pub desc: String,
    /// Right-aligned muted text (key hint, time, ...).
    pub right: String,
    pub section: String,
    /// Small badge before `right` (text, colour).
    pub badge: Option<(String, Color)>,
    pub title_color: Option<Color>,
    /// Leading marker (e.g. `●` current).
    pub marker: Option<(String, Color)>,
    /// Show a spinner instead of the marker.
    pub spinner: bool,
    /// Extra search text.
    pub keywords: String,
}

pub struct ListDialog {
    pub kind: Kind,
    pub title: String,
    pub items: Vec<Item>,
    pub filter: Editor,
    /// Indices into `items` after filtering.
    visible: Vec<usize>,
    pub sel: usize,
    scroll: usize,
    pub hint: String,
    pub opened: Instant,
    /// Pending destructive confirmation (item key).
    pub confirm: Option<String>,
    /// Text shown on the confirming row (default: "press ctrl+d again to confirm").
    pub confirm_text: String,
    /// Screen rows of items at last draw: (y, visible index).
    rows: Vec<(u16, usize)>,
    pub rect: Rect,
}

impl ListDialog {
    pub fn new(kind: Kind, title: impl Into<String>, items: Vec<Item>) -> Self {
        let mut d = ListDialog {
            kind,
            title: title.into(),
            items,
            filter: Editor::default(),
            visible: Vec::new(),
            sel: 0,
            scroll: 0,
            hint: String::new(),
            opened: Instant::now(),
            confirm: None,
            confirm_text: String::new(),
            rows: Vec::new(),
            rect: Rect::default(),
        };
        d.refilter();
        d
    }

    pub fn with_hint(mut self, h: impl Into<String>) -> Self {
        self.hint = h.into();
        self
    }

    /// Replaces items, keeping the selected key if possible.
    pub fn set_items(&mut self, items: Vec<Item>) {
        let key = self.selected().map(|i| i.key.clone());
        let old_sel = self.sel;
        let confirm = self.confirm.take();
        self.items = items;
        self.refilter();
        match key.and_then(|k| self.visible.iter().position(|&i| self.items[i].key == k)) {
            Some(p) => {
                self.sel = p;
                // A pending confirmation survives live refreshes while its item is still selected.
                self.confirm = confirm.filter(|c| self.items[self.visible[p]].key == *c);
            }
            // Selected item vanished (e.g. just closed): stay at the same position.
            None => self.sel = old_sel.min(self.visible.len().saturating_sub(1)),
        }
    }

    pub fn select_key(&mut self, key: &str) {
        if let Some(p) = self.visible.iter().position(|&i| self.items[i].key == key) {
            self.sel = p;
        }
    }

    pub fn refilter(&mut self) {
        let q = self.filter.text.trim().to_string();
        if q.is_empty() {
            self.visible = (0..self.items.len()).collect();
        } else {
            let mut scored: Vec<(i64, usize)> = self
                .items
                .iter()
                .enumerate()
                .filter_map(|(i, it)| {
                    let a = text::fuzzy_score(&it.title, &q).map(|s| s + 20);
                    let b = text::fuzzy_score(
                        &format!("{} {} {} {}", it.title, it.desc, it.section, it.keywords),
                        &q,
                    );
                    a.max(b).map(|s| (s, i))
                })
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            self.visible = scored.into_iter().map(|x| x.1).collect();
        }
        self.sel = self.sel.min(self.visible.len().saturating_sub(1));
        if !q.is_empty() {
            self.sel = 0;
        }
        self.confirm = None;
    }

    pub fn selected(&self) -> Option<&Item> {
        self.visible.get(self.sel).and_then(|&i| self.items.get(i))
    }

    pub fn move_sel(&mut self, d: i32) {
        let n = self.visible.len();
        if n == 0 {
            return;
        }
        let s = (self.sel as i32 + d).rem_euclid(n as i32);
        self.sel = s as usize;
        self.confirm = None;
    }

    pub fn page(&mut self, d: i32) {
        let n = self.visible.len() as i32;
        if n == 0 {
            return;
        }
        self.sel = (self.sel as i32 + d * 10).clamp(0, n - 1) as usize;
    }

    /// Item index (in visible list) at a screen row.
    pub fn item_at(&self, x: u16, y: u16) -> Option<usize> {
        if x < self.rect.x || x >= self.rect.x + self.rect.width {
            return None;
        }
        self.rows.iter().find(|(ry, _)| *ry == y).map(|(_, i)| *i)
    }

    pub fn click(&mut self, x: u16, y: u16) -> bool {
        if let Some(i) = self.item_at(x, y) {
            self.sel = i;
            return true;
        }
        false
    }

    pub fn scroll(&mut self, d: i32) {
        self.move_sel(d);
    }

    pub fn draw(&mut self, buf: &mut Buffer, screen: Rect, tick: u64) {
        let w = (screen.width.saturating_sub(4)).clamp(30, 84);
        let h = screen.height.saturating_sub(4).clamp(8, 28);
        let t = (self.opened.elapsed().as_secs_f32() / 0.12).min(1.0);
        let r = frame(buf, screen, w, h, &self.title, t);
        self.rect = r;
        let inner = Rect::new(
            r.x + 2,
            r.y + 1,
            r.width.saturating_sub(4),
            r.height.saturating_sub(2),
        );
        let bg = theme::PANEL;
        let fade = |c: Color| theme::blend(bg, c, t);
        // Filter line.
        let fy = inner.y + 1;
        buf.set_string(
            inner.x,
            fy,
            "› ",
            Style::new().fg(fade(theme::PRIMARY)).bg(bg),
        );
        let farea = Rect::new(inner.x + 2, fy, inner.width.saturating_sub(2), 1);
        self.filter.draw(buf, farea, "Search…", false);
        // Fake cursor in the filter (real cursor handled by caller).
        let list_top = fy + 2;
        let list_h = (inner.y + inner.height).saturating_sub(list_top + 1) as usize;
        // Build display rows: section headers + items.
        enum Disp {
            Gap,
            Section(String),
            Item(usize),
        }
        let mut disp: Vec<Disp> = Vec::new();
        let filtering = !self.filter.text.trim().is_empty();
        let mut last_section: Option<&str> = None;
        for (vi, &ii) in self.visible.iter().enumerate() {
            let it = &self.items[ii];
            if !filtering && !it.section.is_empty() && last_section != Some(it.section.as_str()) {
                if last_section.is_some() {
                    disp.push(Disp::Gap);
                }
                disp.push(Disp::Section(it.section.clone()));
                last_section = Some(it.section.as_str());
            }
            disp.push(Disp::Item(vi));
        }
        let sel_row = disp
            .iter()
            .position(|d| matches!(d, Disp::Item(v) if *v == self.sel))
            .unwrap_or(0);
        if sel_row < self.scroll {
            self.scroll = sel_row.saturating_sub(1);
        }
        if sel_row >= self.scroll + list_h {
            self.scroll = sel_row + 1 - list_h;
        }
        self.scroll = self.scroll.min(disp.len().saturating_sub(list_h));
        self.rows.clear();
        if self.visible.is_empty() {
            buf.set_string(
                inner.x,
                list_top,
                "No results",
                Style::new().fg(fade(theme::MUTED)).bg(bg),
            );
        }
        let iw = inner.width as usize;
        for (k, d) in disp.iter().enumerate().skip(self.scroll).take(list_h) {
            let y = list_top + (k - self.scroll) as u16;
            match d {
                Disp::Gap => {}
                Disp::Section(s) => {
                    buf.set_string(
                        inner.x,
                        y,
                        text::truncate(s, iw),
                        Style::new()
                            .fg(fade(theme::ACCENT))
                            .bg(bg)
                            .add_modifier(Modifier::BOLD),
                    );
                }
                Disp::Item(vi) => {
                    let it = &self.items[self.visible[*vi]];
                    let selected = *vi == self.sel;
                    let rbg = if selected { theme::PRIMARY } else { bg };
                    let fgc = |c: Color| if selected { theme::PANEL } else { fade(c) };
                    let row = Rect::new(inner.x.saturating_sub(1), y, inner.width + 2, 1);
                    buf.set_style(row, Style::new().bg(rbg));
                    let mut left: Vec<Span<'static>> = vec![Span::raw(" ")];
                    if it.spinner {
                        left.push(Span::styled(
                            format!("{} ", theme::spinner(tick)),
                            Style::new().fg(fgc(theme::PRIMARY)),
                        ));
                    } else if let Some((m, c)) = &it.marker {
                        left.push(Span::styled(format!("{m} "), Style::new().fg(fgc(*c))));
                    }
                    let tstyle = Style::new().fg(fgc(it.title_color.unwrap_or(theme::TEXT)));
                    left.push(Span::styled(
                        it.title.clone(),
                        if selected {
                            tstyle.add_modifier(Modifier::BOLD)
                        } else {
                            tstyle
                        },
                    ));
                    // Section headers are hidden while filtering; keep the provider visible for
                    // models (the same model id often exists under several providers).
                    if filtering && self.kind == Kind::Models && !it.section.is_empty() {
                        left.push(Span::styled(
                            format!("  {}", it.section),
                            Style::new().fg(fgc(theme::ACCENT)),
                        ));
                    }
                    if !it.desc.is_empty() {
                        left.push(Span::styled(
                            format!("  {}", it.desc),
                            Style::new().fg(fgc(theme::MUTED)),
                        ));
                    }
                    let mut right: Vec<Span<'static>> = Vec::new();
                    if self.confirm.as_deref() == Some(it.key.as_str()) && selected {
                        let t = if self.confirm_text.is_empty() {
                            "press ctrl+d again to confirm "
                        } else {
                            self.confirm_text.as_str()
                        };
                        right.push(Span::styled(
                            t.to_string(),
                            Style::new().fg(theme::PANEL).add_modifier(Modifier::BOLD),
                        ));
                    } else {
                        if let Some((b, c)) = &it.badge {
                            right.push(Span::styled(
                                format!(" {b} "),
                                Style::new()
                                    .fg(if selected { *c } else { theme::PANEL })
                                    .bg(if selected { theme::PANEL } else { fade(*c) }),
                            ));
                            right.push(Span::raw(" "));
                        }
                        if !it.right.is_empty() {
                            right.push(Span::styled(
                                it.right.clone(),
                                Style::new().fg(fgc(theme::MUTED)),
                            ));
                        }
                        right.push(Span::raw(" "));
                    }
                    // Keep the title readable: shrink the right column first (to at most ~half).
                    let lw: usize = left.iter().map(|s| width(&s.content)).sum();
                    let rw: usize = right.iter().map(|s| width(&s.content)).sum();
                    let room = (iw + 2).saturating_sub(lw + 1);
                    let cap = room.max((iw + 2) / 2);
                    if rw > cap {
                        right = shrink_right(right, cap);
                    }
                    let (line, _) = super::message::compose(left, right, iw + 2);
                    buf.set_line(inner.x.saturating_sub(1), y, &line, inner.width + 2);
                    self.rows.push((y, *vi));
                }
            }
        }
        // Footer hint.
        let hint = if self.hint.is_empty() {
            "↑↓ select · enter confirm · esc close".to_string()
        } else {
            self.hint.clone()
        };
        let hy = inner.y + inner.height.saturating_sub(1);
        let count = format!("{}/{}", self.visible.len(), self.items.len());
        buf.set_string(
            inner.x,
            hy,
            text::truncate(&hint, iw.saturating_sub(width(&count) + 2)),
            Style::new().fg(fade(theme::DIM)).bg(bg),
        );
        buf.set_string(
            inner.x + inner.width.saturating_sub(width(&count) as u16),
            hy,
            &count,
            Style::new().fg(fade(theme::DIM)).bg(bg),
        );
    }

    /// Cursor position for the filter input.
    pub fn cursor(&self) -> (u16, u16) {
        let x = self.rect.x + 4 + width(&self.filter.text[..self.filter.cursor]) as u16;
        (
            x.min(self.rect.x + self.rect.width.saturating_sub(3)),
            self.rect.y + 2,
        )
    }
}

/// Truncates the last (longest, free-text) span of a right-aligned group so it fits `max`.
fn shrink_right(mut spans: Vec<Span<'static>>, max: usize) -> Vec<Span<'static>> {
    let total: usize = spans.iter().map(|s| width(&s.content)).sum();
    if total <= max {
        return spans;
    }
    // Trailing pad span (" ") stays; truncate the text span before it.
    let idx = spans.iter().rposition(|s| width(&s.content) > 1);
    if let Some(i) = idx {
        let w = width(&spans[i].content);
        let keep = w.saturating_sub(total - max);
        if keep >= 2 {
            let st = spans[i].style;
            spans[i] = Span::styled(text::truncate(&spans[i].content, keep), st);
            return spans;
        }
        spans.remove(i);
    }
    spans
}

/// Draws a centered rounded panel (with open animation `t` 0→1); returns its rect.
pub fn frame(buf: &mut Buffer, screen: Rect, w: u16, h: u16, title: &str, t: f32) -> Rect {
    // Slight grow-in: start 2 cols/1 row smaller.
    let shrink = ((1.0 - t) * 2.0).round() as u16;
    let w = w.min(screen.width).saturating_sub(shrink * 2);
    let h = h.min(screen.height).saturating_sub(shrink);
    let x = screen.x + (screen.width.saturating_sub(w)) / 2;
    let y = screen.y + (screen.height.saturating_sub(h)) / 3;
    let r = Rect::new(x, y, w, h);
    // Dim the background a little.
    dim(buf, screen, 0.35 * t);
    clear_wide_edges(buf, r);
    let bg = theme::PANEL;
    let bc = theme::blend(bg, theme::BORDER_ACTIVE, t);
    for yy in r.y..r.y + r.height {
        for xx in r.x..r.x + r.width {
            if let Some(c) = buf.cell_mut((xx, yy)) {
                c.reset();
                c.set_style(Style::new().bg(bg));
            }
        }
    }
    let bst = Style::new().fg(bc).bg(bg);
    for xx in r.x + 1..r.x + r.width.saturating_sub(1) {
        buf.set_string(xx, r.y, "─", bst);
        buf.set_string(xx, r.y + r.height - 1, "─", bst);
    }
    for yy in r.y + 1..r.y + r.height.saturating_sub(1) {
        buf.set_string(r.x, yy, "│", bst);
        buf.set_string(r.x + r.width - 1, yy, "│", bst);
    }
    buf.set_string(r.x, r.y, "╭", bst);
    buf.set_string(r.x + r.width - 1, r.y, "╮", bst);
    buf.set_string(r.x, r.y + r.height - 1, "╰", bst);
    buf.set_string(r.x + r.width - 1, r.y + r.height - 1, "╯", bst);
    let title = format!(" {title} ");
    buf.set_string(
        r.x + 2,
        r.y,
        text::truncate(&title, w.saturating_sub(4) as usize),
        Style::new()
            .fg(theme::blend(bg, theme::TEXT, t))
            .bg(bg)
            .add_modifier(Modifier::BOLD),
    );
    let esc = " esc ";
    if w > 20 {
        buf.set_string(
            r.x + r.width - 2 - esc.len() as u16,
            r.y,
            esc,
            Style::new().fg(theme::blend(bg, theme::DIM, t)).bg(bg),
        );
    }
    r
}

/// Before drawing an overlay over `r`: replace wide glyphs (CJK, emoji) that start just left
/// of the overlay and would spill into its first column. Without this the terminal diff skips
/// the overlay's left border cell (it is "covered" by the wide glyph) and background text
/// bleeds through.
pub fn clear_wide_edges(buf: &mut Buffer, r: Rect) {
    if r.x == 0 {
        return;
    }
    for yy in r.y..r.y.saturating_add(r.height) {
        if let Some(c) = buf.cell_mut((r.x - 1, yy))
            && unicode_width::UnicodeWidthStr::width(c.symbol()) > 1
        {
            c.set_symbol(" ");
        }
    }
}

/// Darkens every cell's colours by `amount` (0..1).
pub fn dim(buf: &mut Buffer, area: Rect, amount: f32) {
    if amount <= 0.0 {
        return;
    }
    let dark = Color::Rgb(0, 0, 0);
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            if let Some(c) = buf.cell_mut((x, y)) {
                let fg = match c.fg {
                    Color::Reset => theme::TEXT,
                    f => f,
                };
                c.fg = theme::blend(fg, dark, amount * 1.4);
                if let Color::Rgb(..) = c.bg {
                    c.bg = theme::blend(c.bg, dark, amount);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Help
// ---------------------------------------------------------------------------

pub struct HelpDialog {
    pub scroll: usize,
    pub opened: Instant,
    lines: Vec<Line<'static>>,
    max_scroll: usize,
    title: String,
    /// Width hint (widest line + padding), capped at 80 when drawing.
    width: u16,
}

pub const KEYBINDS: &[(&str, &[(&str, &str)])] = &[
    (
        "General",
        &[
            ("ctrl+p", "command palette"),
            ("ctrl+x", "leader key (then a second key)"),
            ("esc esc", "interrupt the running turn"),
            ("ctrl+c", "copy selection / clear prompt / exit (twice)"),
            ("ctrl+d", "exit (empty prompt)"),
            ("tab / shift+tab", "cycle agent"),
            ("ctrl+t", "cycle model variant"),
            ("ctrl+v", "paste image or text from clipboard"),
        ],
    ),
    (
        "Leader (ctrl+x …)",
        &[
            ("n", "new session"),
            (
                "l",
                "session list (live + history; ctrl+d closes / deletes)",
            ),
            ("m / a", "models / agents"),
            ("t / d", "toggle thinking / tool details"),
            ("c / u", "compact / undo last prompt"),
            ("k (twice)", "close this session (split view: focused tile)"),
            ("e", "compose in $EDITOR"),
            ("x", "export transcript"),
            ("y", "copy last assistant message"),
            ("w", "split view of background sessions"),
            ("b", "toggle sidebar"),
            ("h / q", "help / quit"),
        ],
    ),
    (
        "Transcript",
        &[
            ("pgup / pgdn", "scroll a page"),
            ("ctrl+alt+u / d", "half page"),
            ("shift+↑ / ↓", "scroll a line"),
            ("home / end", "top / bottom (empty prompt)"),
            ("mouse drag", "select text (never auto-copied)"),
            ("double / triple click", "select word / line"),
            ("ctrl+c, ctrl+shift+c, y", "copy selection"),
            ("right click", "copy selection"),
            ("click ▸ line", "expand / collapse tool or thinking"),
            ("click ⧉ copy", "copy raw markdown of a message"),
            ("click ↶ undo", "remove a prompt and everything after it"),
            (
                "click ✕ remove",
                "withdraw a queued prompt before it is sent",
            ),
        ],
    ),
    (
        "Prompt",
        &[
            ("enter", "send"),
            ("shift+enter, alt+enter, ctrl+j", "newline"),
            ("↑ / ↓", "history (at first / last line)"),
            ("ctrl+a / ctrl+e", "line start / end"),
            ("alt+b / alt+f, ctrl+←/→", "word left / right"),
            ("ctrl+w / alt+d", "delete word back / forward"),
            ("ctrl+u / ctrl+k", "delete to line start / end"),
            ("shift+arrows", "select; ctrl+c copies"),
            ("ctrl+z / ctrl+y", "undo / redo"),
            ("/", "slash commands"),
            ("@", "mention a file"),
            ("!", "shell mode"),
        ],
    ),
    (
        "Split view",
        &[
            ("click / tab / alt+arrows", "focus a tile"),
            ("enter", "send to focused session"),
            ("esc", "clear input / unfocus the tile"),
            ("esc esc", "abort focused session (busy)"),
            (
                "wheel, pgup / pgdn, shift+↑↓",
                "scroll a tile (end: back to bottom)",
            ),
            ("enter on empty, ctrl+o", "zoom (attach as foreground)"),
            ("ctrl+n / ctrl+b", "next / previous page"),
            ("ctrl+x k (twice)", "close the focused session"),
            ("ctrl+x w", "back to chat"),
        ],
    ),
];

impl HelpDialog {
    pub fn new(commands: &[(String, String)]) -> Self {
        let mut lines = Vec::new();
        for (sec, items) in KEYBINDS {
            lines.push(Line::from(Span::styled(
                sec.to_string(),
                theme::bold(theme::ACCENT),
            )));
            for (k, d) in *items {
                lines.push(Line::from(vec![
                    Span::styled(format!("  {:<30}", k), theme::fg(theme::PRIMARY)),
                    Span::styled(d.to_string(), theme::text()),
                ]));
            }
            lines.push(Line::default());
        }
        lines.push(Line::from(Span::styled(
            "Slash commands",
            theme::bold(theme::ACCENT),
        )));
        for (n, d) in commands {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {:<30}", format!("/{n}")),
                    theme::fg(theme::PRIMARY),
                ),
                Span::styled(d.clone(), theme::text()),
            ]));
        }
        HelpDialog {
            scroll: 0,
            opened: Instant::now(),
            lines,
            max_scroll: 0,
            title: "Help".into(),
            width: 80,
        }
    }

    /// Read-only text viewer (command results). Long lines are wrapped when drawn.
    pub fn text(title: &str, body: &str) -> Self {
        let lines: Vec<Line<'static>> = body
            .lines()
            .map(|l| Line::from(Span::styled(l.replace('\t', "    "), theme::text())))
            .collect();
        let widest = body.lines().map(width).max().unwrap_or(0) as u16;
        HelpDialog {
            scroll: 0,
            opened: Instant::now(),
            lines,
            max_scroll: 0,
            title: title.to_string(),
            width: (widest + 6).max(40),
        }
    }

    pub fn scroll_by(&mut self, d: i32) {
        self.scroll = (self.scroll as i32 + d).clamp(0, self.max_scroll as i32) as usize;
    }

    pub fn draw(&mut self, buf: &mut Buffer, screen: Rect) {
        let t = (self.opened.elapsed().as_secs_f32() / 0.12).min(1.0);
        let w = screen
            .width
            .saturating_sub(4)
            .clamp(30, self.width.min(110));
        let iw = w.saturating_sub(4) as usize;
        let wrapped: Vec<Line<'static>> = self
            .lines
            .iter()
            .flat_map(|l| {
                let rows = text::wrap_spans(&l.spans, iw.max(1));
                if rows.is_empty() {
                    vec![Line::default()]
                } else {
                    rows
                }
            })
            .collect();
        let h = (wrapped.len() as u16 + 2).clamp(3, screen.height.saturating_sub(4).clamp(3, 40));
        let r = frame(buf, screen, w, h, &self.title, t);
        let inner = Rect::new(
            r.x + 2,
            r.y + 1,
            r.width.saturating_sub(4),
            r.height.saturating_sub(2),
        );
        let vh = inner.height as usize;
        self.max_scroll = wrapped.len().saturating_sub(vh);
        self.scroll = self.scroll.min(self.max_scroll);
        for (i, l) in wrapped.iter().skip(self.scroll).take(vh).enumerate() {
            let mut l = l.clone();
            for s in &mut l.spans {
                s.style = s.style.bg(theme::PANEL);
                if let Some(c) = s.style.fg {
                    s.style.fg = Some(theme::blend(theme::PANEL, c, t));
                }
            }
            buf.set_line(inner.x, inner.y + i as u16, &l, inner.width);
        }
    }
}

#[allow(clippy::large_enum_variant)]
pub enum Dialog {
    List(ListDialog),
    Help(HelpDialog),
    Mcp(super::mcp::McpDialog),
}

impl Dialog {
    pub fn animating(&self) -> bool {
        let o = match self {
            Dialog::List(d) => d.opened,
            Dialog::Help(d) => d.opened,
            Dialog::Mcp(d) => d.opened,
        };
        o.elapsed().as_secs_f32() < 0.15
    }
}
