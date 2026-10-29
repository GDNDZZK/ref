//! Transcript view: per-message render cache, scrolling (with easing), hit testing, selection.

use std::collections::{BTreeSet, HashMap};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ref_proto::{AgentInfo, ModelInfo, SessionStatus};

use super::message::{self, Ctx};
use super::selection::{self, Selection};
use super::text::{self, width};
use super::{Hit, Row, theme};
use crate::state::SessionView;

/// Shared rendering environment.
pub struct Env<'a> {
    pub agents: &'a [AgentInfo],
    pub models: &'a [ModelInfo],
    pub details: bool,
    pub thinking: bool,
    pub tick: u64,
    pub now: i64,
}

struct Cached {
    key: u64,
    rows: Vec<Row>,
}

#[derive(Default)]
pub struct ChatState {
    cache: HashMap<String, Cached>,
    pub toggled: HashMap<String, BTreeSet<usize>>,
    pub show_all: HashMap<String, BTreeSet<usize>>,
    /// Start row of each message block; last element = start of tail rows.
    offsets: Vec<usize>,
    ids: Vec<String>,
    tail: Vec<Row>,
    total: usize,
    /// Displayed scroll position (rows from top) and animation target.
    pos: f64,
    target: f64,
    /// Stick to the bottom.
    pub follow: bool,
    initialized: bool,
    pub new_below: bool,
    last_total: usize,
    /// Last drawn viewport (content area) and its height.
    pub area: Rect,
    pub content_x: u16,
    pub sel: Option<Selection>,
    pub hover: Option<(u16, u16)>,
    /// Pending click (row, col) captured at mouse down.
    press: Option<(usize, u16)>,
    /// Auto-scroll direction while dragging beyond edges.
    pub drag_edge: i8,
    /// Message ids flagged as tick-dependent at last render.
    pub animating: bool,
    /// Compact (split tile) render cache.
    tile_cache: HashMap<String, Cached>,
}

pub fn hash_key(parts: &[u64]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for p in parts {
        h ^= *p;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn set_hash(s: Option<&BTreeSet<usize>>) -> u64 {
    s.map(|s| s.iter().fold(7u64, |a, x| a.wrapping_mul(31).wrapping_add(*x as u64 + 1))).unwrap_or(0)
}

impl ChatState {
    pub fn max_scroll(&self) -> f64 {
        self.total.saturating_sub(self.area.height as usize) as f64
    }

    pub fn top(&self) -> usize {
        self.pos.round().max(0.0) as usize
    }

    pub fn scroll_by(&mut self, delta: i32, animate: bool) {
        let max = self.max_scroll();
        let base = if animate { self.target } else { self.pos };
        let t = (base + delta as f64).clamp(0.0, max);
        self.target = t;
        if !animate {
            self.pos = t;
        }
        self.follow = t >= max - 0.01;
        if self.follow {
            self.new_below = false;
        }
    }

    pub fn page(&mut self, dir: i32, half: bool) {
        let h = self.area.height.max(2) as i32;
        let step = if half { h / 2 } else { h - 2 };
        self.scroll_by(dir * step.max(1), true);
    }

    pub fn scroll_to_bottom(&mut self, animate: bool) {
        self.follow = true;
        self.new_below = false;
        self.target = self.max_scroll();
        if !animate {
            self.pos = self.target;
        }
    }

    pub fn scroll_to_top(&mut self) {
        self.follow = false;
        self.target = 0.0;
    }

    /// Advances the scroll animation. Returns true if still moving.
    pub fn step_anim(&mut self) -> bool {
        let d = self.target - self.pos;
        if d.abs() < 0.5 {
            self.pos = self.target;
            return false;
        }
        // Ease out: cover ~40% of the remaining distance each frame (≈120ms at 60fps).
        let step = d * 0.4;
        self.pos += if step.abs() < 1.0 { d.signum() } else { step };
        true
    }

    pub fn scrolling(&self) -> bool {
        (self.target - self.pos).abs() >= 0.5
    }

    pub fn invalidate(&mut self) {
        self.cache.clear();
        self.tile_cache.clear();
    }

    /// Cached compact rows for a split tile.
    pub fn tile_rows(&mut self, id: &str, key: u64, f: impl FnOnce() -> Vec<Row>) -> &[Row] {
        let hit = self.tile_cache.get(id).is_some_and(|c| c.key == key);
        if !hit {
            if self.tile_cache.len() >= 64 {
                // Keep it bounded; entries are cheap to rebuild.
                self.tile_cache.clear();
            }
            self.tile_cache.insert(id.to_string(), Cached { key, rows: f() });
        }
        self.tile_cache.get(id).map(|c| c.rows.as_slice()).unwrap_or(&[])
    }

    /// Global row accessor.
    pub fn row(&self, i: usize) -> Option<&Row> {
        let tail_start = *self.offsets.last()?;
        if i >= tail_start {
            return self.tail.get(i - tail_start);
        }
        let k = match self.offsets.binary_search(&i) {
            Ok(k) => k,
            Err(k) => k.checked_sub(1)?,
        };
        let id = self.ids.get(k)?;
        self.cache.get(id)?.rows.get(i - self.offsets[k])
    }

    fn screen_to_content(&self, x: u16, y: u16) -> Option<(usize, u16)> {
        if y < self.area.y || y >= self.area.y + self.area.height {
            return None;
        }
        let row = self.top() + (y - self.area.y) as usize;
        Some((row, x.saturating_sub(self.content_x)))
    }

    /// Clamped conversion for drags (outside the area → edge).
    fn screen_to_content_clamped(&self, x: u16, y: u16) -> (usize, u16) {
        let yy = y.clamp(self.area.y, (self.area.y + self.area.height).saturating_sub(1));
        let row = self.top() + (yy - self.area.y) as usize;
        let row = row.min(self.total.saturating_sub(1));
        (row, x.saturating_sub(self.content_x))
    }

    pub fn contains(&self, x: u16, y: u16) -> bool {
        x >= self.area.x && x < self.area.x + self.area.width && y >= self.area.y && y < self.area.y + self.area.height
    }

    pub fn hit_at(&self, x: u16, y: u16) -> Option<Hit> {
        let (r, c) = self.screen_to_content(x, y)?;
        if !self.contains(x, y) {
            return None;
        }
        self.row(r)?.hit_at(c).cloned()
    }

    /// Mouse down: start selection (clicks = 1/2/3 for char/word/line).
    pub fn mouse_down(&mut self, x: u16, y: u16, clicks: u8) {
        let Some((r, c)) = self.screen_to_content(x, y) else { return };
        self.press = Some((r, c));
        match clicks {
            2 => {
                if let Some(row) = self.row(r) {
                    let (a, b) = selection::word_at(row, c);
                    self.sel = Some(Selection { anchor: (r, a), head: (r, b), dragging: false, active: true });
                }
            }
            3 => {
                if let Some(row) = self.row(r) {
                    let e = selection::row_end(row);
                    self.sel = Some(Selection { anchor: (r, row.skip), head: (r, e), dragging: false, active: true });
                }
            }
            _ => {
                self.sel = Some(Selection { anchor: (r, c), head: (r, c), dragging: true, active: false });
            }
        }
    }

    pub fn mouse_drag(&mut self, x: u16, y: u16) {
        let edge = if y < self.area.y {
            -1
        } else if y >= self.area.y + self.area.height {
            1
        } else {
            0
        };
        self.drag_edge = edge;
        if edge != 0 {
            self.scroll_by(edge as i32, false);
        }
        let p = self.screen_to_content_clamped(x, y);
        if let Some(s) = self.sel.as_mut()
            && s.dragging
        {
            if p != s.anchor {
                s.active = true;
            }
            s.head = p;
        }
    }

    /// Continues edge auto-scroll while the button is held outside the view.
    pub fn drag_tick(&mut self) -> bool {
        if self.drag_edge == 0 || !self.sel.as_ref().is_some_and(|s| s.dragging) {
            return false;
        }
        self.scroll_by(self.drag_edge as i32, false);
        let y = if self.drag_edge < 0 { self.area.y } else { self.area.y + self.area.height.saturating_sub(1) };
        let r = (self.top() + (y - self.area.y) as usize).min(self.total.saturating_sub(1));
        if let Some(s) = self.sel.as_mut() {
            s.head.0 = r;
        }
        true
    }

    /// Mouse up: returns a click target when this was a click (no selection made).
    pub fn mouse_up(&mut self, x: u16, y: u16) -> Option<Hit> {
        self.drag_edge = 0;
        let press = self.press.take();
        if let Some(s) = self.sel.as_mut() {
            s.dragging = false;
            if !s.active {
                self.sel = None;
            } else {
                return None;
            }
        }
        let (r, c) = self.screen_to_content(x, y)?;
        if press != Some((r, c)) && press.map(|p| p.0) != Some(r) {
            return None;
        }
        self.row(r)?.hit_at(c).cloned()
    }

    pub fn selected_text(&self) -> Option<String> {
        let s = self.sel.as_ref().filter(|s| s.active)?;
        let t = selection::extract(s, |i| self.row(i));
        (!t.is_empty()).then_some(t)
    }

    pub fn has_selection(&self) -> bool {
        self.sel.as_ref().is_some_and(|s| s.active)
    }

    pub fn toggle(&mut self, msg: &str, part: usize) {
        let set = self.toggled.entry(msg.to_string()).or_default();
        if !set.remove(&part) {
            set.insert(part);
        }
        self.sel = None;
    }

    pub fn show_all(&mut self, msg: &str, part: usize) {
        self.show_all.entry(msg.to_string()).or_default().insert(part);
        self.sel = None;
    }
}

/// Index of the last compaction message: the model's context starts there.
pub fn last_compaction(messages: &[ref_proto::Message]) -> Option<usize> {
    messages.iter().rposition(|m| m.role == ref_proto::Role::User && m.source == ref_proto::MessageSource::Compaction)
}

/// Builds (or reuses) cached rows for all messages and the tail, for content width `w`.
fn layout(view: &mut SessionView, env: &Env<'_>, w: usize) {
    let busy = !view.info.status.is_idle();
    let SessionView { messages, versions, chat, info, .. } = view;
    let mut offsets = Vec::with_capacity(messages.len() + 1);
    let mut ids = Vec::with_capacity(messages.len());
    let mut total = 0usize;
    let mut animating = false;
    let n = messages.len();
    let last_compaction = last_compaction(messages);
    for (i, m) in messages.iter().enumerate() {
        let stale = last_compaction.is_some_and(|c| i < c);
        let context_start = i > 0 && last_compaction == Some(i);
        // Only the tail of a busy session animates (stale unfinished messages render statically).
        let live = busy && crate::state::message_live(m);
        animating |= live;
        let ver = versions.get(i).copied().unwrap_or(0);
        let tog = chat.toggled.get(&m.id);
        let all = chat.show_all.get(&m.id);
        let anim = if live { env.tick } else { 0 };
        let secs = if live { (env.now / 1000) as u64 } else { 0 };
        let key = hash_key(&[ver, w as u64, set_hash(tog), set_hash(all), env.details as u64, env.thinking as u64, anim, secs, stale as u64, context_start as u64, 7]);
        let hit = chat.cache.get(&m.id).is_some_and(|c| c.key == key);
        if !hit {
            let cx = Ctx {
                agents: env.agents,
                models: env.models,
                toggled: tog,
                show_all: all,
                details: env.details,
                thinking: env.thinking,
                now: env.now,
                tick: env.tick,
                compact: false,
                session_agent: &info.agent,
                stale,
                context_start,
            };
            let mut rows = vec![Row::blank()];
            rows.extend(message::render(m, &cx, w));
            chat.cache.insert(m.id.clone(), Cached { key, rows });
        }
        offsets.push(total);
        ids.push(m.id.clone());
        total += chat.cache.get(&m.id).map(|c| c.rows.len()).unwrap_or(0);
        let _ = n;
    }
    // Evict cache entries for removed messages.
    if chat.cache.len() > ids.len() + 8 {
        let keep: std::collections::HashSet<&String> = ids.iter().collect();
        chat.cache.retain(|k, _| keep.contains(k));
    }
    offsets.push(total);
    chat.offsets = offsets;
    chat.ids = ids;
    chat.animating = animating;
    total += chat.tail.len();
    chat.total = total;
}

/// Rows shown after the transcript: retry status, error, queued prompts.
fn tail_rows(view: &SessionView, env: &Env<'_>, w: usize) -> Vec<Row> {
    let mut rows = Vec::new();
    if let SessionStatus::Retrying { attempt, next_at, error } = &view.info.status {
        rows.push(Row::blank());
        let secs = ((next_at - env.now).max(0) + 999) / 1000;
        let msg = format!("{} Retrying in {secs}s (attempt {attempt}): {}", theme::spinner(env.tick), text::one_line(error));
        for (i, (l, sp)) in text::wrap_spans_flags(&[Span::styled(msg, theme::fg(theme::WARNING))], w.saturating_sub(4)).into_iter().enumerate() {
            rows.push(Row::new(l).soft_sp(i > 0, sp).indent(vec![Span::raw("  "), Span::styled("┃ ", theme::fg(theme::WARNING))]));
        }
    }
    if let SessionStatus::Compacting = view.info.status {
        rows.push(Row::blank());
        let mut spans = vec![Span::raw("  "), Span::styled(format!("{} ", theme::spinner(env.tick)), theme::fg(theme::ACCENT))];
        spans.extend(message::shimmer("Compacting context…", env.tick, theme::MUTED, theme::ACCENT));
        rows.push(Row::new(Line::from(spans)).decor());
    }
    if let Some(e) = &view.error
        && !view.messages.last().is_some_and(|m| m.error.as_deref() == Some(e.as_str()))
    {
        rows.push(Row::blank());
        for (i, (l, sp)) in text::wrap_spans_flags(&[Span::styled(text::one_line(e), theme::fg(theme::ERROR))], w.saturating_sub(4)).into_iter().enumerate() {
            rows.push(Row::new(l).soft_sp(i > 0, sp).indent(vec![Span::raw("  "), Span::styled("┃ ", theme::fg(theme::ERROR))]));
        }
    }
    if !view.queued.is_empty() {
        rows.push(Row::blank());
        for q in &view.queued {
            let line = Line::from(vec![
                Span::styled("  queued ", theme::fg(theme::DIM).add_modifier(Modifier::BOLD)),
                Span::styled(text::truncate(&text::one_line(q), w.saturating_sub(10)), theme::fg(theme::DIM)),
            ]);
            rows.push(Row::new(line).decor());
        }
    }
    rows.push(Row::blank().decor());
    rows
}

fn draw_welcome(buf: &mut Buffer, area: Rect, view: &SessionView, loaded: bool) {
    if area.height < 6 || area.width < 20 {
        return;
    }
    let mut lines: Vec<Line<'static>> = Vec::new();
    for l in crate::app::LOGO.iter() {
        lines.push(Line::from(Span::styled(l.to_string(), theme::bold(theme::PRIMARY))));
    }
    lines.push(Line::default());
    if !loaded {
        lines.push(Line::from(Span::styled("connecting…", theme::muted())));
    } else {
        lines.push(Line::from(vec![
            Span::styled("session ", theme::muted()),
            Span::styled(view.info.name.clone(), theme::text()),
            Span::styled(format!("  ·  {}", text::tilde(&view.info.cwd)), theme::muted()),
        ]));
        lines.push(Line::default());
        for (k, d) in [("/", "commands"), ("@", "attach files"), ("!", "run shell command"), ("ctrl+p", "command palette"), ("ctrl+x l", "sessions"), ("ctrl+x w", "background sessions")] {
            lines.push(Line::from(vec![Span::styled(format!("{k:>10}  "), theme::fg(theme::PRIMARY)), Span::styled(format!("{d:<20}"), theme::muted())]));
        }
    }
    let h = lines.len() as u16;
    let y0 = area.y + area.height.saturating_sub(h) / 2;
    for (i, l) in lines.iter().enumerate() {
        let y = y0 + i as u16;
        if y >= area.y + area.height {
            break;
        }
        let lw = text::line_width(l) as u16;
        let x = area.x + area.width.saturating_sub(lw) / 2;
        buf.set_line(x, y, l, area.width);
    }
}

/// Draws the transcript of `view` into `area`.
pub fn draw(buf: &mut Buffer, area: Rect, view: &mut SessionView, env: &Env<'_>) {
    if area.width < 4 || area.height == 0 {
        return;
    }
    // 1 col left pad, 1 col right pad + scrollbar column.
    let cw = area.width.saturating_sub(3).max(1) as usize;
    let tail = tail_rows(view, env, cw);
    view.chat.tail = tail;
    layout(view, env, cw);
    let loaded = view.loaded;
    let chat = &mut view.chat;
    chat.area = area;
    chat.content_x = area.x + 1;
    let max = chat.max_scroll();
    if !chat.initialized {
        chat.initialized = true;
        chat.follow = true;
        chat.pos = max;
        chat.target = max;
    }
    if chat.total > chat.last_total && !chat.follow && chat.last_total > 0 {
        chat.new_below = true;
    }
    chat.last_total = chat.total;
    if chat.follow {
        // Stick to bottom: jump directly when content grows (streaming must not lag behind).
        if (chat.target - max).abs() > 0.01 {
            chat.target = max;
            if !chat.scrolling() || chat.pos > max {
                chat.pos = max;
            }
        }
    }
    chat.target = chat.target.clamp(0.0, max);
    chat.pos = chat.pos.clamp(0.0, max.max(0.0));

    if view.messages.is_empty() && !view.is_busy() {
        draw_welcome(buf, area, view, loaded);
        return;
    }
    let chat = &view.chat;
    let top = chat.top();
    let hover = chat.hover.and_then(|(x, y)| {
        let (r, c) = chat.screen_to_content(x, y)?;
        chat.contains(x, y).then_some((r, c))
    });
    for dy in 0..area.height {
        let gi = top + dy as usize;
        let Some(row) = chat.row(gi) else { break };
        let y = area.y + dy;
        buf.set_line(chat.content_x, y, &row.line, cw as u16);
        // Hover feedback on click targets.
        if let Some((hr, hc)) = hover
            && hr == gi
            && let Some(z) = row.hits.iter().find(|z| hc >= z.x0 && hc < z.x1)
        {
            if z.hit.is_button() {
                for x in z.x0..z.x1.min(cw as u16) {
                    if let Some(cell) = buf.cell_mut((chat.content_x + x, y)) {
                        cell.set_style(Style::new().fg(theme::PRIMARY).add_modifier(Modifier::BOLD));
                    }
                }
            } else {
                // Brighten the disclosure arrow (first non-blank cell).
                let lw = (text::line_width(&row.line) as u16).min(cw as u16);
                if let Some(x) = (0..lw).find(|&x| buf.cell((chat.content_x + x, y)).is_some_and(|c| !c.symbol().trim().is_empty()))
                    && let Some(c) = buf.cell_mut((chat.content_x + x, y))
                {
                    c.set_style(Style::new().fg(theme::PRIMARY).add_modifier(Modifier::BOLD));
                }
            }
        }
        // Selection highlight.
        if let Some(sel) = chat.sel.as_ref().filter(|s| s.active) {
            let ((r0, c0), (r1, c1)) = sel.range();
            if gi >= r0 && gi <= r1 {
                let from = if gi == r0 { c0 } else { 0 }.max(row.skip);
                let lw = text::line_width(&row.line) as u16;
                let to = if gi == r1 { c1.min(lw.saturating_sub(1)) } else { lw.saturating_sub(1) };
                let to = to.min(cw as u16 - 1);
                if !row.decor && from <= to {
                    for x in from..=to {
                        if let Some(cell) = buf.cell_mut((chat.content_x + x, y)) {
                            cell.set_bg(theme::SELECTION);
                        }
                    }
                }
            }
        }
    }
    // Scrollbar.
    let h = area.height as usize;
    if chat.total > h {
        let sx = area.x + area.width - 1;
        let thumb = ((h * h) as f64 / chat.total as f64).ceil().max(1.0) as usize;
        let maxs = chat.max_scroll().max(1.0);
        let start = ((chat.pos / maxs) * (h - thumb) as f64).round() as usize;
        for i in 0..h {
            let (sym, c) = if i >= start && i < start + thumb { ("┃", theme::BORDER_ACTIVE) } else { ("│", theme::BORDER_SUBTLE) };
            if let Some(cell) = buf.cell_mut((sx, area.y + i as u16)) {
                cell.set_symbol(sym).set_style(Style::new().fg(c));
            }
        }
    }
    // "new messages" pill.
    if chat.new_below && !chat.follow {
        let label = " ↓ new messages ";
        let lw = width(label) as u16;
        let x = area.x + area.width.saturating_sub(lw) / 2;
        let y = area.y + area.height - 1;
        buf.set_string(x, y, label, Style::new().fg(theme::PANEL).bg(theme::PRIMARY).add_modifier(Modifier::BOLD));
    }
}

/// Whether the pill at the bottom was clicked.
pub fn pill_hit(chat: &ChatState, x: u16, y: u16) -> bool {
    if !(chat.new_below && !chat.follow) {
        return false;
    }
    let lw = 16u16;
    let a = chat.area;
    let x0 = a.x + a.width.saturating_sub(lw) / 2;
    y == a.y + a.height.saturating_sub(1) && x >= x0 && x < x0 + lw
}
