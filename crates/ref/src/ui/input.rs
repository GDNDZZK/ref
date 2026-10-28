//! Input box + hint line.

use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;

use super::chat::lr;
use super::theme::*;
use crate::app::{App, Mode};
use crate::editor::{chip_id, Editor};

pub fn cursor(app: &App) -> Option<Position> {
    app.regions.cursor
}

/// Render rows of `ed` (from `scroll`) into `area` (one row per line), chips styled.
pub fn render_rows(buf: &mut Buffer, area: Rect, ed: &Editor, scroll: usize, base: Style) {
    let rows = ed.rows(area.width as usize);
    for (i, r) in rows.iter().skip(scroll).take(area.height as usize).enumerate() {
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut plain = String::new();
        for g in ed.text()[r.start..r.end].graphemes(true) {
            let mut cs = g.chars();
            let chip = match (cs.next(), cs.next()) {
                (Some(c), None) => chip_id(c),
                _ => None,
            };
            match chip {
                Some(id) => {
                    if !plain.is_empty() {
                        spans.push(Span::styled(std::mem::take(&mut plain), base));
                    }
                    spans.push(Span::styled(ed.chip_label(id), Style::new().fg(PANEL).bg(ACCENT).add_modifier(Modifier::BOLD)));
                }
                None => plain.push_str(g),
            }
        }
        if !plain.is_empty() {
            spans.push(Span::styled(plain, base));
        }
        buf.set_line(area.x, area.y + i as u16, &Line::from(spans), area.width);
    }
}

pub fn draw(buf: &mut Buffer, area: Rect, app: &mut App) {
    app.regions.input = area;
    app.regions.cursor = None;
    let agent = app.current_view().map(|v| v.info.agent.clone()).unwrap_or_default();
    let col = app.agent_color(&agent);
    let bg = Style::new().bg(PANEL);
    buf.set_style(area, bg);
    for y in area.y..area.y + area.height {
        let ch = if y + 1 == area.y + area.height { "╹" } else { "┃" };
        buf.set_string(area.x, y, ch, Style::new().fg(col).bg(PANEL));
    }
    let text_area = Rect { x: area.x + 2, y: area.y + 1, width: area.width.saturating_sub(4), height: area.height.saturating_sub(2) };
    app.regions.input_text_x = text_area.x;
    let ed = &app.editor;
    let rows = ed.rows(text_area.width as usize);
    let (cr, cc) = ed.cursor_pos(&rows);
    let h = text_area.height as usize;
    let mut scroll = app.regions.input_scroll;
    if cr < scroll {
        scroll = cr;
    } else if cr >= scroll + h {
        scroll = cr + 1 - h;
    }
    scroll = scroll.min(rows.len().saturating_sub(h));
    app.regions.input_scroll = scroll;
    if ed.is_empty() {
        let ph = if app.current.is_some() { "Ask anything…  (/ for commands)" } else { "Ask anything…" };
        buf.set_string(text_area.x, text_area.y, ph, muted().bg(PANEL));
    } else {
        render_rows(buf, text_area, ed, scroll, text().bg(PANEL));
    }
    if app.dialog.is_none() && app.pending_permission().is_none() {
        app.regions.cursor = Some(Position { x: text_area.x + cc as u16, y: text_area.y + (cr - scroll) as u16 });
    }
}

pub fn draw_hints(buf: &mut Buffer, area: Rect, app: &App) {
    let mut left = vec![Span::raw(" ")];
    if let Some(v) = app.current_view() {
        let i = &v.info;
        left.push(Span::styled(i.agent.clone(), fg(app.agent_color(&i.agent)).add_modifier(Modifier::BOLD)));
        left.push(Span::styled(" · ", fg(BORDER_ACTIVE)));
        let model_name = app
            .store
            .catalog
            .as_ref()
            .and_then(|c| c.models.iter().find(|m| m.id == i.model))
            .map(|m| format!("{} ", m.name))
            .unwrap_or_default();
        left.push(Span::styled(model_name, text()));
        left.push(Span::styled(i.model.split_once('/').map(|x| x.0.to_string()).unwrap_or_default(), muted()));
        if let Some(var) = &i.variant {
            left.push(Span::styled(" · ", fg(BORDER_ACTIVE)));
            left.push(Span::styled(var.clone(), fg(WARNING)));
        }
        let imgs = app.editor.image_count();
        if imgs > 0 {
            left.push(Span::styled(format!(" · {imgs} image{}", if imgs > 1 { "s" } else { "" }), fg(ACCENT)));
        }
    }
    let key = |k: &str, d: &str| vec![Span::styled(k.to_string(), text()), Span::styled(format!(" {d}  "), muted())];
    let mut right: Vec<Span<'static>> = vec![];
    if app.leader.is_some() {
        right.push(Span::styled(" ctrl+x … ", Style::new().fg(PANEL).bg(PRIMARY).add_modifier(Modifier::BOLD)));
        right.push(Span::raw("  "));
        right.extend(key("n", "new"));
        right.extend(key("l", "sessions"));
        right.extend(key("m", "models"));
        right.extend(key("h", "help"));
    } else {
        let busy = app.target_session().and_then(|s| app.store.get(&s)).map(|v| v.busy()).unwrap_or(false);
        if app.esc_at.is_some() && busy {
            right.push(Span::styled("press esc again to interrupt  ", fg(WARNING)));
        } else if busy {
            right.extend(key("esc", "interrupt"));
        }
        if app.selection_session().is_some() {
            right.extend(key("ctrl+c", "copy selection"));
        }
        if app.mode == Mode::Normal && app.current.as_ref().and_then(|s| app.views.get(s)).and_then(|v| v.msg_select).is_some() {
            right.extend(key("↑↓", "select"));
            right.extend(key("y", "copy"));
            right.extend(key("enter", "expand"));
        } else {
            right.extend(key("tab", "agents"));
            right.extend(key("ctrl+p", "commands"));
        }
    }
    let (l, _) = lr(left, right, area.width as usize);
    buf.set_line(area.x, area.y, &l, area.width);
}
