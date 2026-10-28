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
        SessionStatus::Retrying { attempt, at, error } => {
            let secs = ((at - now).max(0) + 999) / 1000;
            vec![Span::styled(format!("{} retrying in {secs}s (attempt {attempt}): {}", spinner(tick), error.replace('\n', " ")), fg(WARNING))]
        }
    }
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

fn draw_header(buf: &mut Buffer, area: Rect, app: &App) {
    let bg = Style::new().bg(PANEL);
    buf.set_style(area, bg);
    let Some(v) = app.current_view() else {
        let msg = if app.connected { "creating session…" } else { "connecting…" };
        buf.set_line(area.x + 1, area.y, &Line::from(Span::styled(format!("{} {msg}", spinner(app.tick)), muted())), area.width);
        return;
    };
    let i = &v.info;
    let sep = || Span::styled(" · ", fg(BORDER_ACTIVE));
    let mut left = vec![Span::styled(i.name.clone(), Style::new().fg(TEXT).add_modifier(Modifier::BOLD))];
    left.push(sep());
    left.push(Span::styled(i.agent.clone(), fg(app.agent_color(&i.agent))));
    left.push(sep());
    left.push(Span::styled(short_model(&i.model), text()));
    if let Some(var) = &i.variant {
        left.push(Span::styled(format!(" {var}"), fg(WARNING)));
    }
    left.push(sep());
    left.extend(ctx_spans(i));
    left.push(sep());
    left.push(Span::styled(fmt_cost(i.cost), muted()));
    left.push(sep());
    left.extend(status_spans(i, app.tick, ref_proto::now_ms()));
    let mut right = vec![];
    let others = app.store.list.iter().filter(|s| s.open && s.id != i.id).count();
    if others > 0 {
        let busy = app.store.list.iter().filter(|s| s.open && s.id != i.id && s.status != SessionStatus::Idle).count();
        right.push(Span::styled(format!("{others} other open{} ", if busy > 0 { format!(", {busy} busy") } else { String::new() }), muted()));
    }
    if !app.mouse_capture {
        right.push(Span::styled(" mouse off ", Style::new().fg(PANEL).bg(WARNING)));
    }
    let (l, _) = lr(left, right, area.width.saturating_sub(2) as usize);
    let l = Line::from(l.spans.into_iter().map(|s| Span::styled(s.content, s.style.bg(PANEL))).collect::<Vec<_>>());
    buf.set_line(area.x + 1, area.y, &l, area.width.saturating_sub(2));
}

fn draw_banner(buf: &mut Buffer, area: Rect, app: &App) {
    let msg = match &app.conn_error {
        Some(e) => format!(" {} disconnected — reconnecting…  ({}) ", spinner(app.tick), e),
        None => format!(" {} disconnected — reconnecting… ", spinner(app.tick)),
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
    let rows = app.editor.rows(inner_w as usize).len().clamp(1, 10) as u16;
    let input_h = rows + 2;
    let hint_y = bottom.saturating_sub(1);
    let input_rect = Rect { x: main.x, y: hint_y.saturating_sub(input_h), width: main.width, height: input_h };
    input::draw(buf, input_rect, app);
    input::draw_hints(buf, Rect { y: hint_y, height: 1, ..main }, app);
    let mut chat_bottom = input_rect.y;
    // Permission panel above input.
    if let Some((req, n)) = app.pending_permission() {
        if app.perm_since.as_ref().map(|(id, _)| *id != req.call_id).unwrap_or(true) {
            app.perm_since = Some((req.call_id.clone(), std::time::Instant::now()));
        }
        let max_h = ((chat_bottom.saturating_sub(y)) * 2 / 3).max(6);
        let r = perm_panel(buf, Rect { x: main.x, y, width: main.width, height: chat_bottom - y }, &req, n, max_h, app, false);
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
    if sv.messages.is_empty() && chat_rect.height > 6 {
        draw_empty(buf, chat_rect, sv);
    }
}

fn draw_empty(buf: &mut Buffer, area: Rect, sv: &crate::state::SessionView) {
    let logo = ["█▀█ █▀▀ █▀▀", "█▀▄ ██▄ █▀ "];
    let cy = area.y + area.height / 2 - 3;
    for (i, l) in logo.iter().enumerate() {
        let w = width(l) as u16;
        buf.set_string(area.x + (area.width.saturating_sub(w)) / 2, cy + i as u16, l, fg(if i == 0 { TEXT } else { MUTED }));
    }
    let lines = [
        format!("{}  ·  {}", sv.info.name, crate::actions::short_cwd(&sv.info.cwd)),
        "enter send · / commands · ctrl+p palette · ctrl+x h help".to_string(),
    ];
    for (i, l) in lines.iter().enumerate() {
        let t = truncate(l, area.width as usize);
        let w = width(&t) as u16;
        buf.set_string(area.x + (area.width.saturating_sub(w)) / 2, cy + 3 + i as u16, t, muted());
    }
}

/// Draw the permission panel at the bottom of `area` (max height `max_h`). Returns the rect used.
pub fn perm_panel(buf: &mut Buffer, area: Rect, req: &PermissionRequest, n: usize, max_h: u16, app: &mut App, compact: bool) -> Rect {
    let w = area.width as usize;
    let inner_w = w.saturating_sub(4);
    let json = serde_json::to_string_pretty(&req.input).unwrap_or_default();
    let body = hard_wrap(&json, inner_w, fg(INFO), &[]);
    let head_h: u16 = if compact { 1 } else { 2 };
    let want = body.len() as u16 + head_h + 3;
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
    if max_scroll > 0 {
        let more = format!(" ↑↓ scroll · {}/{} lines ", (app.perm_scroll + body_h).min(body.len()), body.len());
        let mw = width(&more) as u16;
        buf.set_string(rect.x + rect.width.saturating_sub(mw + 1), rect.y + rect.height - 2, more, muted().bg(PANEL));
    }
    // Buttons.
    let by = rect.y + rect.height - 1;
    let labels = [("[a] allow once", PermissionDecision::Once, SUCCESS), ("[A] always (session)", PermissionDecision::Always, INFO), ("[d] deny", PermissionDecision::Deny, ERROR)];
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
    rect
}

#[cfg(test)]
mod tests;
