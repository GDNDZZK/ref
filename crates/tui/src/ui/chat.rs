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
    /// Messages laid out (cache misses) so far; tests check what a frame re-renders.
    #[cfg(test)]
    pub laid_out: usize,
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
    /// The last layout was the compact (split tile) one: rows come from `tile_cache`.
    compact: bool,
    /// Rows added while scrolled away from the bottom (shown in the tile pill).
    pub new_rows: usize,
    /// Screen rect of the "new messages" pill at last draw.
    pub pill: Option<Rect>,
    /// Content width of the last layout (re-wrapping is not "new content").
    last_w: usize,
    hold: Hold,
}

/// Blank rows kept under the transcript while a turn runs, so streaming content that gives
/// room back (see `ChatState::update_hold`) does not make the view bounce.
///
/// Follows the GUI's `holdEnd` in ChatView.qml (hold only while following; resume from the
/// current extent) with two differences: the space is capped at the viewport height minus
/// one, and it is released at once when the turn ends instead of with an animation.
#[derive(Default)]
struct Hold {
    /// Blank rows under the last content row.
    space: usize,
    /// Highest `total + space` seen since following (re)started in this turn.
    high_water: usize,
    was_busy: bool,
    was_following: bool,
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
    s.map(|s| {
        s.iter()
            .fold(7u64, |a, x| a.wrapping_mul(31).wrapping_add(*x as u64 + 1))
    })
    .unwrap_or(0)
}

impl ChatState {
    /// Rows the scroll range spans: the content plus the held blank rows.
    fn scroll_total(&self) -> usize {
        self.total + self.hold.space
    }

    pub fn max_scroll(&self) -> f64 {
        self.scroll_total()
            .saturating_sub(self.area.height as usize) as f64
    }

    /// While a turn runs and the view follows the end, the end never moves up: rows the
    /// content gives back stay as blank space until the turn ends (then they are released at
    /// once). Streaming content often takes more room than it needs in the end: a retry
    /// notice, a tool's live output, or reasoning that is expanded while it streams and
    /// folded to one line afterwards.
    ///
    /// The blank space is capped at the viewport height minus one, so the last content row
    /// stays visible. Further up (not following) the end moving is out of sight and the held
    /// space stays as it is.
    fn update_hold(&mut self, busy: bool) {
        let hold = &mut self.hold;
        if !busy {
            *hold = Hold {
                was_following: self.follow,
                ..Hold::default()
            };
            return;
        }
        let turn_started = !hold.was_busy;
        let follow_resumed = self.follow && !hold.was_following;
        if turn_started || follow_resumed {
            hold.high_water = self.total + hold.space;
        }
        hold.was_busy = true;
        hold.was_following = self.follow;
        if !self.follow {
            return;
        }
        let cap = (self.area.height as usize).saturating_sub(1);
        hold.high_water = hold.high_water.max(self.total).min(self.total + cap);
        hold.space = hold.high_water - self.total;
    }

    /// Drops the held space after a re-wrap: row counts of the old width mean nothing now.
    /// Removed messages do not release it: a retry removes the unfinished assistant message,
    /// which is exactly the shrinking that the hold absorbs.
    fn release_hold(&mut self) {
        self.hold = Hold {
            was_busy: self.hold.was_busy,
            was_following: self.hold.was_following,
            ..Hold::default()
        };
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
            self.new_rows = 0;
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
        self.new_rows = 0;
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
        let cache = if self.compact {
            &self.tile_cache
        } else {
            &self.cache
        };
        cache.get(id)?.rows.get(i - self.offsets[k])
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
        let yy = y.clamp(
            self.area.y,
            (self.area.y + self.area.height).saturating_sub(1),
        );
        let row = self.top() + (yy - self.area.y) as usize;
        let row = row.min(self.total.saturating_sub(1));
        (row, x.saturating_sub(self.content_x))
    }

    pub fn contains(&self, x: u16, y: u16) -> bool {
        x >= self.area.x
            && x < self.area.x + self.area.width
            && y >= self.area.y
            && y < self.area.y + self.area.height
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
        let Some((r, c)) = self.screen_to_content(x, y) else {
            return;
        };
        self.press = Some((r, c));
        match clicks {
            2 => {
                if let Some(row) = self.row(r) {
                    let (a, b) = selection::word_at(row, c);
                    self.sel = Some(Selection {
                        anchor: (r, a),
                        head: (r, b),
                        dragging: false,
                        active: true,
                    });
                }
            }
            3 => {
                if let Some(row) = self.row(r) {
                    let e = selection::row_end(row);
                    self.sel = Some(Selection {
                        anchor: (r, row.skip),
                        head: (r, e),
                        dragging: false,
                        active: true,
                    });
                }
            }
            _ => {
                self.sel = Some(Selection {
                    anchor: (r, c),
                    head: (r, c),
                    dragging: true,
                    active: false,
                });
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
        let y = if self.drag_edge < 0 {
            self.area.y
        } else {
            self.area.y + self.area.height.saturating_sub(1)
        };
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
        // The re-layout isn't new content: no "new messages" pill.
        self.last_total = 0;
    }

    pub fn show_all(&mut self, msg: &str, part: usize) {
        self.show_all
            .entry(msg.to_string())
            .or_default()
            .insert(part);
        self.sel = None;
        self.last_total = 0;
    }
}

/// Index of the last compaction message: the model's context starts there.
pub fn last_compaction(messages: &[ref_proto::Message]) -> Option<usize> {
    messages.iter().rposition(|m| {
        m.role == ref_proto::Role::User && m.source == ref_proto::MessageSource::Compaction
    })
}

/// Builds (or reuses) cached rows for all messages and the tail, for content width `w`.
/// `compact` selects the split-tile rendering (own cache; tools / reasoning one-line).
fn layout(view: &mut SessionView, env: &Env<'_>, w: usize, compact: bool) {
    let busy = !view.info.status.is_idle();
    let SessionView {
        messages,
        versions,
        chat,
        info,
        ..
    } = view;
    let mut offsets = Vec::with_capacity(messages.len() + 1);
    let mut ids = Vec::with_capacity(messages.len());
    let mut total = 0usize;
    let mut animating = false;
    let last_compaction = last_compaction(messages);
    chat.compact = compact;
    for (i, m) in messages.iter().enumerate() {
        let stale = last_compaction.is_some_and(|c| i < c);
        let context_start = i > 0 && last_compaction == Some(i);
        // Only the tail of a busy session animates (stale unfinished messages render statically).
        let live = busy && crate::state::message_live(m);
        animating |= live;
        let ver = versions.get(i).copied().unwrap_or(0);
        let anim = if live { env.tick } else { 0 };
        let secs = if live { (env.now / 1000) as u64 } else { 0 };
        // Tiles expand tool calls on click too (they share the toggles with the chat view).
        let (tog, all) = (chat.toggled.get(&m.id), chat.show_all.get(&m.id));
        let key = if compact {
            hash_key(&[
                ver,
                w as u64,
                set_hash(tog),
                set_hash(all),
                anim,
                secs,
                stale as u64,
                (i > 0) as u64,
                11,
            ])
        } else {
            hash_key(&[
                ver,
                w as u64,
                set_hash(tog),
                set_hash(all),
                env.details as u64,
                env.thinking as u64,
                anim,
                secs,
                stale as u64,
                context_start as u64,
                7,
            ])
        };
        let cache = if compact {
            &chat.tile_cache
        } else {
            &chat.cache
        };
        if !cache.get(&m.id).is_some_and(|c| c.key == key) {
            let cx = Ctx {
                agents: env.agents,
                models: env.models,
                toggled: tog,
                show_all: all,
                details: env.details && !compact,
                thinking: env.thinking && !compact,
                now: env.now,
                tick: env.tick,
                compact,
                session_agent: &info.agent,
                stale,
                context_start: context_start && !compact,
            };
            // Blank separator above each message (tiles: not above the first one).
            let mut rows = if compact && i == 0 {
                Vec::new()
            } else {
                vec![Row::blank()]
            };
            rows.extend(message::render(m, &cx, w));
            #[cfg(test)]
            {
                chat.laid_out += 1;
            }
            let cache = if compact {
                &mut chat.tile_cache
            } else {
                &mut chat.cache
            };
            cache.insert(m.id.clone(), Cached { key, rows });
        }
        let cache = if compact {
            &chat.tile_cache
        } else {
            &chat.cache
        };
        offsets.push(total);
        ids.push(m.id.clone());
        total += cache.get(&m.id).map(|c| c.rows.len()).unwrap_or(0);
    }
    // Evict cache entries for removed messages.
    let cache = if compact {
        &mut chat.tile_cache
    } else {
        &mut chat.cache
    };
    if cache.len() > ids.len() + 8 {
        let keep: std::collections::HashSet<&String> = ids.iter().collect();
        cache.retain(|k, _| keep.contains(k));
    }
    offsets.push(total);
    // Older messages loaded above: keep the same content on screen (tiles have no separator
    // above the first message, so the old first one gained a row).
    let shift = chat
        .ids
        .first()
        .and_then(|old| ids.iter().position(|id| id == old))
        .filter(|&k| k > 0)
        .map_or(0, |k| offsets[k] + usize::from(compact));
    if shift > 0 {
        chat.pos += shift as f64;
        chat.target += shift as f64;
        chat.last_total += shift;
        if let Some(s) = &mut chat.sel {
            s.anchor.0 += shift;
            s.head.0 += shift;
        }
        if let Some(p) = &mut chat.press {
            p.0 += shift;
        }
    }
    chat.offsets = offsets;
    chat.ids = ids;
    chat.animating = animating;
    total += chat.tail.len();
    chat.total = total;
}

/// Follow / clamp bookkeeping after a layout pass for a viewport of `area`.
fn settle(chat: &mut ChatState, area: Rect, width_changed: bool, busy: bool) {
    chat.area = area;
    if width_changed {
        chat.release_hold();
    }
    chat.update_hold(busy);
    let max = chat.max_scroll();
    if !chat.initialized {
        chat.initialized = true;
        chat.follow = true;
        chat.pos = max;
        chat.target = max;
    }
    // Content grew while scrolled up (re-wrapping after a resize doesn't count).
    if chat.total > chat.last_total && !chat.follow && chat.last_total > 0 && !width_changed {
        chat.new_below = true;
        chat.new_rows += chat.total - chat.last_total;
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
}

/// Rows shown after the transcript: retry status, error, queued prompts.
fn tail_rows(view: &SessionView, env: &Env<'_>, w: usize) -> Vec<Row> {
    let mut rows = Vec::new();
    if let SessionStatus::Retrying {
        attempt,
        next_at,
        error,
    } = &view.info.status
    {
        rows.push(Row::blank());
        let secs = ((next_at - env.now).max(0) + 999) / 1000;
        let msg = format!(
            "{} Retrying in {secs}s (attempt {attempt}): {}",
            theme::spinner(env.tick),
            text::one_line(error)
        );
        for (i, (l, sp)) in text::wrap_spans_flags(
            &[Span::styled(msg, theme::fg(theme::WARNING))],
            w.saturating_sub(4),
        )
        .into_iter()
        .enumerate()
        {
            rows.push(Row::new(l).soft_sp(i > 0, sp).indent(vec![
                Span::raw("  "),
                Span::styled("┃ ", theme::fg(theme::WARNING)),
            ]));
        }
    }
    if let SessionStatus::Compacting = view.info.status {
        rows.push(Row::blank());
        let mut spans = vec![
            Span::raw("  "),
            Span::styled(
                format!("{} ", theme::spinner(env.tick)),
                theme::fg(theme::ACCENT),
            ),
        ];
        spans.extend(message::shimmer(
            "Compacting context…",
            env.tick,
            theme::MUTED,
            theme::ACCENT,
        ));
        rows.push(Row::new(Line::from(spans)).decor());
    }
    if let Some(e) = &view.error
        && !view
            .messages
            .last()
            .is_some_and(|m| m.error.as_deref() == Some(e.as_str()))
    {
        rows.push(Row::blank());
        for (i, (l, sp)) in text::wrap_spans_flags(
            &[Span::styled(text::one_line(e), theme::fg(theme::ERROR))],
            w.saturating_sub(4),
        )
        .into_iter()
        .enumerate()
        {
            rows.push(Row::new(l).soft_sp(i > 0, sp).indent(vec![
                Span::raw("  "),
                Span::styled("┃ ", theme::fg(theme::ERROR)),
            ]));
        }
    }
    if !view.queued.is_empty() {
        rows.push(Row::blank());
        rows.extend(queued_rows(view, w));
    }
    rows.push(Row::blank().decor());
    rows
}

/// One row per queued prompt, with a `✕ remove` button on the ones this user sent.
fn queued_rows(view: &SessionView, w: usize) -> Vec<Row> {
    let mut rows = Vec::new();
    for q in &view.queued {
        let mut left = vec![Span::styled(
            "  queued ",
            theme::fg(theme::DIM).add_modifier(Modifier::BOLD),
        )];
        if let Some(from) = &q.from {
            left.push(Span::styled(
                format!("from {from} "),
                theme::fg(theme::ACCENT),
            ));
        }
        left.push(Span::styled(text::one_line(&q.text), theme::fg(theme::DIM)));
        // Messages from other sessions are shown only.
        let label = if q.from.is_some() {
            ""
        } else if q.removing {
            "removing…"
        } else {
            message::UNQUEUE_LABEL
        };
        let right = if label.is_empty() {
            vec![]
        } else {
            vec![Span::styled(label, theme::fg(theme::DIM))]
        };
        let (line, rx) = message::compose(left, right, w);
        let mut row = Row::new(line).decor();
        if !q.removing && q.from.is_none() {
            let x0 = rx as u16;
            row = row.zone(x0, x0 + width(label) as u16, Hit::Unqueue { key: q.key });
        }
        rows.push(row);
    }
    rows
}

fn draw_welcome(buf: &mut Buffer, area: Rect, view: &SessionView, loaded: bool) {
    if area.height < 6 || area.width < 20 {
        return;
    }
    let mut lines: Vec<Line<'static>> = Vec::new();
    for l in crate::app::LOGO.iter() {
        lines.push(Line::from(Span::styled(
            l.to_string(),
            theme::bold(theme::PRIMARY),
        )));
    }
    lines.push(Line::default());
    if !loaded {
        lines.push(Line::from(Span::styled("connecting…", theme::muted())));
    } else {
        lines.push(Line::from(vec![
            Span::styled("session ", theme::muted()),
            Span::styled(view.info.name.clone(), theme::text()),
            Span::styled(
                format!("  ·  {}", text::tilde(&view.info.cwd)),
                theme::muted(),
            ),
        ]));
        lines.push(Line::default());
        for (k, d) in [
            ("/", "commands"),
            ("@", "attach files"),
            ("!", "run shell command"),
            ("ctrl+p", "command palette"),
            ("ctrl+x l", "sessions"),
            ("ctrl+x w", "background sessions"),
        ] {
            lines.push(Line::from(vec![
                Span::styled(format!("{k:>10}  "), theme::fg(theme::PRIMARY)),
                Span::styled(format!("{d:<20}"), theme::muted()),
            ]));
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
    let reflow = view.chat.last_w != cw || view.chat.compact;
    layout(view, env, cw, false);
    view.chat.last_w = cw;
    let loaded = view.loaded;
    let busy = view.is_busy();
    let chat = &mut view.chat;
    chat.content_x = area.x + 1;
    settle(chat, area, reflow, busy);
    chat.pill = None;

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
                        cell.set_style(
                            Style::new().fg(theme::PRIMARY).add_modifier(Modifier::BOLD),
                        );
                    }
                }
            } else {
                // Brighten the disclosure arrow (first non-blank cell).
                let lw = (text::line_width(&row.line) as u16).min(cw as u16);
                if let Some(x) = (0..lw).find(|&x| {
                    buf.cell((chat.content_x + x, y))
                        .is_some_and(|c| !c.symbol().trim().is_empty())
                }) && let Some(c) = buf.cell_mut((chat.content_x + x, y))
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
                let to = if gi == r1 {
                    c1.min(lw.saturating_sub(1))
                } else {
                    lw.saturating_sub(1)
                };
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
    if chat.scroll_total() > area.height as usize {
        draw_scrollbar(
            buf,
            Rect::new(area.x + area.width - 1, area.y, 1, area.height),
            chat.scroll_total(),
            chat.pos,
            chat.max_scroll(),
            ("┃", theme::BORDER_ACTIVE),
            Some(("│", theme::BORDER_SUBTLE)),
        );
    }
    // "new messages" pill.
    if chat.new_below && !chat.follow {
        let pill = draw_pill(buf, area, " ↓ new messages ");
        view.chat.pill = pill;
    }
}

/// Draws a vertical scrollbar into the one-column rect `r` for `total` rows scrolled to `pos`.
/// `track: None` keeps whatever is drawn under the track (e.g. a tile border).
pub fn draw_scrollbar(
    buf: &mut Buffer,
    r: Rect,
    total: usize,
    pos: f64,
    max: f64,
    thumb_sym: (&str, ratatui::style::Color),
    track: Option<(&str, ratatui::style::Color)>,
) {
    let h = r.height as usize;
    if h == 0 || total <= h {
        return;
    }
    let thumb = ((h * h) as f64 / total as f64).ceil().clamp(1.0, h as f64) as usize;
    let start = ((pos / max.max(1.0)) * (h - thumb) as f64).round() as usize;
    for i in 0..h {
        let Some((sym, c)) = (if i >= start && i < start + thumb {
            Some(thumb_sym)
        } else {
            track
        }) else {
            continue;
        };
        if let Some(cell) = buf.cell_mut((r.x, r.y + i as u16)) {
            cell.set_symbol(sym).set_style(Style::new().fg(c));
        }
    }
}

/// Bottom-centred pill in `area`; returns its rect.
fn draw_pill(buf: &mut Buffer, area: Rect, label: &str) -> Option<Rect> {
    let label = text::truncate(label, area.width as usize);
    let lw = width(&label) as u16;
    if lw == 0 || area.height == 0 {
        return None;
    }
    let x = area.x + area.width.saturating_sub(lw) / 2;
    let y = area.y + area.height - 1;
    // Clear the row first so wide glyphs cut by the pill don't leave half cells behind.
    for xx in x..x + lw {
        if let Some(c) = buf.cell_mut((xx, y)) {
            c.reset();
        }
    }
    buf.set_string(
        x,
        y,
        &label,
        Style::new()
            .fg(theme::PANEL)
            .bg(theme::PRIMARY)
            .add_modifier(Modifier::BOLD),
    );
    Some(Rect::new(x, y, lw, 1))
}

/// Draws the compact transcript of a split tile into `area` (content columns), with the scroll
/// position shown on the tile's right border column `bar_x` while scrolled up. Uses the same
/// scroll state as the main view (wheel / PgUp / PgDn / End, follow at bottom).
pub fn draw_tile(buf: &mut Buffer, area: Rect, bar_x: u16, view: &mut SessionView, env: &Env<'_>) {
    if area.width == 0 || area.height == 0 {
        view.chat.area = area;
        return;
    }
    let cw = area.width as usize;
    // Queued prompts under the transcript (no trailing blank: tiles are small).
    view.chat.tail = if view.queued.is_empty() {
        Vec::new()
    } else {
        std::iter::once(Row::blank())
            .chain(queued_rows(view, cw))
            .collect()
    };
    let reflow = view.chat.last_w != cw || !view.chat.compact;
    layout(view, env, cw, true);
    view.chat.last_w = cw;
    let busy = view.is_busy();
    let chat = &mut view.chat;
    chat.content_x = area.x;
    settle(chat, area, reflow, busy);
    chat.pill = None;
    let top = chat.top();
    for dy in 0..area.height {
        let Some(row) = chat.row(top + dy as usize) else {
            break;
        };
        buf.set_line(area.x, area.y + dy, &row.line, area.width);
    }
    if chat.follow || chat.scroll_total() <= area.height as usize {
        return;
    }
    // Scrolled up: thumb on the right border; "↓ N new lines" once more content arrives.
    draw_scrollbar(
        buf,
        Rect::new(bar_x, area.y, 1, area.height),
        chat.scroll_total(),
        chat.pos,
        chat.max_scroll(),
        ("┃", theme::MUTED),
        None,
    );
    if chat.new_rows > 0 {
        let n = chat.new_rows;
        let pill = draw_pill(
            buf,
            area,
            &format!(" ↓ {n} new line{} ", if n == 1 { "" } else { "s" }),
        );
        view.chat.pill = pill;
    }
}

/// Whether the "new messages" pill (or a tile's "↓" marker) was clicked.
pub fn pill_hit(chat: &ChatState, x: u16, y: u16) -> bool {
    chat.pill
        .is_some_and(|r| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height)
}

#[cfg(test)]
mod hold_tests {
    use super::*;

    const HEIGHT: u16 = 10;

    /// An idle, following view over `total` rows (the first pass only initialises it).
    fn following(total: usize) -> ChatState {
        let mut chat = ChatState::default();
        set_total(&mut chat, total, false);
        set_total(&mut chat, total, false);
        assert!(chat.follow);
        chat
    }

    /// Lays out `total` rows in a `HEIGHT`-row viewport, like one draw pass, and lets the
    /// scroll easing finish.
    fn set_total(chat: &mut ChatState, total: usize, busy: bool) {
        chat.total = total;
        settle(chat, Rect::new(0, 0, 40, HEIGHT), false, busy);
        while chat.step_anim() {}
    }

    /// A following view whose turn has just started over `total` rows.
    fn mid_turn(total: usize) -> ChatState {
        let mut chat = following(total);
        set_total(&mut chat, total, true);
        chat
    }

    #[test]
    fn idle_view_follows_the_end_down() {
        let mut chat = following(50);
        set_total(&mut chat, 46, false);
        assert_eq!(chat.top(), 36);
    }

    #[test]
    fn busy_view_keeps_its_position_when_content_shrinks() {
        let mut chat = mid_turn(50);
        set_total(&mut chat, 46, true);
        assert_eq!(chat.top(), 40, "the end did not move up");
        assert_eq!(chat.hold.space, 4);
        assert_eq!(chat.max_scroll(), 40.0);
    }

    #[test]
    fn content_grows_back_into_the_held_space() {
        let mut chat = mid_turn(50);
        set_total(&mut chat, 46, true);
        set_total(&mut chat, 49, true);
        assert_eq!(chat.top(), 40);
        assert_eq!(chat.hold.space, 1);
        set_total(&mut chat, 53, true);
        assert_eq!(chat.top(), 43, "past the old end the view follows again");
        assert_eq!(chat.hold.space, 0);
    }

    #[test]
    fn end_of_turn_releases_the_space_at_once() {
        let mut chat = following(50);
        set_total(&mut chat, 46, true);
        set_total(&mut chat, 46, false);
        assert_eq!(chat.top(), 36);
        assert_eq!(chat.hold.space, 0);
    }

    #[test]
    fn held_space_is_capped_below_the_viewport_height() {
        let mut chat = mid_turn(50);
        set_total(&mut chat, 20, true);
        assert_eq!(chat.hold.space, HEIGHT as usize - 1);
        assert_eq!(chat.top(), 19, "the last row is still visible");
        // Later growth fills the capped space; the view stays put until it is used up.
        set_total(&mut chat, 25, true);
        assert_eq!(chat.hold.space, 4);
        assert_eq!(chat.top(), 19);
    }

    #[test]
    fn nothing_is_held_while_scrolled_up() {
        let mut chat = mid_turn(50);
        chat.scroll_by(-5, false);
        assert!(!chat.follow);
        set_total(&mut chat, 46, true);
        assert_eq!(chat.hold.space, 0);
        assert_eq!(chat.top(), 35);
    }

    #[test]
    fn following_again_mid_turn_starts_from_the_current_extent() {
        let mut chat = mid_turn(50);
        set_total(&mut chat, 48, true);
        assert_eq!(chat.hold.space, 2);
        chat.scroll_by(-5, false);
        // The end moves while the user reads further up: nothing is held for it.
        set_total(&mut chat, 40, true);
        assert_eq!(chat.hold.space, 2);
        chat.scroll_to_bottom(false);
        set_total(&mut chat, 40, true);
        assert_eq!(chat.hold.space, 2, "the old high-water mark is not revived");
        assert_eq!(chat.top(), 32);
        set_total(&mut chat, 37, true);
        assert_eq!(chat.hold.space, 5);
        assert_eq!(chat.top(), 32);
    }

    #[test]
    fn rewrapping_drops_the_held_space() {
        let mut chat = mid_turn(50);
        set_total(&mut chat, 46, true);
        chat.total = 30;
        settle(&mut chat, Rect::new(0, 0, 30, HEIGHT), true, true);
        assert_eq!(chat.hold.space, 0);
        assert_eq!(chat.top(), 20);
    }

    #[test]
    fn new_turn_starts_without_leftover_space() {
        let mut chat = following(50);
        set_total(&mut chat, 46, true);
        set_total(&mut chat, 46, false);
        set_total(&mut chat, 46, true);
        assert_eq!(chat.hold.space, 0);
        set_total(&mut chat, 44, true);
        assert_eq!(chat.hold.space, 2);
    }
}
