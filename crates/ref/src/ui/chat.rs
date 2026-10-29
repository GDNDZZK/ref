//! Transcript rendering: messages → cached, wrapped lines with click targets; virtualized drawing;
//! in-app text selection.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ref_proto::*;
use unicode_segmentation::UnicodeSegmentation;

use super::markdown;
use super::text::{self, hard_wrap, line_width, pad_line, tail_fit, truncate, width, wrap_text};
use super::theme::*;
use crate::state::SessionView;
use crate::util::{fmt_dur, fmt_time, fmt_tokens, spinner};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Action {
    Copy(MessageId),
    TogglePart(PartId),
    ToggleMsg(MessageId),
}

#[derive(Debug, Clone)]
pub struct Hit {
    pub x0: usize,
    pub x1: usize,
    pub action: Action,
}

#[derive(Debug, Clone)]
pub struct RLine {
    pub line: Line<'static>,
    pub hits: Vec<Hit>,
}

impl RLine {
    fn plain(line: Line<'static>) -> Self {
        RLine { line, hits: vec![] }
    }
    fn with(line: Line<'static>, action: Action) -> Self {
        let w = line_width(&line);
        RLine { line, hits: vec![Hit { x0: 0, x1: w, action }] }
    }
}

pub struct Opts<'a> {
    pub width: usize,
    pub compact: bool,
    pub details: bool,
    pub thinking: bool,
    pub tick: u64,
    pub now: i64,
    pub agent_colors: &'a HashMap<String, Color>,
    pub highlight: bool,
}

impl Opts<'_> {
    pub fn agent_color(&self, a: &str) -> Color {
        self.agent_colors.get(a).copied().unwrap_or(SECONDARY)
    }
}

/// Selection in absolute content coordinates (line index, display column).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sel {
    pub anchor: (usize, usize),
    pub head: (usize, usize),
}

impl Sel {
    pub fn ordered(&self) -> ((usize, usize), (usize, usize)) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }
    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }
}

/// Per-session view state (scroll, expansion, caches).
pub struct ChatView {
    /// First visible line (absolute).
    pub scroll_top: usize,
    pub follow: bool,
    /// Parts / messages whose expansion differs from the default.
    pub toggled: HashSet<String>,
    pub selection: Option<Sel>,
    /// Message select mode: index of the highlighted message.
    pub msg_select: Option<usize>,
    /// Unseen content below while scrolled up.
    pub unseen: bool,
    cache: HashMap<MessageId, (u64, Arc<Vec<RLine>>)>,
    md_cache: HashMap<PartId, (u64, Arc<Vec<Line<'static>>>)>,
    /// Layout from the last draw.
    pub blocks: Vec<(usize, Arc<Vec<RLine>>)>,
    pub block_msgs: Vec<MessageId>,
    pub total: usize,
    pub area: Rect,
    pub height: usize,
    last_rev: u64,
    layout_width: usize,
}

impl Default for ChatView {
    fn default() -> Self {
        ChatView {
            scroll_top: 0,
            follow: true,
            toggled: HashSet::new(),
            selection: None,
            msg_select: None,
            unseen: false,
            cache: HashMap::new(),
            md_cache: HashMap::new(),
            blocks: Vec::new(),
            block_msgs: Vec::new(),
            total: 0,
            area: Rect::default(),
            height: 0,
            last_rev: 0,
            layout_width: 0,
        }
    }
}

impl ChatView {

    pub fn is_expanded(&self, id: &str, default: bool) -> bool {
        default ^ self.toggled.contains(id)
    }

    pub fn toggle(&mut self, id: &str) {
        if !self.toggled.remove(id) {
            self.toggled.insert(id.to_string());
        }
    }

    pub fn scroll_by(&mut self, delta: isize) {
        let max = self.total.saturating_sub(self.height);
        let top = (self.scroll_top as isize + delta).clamp(0, max as isize) as usize;
        self.scroll_top = top;
        self.follow = top >= max;
        if self.follow {
            self.unseen = false;
        }
    }

    pub fn scroll_to_top(&mut self) {
        self.scroll_top = 0;
        self.follow = self.total <= self.height;
    }

    pub fn scroll_to_bottom(&mut self) {
        self.follow = true;
        self.unseen = false;
        self.scroll_top = self.total.saturating_sub(self.height);
    }

    /// Absolute line index of the start of message `i` (from the last layout).
    pub fn msg_start(&self, i: usize) -> Option<(usize, usize)> {
        self.blocks.get(i).map(|(s, b)| (*s, b.len()))
    }

    pub fn ensure_visible(&mut self, i: usize) {
        if let Some((s, len)) = self.msg_start(i) {
            if s < self.scroll_top {
                self.scroll_top = s;
                self.follow = false;
            } else if s + len > self.scroll_top + self.height {
                let want = (s + len).saturating_sub(self.height).min(s);
                self.scroll_top = want;
                self.follow = self.scroll_top + self.height >= self.total;
            }
        }
    }

    /// Absolute line at a screen row (relative to area.y).
    pub fn line_at(&self, abs: usize) -> Option<(&RLine, usize)> {
        let i = self.blocks.partition_point(|(s, _)| *s <= abs).checked_sub(1)?;
        let (s, b) = &self.blocks[i];
        b.get(abs - s).map(|l| (l, i))
    }

    pub fn hit_at(&self, abs: usize, col: usize) -> Option<Action> {
        let (l, _) = self.line_at(abs)?;
        l.hits.iter().find(|h| col >= h.x0 && col < h.x1).map(|h| h.action.clone())
    }

    pub fn msg_index_at(&self, abs: usize) -> Option<usize> {
        self.line_at(abs).map(|(_, i)| i)
    }

    /// Text of the current selection: visible text, trailing spaces trimmed per line.
    pub fn selected_text(&self) -> String {
        let Some(sel) = self.selection else { return String::new() };
        let ((l0, c0), (l1, c1)) = sel.ordered();
        let mut out = Vec::new();
        for abs in l0..=l1 {
            let Some((l, _)) = self.line_at(abs) else { continue };
            let s = text::line_text(&l.line);
            let from = if abs == l0 { c0 } else { 0 };
            let to = if abs == l1 { c1 } else { usize::MAX };
            out.push(slice_cols(&s, from, to).trim_end().to_string());
        }
        out.join("\n")
    }

    /// Build (or fetch from cache) the lines of all messages.
    pub fn layout(&mut self, sv: &SessionView, o: &Opts) {
        // When the width changes while scrolled up, keep the same message line at the top.
        let anchor = if !self.follow && self.layout_width != o.width && !self.blocks.is_empty() {
            let i = self.blocks.partition_point(|(s, _)| *s <= self.scroll_top).saturating_sub(1);
            self.block_msgs.get(i).map(|id| (id.clone(), self.scroll_top - self.blocks[i].0))
        } else {
            None
        };
        self.layout_width = o.width;
        let mut blocks = Vec::with_capacity(sv.messages.len());
        let mut ids = Vec::with_capacity(sv.messages.len());
        let mut start = 0usize;
        let n = sv.messages.len();
        for (i, e) in sv.messages.iter().enumerate() {
            let m = &e.msg;
            let live = is_live(m);
            let mut h = std::collections::hash_map::DefaultHasher::new();
            e.rev.hash(&mut h);
            o.width.hash(&mut h);
            o.compact.hash(&mut h);
            o.details.hash(&mut h);
            o.thinking.hash(&mut h);
            (i + 1 == n).hash(&mut h);
            for p in &m.parts {
                self.toggled.contains(p.id()).hash(&mut h);
            }
            self.toggled.contains(&m.id).hash(&mut h);
            if live || has_countdown(m) {
                o.tick.hash(&mut h);
            }
            let key = h.finish();
            let lines = match self.cache.get(&m.id) {
                Some((k, l)) if *k == key => l.clone(),
                _ => {
                    let l = Arc::new(self.render_message(sv, m, o, i + 1 == n));
                    self.cache.insert(m.id.clone(), (key, l.clone()));
                    l
                }
            };
            blocks.push((start, lines.clone()));
            ids.push(m.id.clone());
            start += lines.len();
        }
        // Drop caches of removed messages occasionally.
        if self.cache.len() > n + 64 {
            let keep: HashSet<&MessageId> = ids.iter().collect();
            self.cache.retain(|k, _| keep.contains(k));
        }
        if start > self.total && !self.follow && sv.rev != self.last_rev {
            self.unseen = true;
        }
        self.last_rev = sv.rev;
        self.total = start;
        self.blocks = blocks;
        self.block_msgs = ids;
        if let Some((id, off)) = anchor {
            if let Some(i) = self.block_msgs.iter().position(|m| *m == id) {
                let (s, b) = &self.blocks[i];
                self.scroll_top = s + off.min(b.len().saturating_sub(1));
            }
        }
    }

    fn md(&mut self, id: &str, text: &str, width: usize, style: Style, highlight: bool) -> Arc<Vec<Line<'static>>> {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut h);
        width.hash(&mut h);
        format!("{style:?}").hash(&mut h);
        let key = h.finish();
        if let Some((k, l)) = self.md_cache.get(id) {
            if *k == key {
                return l.clone();
            }
        }
        let l = Arc::new(markdown::render(text, width, style, highlight));
        if self.md_cache.len() > 4096 {
            self.md_cache.clear();
        }
        self.md_cache.insert(id.to_string(), (key, l.clone()));
        l
    }

    fn render_message(&mut self, sv: &SessionView, m: &Message, o: &Opts, last: bool) -> Vec<RLine> {
        let mut out = match m.role {
            Role::User => self.render_user(sv, m, o),
            Role::Assistant => self.render_assistant(sv, m, o, last),
        };
        out.push(RLine::plain(Line::default()));
        out
    }

    fn render_user(&mut self, sv: &SessionView, m: &Message, o: &Opts) -> Vec<RLine> {
        let w = o.width;
        let mut out = Vec::new();
        let agent_col = o.agent_color(m.agent.as_deref().unwrap_or(&sv.info.agent));
        let time = fmt_time(m.time);
        let copy = "⧉ copy";
        // Compaction: separator box.
        if m.origin == MessageOrigin::Compaction {
            let expanded = self.is_expanded(&m.id, false);
            let label = format!(" {} context compacted ", if expanded { "▾" } else { "▸" });
            let right = format!(" {time}  ");
            let fill = w.saturating_sub(2 + width(&label) + width(&right) + width(copy));
            let line = Line::from(vec![
                Span::styled("──", fg(BORDER)),
                Span::styled(label, bold(INFO)),
                Span::styled("─".repeat(fill), fg(BORDER)),
                Span::styled(right, muted()),
                Span::styled(copy, fg(MUTED)),
            ]);
            let lw = line_width(&line);
            let mut rl = RLine::with(line, Action::ToggleMsg(m.id.clone()));
            rl.hits[0].x1 = lw.saturating_sub(width(copy));
            rl.hits.push(Hit { x0: lw - width(copy), x1: lw, action: Action::Copy(m.id.clone()) });
            out.push(rl);
            if expanded {
                let md = self.md(&format!("{}-c", m.id), &m.raw_text(), w.saturating_sub(4), muted(), false);
                for l in md.iter() {
                    let mut spans = vec![Span::styled("  │ ", fg(BORDER))];
                    spans.extend(l.spans.iter().cloned());
                    out.push(RLine::plain(Line::from(spans)));
                }
                out.push(RLine::plain(Line::from(Span::styled("─".repeat(w), fg(BORDER)))));
            }
            return out;
        }
        let (label, border, text_style): (Option<Span<'static>>, Color, Style) = match &m.origin {
            MessageOrigin::User => (None, agent_col, text()),
            MessageOrigin::Peer { from_name, .. } => {
                (Some(Span::styled(format!("✉ from \"{from_name}\""), bold(ACCENT))), ACCENT, text())
            }
            MessageOrigin::Spawn { from_name, .. } => {
                (Some(Span::styled(format!("↳ spawned by \"{from_name}\""), bold(INFO))), INFO, text())
            }
            MessageOrigin::Reminder => (
                Some(Span::styled("⟳ reminder", muted().add_modifier(Modifier::ITALIC))),
                MUTED,
                muted().add_modifier(Modifier::ITALIC),
            ),
            MessageOrigin::Compaction => unreachable!(),
        };
        let bg = Style::new().bg(PANEL);
        let bar = Span::styled("┃", Style::new().fg(border).bg(PANEL));
        let pad = if o.compact { " " } else { "  " };
        let inner = w.saturating_sub(1 + pad.len() + 1).max(4);
        let row = |spans: Vec<Span<'static>>| -> Line<'static> {
            let mut v = vec![bar.clone(), Span::styled(pad, bg)];
            v.extend(spans.into_iter().map(|s| {
                let st = s.style.bg(PANEL);
                Span::styled(s.content, st)
            }));
            pad_line(Line::from(v), w, bg)
        };
        // Header row: label left, time + copy right.
        let right = vec![Span::styled(format!("{time}  "), muted()), Span::styled(copy, fg(MUTED))];
        let left = label.map(|l| vec![l]).unwrap_or_default();
        let (hl, _) = lr(left, right, inner);
        let mut rl = RLine::plain(row(hl.spans));
        let off = 1 + pad.len();
        rl.hits.push(Hit { x0: off + inner - width(copy), x1: off + inner, action: Action::Copy(m.id.clone()) });
        out.push(rl);
        // Body.
        let mut img = 0;
        for p in &m.parts {
            match p {
                Part::Text(t) => {
                    for l in wrap_text(t.text.trim_end(), inner, text_style, &[], &[]) {
                        out.push(RLine::plain(row(l.spans)));
                    }
                }
                Part::Image(i) => {
                    img += 1;
                    let name = i.name.as_ref().map(|n| format!(": {n}")).unwrap_or_default();
                    out.push(RLine::plain(row(vec![Span::styled(format!("[Image {img}{name}]"), bold(ACCENT))])));
                }
                _ => {}
            }
        }
        if !o.compact {
            out.push(RLine::plain(row(vec![])));
        }
        out
    }

    fn render_assistant(&mut self, sv: &SessionView, m: &Message, o: &Opts, last: bool) -> Vec<RLine> {
        let w = o.width;
        let indent = if o.compact { 1 } else { 3 };
        let ind = " ".repeat(indent);
        let inner = w.saturating_sub(indent).max(8);
        let mut out: Vec<RLine> = Vec::new();
        let mut prev_text = false;
        let msg_live = is_live(m) && last && (sv.busy() || m.time_end.is_none() && o.now - m.time < 120_000);
        for (pi, p) in m.parts.iter().enumerate() {
            match p {
                Part::Text(t) => {
                    if t.text.trim().is_empty() {
                        continue;
                    }
                    if pi > 0 && !out.is_empty() {
                        out.push(RLine::plain(Line::default()));
                    }
                    let src = if msg_live { markdown::close_partial(&t.text) } else { t.text.as_str().into() };
                    let md = self.md(&t.id, &src, inner, fg(TEXT), o.highlight);
                    for l in md.iter() {
                        let mut spans = vec![Span::raw(ind.clone())];
                        spans.extend(l.spans.iter().cloned());
                        out.push(RLine::plain(Line::from(spans)));
                    }
                    prev_text = true;
                }
                Part::Reasoning(r) => {
                    let live = r.time_end.is_none() && msg_live;
                    if r.text.trim().is_empty() && !live {
                        continue;
                    }
                    if prev_text {
                        out.push(RLine::plain(Line::default()));
                    }
                    prev_text = false;
                    let expanded = self.is_expanded(&r.id, o.thinking);
                    let icon = if live { spinner(o.tick).to_string() } else if expanded { "▾".into() } else { "▸".into() };
                    let label = if live {
                        "Thinking".to_string()
                    } else {
                        match r.time_end {
                            Some(e) => format!("Thought for {}", fmt_dur_s(e - r.time)),
                            None => "Thought".to_string(),
                        }
                    };
                    let head = vec![
                        Span::raw(ind.clone()),
                        Span::styled(format!("{icon} "), fg(if live { ACCENT } else { MUTED })),
                        Span::styled(label, Style::new().fg(MUTED).add_modifier(Modifier::BOLD)),
                    ];
                    let used = head.iter().map(|s| width(&s.content)).sum::<usize>() + 2;
                    let mut line = Line::from(head);
                    if !expanded {
                        let tail = tail_fit(&r.text, w.saturating_sub(used));
                        if !tail.is_empty() {
                            line.spans.push(Span::raw("  "));
                            line.spans.push(Span::styled(tail, muted().add_modifier(Modifier::ITALIC)));
                        }
                    }
                    out.push(RLine::with(line, Action::TogglePart(r.id.clone())));
                    if expanded {
                        let bar = vec![Span::raw(ind.clone()), Span::styled("│ ", fg(BORDER_SUBTLE))];
                        for l in wrap_text(r.text.trim(), w, muted().add_modifier(Modifier::ITALIC), &bar, &bar) {
                            out.push(RLine::plain(l));
                        }
                    }
                }
                Part::Tool(t) => {
                    if prev_text {
                        out.push(RLine::plain(Line::default()));
                    }
                    prev_text = false;
                    self.render_tool(sv, t, o, indent, &mut out);
                }
                Part::Image(i) => {
                    out.push(RLine::plain(Line::from(vec![
                        Span::raw(ind.clone()),
                        Span::styled(
                            format!(" Image{} ", i.name.as_ref().map(|n| format!(": {n}")).unwrap_or_default()),
                            Style::new().fg(PANEL).bg(ACCENT),
                        ),
                    ])));
                }
            }
        }
        if let Some(err) = &m.error {
            if !out.is_empty() {
                out.push(RLine::plain(Line::default()));
            }
            let first = vec![Span::raw(ind.clone()), Span::styled("✗ ", fg(ERROR))];
            let cont = vec![Span::raw(" ".repeat(indent + 2))];
            for l in wrap_text(err, w, fg(ERROR), &first, &cont) {
                out.push(RLine::plain(l));
            }
        }
        // Footer.
        let live = msg_live;
        let agent = m.agent.clone().unwrap_or_else(|| sv.info.agent.clone());
        let model = m.model.clone().unwrap_or_default();
        let model_short = model.split_once('/').map(|(_, b)| b.to_string()).unwrap_or(model);
        let mut left = vec![
            Span::raw(ind.clone()),
            Span::styled(if live { format!("{} ", spinner(o.tick)) } else { "▣ ".into() }, fg(o.agent_color(&agent))),
            Span::styled(agent.clone(), fg(o.agent_color(&agent))),
            Span::styled(format!(" · {model_short}"), muted()),
        ];
        if let Some(v) = &m.variant {
            left.push(Span::styled(format!(" · {v}"), fg(WARNING)));
        }
        let end = m.time_end.unwrap_or(if live { o.now } else { m.time });
        if end > m.time {
            left.push(Span::styled(format!(" · {}", fmt_dur(end - m.time)), muted()));
        }
        if let Some(u) = &m.usage {
            if !o.compact || w > 60 {
                left.push(Span::styled(
                    format!(" · ↑{} ↓{}", fmt_tokens(u.input + u.cache_read + u.cache_write), fmt_tokens(u.output)),
                    muted(),
                ));
            }
        }
        match m.finish.as_deref() {
            Some("aborted") => left.push(Span::styled(" · interrupted", fg(WARNING))),
            Some("length") => left.push(Span::styled(" · max tokens", fg(WARNING))),
            _ => {}
        }
        // Copy is offered on every message with content (raw text, else the raw tool calls).
        let has_text = !m.parts.is_empty() || m.error.is_some();
        let mut right = vec![Span::styled(fmt_time(m.time), muted())];
        if has_text {
            right.push(Span::styled("  ⧉ copy", fg(MUTED)));
        }
        if !out.is_empty() {
            out.push(RLine::plain(Line::default()));
        }
        let (line, _) = lr(left, right, w);
        let mut rl = RLine::plain(line);
        if has_text {
            rl.hits.push(Hit { x0: w.saturating_sub(width("⧉ copy")), x1: w, action: Action::Copy(m.id.clone()) });
        }
        out.push(rl);
        out
    }

    fn render_tool(&mut self, sv: &SessionView, t: &ToolPart, o: &Opts, indent: usize, out: &mut Vec<RLine>) {
        let w = o.width;
        let ind = " ".repeat(indent);
        let (icon, icol) = match t.state {
            ToolState::Streaming => (spinner(o.tick).to_string(), MUTED),
            ToolState::Running => (spinner(o.tick).to_string(), PRIMARY),
            ToolState::AwaitingPermission => ("⏸".to_string(), WARNING),
            ToolState::Delayed => ("◷".to_string(), INFO),
            ToolState::Done => ("✓".to_string(), SUCCESS),
            ToolState::Error => ("✗".to_string(), ERROR),
            ToolState::Denied | ToolState::Cancelled => ("⊘".to_string(), WARNING),
        };
        let expanded = self.is_expanded(&t.id, o.details && !o.compact);
        let name_style = match t.state {
            ToolState::AwaitingPermission => bold(WARNING),
            ToolState::Error => bold(ERROR),
            ToolState::Denied | ToolState::Cancelled => Style::new().fg(MUTED).add_modifier(Modifier::BOLD),
            _ => Style::new().fg(TEXT).add_modifier(Modifier::BOLD),
        };
        let start = t.time_start.unwrap_or(t.time);
        let mut right = vec![Span::styled(fmt_time(start), Style::new().fg(BORDER_ACTIVE))];
        let status = match t.state {
            ToolState::Done => t.time_end.map(|e| (fmt_dur(e - start), BORDER_ACTIVE)),
            ToolState::Error => Some((format!("failed{}", t.time_end.map(|e| format!(" · {}", fmt_dur(e - start))).unwrap_or_default()), ERROR)),
            ToolState::Running => Some((fmt_dur(o.now - start), BORDER_ACTIVE)),
            ToolState::Denied => Some(("denied".to_string(), WARNING)),
            ToolState::Cancelled => Some(("cancelled".to_string(), WARNING)),
            ToolState::AwaitingPermission => Some(("awaiting permission".to_string(), WARNING)),
            ToolState::Delayed => {
                let secs = t.run_at.map(|r| ((r - o.now).max(0) + 999) / 1000).unwrap_or(0);
                let hint = if w >= 90 { " · esc esc cancels" } else { "" };
                Some((format!("runs in {secs}s{hint}"), INFO))
            }
            ToolState::Streaming => None,
        };
        if let Some((s, c)) = status {
            right.push(Span::styled(" · ", fg(BORDER_SUBTLE)));
            right.push(Span::styled(s, fg(c)));
        }
        let rw: usize = right.iter().map(|s| width(&s.content)).sum();
        let lw_nat = indent + 2 + width(&t.name) + 2 + width(&t.purpose);
        // Narrow: keep the purpose readable; drop the start time first (the status stays).
        if lw_nat + rw + 1 > w && right.len() > 1 && w < 100 {
            right.drain(..2);
        }
        let purpose = if t.purpose.is_empty() {
            match t.state {
                ToolState::Streaming => "…".to_string(),
                _ => summarize_input(&t.input),
            }
        } else {
            t.purpose.clone()
        };
        let left = vec![
            Span::raw(ind.clone()),
            Span::styled(format!("{icon} "), fg(icol)),
            Span::styled(t.name.clone(), name_style),
            Span::raw("  "),
            Span::styled(purpose.replace('\n', " "), muted()),
        ];
        let (line, _) = lr(left, right, w);
        out.push(RLine::with(line, Action::TogglePart(t.id.clone())));
        if !expanded {
            return;
        }
        let pre = vec![Span::raw(format!("{ind}  ")), Span::styled("│ ", fg(BORDER))];
        let label = |s: &str, c: Color| {
            RLine::with(
                Line::from(vec![Span::raw(format!("{ind}  ")), Span::styled(format!("┌ {s}"), fg(c))]),
                Action::TogglePart(t.id.clone()),
            )
        };
        out.push(label("input", BORDER_ACTIVE));
        let input = if t.input.is_null() || (t.state == ToolState::Streaming && !t.input_raw.is_empty()) {
            t.input_raw.clone()
        } else {
            serde_json::to_string_pretty(&t.input).unwrap_or_default()
        };
        for l in hard_wrap(&input, w, fg(INFO), &pre) {
            out.push(RLine::plain(l));
        }
        let live = sv.live_output.get(&t.id);
        let (body, is_tail) = if !t.output.is_empty() || t.state.is_final() {
            (t.output.clone(), false)
        } else if let Some(l) = live {
            let lines: Vec<&str> = l.lines().collect();
            let tail = lines[lines.len().saturating_sub(30)..].join("\n");
            (tail, lines.len() > 30)
        } else {
            (String::new(), false)
        };
        if !body.is_empty() || t.state.is_final() {
            let c = if t.state == ToolState::Error { ERROR } else { BORDER_ACTIVE };
            out.push(label(if is_tail { "output (live tail)" } else { "output" }, c));
            let st = if t.state == ToolState::Error { fg(ERROR) } else { fg(TEXT) };
            for l in hard_wrap(body.trim_end_matches('\n'), w, st, &pre) {
                out.push(RLine::plain(l));
            }
        }
        for i in &t.images {
            let mut v = pre.clone();
            v.push(Span::styled(
                format!("[image {}{}]", i.mime, i.name.as_ref().map(|n| format!(" {n}")).unwrap_or_default()),
                fg(ACCENT),
            ));
            out.push(RLine::plain(Line::from(v)));
        }
        let lines = body.lines().count();
        out.push(RLine::with(
            Line::from(vec![
                Span::raw(format!("{ind}  ")),
                Span::styled(format!("└ {lines} lines · {} chars", body.chars().count()), fg(BORDER)),
            ]),
            Action::TogglePart(t.id.clone()),
        ));
    }
}

fn fmt_dur_s(ms: i64) -> String {
    if ms < 1000 {
        "<1s".to_string()
    } else if ms < 60_000 {
        format!("{}s", (ms.max(0) + 500) / 1000)
    } else {
        fmt_dur(ms)
    }
}

/// Something in the message changes with time (spinner, elapsed time).
pub fn is_live(m: &Message) -> bool {
    if m.role == Role::Assistant && m.time_end.is_none() && m.finish.is_none() {
        return true;
    }
    m.parts.iter().any(|p| matches!(p, Part::Tool(t) if !t.state.is_final()))
}

fn has_countdown(m: &Message) -> bool {
    m.parts.iter().any(|p| matches!(p, Part::Tool(t) if t.state == ToolState::Delayed))
}

fn summarize_input(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Object(o) => o
            .iter()
            .filter(|(k, _)| k.as_str() != "purpose")
            .take(3)
            .map(|(k, v)| {
                let s = match v {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                format!("{k}={}", truncate(&s.replace('\n', " "), 40))
            })
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

/// Left + right aligned line within `w`; left is truncated if needed. Returns the line and the column at
/// which the right part starts.
pub fn lr(left: Vec<Span<'static>>, right: Vec<Span<'static>>, w: usize) -> (Line<'static>, usize) {
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
    let gap = w.saturating_sub(used + rw);
    out.push(Span::raw(" ".repeat(gap)));
    let x = used + gap;
    out.extend(right);
    (Line::from(out), x)
}

/// Slice a string by display columns [from, to).
pub fn slice_cols(s: &str, from: usize, to: usize) -> String {
    let mut out = String::new();
    let mut c = 0;
    for g in s.graphemes(true) {
        let gw = width(g);
        if c >= to {
            break;
        }
        if c + gw > from {
            out.push_str(g);
        }
        c += gw;
    }
    out
}

/// Draw the chat into `area` (virtualized). Call `view.layout` first.
pub fn draw(buf: &mut Buffer, area: Rect, view: &mut ChatView, selected_msg: Option<usize>) {
    draw_ex(buf, area, view, selected_msg, true)
}

/// Like `draw`; with `track == false` only the scrollbar thumb is drawn (over a pane border).
pub fn draw_ex(buf: &mut Buffer, area: Rect, view: &mut ChatView, selected_msg: Option<usize>, track: bool) {
    view.area = area;
    view.height = area.height as usize;
    let h = area.height as usize;
    let max_top = view.total.saturating_sub(h);
    if view.follow {
        view.scroll_top = max_top;
    }
    view.scroll_top = view.scroll_top.min(max_top);
    if view.scroll_top >= max_top {
        view.follow = true;
        view.unseen = false;
    }
    let top = view.scroll_top;
    let gutter = 1u16;
    let x0 = area.x + gutter;
    let cw = area.width.saturating_sub(gutter + 1);
    // Find the first block intersecting.
    let first = view.blocks.partition_point(|(s, _)| *s <= top).saturating_sub(1);
    let mut y = 0usize;
    'outer: for (bi, (s, lines)) in view.blocks.iter().enumerate().skip(first) {
        let skip = top.saturating_sub(*s);
        for l in lines.iter().skip(skip) {
            if y >= h {
                break 'outer;
            }
            let row = area.y + y as u16;
            buf.set_line(x0, row, &l.line, cw);
            if selected_msg == Some(bi) {
                buf.set_string(area.x, row, "▌", fg(PRIMARY));
            }
            y += 1;
        }
    }
    // Selection highlight.
    if let Some(sel) = view.selection {
        if !sel.is_empty() {
            let ((l0, c0), (l1, c1)) = sel.ordered();
            for yy in 0..h {
                let abs = top + yy;
                if abs < l0 || abs > l1 {
                    continue;
                }
                let lw = view.line_at(abs).map(|(l, _)| line_width(&l.line)).unwrap_or(0);
                let from = if abs == l0 { c0 } else { 0 };
                let to = if abs == l1 { c1 } else { lw.max(from + 1) };
                for c in from..to.min(cw as usize) {
                    if let Some(cell) = buf.cell_mut((x0 + c as u16, area.y + yy as u16)) {
                        cell.set_bg(SELECTION);
                    }
                }
            }
        }
    }
    // Scrollbar.
    if view.total > h && h > 2 {
        let thumb = ((h * h) / view.total).max(1);
        let pos = (top * (h - thumb)).checked_div(max_top).unwrap_or(0);
        let sx = area.x + area.width - 1;
        for i in 0..h {
            let on = i >= pos && i < pos + thumb;
            if on {
                buf.set_string(sx, area.y + i as u16, "┃", fg(BORDER_ACTIVE));
            } else if track {
                buf.set_string(sx, area.y + i as u16, "│", fg(BORDER_SUBTLE));
            }
        }
    }
    if view.unseen && !view.follow {
        let label = " ↓ new messages ";
        let lw = width(label) as u16;
        if area.width > lw + 2 && h > 0 {
            let x = area.x + (area.width - lw) / 2;
            buf.set_string(x, area.y + area.height - 1, label, Style::new().fg(PANEL).bg(PRIMARY).add_modifier(Modifier::BOLD));
        }
    }
}

/// Column within the content area (excludes gutter) for a screen x.
pub fn content_col(view: &ChatView, x: u16) -> usize {
    x.saturating_sub(view.area.x + 1) as usize
}
