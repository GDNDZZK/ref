//! Small span/line helpers shared by renderers.

use crate::text;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;

/// Total display width of spans.
pub fn spans_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|s| s.width()).sum()
}

/// Truncate spans to `max` columns, adding a `…` (in the last span's style) when cut.
pub fn fit_spans(spans: Vec<Span<'static>>, max: usize) -> Vec<Span<'static>> {
    if spans_width(&spans) <= max {
        return spans;
    }
    let mut out = Vec::new();
    let mut w = 0;
    let limit = max.saturating_sub(1);
    for s in spans {
        let sw = s.width();
        if w + sw <= limit {
            w += sw;
            out.push(s);
            continue;
        }
        let mut cut = String::new();
        for g in s.content.graphemes(true) {
            let gw = text::gwidth(g);
            if w + gw > limit {
                break;
            }
            cut.push_str(g);
            w += gw;
        }
        let style = s.style;
        if !cut.is_empty() {
            out.push(Span::styled(cut, style));
        }
        if max > 0 {
            out.push(Span::styled("…", style));
        }
        break;
    }
    out
}

/// Compose `left` and `right` spans on one line of `width` columns, padding the gap
/// with `fill`. The left side is truncated if needed; returns the line and the
/// starting column of the right part.
pub fn left_right(
    left: Vec<Span<'static>>,
    right: Vec<Span<'static>>,
    width: usize,
    fill: Style,
) -> (Line<'static>, usize) {
    let rw = spans_width(&right);
    let room = width.saturating_sub(rw + if rw > 0 { 1 } else { 0 });
    // Keep at least two columns between a truncated left side and the right side.
    let room = if rw > 0 && spans_width(&left) > room { room.saturating_sub(1) } else { room };
    let left = fit_spans(left, room);
    let lw = spans_width(&left);
    let mut spans = left;
    let gap = width.saturating_sub(lw + rw);
    if gap > 0 {
        spans.push(Span::styled(" ".repeat(gap), fill));
    }
    let rx = lw + gap;
    spans.extend(right);
    (Line::from(spans), rx)
}

/// Apply a background color to all spans of a line (keeping their fg) and pad to `width`.
pub fn with_bg(mut line: Line<'static>, width: usize, bg: ratatui::style::Color) -> Line<'static> {
    for s in line.spans.iter_mut() {
        s.style = s.style.bg(bg);
    }
    text::pad_line(line, width, Style::new().bg(bg))
}
