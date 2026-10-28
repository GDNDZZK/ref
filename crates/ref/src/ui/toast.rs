//! Toast notifications (top-right, auto-hide).

use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ref_proto::NoticeLevel;

use super::text::{width, wrap_text};
use super::theme::*;

pub struct Toast {
    pub text: String,
    pub level: NoticeLevel,
    pub until: Instant,
}

#[derive(Default)]
pub struct Toasts {
    pub items: Vec<Toast>,
}

impl Toasts {
    pub fn push(&mut self, level: NoticeLevel, text: impl Into<String>) {
        let text = text.into();
        let secs = if level == NoticeLevel::Error { 6 } else { 3 };
        // Replace identical toast instead of stacking.
        self.items.retain(|t| t.text != text);
        self.items.push(Toast { text, level, until: Instant::now() + Duration::from_secs(secs) });
        if self.items.len() > 4 {
            self.items.remove(0);
        }
    }

    /// Remove expired; returns true if anything changed.
    pub fn gc(&mut self) -> bool {
        let n = self.items.len();
        let now = Instant::now();
        self.items.retain(|t| t.until > now);
        n != self.items.len()
    }

    pub fn draw(&self, buf: &mut Buffer, area: Rect) {
        let mut y = area.y + 1;
        let maxw = (area.width as usize).saturating_sub(4).clamp(10, 60);
        for t in &self.items {
            let col = match t.level {
                NoticeLevel::Info => INFO,
                NoticeLevel::Success => SUCCESS,
                NoticeLevel::Warning => WARNING,
                NoticeLevel::Error => ERROR,
            };
            let lines = wrap_text(&t.text, maxw - 3, text(), &[], &[]);
            let w = lines.iter().map(|l| l.spans.iter().map(|s| width(&s.content)).sum::<usize>()).max().unwrap_or(0) + 4;
            let x = area.x + area.width.saturating_sub(w as u16 + 1);
            let bg = Style::new().bg(ELEMENT);
            for (i, l) in lines.iter().take(6).enumerate() {
                if y >= area.y + area.height {
                    return;
                }
                let mut spans = vec![Span::styled("┃ ", Style::new().fg(col).bg(ELEMENT))];
                spans.extend(l.spans.iter().map(|s| Span::styled(s.content.clone(), s.style.bg(ELEMENT))));
                let line = super::text::pad_line(Line::from(spans), w, bg);
                buf.set_line(x, y, &line, w as u16);
                let _ = i;
                y += 1;
            }
            y += 1;
        }
    }
}
