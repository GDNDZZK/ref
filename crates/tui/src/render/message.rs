//! Transcript renderer: turns a [`SessionState`] into a document of cached line
//! blocks, with click targets (toggle / copy) and per-part focus targets.
//!
//! Static content (markdown text, expanded tool bodies, reasoning bodies) is cached
//! per part keyed by `(part revision, width)`, so streaming only re-renders the
//! part that changed. One-line headers (spinners, timers) are rebuilt per frame.

use crate::catalog::{title_case, Catalog};
use crate::render::markdown;
use crate::render::util::{fit_spans, left_right, spans_width, with_bg};
use crate::store::{MsgEntry, SessionState};
use crate::text::{self, fmt_countdown, fmt_dur, fmt_time};
use crate::theme;
use hn_proto::{Origin, Part, Role, ToolPart, ToolState};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// A focusable/toggleable message part.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Target {
    pub msg: String,
    pub idx: usize,
}

/// Something clickable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hit {
    Toggle(Target),
    CopyMessage(String),
    CopyToolInput(Target),
    CopyToolOutput(Target),
}

pub struct Item {
    pub lines: Rc<Vec<Line<'static>>>,
    pub target: Option<Target>,
    /// Columns of decoration on the left (bars/padding), excluded from copied selections.
    pub deco: u16,
    /// (line index within item, x0, x1, hit)
    pub hits: Vec<(usize, u16, u16, Hit)>,
}

impl Item {
    fn one(line: Line<'static>, deco: u16) -> Item {
        Item { lines: Rc::new(vec![line]), target: None, deco, hits: Vec::new() }
    }
    fn blank() -> Item {
        Item::one(Line::default(), 0)
    }
}

#[derive(Default)]
pub struct Doc {
    pub items: Vec<Item>,
    /// Start row of each item.
    pub starts: Vec<usize>,
    pub height: usize,
    /// Start row of each message (by id) — used for "jump" helpers.
    pub msg_rows: Vec<(String, usize)>,
}

impl Doc {
    fn push(&mut self, it: Item) {
        self.starts.push(self.height);
        self.height += it.lines.len();
        self.items.push(it);
    }

    /// Item index + line index for a document row.
    pub fn locate(&self, row: usize) -> Option<(usize, usize)> {
        if row >= self.height {
            return None;
        }
        let i = match self.starts.binary_search(&row) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        // Skip empty items that share the same start.
        let mut i = i;
        while self.items[i].lines.is_empty() && i + 1 < self.items.len() {
            i += 1;
        }
        Some((i, row - self.starts[i]))
    }

    pub fn line(&self, row: usize) -> Option<(&Line<'static>, &Item)> {
        let (i, l) = self.locate(row)?;
        let it = &self.items[i];
        it.lines.get(l).map(|line| (line, it))
    }

    pub fn hit_at(&self, row: usize, col: u16) -> Option<&Hit> {
        let (i, l) = self.locate(row)?;
        self.items[i].hits.iter().find(|(hl, x0, x1, _)| *hl == l && col >= *x0 && col < *x1).map(|h| &h.3)
    }


    /// Ordered list of focusable targets with their first row.
    pub fn targets(&self) -> Vec<(Target, usize)> {
        let mut out: Vec<(Target, usize)> = Vec::new();
        for (i, it) in self.items.iter().enumerate() {
            if let Some(t) = &it.target {
                if out.last().map(|(lt, _)| lt != t).unwrap_or(true) && !out.iter().any(|(x, _)| x == t) {
                    out.push((t.clone(), self.starts[i]));
                }
            }
        }
        out
    }

    /// Row range of a target.
    pub fn target_rows(&self, t: &Target) -> Option<(usize, usize)> {
        let mut first = None;
        let mut last = 0;
        for (i, it) in self.items.iter().enumerate() {
            if it.target.as_ref() == Some(t) {
                if first.is_none() {
                    first = Some(self.starts[i]);
                }
                last = self.starts[i] + it.lines.len();
            }
        }
        first.map(|f| (f, last))
    }
}

/// Text selection in document coordinates (row, col).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: (usize, u16),
    pub head: (usize, u16),
}

impl Selection {
    pub fn ordered(&self) -> ((usize, u16), (usize, u16)) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }
    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }
    fn contains(&self, row: usize, col: u16) -> bool {
        let (a, b) = self.ordered();
        (row, col) >= a && (row, col) < b
    }
}

struct CacheEntry {
    rev: u64,
    width: usize,
    lines: Rc<Vec<Line<'static>>>,
}

/// Per-session view state: expansion toggles, scroll, focus, selection, caches.
pub struct ViewState {
    /// Parts whose expansion differs from the default.
    toggled: HashSet<Target>,
    pub scroll: usize,
    pub stick_bottom: bool,
    pub focus: Option<Target>,
    pub selection: Option<Selection>,
    pub selecting: bool,
    cache: HashMap<(Target, u8), CacheEntry>,
    cache_width: usize,
    /// Height of the doc at the last render, to detect new content while scrolled up.
    pub last_height: usize,
    pub unseen: bool,
    pub viewport: usize,
    /// After a toggle: keep this target's header at this viewport row on the next frame.
    pub anchor: Option<(Target, usize)>,
}

impl Default for ViewState {
    fn default() -> Self {
        ViewState {
            toggled: HashSet::new(),
            scroll: 0,
            stick_bottom: true,
            focus: None,
            selection: None,
            selecting: false,
            cache: HashMap::new(),
            cache_width: 0,
            last_height: 0,
            unseen: false,
            viewport: 0,
            anchor: None,
        }
    }
}

/// Global display options.
#[derive(Clone, Copy)]
pub struct Opts {
    pub now: i64,
    pub show_thinking: bool,
    /// Expand all tool calls by default.
    pub details: bool,
    pub compact: bool,
}

const SUB_TEXT: u8 = 0;
const SUB_REASON: u8 = 1;
const SUB_TOOL_IN: u8 = 2;
const SUB_TOOL_OUT: u8 = 3;
const SUB_USER: u8 = 4;

impl ViewState {
    pub fn toggle(&mut self, t: &Target) {
        if !self.toggled.remove(t) {
            self.toggled.insert(t.clone());
        }
    }

    /// Toggle and keep the target's header at `row` (viewport-relative) so the block
    /// expands/collapses under the cursor instead of jumping.
    pub fn toggle_anchored(&mut self, t: &Target, row: usize) {
        self.toggle(t);
        self.anchor = Some((t.clone(), row));
    }

    /// Apply a pending anchor against the freshly built doc.
    pub fn apply_anchor(&mut self, doc: &Doc, viewport: usize) {
        let Some((t, row)) = self.anchor.take() else { return };
        let Some((a, _)) = doc.target_rows(&t) else { return };
        let max = doc.height.saturating_sub(viewport);
        let s = a.saturating_sub(row).min(max);
        self.scroll = s;
        self.stick_bottom = s >= max;
        self.last_height = doc.height;
    }

    /// Reset per-part toggles (used when switching the global "details" mode).
    pub fn reset_toggles(&mut self) {
        self.toggled.clear();
    }

    fn is_expanded(&self, t: &Target, default: bool) -> bool {
        default ^ self.toggled.contains(t)
    }

    fn cached(
        &mut self,
        key: (Target, u8),
        rev: u64,
        width: usize,
        f: impl FnOnce() -> Vec<Line<'static>>,
    ) -> Rc<Vec<Line<'static>>> {
        if let Some(e) = self.cache.get(&key) {
            if e.rev == rev && e.width == width {
                return e.lines.clone();
            }
        }
        let lines = Rc::new(f());
        self.cache.insert(key, CacheEntry { rev, width, lines: lines.clone() });
        lines
    }

    /// Clamp scroll for a doc height and viewport.
    pub fn clamp(&mut self, height: usize, viewport: usize) {
        self.viewport = viewport;
        let max = height.saturating_sub(viewport);
        if self.stick_bottom {
            self.scroll = max;
            self.unseen = false;
        } else {
            if height > self.last_height && self.last_height != 0 {
                self.unseen = true;
            }
            self.scroll = self.scroll.min(max);
            if self.scroll >= max {
                self.stick_bottom = true;
                self.unseen = false;
            }
        }
        self.last_height = height;
    }

    pub fn scroll_by(&mut self, delta: i64) {
        let max = self.last_height.saturating_sub(self.viewport);
        let s = (self.scroll as i64 + delta).clamp(0, max as i64) as usize;
        self.scroll = s;
        self.stick_bottom = s >= max;
        if self.stick_bottom {
            self.unseen = false;
        }
    }

    pub fn scroll_to_bottom(&mut self) {
        self.stick_bottom = true;
        self.unseen = false;
    }

    pub fn scroll_to_top(&mut self) {
        self.stick_bottom = false;
        self.scroll = 0;
    }

    /// Ensure rows [a, b) are visible.
    pub fn reveal(&mut self, a: usize, b: usize) {
        let vp = self.viewport.max(1);
        if a < self.scroll {
            self.scroll = a;
            self.stick_bottom = false;
        } else if b > self.scroll + vp {
            self.scroll = b.saturating_sub(vp).min(a);
            let max = self.last_height.saturating_sub(vp);
            self.stick_bottom = self.scroll >= max;
        }
    }
}

// ---------------------------------------------------------------------------
// Building the document
// ---------------------------------------------------------------------------

struct B<'a> {
    doc: Doc,
    view: &'a mut ViewState,
    cat: &'a Catalog,
    o: Opts,
    width: usize,
    pad: usize,
    /// The last pushed block was a collapsed one-liner (tool / thinking / header):
    /// further one-liners stack without a blank line, even across LLM steps.
    last_compact: bool,
    /// Just pushed a turn header: the next block follows without a blank line.
    after_header: bool,
}

/// Build the transcript document for `width` columns.
pub fn build(s: &SessionState, view: &mut ViewState, cat: &Catalog, o: Opts, width: usize) -> Doc {
    let width = width.max(16);
    if view.cache_width != width {
        view.cache.clear();
        view.cache_width = width;
    }
    // Prune cache entries for removed messages occasionally.
    if view.cache.len() > 64 && view.cache.len() > s.msgs.len() * 6 {
        let ids: HashSet<&str> = s.msgs.iter().map(|m| m.msg.id.as_str()).collect();
        view.cache.retain(|k, _| ids.contains(k.0.msg.as_str()));
    }
    let pad = if o.compact { 1 } else { 3 };
    let mut b = B { doc: Doc::default(), view, cat, o, width, pad, last_compact: false, after_header: false };
    if !o.compact {
        b.doc.push(Item::blank());
    }
    let n = s.msgs.len();
    // (agent, model) of the previous assistant step in the current turn; None after a user message.
    let mut prev_step: Option<(Option<String>, Option<hn_proto::ModelRef>)> = None;
    for (i, e) in s.msgs.iter().enumerate() {
        let first = i == 0;
        match e.msg.role {
            Role::User => {
                if !first {
                    b.doc.push(Item::blank());
                }
                b.doc.msg_rows.push((e.msg.id.clone(), b.doc.height));
                b.user(e);
                b.last_compact = false;
                prev_step = None;
            }
            Role::Assistant => {
                let me = (e.msg.agent.clone(), e.msg.model.clone());
                let header = prev_step.as_ref() != Some(&me);
                prev_step = Some(me);
                if header && !first {
                    b.doc.push(Item::blank());
                }
                b.doc.msg_rows.push((e.msg.id.clone(), b.doc.height));
                b.assistant(e, i + 1 == n, &s.info.status, header);
            }
        }
    }
    b.doc.push(Item::blank());
    b.doc
}

impl B<'_> {
    fn muted(&self) -> Style {
        theme::muted()
    }

    fn user(&mut self, e: &MsgEntry) {
        let m = &e.msg;
        let w = self.width;
        let agent_color = m.agent.as_deref().map(|a| self.cat.agent_color(a)).unwrap_or(theme::SECONDARY);
        let (bar_color, label): (_, Option<(String, ratatui::style::Color)>) = match &m.origin {
            Origin::Human => (agent_color, None),
            Origin::Peer { from_name, .. } => (theme::ACCENT, Some((format!("↩ from {from_name}"), theme::ACCENT))),
            Origin::Spawn { from_name, .. } => (theme::INFO, Some((format!("⤷ spawned by {from_name}"), theme::INFO))),
            Origin::System => (theme::BORDER, Some(("⚙ system".to_string(), theme::MUTED))),
            Origin::Compaction => return self.compaction(e),
        };
        let inner_pad = if self.o.compact { 1 } else { 2 };
        let bar = Span::styled("┃", Style::new().fg(bar_color).bg(theme::PANEL));
        let deco = (1 + inner_pad) as u16;
        let tw = w.saturating_sub(1 + inner_pad + 1).max(4);
        let rev = e.revs.iter().copied().max().unwrap_or(0).max(e.rev);
        let key = (Target { msg: m.id.clone(), idx: usize::MAX }, SUB_USER);
        let label2 = label.clone();
        let body = self.view.cached(key, rev, w, || {
            let mut lines = Vec::new();
            let pad_s = " ".repeat(inner_pad);
            let mut push = |spans: Vec<Span<'static>>| {
                let mut v = vec![bar.clone(), Span::styled(pad_s.clone(), Style::new().bg(theme::PANEL))];
                v.extend(spans);
                lines.push(with_bg(Line::from(v), w, theme::PANEL));
            };
            if let Some((l, c)) = &label2 {
                push(vec![Span::styled(l.clone(), theme::fg(*c))]);
            }
            for p in &m.parts {
                match p {
                    Part::Text { text: t, .. } => {
                        let st = if matches!(m.origin, Origin::System) { theme::muted() } else { theme::text() };
                        for logical in text::sanitize(t).split('\n') {
                            for wl in text::wrap_segments(&[(logical.to_string(), st)], tw) {
                                push(wl.spans);
                            }
                        }
                    }
                    Part::Image(img) => push(vec![image_chip(img)]),
                    _ => {}
                }
            }
            lines
        });
        // Meta line (time + copy button), dynamic-free but cheap.
        let mut left = vec![
            Span::styled("┃", Style::new().fg(bar_color)),
            Span::raw(" ".repeat(inner_pad)),
            Span::styled(fmt_time(m.time), self.muted()),
        ];
        if m.agent.is_some() && !self.o.compact {
            left.push(Span::styled(" · ", theme::faint()));
            left.push(Span::styled(title_case(m.agent.as_deref().unwrap_or("")), theme::fg(agent_color)));
        }
        let has_text = m.parts.iter().any(|p| matches!(p, Part::Text { text, .. } if !text.trim().is_empty()));
        let copy = if has_text { copy_button() } else { Vec::new() };
        let cw = spans_width(&copy);
        let (line, rx) = left_right(left, [copy, vec![Span::raw(" ")]].concat(), w, Style::new());
        let meta = with_bg(line, w, theme::PANEL);
        self.doc.push(Item { lines: body, target: None, deco, hits: Vec::new() });
        let mut mi = Item::one(meta, deco);
        if has_text {
            mi.hits.push((0, rx as u16, (rx + cw) as u16, Hit::CopyMessage(m.id.clone())));
        }
        self.doc.push(mi);
    }

    fn compaction(&mut self, e: &MsgEntry) {
        let m = &e.msg;
        let t = Target { msg: m.id.clone(), idx: 0 };
        let expanded = self.view.is_expanded(&t, false);
        let w = self.width;
        let chev = if expanded { "▾ " } else { "▸ " };
        let left = vec![
            Span::raw(" ".repeat(self.pad.saturating_sub(2))),
            Span::styled(chev, theme::faint()),
            Span::styled("◆ Context compacted", theme::fg(theme::INFO)),
            Span::styled("  summary of earlier conversation", theme::faint()),
        ];
        let right = vec![Span::styled(fmt_time(m.time), self.muted()), Span::raw(" ")];
        let (line, _) = left_right(left, right, w, Style::new());
        let mut it = Item::one(line, self.pad as u16);
        it.target = Some(t.clone());
        it.hits.push((0, 0, w as u16, Hit::Toggle(t.clone())));
        self.doc.push(it);
        if expanded {
            let rev = e.revs.first().copied().unwrap_or(0);
            let pad = self.pad + 2;
            let tw = w.saturating_sub(pad + 1);
            let lines = self.view.cached((t.clone(), SUB_TEXT), rev, w, || {
                indent(markdown::render(&m.raw_text(), tw, theme::muted()), pad, None)
            });
            self.doc.push(Item { lines, target: Some(t), deco: pad as u16, hits: Vec::new() });
        }
    }

    /// Blank line before a block unless one-liners stack (or it directly follows a header).
    fn gap(&mut self, compact_block: bool) {
        if self.after_header {
            self.after_header = false;
            return;
        }
        if !(compact_block && self.last_compact) {
            self.doc.push(Item::blank());
        }
    }

    fn assistant(&mut self, e: &MsgEntry, last: bool, status: &hn_proto::SessionStatus, header: bool) {
        let m = &e.msg;
        let w = self.width;
        let pad = self.pad;
        let live = last && m.completed.is_none() && status.is_active();
        if header {
            // Turn header (start of a turn, or when agent/model changes): ■ Build · Model · variant
            let agent = m.agent.clone().unwrap_or_default();
            let ac = self.cat.agent_color(&agent);
            let mut left = vec![Span::raw(" ".repeat(pad))];
            if !agent.is_empty() {
                left.push(Span::styled("■ ", theme::fg(ac)));
                left.push(Span::styled(title_case(&agent), theme::fg(ac)));
            }
            if let Some(model) = &m.model {
                if !self.o.compact {
                    if !agent.is_empty() {
                        left.push(Span::styled(" · ", theme::faint()));
                    }
                    left.push(Span::styled(self.cat.model_name(model), self.muted()));
                    if let Some(v) = &model.variant {
                        left.push(Span::styled(format!(" {v}"), theme::fg(theme::WARNING)));
                    }
                }
            }
            if spans_width(&left) > pad {
                self.doc.push(Item::one(Line::from(fit_spans(left, w)), pad as u16));
                self.after_header = true;
                self.last_compact = true;
            }
        }
        let last_text = m.parts.iter().rposition(|p| matches!(p, Part::Text { text, .. } if !text.trim().is_empty()));
        let mut meta_done = false;
        for (idx, p) in m.parts.iter().enumerate() {
            let t = Target { msg: m.id.clone(), idx };
            let rev = e.revs.get(idx).copied().unwrap_or(0);
            match p {
                Part::Text { text: txt, .. } => {
                    if txt.trim().is_empty() {
                        continue;
                    }
                    self.gap(false);
                    let tw = w.saturating_sub(pad + 1);
                    let lines = self.view.cached((t.clone(), SUB_TEXT), rev, w, || {
                        indent(markdown::render(txt, tw, theme::text()), pad, None)
                    });
                    self.doc.push(Item { lines, target: None, deco: pad as u16, hits: Vec::new() });
                    self.last_compact = false;
                    if Some(idx) == last_text {
                        let at = match p {
                            Part::Text { time, .. } if *time > 0 => *time,
                            _ => m.time,
                        };
                        self.step_meta(e, at, live, true);
                        meta_done = true;
                    }
                }
                Part::Reasoning { text: rt, time_start, time_end, .. } => {
                    if !self.o.show_thinking || rt.trim().is_empty() && time_end.is_some() {
                        continue;
                    }
                    let expanded = self.view.is_expanded(&t, false);
                    self.gap(!expanded);
                    self.reasoning(&t, rev, rt, *time_start, *time_end, live, expanded);
                    self.last_compact = !expanded;
                }
                Part::Tool(tp) => {
                    let expanded = self.view.is_expanded(&t, self.o.details);
                    self.gap(!expanded);
                    self.tool(&t, rev, tp, expanded);
                    self.last_compact = !expanded;
                }
                Part::Image(img) => {
                    self.gap(true);
                    let line = Line::from(vec![Span::raw(" ".repeat(pad)), image_chip(img)]);
                    self.doc.push(Item::one(line, pad as u16));
                    self.last_compact = true;
                }
            }
        }
        let visible = m.parts.iter().any(|p| match p {
            Part::Text { text, .. } => !text.trim().is_empty(),
            Part::Reasoning { text, time_end, .. } => self.o.show_thinking && (!text.trim().is_empty() || time_end.is_none()),
            _ => true,
        });
        if live && !visible && m.error.is_none() {
            // Request sent, nothing streamed yet (or retrying): keep the step visibly alive.
            self.gap(true);
            let (label, color) = match status {
                hn_proto::SessionStatus::Retry { .. } => ("Waiting to retry…", theme::WARNING),
                _ => ("Waiting for the model…", theme::MUTED),
            };
            let line = Line::from(vec![
                Span::raw(" ".repeat(pad)),
                Span::styled(format!("{} ", theme::spinner(self.o.now)), theme::fg(theme::PRIMARY)),
                Span::styled(label, theme::fg(color)),
                Span::styled(format!("  {}", fmt_dur(self.o.now - m.time)), theme::faint()),
            ]);
            self.doc.push(Item::one(line, pad as u16));
            self.last_compact = true;
        }
        if !meta_done && matches!(m.finish.as_deref(), Some("aborted") | Some("length")) {
            self.step_meta(e, m.time, live, false);
        }
        if let Some(err) = &m.error {
            self.after_header = false;
            self.doc.push(Item::blank());
            let tw = w.saturating_sub(pad + 3);
            let mut lines = Vec::new();
            let st = theme::fg(theme::ERROR);
            for logical in text::sanitize(err).split('\n') {
                for wl in text::wrap_segments(&[(logical.to_string(), st)], tw) {
                    let mut v = vec![Span::raw(" ".repeat(pad)), Span::styled("┃ ", st)];
                    v.extend(wl.spans);
                    lines.push(Line::from(v));
                }
            }
            self.doc.push(Item { lines: Rc::new(lines), target: None, deco: (pad + 2) as u16, hits: Vec::new() });
            self.last_compact = false;
        }
    }

    /// Subtle per-step meta line under the text: `11:22:30 · 9.1s · interrupted        ⧉ copy`.
    /// `at` = when the text was produced; the duration is the whole step's.
    fn step_meta(&mut self, e: &MsgEntry, at: i64, live: bool, copy: bool) {
        let m = &e.msg;
        let w = self.width;
        let pad = self.pad;
        let mut left = vec![Span::raw(" ".repeat(pad)), Span::styled(fmt_time(at), theme::faint())];
        if let Some(done) = m.completed {
            left.push(Span::styled(format!(" · {}", fmt_dur(done - m.time)), theme::faint()));
        } else if live {
            left.push(Span::styled(format!(" · {}", fmt_dur(self.o.now - m.time)), theme::faint()));
        }
        match m.finish.as_deref() {
            Some("aborted") => left.push(Span::styled(" · interrupted", theme::fg(theme::WARNING))),
            Some("length") => left.push(Span::styled(" · truncated (max tokens)", theme::fg(theme::WARNING))),
            _ => {}
        }
        let mut right = Vec::new();
        if copy {
            right = copy_button();
            right.push(Span::raw(" "));
        }
        let cw = spans_width(&right).saturating_sub(1);
        let (line, rx) = left_right(left, right, w, Style::new());
        let mut it = Item::one(line, pad as u16);
        if copy {
            it.hits.push((0, rx as u16, (rx + cw) as u16, Hit::CopyMessage(m.id.clone())));
        }
        self.doc.push(it);
        self.last_compact = false;
        self.after_header = false;
    }

    #[allow(clippy::too_many_arguments)]
    fn reasoning(
        &mut self,
        t: &Target,
        rev: u64,
        rt: &str,
        start: i64,
        end: Option<i64>,
        live: bool,
        expanded: bool,
    ) {
        let w = self.width;
        let pad = self.pad;
        let chev = if expanded { "▾ " } else { "▸ " };
        let mut left = vec![Span::raw(" ".repeat(pad.saturating_sub(2))), Span::styled(chev, theme::faint())];
        let streaming = end.is_none() && live;
        let dur = match end {
            Some(e) => Some(e - start),
            None if streaming => Some(self.o.now - start),
            None => None,
        };
        if streaming {
            left.push(Span::styled(format!("{} ", theme::spinner(self.o.now)), theme::fg(theme::WARNING)));
            left.push(Span::styled("Thinking", theme::fg(theme::WARNING)));
        } else {
            left.push(Span::styled("Thought", theme::fg(theme::blend(theme::WARNING, theme::MUTED, 0.5))));
        }
        // Right column matches tool lines: `HH:MM:SS · 1.2s`.
        let mut right = Vec::new();
        if start > 0 && !self.o.compact {
            right.push(Span::styled(fmt_time(start), theme::faint()));
        }
        if let Some(d) = dur {
            let sep = if right.is_empty() { "" } else { " · " };
            right.push(Span::styled(format!("{sep}{}", fmt_dur(d)), theme::faint()));
        }
        if !right.is_empty() {
            right.push(Span::raw(" "));
        }
        let rw = spans_width(&right);
        if !expanded {
            let used = spans_width(&left) + 2;
            let room = w.saturating_sub(used + 1 + if rw > 0 { rw + 3 } else { 0 });
            if room > 4 {
                let tail = text::tail_fit(rt, room);
                left.push(Span::raw("  "));
                left.push(Span::styled(tail, theme::faint().add_modifier(Modifier::ITALIC)));
            }
        }
        let (line, _) = left_right(left, right, w, Style::new());
        let mut it = Item::one(line, pad as u16);
        it.target = Some(t.clone());
        it.hits.push((0, 0, w as u16, Hit::Toggle(t.clone())));
        self.doc.push(it);
        if expanded {
            let tw = w.saturating_sub(pad + 3);
            let lines = self.view.cached((t.clone(), SUB_REASON), rev, w, || {
                let body = markdown::render(rt, tw, theme::muted().add_modifier(Modifier::ITALIC));
                indent(body, pad, Some(Span::styled("│ ", theme::fg(theme::BORDER_SUBTLE))))
            });
            self.doc.push(Item { lines, target: Some(t.clone()), deco: (pad + 2) as u16, hits: Vec::new() });
        }
    }

    fn tool(&mut self, t: &Target, rev: u64, tp: &ToolPart, expanded: bool) {
        let w = self.width;
        let pad = self.pad;
        let now = self.o.now;
        let focused = self.view.focus.as_ref() == Some(t);
        let chev = if expanded { "▾ " } else { "▸ " };
        let (icon, icon_style) = tool_icon(tp.state, now);
        let name_style = match tp.state {
            ToolState::Error => theme::fg(theme::ERROR),
            ToolState::Denied => theme::muted().add_modifier(Modifier::CROSSED_OUT),
            ToolState::Cancelled => theme::muted(),
            ToolState::Permission | ToolState::Delayed => theme::fg(theme::WARNING),
            _ => theme::text(),
        };
        let mut left = vec![
            Span::raw(" ".repeat(pad.saturating_sub(2))),
            Span::styled(chev, if focused { theme::fg(theme::PRIMARY) } else { theme::faint() }),
            Span::styled(format!("{icon} "), icon_style),
            Span::styled(tp.name.clone(), name_style.add_modifier(Modifier::BOLD)),
        ];
        let purpose = tool_purpose(tp);
        if !purpose.is_empty() {
            left.push(Span::raw("  "));
            left.push(Span::styled(text::one_line(&purpose), self.muted()));
        }
        if !tp.purpose.is_empty() && !tp.title.is_empty() && !self.o.compact {
            left.push(Span::styled(format!(" · {}", text::one_line(&tp.title)), theme::faint()));
        }
        let mut right: Vec<Span<'static>> = Vec::new();
        match tp.state {
            ToolState::Delayed => {
                let left_ms = tp.run_at.map(|r| r - now).unwrap_or(0);
                right.push(Span::styled(format!("runs in {}", fmt_countdown(left_ms)), theme::fg(theme::WARNING)));
            }
            ToolState::Permission => {
                right.push(Span::styled("awaiting permission", theme::fg(theme::WARNING)));
            }
            ToolState::Denied => right.push(Span::styled("denied", theme::fg(theme::ERROR))),
            ToolState::Cancelled => right.push(Span::styled("cancelled", self.muted())),
            _ => {}
        }
        if !matches!(tp.state, ToolState::Delayed | ToolState::Permission) {
            if !right.is_empty() {
                right.push(Span::styled(" · ", theme::faint()));
            }
            if tp.time_start > 0 {
                right.push(Span::styled(fmt_time(tp.time_start), theme::faint()));
            }
            let dur = match tp.time_end {
                Some(e) => Some(e - tp.time_start),
                None if !tp.state.is_final() && tp.time_start > 0 => Some(now - tp.time_start),
                None => None,
            };
            if let Some(d) = dur {
                right.push(Span::styled(format!(" · {}", fmt_dur(d)), theme::faint()));
            }
        }
        right.push(Span::raw(" "));
        let (line, _) = left_right(left, right, w, Style::new());
        let mut it = Item::one(line, pad as u16);
        it.target = Some(t.clone());
        it.hits.push((0, 0, w as u16, Hit::Toggle(t.clone())));
        self.doc.push(it);
        if !expanded {
            return;
        }
        // Expanded body: input + output sections with copy buttons.
        let ind = pad + 2;
        let bw = w.saturating_sub(ind + 1).max(8);
        let sect = |label: &str| -> (Line<'static>, usize) {
            let l = vec![
                Span::raw(" ".repeat(ind)),
                Span::styled(label.to_string(), theme::fg(theme::MUTED).add_modifier(Modifier::BOLD)),
            ];
            let mut r = copy_button();
            r.push(Span::raw(" "));
            let (line, rx) = left_right(l, r, w, Style::new());
            (line, rx)
        };
        let cbw = spans_width(&copy_button()) as u16;
        let (l, rx) = sect(if tp.state == ToolState::Streaming { "input (streaming)" } else { "input" });
        let mut hi = Item::one(l, ind as u16);
        hi.target = Some(t.clone());
        hi.hits.push((0, rx as u16, rx as u16 + cbw, Hit::CopyToolInput(t.clone())));
        self.doc.push(hi);
        let lines =
            self.view.cached((t.clone(), SUB_TOOL_IN), rev, w, || code_box(&tool_input_text(tp), ind, bw, "json"));
        self.doc.push(Item { lines, target: Some(t.clone()), deco: (ind + 1) as u16, hits: Vec::new() });

        let has_output = !tp.output.is_empty() || !tp.images.is_empty();
        if has_output || tp.state.is_final() || tp.state == ToolState::Running {
            let label = match tp.state {
                ToolState::Running => "output (live)",
                ToolState::Error => "error",
                _ => "output",
            };
            let (l, rx) = sect(label);
            let mut hi = Item::one(l, ind as u16);
            hi.target = Some(t.clone());
            hi.hits.push((0, rx as u16, rx as u16 + cbw, Hit::CopyToolOutput(t.clone())));
            self.doc.push(hi);
            let lang = if tp.state == ToolState::Error { "error" } else { "" };
            let lines = self.view.cached((t.clone(), SUB_TOOL_OUT), rev, w, || {
                let mut out = tp.output.clone();
                for img in &tp.images {
                    out.push_str(&format!("\n[image {} · {}]", img.mime, human_bytes(img.data.len() * 3 / 4)));
                }
                if out.is_empty() {
                    out = if tp.state == ToolState::Running { "…".into() } else { "(no output)".into() };
                }
                code_box(&out, ind, bw, lang)
            });
            self.doc.push(Item { lines, target: Some(t.clone()), deco: (ind + 1) as u16, hits: Vec::new() });
        }
    }
}

fn copy_button() -> Vec<Span<'static>> {
    vec![Span::styled("⧉ copy", theme::faint())]
}

fn image_chip(img: &hn_proto::ImageData) -> Span<'static> {
    Span::styled(
        format!(" ▣ image {} · {} ", img.mime, human_bytes(img.data.len() * 3 / 4)),
        Style::new().fg(theme::BG).bg(theme::ACCENT),
    )
}

pub fn human_bytes(n: usize) -> String {
    if n < 1024 {
        format!("{n}B")
    } else if n < 1024 * 1024 {
        format!("{:.1}KB", n as f64 / 1024.0)
    } else {
        format!("{:.1}MB", n as f64 / 1048576.0)
    }
}

fn indent(lines: Vec<Line<'static>>, pad: usize, bar: Option<Span<'static>>) -> Vec<Line<'static>> {
    let p = " ".repeat(pad);
    lines
        .into_iter()
        .map(|l| {
            let mut v = vec![Span::raw(p.clone())];
            if let Some(b) = &bar {
                v.push(b.clone());
            }
            v.extend(l.spans);
            Line::from(v)
        })
        .collect()
}

/// A raw text block in a panel with a left bar. Hard-wrapped, highlighted for json.
fn code_box(src: &str, ind: usize, width: usize, lang: &str) -> Vec<Line<'static>> {
    let bg = theme::PANEL;
    let bar_color = if lang == "error" { theme::ERROR } else { theme::BORDER };
    let base = Style::new().fg(if lang == "error" { theme::ERROR } else { theme::TEXT }).bg(bg);
    let mut hl = crate::render::highlight::Highlighter::new(lang);
    let inner = width.saturating_sub(2).max(4);
    let mut out = Vec::new();
    let src = text::sanitize(src);
    let src = src.strip_suffix('\n').unwrap_or(&src);
    for raw in src.split('\n') {
        let segs = hl.line(raw, base);
        let mut cur: Vec<Span<'static>> = Vec::new();
        let mut w = 0usize;
        let mut rows: Vec<(Vec<Span<'static>>, usize)> = Vec::new();
        for (s, st) in segs {
            for g in unicode_segmentation::UnicodeSegmentation::graphemes(s.as_str(), true) {
                let gw = text::gwidth(g);
                if w + gw > inner && w > 0 {
                    rows.push((std::mem::take(&mut cur), w));
                    w = 0;
                }
                match cur.last_mut() {
                    Some(last) if last.style == st => last.content.to_mut().push_str(g),
                    _ => cur.push(Span::styled(g.to_string(), st)),
                }
                w += gw;
            }
        }
        rows.push((cur, w));
        for (spans, w) in rows {
            let mut v = vec![
                Span::raw(" ".repeat(ind)),
                Span::styled("▌", Style::new().fg(bar_color).bg(bg)),
                Span::styled(" ", Style::new().bg(bg)),
            ];
            v.extend(spans);
            if w < inner {
                v.push(Span::styled(" ".repeat(inner - w), Style::new().bg(bg)));
            }
            out.push(Line::from(v));
        }
    }
    out
}

pub fn tool_icon(state: ToolState, now: i64) -> (&'static str, Style) {
    match state {
        ToolState::Streaming | ToolState::Running => (theme::spinner(now), theme::fg(theme::PRIMARY)),
        ToolState::Permission => ("△", theme::fg(theme::WARNING)),
        ToolState::Delayed => ("◷", theme::fg(theme::WARNING)),
        ToolState::Done => ("✓", theme::fg(theme::SUCCESS)),
        ToolState::Error => ("✗", theme::fg(theme::ERROR)),
        ToolState::Denied => ("⊘", theme::fg(theme::ERROR)),
        ToolState::Cancelled => ("■", theme::muted()),
    }
}

/// Purpose for display: ToolPart.purpose, else title, else scraped from streaming JSON.
pub fn tool_purpose(tp: &ToolPart) -> String {
    if !tp.purpose.is_empty() {
        return tp.purpose.clone();
    }
    if !tp.title.is_empty() {
        return tp.title.clone();
    }
    if let Some(p) = tp.input.get("purpose").and_then(|v| v.as_str()) {
        return p.to_string();
    }
    scrape_json_string(&tp.raw_input, "purpose").unwrap_or_default()
}

/// Best-effort extraction of a string field from (possibly incomplete) JSON text.
pub fn scrape_json_string(raw: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\"");
    let i = raw.find(&pat)? + pat.len();
    let rest = raw[i..].trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => break,
            '\\' => match chars.next() {
                Some('n') => out.push(' '),
                Some('t') => out.push(' '),
                Some(c) => out.push(c),
                None => break,
            },
            c => out.push(c),
        }
    }
    Some(out)
}

/// Pretty JSON input, or the raw streamed text if not parsed yet.
pub fn tool_input_text(tp: &ToolPart) -> String {
    if !tp.input.is_null() {
        if let Ok(s) = serde_json::to_string_pretty(&tp.input) {
            return s;
        }
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&tp.raw_input) {
        if let Ok(s) = serde_json::to_string_pretty(&v) {
            return s;
        }
    }
    if tp.raw_input.is_empty() {
        "…".into()
    } else {
        tp.raw_input.clone()
    }
}


// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

/// Draw the visible window of `doc` into `area`, applying focus and selection highlights.
pub fn draw(doc: &Doc, view: &ViewState, area: Rect, buf: &mut Buffer) {
    let focus_rows = view.focus.as_ref().and_then(|t| doc.target_rows(t));
    for y in 0..area.height {
        let row = view.scroll + y as usize;
        let Some((line, _)) = doc.line(row) else { break };
        buf.set_line(area.x, area.y + y, line, area.width);
        if let Some((a, b)) = focus_rows {
            if row >= a && row < b {
                // Focus: subtle marker in the gutter + element bg on the header row.
                if let Some(c) = buf.cell_mut((area.x, area.y + y)) {
                    c.set_symbol("▎");
                    c.set_fg(theme::PRIMARY);
                }
                if row == a {
                    for x in 1..area.width {
                        if let Some(c) = buf.cell_mut((area.x + x, area.y + y)) {
                            c.set_bg(theme::ELEMENT);
                        }
                    }
                }
            }
        }
        if let Some(sel) = &view.selection {
            if !sel.is_empty() {
                for x in 0..area.width {
                    if sel.contains(row, x) {
                        if let Some(c) = buf.cell_mut((area.x + x, area.y + y)) {
                            c.set_bg(theme::SELECTION);
                        }
                    }
                }
            }
        }
    }
}

/// Plain text of the selected region (decorations stripped, trailing spaces trimmed).
pub fn selection_text(doc: &Doc, sel: &Selection) -> String {
    let (a, b) = sel.ordered();
    let mut out: Vec<String> = Vec::new();
    for row in a.0..=b.0 {
        let Some((line, it)) = doc.line(row) else { break };
        let x0 = if row == a.0 { a.1 as usize } else { 0 }.max(it.deco as usize);
        let x1 = if row == b.0 { b.1 as usize } else { usize::MAX };
        if x1 <= x0 {
            out.push(String::new());
            continue;
        }
        let s = text::slice_cols(line, x0, x1);
        // Strip a code-box bar if the selection started at it.
        let s = s.strip_prefix("▌ ").unwrap_or(&s).trim_end();
        // Buttons are UI, not content.
        let s = s.strip_suffix("⧉ copy").unwrap_or(s).trim_end().to_string();
        out.push(s);
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use hn_proto::{Event, Message, SessionInfo};

    fn session() -> SessionState {
        let mut s = SessionState::default();
        let tool = ToolPart {
            call_id: "c1".into(),
            name: "bash".into(),
            purpose: "list files".into(),
            input: serde_json::json!({"command": "ls", "purpose": "list files"}),
            state: ToolState::Done,
            output: "a\nb".into(),
            time_start: 1,
            time_end: Some(1001),
            ..Default::default()
        };
        let user = Message {
            id: "u".into(),
            role: Role::User,
            origin: Origin::Human,
            time: 1,
            completed: None,
            parts: vec![Part::Text { text: "你好 world".into(), time: 1 }],
            model: None,
            agent: Some("build".into()),
            usage: None,
            finish: None,
            error: None,
            replay_key: None,
        };
        let mut asst = user.clone();
        asst.id = "a".into();
        asst.role = Role::Assistant;
        asst.time = 2;
        asst.parts = vec![
            Part::Reasoning { text: "thinking hard".into(), time_start: 2, time_end: Some(1002), replay: Default::default() },
            Part::Tool(tool),
            Part::Text { text: "**done**".into(), time: 3 },
        ];
        s.apply(Event::Snapshot { info: SessionInfo::default(), messages: vec![user, asst] });
        s
    }

    fn opts() -> Opts {
        Opts { now: 10_000, show_thinking: true, details: false, compact: false }
    }

    #[test]
    fn builds_and_toggles_tool() {
        let s = session();
        let cat = Catalog::default();
        let mut v = ViewState::default();
        let d1 = build(&s, &mut v, &cat, opts(), 80);
        let t = Target { msg: "a".into(), idx: 1 };
        let (a, b) = d1.target_rows(&t).unwrap();
        assert_eq!(b - a, 1);
        assert!(matches!(d1.hit_at(a, 5), Some(Hit::Toggle(x)) if *x == t));
        v.toggle(&t);
        let d2 = build(&s, &mut v, &cat, opts(), 80);
        let (a, b) = d2.target_rows(&t).unwrap();
        assert!(b - a > 4, "expanded tool shows input/output");
        let all: Vec<String> = (0..d2.height).map(|r| text::line_text(d2.line(r).unwrap().0)).collect();
        assert!(all.iter().any(|l| l.contains("\"command\": \"ls\"")));
        // every line fits the width
        for r in 0..d2.height {
            assert!(d2.line(r).unwrap().0.width() <= 80, "{:?}", all[r]);
        }
    }

    #[test]
    fn reasoning_collapsed_one_line_with_tail() {
        let s = session();
        let mut v = ViewState::default();
        let d = build(&s, &mut v, &Catalog::default(), opts(), 60);
        let t = Target { msg: "a".into(), idx: 0 };
        let (a, b) = d.target_rows(&t).unwrap();
        assert_eq!(b - a, 1);
        let l = text::line_text(d.line(a).unwrap().0);
        assert!(l.contains("Thought") && l.contains("1.0s"), "{l}");
        assert!(l.contains("thinking hard"));
    }

    #[test]
    fn selection_copies_without_decoration() {
        let s = session();
        let mut v = ViewState::default();
        let d = build(&s, &mut v, &Catalog::default(), opts(), 60);
        // find the user text row
        let row = (0..d.height).find(|&r| text::line_text(d.line(r).unwrap().0).contains("你好")).unwrap();
        let sel = Selection { anchor: (row, 0), head: (row, 60) };
        assert_eq!(selection_text(&d, &sel), "你好 world");
    }

    #[test]
    fn scrape_purpose_from_partial_json() {
        assert_eq!(scrape_json_string(r#"{"command":"ls","purpose":"list th"#, "purpose").unwrap(), "list th");
        assert!(scrape_json_string(r#"{"comm"#, "purpose").is_none());
    }
}
