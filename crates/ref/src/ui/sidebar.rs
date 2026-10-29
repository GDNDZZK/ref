//! Right sidebar: session info, context bar, cost, MCP, other sessions.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::text::{truncate, wrap_text};
use super::theme::*;
use crate::app::App;
use crate::util::{fmt_cost, fmt_tokens};
use ref_proto::SessionStatus;

pub fn draw(buf: &mut Buffer, area: Rect, app: &App) {
    let bg = Style::new().bg(PANEL);
    buf.set_style(area, bg);
    let x = area.x + 2;
    let w = area.width.saturating_sub(4) as usize;
    let mut y = area.y + 1;
    let bottom = area.y + area.height;
    let mut put = |y: &mut u16, l: Line<'static>| {
        if *y < bottom {
            let l = Line::from(l.spans.into_iter().map(|s| Span::styled(s.content, s.style.bg(PANEL))).collect::<Vec<_>>());
            buf.set_line(x, *y, &l, w as u16);
        }
        *y += 1;
    };
    if let Some(v) = app.current_view() {
        let i = &v.info;
        for l in wrap_text(&i.name, w, bold(TEXT), &[], &[]).into_iter().take(3) {
            put(&mut y, l);
        }
        put(&mut y, Line::from(Span::styled(truncate(&crate::actions::short_cwd(&i.cwd), w), muted())));
        put(&mut y, Line::from(Span::styled(truncate(&i.id, w), fg(BORDER_ACTIVE))));
        y += 1;
        put(&mut y, Line::from(Span::styled("Context", bold(TEXT))));
        let c = &i.context;
        let pct = if c.max > 0 { c.used as f64 / c.max as f64 } else { 0.0 };
        put(&mut y, Line::from(Span::styled(format!("{} / {} tokens", fmt_tokens(c.used), fmt_tokens(c.max)), muted())));
        let bw = w.min(36);
        let filled = ((pct.min(1.0)) * bw as f64).round() as usize;
        let col = if c.compact_at > 0 && c.used >= c.compact_at { ERROR } else if pct > 0.7 { WARNING } else { SUCCESS };
        let mark = if c.max > 0 && c.compact_at > 0 { Some((c.compact_at as f64 / c.max as f64 * bw as f64) as usize) } else { None };
        let mut spans = vec![];
        for k in 0..bw {
            let ch = if Some(k) == mark { "┃" } else if k < filled { "█" } else { "░" };
            let st = if Some(k) == mark { fg(ERROR) } else if k < filled { fg(col) } else { fg(BORDER_SUBTLE) };
            spans.push(Span::styled(ch, st));
        }
        put(&mut y, Line::from(spans));
        put(&mut y, Line::from(Span::styled(format!("{:.0}% used · {} spent", pct * 100.0, fmt_cost(i.cost)), muted())));
        y += 1;
        put(&mut y, Line::from(Span::styled("Model", bold(TEXT))));
        put(&mut y, Line::from(vec![Span::styled(truncate(&i.model, w), text())]));
        let mut l = vec![Span::styled(i.agent.clone(), fg(app.agent_color(&i.agent)))];
        if let Some(var) = &i.variant {
            l.push(Span::styled(format!(" · {var}"), fg(WARNING)));
        }
        put(&mut y, Line::from(l));
        y += 1;
    }
    if let Some(cat) = &app.store.catalog {
        if !cat.mcp.is_empty() {
            put(&mut y, Line::from(Span::styled("MCP", bold(TEXT))));
            for m in &cat.mcp {
                let (col, label) = match m.status.as_str() {
                    "connected" => (SUCCESS, format!("Connected · {} tools", m.tools)),
                    "failed" => (ERROR, format!("Failed{}", m.error.as_ref().map(|e| format!(": {e}")).unwrap_or_default())),
                    "connecting" => (WARNING, "Connecting…".to_string()),
                    "disabled" => (MUTED, "Disabled".to_string()),
                    other => (MUTED, other.to_string()),
                };
                let line = Line::from(vec![Span::styled("• ", fg(col)), Span::styled(format!("{} ", m.name), text()), Span::styled(label, muted())]);
                let s: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
                let _ = s;
                put(&mut y, Line::from(line.spans.into_iter().map(|s| Span::styled(truncate(&s.content, w), s.style)).collect::<Vec<_>>()));
            }
            y += 1;
        }
    }
    let mut others: Vec<_> = app.store.list.iter().filter(|s| s.open && Some(&s.id) != app.current.as_ref()).collect();
    others.sort_by(|a, b| a.created.cmp(&b.created).then(a.id.cmp(&b.id)));
    if !others.is_empty() {
        put(&mut y, Line::from(Span::styled("Sessions", bold(TEXT))));
        for s in others {
            let (icon, col) = match s.status {
                SessionStatus::Idle => ("○", MUTED),
                SessionStatus::Busy => ("●", PRIMARY),
                SessionStatus::Retrying { .. } => ("●", WARNING),
            };
            let place = if s.attached > 0 { "fg" } else { "bg" };
            let perm = if s.pending_permissions > 0 { " ⏸" } else { "" };
            let name = truncate(&s.name, w.saturating_sub(8));
            put(
                &mut y,
                Line::from(vec![
                    Span::styled(format!("{icon} "), fg(col)),
                    Span::styled(name, text()),
                    Span::styled(format!(" {place}"), muted().add_modifier(Modifier::ITALIC)),
                    Span::styled(perm, fg(WARNING)),
                ]),
            );
        }
    }
}
