//! Watch mode: split view of open background sessions.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Widget};

use super::chat::{self, lr, Opts};
use super::layout;
use super::text::truncate;
use super::theme::*;
use crate::app::App;

pub fn draw(buf: &mut Buffer, area: Rect, app: &mut App) {
    let ids = app.watch_ids();
    app.watch.panes.clear();
    // Top bar.
    let top = Rect { height: 1, ..area };
    buf.set_style(top, Style::new().bg(PANEL));
    let mut left = vec![
        Span::styled(" watch ", Style::new().fg(PANEL).bg(PRIMARY).add_modifier(Modifier::BOLD)),
        Span::styled(format!("  {} background session{}", ids.len(), if ids.len() == 1 { "" } else { "s" }), muted()),
    ];
    if !app.connected {
        left.push(Span::styled("  disconnected — reconnecting…", fg(WARNING)));
    }
    let right = vec![Span::styled("ctrl+arrows focus · ctrl+x enter attach · ctrl+x z zoom · ctrl+x w exit ", muted())];
    let (l, _) = lr(left, right, area.width as usize);
    let l = Line::from(l.spans.into_iter().map(|s| Span::styled(s.content, s.style.bg(PANEL))).collect::<Vec<_>>());
    buf.set_line(area.x, area.y, &l, area.width);
    let body = Rect { y: area.y + 1, height: area.height.saturating_sub(1), ..area };
    if ids.is_empty() {
        let msg = "No background sessions. They appear here when created (e.g. by create_session) or detached.";
        let t = truncate(msg, body.width as usize);
        let lr = layout::centered(body, super::text::width(&t) as u16, 1);
        buf.set_string(lr.x, lr.y, t, muted());
        return;
    }
    // Zoom: only the focused pane.
    let shown: Vec<String> = if app.watch.zoom {
        app.watch.focus.clone().into_iter().collect()
    } else {
        let (size, pages) = layout::pages(ids.len(), body);
        app.watch.pages = pages;
        app.watch.page = app.watch.page.min(pages - 1);
        let start = app.watch.page * size;
        let v: Vec<String> = ids.iter().skip(start).take(size).cloned().collect();
        app.watch.more = ids.len() - v.len();
        if app.watch.focus.as_ref().map(|f| !v.contains(f)).unwrap_or(true) {
            app.watch.focus = v.first().cloned();
        }
        v
    };
    let rects = layout::grid(shown.len(), body);
    for (id, r) in shown.iter().zip(rects) {
        draw_pane(buf, r, app, id);
    }
    if app.watch.more > 0 && !app.watch.zoom {
        let label = format!(" +{} more · page {}/{} · ctrl+x [ ] ", app.watch.more, app.watch.page + 1, app.watch.pages);
        let w = super::text::width(&label) as u16;
        buf.set_string(area.x + area.width.saturating_sub(w + 1), area.y + area.height - 1, label, Style::new().fg(PANEL).bg(ACCENT));
    }
}

fn draw_pane(buf: &mut Buffer, r: Rect, app: &mut App, id: &str) {
    let focused = app.watch.focus.as_deref() == Some(id);
    let Some(sv) = app.store.get(id) else {
        let b = Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(fg(BORDER));
        let inner = b.inner(r);
        b.render(r, buf);
        buf.set_string(inner.x, inner.y, "loading…", muted());
        app.watch.panes.push((id.to_string(), r, inner));
        return;
    };
    let info = sv.info.clone();
    let bcol = if focused { PRIMARY } else { BORDER };
    let name = truncate(&info.name, (r.width as usize).saturating_sub(10));
    let title = Line::from(vec![Span::styled(format!(" {name} "), if focused { bold(PRIMARY) } else { bold(TEXT) })]);
    let block = Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(fg(bcol)).title(title);
    let inner = block.inner(r);
    block.render(r, buf);
    if inner.height < 2 || inner.width < 10 {
        app.watch.panes.push((id.to_string(), r, inner));
        return;
    }
    // Header line inside the pane.
    let now = ref_proto::now_ms();
    let mut left = vec![
        Span::styled(info.agent.clone(), fg(app.agent_color(&info.agent))),
        Span::styled(" · ", fg(BORDER_ACTIVE)),
        Span::styled(info.model.split_once('/').map(|x| x.1.to_string()).unwrap_or(info.model.clone()), muted()),
        Span::styled(" · ", fg(BORDER_ACTIVE)),
    ];
    left.extend(super::status_spans(&info, app.tick, now));
    let ctx = (info.context.used * 100).checked_div(info.context.max).map(|p| format!("{p}%")).unwrap_or_default();
    let (hl, _) = lr(left, vec![Span::styled(ctx, muted())], inner.width as usize);
    buf.set_line(inner.x, inner.y, &hl, inner.width);
    let mut bottom = inner.y + inner.height;
    // Input line (focused only).
    if focused {
        bottom -= 1;
        let ir = Rect { x: inner.x, y: bottom, width: inner.width, height: 1 };
        buf.set_style(ir, Style::new().bg(ELEMENT));
        buf.set_string(ir.x, ir.y, "› ", fg(PRIMARY).bg(ELEMENT));
        let ed = app.pane_editors.entry(id.to_string()).or_default();
        let tw = ir.width.saturating_sub(3);
        let rows = ed.rows(1000);
        let (_, cc) = ed.cursor_pos(&rows);
        if ed.is_empty() {
            buf.set_string(ir.x + 2, ir.y, "Message this session…", muted().bg(ELEMENT));
        } else {
            // Horizontal scroll so the cursor stays visible.
            let off = (cc + 1).saturating_sub(tw as usize);
            let text: String = ed.display_text().replace('\n', "⏎");
            let shown = chat::slice_cols(&text, off, off + tw as usize);
            buf.set_string(ir.x + 2, ir.y, shown, text_style().bg(ELEMENT));
        }
        if app.dialog.is_none() {
            let off = (cc + 1).saturating_sub(tw as usize);
            app.regions.cursor = Some(ratatui::layout::Position { x: ir.x + 2 + (cc - off) as u16, y: ir.y });
        }
    }
    // Permission prompt inside the pane.
    let sv = app.store.get(id).unwrap();
    let perm = sv.permissions.first().cloned().map(|p| (p, sv.permissions.len()));
    let chat_top = inner.y + 1;
    if let (Some((req, n)), true) = (perm.clone(), focused) {
        let avail = Rect { x: inner.x, y: chat_top, width: inner.width, height: bottom - chat_top };
        let pr = super::perm_panel(buf, avail, &req, n, (avail.height / 2).max(5), app, true);
        bottom = pr.y;
    } else if let Some((req, _)) = perm {
        bottom -= 1;
        buf.set_string(inner.x, bottom, truncate(&format!("⏸ permission: {} — focus pane to answer", req.tool), inner.width as usize), fg(WARNING));
    }
    let chat_rect = Rect { x: inner.x, y: chat_top, width: inner.width, height: bottom.saturating_sub(chat_top) };
    let colors = app.agent_colors();
    let o = Opts {
        width: chat_rect.width.saturating_sub(3) as usize,
        compact: true,
        details: false,
        thinking: false,
        tick: app.tick,
        now,
        agent_colors: &colors,
        highlight: app.highlight,
    };
    let sv = app.store.get(id).unwrap();
    let view = app.views.entry(id.to_string()).or_default();
    view.layout(sv, &o);
    chat::draw(buf, chat_rect, view, None);
    app.watch.panes.push((id.to_string(), r, chat_rect));
}

fn text_style() -> Style {
    text()
}
