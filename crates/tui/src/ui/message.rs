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
use crate::util::ABORTED_ERROR;

/// Max output lines shown in an expanded tool block before "show all".
const OUTPUT_PREVIEW_LINES: usize = 200;

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
    fn shows_full_output(&self, idx: usize) -> bool {
        self.show_all.is_some_and(|t| t.contains(&idx))
    }
}

/// Ticks of a shimmer sweep spent off the label, and where the sweep starts relative to it.
const SHIMMER_PAD: i64 = 8;
const SHIMMER_LEAD_IN: i64 = 4;
/// Distance in characters over which the highlight fades out.
const SHIMMER_RADIUS: f32 = 3.0;
/// Rows are never laid out narrower than this.
const MIN_RENDER_WIDTH: usize = 20;

/// Columns a user message needs besides its text: bar, two spaces and a margin.
const USER_BOX_CHROME: usize = 5;
/// Columns left of the text in a user bubble: bar and two spaces.
const BUBBLE_INSET: usize = 3;
/// Longest image name shown in a badge.
const IMAGE_NAME_MAX: usize = 30;
/// Most lines of a user message shown in a split tile.
const COMPACT_USER_LINES: usize = 6;

/// Narrowest width of assistant content and of an expanded tool block.
const MIN_BLOCK_WIDTH: usize = 10;
/// Narrowest text inside an expanded tool block.
const MIN_BLOCK_INNER_WIDTH: usize = 4;
/// Split tiles narrower than this hide the tool status text.
const COMPACT_HIDE_STATUS_BELOW: usize = 50;
/// Most todo rows shown in a split tile.
const COMPACT_TODO_ROWS: usize = 8;
/// Last lines of a running tool's live output that are shown.
const LIVE_TAIL_LINES: usize = 20;
/// Ticks per phase of the blinking cursor.
const CURSOR_BLINK_TICKS: u64 = 6;
/// Faster throughputs are not shown (the duration is too short to be meaningful).
const MAX_DISPLAYED_TPS: f64 = 100_000.0;
/// A generic tool summary shows at most this many `key=value` args, each value this long.
const GENERIC_ARG_MAX: usize = 3;
const GENERIC_ARG_VALUE_MAX: usize = 40;
/// Longest value shown in the meta section of an expanded tool block.
const META_VALUE_MAX: usize = 60;

/// Left padding of assistant content.
const PAD: usize = 2;

pub fn render(m: &Message, cx: &Ctx<'_>, w: usize) -> Vec<Row> {
    let w = w.max(MIN_RENDER_WIDTH);
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
const STALE_DIM: f32 = 0.3;

/// Fades foreground colours (backgrounds and click zones are kept).
fn dim_rows(rows: &mut [Row]) {
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
pub(crate) const CONTEXT_HINT: &str =
    "messages above stay visible but are no longer sent to the model";

/// Full-width `──── ◇ Context compacted · 14:02:11 · click to view summary ──── ⧉ copy` row.
fn compaction_divider(m: &Message, expanded: bool, w: usize) -> Row {
    let total = w.saturating_sub(1).max(MIN_RENDER_WIDTH);
    let border_style = theme::fg(theme::BORDER);
    let time = text::fmt_time(m.time);
    let right = vec![Span::raw(" "), copy_button()];
    let rw = text::spans_width(&right);
    let head = |hint: bool| {
        let mut v = vec![
            Span::styled(if expanded { "▾ " } else { "▸ " }, theme::fg(theme::DIM)),
            Span::styled("──── ", border_style),
            Span::styled("◇ ", theme::fg(theme::ACCENT)),
            Span::styled("Context compacted", theme::bold(theme::ACCENT)),
            Span::styled(format!(" · {time}"), theme::muted()),
        ];
        if hint {
            v.push(Span::styled(
                if expanded {
                    " · click to hide summary"
                } else {
                    " · click to view summary"
                },
                theme::muted(),
            ));
        }
        v.push(Span::raw(" "));
        v
    };
    let mut left = head(true);
    if text::spans_width(&left) + rw + 4 > total {
        left = head(false);
    }
    let left = truncate_spans(left, total.saturating_sub(rw));
    let fill = total.saturating_sub(text::spans_width(&left) + rw);
    let mut spans = left;
    spans.push(Span::styled("─".repeat(fill), border_style));
    spans.extend(right);
    let x0 = total as u16 - copy_width();
    Row::new(Line::from(spans))
        .decor()
        .zone(x0, x0 + copy_width(), Hit::Copy { msg: m.id.clone() })
        .hit(Hit::Toggle {
            msg: m.id.clone(),
            part: 0,
        })
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Builds `left … right` within `w` columns, truncating `left` as needed.
pub fn compose(
    left: Vec<Span<'static>>,
    right: Vec<Span<'static>>,
    w: usize,
) -> (Line<'static>, usize) {
    let rw: usize = text::spans_width(&right);
    let avail = w.saturating_sub(rw + if rw > 0 { 1 } else { 0 });
    let mut l = truncate_spans(left, avail);
    let lw: usize = text::spans_width(&l);
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
    let total: usize = text::spans_width(&spans);
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

fn copy_button() -> Span<'static> {
    Span::styled(COPY_LABEL, theme::fg(theme::DIM))
}

/// Width of the copy button, for its click zone.
fn copy_width() -> u16 {
    width(COPY_LABEL) as u16
}

const COPY_LABEL: &str = "⧉ copy";
const UNDO_LABEL: &str = "↶ undo";
/// Gap between buttons in the meta row.
const BUTTON_GAP: &str = "  ";
/// Button on a queued prompt row (removes it from the queue).
pub const UNQUEUE_LABEL: &str = "✕ remove";

fn undo_button() -> Span<'static> {
    Span::styled(UNDO_LABEL, theme::fg(theme::DIM))
}

/// Shimmer: a soft highlight sweeping over `label`.
pub fn shimmer(label: &str, tick: u64, base: Color, hi: Color) -> Vec<Span<'static>> {
    let chars: Vec<char> = label.chars().collect();
    let n = chars.len() as i64;
    let period = n + SHIMMER_PAD;
    let pos = (tick as i64 % period) - SHIMMER_LEAD_IN;
    chars
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let d = (i as i64 - pos).abs() as f32;
            let t = (1.0 - d / SHIMMER_RADIUS).max(0.0);
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
    if cx.compact {
        return render_user_compact(m, w, bar_color);
    }
    let raw = m.raw_text();
    if m.source == MessageSource::Compaction {
        return compaction_rows(m, cx, &raw, w);
    }

    let bg = theme::PANEL;
    let bubble = Bubble {
        bar: Span::styled("┃", Style::new().fg(bar_color).bg(bg)),
        bg,
        w,
    };
    let inner = w.saturating_sub(USER_BOX_CHROME).max(1);
    let mut rows = vec![bubble.wrap(Row::new(user_header(&m.source, bg))).decor()];
    rows.extend(user_body_rows(m, &raw, inner, &bubble));
    rows.extend(image_badge_row(m, inner, &bubble));
    rows.push(user_meta_row(m, inner, &bubble));
    rows
}

/// Compaction: full-width divider (expandable summary below it).
fn compaction_rows(m: &Message, cx: &Ctx<'_>, raw: &str, w: usize) -> Vec<Row> {
    let expanded = cx.expanded(0, false);
    let mut rows = vec![compaction_divider(m, expanded, w)];
    if cx.context_start {
        let hint = text::truncate(CONTEXT_HINT, w.saturating_sub(4));
        let pad = w.saturating_sub(1 + width(&hint)) / 2;
        rows.push(
            Row::new(Line::from(vec![
                Span::raw(" ".repeat(pad)),
                Span::styled(hint, theme::fg(theme::DIM).add_modifier(Modifier::ITALIC)),
            ]))
            .decor(),
        );
    }
    if expanded {
        for r in markdown::render(raw, w.saturating_sub(PAD + 4), theme::muted()) {
            rows.push(r.indent(vec![
                Span::raw(" ".repeat(PAD)),
                Span::styled("│ ", theme::fg(theme::ACCENT)),
                Span::raw(" "),
            ]));
        }
    }
    rows
}

/// The coloured bar and background that frame the rows of a user message.
struct Bubble {
    bar: Span<'static>,
    bg: Color,
    /// Width of the whole message.
    w: usize,
}

impl Bubble {
    /// Indents `r` behind the bar and fills the rest of the row with the background.
    fn wrap(&self, r: Row) -> Row {
        let lw = text::line_width(&r.line);
        let bg = Style::new().bg(self.bg);
        let mut r = r.indent(vec![self.bar.clone(), Span::styled("  ", bg)]);
        r.line.spans.push(Span::styled(
            " ".repeat(self.w.saturating_sub(lw + BUBBLE_INSET)),
            bg,
        ));
        if r.end.is_none() {
            r.end = Some((BUBBLE_INSET + lw) as u16);
        }
        r
    }
}

/// Header line of a message that came from somewhere else; empty for the user's own.
fn user_header(source: &MessageSource, bg: Color) -> Line<'static> {
    let header = match source {
        MessageSource::Human | MessageSource::Compaction => return Line::default(),
        MessageSource::Peer { from_name, .. } => Span::styled(
            format!("↘ message from \"{from_name}\""),
            theme::bold(theme::INFO).bg(bg),
        ),
        MessageSource::Spawn { from_name, .. } => Span::styled(
            format!("↘ created by \"{from_name}\""),
            theme::bold(theme::INFO).bg(bg),
        ),
        MessageSource::Reminder => Span::styled("⟳ reminder", theme::bold(theme::WARNING).bg(bg)),
        MessageSource::Shell => Span::styled("$ shell", theme::bold(theme::WARNING).bg(bg)),
    };
    Line::from(header)
}

/// The text of a user message (markdown for shell output), one bubble row per wrapped line.
fn user_body_rows(m: &Message, raw: &str, inner: usize, bubble: &Bubble) -> Vec<Row> {
    let bg = bubble.bg;
    let body_style = Style::new().fg(theme::TEXT).bg(bg);
    let mut rows = Vec::new();
    if m.source == MessageSource::Shell {
        for mut r in markdown::render(raw, inner, body_style) {
            for s in &mut r.line.spans {
                if s.style.bg.is_none() {
                    s.style = s.style.bg(bg);
                }
            }
            rows.push(bubble.wrap(r));
        }
        return rows;
    }
    let clean = text::sanitize(raw);
    let clean = clean.trim_matches('\n');
    let has_image = m.parts.iter().any(|p| matches!(p, Part::Image { .. }));
    if clean.is_empty() {
        if !has_image {
            let empty = Span::styled("(empty)", theme::muted().bg(bg));
            rows.push(bubble.wrap(Row::new(Line::from(empty))));
        }
        return rows;
    }
    for logical in clean.split('\n') {
        let wrapped =
            text::wrap_spans_flags(&[Span::styled(logical.to_string(), body_style)], inner);
        for (i, (l, sp)) in wrapped.into_iter().enumerate() {
            rows.push(bubble.wrap(Row::new(l).soft_sp(i > 0, sp)));
        }
    }
    rows
}

/// One row of `image name` badges for the attached images, if any.
fn image_badge_row(m: &Message, inner: usize, bubble: &Bubble) -> Option<Row> {
    let bg = bubble.bg;
    let images: Vec<String> = m
        .parts
        .iter()
        .filter_map(|p| match p {
            Part::Image { name, image } => Some(name.clone().unwrap_or_else(|| image.mime.clone())),
            _ => None,
        })
        .collect();
    if images.is_empty() {
        return None;
    }
    let mut spans = Vec::new();
    for (i, n) in images.iter().enumerate() {
        spans.push(Span::styled(
            " image ",
            Style::new()
                .fg(bg)
                .bg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(" {} ", text::truncate(n, IMAGE_NAME_MAX)),
            Style::new().fg(theme::MUTED).bg(theme::PANEL_HOVER),
        ));
        if i + 1 < images.len() {
            spans.push(Span::styled(" ", Style::new().bg(bg)));
        }
    }
    Some(bubble.wrap(Row::new(Line::from(truncate_spans(spans, inner))).decor()))
}

/// Meta row: time + [undo] + copy, right aligned.
fn user_meta_row(m: &Message, inner: usize, bubble: &Bubble) -> Row {
    let bg = bubble.bg;
    let time = text::fmt_time(m.time);
    let gap = || Span::styled(BUTTON_GAP, Style::new().bg(bg));
    let human = m.source == MessageSource::Human;
    let mut right = vec![Span::styled(time.clone(), theme::muted().bg(bg)), gap()];
    if human {
        right.push(undo_button());
        right.push(gap());
    }
    right.push(copy_button());
    // One column wider than the body so the button lines up with assistant footers / tool rows.
    let (line, rx) = compose(vec![], right, inner + 1);
    let mut x0 = (rx + width(&time) + width(BUTTON_GAP)) as u16;
    let mut row = Row::new(line);
    if human {
        let uw = width(UNDO_LABEL) as u16;
        row = row.zone(x0, x0 + uw, Hit::Undo { msg: m.id.clone() });
        x0 += uw + width(BUTTON_GAP) as u16;
    }
    let row = row.zone(x0, x0 + copy_width(), Hit::Copy { msg: m.id.clone() });
    let mut meta = bubble.wrap(row).decor();
    for s in &mut meta.line.spans {
        if s.style.bg.is_none() {
            s.style = s.style.bg(bg);
        }
    }
    meta
}

/// Wraps `text` into rows of `wrap_width` columns, each led by `lead_pad` spaces and a `┃ ` bar.
pub fn bar_rows(
    text: &str,
    style: Style,
    bar: Color,
    wrap_width: usize,
    lead_pad: usize,
) -> Vec<Row> {
    text::wrap_spans_flags(&[Span::styled(text.to_string(), style)], wrap_width)
        .into_iter()
        .enumerate()
        .map(|(i, (line, soft_space))| {
            let mut lead = Vec::new();
            if lead_pad > 0 {
                lead.push(Span::raw(" ".repeat(lead_pad)));
            }
            lead.push(Span::styled("┃ ", theme::fg(bar)));
            Row::new(line).soft_sp(i > 0, soft_space).indent(lead)
        })
        .collect()
}

fn render_user_compact(m: &Message, w: usize, color: Color) -> Vec<Row> {
    if m.source == MessageSource::Compaction {
        // Compact divider: `── ◇ compacted 14:02:11 ─────`.
        let border_style = theme::fg(theme::BORDER);
        let left = truncate_spans(
            vec![
                Span::styled("── ", border_style),
                Span::styled("◇ ", theme::fg(theme::ACCENT)),
                Span::styled("compacted", theme::bold(theme::ACCENT)),
                Span::styled(format!(" {} ", text::fmt_clock(m.time)), theme::muted()),
            ],
            w,
        );
        let fill = w.saturating_sub(text::spans_width(&left));
        let mut spans = left;
        spans.push(Span::styled("─".repeat(fill), border_style));
        return vec![Row::new(Line::from(spans)).decor()];
    }
    let label = match &m.source {
        MessageSource::Human => "you".to_string(),
        MessageSource::Peer { from_name, .. } => format!("from {from_name}"),
        MessageSource::Spawn { from_name, .. } => format!("by {from_name}"),
        MessageSource::Reminder => "reminder".into(),
        MessageSource::Compaction => unreachable!("returned above"),
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
    for l in lines.iter().take(COMPACT_USER_LINES) {
        rows.extend(bar_rows(l, theme::text(), color, w.saturating_sub(2), 0));
    }
    if lines.len() > COMPACT_USER_LINES {
        rows.push(
            Row::new(Line::from(Span::styled(
                format!(
                    "┃ {} {} more lines",
                    text::ELLIPSIS,
                    lines.len() - COMPACT_USER_LINES
                ),
                theme::muted(),
            )))
            .decor(),
        );
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
        Part::Tool { name, input, .. } if name == "todo" && !parse_todos(input).is_empty() => {
            (false, false)
        }
        Part::Tool { .. } => (true, !cx.expanded(idx, cx.details)),
        Part::Reasoning { .. } => (true, cx.compact || !cx.expanded(idx, cx.thinking)),
        _ => (false, false),
    }
}

/// The message is still being produced.
fn is_streaming(m: &Message) -> bool {
    m.time_end.is_none() && m.error.is_none()
}

fn render_assistant(m: &Message, cx: &Ctx<'_>, w: usize) -> Vec<Row> {
    let streaming = is_streaming(m);
    let mut rows: Vec<Row> = Vec::new();
    let mut prev_single: Option<bool> = None;
    let last_idx = m.parts.len().saturating_sub(1);
    let pad = if cx.compact { 0 } else { PAD };
    let inner = w
        .saturating_sub(pad + if cx.compact { 0 } else { 1 })
        .max(MIN_BLOCK_WIDTH);
    for (idx, part) in m.parts.iter().enumerate() {
        let is_streaming_tail = streaming && idx == last_idx;
        // Skip empty text parts entirely (except while they're the streaming tail).
        if let Part::Text { text: t, .. } | Part::Reasoning { text: t, .. } = part
            && t.trim().is_empty()
            && !is_streaming_tail
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
                if is_streaming_tail {
                    append_cursor(&mut r, cx.tick, inner);
                }
                r
            }
            Part::Reasoning { .. } => render_reasoning(m, idx, part, cx, inner),
            Part::Tool {
                name,
                purpose,
                input,
                raw_input,
                state,
                output,
                meta,
                images,
                time,
                time_end,
                run_at,
                ..
            } => {
                let call = ToolCall {
                    name,
                    purpose,
                    input,
                    raw_input,
                    state: *state,
                    output: output.as_deref(),
                    meta: meta.as_ref(),
                    image_count: images.len(),
                    time: *time,
                    time_end: *time_end,
                    run_at: *run_at,
                };
                render_tool(m, idx, &call, cx, inner)
            }
            Part::Image { name, image } => vec![
                Row::new(Line::from(vec![
                    Span::styled(
                        " image ",
                        Style::new()
                            .fg(theme::PANEL)
                            .bg(theme::ACCENT)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(" {}", name.clone().unwrap_or_else(|| image.mime.clone())),
                        theme::muted(),
                    ),
                ]))
                .decor(),
            ],
        };
        for r in part_rows {
            rows.push(if pad > 0 {
                r.indent(vec![Span::raw(" ".repeat(pad))])
            } else {
                r
            });
        }
    }
    if m.parts.is_empty() && streaming {
        let mut spans = vec![Span::styled(
            format!("{} ", theme::spinner(cx.tick)),
            theme::fg(theme::PRIMARY),
        )];
        spans.extend(shimmer(
            "Waiting for response…",
            cx.tick,
            theme::MUTED,
            theme::TEXT,
        ));
        rows.push(
            Row::new(Line::from(spans))
                .decor()
                .indent(vec![Span::raw(" ".repeat(pad))]),
        );
    }

    let interrupted = m
        .error
        .as_deref()
        .is_some_and(|e| e.eq_ignore_ascii_case(ABORTED_ERROR));
    if let Some(err) = m.error.as_deref().filter(|_| !interrupted) {
        if !rows.is_empty() {
            rows.push(Row::blank());
        }
        let clean = text::sanitize(err);
        for (li, logical) in clean.trim().split('\n').enumerate() {
            let content = if li == 0 {
                format!("Error: {logical}")
            } else {
                logical.to_string()
            };
            rows.extend(bar_rows(
                &content,
                theme::fg(theme::ERROR),
                theme::ERROR,
                inner.saturating_sub(2),
                pad,
            ));
        }
    }
    if cx.compact {
        return rows;
    }

    if !rows.is_empty() {
        rows.push(Row::blank());
    }
    rows.push(assistant_footer(m, cx, inner, pad, interrupted));
    rows
}

/// Blinking cursor at the end of streaming text.
fn append_cursor(rows: &mut Vec<Row>, tick: u64, inner: usize) {
    let on = (tick / CURSOR_BLINK_TICKS).is_multiple_of(2);
    let cur = Span::styled(if on { "▍" } else { " " }, theme::fg(theme::PRIMARY));
    match rows.last_mut() {
        Some(last) if text::line_width(&last.line) < inner => last.line.spans.push(cur),
        _ => rows.push(Row::new(Line::from(cur))),
    }
    if let Some(last) = rows.last_mut() {
        let e = text::line_width(&last.line).saturating_sub(1) as u16;
        last.end = Some(e);
    }
}

/// Footer: agent · model · start → end (dur) · tokens        ⧉ copy
fn assistant_footer(m: &Message, cx: &Ctx<'_>, inner: usize, pad: usize, interrupted: bool) -> Row {
    let live = crate::state::message_live(m);
    let agent = m
        .agent
        .clone()
        .unwrap_or_else(|| cx.session_agent.to_string());
    let acolor = if m.error.is_some() && !interrupted {
        theme::MUTED
    } else {
        theme::agent_color(cx.agents, &agent)
    };
    let mut left = vec![
        Span::styled(
            if live {
                format!("{} ", theme::spinner(cx.tick))
            } else {
                "▣ ".to_string()
            },
            theme::fg(acolor),
        ),
        Span::styled(titlecase(&agent), theme::fg(acolor)),
    ];
    if let Some(model) = &m.model {
        left.push(Span::styled(
            format!(" · {}", model_name(cx.models, model)),
            theme::muted(),
        ));
        if let Some(v) = &model.variant {
            left.push(Span::styled(format!(" {v}"), theme::fg(theme::WARNING)));
        }
    }

    let start = text::fmt_time(m.time);
    match m.time_end {
        Some(end) => {
            left.push(Span::styled(
                format!(
                    " · {start} → {} ({})",
                    text::fmt_clock(end),
                    text::fmt_dur(end - m.time)
                ),
                theme::muted(),
            ));
        }
        None => {
            left.push(Span::styled(
                format!(" · {start} · {}", text::fmt_elapsed(cx.now - m.time)),
                theme::muted(),
            ));
        }
    }
    if let Some(u) = &m.usage {
        let input = u.input + u.cache_read + u.cache_write;
        left.push(Span::styled(
            format!(
                " · ↑{} ↓{}",
                text::fmt_tokens(input),
                text::fmt_tokens(u.output)
            ),
            theme::muted(),
        ));
        if u.output > 0
            && let Some(end) = m.time_end
            && end > m.time
        {
            let secs = (end - m.time) as f64 / text::MS_PER_SEC as f64;
            let tokens_per_sec = u.output as f64 / secs;
            if tokens_per_sec < MAX_DISPLAYED_TPS {
                left.push(Span::styled(
                    format!(" · {tokens_per_sec:.0} tok/s"),
                    theme::fg(theme::DIM),
                ));
            }
        }
    }
    if interrupted {
        left.push(Span::styled(" · interrupted", theme::fg(theme::WARNING)));
    }

    let right = vec![copy_button()];
    let (line, rx) = compose(left, right, inner);
    let x0 = rx as u16;
    Row::new(line)
        .zone(x0, x0 + copy_width(), Hit::Copy { msg: m.id.clone() })
        .indent(vec![Span::raw(" ".repeat(pad))])
        .decor()
}

fn render_reasoning(m: &Message, idx: usize, part: &Part, cx: &Ctx<'_>, w: usize) -> Vec<Row> {
    let Part::Reasoning {
        text: t,
        time,
        time_end,
        ..
    } = part
    else {
        return Vec::new();
    };
    let active = time_end.is_none() && is_streaming(m);
    let time = *time;
    let expanded = cx.expanded(idx, cx.thinking);
    let dur = match time_end {
        Some(e) if time > 0 => Some(text::fmt_dur(e - time)),
        None if active && time > 0 => Some(text::fmt_elapsed(cx.now - time)),
        _ => None,
    };
    let mut head: Vec<Span<'static>> = Vec::new();
    if cx.compact {
        head.push(Span::styled("✱ ", theme::fg(theme::DIM)));
    } else {
        head.push(Span::styled(
            if expanded { "▾ " } else { "▸ " },
            theme::fg(theme::DIM),
        ));
    }
    if active {
        head.extend(shimmer("Thinking", cx.tick, theme::MUTED, theme::PRIMARY));
    } else {
        head.push(Span::styled(
            "Thought",
            theme::fg(theme::MUTED).add_modifier(Modifier::ITALIC),
        ));
    }
    if let Some(d) = dur {
        head.push(Span::styled(format!(" · {d}"), theme::fg(theme::DIM)));
    }
    let hw = text::spans_width(&head);
    let hit = Hit::Toggle {
        msg: m.id.clone(),
        part: idx,
    };
    if expanded && !cx.compact {
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
        spans.push(Span::styled(
            tail,
            theme::fg(theme::DIM).add_modifier(Modifier::ITALIC),
        ));
    }
    let row = Row::new(Line::from(spans)).hit(hit).skip((hw + 2) as u16);
    vec![row]
}

/// One-line summary detail for a tool's input.
fn tool_detail(name: &str, input: &Value) -> String {
    let string_arg = |k: &str| input.get(k).and_then(Value::as_str).map(str::to_string);

    let d = match name {
        "bash" => string_arg("command").map(|c| format!("$ {}", c.lines().next().unwrap_or(""))),
        "read" | "write" | "delete" => string_arg("path"),
        "edit" => string_arg("path").map(|p| {
            let n = input
                .get("edits")
                .and_then(Value::as_array)
                .map(|a| a.len())
                .unwrap_or(0);
            if n > 1 { format!("{p} ({n} edits)") } else { p }
        }),
        "grep" => string_arg("pattern").map(|p| match string_arg("path") {
            Some(path) => format!("\"{p}\" in {path}"),
            None => format!("\"{p}\""),
        }),
        "find" => string_arg("pattern").or_else(|| string_arg("path")),
        "webfetch" => {
            string_arg("url").map(|u| match string_arg("format").filter(|f| f != "markdown") {
                Some(f) => format!("{u} ({f})"),
                None => u,
            })
        }
        "send_message" => string_arg("to").map(|t| format!("→ {t}")),
        "create_session" => {
            string_arg("name").map(|n| format!("{n} ({})", string_arg("agent").unwrap_or_default()))
        }
        "read_session" | "close_session" | "context_window" => string_arg("session"),
        "todo" => {
            let t = parse_todos(input);
            (!t.is_empty()).then(|| {
                format!(
                    "{}/{} done",
                    t.iter()
                        .filter(|x| x.status == TodoStatus::Completed)
                        .count(),
                    t.len()
                )
            })
        }
        _ => None,
    };
    if let Some(d) = d {
        return text::one_line(&d);
    }
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
            parts.push(format!(
                "{k}={}",
                text::truncate(&vs, GENERIC_ARG_VALUE_MAX)
            ));
            if parts.len() >= GENERIC_ARG_MAX {
                break;
            }
        }
    }
    parts.join(" ")
}

/// Display width as the terminal buffer will lay it out (per grapheme, zero-width dropped).
fn cells_width(spans: &[Span<'_>]) -> usize {
    use unicode_segmentation::UnicodeSegmentation;
    spans
        .iter()
        .flat_map(|s| s.content.graphemes(true))
        .map(text::gwidth)
        .sum()
}

/// `╭─ label ───╮` / `├─ label ───┤`, exactly `bw` columns.
fn box_section(label: &str, first: bool, bw: usize) -> Row {
    let border_style = theme::fg(theme::BORDER);
    let (l, r) = if first {
        ("╭", "╮")
    } else {
        ("├", "┤")
    };
    // l + "─ " + label + " " + fill + r
    let label = text::truncate(label, bw.saturating_sub(6));
    let lw = cells_width(&[Span::raw(label.as_str())]);
    let fill = bw.saturating_sub(5 + lw);
    Row::new(Line::from(vec![
        Span::styled(format!("{l}─ "), border_style),
        Span::styled(label, theme::bold(theme::MUTED)),
        Span::styled(format!(" {}{r}", "─".repeat(fill)), border_style),
    ]))
    .decor()
}

/// `│ text … │`: content padded to `iw` columns (box width = iw + 4).
fn box_body(spans: Vec<Span<'static>>, iw: usize, soft: bool) -> Row {
    let border_style = theme::fg(theme::BORDER);
    let lw = cells_width(&spans);
    let mut s = vec![Span::styled("│ ", border_style)];
    s.extend(spans);
    s.push(Span::raw(" ".repeat(iw.saturating_sub(lw))));
    s.push(Span::styled(" │", border_style));
    Row::new(Line::from(s))
        .skip(2)
        .end((2 + iw) as u16)
        .soft(soft)
}

/// The fields of a `Part::Tool` that `render_tool` reads.
struct ToolCall<'a> {
    name: &'a str,
    purpose: &'a str,
    input: &'a Value,
    raw_input: &'a str,
    state: ToolState,
    output: Option<&'a str>,
    meta: Option<&'a Value>,
    image_count: usize,
    time: i64,
    time_end: Option<i64>,
    run_at: Option<i64>,
}

fn render_tool(m: &Message, idx: usize, t: &ToolCall<'_>, cx: &Ctx<'_>, w: usize) -> Vec<Row> {
    let expanded = cx.expanded(idx, cx.details);
    let hit = Hit::Toggle {
        msg: m.id.clone(),
        part: idx,
    };
    let mut rows = vec![
        Row::new(tool_header(t, cx, w, expanded))
            .hit(hit.clone())
            .decor(),
    ];
    // Todo lists are always shown inline (like opencode's todo block).
    if t.name == "todo" {
        let todos = parse_todos(t.input);
        let max = if cx.compact {
            COMPACT_TODO_ROWS
        } else {
            usize::MAX
        };
        for r in todo_rows(&todos, w.saturating_sub(2), max) {
            rows.push(r.hit(hit.clone()).indent(vec![Span::raw("  ")]));
        }
    }

    let out_text = t.output.map(text::sanitize).unwrap_or_default();
    if !expanded {
        rows.extend(error_preview(t, &out_text, w));
        return rows;
    }

    let block = ToolBlock::new(&m.id, idx, cx.shows_full_output(idx), w);
    let mut block_rows = vec![block.section("input", true)];
    block_rows.extend(block.input_rows(t));
    // Diff (edit/write).
    if let Some(diff) = t
        .meta
        .and_then(|x| x.get("diff"))
        .and_then(Value::as_str)
        .filter(|d| !d.trim().is_empty())
    {
        block_rows.push(block.section("diff", false));
        block_rows.extend(block.diff_rows(diff));
    }
    block_rows.extend(block.output_rows(t, &out_text));
    if t.image_count > 0 {
        let note = format!("[{} image(s) attached]", t.image_count);
        let span = Span::styled(note, theme::fg(theme::ACCENT));
        block_rows.push(block.body(vec![span], false).decor());
    }
    block_rows.extend(block.meta_rows(t.meta));
    let border_style = theme::fg(theme::BORDER);
    block_rows.push(
        Row::new(Line::from(Span::styled(
            format!("╰{}╯", "─".repeat(block.bw.saturating_sub(2))),
            border_style,
        )))
        .decor(),
    );
    for r in block_rows {
        rows.push(r.indent(vec![Span::raw("  ")]));
    }
    rows
}

/// The one-line header of a tool call: state icon, name, purpose, detail and status.
fn tool_header(t: &ToolCall<'_>, cx: &Ctx<'_>, w: usize, expanded: bool) -> Line<'static> {
    let (icon, icolor) = match t.state {
        ToolState::Streaming | ToolState::Running => {
            (theme::spinner(cx.tick).to_string(), theme::PRIMARY)
        }
        ToolState::AwaitingPermission => ("?".into(), theme::WARNING),
        ToolState::Delayed => ("◷".into(), theme::WARNING),
        ToolState::Done => ("✓".into(), theme::SUCCESS),
        ToolState::Error => ("✗".into(), theme::ERROR),
        ToolState::Denied | ToolState::Cancelled => ("⊘".into(), theme::MUTED),
    };
    let dim_state = matches!(t.state, ToolState::Denied | ToolState::Cancelled);
    let name_disp = if t.name.is_empty() { "tool" } else { t.name };
    let mut name_style = theme::bold(if dim_state { theme::MUTED } else { theme::TEXT });
    if t.state == ToolState::Denied {
        name_style = name_style.add_modifier(Modifier::CROSSED_OUT);
    }

    let mut left: Vec<Span<'static>> = vec![Span::styled(
        if expanded { "▾ " } else { "▸ " },
        theme::fg(theme::DIM),
    )];
    left.push(Span::styled(format!("{icon} "), theme::fg(icolor)));
    left.push(Span::styled(name_disp.to_string(), name_style));
    let purpose = text::one_line(t.purpose);
    if !purpose.is_empty() {
        left.push(Span::styled(
            format!("  {purpose}"),
            if dim_state {
                theme::muted()
            } else {
                theme::text()
            },
        ));
    }
    let detail = tool_detail(t.name, t.input);
    if !detail.is_empty() && !cx.compact {
        left.push(Span::styled(format!("  {detail}"), theme::fg(theme::DIM)));
    }

    let mut right = tool_status_spans(t, cx);
    if cx.compact && w < COMPACT_HIDE_STATUS_BELOW {
        right.clear();
    }
    compose(left, right, w).0
}

/// Right side of the header: status + duration + start time.
fn tool_status_spans(t: &ToolCall<'_>, cx: &Ctx<'_>) -> Vec<Span<'static>> {
    let mut right: Vec<Span<'static>> = Vec::new();
    match t.state {
        ToolState::Streaming => right.push(Span::styled("writing…", theme::muted())),
        ToolState::AwaitingPermission => right.push(Span::styled(
            "awaiting permission",
            theme::fg(theme::WARNING),
        )),
        ToolState::Delayed => {
            let secs = t.run_at.map(|r| text::ceil_secs(r - cx.now)).unwrap_or(0);
            right.push(Span::styled(
                format!("runs in {secs}s · esc to cancel"),
                theme::fg(theme::WARNING),
            ));
        }
        ToolState::Running => right.push(Span::styled(
            format!("running {}", text::fmt_elapsed(cx.now - t.time)),
            theme::fg(theme::PRIMARY),
        )),
        ToolState::Done => {
            if let Some(e) = t.time_end {
                right.push(Span::styled(text::fmt_dur(e - t.time), theme::muted()));
            }
        }
        ToolState::Error => {
            right.push(Span::styled("error", theme::fg(theme::ERROR)));
            if let Some(e) = t.time_end {
                right.push(Span::styled(
                    format!(" {}", text::fmt_dur(e - t.time)),
                    theme::muted(),
                ));
            }
        }
        ToolState::Denied => right.push(Span::styled("denied", theme::muted())),
        ToolState::Cancelled => right.push(Span::styled("cancelled", theme::muted())),
    }
    if t.time > 0 && !cx.compact {
        right.push(Span::styled(
            format!("  {}", text::fmt_clock(t.time)),
            theme::fg(theme::DIM),
        ));
    }
    right
}

/// Errors: the first output line under the collapsed header.
fn error_preview(t: &ToolCall<'_>, out_text: &str, w: usize) -> Option<Row> {
    if t.state != ToolState::Error {
        return None;
    }
    let first = out_text.lines().map(str::trim).find(|l| !l.is_empty())?;
    let l = Line::from(vec![
        Span::styled("  └ ", theme::fg(theme::DIM)),
        Span::styled(
            text::truncate(first, w.saturating_sub(4)),
            theme::fg(theme::ERROR),
        ),
    ]);
    Some(Row::new(l).skip(4))
}

/// Layout of the expanded block of a tool call. Every row is exactly `bw` columns wide so the
/// right borders line up.
struct ToolBlock<'a> {
    msg: &'a str,
    idx: usize,
    /// The full output is shown instead of the first `OUTPUT_PREVIEW_LINES` lines.
    full: bool,
    /// Box total width.
    bw: usize,
    /// Inner text width.
    iw: usize,
}

impl<'a> ToolBlock<'a> {
    fn new(msg: &'a str, idx: usize, full: bool, w: usize) -> Self {
        let bw = w.saturating_sub(2).max(MIN_BLOCK_WIDTH);
        let iw = bw.saturating_sub(4).max(MIN_BLOCK_INNER_WIDTH);
        ToolBlock {
            msg,
            idx,
            full,
            bw,
            iw,
        }
    }

    fn section(&self, label: &str, first: bool) -> Row {
        box_section(label, first, self.bw)
    }

    fn body(&self, spans: Vec<Span<'static>>, soft: bool) -> Row {
        box_body(spans, self.iw, soft)
    }

    /// Lines shown before "show all": none are cut when the full output is shown.
    fn preview_limit(&self) -> Option<usize> {
        (!self.full).then_some(OUTPUT_PREVIEW_LINES)
    }

    /// `… N more lines (click to show all)`.
    fn show_all_row(&self, hidden: usize) -> Row {
        let note = format!("{} {hidden} more lines (click to show all)", text::ELLIPSIS);
        self.body(vec![Span::styled(note, theme::fg(theme::WARNING))], false)
            .hit(Hit::ShowAll {
                msg: self.msg.to_string(),
                part: self.idx,
            })
            .decor()
    }

    /// Pushes `t` as wrapped rows, at most `limit` logical lines; returns how many were cut.
    fn push_text(&self, rows: &mut Vec<Row>, t: &str, style: Style, limit: Option<usize>) -> usize {
        let lines: Vec<&str> = t.split('\n').collect();
        let n = lines.len();
        let shown = limit.map(|l| l.min(n)).unwrap_or(n);
        for l in &lines[..shown] {
            for (i, wl) in text::wrap_hard(&[Span::styled(l.to_string(), style)], self.iw)
                .into_iter()
                .enumerate()
            {
                rows.push(self.body(wl.spans, i > 0));
            }
        }
        n - shown
    }

    fn input_rows(&self, t: &ToolCall<'_>) -> Vec<Row> {
        let raw_input_only = t.input.is_null()
            || (t.input.as_object().is_some_and(|o| o.is_empty())
                && !t.raw_input.trim().is_empty()
                && t.raw_input.trim() != "{}");
        let input_text = if raw_input_only {
            t.raw_input.to_string()
        } else {
            serde_json::to_string_pretty(t.input).expect("a JSON value always serialises")
        };
        let input_text = text::sanitize(&input_text);
        let mut rows = Vec::new();
        let hidden = self.push_text(
            &mut rows,
            input_text.trim_end(),
            theme::fg(theme::INFO),
            self.preview_limit(),
        );
        if hidden > 0 {
            rows.push(self.show_all_row(hidden));
        }
        rows
    }

    fn diff_rows(&self, diff: &str) -> Vec<Row> {
        let diff = text::sanitize(diff);
        let lines: Vec<&str> = diff.trim_end().split('\n').collect();
        let limit = self
            .preview_limit()
            .map_or(lines.len(), |l| lines.len().min(l));
        let mut rows = Vec::new();
        for l in &lines[..limit] {
            let st = if l.starts_with("+++") || l.starts_with("---") {
                theme::bold(theme::MUTED)
            } else if l.starts_with('+') {
                Style::new().fg(theme::DIFF_ADDED).bg(theme::DIFF_ADDED_BG)
            } else if l.starts_with('-') {
                Style::new()
                    .fg(theme::DIFF_REMOVED)
                    .bg(theme::DIFF_REMOVED_BG)
            } else if l.starts_with("@@") {
                theme::fg(theme::INFO)
            } else {
                theme::fg(theme::DIFF_CONTEXT)
            };
            for (i, wl) in text::wrap_hard(&[Span::styled(l.to_string(), st)], self.iw)
                .into_iter()
                .enumerate()
            {
                let lw = text::line_width(&wl);
                let mut spans = wl.spans;
                if st.bg.is_some() {
                    spans.push(Span::styled(" ".repeat(self.iw.saturating_sub(lw)), st));
                }
                rows.push(self.body(spans, i > 0));
            }
        }
        if limit < lines.len() {
            rows.push(self.show_all_row(lines.len() - limit));
        }
        rows
    }

    /// The output section (or live output while running); empty when there is nothing to show.
    fn output_rows(&self, t: &ToolCall<'_>, out_text: &str) -> Vec<Row> {
        let live_out = t
            .meta
            .and_then(|x| x.get("live_output"))
            .and_then(Value::as_str)
            .map(text::sanitize);
        let (label, out) = match (t.state, &live_out) {
            (ToolState::Running, Some(lo)) => ("output (live)", lo.clone()),
            _ => ("output", out_text.to_string()),
        };
        if out.is_empty() && !t.state.is_final() {
            return Vec::new();
        }
        let mut rows = vec![self.section(label, false)];
        let st = if t.state == ToolState::Error {
            theme::fg(theme::ERROR)
        } else {
            theme::text()
        };
        if out.trim().is_empty() {
            rows.push(
                self.body(vec![Span::styled("(no output)", theme::muted())], false)
                    .decor(),
            );
        } else if t.state == ToolState::Running {
            // Live tail.
            let lines: Vec<&str> = out.trim_end().split('\n').collect();
            let start = lines.len().saturating_sub(LIVE_TAIL_LINES);
            if start > 0 {
                let note = format!("{} {start} earlier lines", text::ELLIPSIS);
                rows.push(
                    self.body(vec![Span::styled(note, theme::muted())], false)
                        .decor(),
                );
            }
            self.push_text(&mut rows, &lines[start..].join("\n"), st, None);
        } else {
            let hidden = self.push_text(&mut rows, out.trim_end(), st, self.preview_limit());
            if hidden > 0 {
                rows.push(self.show_all_row(hidden));
            }
        }
        rows
    }

    /// Extra meta (exit code etc.), excluding big fields.
    fn meta_rows(&self, meta: Option<&Value>) -> Vec<Row> {
        let Some(obj) = meta.and_then(Value::as_object) else {
            return Vec::new();
        };
        let mut bits = Vec::new();
        for (k, v) in obj {
            if matches!(k.as_str(), "diff" | "live_output") {
                continue;
            }
            let vs = match v {
                Value::Null => continue,
                Value::String(s) => text::truncate(&text::one_line(s), META_VALUE_MAX),
                Value::Bool(_) | Value::Number(_) => v.to_string(),
                _ => continue,
            };
            bits.push(format!("{k}: {vs}"));
        }
        if bits.is_empty() {
            return Vec::new();
        }
        let mut rows = vec![self.section("meta", false)];
        let joined = bits.join("  ·  ");
        for (i, wl) in text::wrap_spans(&[Span::styled(joined, theme::muted())], self.iw)
            .into_iter()
            .enumerate()
        {
            rows.push(self.body(wl.spans, i > 0));
        }
        rows
    }
}
