//! Split view: tiling algorithm and tile chrome.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::text::{self, width};
use super::theme;
use crate::state::SessionView;

pub const MIN_TILE_W: u16 = 30;
pub const MIN_TILE_H: u16 = 8;

/// Picks (cols, rows) for `n` tiles in a `w`×`h` cell area: target tile aspect ≈1.6 in pixel terms
/// (cells are ~2:1), with a penalty for empty grid cells.
pub fn grid(n: usize, w: u16, h: u16) -> (usize, usize) {
    if n == 0 {
        return (1, 1);
    }
    let mut best = (1usize, n);
    let mut best_score = f64::MAX;
    for cols in 1..=n {
        let rows = n.div_ceil(cols);
        let tw = w as f64 / cols as f64;
        let th = h as f64 / rows as f64;
        let aspect = tw / (th * 2.0);
        let empty = (cols * rows - n) as f64;
        let mut score = (aspect - 1.6).abs() + empty * 0.35;
        // Tiles below the minimum size are strongly discouraged.
        if tw < MIN_TILE_W as f64 || th < MIN_TILE_H as f64 {
            score += 100.0;
        }
        if score < best_score {
            best_score = score;
            best = (cols, rows);
        }
    }
    best
}

/// Maximum tiles per page that keep tiles at least MIN size.
pub fn per_page(w: u16, h: u16) -> usize {
    let c = (w / MIN_TILE_W).max(1) as usize;
    let r = (h / MIN_TILE_H).max(1) as usize;
    c * r
}

/// Tile rectangles for `n` tiles. The last row's tiles stretch to fill the width.
pub fn layout(n: usize, area: Rect) -> Vec<Rect> {
    if n == 0 || area.width == 0 || area.height == 0 {
        return Vec::new();
    }
    let (cols, rows) = grid(n, area.width, area.height);
    let mut out = Vec::with_capacity(n);
    let mut placed = 0;
    for r in 0..rows {
        let y0 = area.y + (area.height as usize * r / rows) as u16;
        let y1 = area.y + (area.height as usize * (r + 1) / rows) as u16;
        let in_row = (n - placed).min(cols);
        for c in 0..in_row {
            let x0 = area.x + (area.width as usize * c / in_row) as u16;
            let x1 = area.x + (area.width as usize * (c + 1) / in_row) as u16;
            out.push(Rect::new(x0, y0, x1 - x0, y1 - y0));
        }
        placed += in_row;
    }
    out
}

/// Nearest tile from `from` in direction (dx, dy).
pub fn neighbor(rects: &[Rect], from: usize, dx: i32, dy: i32) -> Option<usize> {
    let a = rects.get(from)?;
    let (ax, ay) = (
        a.x as i32 + a.width as i32 / 2,
        a.y as i32 + a.height as i32 / 2,
    );
    let mut best: Option<(i64, usize)> = None;
    for (i, r) in rects.iter().enumerate() {
        if i == from {
            continue;
        }
        let (bx, by) = (
            r.x as i32 + r.width as i32 / 2,
            r.y as i32 + r.height as i32 / 2,
        );
        let (ddx, ddy) = (bx - ax, by - ay);
        let ok = (dx > 0 && ddx > 0)
            || (dx < 0 && ddx < 0)
            || (dy > 0 && ddy > 0)
            || (dy < 0 && ddy < 0);
        if !ok {
            continue;
        }
        // Penalise off-axis distance.
        let d = if dx != 0 {
            (ddx.abs() as i64) + 3 * ddy.abs() as i64
        } else {
            (ddy.abs() as i64) * 2 + 3 * ddx.abs() as i64
        };
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, i));
        }
    }
    best.map(|b| b.1)
}

fn status_text(v: &SessionView, tick: u64) -> (String, ratatui::style::Color) {
    use ref_proto::SessionStatus::*;
    match &v.info.status {
        Idle => ("idle".into(), theme::MUTED),
        Busy => (format!("{} working", theme::spinner(tick)), theme::PRIMARY),
        Running => (format!("{} running", theme::spinner(tick)), theme::PRIMARY),
        WaitingPermission => ("? permission".into(), theme::WARNING),
        WaitingQuestion => ("? question".into(), theme::WARNING),
        Retrying { attempt, .. } => (
            format!("{} retry #{attempt}", theme::spinner(tick)),
            theme::WARNING,
        ),
        Compacting => (
            format!("{} compacting", theme::spinner(tick)),
            theme::ACCENT,
        ),
    }
}

/// Draws the frame of a tile; returns the body rect (inside borders, above the input row if focused).
pub fn draw_frame(
    buf: &mut Buffer,
    r: Rect,
    v: &SessionView,
    focused: bool,
    tick: u64,
    agents: &[ref_proto::AgentInfo],
) -> Rect {
    if r.width < 4 || r.height < 3 {
        return Rect::default();
    }
    let busy = v.is_busy();
    let bc = if focused {
        theme::PRIMARY
    } else if !v.permissions.is_empty() {
        theme::WARNING
    } else if busy {
        theme::BORDER_ACTIVE
    } else {
        theme::BORDER
    };
    let bst = Style::new().fg(bc);
    let (x0, y0, x1, y1) = (r.x, r.y, r.x + r.width - 1, r.y + r.height - 1);
    for x in x0 + 1..x1 {
        buf.set_string(x, y0, "─", bst);
        buf.set_string(x, y1, "─", bst);
    }
    for y in y0 + 1..y1 {
        buf.set_string(x0, y, "│", bst);
        buf.set_string(x1, y, "│", bst);
    }
    buf.set_string(x0, y0, "╭", bst);
    buf.set_string(x1, y0, "╮", bst);
    buf.set_string(x0, y1, "╰", bst);
    buf.set_string(x1, y1, "╯", bst);
    // Title: name · agent · status · ctx%
    let (st, sc) = status_text(v, tick);
    let ac = theme::agent_color(agents, &v.info.agent);
    let mut x = x0 + 2;
    let maxx = x1.saturating_sub(1);
    let mut put = |s: &str, style: Style, x: &mut u16| {
        if *x >= maxx {
            return;
        }
        let avail = (maxx - *x) as usize;
        let t = text::truncate(s, avail);
        buf.set_string(*x, y0, &t, style);
        *x += width(&t) as u16;
    };
    put(" ", bst, &mut x);
    let name_style = if focused {
        theme::bold(theme::PRIMARY)
    } else {
        theme::bold(theme::TEXT)
    };
    put(&v.info.name, name_style, &mut x);
    put(" · ", theme::muted(), &mut x);
    put(&v.info.agent, theme::fg(ac), &mut x);
    put(" · ", theme::muted(), &mut x);
    put(&st, Style::new().fg(sc), &mut x);
    if let Some(p) = v.context_pct() {
        put(&format!(" · {p}%"), theme::muted(), &mut x);
    }
    // Current todo item, compactly: ` · ◐ Fix bug · 2/5`.
    let avail = (maxx.saturating_sub(x) as usize).saturating_sub(4);
    if let Some(spans) = super::todo::progress_spans(&v.info.todos, avail) {
        put(" · ", theme::muted(), &mut x);
        for s in spans {
            put(&s.content, s.style, &mut x);
        }
    }
    put(" ", bst, &mut x);
    // Title of the conversation on the bottom border.
    if !v.info.title.is_empty() && r.width > 12 {
        let t = text::truncate(&text::one_line(&v.info.title), (r.width - 6) as usize);
        buf.set_string(
            x0 + 2,
            y1,
            format!(" {t} "),
            theme::fg(theme::DIM).add_modifier(Modifier::ITALIC),
        );
    }
    let input_h = if focused { 2 } else { 0 };
    Rect::new(
        x0 + 1,
        y0 + 1,
        r.width - 2,
        (r.height - 2).saturating_sub(input_h),
    )
}

/// Draws the focused tile's input separator; returns the input rect.
pub fn draw_input_sep(buf: &mut Buffer, r: Rect) -> Rect {
    let y = r.y + r.height - 3;
    let st = Style::new().fg(theme::BORDER);
    for x in r.x + 1..r.x + r.width - 1 {
        buf.set_string(x, y, "┄", st);
    }
    buf.set_string(r.x + 1, y + 1, "›", theme::fg(theme::PRIMARY));
    Rect::new(r.x + 3, y + 1, r.width.saturating_sub(5), 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_choices() {
        assert_eq!(grid(1, 200, 50), (1, 1));
        // Two tiles on a wide screen go side by side.
        assert_eq!(grid(2, 200, 50), (2, 1));
        // Two tiles on a tall narrow screen stack.
        assert_eq!(grid(2, 80, 100), (1, 2));
        let (c, r) = grid(4, 200, 60);
        assert_eq!(c * r, 4);
        let (c, r) = grid(5, 200, 60);
        assert!(c * r >= 5);
    }

    #[test]
    fn layout_covers_area() {
        let area = Rect::new(0, 0, 200, 60);
        for n in 1..10 {
            let rs = layout(n, area);
            assert_eq!(rs.len(), n);
            let total: u32 = rs.iter().map(|r| r.width as u32 * r.height as u32).sum();
            assert_eq!(total, 200 * 60, "n={n}");
        }
        assert!(layout(0, area).is_empty());
    }

    #[test]
    fn neighbors() {
        let rs = layout(4, Rect::new(0, 0, 200, 60));
        let (c, _) = grid(4, 200, 60);
        if c == 2 {
            assert_eq!(neighbor(&rs, 0, 1, 0), Some(1));
            assert_eq!(neighbor(&rs, 0, 0, 1), Some(2));
            assert_eq!(neighbor(&rs, 3, -1, 0), Some(2));
        }
        assert_eq!(neighbor(&rs, 0, -1, 0), None);
    }

    #[test]
    fn paging() {
        assert_eq!(per_page(90, 24), 9);
        assert_eq!(per_page(10, 5), 1);
    }
}
