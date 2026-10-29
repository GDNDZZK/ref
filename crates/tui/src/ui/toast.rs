//! Transient notifications (top-right), with fade in/out.

use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ref_proto::ToastLevel;

use super::text::{self, width};
use super::theme;

pub struct Toast {
    pub level: ToastLevel,
    pub text: String,
    pub born: Instant,
    pub ttl: Duration,
}

#[derive(Default)]
pub struct Toasts {
    pub items: Vec<Toast>,
}

const FADE_IN: f32 = 0.15;
const FADE_OUT: f32 = 0.35;

impl Toasts {
    pub fn push(&mut self, level: ToastLevel, text: impl Into<String>) {
        let text = text.into();
        // Replace an identical visible toast instead of stacking duplicates.
        self.items.retain(|t| t.text != text);
        let ttl = match level {
            ToastLevel::Error => Duration::from_secs(5),
            ToastLevel::Warn => Duration::from_secs(4),
            ToastLevel::Info => Duration::from_secs(3),
        };
        self.items.push(Toast { level, text, born: Instant::now(), ttl });
        if self.items.len() > 4 {
            self.items.remove(0);
        }
    }

    pub fn info(&mut self, t: impl Into<String>) {
        self.push(ToastLevel::Info, t)
    }
    pub fn warn(&mut self, t: impl Into<String>) {
        self.push(ToastLevel::Warn, t)
    }
    pub fn error(&mut self, t: impl Into<String>) {
        self.push(ToastLevel::Error, t)
    }

    /// Drops expired toasts; returns true if any toast is visible (needs frames).
    pub fn gc(&mut self) -> bool {
        self.items.retain(|t| t.born.elapsed() < t.ttl);
        !self.items.is_empty()
    }

    /// Whether a fade animation is in progress (needs high frame rate).
    pub fn animating(&self) -> bool {
        self.items.iter().any(|t| {
            let e = t.born.elapsed().as_secs_f32();
            e < FADE_IN || e > t.ttl.as_secs_f32() - FADE_OUT
        })
    }

    /// Time until the next toast expires or starts fading.
    pub fn next_deadline(&self) -> Option<Duration> {
        self.items
            .iter()
            .map(|t| {
                let fade_at = t.ttl.saturating_sub(Duration::from_secs_f32(FADE_OUT));
                fade_at.saturating_sub(t.born.elapsed())
            })
            .min()
    }

    pub fn draw(&self, buf: &mut Buffer, area: Rect) {
        let mut y = area.y + 1;
        let maxw = (area.width as usize).saturating_sub(4).clamp(10, 60);
        for t in self.items.iter().rev() {
            let e = t.born.elapsed().as_secs_f32();
            let ttl = t.ttl.as_secs_f32();
            let alpha = if e < FADE_IN { e / FADE_IN } else if e > ttl - FADE_OUT { ((ttl - e) / FADE_OUT).max(0.0) } else { 1.0 };
            let (accent, label) = match t.level {
                ToastLevel::Info => (theme::INFO, "●"),
                ToastLevel::Warn => (theme::WARNING, "▲"),
                ToastLevel::Error => (theme::ERROR, "✗"),
            };
            let lines = text::wrap_spans(&[ratatui::text::Span::raw(text::one_line(&t.text))], maxw.saturating_sub(4));
            let lines: Vec<String> = lines.iter().take(4).map(text::line_text).collect();
            let inner_w = lines.iter().map(|l| width(l)).max().unwrap_or(0) + 4;
            // Slide in from the right during the fade-in.
            let slide = ((1.0 - alpha.min(1.0)) * 4.0).round() as u16;
            let w = inner_w as u16 + 2;
            let x = (area.x + area.width).saturating_sub(w + 1) + slide.min(3);
            let h = lines.len() as u16;
            if y + h > area.y + area.height {
                break;
            }
            let bg = theme::blend(Color::Rgb(10, 10, 10), theme::PANEL, alpha);
            let fg = theme::blend(theme::PANEL, theme::TEXT, alpha);
            let ac = theme::blend(theme::PANEL, accent, alpha);
            for (i, l) in lines.iter().enumerate() {
                let yy = y + i as u16;
                let row = Rect::new(x, yy, w.min(area.x + area.width - x), 1);
                buf.set_style(row, Style::new().bg(bg));
                for xx in row.x..row.x + row.width {
                    if let Some(c) = buf.cell_mut((xx, yy)) {
                        c.set_symbol(" ");
                    }
                }
                buf.set_string(x, yy, "▌", Style::new().fg(ac).bg(bg));
                if i == 0 {
                    buf.set_string(x + 2, yy, label, Style::new().fg(ac).bg(bg));
                }
                buf.set_stringn(x + 4, yy, l, inner_w.saturating_sub(4), Style::new().fg(fg).bg(bg));
            }
            y += h + 1;
        }
    }
}
