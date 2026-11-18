//! Transient notifications (top-right), with fade in/out.

use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ref_proto::ToastLevel;

use super::theme;
use crate::text::{self, width};

pub struct Toast {
    pub level: ToastLevel,
    pub text: String,
    born: Instant,
    ttl: Duration,
}

#[derive(Default)]
pub struct Toasts {
    pub items: Vec<Toast>,
}

const FADE_IN: f32 = 0.15;
const FADE_OUT: f32 = 0.35;
/// How long a toast of each level stays.
const TTL_ERROR: Duration = Duration::from_secs(5);
const TTL_WARN: Duration = Duration::from_secs(4);
const TTL_INFO: Duration = Duration::from_secs(3);
/// Most toasts shown at once, and most lines of one toast.
const MAX_TOASTS: usize = 4;
const MAX_TOAST_LINES: usize = 4;
/// Maximum width of a toast.
const MAX_TOAST_W: usize = 60;
/// Columns a toast slides in from the right during the fade-in.
const SLIDE_COLS: f32 = 4.0;
/// Background the toast fades in from.
const TOAST_SHADOW: Color = Color::Rgb(10, 10, 10);
/// Columns before the text: accent bar, space, label, space.
const TOAST_TEXT_INSET: usize = 4;

impl Toasts {
    pub fn push(&mut self, level: ToastLevel, text: impl Into<String>) {
        let text = text.into();
        // Replace an identical visible toast instead of stacking duplicates.
        self.items.retain(|t| t.text != text);
        let ttl = match level {
            ToastLevel::Error => TTL_ERROR,
            ToastLevel::Warn => TTL_WARN,
            ToastLevel::Info => TTL_INFO,
        };
        self.items.push(Toast {
            level,
            text,
            born: Instant::now(),
            ttl,
        });
        if self.items.len() > MAX_TOASTS {
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

    /// Drops expired toasts.
    pub fn gc(&mut self) {
        self.items.retain(|t| t.born.elapsed() < t.ttl);
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
        if area.width < 6 || area.height < 2 {
            return;
        }
        let right = area.x.saturating_add(area.width);
        let bottom = area.y.saturating_add(area.height);
        let mut y = area.y + 1;
        let maxw = (area.width as usize).saturating_sub(2).min(MAX_TOAST_W);
        if maxw <= TOAST_TEXT_INSET {
            return;
        }
        for t in self.items.iter().rev() {
            let e = t.born.elapsed().as_secs_f32();
            let ttl = t.ttl.as_secs_f32();
            let alpha = if e < FADE_IN {
                e / FADE_IN
            } else if e > ttl - FADE_OUT {
                ((ttl - e) / FADE_OUT).max(0.0)
            } else {
                1.0
            };
            let (accent, label) = match t.level {
                ToastLevel::Info => (theme::INFO, "●"),
                ToastLevel::Warn => (theme::WARNING, "▲"),
                ToastLevel::Error => (theme::ERROR, "✗"),
            };
            let lines = text::wrap_spans(
                &[ratatui::text::Span::raw(text::one_line(&t.text))],
                maxw.saturating_sub(TOAST_TEXT_INSET).max(1),
            );
            let lines: Vec<String> = lines
                .iter()
                .take(MAX_TOAST_LINES)
                .map(text::line_text)
                .collect();
            let inner_w = lines.iter().map(|l| width(l)).max().unwrap_or(0) + TOAST_TEXT_INSET;
            // Slide in from the right during the fade-in.
            let slide = ((1.0 - alpha) * SLIDE_COLS).round() as u16;
            let w = inner_w as u16 + 2;
            let x = right
                .saturating_sub(w.saturating_add(1))
                .saturating_add(slide.min(3))
                .max(area.x)
                .min(right.saturating_sub(1));
            let h = lines.len() as u16;
            if y.saturating_add(h) > bottom {
                break;
            }
            let bg = theme::blend(TOAST_SHADOW, theme::PANEL, alpha);
            let fg = theme::blend(theme::PANEL, theme::TEXT, alpha);
            let ac = theme::blend(theme::PANEL, accent, alpha);
            for (i, l) in lines.iter().enumerate() {
                let yy = y + i as u16;
                let row_w = w.min(right.saturating_sub(x));
                if row_w == 0 {
                    continue;
                }
                let row = Rect::new(x, yy, row_w, 1);
                crate::ui::dialog::clear_wide_edges(buf, row);
                buf.set_style(row, Style::new().bg(bg));
                for xx in row.x..row.x + row.width {
                    if let Some(c) = buf.cell_mut((xx, yy)) {
                        c.set_symbol(" ");
                    }
                }
                buf.set_string(x, yy, "▌", Style::new().fg(ac).bg(bg));
                if i == 0 && x + 2 < right {
                    buf.set_string(x + 2, yy, label, Style::new().fg(ac).bg(bg));
                }
                let tx = x.saturating_add(TOAST_TEXT_INSET as u16);
                if tx < right {
                    let avail = (right - tx) as usize;
                    buf.set_stringn(
                        tx,
                        yy,
                        l,
                        inner_w.saturating_sub(TOAST_TEXT_INSET).min(avail),
                        Style::new().fg(fg).bg(bg),
                    );
                }
            }
            y = y.saturating_add(h + 1);
        }
    }
}
