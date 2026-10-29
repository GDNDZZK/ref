//! Renders one `Message` into rows (cached by the chat view).

use std::collections::BTreeSet;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ref_proto::{AgentInfo, Message, MessageSource, ModelInfo, Part, Role, ToolState};
use serde_json::Value;

use ref_proto::TodoStatus;

use super::text::{self, width};
use super::todo::{parse_todos, todo_rows};
use super::{Hit, Row, markdown, theme};

/// Max output lines shown in an expanded tool block before "show all".
pub const OUTPUT_PREVIEW_LINES: usize = 200;

pub struct Ctx<'a> {
    pub agents: &'a [AgentInfo],
    pub models: &'a [ModelInfo],
    /// Part indices whose expansion is toggled relative to the default.
    pub toggled: Option<&'a BTreeSet<usize>>,
    /// Part indices whose full output is shown.
    pub show_all: Option<&'a BTreeSet<usize>>,
    /// Default expansion for tools / reasoning.
    pub details: bool,
    pub thinking: bool,
    pub now: i64,
    pub tick: u64,
    /// Compact rendering for split tiles (no blocks, no footer buttons).
    pub compact: bool,
    /// Agent of the session (user bar colour).
    pub session_agent: &'a str,
    /// Message precedes the last compaction: still shown, but no longer sent to the model.
    pub stale: bool,
    /// This is the last compaction message and older messages precede it (the model's context
    /// starts here).
    pub context_start: bool,
}

impl Ctx<'_> {
    fn expanded(&self, idx: usize, default: bool) -> bool {
        default ^ self.toggled.is_some_and(|t| t.contains(&idx))
    }
    fn all(&self, idx: usize) -> bool {
        self.show_all.is_some_and(|t| t.contains(&idx))
    }
}

/// Left padding of assistant content.
const PAD: usize = 2;

pub fn render(m: &Message, cx: &Ctx<'_>, w: usize) -> Vec<Row> {
    let w = w.max(20);
    let mut rows = match m.role {
        Role::User => render_user(m, cx, w),
        Role::Assistant => render_assistant(m, cx, w),
    };
    if cx.stale {
        dim_rows(&mut rows);
    }
    rows
}

/// How much text before the last compaction is faded towards the background.
pub const STALE_DIM: f32 = 0.3;

/// Fades foreground colours (backgrounds and click zones are kept).
pub fn dim_rows(rows: &mut [Row]) {
    let dark = Color::Rgb(0, 0, 0);
    for r in rows {
        for s in &mut r.line.spans {
            let fg = match s.style.fg {
                None | Some(Color::Reset) => theme::TEXT,
                Some(c) => c,
            };
            s.style.fg = Some(theme::blend(fg, dark, STALE_DIM));
        }
    }
}

/// Text of the hint under the divider where the model's context starts.
pub const CONTEXT_HINT: &str = "messages above stay visible but are no longer sent to the model";

/// Full-width `──── ◇ Context compacted · 14:02:11 · click to view summary ──── ⧉ copy` row.
fn compaction_divider(m: &Message, exp: bool, w: usize) -> Row {
    let total = w.saturating_sub(1).max(20);
    let bst = theme::fg(theme::BORDER);
    let time = text::fmt_time(m.time);
    let right = vec![Span::raw(" "), copy_button(false)];
    let rw = spans_width(&right);
    let head = |hint: bool| {
        let mut v = vec![
            Span::styled(if exp { "▾ " } else { "▸ " }, theme::fg(theme::DIM)),
            Span::styled("──── ", bst),
            Span::styled("◇ ", theme::fg(theme::ACCENT)),
            Span::styled("Context compacted", theme::bold(theme::ACCENT)),
            Span::styled(format!(" · {time}"), theme::muted()),
        ];
        if hint {
            v.push(Span::styled(if exp { " · click to hide summary" } else { " · click to view summary" }, theme::muted()));
        }
        v.push(Span::raw(" "));
        v
    };
    let mut left = head(true);
    if spans_width(&left) + rw + 4 > total {
        left = head(false);
    }
    let left = truncate_spans(left, total.saturating_sub(rw));
    let fill = total.saturating_sub(spans_width(&left) + rw);
    let mut spans = left;
    spans.push(Span::styled("─".repeat(fill), bst));
    spans.extend(right);
    let x0 = (total - 6) as u16;
    Row::new(Line::from(spans)).decor().zone(x0, x0 + 6, Hit::Copy { msg: m.id.clone() }).hit(Hit::Toggle { msg: m.id.clone(), part: 0 })
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Builds `left … right` within `w` columns, truncating `left` as needed.
pub fn compose(left: Vec<Span<'static>>, right: Vec<Span<'static>>, w: usize) -> (Line<'static>, usize) {
    let rw: usize = right.iter().map(|s| width(&s.content)).sum();
    let avail = w.saturating_sub(rw + if rw > 0 { 1 } else { 0 });
    let mut l = truncate_spans(left, avail);
    let lw: usize = l.iter().map(|s| width(&s.content)).sum();
    let gap = w.saturating_sub(lw + rw);
    let right_x = lw + gap;
    if rw > 0 && rw <= w {
        l.push(Span::raw(" ".repeat(gap)));
        l.extend(right);
    }
    (Line::from(l), right_x)
}

pub fn truncate_spans(spans: Vec<Span<'static>>, max: usize) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    let mut used = 0;
    let total: usize = spans.iter().map(|s| width(&s.content)).sum();
    if total <= max {
        return spans;
    }
    for s in spans {
        let sw = width(&s.content);
        if used + sw < max {
            used += sw;
            out.push(s);
        } else {
            let t = text::truncate(&s.content, max - used);
            out.push(Span::styled(t, s.style));
            break;
        }
    }
    out
}

fn spans_width(s: &[Span<'_>]) -> usize {
    s.iter().map(|x| width(&x.content)).sum()
}

fn model_name(models: &[ModelInfo], m: &ref_proto::ModelRef) -> String {
    models
        .iter()
        .find(|x| x.provider == m.provider && x.id == m.model)
        .map(|x| x.name.clone())
        .unwrap_or_else(|| m.model.clone())
}

fn titlecase(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn copy_button(hover: bool) -> Span<'static> {
    let _ = hover;
    Span::styled("⧉ copy", theme::fg(theme::DIM))
}

pub const UNDO_LABEL: &str = "↶ undo";

fn undo_button() -> Span<'static> {
    Span::styled(UNDO_LABEL, theme::fg(theme::DIM))
}

/// Shimmer: a soft highlight sweeping over `label`.
pub fn shimmer(label: &str, tick: u64, base: Color, hi: Color) -> Vec<Span<'static>> {
    let chars: Vec<char> = label.chars().collect();
    let n = chars.len() as i64;
    let period = n + 8;
    let pos = (tick as i64 % period) - 4;
    chars
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let d = (i as i64 - pos).abs() as f32;
            let t = (1.0 - d / 3.0).max(0.0);
            Span::styled(c.to_string(), Style::new().fg(theme::blend(base, hi, t)))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// user messages
// ---------------------------------------------------------------------------

fn render_user(m: &Message, cx: &Ctx<'_>, w: usize) -> Vec<Row> {
    let agent = m.agent.as_deref().unwrap_or(cx.session_agent);
    let bar_color = match &m.source {
        MessageSource::Human => theme::agent_color(cx.agents, agent),
        MessageSource::Peer { .. } | MessageSource::Spawn { .. } => theme::INFO,
        MessageSource::Reminder => theme::WARNING,
        MessageSource::Compaction => theme::ACCENT,
        MessageSource::Shell => theme::WARNING,
    };
    let raw = m.raw_text();
    let bg = theme::PANEL;
    let bar = Span::styled("┃", Style::new().fg(bar_color).bg(bg));
    let time = text::fmt_time(m.time);

    if cx.compact {
        return render_user_compact(m, cx, w, bar_color);
    }

    // Compaction: full-width divider (expandable summary below it).
    if m.source == MessageSource::Compaction {
        let exp = cx.expanded(0, false);
        let mut rows = vec![compaction_divider(m, exp, w)];
        if cx.context_start {
            let hint = text::truncate(CONTEXT_HINT, w.saturating_sub(4));
            let pad = w.saturating_sub(1 + width(&hint)) / 2;
            rows.push(Row::new(Line::from(vec![Span::raw(" ".repeat(pad)), Span::styled(hint, theme::fg(theme::DIM).add_modifier(Modifier::ITALIC))])).decor());
        }
        if exp {
            for r in markdown::render(&raw, w.saturating_sub(PAD + 4), theme::muted()) {
                rows.push(r.indent(vec![Span::raw(" ".repeat(PAD)), Span::styled("│ ", theme::fg(theme::ACCENT)), Span::raw(" ")]));
            }
        }
        return rows;
    }

    let inner = w.saturating_sub(5).max(1);
    let mut rows: Vec<Row> = Vec::new();
    let pad = |r: Row| -> Row {
        let lw = text::line_width(&r.line);
        let mut r = r.indent(vec![bar.clone(), Span::styled("  ", Style::new().bg(bg))]);
        r.line.spans.push(Span::styled(" ".repeat(w.saturating_sub(lw + 3)), Style::new().bg(bg)));
        let e = (3 + lw) as u16;
        if r.end.is_none() {
            r.end = Some(e);
        }
        r
    };
    // Header / top padding row.
    let header: Option<Vec<Span<'static>>> = match &m.source {
        MessageSource::Human => None,
        MessageSource::Peer { from_name, .. } => {
            Some(vec![Span::styled(format!("↘ message from \"{from_name}\""), theme::bold(theme::INFO).bg(bg))])
        }
        MessageSource::Spawn { from_name, .. } => {
            Some(vec![Span::styled(format!("↘ created by \"{from_name}\""), theme::bold(theme::INFO).bg(bg))])
        }
        MessageSource::Reminder => Some(vec![Span::styled("⟳ reminder", theme::bold(theme::WARNING).bg(bg))]),
        MessageSource::Shell => Some(vec![Span::styled("$ shell", theme::bold(theme::WARNING).bg(bg))]),
        MessageSource::Compaction => None,
    };
    match header {
        Some(h) => rows.push(pad(Row::new(Line::from(h))).decor()),
        None => rows.push(pad(Row::new(Line::default())).decor()),
    }
    let body_style = Style::new().fg(theme::TEXT).bg(bg);
    if m.source == MessageSource::Shell {
        for r in markdown::render(&raw, inner, body_style) {
            let mut r = r;
            for s in &mut r.line.spans {
                if s.style.bg.is_none() {
                    s.style = s.style.bg(bg);
                }
            }
            rows.push(pad(r));
        }
    } else {
        let clean = text::sanitize(&raw);
        let clean = clean.trim_matches('\n');
        if clean.is_empty() && !m.parts.iter().any(|p| matches!(p, Part::Image { .. })) {
            rows.push(pad(Row::new(Line::from(Span::styled("(empty)", theme::muted().bg(bg))))));
        }
        if !clean.is_empty() {
            for logical in clean.split('\n') {
                let wrapped = text::wrap_spans_flags(&[Span::styled(logical.to_string(), body_style)], inner);
                for (i, (l, sp)) in wrapped.into_iter().enumerate() {
                    rows.push(pad(Row::new(l).soft_sp(i > 0, sp)));
                }
            }
        }
    }
    // Image badges.
    let images: Vec<String> = m
        .parts
        .iter()
        .filter_map(|p| match p {
            Part::Image { name, image } => Some(name.clone().unwrap_or_else(|| image.mime.clone())),
            _ => None,
        })
        .collect();
    if !images.is_empty() {
        let mut spans = Vec::new();
        for (i, n) in images.iter().enumerate() {
            spans.push(Span::styled(" image ", Style::new().fg(bg).bg(theme::ACCENT).add_modifier(Modifier::BOLD)));
            spans.push(Span::styled(format!(" {} ", text::truncate(n, 30)), Style::new().fg(theme::MUTED).bg(theme::PANEL_HOVER)));
            if i + 1 < images.len() {
                spans.push(Span::styled(" ", Style::new().bg(bg)));
            }
        }
        rows.push(pad(Row::new(Line::from(truncate_spans(spans, inner))).decor()));
    }
    // Meta row: time + [undo] + copy, right aligned.
    let gap = || Span::styled("  ", Style::new().bg(bg));
    let human = m.source == MessageSource::Human;
    let mut right = vec![Span::styled(time.clone(), theme::muted().bg(bg)), gap()];
    if human {
        right.push(undo_button());
        right.push(gap());
    }
    right.push(copy_button(false));
    // One column wider than the body so the button lines up with assistant footers / tool rows.
    let (line, rx) = compose(vec![], right, inner + 1);
    let mut x0 = (rx + width(&time) + 2) as u16;
    let mut row = Row::new(line);
    if human {
        let uw = width(UNDO_LABEL) as u16;
        row = row.zone(x0, x0 + uw, Hit::Undo { msg: m.id.clone() });
        x0 += uw + 2;
    }
    let mut meta = pad(row.zone(x0, x0 + 6, Hit::Copy { msg: m.id.clone() })).decor();
    for s in &mut meta.line.spans {
        if s.style.bg.is_none() {
            s.style = s.style.bg(bg);
        }
    }
    rows.push(meta);
    rows
}

fn render_user_compact(m: &Message, _cx: &Ctx<'_>, w: usize, color: Color) -> Vec<Row> {
    if m.source == MessageSource::Compaction {
        // Compact divider: `── ◇ compacted 14:02:11 ─────`.
        let bst = theme::fg(theme::BORDER);
        let left = truncate_spans(
            vec![
                Span::styled("── ", bst),
                Span::styled("◇ ", theme::fg(theme::ACCENT)),
                Span::styled("compacted", theme::bold(theme::ACCENT)),
                Span::styled(format!(" {} ", text::fmt_clock(m.time)), theme::muted()),
            ],
            w,
        );
        let fill = w.saturating_sub(spans_width(&left));
        let mut spans = left;
        spans.push(Span::styled("─".repeat(fill), bst));
        return vec![Row::new(Line::from(spans)).decor()];
    }
    let label = match &m.source {
        MessageSource::Human => "you".to_string(),
        MessageSource::Peer { from_name, .. } => format!("from {from_name}"),
        MessageSource::Spawn { from_name, .. } => format!("by {from_name}"),
        MessageSource::Reminder => "reminder".into(),
        MessageSource::Compaction => "compacted".into(),
        MessageSource::Shell => "shell".into(),
    };
    let raw = text::sanitize(&m.raw_text());
    let mut rows = Vec::new();
    let head = vec![
        Span::styled("┃ ", theme::fg(color)),
        Span::styled(label, theme::bold(color)),
        Span::styled(format!(" {}", text::fmt_clock(m.time)), theme::muted()),
    ];
    rows.push(Row::new(Line::from(truncate_spans(head, w))).decor());
    let lines: Vec<&str> = raw.trim().lines().collect();
    let max = if m.source == MessageSource::Compaction { 1 } else { 6 };
    for l in lines.iter().take(max) {
        for (i, (wl, sp)) in text::wrap_spans_flags(&[Span::styled(l.to_string(), theme::text())], w.saturating_sub(2)).into_iter().enumerate() {
            rows.push(Row::new(wl).indent(vec![Span::styled("┃ ", theme::fg(color))]).soft_sp(i > 0, sp));
        }
    }
    if lines.len() > max {
        rows.push(Row::new(Line::from(Span::styled(format!("┃ … {} more lines", lines.len() - max), theme::muted()))).decor());
    }
    rows
}

// ---------------------------------------------------------------------------
// assistant messages
// ---------------------------------------------------------------------------

/// Whether a part starts / ends with a one-line row (header rows of tools and reasoning), as
/// `(top, bottom)`. One-line edges of neighbouring parts stack without a blank separator.
/// Expanding a tool or reasoning block only changes its bottom edge, so its header row never moves.
fn single_edges(p: &Part, idx: usize, cx: &Ctx<'_>) -> (bool, bool) {
    match p {
        // Todo rows carry their list inline: keep them visually separated from neighbours.
        Part::Tool { name, input, .. } if name == "todo" && !parse_todos(input).is_empty() => (false, false),
        Part::Tool { .. } => (true, cx.compact || !cx.expanded(idx, cx.details)),
        Part::Reasoning { .. } => (true, cx.compact || !cx.expanded(idx, cx.thinking)),
        _ => (false, false),
    }
}

fn render_assistant(m: &Message, cx: &Ctx<'_>, w: usize) -> Vec<Row> {
    let live = crate::state::message_live(m);
    let streaming = m.time_end.is_none() && m.error.is_none();
    let mut rows: Vec<Row> = Vec::new();
    let mut prev_single: Option<bool> = None;
    let last_idx = m.parts.len().saturating_sub(1);
    let pad = if cx.compact { 0 } else { PAD };
    let inner = w.saturating_sub(pad + if cx.compact { 0 } else { 1 }).max(10);
    for (idx, part) in m.parts.iter().enumerate() {
        // Skip empty text parts entirely (except while they're the streaming tail).
        if let Part::Text { text: t, .. } = part
            && t.trim().is_empty()
            && !(streaming && idx == last_idx)
        {
            continue;
        }
        if let Part::Reasoning { text: t, .. } = part
            && t.trim().is_empty()
            && !(streaming && idx == last_idx)
        {
            continue;
        }
        let (top, bottom) = single_edges(part, idx, cx);
        if let Some(ps) = prev_single
            && !(ps && top)
        {
            rows.push(Row::blank());
        }
        prev_single = Some(bottom);
        let part_rows = match part {
            Part::Text { text: t, .. } => {
                let mut r = markdown::render(t, inner, theme::text());
                if streaming && idx == last_idx {
                    // Blinking cursor at the end of streaming text.
                    let on = (cx.tick / 6).is_multiple_of(2);
                    let cur = Span::styled(if on { "▍" } else { " " }, theme::fg(theme::PRIMARY));
                    match r.last_mut() {
                        Some(last) if text::line_width(&last.line) < inner => last.line.spans.push(cur),
                        _ => r.push(Row::new(Line::from(cur))),
                    }
                    if let Some(last) = r.last_mut() {
                        let e = text::line_width(&last.line).saturating_sub(1) as u16;
                        last.end = Some(e);
                    }
                }
                r
            }
            Part::Reasoning { text: t, time, time_end, .. } => {
                let active = time_end.is_none() && streaming;
                render_reasoning(m, idx, t, *time, *time_end, active, cx, inner)
            }
            Part::Tool { .. } => render_tool(m, idx, part, cx, inner),
            Part::Image { name, image } => vec![
                Row::new(Line::from(vec![
                    Span::styled(" image ", Style::new().fg(theme::PANEL).bg(theme::ACCENT).add_modifier(Modifier::BOLD)),
                    Span::styled(format!(" {}", name.clone().unwrap_or_else(|| image.mime.clone())), theme::muted()),
                ]))
                .decor(),
            ],
        };
        for r in part_rows {
            rows.push(if pad > 0 { r.indent(vec![Span::raw(" ".repeat(pad))]) } else { r });
        }
    }
    if m.parts.is_empty() && streaming {
        let mut spans = vec![Span::styled(format!("{} ", theme::spinner(cx.tick)), theme::fg(theme::PRIMARY))];
        spans.extend(shimmer("Waiting for response…", cx.tick, theme::MUTED, theme::TEXT));
        rows.push(Row::new(Line::from(spans)).decor().indent(vec![Span::raw(" ".repeat(pad))]));
    }
    // Error.
    let interrupted = m.error.as_deref().is_some_and(|e| e.eq_ignore_ascii_case("aborted"));
    if let Some(err) = m.error.as_deref().filter(|_| !interrupted) {
        if !rows.is_empty() {
            rows.push(Row::blank());
        }
        let clean = text::sanitize(err);
        for (li, logical) in clean.trim().split('\n').enumerate() {
            let content = if li == 0 { format!("Error: {logical}") } else { logical.to_string() };
            for (i, (l, sp)) in text::wrap_spans_flags(&[Span::styled(content, theme::fg(theme::ERROR))], inner.saturating_sub(2)).into_iter().enumerate() {
                rows.push(
                    Row::new(l)
                        .soft_sp(i > 0, sp)
                        .indent(vec![Span::raw(" ".repeat(pad)), Span::styled("┃ ", theme::fg(theme::ERROR))]),
                );
            }
        }
    }
    if cx.compact {
        return rows;
    }
    // Footer: agent · model · start → end (dur) · tokens        ⧉ copy
    if !rows.is_empty() {
        rows.push(Row::blank());
    }
    let agent = m.agent.clone().unwrap_or_else(|| cx.session_agent.to_string());
    let acolor = if m.error.is_some() && !interrupted { theme::MUTED } else { theme::agent_color(cx.agents, &agent) };
    let mut left = vec![
        Span::styled(if live { format!("{} ", theme::spinner(cx.tick)) } else { "▣ ".to_string() }, theme::fg(acolor)),
        Span::styled(titlecase(&agent), theme::fg(acolor)),
    ];
    if let Some(model) = &m.model {
        left.push(Span::styled(format!(" · {}", model_name(cx.models, model)), theme::muted()));
        if let Some(v) = &model.variant {
            left.push(Span::styled(format!(" {v}"), theme::fg(theme::WARNING)));
        }
    }
    let start = text::fmt_time(m.time);
    match m.time_end {
        Some(end) => {
            left.push(Span::styled(format!(" · {start} → {} ({})", text::fmt_clock(end), text::fmt_dur(end - m.time)), theme::muted()));
        }
        None => {
            left.push(Span::styled(format!(" · {start} · {}", text::fmt_elapsed(cx.now - m.time)), theme::muted()));
        }
    }
    if let Some(u) = &m.usage {
        let input = u.input + u.cache_read + u.cache_write;
        left.push(Span::styled(format!(" · ↑{} ↓{}", text::fmt_tokens(input), text::fmt_tokens(u.output)), theme::muted()));
        if u.output > 0
            && let Some(end) = m.time_end
            && end > m.time
        {
            let tps = u.output as f64 / ((end - m.time) as f64 / 1000.0);
            if tps.is_finite() && tps < 100_000.0 {
                left.push(Span::styled(format!(" · {tps:.0} tok/s"), theme::fg(theme::DIM)));
            }
        }
    }
    if interrupted {
        left.push(Span::styled(" · interrupted", theme::fg(theme::WARNING)));
    }
    let right = vec![copy_button(false)];
    let (line, rx) = compose(left, right, inner);
    let x0 = rx as u16;
    let foot = Row::new(line).zone(x0, x0 + 6, Hit::Copy { msg: m.id.clone() }).indent(vec![Span::raw(" ".repeat(pad))]).decor();
    rows.push(foot);
    rows
}

#[allow(clippy::too_many_arguments)]
fn render_reasoning(
    m: &Message,
    idx: usize,
    t: &str,
    time: i64,
    time_end: Option<i64>,
    active: bool,
    cx: &Ctx<'_>,
    w: usize,
) -> Vec<Row> {
    let exp = cx.expanded(idx, cx.thinking);
    let dur = match time_end {
        Some(e) if time > 0 => Some(text::fmt_dur(e - time)),
        None if active && time > 0 => Some(text::fmt_elapsed(cx.now - time)),
        _ => None,
    };
    let mut head: Vec<Span<'static>> = Vec::new();
    if cx.compact {
        head.push(Span::styled("✱ ", theme::fg(theme::DIM)));
    } else {
        head.push(Span::styled(if exp { "▾ " } else { "▸ " }, theme::fg(theme::DIM)));
    }
    if active {
        head.extend(shimmer("Thinking", cx.tick, theme::MUTED, theme::PRIMARY));
    } else {
        head.push(Span::styled("Thought", theme::fg(theme::MUTED).add_modifier(Modifier::ITALIC)));
    }
    if let Some(d) = dur {
        head.push(Span::styled(format!(" · {d}"), theme::fg(theme::DIM)));
    }
    let hw = spans_width(&head);
    let hit = Hit::Toggle { msg: m.id.clone(), part: idx };
    if exp && !cx.compact {
        let mut rows = vec![Row::new(Line::from(head)).decor().hit(hit)];
        let st = theme::muted().add_modifier(Modifier::ITALIC);
        for r in markdown::render(t, w.saturating_sub(2), st) {
            rows.push(r.indent(vec![Span::styled("│ ", theme::fg(theme::BORDER))]));
        }
        return rows;
    }
    // Collapsed: the LAST characters of the reasoning that fit on this same line.
    let flat = text::one_line(t);
    let avail = w.saturating_sub(hw + 2);
    let tail = text::tail_fit(&flat, avail);
    let mut spans = head;
    if !tail.is_empty() {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(tail, theme::fg(theme::DIM).add_modifier(Modifier::ITALIC)));
    }
    let row = Row::new(Line::from(spans)).hit(hit).skip((hw + 2) as u16);
    vec![row]
}

/// One-line summary detail for a tool's input.
pub fn tool_detail(name: &str, input: &Value) -> String {
    let s = |k: &str| input.get(k).and_then(Value::as_str).map(str::to_string);
    let base = name.rsplit('_').next().unwrap_or(name);
    let d = match name {
        "bash" => s("command").map(|c| format!("$ {}", c.lines().next().unwrap_or(""))),
        "read" | "write" | "delete" => s("path"),
        "edit" => s("path").map(|p| {
            let n = input.get("edits").and_then(Value::as_array).map(|a| a.len()).unwrap_or(0);
            if n > 1 { format!("{p} ({n} edits)") } else { p }
        }),
        "grep" => s("pattern").map(|p| match s("path") {
            Some(path) => format!("\"{p}\" in {path}"),
            None => format!("\"{p}\""),
        }),
        "find" => s("pattern").or_else(|| s("path")),
        "send_message" => s("to").map(|t| format!("→ {t}")),
        "create_session" => s("name").map(|n| format!("{n} ({})", s("agent").unwrap_or_default())),
        "read_session" | "close_session" | "context_window" => s("session"),
        "todo" => {
            let t = parse_todos(input);
            (!t.is_empty()).then(|| format!("{}/{} done", t.iter().filter(|x| x.status == TodoStatus::Completed).count(), t.len()))
        }
        _ => None,
    };
    if let Some(d) = d {
        return text::one_line(&d);
    }
    let _ = base;
    // Generic: primitive args `key=value`.
    let mut parts = Vec::new();
    if let Some(o) = input.as_object() {
        for (k, v) in o {
            let vs = match v {
                Value::String(x) => text::one_line(x),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                _ => continue,
            };
            parts.push(format!("{k}={}", text::truncate(&vs, 40)));
            if parts.len() >= 3 {
                break;
            }
        }
    }
    parts.join(" ")
}

/// Display width as the terminal buffer will lay it out (per grapheme, zero-width dropped).
fn cells_width(spans: &[Span<'_>]) -> usize {
    use unicode_segmentation::UnicodeSegmentation;
    spans.iter().flat_map(|s| s.content.graphemes(true)).map(text::gwidth).sum()
}

/// `╭─ label ───╮` / `├─ label ───┤`, exactly `bw` columns.
fn box_section(label: &str, first: bool, bw: usize) -> Row {
    let bst = theme::fg(theme::BORDER);
    let (l, r) = if first { ("╭", "╮") } else { ("├", "┤") };
    // l + "─ " + label + " " + fill + r
    let label = text::truncate(label, bw.saturating_sub(6));
    let lw = cells_width(&[Span::raw(label.as_str())]);
    let fill = bw.saturating_sub(5 + lw);
    Row::new(Line::from(vec![
        Span::styled(format!("{l}─ "), bst),
        Span::styled(label, theme::bold(theme::MUTED)),
        Span::styled(format!(" {}{r}", "─".repeat(fill)), bst),
    ]))
    .decor()
}

/// `│ text … │`: content padded to `iw` columns (box width = iw + 4).
fn box_body(spans: Vec<Span<'static>>, iw: usize, soft: bool) -> Row {
    let bst = theme::fg(theme::BORDER);
    let lw = cells_width(&spans);
    let mut s = vec![Span::styled("│ ", bst)];
    s.extend(spans);
    s.push(Span::raw(" ".repeat(iw.saturating_sub(lw))));
    s.push(Span::styled(" │", bst));
    Row::new(Line::from(s)).skip(2).end((2 + iw) as u16).soft(soft)
}

fn render_tool(m: &Message, idx: usize, part: &Part, cx: &Ctx<'_>, w: usize) -> Vec<Row> {
    let Part::Tool { name, purpose, input, raw_input, state, output, meta, images, time, time_end, run_at, .. } = part else {
        return Vec::new();
    };
    let exp = cx.expanded(idx, cx.details);
    let tick = cx.tick;
    let (icon, icolor) = match state {
        ToolState::Streaming | ToolState::Running => (theme::spinner(tick).to_string(), theme::PRIMARY),
        ToolState::AwaitingPermission => ("?".into(), theme::WARNING),
        ToolState::Delayed => ("◷".into(), theme::WARNING),
        ToolState::Done => ("✓".into(), theme::SUCCESS),
        ToolState::Error => ("✗".into(), theme::ERROR),
        ToolState::Denied | ToolState::Cancelled => ("⊘".into(), theme::MUTED),
    };
    let dim_state = matches!(state, ToolState::Denied | ToolState::Cancelled);
    let name_disp = if name.is_empty() { "tool".to_string() } else { name.clone() };
    let mut name_style = theme::bold(if dim_state { theme::MUTED } else { theme::TEXT });
    if *state == ToolState::Denied {
        name_style = name_style.add_modifier(Modifier::CROSSED_OUT);
    }
    let mut left: Vec<Span<'static>> = Vec::new();
    if !cx.compact {
        left.push(Span::styled(if exp { "▾ " } else { "▸ " }, theme::fg(theme::DIM)));
    }
    left.push(Span::styled(format!("{icon} "), theme::fg(icolor)));
    left.push(Span::styled(name_disp, name_style));
    let purpose = text::one_line(purpose);
    if !purpose.is_empty() {
        left.push(Span::styled(format!("  {purpose}"), if dim_state { theme::muted() } else { theme::text() }));
    }
    let detail = tool_detail(name, input);
    if !detail.is_empty() && !cx.compact {
        left.push(Span::styled(format!("  {detail}"), theme::fg(theme::DIM)));
    }
    // Right side: status + duration + start time.
    let mut right: Vec<Span<'static>> = Vec::new();
    match state {
        ToolState::Streaming => right.push(Span::styled("writing…", theme::muted())),
        ToolState::AwaitingPermission => right.push(Span::styled("awaiting permission", theme::fg(theme::WARNING))),
        ToolState::Delayed => {
            let secs = run_at.map(|r| ((r - cx.now).max(0) + 999) / 1000).unwrap_or(0);
            right.push(Span::styled(format!("runs in {secs}s · esc to cancel"), theme::fg(theme::WARNING)));
        }
        ToolState::Running => {
            right.push(Span::styled(format!("running {}", text::fmt_elapsed(cx.now - time)), theme::fg(theme::PRIMARY)))
        }
        ToolState::Done => {
            if let Some(e) = time_end {
                right.push(Span::styled(text::fmt_dur(e - time), theme::muted()));
            }
        }
        ToolState::Error => {
            right.push(Span::styled("error", theme::fg(theme::ERROR)));
            if let Some(e) = time_end {
                right.push(Span::styled(format!(" {}", text::fmt_dur(e - time)), theme::muted()));
            }
        }
        ToolState::Denied => right.push(Span::styled("denied", theme::muted())),
        ToolState::Cancelled => right.push(Span::styled("cancelled", theme::muted())),
    }
    if *time > 0 && !cx.compact {
        right.push(Span::styled(format!("  {}", text::fmt_clock(*time)), theme::fg(theme::DIM)));
    }
    if cx.compact && w < 50 {
        right.clear();
    }
    let (line, _) = compose(left, right, w);
    let hit = Hit::Toggle { msg: m.id.clone(), part: idx };
    let mut rows = vec![Row::new(line).hit(hit.clone()).decor()];
    // Todo lists are always shown inline (like opencode's todo block).
    if name == "todo" {
        let todos = parse_todos(input);
        let max = if cx.compact { 8 } else { usize::MAX };
        for r in todo_rows(&todos, w.saturating_sub(2), max) {
            rows.push(r.hit(hit.clone()).indent(vec![Span::raw("  ")]));
        }
    }

    let out_text = output.as_deref().map(text::sanitize).unwrap_or_default();
    if !exp || cx.compact {
        // Errors: show the first output line under the collapsed row.
        if *state == ToolState::Error
            && let Some(first) = out_text.lines().map(str::trim).find(|l| !l.is_empty())
        {
            let l = Line::from(vec![
                Span::styled("  └ ", theme::fg(theme::DIM)),
                Span::styled(text::truncate(first, w.saturating_sub(4)), theme::fg(theme::ERROR)),
            ]);
            rows.push(Row::new(l).skip(4));
        }
        return rows;
    }

    // Expanded block.
    let bw = w.saturating_sub(2).max(10); // box total width
    let iw = bw.saturating_sub(4).max(4); // inner text width
    let bst = theme::fg(theme::BORDER);
    // Every row is exactly `bw` columns wide so the right borders line up.
    let section = |label: &str, first: bool| -> Row { box_section(label, first, bw) };
    let body = |spans: Vec<Span<'static>>, soft: bool| -> Row { box_body(spans, iw, soft) };
    let push_text = |rows: &mut Vec<Row>, t: &str, style: Style, limit: Option<usize>| -> usize {
        let lines: Vec<&str> = t.split('\n').collect();
        let n = lines.len();
        let shown = limit.map(|l| l.min(n)).unwrap_or(n);
        for l in &lines[..shown] {
            for (i, wl) in text::wrap_hard(&[Span::styled(l.to_string(), style)], iw).into_iter().enumerate() {
                rows.push(body(wl.spans, i > 0));
            }
        }
        n - shown
    };

    let mut blk: Vec<Row> = Vec::new();
    blk.push(section("input", true));
    let input_text = if input.is_null() || (input.as_object().is_some_and(|o| o.is_empty()) && !raw_input.trim().is_empty() && raw_input.trim() != "{}") {
        raw_input.clone()
    } else {
        serde_json::to_string_pretty(input).unwrap_or_default()
    };
    let input_text = text::sanitize(&input_text);
    let in_hidden = push_text(&mut blk, input_text.trim_end(), theme::fg(theme::INFO), if cx.all(idx) { None } else { Some(OUTPUT_PREVIEW_LINES) });
    if in_hidden > 0 {
        blk.push(body(vec![Span::styled(format!("… {in_hidden} more lines (click to show all)"), theme::fg(theme::WARNING))], false).hit(Hit::ShowAll { msg: m.id.clone(), part: idx }).decor());
    }
    // Diff (edit/write).
    if let Some(diff) = meta.as_ref().and_then(|x| x.get("diff")).and_then(Value::as_str).filter(|d| !d.trim().is_empty()) {
        blk.push(section("diff", false));
        let diff = text::sanitize(diff);
        let lines: Vec<&str> = diff.trim_end().split('\n').collect();
        let limit = if cx.all(idx) { lines.len() } else { lines.len().min(OUTPUT_PREVIEW_LINES) };
        for l in &lines[..limit] {
            let st = if l.starts_with("+++") || l.starts_with("---") {
                theme::bold(theme::MUTED)
            } else if l.starts_with('+') {
                Style::new().fg(theme::DIFF_ADDED).bg(theme::DIFF_ADDED_BG)
            } else if l.starts_with('-') {
                Style::new().fg(theme::DIFF_REMOVED).bg(theme::DIFF_REMOVED_BG)
            } else if l.starts_with("@@") {
                theme::fg(theme::INFO)
            } else {
                theme::fg(theme::DIFF_CONTEXT)
            };
            for (i, wl) in text::wrap_hard(&[Span::styled(l.to_string(), st)], iw).into_iter().enumerate() {
                let lw = text::line_width(&wl);
                let mut spans = wl.spans;
                if st.bg.is_some() {
                    spans.push(Span::styled(" ".repeat(iw.saturating_sub(lw)), st));
                }
                blk.push(body(spans, i > 0));
            }
        }
        if limit < lines.len() {
            blk.push(body(vec![Span::styled(format!("… {} more lines (click to show all)", lines.len() - limit), theme::fg(theme::WARNING))], false).hit(Hit::ShowAll { msg: m.id.clone(), part: idx }).decor());
        }
    }
    // Output (or live output while running).
    let live_out = meta.as_ref().and_then(|x| x.get("live_output")).and_then(Value::as_str).map(text::sanitize);
    let (label, out) = match (state, &live_out) {
        (ToolState::Running, Some(lo)) => ("output (live)", lo.clone()),
        _ => ("output", out_text.clone()),
    };
    if !out.is_empty() || state.is_final() {
        blk.push(section(label, false));
        let st = if *state == ToolState::Error { theme::fg(theme::ERROR) } else { theme::text() };
        if out.trim().is_empty() {
            blk.push(body(vec![Span::styled("(no output)", theme::muted())], false).decor());
        } else if *state == ToolState::Running {
            // Live tail.
            let lines: Vec<&str> = out.trim_end().split('\n').collect();
            let start = lines.len().saturating_sub(20);
            if start > 0 {
                blk.push(body(vec![Span::styled(format!("… {start} earlier lines"), theme::muted())], false).decor());
            }
            push_text(&mut blk, &lines[start..].join("\n"), st, None);
        } else {
            let hidden = push_text(&mut blk, out.trim_end(), st, if cx.all(idx) { None } else { Some(OUTPUT_PREVIEW_LINES) });
            if hidden > 0 {
                blk.push(
                    body(vec![Span::styled(format!("… {hidden} more lines (click to show all)"), theme::fg(theme::WARNING))], false)
                        .hit(Hit::ShowAll { msg: m.id.clone(), part: idx })
                        .decor(),
                );
            }
        }
    }
    if !images.is_empty() {
        blk.push(body(vec![Span::styled(format!("[{} image(s) attached]", images.len()), theme::fg(theme::ACCENT))], false).decor());
    }
    // Extra meta (exit code etc.), excluding big fields.
    if let Some(obj) = meta.as_ref().and_then(Value::as_object) {
        let mut bits = Vec::new();
        for (k, v) in obj {
            if matches!(k.as_str(), "diff" | "live_output") {
                continue;
            }
            let vs = match v {
                Value::Null => continue,
                Value::String(s) => text::truncate(&text::one_line(s), 60),
                Value::Bool(_) | Value::Number(_) => v.to_string(),
                _ => continue,
            };
            bits.push(format!("{k}: {vs}"));
        }
        if !bits.is_empty() {
            blk.push(section("meta", false));
            let joined = bits.join("  ·  ");
            for (i, wl) in text::wrap_spans(&[Span::styled(joined, theme::muted())], iw).into_iter().enumerate() {
                blk.push(body(wl.spans, i > 0));
            }
        }
    }
    blk.push(Row::new(Line::from(Span::styled(format!("╰{}╯", "─".repeat(bw.saturating_sub(2))), bst))).decor());
    for r in blk {
        rows.push(r.indent(vec![Span::raw("  ")]));
    }
    rows
}
