//! Permission request modal.

use std::time::Instant;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ref_proto::PermissionRequest;
use serde_json::Value;

use super::dialog::{frame, open_progress};
use super::message::styled_diff_lines;
use super::theme;
use crate::text::{self, width};

/// Most diff lines shown inline in the permission modal before a marker row.
const PREVIEW_MAX_LINES: usize = 12;
/// What a permission button answers.
#[derive(Clone, Copy)]
pub enum PermissionAction {
    AllowOnce,
    AllowAlways,
    Deny,
}

pub struct PermissionView {
    pub scroll: usize,
    pub opened: Instant,
    pub call_id: String,
    max_scroll: usize,
    /// Button zones at last draw: (y, x0, x1, action).
    buttons: Vec<(u16, u16, u16, PermissionAction)>,
    pub rect: Rect,
}

impl PermissionView {
    pub fn new(call_id: &str) -> Self {
        PermissionView {
            scroll: 0,
            opened: Instant::now(),
            call_id: call_id.to_string(),
            max_scroll: 0,
            buttons: Vec::new(),
            rect: Rect::default(),
        }
    }

    pub fn scroll_by(&mut self, d: i32) {
        self.scroll = (self.scroll as i32 + d).clamp(0, self.max_scroll as i32) as usize;
    }

    pub fn button_at(&self, x: u16, y: u16) -> Option<PermissionAction> {
        self.buttons
            .iter()
            .find(|(by, x0, x1, _)| *by == y && x >= *x0 && x < *x1)
            .map(|b| b.3)
    }

    pub fn draw(
        &mut self,
        buf: &mut Buffer,
        screen: Rect,
        req: &PermissionRequest,
        session_name: Option<&str>,
        queued: usize,
    ) {
        if screen.width < 6 || screen.height < 4 {
            self.rect = Rect::default();
            self.buttons.clear();
            return;
        }
        let t = open_progress(self.opened);
        let w = screen
            .width
            .saturating_sub(4)
            .clamp(6, 96)
            .min(screen.width);
        let title = match session_name {
            Some(n) => format!("△ Permission required · {n}"),
            None => "△ Permission required".to_string(),
        };
        // Content is laid out for the final (fully opened) width; the frame only grows in.
        let iw = w.saturating_sub(4).max(1) as usize;
        let lines = content(req, iw);
        // Height fits the content (borders + top pad + gap + scroll hint + buttons), capped.
        let max_h = screen
            .height
            .saturating_sub(2)
            .clamp(4, 36)
            .min(screen.height);
        let h = (lines.len() as u16 + 5).clamp(4, max_h).min(screen.height);
        let r = frame(buf, screen, w, h, &title, t);
        self.rect = r;
        if r.width < 6 || r.height < 4 {
            self.buttons.clear();
            return;
        }
        let bg = theme::PANEL;
        let inner = Rect::new(
            r.x + 2,
            r.y + 1,
            r.width.saturating_sub(4),
            r.height.saturating_sub(2),
        );
        let body_h = (inner.height as usize).saturating_sub(3);
        self.max_scroll = lines.len().saturating_sub(body_h);
        self.scroll = self.scroll.min(self.max_scroll);
        for (i, l) in lines.iter().skip(self.scroll).take(body_h).enumerate() {
            let mut l = l.clone();
            for s in &mut l.spans {
                if s.style.bg.is_none() {
                    s.style = s.style.bg(bg);
                }
            }
            buf.set_line(inner.x, inner.y + i as u16, &l, inner.width);
        }
        if self.max_scroll > 0 && body_h > 0 {
            super::chat::draw_scrollbar(
                buf,
                Rect::new(r.x + r.width - 2, inner.y, 1, body_h as u16),
                lines.len(),
                self.scroll as f64,
                self.max_scroll as f64,
                ("┃", theme::BORDER_ACTIVE),
                Some(("│", theme::BORDER_SUBTLE)),
            );
        }
        if self.max_scroll > 0 && inner.height >= 2 && inner.width > 0 {
            let more = text::truncate(
                &format!("↑↓ scroll ({}/{})", self.scroll, self.max_scroll),
                inner.width as usize,
            );
            let my = inner.y + body_h as u16;
            if my < inner.y + inner.height.saturating_sub(1) {
                buf.set_string(
                    inner.x + inner.width.saturating_sub(width(&more) as u16),
                    my,
                    more,
                    Style::new().fg(theme::DIM).bg(bg),
                );
            }
        }
        // Adaptive 3-tier buttons fitting narrow widths without overflow.
        let by = inner.y + inner.height.saturating_sub(1);
        self.buttons.clear();
        if inner.width == 0 {
            return;
        }
        let inner_right = inner.x + inner.width;
        const PERM_FULL_BTNS_MIN_W: u16 = 45;
        const PERM_SHORT_BTNS_MIN_W: u16 = 31;
        let (btns, gap): (
            [(&str, &str, PermissionAction, ratatui::style::Color); 3],
            u16,
        ) = if inner.width >= PERM_FULL_BTNS_MIN_W {
            (
                [
                    (
                        " a ",
                        " Allow once ",
                        PermissionAction::AllowOnce,
                        theme::SUCCESS,
                    ),
                    (
                        " s ",
                        " Always allow ",
                        PermissionAction::AllowAlways,
                        theme::INFO,
                    ),
                    (" d ", " Deny ", PermissionAction::Deny, theme::ERROR),
                ],
                2,
            )
        } else if inner.width >= PERM_SHORT_BTNS_MIN_W {
            (
                [
                    (" a ", " Once ", PermissionAction::AllowOnce, theme::SUCCESS),
                    (
                        " s ",
                        " Always ",
                        PermissionAction::AllowAlways,
                        theme::INFO,
                    ),
                    (" d ", " Deny ", PermissionAction::Deny, theme::ERROR),
                ],
                1,
            )
        } else {
            (
                [
                    (" a ", "", PermissionAction::AllowOnce, theme::SUCCESS),
                    (" s ", "", PermissionAction::AllowAlways, theme::INFO),
                    (" d ", "", PermissionAction::Deny, theme::ERROR),
                ],
                1,
            )
        };
        let mut x = inner.x;
        for (k, label, act, c) in btns {
            let kw = width(k) as u16;
            let lw = width(label) as u16;
            if x.saturating_add(kw) > inner_right {
                break;
            }
            buf.set_string(
                x,
                by,
                k,
                Style::new()
                    .fg(theme::PANEL)
                    .bg(c)
                    .add_modifier(Modifier::BOLD),
            );
            let mut end_x = x + kw;
            if lw > 0 && end_x < inner_right {
                let avail = (inner_right - end_x) as usize;
                let fit = text::truncate(label, avail);
                let fw = width(&fit) as u16;
                buf.set_string(
                    end_x,
                    by,
                    fit,
                    Style::new().fg(theme::TEXT).bg(theme::PANEL_HOVER),
                );
                end_x += fw;
            }
            self.buttons.push((by, x, end_x, act));
            x = end_x.saturating_add(gap);
        }
        if queued > 0 {
            let q = format!("+{queued} more pending");
            let qw = width(&q) as u16;
            if x.saturating_add(qw) <= inner_right {
                buf.set_string(
                    inner_right - qw,
                    by,
                    q,
                    Style::new().fg(theme::MUTED).bg(bg),
                );
            }
        }
    }
}

/// Modal body: headline, prominent fields (command / path) and the full pretty-printed input.
fn content(req: &PermissionRequest, iw: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    // Headline (wrapped for narrow terminals).
    lines.extend(text::wrap_spans(
        &[
            Span::styled(req.tool.clone(), theme::bold(theme::WARNING)),
            Span::styled(format!("  {}", text::one_line(&req.purpose)), theme::text()),
        ],
        iw.max(1),
    ));
    lines.push(Line::default());
    // Prominent fields.
    let s = |k: &str| req.input.get(k).and_then(Value::as_str).map(str::to_string);
    if let Some(cmd) = s("command") {
        lines.push(Line::from(Span::styled("command", theme::muted())));
        for l in text::sanitize(&cmd).lines() {
            for wl in text::wrap_hard(
                &[Span::styled(format!("$ {l}"), theme::bold(theme::TEXT))],
                iw.max(1),
            ) {
                lines.push(wl);
            }
        }
        lines.push(Line::default());
    }
    if let Some(p) = s("path") {
        lines.extend(text::wrap_hard(
            &[
                Span::styled("path  ", theme::muted()),
                Span::styled(text::one_line(&p), theme::bold(theme::INFO)),
            ],
            iw.max(1),
        ));
        lines.push(Line::default());
    }
    if let Some(preview) = &req.preview {
        let mut hdr = vec![
            Span::styled("preview", theme::muted()),
            Span::styled(
                format!("  +{}", preview.added),
                theme::fg(theme::DIFF_ADDED),
            ),
            Span::styled(
                format!(" -{}", preview.removed),
                theme::fg(theme::DIFF_REMOVED),
            ),
        ];
        if preview.truncated {
            hdr.push(Span::styled(" (preview cut)", theme::fg(theme::WARNING)));
        }
        lines.push(Line::from(hdr));
        if !preview.diff.trim().is_empty() {
            let (styled, hidden) = styled_diff_lines(&preview.diff, iw, Some(PREVIEW_MAX_LINES));
            for (spans, _) in styled {
                lines.push(Line::from(spans));
            }
            if hidden > 0 {
                let mut marker = format!("… {hidden} more lines");
                if preview.truncated {
                    marker.push_str(" (preview cut)");
                }
                lines.push(Line::from(Span::styled(marker, theme::fg(theme::WARNING))));
            }
        }
        lines.push(Line::default());
    }
    lines.push(Line::from(Span::styled("input", theme::muted())));
    let pretty = serde_json::to_string_pretty(&req.input).expect("a JSON value always serialises");
    for l in text::sanitize(&pretty).lines() {
        for wl in text::wrap_hard(&[Span::styled(l.to_string(), theme::fg(theme::INFO))], iw) {
            lines.push(wl);
        }
    }
    lines
}
