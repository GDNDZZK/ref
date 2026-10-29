//! Top-level drawing.

pub mod chat;
pub mod dialogs;
pub mod layout;
pub mod markdown;
pub mod text;
pub mod theme;
pub mod toast;
mod input;
mod sidebar;
mod watch;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Widget};
use ratatui::Frame;
use ref_proto::*;

use crate::app::{App, Mode};
use crate::util::{fmt_cost, fmt_tokens, spinner};
use chat::{lr, Opts};
use text::{hard_wrap, truncate, width};
use theme::*;

pub const SIDEBAR_W: u16 = 42;

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    app.size = (area.width, area.height);
    app.regions.perm_buttons.clear();
    app.regions.perm = Rect::default();
    app.regions.cursor = None;
    let buf = f.buffer_mut();
    if area.width < 20 || area.height < 8 {
        // Too small for any useful layout.
        app.regions = crate::app::Regions::default();
        app.watch.panes.clear();
        let msg = truncate("terminal too small", area.width as usize);
        let y = area.y + area.height / 2;
        buf.set_string(area.x + area.width.saturating_sub(width(&msg) as u16) / 2, y, msg, muted());
        return;
    }
    match app.mode {
        Mode::Normal => draw_normal(buf, area, app),
        Mode::Watch => watch::draw(buf, area, app),
    }
    let buf = f.buffer_mut();
    if let Some(p) = app.popup.as_mut() {
        p.draw(buf, app.regions.input);
    }
    if let Some(d) = app.dialog.as_mut() {
        d.draw(buf, area);
    }
    app.toasts.draw(buf, area);
    if let Some(pos) = input::cursor(app) {
        if app.dialog.is_none() {
            f.set_cursor_position(pos);
        }
    }
}

pub fn status_spans(info: &SessionInfo, tick: u64, now: i64) -> Vec<Span<'static>> {
    match &info.status {
        SessionStatus::Idle => {
            if info.open {
                vec![Span::styled("idle", muted())]
            } else {
                vec![Span::styled("closed", muted())]
            }
        }
        SessionStatus::Busy => vec![Span::styled(format!("{} working", spinner(tick)), fg(PRIMARY))],
        SessionStatus::Retrying { attempt, at, .. } => {
            let secs = ((at - now).max(0) + 999) / 1000;
            vec![Span::styled(format!("{} retry #{attempt} in {secs}s", spinner(tick)), fg(WARNING))]
        }
    }
}

/// Lines for the retry status bar shown above the input (full error, wrapped to at most 3 lines).
pub fn retry_lines(info: &SessionInfo, tick: u64, now: i64, w: usize) -> Vec<Line<'static>> {
    let SessionStatus::Retrying { attempt, at, error } = &info.status else { return vec![] };
    let secs = ((at - now).max(0) + 999) / 1000;
    let head = vec![
        Span::styled(format!("{} ", spinner(tick)), fg(WARNING)),
        Span::styled(format!("Retrying in {secs}s"), bold(WARNING)),
        Span::styled(format!(" (attempt {attempt})"), muted()),
        Span::styled("  esc esc to stop", muted()),
    ];
    let mut out = vec![Line::from(head)];
    let err = error.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut body = text::wrap_text(&err, w, fg(ERROR), &[], &[]);
    if body.len() > 2 {
        body.truncate(2);
        if let Some(l) = body.last_mut() {
            let t = text::line_text(l);
            *l = Line::from(Span::styled(truncate(&format!("{t}…"), w), fg(ERROR)));
        }
    }
    out.extend(body);
    out
}

pub fn ctx_spans(info: &SessionInfo) -> Vec<Span<'static>> {
    let c = &info.context;
    if c.max == 0 {
        return vec![Span::styled(fmt_tokens(c.used), muted())];
    }
    let pct = (c.used as f64 * 100.0 / c.max as f64).round() as u64;
    let col = if c.compact_at > 0 && c.used >= c.compact_at {
        ERROR
    } else if pct >= 70 {
        WARNING
    } else {
        MUTED
    };
    vec![Span::styled(format!("{}/{} ({pct}%)", fmt_tokens(c.used), fmt_tokens(c.max)), fg(col))]
}

fn short_model(m: &str) -> String {
    m.split_once('/').map(|x| x.1.to_string()).unwrap_or_else(|| m.to_string())
}

/// Fit `segs` (priority, spans) into `w` columns: lowest-priority segments are dropped first; the
/// priority-0 segment (the name) is truncated as a last resort. Segments keep their order.
pub fn fit_segments(segs: Vec<(u8, Vec<Span<'static>>)>, w: usize, sep: Span<'static>) -> Vec<Span<'static>> {
    let sw = width(&sep.content);
    let seg_w = |s: &Vec<Span<'static>>| s.iter().map(|x| width(&x.content)).sum::<usize>();
    let mut keep: Vec<bool> = vec![true; segs.len()];
    let total = |keep: &[bool]| -> usize {
        let ws: Vec<usize> = segs.iter().zip(keep).filter(|(_, k)| **k).map(|((_, s), _)| seg_w(s)).collect();
        ws.iter().sum::<usize>() + sw * ws.len().saturating_sub(1)
    };
    while total(&keep) > w {
        // Drop the highest priority number (least important) still kept, except priority 0.
        let Some(i) = (0..segs.len()).filter(|i| keep[*i] && segs[*i].0 > 0).max_by_key(|i| (segs[*i].0, *i)) else { break };
        keep[i] = false;
    }
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut first = true;
    for ((_, s), k) in segs.into_iter().zip(&keep) {
        if !k {
            continue;
        }
        if !first {
            out.push(sep.clone());
        }
        first = false;
        out.extend(s);
    }
    truncate_spans(out, w)
}

/// Truncate spans to `w` columns (an ellipsis marks the cut).
pub fn truncate_spans(spans: Vec<Span<'static>>, w: usize) -> Vec<Span<'static>> {
    let total: usize = spans.iter().map(|s| width(&s.content)).sum();
    if total <= w {
        return spans;
    }
    let mut out = Vec::new();
    let mut used = 0;
    for s in spans {
        let sw = width(&s.content);
        if used + sw < w {
            used += sw;
            out.push(s);
        } else {
            out.push(Span::styled(truncate(&s.content, w - used), s.style));
            break;
        }
    }
    out
}

fn draw_header(buf: &mut Buffer, area: Rect, app: &App) {
    let bg = Style::new().bg(PANEL);
    buf.set_style(area, bg);
    let Some(v) = app.current_view() else {
        let msg = if !app.connected {
            "connecting…"
        } else if app.current.is_some() {
            "loading session…"
        } else {
            "creating session…"
        };
        buf.set_line(area.x + 1, area.y, &Line::from(Span::styled(format!("{} {msg}", spinner(app.tick)), muted().bg(PANEL))), area.width.saturating_sub(1));
        return;
    };
    let i = &v.info;
    let now = ref_proto::now_ms();
    let w = area.width.saturating_sub(2) as usize;
    // Right side: status first (always visible), then other open sessions.
    let mut right = status_spans(i, app.tick, now);
    let others = app.store.list.iter().filter(|s| s.open && s.id != i.id).count();
    if others > 0 && w >= 90 {
        let busy = app.store.list.iter().filter(|s| s.open && s.id != i.id && s.status != SessionStatus::Idle).count();
        let txt = if busy > 0 { format!("  {others} other · {busy} busy") } else { format!("  {others} other") };
        right.push(Span::styled(txt, muted()));
    }
    if !app.mouse_capture {
        right.push(Span::raw(" "));
        right.push(Span::styled(" mouse off ", Style::new().fg(PANEL).bg(WARNING)));
    }
    let rw: usize = right.iter().map(|s| width(&s.content)).sum();
    let mut model = vec![Span::styled(short_model(&i.model), text())];
    if let Some(var) = &i.variant {
        model.push(Span::styled(format!(" {var}"), fg(WARNING)));
    }
    let segs = vec![
        (0, vec![Span::styled(i.name.clone(), Style::new().fg(TEXT).add_modifier(Modifier::BOLD))]),
        (2, vec![Span::styled(i.agent.clone(), fg(app.agent_color(&i.agent)))]),
        (1, model),
        (3, ctx_spans(i)),
        (4, vec![Span::styled(fmt_cost(i.cost), muted())]),
    ];
    let left = fit_segments(segs, w.saturating_sub(rw + 2), Span::styled(" · ", fg(BORDER_ACTIVE)));
    let (l, _) = lr(left, right, w);
    let l = Line::from(l.spans.into_iter().map(|s| Span::styled(s.content, s.style.bg(PANEL))).collect::<Vec<_>>());
    buf.set_line(area.x + 1, area.y, &l, area.width.saturating_sub(2));
}

/// Short human description of a connection error (raw transport errors are long and technical).
pub fn friendly_conn_error(e: &str) -> String {
    let l = e.to_lowercase();
    if l.contains("reset") || l.contains("closed") || l.contains("broken pipe") {
        "(connection lost)".into()
    } else if l.contains("refused") {
        "(refd not reachable)".into()
    } else if l.contains("did not come up") || l.contains("spawning") {
        format!("({})", e.lines().next().unwrap_or(e))
    } else {
        format!("({})", truncate(e.lines().next().unwrap_or(e), 60))
    }
}

fn draw_banner(buf: &mut Buffer, area: Rect, app: &App) {
    let msg = match app.conn_error.as_deref().map(friendly_conn_error) {
        Some(e) if !e.is_empty() => format!(" {} Backend disconnected — reconnecting…  {e} ", spinner(app.tick)),
        _ => format!(" {} Connecting to backend… ", spinner(app.tick)),
    };
    let st = Style::new().fg(PANEL).bg(WARNING).add_modifier(Modifier::BOLD);
    buf.set_style(area, st);
    buf.set_line(area.x, area.y, &Line::from(Span::styled(truncate(&msg, area.width as usize), st)), area.width);
}

fn draw_normal(buf: &mut Buffer, area: Rect, app: &mut App) {
    let mut main = area;
    if app.sidebar_visible() {
        let sw = SIDEBAR_W.min(area.width / 3);
        let sb = Rect { x: area.x + area.width - sw, width: sw, ..area };
        main.width -= sw;
        app.regions.sidebar = sb;
        sidebar::draw(buf, sb, app);
    } else {
        app.regions.sidebar = Rect::default();
    }
    let mut y = main.y;
    draw_header(buf, Rect { y, height: 1, ..main }, app);
    y += 1;
    if !app.connected {
        draw_banner(buf, Rect { y, height: 1, ..main }, app);
        y += 1;
    }
    y += 1; // spacing below header
    let bottom = main.y + main.height;
    // Input & hint at the bottom.
    let inner_w = main.width.saturating_sub(4).max(4);
    app.regions.input_text_w = inner_w;
    // The input grows with its content up to 10 rows, but never past ~40% of the screen.
    let max_rows = ((main.height.saturating_sub(4)) * 2 / 5).clamp(1, 10) as usize;
    let rows = app.editor.rows(inner_w as usize).len().clamp(1, max_rows) as u16;
    let input_h = rows + 2;
    let hint_y = bottom.saturating_sub(1);
    let input_rect = Rect { x: main.x, y: hint_y.saturating_sub(input_h), width: main.width, height: input_h };
    input::draw(buf, input_rect, app);
    input::draw_hints(buf, Rect { y: hint_y, height: 1, ..main }, app);
    let mut chat_bottom = input_rect.y;
    // Retry status (full error text) right above the input.
    if let Some(v) = app.current_view() {
        let lines = retry_lines(&v.info, app.tick, ref_proto::now_ms(), main.width.saturating_sub(4) as usize);
        let h = lines.len() as u16;
        if h > 0 && chat_bottom >= y + h + 3 {
            let r = Rect { x: main.x, y: chat_bottom - h, width: main.width, height: h };
            buf.set_style(r, Style::new().bg(PANEL));
            for (k, l) in lines.iter().enumerate() {
                let yy = r.y + k as u16;
                buf.set_string(r.x, yy, "┃", Style::new().fg(WARNING).bg(PANEL));
                let l = Line::from(l.spans.iter().map(|s| Span::styled(s.content.clone(), s.style.bg(PANEL))).collect::<Vec<_>>());
                buf.set_line(r.x + 2, yy, &l, r.width.saturating_sub(3));
            }
            chat_bottom = r.y;
        }
    }
    // Permission panel above input.
    if let Some((req, n)) = app.pending_permission() {
        // One blank row separates the panel from the input box.
        let avail = chat_bottom.saturating_sub(y + 1);
        let max_h = (avail * 2 / 3).max(6);
        let r = perm_panel(buf, Rect { x: main.x, y, width: main.width, height: avail }, &req, n, max_h, app, false);
        chat_bottom = r.y;
    }
    let chat_rect = Rect { x: main.x, y, width: main.width, height: chat_bottom.saturating_sub(y + 1) };
    app.regions.chat = chat_rect;
    let Some(sid) = app.current.clone() else { return };
    let colors = app.agent_colors();
    let o = Opts {
        width: chat_rect.width.saturating_sub(3) as usize,
        compact: false,
        details: app.details,
        thinking: app.thinking,
        tick: app.tick,
        now: ref_proto::now_ms(),
        agent_colors: &colors,
        highlight: app.highlight,
    };
    let Some(sv) = app.store.get(&sid) else { return };
    let view = app.views.entry(sid.clone()).or_default();
    view.layout(sv, &o);
    let sel = view.msg_select;
    chat::draw(buf, chat_rect, view, sel);
    if sv.messages.is_empty() {
        draw_empty(buf, chat_rect, sv, app);
    }
}

/// Block-letter logo in the opencode style. `_` = shadow cell, `^` = upper half block over shadow,
/// `~` = upper half block in the shadow color.
const LOGO: [&str; 3] = ["█▀▀█ █▀▀█ █▀▀▀", "█^^▄ █^^^ █▀▀ ", "▀  ▀ ▀▀▀▀ ▀   "];

fn draw_logo(buf: &mut Buffer, x: u16, y: u16, fgc: ratatui::style::Color, shadow: ratatui::style::Color) {
    for (i, l) in LOGO.iter().enumerate() {
        for (j, ch) in l.chars().enumerate() {
            let mut tmp = [0u8; 4];
            let (sym, st): (&str, Style) = match ch {
                '_' => (" ", Style::new().bg(shadow)),
                '^' => ("▀", Style::new().fg(fgc).bg(shadow)),
                '~' => ("▀", Style::new().fg(shadow)),
                ' ' => continue,
                c => (c.encode_utf8(&mut tmp), Style::new().fg(fgc)),
            };
            if let Some(cell) = buf.cell_mut((x + j as u16, y + i as u16)) {
                cell.set_symbol(sym).set_style(st);
            }
        }
    }
}

fn draw_empty(buf: &mut Buffer, area: Rect, sv: &crate::state::SessionView, app: &App) {
    let tips: [(&str, &str); 4] = [("/", "commands"), ("ctrl+p", "palette"), ("tab", "switch agent"), ("ctrl+x h", "help")];
    let full_h: u16 = 3 + 2 + 2 + tips.len() as u16;
    let show_tips = area.height >= full_h + 2;
    let block_h = if show_tips { full_h } else { 5 };
    if area.height < block_h + 1 || area.width < 20 {
        return;
    }
    let cy = area.y + (area.height - block_h) / 2;
    let lw = width(LOGO[0]) as u16;
    draw_logo(buf, area.x + (area.width - lw) / 2, cy, TEXT, rgb(0x404040));
    let center = |buf: &mut Buffer, y: u16, l: Line<'static>| {
        let l = Line::from(truncate_spans(l.spans, area.width as usize));
        let w = l.spans.iter().map(|s| width(&s.content)).sum::<usize>() as u16;
        buf.set_line(area.x + (area.width.saturating_sub(w)) / 2, y, &l, area.width);
    };
    let i = &sv.info;
    let mut y = cy + 4;
    center(
        buf,
        y,
        Line::from(vec![
            Span::styled(i.agent.clone(), fg(app.agent_color(&i.agent))),
            Span::styled(" · ", fg(BORDER_ACTIVE)),
            Span::styled(short_model(&i.model), text()),
            Span::styled(" · ", fg(BORDER_ACTIVE)),
            Span::styled(crate::actions::short_cwd(&i.cwd), muted()),
        ]),
    );
    if !show_tips {
        return;
    }
    y += 2;
    let kw = tips.iter().map(|(k, _)| width(k)).max().unwrap_or(0);
    let dw = tips.iter().map(|(_, d)| width(d)).max().unwrap_or(0);
    let bw = (kw + 2 + dw) as u16;
    let bx = area.x + area.width.saturating_sub(bw) / 2;
    for (k, d) in tips {
        let l = Line::from(vec![Span::styled(format!("{k:>kw$}"), text()), Span::styled(format!("  {d}"), muted())]);
        buf.set_line(bx, y, &l, area.width.saturating_sub(bx - area.x));
        y += 1;
    }
}

/// Draw the permission panel at the bottom of `area` (max height `max_h`). Returns the rect used.
pub fn perm_panel(buf: &mut Buffer, area: Rect, req: &PermissionRequest, n: usize, max_h: u16, app: &mut App, compact: bool) -> Rect {
    if app.perm_since.as_ref().map(|(id, _)| *id != req.call_id).unwrap_or(true) {
        app.perm_since = Some((req.call_id.clone(), std::time::Instant::now()));
    }
    let w = area.width as usize;
    let inner_w = w.saturating_sub(4);
    let json = serde_json::to_string_pretty(&req.input).unwrap_or_default();
    let body = hard_wrap(&json, inner_w, fg(INFO), &[]);
    let head_h: u16 = if compact { 1 } else { 2 };
    // title (+ blank) · body · blank · buttons
    let want = body.len() as u16 + head_h + 2;
    let h = want.min(max_h).min(area.height).max(4.min(area.height));
    let rect = Rect { x: area.x, y: area.y + area.height - h, width: area.width, height: h };
    app.regions.perm = rect;
    Clear.render(rect, buf);
    let bg = Style::new().bg(PANEL);
    buf.set_style(rect, bg);
    for yy in rect.y..rect.y + rect.height {
        buf.set_string(rect.x, yy, "┃", Style::new().fg(WARNING).bg(PANEL));
    }
    let mut y = rect.y;
    let queue = if n > 1 { format!("  (1/{n})") } else { String::new() };
    let title = Line::from(vec![
        Span::styled("⏸ Permission required", bold(WARNING).bg(PANEL)),
        Span::styled(queue, muted().bg(PANEL)),
        Span::styled("  ", bg),
        Span::styled(req.tool.clone(), bold(TEXT).bg(PANEL)),
        Span::styled("  ", bg),
        Span::styled(req.purpose.replace('\n', " "), muted().bg(PANEL)),
    ]);
    buf.set_line(rect.x + 2, y, &title, rect.width.saturating_sub(3));
    y += 1;
    if !compact {
        y += 1;
    }
    let body_h = (rect.y + rect.height).saturating_sub(y + 2) as usize;
    let max_scroll = body.len().saturating_sub(body_h);
    app.perm_scroll = app.perm_scroll.min(max_scroll);
    for l in body.iter().skip(app.perm_scroll).take(body_h) {
        let l = Line::from(l.spans.iter().map(|s| Span::styled(s.content.clone(), s.style.bg(PANEL))).collect::<Vec<_>>());
        buf.set_line(rect.x + 2, y, &l, rect.width.saturating_sub(3));
        y += 1;
    }
    let more = if max_scroll > 0 {
        format!("↑↓ scroll · {}/{} lines ", (app.perm_scroll + body_h).min(body.len()), body.len())
    } else {
        String::new()
    };
    // Buttons.
    let by = rect.y + rect.height - 1;
    let long = [("[a] allow once", PermissionDecision::Once, SUCCESS), ("[A] always (session)", PermissionDecision::Always, INFO), ("[d] deny", PermissionDecision::Deny, ERROR)];
    let short = [("a once", PermissionDecision::Once, SUCCESS), ("A always", PermissionDecision::Always, INFO), ("d deny", PermissionDecision::Deny, ERROR)];
    let fits = |ls: &[(&str, PermissionDecision, ratatui::style::Color)]| ls.iter().map(|(l, _, _)| width(l) + 4).sum::<usize>() + 2 <= rect.width as usize;
    let labels = if fits(&long) { long } else { short };
    let mut x = rect.x + 2;
    for (i, (l, d, c)) in labels.iter().enumerate() {
        let sel = app.perm_sel == i;
        let st = if sel { Style::new().fg(PANEL).bg(*c).add_modifier(Modifier::BOLD) } else { Style::new().fg(*c).bg(ELEMENT) };
        let label = format!(" {l} ");
        let lw = width(&label) as u16;
        if x + lw > rect.x + rect.width {
            break;
        }
        buf.set_string(x, by, &label, st);
        app.regions.perm_buttons.push((Rect { x, y: by, width: lw, height: 1 }, *d));
        x += lw + 2;
    }
    let mw = width(&more) as u16;
    if !more.is_empty() && x + mw < rect.x + rect.width {
        buf.set_string(rect.x + rect.width - mw, by, more, muted().bg(PANEL));
    }
    rect
}

#[cfg(test)]
mod tests;
