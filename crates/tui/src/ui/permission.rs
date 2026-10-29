//! Permission request modal.

use std::time::Instant;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ref_proto::PermissionRequest;
use serde_json::Value;

use super::dialog::frame;
use super::text::{self, width};
use super::theme;

pub struct PermissionView {
    pub scroll: usize,
    pub opened: Instant,
    pub call_id: String,
    max_scroll: usize,
    /// Button zones at last draw: (y, x0, x1, action) action: 0 allow, 1 always, 2 deny.
    pub buttons: Vec<(u16, u16, u16, u8)>,
    pub rect: Rect,
}

impl PermissionView {
    pub fn new(call_id: &str) -> Self {
        PermissionView { scroll: 0, opened: Instant::now(), call_id: call_id.to_string(), max_scroll: 0, buttons: Vec::new(), rect: Rect::default() }
    }

    pub fn scroll_by(&mut self, d: i32) {
        self.scroll = (self.scroll as i32 + d).clamp(0, self.max_scroll as i32) as usize;
    }

    pub fn button_at(&self, x: u16, y: u16) -> Option<u8> {
        self.buttons.iter().find(|(by, x0, x1, _)| *by == y && x >= *x0 && x < *x1).map(|b| b.3)
    }

    pub fn draw(&mut self, buf: &mut Buffer, screen: Rect, req: &PermissionRequest, session_name: Option<&str>, queued: usize) {
        let t = (self.opened.elapsed().as_secs_f32() / 0.12).min(1.0);
        let w = screen.width.saturating_sub(4).clamp(30, 96);
        let title = match session_name {
            Some(n) => format!("△ Permission required · {n}"),
            None => "△ Permission required".to_string(),
        };
        // Content is laid out for the final (fully opened) width; the frame only grows in.
        let iw = w.min(screen.width).saturating_sub(4) as usize;
        let lines = content(req, iw);
        // Height fits the content (borders + top pad + gap + scroll hint + buttons), capped.
        let max_h = screen.height.saturating_sub(2).clamp(10, 36);
        let h = (lines.len() as u16 + 5).clamp(10, max_h);
        let r = frame(buf, screen, w, h, &title, t);
        self.rect = r;
        let bg = theme::PANEL;
        let inner = Rect::new(r.x + 2, r.y + 1, r.width.saturating_sub(4), r.height.saturating_sub(2));
        let body_h = (inner.height as usize).saturating_sub(3);
        self.max_scroll = lines.len().saturating_sub(body_h);
        self.scroll = self.scroll.min(self.max_scroll);
        for (i, l) in lines.iter().skip(self.scroll).take(body_h).enumerate() {
            let mut l = l.clone();
            for s in &mut l.spans {
                s.style = s.style.bg(bg);
            }
            buf.set_line(inner.x, inner.y + i as u16, &l, inner.width);
        }
        if self.max_scroll > 0 {
            let more = format!("↑↓ scroll ({}/{})", self.scroll, self.max_scroll);
            buf.set_string(inner.x + inner.width.saturating_sub(width(&more) as u16), inner.y + body_h as u16, more, Style::new().fg(theme::DIM).bg(bg));
        }
        // Buttons.
        let by = inner.y + inner.height.saturating_sub(1);
        self.buttons.clear();
        let mut x = inner.x;
        let btns: [(&str, &str, u8, ratatui::style::Color); 3] =
            [(" a ", "Allow once ", 0, theme::SUCCESS), (" s ", "Always allow ", 1, theme::INFO), (" d ", "Deny ", 2, theme::ERROR)];
        for (k, label, act, c) in btns {
            let kw = width(k) as u16;
            let lw = width(label) as u16;
            buf.set_string(x, by, k, Style::new().fg(theme::PANEL).bg(c).add_modifier(Modifier::BOLD));
            buf.set_string(x + kw, by, format!(" {label}"), Style::new().fg(theme::TEXT).bg(theme::PANEL_HOVER));
            self.buttons.push((by, x, x + kw + lw + 1, act));
            x += kw + lw + 3;
        }
        if queued > 0 {
            let q = format!("+{queued} more pending");
            buf.set_string(inner.x + inner.width.saturating_sub(width(&q) as u16), by, q, Style::new().fg(theme::MUTED).bg(bg));
        }
    }
}

/// Modal body: headline, prominent fields (command / path) and the full pretty-printed input.
fn content(req: &PermissionRequest, iw: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    // Headline.
    lines.push(Line::from(vec![
        Span::styled(req.tool.clone(), theme::bold(theme::WARNING)),
        Span::styled(format!("  {}", text::one_line(&req.purpose)), theme::text()),
    ]));
    lines.push(Line::default());
    // Prominent fields.
    let s = |k: &str| req.input.get(k).and_then(Value::as_str).map(str::to_string);
    if let Some(cmd) = s("command") {
        lines.push(Line::from(Span::styled("command", theme::muted())));
        for l in text::sanitize(&cmd).lines() {
            for wl in text::wrap_hard(&[Span::styled(format!("$ {l}"), theme::bold(theme::TEXT))], iw) {
                lines.push(wl);
            }
        }
        lines.push(Line::default());
    }
    if let Some(p) = s("path") {
        lines.push(Line::from(vec![Span::styled("path  ", theme::muted()), Span::styled(p, theme::bold(theme::INFO))]));
        lines.push(Line::default());
    }
    lines.push(Line::from(Span::styled("input", theme::muted())));
    let pretty = serde_json::to_string_pretty(&req.input).unwrap_or_default();
    for l in text::sanitize(&pretty).lines() {
        for wl in text::wrap_hard(&[Span::styled(l.to_string(), theme::fg(theme::INFO))], iw) {
            lines.push(wl);
        }
    }
    lines
}
