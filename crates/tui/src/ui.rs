//! Shared widgets: message area, permission panel, status lines, prompt, toasts,
//! autocomplete popup.

use crate::app::{App, UiHit};
use crate::catalog::title_case;
use crate::input::Editor;
use crate::render::message::{self, tool_icon, tool_input_text, tool_purpose, Opts};
use crate::render::util::{fit_spans, left_right, spans_width, with_bg};
use crate::text::{self, fmt_countdown, fmt_time};
use crate::theme;
use hn_proto::{PermissionDecision, SessionStatus, ToolPart};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub fn fill(buf: &mut Buffer, r: Rect, bg: Color) {
    for y in r.y..r.bottom() {
        for x in r.x..r.right() {
            if let Some(c) = buf.cell_mut((x, y)) {
                c.reset();
                c.set_bg(bg);
                c.set_fg(theme::TEXT);
            }
        }
    }
}

pub fn put(buf: &mut Buffer, x: u16, y: u16, line: &Line<'_>, w: u16) {
    if w > 0 {
        buf.set_line(x, y, line, w);
    }
}

pub fn opts(app: &App, compact: bool) -> Opts {
    Opts { now: hn_proto::now_ms(), show_thinking: app.show_thinking, details: app.details, compact }
}

/// Draw the transcript of `sid` into `r` (rightmost column = scrollbar).
pub fn draw_messages(app: &mut App, sid: &str, r: Rect, buf: &mut Buffer, compact: bool) {
    if r.width < 4 || r.height == 0 {
        return;
    }
    let o = opts(app, compact);
    let width = (r.width - 1) as usize;
    let Some(s) = app.sessions.get(sid) else { return };
    let view = app.views.entry(sid.to_string()).or_default();
    let doc = message::build(s, view, &app.catalog, o, width);
    view.apply_anchor(&doc, r.height as usize);
    view.clamp(doc.height, r.height as usize);
    let area = Rect { width: r.width - 1, ..r };
    message::draw(&doc, view, area, buf);
    // Loading / empty state
    if !s.loaded && s.msgs.is_empty() {
        let l = Line::from(vec![
            Span::styled(format!("{} ", theme::spinner(o.now)), theme::fg(theme::PRIMARY)),
            Span::styled("Loading session…", theme::muted()),
        ]);
        put(buf, r.x + 3, r.y + 1, &l, r.width.saturating_sub(4));
    }
    // Scrollbar: only while scrolled away from the bottom (at the bottom it's just noise,
    // and a static bar next to the sidebar reads like a broken border).
    if doc.height > r.height as usize && !view.stick_bottom {
        let h = r.height as usize;
        let th = ((h * h) / doc.height).max(1);
        let max = doc.height - h;
        let ty = (view.scroll * (h - th)).checked_div(max).unwrap_or(0);
        for i in 0..h {
            let on = i >= ty && i < ty + th;
            if let Some(c) = buf.cell_mut((r.right() - 1, r.y + i as u16)) {
                c.set_symbol(if on { "┃" } else { "│" });
                c.set_fg(if on { theme::BORDER_ACTIVE } else { theme::ELEMENT });
            }
        }
    }
    // Pushed before overlays (badge) so those win in reverse hit-testing.
    app.hits.push((area, UiHit::Messages(sid.to_string())));
    // "↓ new messages" badge
    if !view.stick_bottom && view.unseen {
        let label = " ↓ new messages ";
        let w = text::width(label) as u16;
        let x = r.x + r.width.saturating_sub(w) / 2;
        let y = r.bottom() - 1;
        let l = Line::from(Span::styled(label, Style::new().fg(theme::BG).bg(theme::PRIMARY)));
        put(buf, x, y, &l, w);
        app.hits.push((Rect::new(x, y, w, 1), UiHit::NewMessages(sid.to_string())));
    } else if !view.stick_bottom && compact {
        // Small indicator in compact panes.
        let l = Line::from(Span::styled("↓", theme::fg(theme::PRIMARY)));
        put(buf, r.right() - 1, r.bottom() - 1, &l, 1);
    }
    app.docs.insert(sid.to_string(), doc);
}

// ---------------------------------------------------------------------------
// Status lines (retry, delayed tool, connection)
// ---------------------------------------------------------------------------

pub fn status_lines(app: &App, sid: &str, width: usize) -> Vec<Line<'static>> {
    let now = hn_proto::now_ms();
    let mut out = Vec::new();
    if let Some(s) = app.sessions.get(sid) {
        if let SessionStatus::Retry { attempt, next_at, error } = &s.info.status {
            let l = vec![
                Span::styled("⟳ ", theme::fg(theme::WARNING)),
                Span::styled(
                    format!("Retrying (attempt {attempt}) in {}", fmt_countdown(next_at - now)),
                    theme::fg(theme::WARNING),
                ),
                Span::styled(": ", theme::muted()),
                Span::styled(text::one_line(error), theme::muted()),
            ];
            out.push(Line::from(fit_spans(l, width)));
        }
        if let Some(tp) = s.delayed_tool() {
            let left = tp.run_at.map(|r| r - now).unwrap_or(0);
            let l = vec![
                Span::styled("◷ ", theme::fg(theme::WARNING)),
                Span::styled(tp.name.clone(), theme::bold(theme::WARNING)),
                Span::styled(format!(" runs in {}", fmt_countdown(left)), theme::fg(theme::WARNING)),
                Span::styled("  ", theme::muted()),
                Span::styled(text::one_line(&tool_purpose(tp)), theme::muted()),
                Span::styled("  · ", theme::faint()),
                Span::styled("esc", theme::text()),
                Span::styled(" to stop", theme::muted()),
            ];
            out.push(Line::from(fit_spans(l, width)));
        }
        if s.closed {
            out.push(Line::from(Span::styled("■ Session closed on the backend", theme::fg(theme::ERROR))));
        }
    }
    out
}

pub fn conn_line(app: &App) -> Option<Line<'static>> {
    use crate::client::ConnState::*;
    let now = hn_proto::now_ms();
    match &app.conn {
        Connected => None,
        Connecting => Some(Line::from(vec![
            Span::styled(format!("{} ", theme::spinner(now)), theme::fg(theme::PRIMARY)),
            Span::styled("Connecting…", theme::muted()),
        ])),
        Reconnecting { attempt, next_at, error } => Some(Line::from(vec![
            Span::styled(format!("{} ", theme::spinner(now)), theme::fg(theme::WARNING)),
            Span::styled(
                format!("Disconnected · reconnecting in {} (attempt {attempt})", fmt_countdown(next_at - now)),
                theme::fg(theme::WARNING),
            ),
            Span::styled(format!("  {}", text::one_line(error)), theme::faint()),
        ])),
        Failed(e) => Some(Line::from(Span::styled(format!("✗ {e}"), theme::fg(theme::ERROR)))),
    }
}

// ---------------------------------------------------------------------------
// Permission panel
// ---------------------------------------------------------------------------

pub const PERM_CHOICES: [(PermissionDecision, &str, &str); 3] = [
    (PermissionDecision::Once, "Allow once", "a"),
    (PermissionDecision::Always, "Always allow", "A"),
    (PermissionDecision::Reject, "Reject", "r"),
];

fn json_lines(tp: &ToolPart, width: usize) -> Vec<Line<'static>> {
    let src = tool_input_text(tp);
    let mut hl = crate::render::highlight::Highlighter::new("json");
    let mut out = Vec::new();
    for raw in text::sanitize(&src).split('\n') {
        let segs = hl.line(raw, theme::text());
        for w in text::wrap_segments(&segs, width.max(4)) {
            out.push(Line::from(w.spans));
        }
    }
    out
}

/// Height the permission panel wants for `width`, capped at `max`.
pub fn perm_height(tp: &ToolPart, width: u16, max: u16, compact: bool) -> u16 {
    let inner = width.saturating_sub(if compact { 4 } else { 6 }) as usize;
    let n = json_lines(tp, inner).len() as u16;
    let chrome = if compact { 4 } else { 7 };
    (chrome + n).min(max).max(chrome + 1).min(max)
}

pub fn draw_permission(app: &mut App, sid: &str, msg_id: &str, tp: &ToolPart, r: Rect, buf: &mut Buffer, compact: bool) {
    let _ = msg_id;
    let bg = theme::PANEL;
    fill(buf, r, bg);
    for y in r.y..r.bottom() {
        if let Some(c) = buf.cell_mut((r.x, y)) {
            c.set_symbol("┃");
            c.set_fg(theme::WARNING);
        }
    }
    let x = r.x + if compact { 2 } else { 3 };
    let w = r.width.saturating_sub(if compact { 3 } else { 5 });
    let mut y = r.y + if compact { 0 } else { 1 };
    let (title, _) = left_right(
        vec![Span::styled("△ ", theme::fg(theme::WARNING)), Span::styled("Permission required", theme::bold(theme::TEXT))],
        vec![Span::styled(fmt_time(tp.time_start.max(1)), theme::muted())],
        w as usize,
        Style::new(),
    );
    put(buf, x, y, &with_bg(title, w as usize, bg), w);
    y += 1;
    let (icon, ist) = tool_icon(tp.state, hn_proto::now_ms());
    let _ = icon;
    let l = vec![
        Span::styled("⚙ ", ist),
        Span::styled(tp.name.clone(), theme::bold(theme::TEXT)),
        Span::styled("  ", theme::text()),
        Span::styled(text::one_line(&tool_purpose(tp)), theme::muted()),
    ];
    put(buf, x, y, &with_bg(Line::from(fit_spans(l, w as usize)), w as usize, bg), w);
    y += 1;
    if !compact {
        y += 1;
    }
    // Buttons + hints at the bottom
    let by = r.bottom().saturating_sub(if compact { 1 } else { 2 });
    let json_h = by.saturating_sub(y + if compact { 0 } else { 1 });
    let lines = json_lines(tp, w.saturating_sub(1) as usize);
    let max_scroll = lines.len().saturating_sub(json_h as usize);
    let scroll = app.perm_scroll.entry(tp.call_id.clone()).or_insert(0);
    *scroll = (*scroll).min(max_scroll);
    let scroll = *scroll;
    for (i, l) in lines.iter().skip(scroll).take(json_h as usize).enumerate() {
        let mut v = vec![Span::styled("│", theme::fg(theme::BORDER_SUBTLE))];
        v.extend(l.spans.iter().cloned());
        put(buf, x, y + i as u16, &with_bg(Line::from(v), w as usize, bg), w);
    }
    if max_scroll > 0 && json_h > 0 {
        let more_below = lines.len().saturating_sub(scroll + json_h as usize);
        let ind = format!(" ↑↓ {}/{} ", scroll + json_h as usize, lines.len());
        let st = if more_below > 0 { theme::fg(theme::PRIMARY) } else { theme::muted() };
        let iw = text::width(&ind) as u16;
        put(buf, x + w.saturating_sub(iw), y + json_h - 1, &Line::from(Span::styled(ind, st.bg(theme::ELEMENT))), iw);
    }
    let sel = *app.perm_sel.get(&tp.call_id).unwrap_or(&0);
    let mut bx = x;
    for (i, (d, label, key)) in PERM_CHOICES.iter().enumerate() {
        let selected = i == sel;
        let s = format!(" {label} ");
        let style = if selected {
            let c = if *d == PermissionDecision::Reject { theme::ERROR } else { theme::WARNING };
            Style::new().fg(theme::BG).bg(c).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(theme::MUTED).bg(theme::ELEMENT)
        };
        let bw = text::width(&s) as u16;
        if bx + bw > x + w {
            break;
        }
        put(buf, bx, by, &Line::from(Span::styled(s, style)), bw);
        app.hits.push((Rect::new(bx, by, bw, 1), UiHit::PermButton(sid.to_string(), tp.call_id.clone(), *d)));
        bx += bw;
        if !compact {
            let k = format!(" {key}");
            put(buf, bx, by, &Line::from(Span::styled(k.clone(), theme::faint().bg(bg))), 2);
            bx += 2;
        }
        bx += 2;
    }
    if !compact {
        let hint = vec![
            Span::styled("←→", theme::text()),
            Span::styled(" select  ", theme::muted()),
            Span::styled("enter", theme::text()),
            Span::styled(" confirm  ", theme::muted()),
            Span::styled("esc", theme::text()),
            Span::styled(" reject", theme::muted()),
        ];
        let hw = spans_width(&hint) as u16;
        if bx + hw + 2 <= x + w {
            put(buf, x + w - hw, by, &with_bg(Line::from(hint), hw as usize, bg), hw);
        }
    }
}

// ---------------------------------------------------------------------------
// Prompt box
// ---------------------------------------------------------------------------

const PLACEHOLDERS: &[&str] = &[
    "Fix a TODO in the codebase",
    "What is the tech stack of this project?",
    "Explain how the build works",
    "Write tests for the parser",
    "Find and fix a bug",
];

pub fn placeholder(seed: &str) -> String {
    let h = seed.bytes().fold(0usize, |a, b| a.wrapping_mul(31).wrapping_add(b as usize));
    format!("Ask anything… \"{}\"", PLACEHOLDERS[h % PLACEHOLDERS.len()])
}

/// Styled visual rows for the editor (chips highlighted).
pub fn editor_lines(ed: &Editor, width: usize) -> Vec<Line<'static>> {
    let t = ed.text();
    let chips = ed.chip_spans();
    ed.rows(width)
        .into_iter()
        .map(|row| {
            let mut spans = Vec::new();
            let mut pos = row.start;
            while pos < row.end {
                let chip = chips.iter().find(|(s, e)| pos >= *s && pos < *e);
                let (end, style) = match chip {
                    Some((_, e)) => ((*e).min(row.end), Style::new().fg(theme::BG).bg(theme::ACCENT)),
                    None => {
                        let next = chips.iter().filter(|(s, _)| *s > pos).map(|(s, _)| *s).min().unwrap_or(row.end);
                        (next.min(row.end), theme::text())
                    }
                };
                spans.push(Span::styled(t[pos..end].to_string(), style));
                pos = end;
            }
            Line::from(spans)
        })
        .collect()
}

/// Visible input rows for a prompt box: (lines, cursor (row, col) relative to first visible row).
pub fn editor_view(ed: &Editor, width: usize, max_rows: usize) -> (Vec<Line<'static>>, (usize, usize)) {
    let lines = editor_lines(ed, width);
    let (cr, cc) = ed.cursor_pos(width);
    let n = lines.len();
    let vis = n.clamp(1, max_rows);
    let start = if cr >= vis { cr + 1 - vis } else { 0 }.min(n.saturating_sub(vis));
    let out = lines.into_iter().skip(start).take(vis).collect();
    (out, (cr - start, cc))
}

pub fn prompt_inner_width(total: u16) -> usize {
    total.saturating_sub(1 + 2 + 2).max(4) as usize
}

pub fn prompt_height(ed: &Editor, total_w: u16) -> u16 {
    let rows = ed.rows(prompt_inner_width(total_w)).len().clamp(1, 8) as u16;
    rows + 4
}

/// Draw the prompt box; returns the terminal cursor position.
pub fn draw_prompt(app: &mut App, sid: Option<&str>, r: Rect, buf: &mut Buffer) -> Option<(u16, u16)> {
    let info = sid.and_then(|s| app.session_info(s)).cloned();
    let agent = info.as_ref().map(|i| i.agent.clone()).unwrap_or_default();
    let ac = app.catalog.agent_color(&agent);
    let bg = theme::ELEMENT;
    let body = Rect { height: r.height.saturating_sub(1), ..r };
    fill(buf, body, bg);
    for y in body.y..body.bottom() {
        if let Some(c) = buf.cell_mut((r.x, y)) {
            c.set_symbol("┃");
            c.set_fg(ac);
        }
    }
    // Half-block bottom edge
    let by = r.bottom() - 1;
    for x in r.x..r.right() {
        if let Some(c) = buf.cell_mut((x, by)) {
            c.reset();
            c.set_symbol(if x == r.x { "╹" } else { "▀" });
            c.set_fg(if x == r.x { ac } else { bg });
            c.set_bg(theme::BG);
        }
    }
    let iw = prompt_inner_width(r.width);
    app.prompt_width = iw;
    let x = r.x + 3;
    let y0 = r.y + 1;
    let max_rows = r.height.saturating_sub(4).max(1) as usize;
    let ed = &app.editor;
    let mut cursor = None;
    if ed.is_empty() {
        let ph = placeholder(sid.unwrap_or(""));
        put(buf, x, y0, &Line::from(Span::styled(text::truncate(&ph, iw), theme::faint())), iw as u16);
        cursor = Some((x, y0));
    } else {
        let (lines, (cr, cc)) = editor_view(ed, iw, max_rows);
        for (i, l) in lines.iter().enumerate() {
            put(buf, x, y0 + i as u16, l, iw as u16 + 1);
        }
        cursor = cursor.or(Some((x + cc.min(iw) as u16, y0 + cr as u16)));
    }
    app.hits.push((Rect::new(x, y0, iw as u16 + 1, max_rows as u16), UiHit::Prompt(sid.map(str::to_string))));
    // Meta row
    let my = r.bottom().saturating_sub(2);
    let mut meta = Vec::new();
    if !agent.is_empty() {
        meta.push(Span::styled(title_case(&agent), Style::new().fg(ac).add_modifier(Modifier::BOLD)));
    }
    if let Some(i) = &info {
        if !i.model.model.is_empty() {
            meta.push(Span::styled(" · ", theme::faint()));
            meta.push(Span::styled(app.catalog.model_name(&i.model), theme::text()));
            meta.push(Span::styled(format!(" {}", app.catalog.provider_name(&i.model)), theme::muted()));
            if let Some(v) = &i.model.variant {
                meta.push(Span::styled(format!(" · {v}"), theme::bold(theme::WARNING)));
            }
        }
    }
    let mut right = Vec::new();
    let chips = app.editor.chips.len();
    if chips > 0 {
        right.push(Span::styled(format!("{chips} attachment{}", if chips == 1 { "" } else { "s" }), theme::muted()));
    }
    let (ml, _) = left_right(meta, right, iw, Style::new());
    put(buf, x, my, &with_bg(ml, iw, bg), iw as u16);
    cursor
}

// ---------------------------------------------------------------------------
// Autocomplete
// ---------------------------------------------------------------------------

pub struct AcItem {
    pub name: String,
    pub desc: String,
    pub right: String,
    pub builtin: bool,
}

pub fn autocomplete(app: &App, ed: &Editor) -> Vec<AcItem> {
    if app.ac_hidden {
        return Vec::new();
    }
    let Some(q) = ed.slash_query() else { return Vec::new() };
    let mut items: Vec<(i32, AcItem)> = Vec::new();
    for (name, aliases, desc, key) in crate::app::BUILTINS {
        let score = std::iter::once(*name)
            .chain(aliases.iter().copied())
            .filter_map(|n| crate::dialog::fuzzy(q, n).map(|s| if n.starts_with(q) { s + 200 } else { s }))
            .max();
        if let Some(s) = score {
            items.push((s, AcItem { name: name.to_string(), desc: desc.to_string(), right: key.to_string(), builtin: true }));
        }
    }
    for c in &app.catalog.commands {
        if let Some(s) = crate::dialog::fuzzy(q, &c.name) {
            let s = if c.name.starts_with(q) { s + 200 } else { s };
            items.push((s, AcItem { name: c.name.clone(), desc: c.description.clone(), right: c.source.clone(), builtin: false }));
        }
    }
    if !q.is_empty() {
        items.sort_by_key(|x| std::cmp::Reverse(x.0));
    }
    items.into_iter().map(|x| x.1).collect()
}

pub fn draw_autocomplete(app: &mut App, items: &[AcItem], anchor: Rect, buf: &mut Buffer) {
    if items.is_empty() {
        return;
    }
    let n = items.len().min(10);
    let h = n as u16;
    let y = anchor.y.saturating_sub(h);
    let r = Rect { x: anchor.x, y, width: anchor.width, height: h.min(anchor.y) };
    fill(buf, r, theme::PANEL);
    let sel = app.ac_sel.min(items.len() - 1);
    let start = if sel >= n { sel + 1 - n } else { 0 };
    for (i, it) in items.iter().enumerate().skip(start).take(n) {
        let yy = r.y + (i - start) as u16;
        if yy >= r.bottom() {
            break;
        }
        let selected = i == sel;
        let fg = if selected { theme::BG } else { theme::TEXT };
        let sub = if selected { theme::BG } else { theme::MUTED };
        let left = vec![
            Span::raw("  "),
            Span::styled(format!("/{}", it.name), Style::new().fg(fg).add_modifier(Modifier::BOLD)),
            Span::styled(format!("  {}", text::one_line(&it.desc)), Style::new().fg(sub)),
        ];
        let right = vec![Span::styled(format!("{} ", it.right), Style::new().fg(if selected { theme::BG } else { theme::FAINT }))];
        let (l, _) = left_right(left, right, r.width as usize, Style::new());
        let l = with_bg(l, r.width as usize, if selected { theme::PRIMARY } else { theme::PANEL });
        put(buf, r.x, yy, &l, r.width);
        app.hits.push((Rect::new(r.x, yy, r.width, 1), UiHit::Autocomplete(i)));
    }
}

// ---------------------------------------------------------------------------
// Toasts
// ---------------------------------------------------------------------------

pub fn draw_toasts(app: &App, area: Rect, buf: &mut Buffer) {
    let mut y = area.y + 1;
    for t in app.toasts.iter().rev() {
        let color = match t.level {
            hn_proto::ToastLevel::Info => theme::INFO,
            hn_proto::ToastLevel::Success => theme::SUCCESS,
            hn_proto::ToastLevel::Warning => theme::WARNING,
            hn_proto::ToastLevel::Error => theme::ERROR,
        };
        let maxw = (area.width as usize).saturating_sub(4).clamp(10, 56);
        let wrapped = text::wrap_segments(&[(text::one_line(&t.msg), theme::text())], maxw - 4);
        let tw = wrapped.iter().map(|w| w.width).max().unwrap_or(0) + 4;
        let x = area.right().saturating_sub(tw as u16 + 1);
        let h = wrapped.len() as u16 + 2;
        if y + h > area.bottom() {
            break;
        }
        let r = Rect::new(x, y, tw as u16, h);
        fill(buf, r, theme::PANEL);
        for yy in r.y..r.bottom() {
            if let Some(c) = buf.cell_mut((r.x, yy)) {
                c.set_symbol("┃");
                c.set_fg(color);
            }
        }
        for (i, w) in wrapped.into_iter().enumerate() {
            let l = with_bg(Line::from(w.spans), tw - 3, theme::PANEL);
            put(buf, r.x + 2, r.y + 1 + i as u16, &l, tw as u16 - 3);
        }
        y += h + 1;
    }
}

/// opencode-like "blocks" scanner: a lit block bouncing across `n` cells.
pub fn scanner(now: i64, n: usize, color: Color) -> Vec<Span<'static>> {
    let period = (n as i64 - 1).max(1) * 2;
    let step = (now / 60).rem_euclid(period);
    let pos = if step < n as i64 { step } else { period - step } as f32;
    (0..n)
        .map(|i| {
            let d = (pos - i as f32).abs();
            let t = (1.0 - d / 3.0).max(0.0);
            let c = theme::blend(theme::BORDER_SUBTLE, color, t);
            Span::styled(if t > 0.0 { "■" } else { "⬝" }, Style::new().fg(c))
        })
        .collect()
}
