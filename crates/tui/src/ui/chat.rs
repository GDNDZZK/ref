//! Transcript view: per-message render cache, scrolling (with easing), hit testing, selection.

use std::collections::{BTreeSet, HashMap};

use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ref_proto::{
    AgentInfo, MessageLayoutMeta, ModelInfo, Role, SOURCE_KIND_COMPACTION, SessionStatus,
};

use super::message::{self, Ctx};
use super::selection::{self, Selection};
use super::{Hit, Row, theme};
use crate::state::{MessageSlot, SessionView, estimate_lines};
use crate::text::{self, width};

/// Shared rendering environment.
pub struct Env<'a> {
    pub agents: &'a [AgentInfo],
    pub models: &'a [ModelInfo],
    pub details: bool,
    pub thinking: bool,
    pub tick: u64,
    pub now: i64,
}

/// FNV-1a parameters of `hash_key`.
const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;
/// Seed and multiplier of the order-sensitive hash of a toggle set.
const SET_HASH_SEED: u64 = 7;
const SET_HASH_MUL: u64 = 31;
/// Distinguish the compact, full, and placeholder cache keys of the same message.
const COMPACT_KEY_SALT: u64 = 11;
const FULL_KEY_SALT: u64 = 7;
const PLACEHOLDER_KEY_SALT: u64 = 17;
/// A scroll target this close to the end counts as following.
const FOLLOW_EPSILON: f64 = 0.01;
/// The scroll animation stops within this many rows of the target.
const ANIM_STOP_DISTANCE: f64 = 0.5;
/// Share of the remaining distance covered per animation frame.
const EASE_FACTOR: f64 = 0.4;
/// The welcome screen is skipped on a smaller area.
const WELCOME_MIN_H: u16 = 6;
const WELCOME_MIN_W: u16 = 20;
/// Column widths of the key hints on the welcome screen.
const HINT_KEY_W: usize = 10;
const HINT_DESC_W: usize = 20;
/// Cache entries beyond the message count that are kept before evicting.
const CACHE_SLACK: usize = 8;
/// Label of the clickable button on the `Retrying` banner.
const RETRY_NOW_LABEL: &str = "↻ retry now";
struct Cached {
    key: u64,
    rows: Vec<Row>,
}

#[derive(Default)]
pub struct ChatState {
    cache: HashMap<String, Cached>,
    /// Messages laid out (cache misses) so far; tests check what a frame re-renders.
    #[cfg(test)]
    pub(crate) laid_out: usize,
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
    new_below: bool,
    last_total: usize,
    /// Last drawn viewport (content area) and its height.
    pub area: Rect,
    content_x: u16,
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
    new_rows: usize,
    /// Screen rect of the "new messages" pill at last draw.
    pub pill: Option<Rect>,
    /// Content width of the last layout (re-wrapping is not "new content").
    last_w: usize,
    /// Sequence range `[start, end)` overlapping the viewport at the last draw.
    pub visible_seq_range: Option<(usize, usize)>,
}
fn hash_key(parts: &[u64]) -> u64 {
    let mut h: u64 = FNV_OFFSET;
    for p in parts {
        h ^= *p;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

fn set_hash(s: Option<&BTreeSet<usize>>) -> u64 {
    s.map(|s| {
        s.iter().fold(SET_HASH_SEED, |a, x| {
            a.wrapping_mul(SET_HASH_MUL).wrapping_add(*x as u64 + 1)
        })
    })
    .unwrap_or(0)
}

impl ChatState {
    fn cache(&self, compact: bool) -> &HashMap<String, Cached> {
        if compact {
            &self.tile_cache
        } else {
            &self.cache
        }
    }

    fn cache_mut(&mut self, compact: bool) -> &mut HashMap<String, Cached> {
        if compact {
            &mut self.tile_cache
        } else {
            &mut self.cache
        }
    }

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
        self.follow = t >= max - FOLLOW_EPSILON;
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
        if d.abs() < ANIM_STOP_DISTANCE {
            self.pos = self.target;
            return false;
        }
        // Ease out: cover ~40% of the remaining distance each frame (≈120ms at 60fps).
        let step = d * EASE_FACTOR;
        self.pos += if step.abs() < 1.0 { d.signum() } else { step };
        true
    }

    pub fn scrolling(&self) -> bool {
        (self.target - self.pos).abs() >= ANIM_STOP_DISTANCE
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
        let cache = self.cache(self.compact);
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
        self.area.contains(Position::new(x, y))
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
                    self.sel = Some(Selection::at((r, a), (r, b), true));
                }
            }
            3 => {
                if let Some(row) = self.row(r) {
                    let e = selection::row_end(row);
                    self.sel = Some(Selection::at((r, row.skip), (r, e), true));
                }
            }
            _ => {
                self.sel = Some(Selection::at((r, c), (r, c), false));
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
        if press.map(|p| p.0) != Some(r) {
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
fn last_compaction(slots: &[MessageSlot]) -> Option<usize> {
    slots.iter().rposition(MessageSlot::is_compaction)
}

/// What the cache key of one message depends on besides its content.
struct Slot {
    index: usize,
    version: u64,
    /// The message is still streaming, so its rows change with the tick and the clock.
    live: bool,
    stale: bool,
    /// The message is the last compaction, where the model's context starts.
    context_start: bool,
}

/// Hash of everything that decides how message `id` renders.
fn cache_key(
    chat: &ChatState,
    env: &Env<'_>,
    id: &str,
    slot: &Slot,
    w: usize,
    compact: bool,
) -> u64 {
    let (tog, all) = (chat.toggled.get(id), chat.show_all.get(id));
    let anim = if slot.live { env.tick } else { 0 };
    let secs = if slot.live {
        (env.now / 1000) as u64
    } else {
        0
    };
    // Tiles expand tool calls on click too (they share the toggles with the chat view).
    if compact {
        return hash_key(&[
            slot.version,
            w as u64,
            set_hash(tog),
            set_hash(all),
            anim,
            secs,
            slot.stale as u64,
            (slot.index > 0) as u64,
            COMPACT_KEY_SALT,
        ]);
    }
    hash_key(&[
        slot.version,
        w as u64,
        set_hash(tog),
        set_hash(all),
        env.details as u64,
        env.thinking as u64,
        anim,
        secs,
        slot.stale as u64,
        slot.context_start as u64,
        FULL_KEY_SALT,
    ])
}

fn placeholder_cache_key(
    meta: &MessageLayoutMeta,
    version: u64,
    w: usize,
    env: &Env<'_>,
    stale: bool,
    compact: bool,
    first: bool,
) -> u64 {
    hash_key(&[
        version,
        meta.seq,
        w as u64,
        env.details as u64,
        env.thinking as u64,
        stale as u64,
        compact as u64,
        first as u64,
        PLACEHOLDER_KEY_SALT,
    ])
}

fn render_placeholder(
    meta: &MessageLayoutMeta,
    w: usize,
    env: &Env<'_>,
    stale: bool,
    compact: bool,
) -> Vec<Row> {
    let est = estimate_lines(meta, w, env.details, env.thinking, compact);
    let mut rows = Vec::with_capacity(est);
    let dim = if stale {
        theme::fg(theme::BORDER_SUBTLE)
    } else {
        theme::fg(theme::DIM)
    };
    match meta.role {
        Role::User if meta.source_kind == SOURCE_KIND_COMPACTION => {
            rows.push(
                Row::new(Line::from(Span::styled(
                    "──── ◇ Context compacted ···",
                    dim,
                )))
                .decor(),
            );
        }
        Role::User => {
            let bar = Span::styled("┃", theme::fg(theme::BORDER));
            rows.push(Row::new(Line::from(vec![bar.clone(), Span::styled("  ···", dim)])).decor());
            for _ in 1..est {
                rows.push(Row::new(Line::from(bar.clone())).decor());
            }
            return rows;
        }
        Role::Assistant => {
            rows.push(Row::new(Line::from(Span::styled("  ···", dim))).decor());
        }
    }
    while rows.len() < est {
        rows.push(Row::blank().decor());
    }
    rows
}

/// Builds (or reuses) cached rows for all slots and the tail, for content width `w`.
/// `compact` selects the split-tile rendering (own cache; tools / reasoning one-line).
fn layout(view: &mut SessionView, env: &Env<'_>, w: usize, compact: bool) {
    let busy = !view.info.status.is_idle();
    let SessionView {
        slots,
        versions,
        chat,
        info,
        ..
    } = view;
    let mut offsets = std::mem::take(&mut chat.offsets);
    offsets.clear();
    offsets.reserve(slots.len() + 1);
    let mut total = 0usize;
    let mut animating = false;
    let last_comp = last_compaction(slots);
    chat.compact = compact;
    for (i, slot_entry) in slots.iter().enumerate() {
        let stale = last_comp.is_some_and(|c| i < c);
        let ver = versions.get(i).copied().unwrap_or(0);
        let id = slot_entry.id();
        match slot_entry {
            MessageSlot::Materialized(m) => {
                // Only the tail of a busy session animates (stale unfinished messages render statically).
                let live = busy && crate::state::message_live(m);
                animating |= live;
                let slot = Slot {
                    index: i,
                    version: ver,
                    live,
                    stale,
                    context_start: i > 0 && last_comp == Some(i),
                };
                let key = cache_key(chat, env, id, &slot, w, compact);
                if !chat.cache(compact).get(id).is_some_and(|c| c.key == key) {
                    let cx = Ctx {
                        agents: env.agents,
                        models: env.models,
                        toggled: chat.toggled.get(id),
                        show_all: chat.show_all.get(id),
                        details: env.details && !compact,
                        thinking: env.thinking && !compact,
                        now: env.now,
                        tick: env.tick,
                        compact,
                        session_agent: &info.agent,
                        stale: slot.stale,
                        context_start: slot.context_start && !compact,
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
                    chat.cache_mut(compact)
                        .insert(m.id.clone(), Cached { key, rows });
                }
            }
            MessageSlot::Placeholder(meta) => {
                let key = placeholder_cache_key(meta, ver, w, env, stale, compact, i == 0);
                if !chat.cache(compact).get(id).is_some_and(|c| c.key == key) {
                    let mut rows = if compact && i == 0 {
                        Vec::new()
                    } else {
                        vec![Row::blank()]
                    };
                    rows.extend(render_placeholder(meta, w, env, stale, compact));
                    chat.cache_mut(compact)
                        .insert(id.to_string(), Cached { key, rows });
                }
            }
        }
        offsets.push(total);
        // The entry exists: it either matched the key or was inserted above.
        total += chat.cache(compact)[id].rows.len();
    }
    evict_stale(chat.cache_mut(compact), slots);
    offsets.push(total);
    let ids_match = chat.ids.len() == slots.len()
        && chat.ids.iter().zip(slots.iter()).all(|(a, s)| a == s.id());
    if !ids_match {
        shift_for_prepended(chat, slots, &offsets, compact);
        chat.ids.clear();
        chat.ids
            .reserve(slots.len().saturating_sub(chat.ids.capacity()));
        chat.ids.extend(slots.iter().map(|s| s.id().to_string()));
    }
    chat.offsets = offsets;
    chat.animating = animating;
    total += chat.tail.len();
    chat.total = total;
}

fn compute_visible_seq_range(chat: &ChatState) -> Option<(usize, usize)> {
    let msg_count = chat.ids.len();
    let h = chat.area.height as usize;
    if msg_count == 0 || h == 0 || chat.offsets.len() <= msg_count {
        return None;
    }
    let top = chat.top();
    let bottom = top.saturating_add(h);
    let vis_start = chat.offsets[1..=msg_count].partition_point(|&end| end <= top);
    let vis_end = chat.offsets[..msg_count]
        .partition_point(|&start| start < bottom)
        .min(msg_count);
    (vis_start < vis_end).then_some((vis_start, vis_end))
}

/// Evicts cache entries for removed messages once enough have piled up.
fn evict_stale(cache: &mut HashMap<String, Cached>, slots: &[MessageSlot]) {
    if cache.len() <= slots.len() + CACHE_SLACK {
        return;
    }
    let keep: std::collections::HashSet<&str> = slots.iter().map(MessageSlot::id).collect();
    cache.retain(|k, _| keep.contains(k.as_str()));
}

/// Older messages loaded above: keep the same content on screen (tiles have no separator
/// above the first message, so the old first one gained a row).
fn shift_for_prepended(
    chat: &mut ChatState,
    slots: &[MessageSlot],
    offsets: &[usize],
    compact: bool,
) {
    let shift = chat
        .ids
        .first()
        .and_then(|old| slots.iter().position(|s| s.id() == old))
        .filter(|&k| k > 0)
        .map_or(0, |k| offsets[k] + usize::from(compact));
    if shift == 0 {
        return;
    }
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

/// Reading position relative to the message at the top of the viewport, preserved across reflows.
struct ReadingAnchor {
    /// `Some(id)` for a message block, `None` for the tail rows below all messages.
    msg_id: Option<String>,
    msg_idx: usize,
    frac: f64,
}

fn capture_anchor(chat: &ChatState) -> Option<ReadingAnchor> {
    if chat.offsets.is_empty() || chat.total == 0 {
        return None;
    }
    let y = chat.target.max(0.0);
    let msg_count = chat.ids.len();
    let k = chat
        .offsets
        .partition_point(|&start| (start as f64) <= y)
        .saturating_sub(1);
    if k < msg_count {
        let start = chat.offsets[k] as f64;
        let len = (chat.offsets[k + 1] - chat.offsets[k]) as f64;
        let frac = if len > 0.0 {
            ((y - start) / len).clamp(0.0, 1.0)
        } else {
            0.0
        };
        return Some(ReadingAnchor {
            msg_id: Some(chat.ids[k].clone()),
            msg_idx: k,
            frac,
        });
    }
    let start = *chat.offsets.last()? as f64;
    let len = (chat.total as f64 - start).max(0.0);
    let frac = if len > 0.0 {
        ((y - start) / len).clamp(0.0, 1.0)
    } else {
        0.0
    };
    Some(ReadingAnchor {
        msg_id: None,
        msg_idx: msg_count,
        frac,
    })
}

fn restore_anchor(chat: &mut ChatState, anchor: &ReadingAnchor) {
    if chat.offsets.is_empty() {
        return;
    }
    let msg_count = chat.ids.len();
    let (start, len) = if let Some(id) = &anchor.msg_id {
        let idx = chat
            .ids
            .iter()
            .position(|x| x == id)
            .or_else(|| (anchor.msg_idx < msg_count).then_some(anchor.msg_idx));
        if let Some(k) = idx {
            (
                chat.offsets[k] as f64,
                (chat.offsets[k + 1] - chat.offsets[k]) as f64,
            )
        } else {
            (*chat.offsets.last().unwrap_or(&0) as f64, 0.0)
        }
    } else {
        let s = *chat.offsets.last().unwrap_or(&0) as f64;
        (s, (chat.total as f64 - s).max(0.0))
    };
    let new_y = start + anchor.frac * len;
    chat.target = new_y;
    chat.pos = new_y;
}

/// Follow / clamp bookkeeping after a layout pass for a viewport of `area`.
fn settle(chat: &mut ChatState, area: Rect, reflow: bool) {
    let height_changed = chat.area.height != area.height;
    let was_scrolling = chat.scrolling();
    chat.area = area;
    let max = chat.max_scroll();
    if !chat.initialized {
        chat.initialized = true;
        chat.follow = true;
        chat.pos = max;
        chat.target = max;
    }
    // Content grew while scrolled up (re-wrapping after a resize doesn't count).
    if chat.total > chat.last_total && !chat.follow && chat.last_total > 0 && !reflow {
        chat.new_below = true;
        chat.new_rows += chat.total - chat.last_total;
    }
    chat.last_total = chat.total;
    // Shrinking content (e.g. collapsing a card) that brings `target` to/past the end resumes follow.
    if !chat.follow && chat.target >= max - FOLLOW_EPSILON {
        chat.follow = true;
        chat.new_below = false;
        chat.new_rows = 0;
    }
    if chat.follow {
        let resized = reflow || height_changed;
        if resized {
            chat.target = max;
            chat.pos = max;
        } else if (chat.target - max).abs() > FOLLOW_EPSILON {
            chat.target = max;
            // Stick to bottom immediately unless an explicit smooth scroll to bottom is in flight.
            if !was_scrolling || chat.pos > max {
                chat.pos = max;
            }
        }
    }
    chat.target = chat.target.clamp(0.0, max);
    chat.pos = chat.pos.clamp(0.0, max);
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
        let secs = text::ceil_secs(next_at - env.now);
        let status_text = format!(
            "{} Retrying in {secs}s (attempt {attempt})",
            theme::spinner(env.tick)
        );
        let inner = w.saturating_sub(4).max(1);
        let left = vec![Span::styled(status_text, theme::fg(theme::WARNING))];
        let right = vec![Span::styled(RETRY_NOW_LABEL, theme::fg(theme::WARNING))];
        let (line, rx) = message::compose(left, right, inner);
        let x0 = rx as u16;
        let rw = width(RETRY_NOW_LABEL) as u16;
        let lead = vec![
            Span::raw("  "),
            Span::styled("┃ ", theme::fg(theme::WARNING)),
        ];
        let mut header_row = Row::new(line).decor();
        if rx + rw as usize <= inner {
            header_row = header_row.zone(x0, x0 + rw, Hit::RetryNow);
        }
        rows.push(header_row.indent(lead));
        let err_line = text::one_line(error);
        if !err_line.is_empty() {
            rows.extend(message::bar_rows(
                &err_line,
                theme::fg(theme::WARNING),
                theme::WARNING,
                inner,
                2,
            ));
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
            .last_message()
            .is_some_and(|m| m.error.as_deref() == Some(e.as_str()))
    {
        rows.push(Row::blank());
        rows.extend(message::bar_rows(
            &text::one_line(e),
            theme::fg(theme::ERROR),
            theme::ERROR,
            w.saturating_sub(4),
            2,
        ));
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
    if area.height < WELCOME_MIN_H || area.width < WELCOME_MIN_W {
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
                Span::styled(format!("{k:>HINT_KEY_W$}  "), theme::fg(theme::PRIMARY)),
                Span::styled(format!("{d:<HINT_DESC_W$}"), theme::muted()),
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
    if view.chat.follow && !view.is_window_unlimited() && view.slots.len() > view.tail_keep() {
        let keep = view.tail_keep();
        view.trim_to_tail(keep);
    }
    // 1 col left pad, 1 col right pad + scrollbar column.
    let cw = area.width.saturating_sub(3).max(1) as usize;
    let tail = tail_rows(view, env, cw);
    view.chat.tail = tail;
    let reflow = view.chat.last_w != cw || view.chat.compact;
    let restore = reflow || view.needs_anchor_restore;
    let anchor = if restore && !view.chat.follow && view.chat.initialized {
        capture_anchor(&view.chat)
    } else {
        None
    };
    view.needs_anchor_restore = false;
    layout(view, env, cw, false);
    view.chat.last_w = cw;
    let loaded = view.loaded;
    let (follow, visible_seq_range) = {
        let chat = &mut view.chat;
        if let Some(a) = &anchor {
            restore_anchor(chat, a);
        }
        chat.content_x = area.x + 1;
        settle(chat, area, reflow);
        chat.visible_seq_range = compute_visible_seq_range(chat);
        (chat.follow, chat.visible_seq_range)
    };
    if !follow
        && !view.is_window_unlimited()
        && view.resident_count() > view.message_window_size
        && let Some((vs, ve)) = visible_seq_range
    {
        view.evict_outside(vs, ve);
    }

    if view.is_empty() && !view.is_busy() {
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
        if let Some((hr, hc)) = hover
            && hr == gi
        {
            style_hover_target(buf, chat, row, y, hc, cw);
        }
        if let Some(sel) = chat.sel.as_ref().filter(|s| s.active) {
            paint_selection(buf, chat, sel, row, gi, y, cw);
        }
    }
    // Scrollbar.
    if chat.total > area.height as usize {
        draw_scrollbar(
            buf,
            Rect::new(area.x + area.width - 1, area.y, 1, area.height),
            chat.total,
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

/// Hover feedback on the click target under column `hc` of `row` (drawn at screen row `y`).
fn style_hover_target(buf: &mut Buffer, chat: &ChatState, row: &Row, y: u16, hc: u16, cw: usize) {
    let Some(z) = row.hits.iter().find(|z| hc >= z.x0 && hc < z.x1) else {
        return;
    };
    let hover = Style::new().fg(theme::PRIMARY).add_modifier(Modifier::BOLD);
    if z.hit.is_button() {
        for x in z.x0..z.x1.min(cw as u16) {
            if let Some(cell) = buf.cell_mut((chat.content_x + x, y)) {
                cell.set_style(hover);
            }
        }
        return;
    }
    // Brighten the disclosure arrow (first non-blank cell).
    let lw = (text::line_width(&row.line) as u16).min(cw as u16);
    let arrow = (0..lw).find(|&x| {
        buf.cell((chat.content_x + x, y))
            .is_some_and(|c| !c.symbol().trim().is_empty())
    });
    if let Some(x) = arrow
        && let Some(c) = buf.cell_mut((chat.content_x + x, y))
    {
        c.set_style(hover);
    }
}

/// Highlights the part of `sel` that falls on row `gi` (drawn at screen row `y`).
fn paint_selection(
    buf: &mut Buffer,
    chat: &ChatState,
    sel: &Selection,
    row: &Row,
    gi: usize,
    y: u16,
    cw: usize,
) {
    let ((r0, c0), (r1, c1)) = sel.range();
    if gi < r0 || gi > r1 || row.decor {
        return;
    }
    let from = if gi == r0 { c0 } else { 0 }.max(row.skip);
    let lw = text::line_width(&row.line) as u16;
    let to = if gi == r1 {
        c1.min(lw.saturating_sub(1))
    } else {
        lw.saturating_sub(1)
    };
    let to = to.min(cw as u16 - 1);
    if from > to {
        return;
    }
    for x in from..=to {
        if let Some(cell) = buf.cell_mut((chat.content_x + x, y)) {
            cell.set_bg(theme::SELECTION);
        }
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
    if view.chat.follow && !view.is_window_unlimited() && view.slots.len() > view.tail_keep() {
        let keep = view.tail_keep();
        view.trim_to_tail(keep);
    }
    let reflow = view.chat.last_w != cw || !view.chat.compact;
    let restore = reflow || view.needs_anchor_restore;
    let anchor = if restore && !view.chat.follow && view.chat.initialized {
        capture_anchor(&view.chat)
    } else {
        None
    };
    view.needs_anchor_restore = false;
    layout(view, env, cw, true);
    view.chat.last_w = cw;
    let chat = &mut view.chat;
    if let Some(a) = &anchor {
        restore_anchor(chat, a);
    }
    chat.content_x = area.x;
    settle(chat, area, reflow);
    chat.visible_seq_range = compute_visible_seq_range(chat);
    chat.pill = None;
    let top = chat.top();
    for dy in 0..area.height {
        let gi = top + dy as usize;
        let Some(row) = chat.row(gi) else {
            break;
        };
        buf.set_line(area.x, area.y + dy, &row.line, area.width);
        if let Some(sel) = chat.sel.as_ref().filter(|s| s.active) {
            paint_selection(buf, chat, sel, row, gi, area.y + dy, cw);
        }
    }
    if chat.follow || chat.total <= area.height as usize {
        return;
    }
    // Scrolled up: thumb on the right border; "↓ N new lines" once more content arrives.
    draw_scrollbar(
        buf,
        Rect::new(bar_x, area.y, 1, area.height),
        chat.total,
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
    chat.pill.is_some_and(|r| r.contains(Position::new(x, y)))
}

#[cfg(test)]
mod scroll_tests {
    use super::*;

    const HEIGHT: u16 = 10;

    fn following(total: usize) -> ChatState {
        let mut chat = ChatState::default();
        set_total(&mut chat, total);
        set_total(&mut chat, total);
        assert!(chat.follow);
        chat
    }

    fn set_total(chat: &mut ChatState, total: usize) {
        chat.total = total;
        settle(chat, Rect::new(0, 0, 40, HEIGHT), false);
        while chat.step_anim() {}
    }

    #[test]
    fn shrinking_content_snaps_immediately_without_blank_hold_space() {
        let mut chat = following(50);
        set_total(&mut chat, 46);
        assert_eq!(chat.top(), 36);
        assert_eq!(chat.max_scroll(), 36.0);
    }

    #[test]
    fn streaming_growth_while_stationary_at_bottom_sticks_without_lag() {
        let mut chat = following(50);
        chat.total = 55;
        settle(&mut chat, Rect::new(0, 0, 40, HEIGHT), false);
        assert_eq!(chat.top(), 45);
        assert!(!chat.scrolling());
    }

    #[test]
    fn shrinking_past_scrolled_up_target_restores_follow() {
        let mut chat = following(50);
        chat.scroll_by(-4, false);
        assert!(!chat.follow);
        assert_eq!(chat.top(), 36);
        // Collapsing a card shrinks total from 50 -> 42 (max_scroll = 32 <= 36).
        set_total(&mut chat, 42);
        assert!(chat.follow, "clamping target to max restores follow");
        assert_eq!(chat.top(), 32);
    }

    #[test]
    fn viewport_height_shrink_while_following_snaps_immediately() {
        let mut chat = following(50);
        assert_eq!(chat.top(), 40);
        // Focusing a split tile shrinks viewport height by 2 rows.
        settle(&mut chat, Rect::new(0, 0, 40, HEIGHT - 2), false);
        assert_eq!(chat.top(), 42);
        assert!(!chat.scrolling());
    }
}
