//! Centered overlay dialogs: fuzzy-filtered lists (palette, sessions, models, agents) and help.

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use super::editor::Editor;
use super::theme;
use crate::text::{self, width};

/// Score bonus when the filter matches the title itself.
const TITLE_MATCH_BONUS: i64 = 20;
/// Rows moved by one page key in a list or text dialog.
pub const PAGE_ROWS: i32 = 10;
/// The dialog frame starts this many columns smaller and grows in.
const GROW_IN_SHRINK: f32 = 2.0;
/// How strongly the backdrop is darkened behind a dialog.
const BACKDROP_DIM: f32 = 0.35;
/// Foregrounds darken faster than backgrounds by this factor.
const FG_DIM_FACTOR: f32 = 1.4;
/// The " esc " hint is drawn only in frames wider than this.
const MIN_W_FOR_ESC_HINT: u16 = 20;
/// Width of the key column in the help text.
const KEY_COLUMN_W: usize = 30;

/// Length of the open animation, in seconds.
const OPEN_ANIM_SECS: f32 = 0.12;
/// How long after opening a dialog still needs redraws (the animation plus a margin).
pub const OPEN_SETTLE: Duration = Duration::from_millis(150);

/// Open animation progress of an overlay opened at `opened`, from 0 to 1.
pub fn open_progress(opened: Instant) -> f32 {
    (opened.elapsed().as_secs_f32() / OPEN_ANIM_SECS).min(1.0)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Palette,
    Sessions,
    /// Live sessions only; enter closes the selected one and the dialog stays open.
    CloseSessions,
    Models,
    Agents,
    Variants,
    /// Interactive directory picker for `/cd`.
    Cd,
}

/// Inline argument prompt inside the command palette (`Esc` returns to the command list).
#[derive(Debug, Clone)]
pub struct ArgPrompt {
    pub cmd_key: String,
    pub description: String,
    pub placeholder: String,
    pub saved_filter: String,
    pub saved_sel: usize,
    pub saved_scroll: usize,
    pub saved_title: String,
    pub saved_hint: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CreatorFilter {
    #[default]
    All,
    UserCreated,
    ByCreator {
        id: String,
        label: String,
    },
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
    /// Creator `(parent_id, creator_name)` for child sessions.
    pub creator: Option<(String, String)>,
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
    pub creator_filter: CreatorFilter,
    /// Creator remembered when cycling `All -> UserCreated` so `-> ByCreator` keeps the choice.
    pending_creator: Option<(String, String)>,
    /// Target session when opened from a focused split-view tile or explicit target.
    pub target_sid: Option<String>,
    /// Active inline argument prompt in `Kind::Palette`.
    pub asking_arg: Option<ArgPrompt>,
    /// Base session working directory for `Kind::Cd`.
    pub cd_base_cwd: String,
    /// Latest resolved directory path in `Kind::Cd`.
    pub cd_resolved_dir: String,
    filter_rect: Rect,
    cursor_at: Option<(u16, u16)>,
}

/// A display row of the list: a section header, a gap between sections, or a visible item.
enum Disp {
    Gap,
    Section(String),
    Item(usize),
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
            creator_filter: CreatorFilter::All,
            pending_creator: None,
            target_sid: None,
            asking_arg: None,
            cd_base_cwd: String::new(),
            cd_resolved_dir: String::new(),
            filter_rect: Rect::default(),
            cursor_at: None,
        };
        d.refilter();
        d
    }

    pub fn for_session(mut self, sid: Option<String>) -> Self {
        self.target_sid = sid;
        self
    }

    /// Enters inline argument prompt mode inside `Kind::Palette`.
    pub fn begin_arg_prompt(
        &mut self,
        cmd_key: String,
        title: String,
        description: String,
        placeholder: String,
    ) {
        let saved_filter = self.filter.text.clone();
        let saved_sel = self.sel;
        let saved_scroll = self.scroll;
        let saved_title = std::mem::replace(&mut self.title, title);
        let saved_hint =
            std::mem::replace(&mut self.hint, "enter run · esc back to commands".into());
        self.asking_arg = Some(ArgPrompt {
            cmd_key,
            description,
            placeholder,
            saved_filter,
            saved_sel,
            saved_scroll,
            saved_title,
            saved_hint,
        });
        self.filter.clear();
        self.visible.clear();
        self.rows.clear();
    }

    /// Restores the palette command list if currently asking for an argument; returns true if popped.
    pub fn cancel_arg_prompt(&mut self) -> bool {
        let Some(p) = self.asking_arg.take() else {
            return false;
        };
        self.title = p.saved_title;
        self.hint = p.saved_hint;
        self.filter.set_text(&p.saved_filter);
        self.refilter();
        self.sel = p.saved_sel.min(self.visible.len().saturating_sub(1));
        self.scroll = p.saved_scroll;
        true
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

    pub fn cycle_creator_filter(&mut self) {
        if !matches!(self.kind, Kind::Sessions | Kind::CloseSessions) {
            return;
        }
        match &self.creator_filter {
            CreatorFilter::All => {
                self.pending_creator = self.selected().and_then(|it| it.creator.clone());
                self.creator_filter = CreatorFilter::UserCreated;
            }
            CreatorFilter::UserCreated => {
                let target = self
                    .selected()
                    .and_then(|it| it.creator.clone())
                    .or_else(|| self.pending_creator.take());
                self.creator_filter = match target {
                    Some((id, label)) => CreatorFilter::ByCreator { id, label },
                    None => CreatorFilter::All,
                };
            }
            CreatorFilter::ByCreator { .. } => {
                self.pending_creator = None;
                self.creator_filter = CreatorFilter::All;
            }
        }
        self.refilter();
    }

    fn matches_creator(&self, it: &Item) -> bool {
        match &self.creator_filter {
            CreatorFilter::All => true,
            CreatorFilter::UserCreated => it.creator.is_none(),
            CreatorFilter::ByCreator { id, .. } => {
                it.creator.as_ref().is_some_and(|(cid, _)| cid == id)
            }
        }
    }

    fn creator_pill(&self) -> Option<(String, bool)> {
        if !matches!(self.kind, Kind::Sessions | Kind::CloseSessions) {
            return None;
        }
        Some(match &self.creator_filter {
            CreatorFilter::All => (" All ".to_string(), false),
            CreatorFilter::UserCreated => (" User-created ".to_string(), true),
            CreatorFilter::ByCreator { label, .. } => {
                (format!(" By {} ", text::truncate(label, 16)), true)
            }
        })
    }

    pub fn refilter(&mut self) {
        if self.asking_arg.is_some() {
            self.visible.clear();
            self.sel = 0;
            self.confirm = None;
            return;
        }
        if self.kind == Kind::Cd {
            self.visible = (0..self.items.len()).collect();
            self.sel = self.sel.min(self.visible.len().saturating_sub(1));
            self.confirm = None;
            return;
        }
        let q = self.filter.text.trim().to_string();
        if q.is_empty() {
            self.visible = self
                .items
                .iter()
                .enumerate()
                .filter(|(_, it)| self.matches_creator(it))
                .map(|(i, _)| i)
                .collect();
        } else {
            let mut scored: Vec<(i64, usize)> = self
                .items
                .iter()
                .enumerate()
                .filter(|(_, it)| self.matches_creator(it))
                .filter_map(|(i, it)| {
                    let a = text::fuzzy_score(&it.title, &q).map(|s| s + TITLE_MATCH_BONUS);
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
        self.sel = (self.sel as i32 + d * PAGE_ROWS).clamp(0, n - 1) as usize;
    }

    /// Item index (in visible list) at a screen row.
    pub fn item_at(&self, x: u16, y: u16) -> Option<usize> {
        if x < self.rect.x || x >= self.rect.x + self.rect.width {
            return None;
        }
        self.rows.iter().find(|(ry, _)| *ry == y).map(|(_, i)| *i)
    }

    pub fn click_filter(&mut self, x: u16, y: u16) -> bool {
        let r = self.filter_rect;
        if r.width == 0 || y != r.y || x < r.x || x >= r.x + r.width {
            return false;
        }
        let off = self.filter.offset_at_single_line(r, x);
        self.filter.mouse_down(off, 1);
        self.filter.mouse_up();
        true
    }

    pub fn click(&mut self, x: u16, y: u16) -> bool {
        if self.click_filter(x, y) {
            return false;
        }
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
        self.rows.clear();
        self.cursor_at = None;
        self.filter_rect = Rect::default();
        if screen.width < 6 || screen.height < 4 {
            self.rect = Rect::default();
            return;
        }
        let w = screen
            .width
            .saturating_sub(4)
            .clamp(6, 84)
            .min(screen.width);
        let h = if self.asking_arg.is_some() {
            screen
                .height
                .saturating_sub(4)
                .clamp(4, 9)
                .min(screen.height)
        } else {
            screen
                .height
                .saturating_sub(4)
                .clamp(4, 28)
                .min(screen.height)
        };
        let t = open_progress(self.opened);
        let r = frame(buf, screen, w, h, &self.title, t);
        self.rect = r;
        if r.width < 6 || r.height < 4 {
            return;
        }
        let inner = Rect::new(
            r.x + 2,
            r.y + 1,
            r.width.saturating_sub(4),
            r.height.saturating_sub(2),
        );
        let bg = theme::PANEL;
        let fade = |c: Color| theme::blend(bg, c, t);
        // Filter / argument input line.
        let fy = if inner.height >= 4 {
            inner.y + 1
        } else {
            inner.y
        };
        if inner.width >= 2 {
            buf.set_string(
                inner.x,
                fy,
                "› ",
                Style::new().fg(fade(theme::PRIMARY)).bg(bg),
            );
        }
        let pill = if self.asking_arg.is_none() {
            self.creator_pill()
        } else {
            None
        };
        let pill_w = pill.as_ref().map_or(0, |(l, _)| width(l) as u16 + 1);
        let farea = Rect::new(
            inner.x.saturating_add(2),
            fy,
            inner.width.saturating_sub(2 + pill_w),
            1,
        );
        self.filter_rect = farea;
        let placeholder = match (&self.asking_arg, &self.kind) {
            (Some(p), _) => p.placeholder.as_str(),
            (None, Kind::Cd) => "Type a directory path…",
            (None, _) => "Search…",
        };
        self.cursor_at = self
            .filter
            .draw_single_line(buf, farea, Some(bg), placeholder, true);
        if let Some((label, active)) = pill {
            let lw = width(&label) as u16;
            if lw <= inner.width {
                let px = inner.x + inner.width - lw;
                let st = if active {
                    Style::new()
                        .fg(theme::PANEL)
                        .bg(fade(theme::ACCENT))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::new().fg(fade(theme::MUTED)).bg(theme::ELEMENT)
                };
                buf.set_string(px, fy, &label, st);
            }
        }
        if let Some(arg) = &self.asking_arg {
            let desc_y = fy + 2;
            if desc_y < inner.y + inner.height.saturating_sub(1) && !arg.description.is_empty() {
                buf.set_string(
                    inner.x,
                    desc_y,
                    text::truncate(&arg.description, inner.width as usize),
                    Style::new().fg(fade(theme::MUTED)).bg(bg),
                );
            }
            self.draw_footer(buf, inner, t);
            return;
        }
        let list_top = fy.saturating_add(2);
        let list_h = (inner.y + inner.height).saturating_sub(list_top.saturating_add(1)) as usize;
        let disp = self.display_rows();
        let sel_row = disp
            .iter()
            .position(|d| matches!(d, Disp::Item(v) if *v == self.sel))
            .unwrap_or(0);
        if sel_row < self.scroll {
            self.scroll = sel_row.saturating_sub(1);
        }
        if list_h > 0 && sel_row >= self.scroll + list_h {
            self.scroll = sel_row + 1 - list_h;
        }
        let max_scroll = disp.len().saturating_sub(list_h);
        self.scroll = self.scroll.min(max_scroll);
        if self.visible.is_empty() && list_h > 0 && inner.width > 0 {
            buf.set_string(
                inner.x,
                list_top,
                text::truncate("No results", inner.width as usize),
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
                    let selected = *vi == self.sel;
                    let rbg = if selected { theme::PRIMARY } else { bg };
                    let row_w = (inner.width + 2).min(r.width.saturating_sub(2));
                    let row = Rect::new(inner.x.saturating_sub(1), y, row_w, 1);
                    buf.set_style(row, Style::new().bg(rbg));
                    let (left, right) = self.item_row_spans(*vi, t, tick);
                    let (line, _) = super::message::compose(left, right, row_w as usize);
                    buf.set_line(inner.x.saturating_sub(1), y, &line, row_w);
                    self.rows.push((y, *vi));
                }
            }
        }
        if max_scroll > 0 && list_h > 0 {
            super::chat::draw_scrollbar(
                buf,
                Rect::new(r.x + r.width - 2, list_top, 1, list_h as u16),
                disp.len(),
                self.scroll as f64,
                max_scroll as f64,
                ("┃", theme::BORDER_ACTIVE),
                Some(("│", theme::BORDER_SUBTLE)),
            );
        }
        self.draw_footer(buf, inner, t);
    }
    /// Display rows of the visible items: section headers (hidden while filtering) and gaps.
    fn display_rows(&self) -> Vec<Disp> {
        let mut disp = Vec::new();
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
        disp
    }

    /// Left and right spans of the visible item `vi`, faded in by the open progress `t`.
    fn item_row_spans(
        &self,
        vi: usize,
        t: f32,
        tick: u64,
    ) -> (Vec<Span<'static>>, Vec<Span<'static>>) {
        let it = &self.items[self.visible[vi]];
        let selected = vi == self.sel;
        let filtering = !self.filter.text.trim().is_empty();
        let fade = |c: Color| theme::blend(theme::PANEL, c, t);
        let fgc = |c: Color| if selected { theme::PANEL } else { fade(c) };
        let iw = self.rect.width.saturating_sub(4) as usize;
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
            if let Some((_, cname)) = &it.creator {
                let label = format!(" ⟲ {} ", text::truncate(cname, 16));
                right.push(Span::styled(
                    label,
                    Style::new()
                        .fg(if selected {
                            theme::ACCENT
                        } else {
                            theme::PANEL
                        })
                        .bg(if selected {
                            theme::PANEL
                        } else {
                            fade(theme::ACCENT)
                        }),
                ));
                right.push(Span::raw(" "));
            }
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
        let lw: usize = text::spans_width(&left);
        let rw: usize = text::spans_width(&right);
        let room = (iw + 2).saturating_sub(lw + 1);
        let cap = room.max((iw + 2) / 2);
        if rw > cap {
            right = shrink_right(right, cap);
        }
        (left, right)
    }

    /// Footer: the key hint on the left, the visible/total count on the right.
    fn draw_footer(&self, buf: &mut Buffer, inner: Rect, t: f32) {
        if inner.height == 0 || inner.width == 0 {
            return;
        }
        let bg = theme::PANEL;
        let iw = inner.width as usize;
        let hint = if self.hint.is_empty() {
            "↑↓ select · enter confirm · esc close"
        } else {
            self.hint.as_str()
        };
        let hy = inner.y + inner.height - 1;
        let style = Style::new().fg(theme::blend(bg, theme::DIM, t)).bg(bg);
        if self.asking_arg.is_some() {
            buf.set_string(inner.x, hy, text::truncate(hint, iw), style);
            return;
        }
        let count = format!("{}/{}", self.visible.len(), self.items.len());
        let cw = width(&count);
        let hint_max = if cw + 2 < iw { iw - cw - 2 } else { iw };
        buf.set_string(inner.x, hy, text::truncate(hint, hint_max), style);
        if cw + 2 < iw {
            buf.set_string(inner.x + inner.width - cw as u16, hy, &count, style);
        }
    }

    /// Cursor position for the filter / argument input.
    pub fn cursor(&self) -> Option<(u16, u16)> {
        self.cursor_at
    }
}

/// Truncates the last (longest, free-text) span of a right-aligned group so it fits `max`.
fn shrink_right(mut spans: Vec<Span<'static>>, max: usize) -> Vec<Span<'static>> {
    let total: usize = text::spans_width(&spans);
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
    if screen.width < 2 || screen.height < 2 {
        return Rect::default();
    }
    // Slight grow-in: starts smaller by up to `GROW_IN_SHRINK` columns.
    let shrink = ((1.0 - t) * GROW_IN_SHRINK).round() as u16;
    let w = w
        .min(screen.width)
        .saturating_sub(shrink * 2)
        .max(2)
        .min(screen.width);
    let h = h
        .min(screen.height)
        .saturating_sub(shrink)
        .max(2)
        .min(screen.height);
    let x = screen.x + (screen.width.saturating_sub(w)) / 2;
    let y = screen.y + (screen.height.saturating_sub(h)) / 3;
    let r = Rect::new(x, y, w, h);
    dim(buf, screen, BACKDROP_DIM * t);
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
    draw_rounded_border(buf, r, Style::new().fg(bc).bg(bg));
    if r.width > 4 {
        let title = format!(" {title} ");
        buf.set_string(
            r.x + 2,
            r.y,
            text::truncate(&title, r.width.saturating_sub(4) as usize),
            Style::new()
                .fg(theme::blend(bg, theme::TEXT, t))
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        );
    }
    let esc = " esc ";
    if r.width > MIN_W_FOR_ESC_HINT {
        buf.set_string(
            r.x + r.width - 2 - esc.len() as u16,
            r.y,
            esc,
            Style::new().fg(theme::blend(bg, theme::DIM, t)).bg(bg),
        );
    }
    r
}

/// Draws a one-cell rounded border along the edge of `r`.
pub fn draw_rounded_border(buf: &mut Buffer, r: Rect, style: Style) {
    if r.width < 2 || r.height < 2 {
        return;
    }
    let x1 = r.x + r.width - 1;
    let y1 = r.y + r.height - 1;
    for xx in r.x + 1..x1 {
        buf.set_string(xx, r.y, "─", style);
        buf.set_string(xx, y1, "─", style);
    }
    for yy in r.y + 1..y1 {
        buf.set_string(r.x, yy, "│", style);
        buf.set_string(x1, yy, "│", style);
    }
    buf.set_string(r.x, r.y, "╭", style);
    buf.set_string(x1, r.y, "╮", style);
    buf.set_string(r.x, y1, "╰", style);
    buf.set_string(x1, y1, "╯", style);
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
fn dim(buf: &mut Buffer, area: Rect, amount: f32) {
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
                c.fg = theme::blend(fg, dark, amount * FG_DIM_FACTOR);
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
    pub rect: Rect,
    lines: Vec<Line<'static>>,
    max_scroll: usize,
    title: String,
    /// Width hint (widest line + padding), capped at 80 when drawing.
    width: u16,
}

const KEYBINDS: &[(&str, &[(&str, &str)])] = &[
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
                    Span::styled(format!("  {k:<KEY_COLUMN_W$}"), theme::fg(theme::PRIMARY)),
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
                    format!("  {:<KEY_COLUMN_W$}", format!("/{n}")),
                    theme::fg(theme::PRIMARY),
                ),
                Span::styled(d.clone(), theme::text()),
            ]));
        }
        HelpDialog {
            scroll: 0,
            opened: Instant::now(),
            rect: Rect::default(),
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
            .map(|l| Line::from(Span::styled(text::expand_tabs(l), theme::text())))
            .collect();
        let widest = body.lines().map(width).max().unwrap_or(0) as u16;
        HelpDialog {
            scroll: 0,
            opened: Instant::now(),
            rect: Rect::default(),
            lines,
            max_scroll: 0,
            title: title.to_string(),
            width: (widest + 6).max(40),
        }
    }

    pub fn scroll_by(&mut self, d: i32) {
        self.scroll = (self.scroll as i32 + d).clamp(0, self.max_scroll as i32) as usize;
    }

    pub fn scroll_to_top(&mut self) {
        self.scroll = 0;
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll = self.max_scroll;
    }

    pub fn draw(&mut self, buf: &mut Buffer, screen: Rect) {
        if screen.width < 6 || screen.height < 4 {
            self.rect = Rect::default();
            return;
        }
        let t = open_progress(self.opened);
        let w = screen
            .width
            .saturating_sub(4)
            .clamp(6, self.width.min(110))
            .min(screen.width);
        let iw = w.saturating_sub(4).max(1) as usize;
        let wrapped: Vec<Line<'static>> = self
            .lines
            .iter()
            .flat_map(|l| {
                let rows = text::wrap_spans(&l.spans, iw);
                if rows.is_empty() {
                    vec![Line::default()]
                } else {
                    rows
                }
            })
            .collect();
        let max_h = screen
            .height
            .saturating_sub(4)
            .clamp(4, 40)
            .min(screen.height);
        let h = (wrapped.len() as u16 + 2)
            .clamp(4, max_h)
            .min(screen.height);
        let r = frame(buf, screen, w, h, &self.title, t);
        self.rect = r;
        if r.width < 6 || r.height < 4 {
            return;
        }
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
        if self.max_scroll > 0 && vh > 0 {
            super::chat::draw_scrollbar(
                buf,
                Rect::new(r.x + r.width - 2, inner.y, 1, vh as u16),
                wrapped.len(),
                self.scroll as f64,
                self.max_scroll as f64,
                ("┃", theme::BORDER_ACTIVE),
                Some(("│", theme::BORDER_SUBTLE)),
            );
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendField {
    Url,
    Token,
    ConnectTemp,
    SaveDefault,
    RestoreLocal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendAction {
    Close,
    ConnectTemporary { url: String, token: Option<String> },
    SaveDefault { url: String, token: Option<String> },
    RestoreLocal,
}

const BACKEND_DIALOG_W: u16 = 74;
const BACKEND_DIALOG_H: u16 = 15;
const BACKEND_MIN_W: u16 = 24;
const BACKEND_MIN_H: u16 = 12;
const BACKEND_INNER_MIN_W: u16 = 10;
const BACKEND_INNER_MIN_H: u16 = 10;

pub struct BackendDialog {
    pub url: Editor,
    pub token: Editor,
    pub focus: BackendField,
    pub current_summary: String,
    pub error: Option<String>,
    pub opened: Instant,
    pub rect: Rect,
    url_rect: Rect,
    token_rect: Rect,
    btn_rects: Vec<(Rect, BackendField)>,
    cursor_at: Option<(u16, u16)>,
}

impl BackendDialog {
    pub fn new(
        initial_url: Option<&str>,
        initial_token: Option<&str>,
        current_summary: String,
    ) -> Self {
        let mut url = Editor::default();
        if let Some(u) = initial_url {
            url.set_text(u);
        }
        let mut token = Editor::default();
        if let Some(t) = initial_token {
            token.set_text(t);
        }
        Self {
            url,
            token,
            focus: BackendField::Url,
            current_summary,
            error: None,
            opened: Instant::now(),
            rect: Rect::default(),
            url_rect: Rect::default(),
            token_rect: Rect::default(),
            btn_rects: Vec::new(),
            cursor_at: None,
        }
    }

    fn cycle_focus(&mut self, dir: i32) {
        const ORDER: [BackendField; 5] = [
            BackendField::Url,
            BackendField::Token,
            BackendField::ConnectTemp,
            BackendField::SaveDefault,
            BackendField::RestoreLocal,
        ];
        let idx = ORDER.iter().position(|f| *f == self.focus).unwrap_or(0) as i32;
        let next = (idx + dir).rem_euclid(ORDER.len() as i32) as usize;
        self.focus = ORDER[next];
    }

    fn submit_explicit(&mut self, save: bool) -> Option<BackendAction> {
        let raw_url = self.url.text.trim().to_string();
        if let Err(e) = ref_client::validate_explicit_url(&raw_url) {
            self.error = Some(e.to_string());
            self.focus = BackendField::Url;
            return None;
        }
        self.error = None;
        let tok = crate::prefs::normalize_token(Some(&self.token.text));
        if save {
            Some(BackendAction::SaveDefault {
                url: raw_url,
                token: tok,
            })
        } else {
            Some(BackendAction::ConnectTemporary {
                url: raw_url,
                token: tok,
            })
        }
    }

    pub fn handle_key(&mut self, k: &KeyEvent) -> Option<BackendAction> {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Esc => return Some(BackendAction::Close),
            KeyCode::Char('c') if ctrl => return Some(BackendAction::Close),
            KeyCode::Char('s') if ctrl => return self.submit_explicit(true),
            KeyCode::Char('r') if ctrl => return Some(BackendAction::RestoreLocal),
            KeyCode::Tab | KeyCode::Down => self.cycle_focus(1),
            KeyCode::BackTab | KeyCode::Up => self.cycle_focus(-1),
            KeyCode::Enter => match self.focus {
                BackendField::Url | BackendField::Token | BackendField::ConnectTemp => {
                    return self.submit_explicit(false);
                }
                BackendField::SaveDefault => return self.submit_explicit(true),
                BackendField::RestoreLocal => return Some(BackendAction::RestoreLocal),
            },
            _ => match self.focus {
                BackendField::Url => {
                    if crate::input::edit_key(&mut self.url, k, false) {
                        self.error = None;
                    }
                }
                BackendField::Token => {
                    crate::input::edit_key(&mut self.token, k, false);
                }
                _ => {}
            },
        }
        None
    }

    pub fn handle_paste(&mut self, text: &str) {
        let clean = text::one_line(text);
        match self.focus {
            BackendField::Url => {
                self.url.insert(&clean);
                self.error = None;
            }
            BackendField::Token => {
                self.token.insert(&clean);
            }
            _ => {}
        }
    }

    pub fn handle_mouse(&mut self, me: MouseEvent) -> (bool, Option<BackendAction>) {
        let MouseEventKind::Down(MouseButton::Left) = me.kind else {
            return (false, None);
        };
        let pos = Position::new(me.column, me.row);
        if !self.rect.contains(pos) {
            return (true, Some(BackendAction::Close));
        }
        if self.url_rect.contains(pos) {
            self.focus = BackendField::Url;
            let off = self.url.offset_at_single_line(self.url_rect, me.column);
            self.url.mouse_down(off, 1);
            self.url.mouse_up();
            return (true, None);
        }
        if self.token_rect.contains(pos) {
            self.focus = BackendField::Token;
            let off = self.token.offset_at_single_line(self.token_rect, me.column);
            self.token.mouse_down(off, 1);
            self.token.mouse_up();
            return (true, None);
        }
        if let Some((_, field)) = self
            .btn_rects
            .iter()
            .find(|(r, _)| r.contains(pos))
            .copied()
        {
            self.focus = field;
            let act = match field {
                BackendField::ConnectTemp => self.submit_explicit(false),
                BackendField::SaveDefault => self.submit_explicit(true),
                BackendField::RestoreLocal => Some(BackendAction::RestoreLocal),
                _ => None,
            };
            return (true, act);
        }
        (true, None)
    }

    pub fn cursor(&self) -> Option<(u16, u16)> {
        self.cursor_at
    }

    pub fn draw(&mut self, buf: &mut Buffer, screen: Rect) {
        self.cursor_at = None;
        self.btn_rects.clear();
        self.url_rect = Rect::default();
        self.token_rect = Rect::default();
        if screen.width < BACKEND_MIN_W || screen.height < BACKEND_MIN_H {
            self.rect = Rect::default();
            return;
        }
        let t = open_progress(self.opened);
        let w = screen
            .width
            .saturating_sub(4)
            .clamp(BACKEND_MIN_W, BACKEND_DIALOG_W)
            .min(screen.width);
        let h = screen
            .height
            .saturating_sub(4)
            .clamp(BACKEND_MIN_H, BACKEND_DIALOG_H)
            .min(screen.height);
        let r = frame(buf, screen, w, h, "Backend connection", t);
        self.rect = r;
        if r.width < BACKEND_INNER_MIN_W || r.height < BACKEND_INNER_MIN_H {
            return;
        }
        let inner = Rect::new(
            r.x + 2,
            r.y + 1,
            r.width.saturating_sub(4),
            r.height.saturating_sub(2),
        );
        let bg = theme::PANEL;
        let fade = |c: Color| theme::blend(bg, c, t);
        let iw = inner.width as usize;

        buf.set_string(
            inner.x,
            inner.y,
            text::truncate(&format!("Current: {}", self.current_summary), iw),
            Style::new().fg(fade(theme::MUTED)).bg(bg),
        );

        let url_label_y = inner.y + 2;
        let url_y = inner.y + 3;
        let tok_label_y = inner.y + 5;
        let tok_y = inner.y + 6;

        let url_lbl_style = Style::new()
            .fg(fade(if self.focus == BackendField::Url {
                theme::PRIMARY
            } else {
                theme::ACCENT
            }))
            .bg(bg)
            .add_modifier(Modifier::BOLD);
        buf.set_string(
            inner.x,
            url_label_y,
            "Backend WebSocket URL:",
            url_lbl_style,
        );
        let url_area = Rect::new(inner.x, url_y, inner.width, 1);
        self.url_rect = url_area;
        let url_bg = if self.focus == BackendField::Url {
            theme::ELEMENT
        } else {
            bg
        };
        buf.set_style(url_area, Style::new().bg(url_bg));
        let url_cur = self.url.draw_single_line(
            buf,
            url_area,
            Some(url_bg),
            "ws://host:port or wss://host/path",
            self.focus == BackendField::Url,
        );
        if self.focus == BackendField::Url {
            self.cursor_at = url_cur;
        }

        let tok_lbl_style = Style::new()
            .fg(fade(if self.focus == BackendField::Token {
                theme::PRIMARY
            } else {
                theme::ACCENT
            }))
            .bg(bg)
            .add_modifier(Modifier::BOLD);
        buf.set_string(inner.x, tok_label_y, "Token (optional):", tok_lbl_style);
        let tok_area = Rect::new(inner.x, tok_y, inner.width, 1);
        self.token_rect = tok_area;
        let tok_bg = if self.focus == BackendField::Token {
            theme::ELEMENT
        } else {
            bg
        };
        buf.set_style(tok_area, Style::new().bg(tok_bg));
        let tok_cur = self.token.draw_single_line(
            buf,
            tok_area,
            Some(tok_bg),
            "(none)",
            self.focus == BackendField::Token,
        );
        if self.focus == BackendField::Token {
            self.cursor_at = tok_cur;
        }

        let btn_y = inner.y + inner.height.saturating_sub(3);
        let hint_y = inner.y + inner.height.saturating_sub(1);

        if let Some(err) = &self.error {
            let err_y = if btn_y > tok_y + 2 {
                inner.y + 8
            } else {
                btn_y + 1
            };
            if err_y < hint_y {
                buf.set_string(
                    inner.x,
                    err_y,
                    text::truncate(err, iw),
                    Style::new().fg(fade(theme::ERROR)).bg(bg),
                );
            }
        }

        let mut bx = inner.x;
        let btns = [
            ("Connect (temp)", BackendField::ConnectTemp),
            ("Save default (ctrl+s)", BackendField::SaveDefault),
            ("Local auto (ctrl+r)", BackendField::RestoreLocal),
        ];
        for (label, field) in btns {
            let bw = width(label) as u16 + 2;
            if bx + bw > inner.x + inner.width {
                break;
            }
            let focused = self.focus == field;
            let (fg, bbg) = if focused {
                (theme::PANEL, theme::PRIMARY)
            } else {
                (fade(theme::TEXT), theme::ELEMENT)
            };
            buf.set_string(
                bx,
                btn_y,
                format!(" {label} "),
                Style::new().fg(fg).bg(bbg).add_modifier(if focused {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
            );
            self.btn_rects.push((Rect::new(bx, btn_y, bw, 1), field));
            bx += bw + 1;
        }

        buf.set_string(
            inner.x,
            hint_y,
            text::truncate(
                "tab field · enter connect · ctrl+s save default · ctrl+r local auto · esc cancel",
                iw,
            ),
            Style::new().fg(fade(theme::DIM)).bg(bg),
        );
    }
}

#[allow(clippy::large_enum_variant)]
pub enum Dialog {
    List(ListDialog),
    Help(HelpDialog),
    Mcp(super::mcp::McpDialog),
    Backend(BackendDialog),
    Stash(super::stash::StashDialog),
}

impl Dialog {
    pub fn animating(&self) -> bool {
        let o = match self {
            Dialog::List(d) => d.opened,
            Dialog::Help(d) => d.opened,
            Dialog::Mcp(d) => d.opened,
            Dialog::Backend(d) => d.opened,
            Dialog::Stash(d) => d.opened,
        };
        o.elapsed() < OPEN_SETTLE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_dialog_error_never_overlaps_token_buttons_or_hint_at_min_height() {
        for height in [BACKEND_MIN_H, 13, 14, 20] {
            let area = Rect::new(0, 0, 80, height);
            let mut buf = Buffer::empty(area);
            let mut dlg = BackendDialog::new(Some("http://bad"), None, "local".into());
            dlg.error = Some("invalid URL".into());
            dlg.draw(&mut buf, area);

            let row_text = |y: u16| -> String {
                (0..area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            };
            assert!(
                !dlg.btn_rects.is_empty(),
                "buttons must render at height={height}"
            );
            let tok_y = dlg.token_rect.y;
            let btn_y = dlg.btn_rects[0].0.y;
            let hint_y = dlg.rect.y + dlg.rect.height - 2;
            assert!(
                !row_text(tok_y).contains("invalid URL"),
                "error overlapped token at height={height}"
            );
            assert!(
                !row_text(btn_y).contains("invalid URL"),
                "error overlapped buttons at height={height}"
            );
            assert!(
                !row_text(hint_y).contains("invalid URL"),
                "error overlapped hint at height={height}"
            );
            assert!(
                (tok_y + 1..hint_y).any(|y| row_text(y).contains("invalid URL")),
                "error text was not drawn at height={height}"
            );
        }
    }
}
