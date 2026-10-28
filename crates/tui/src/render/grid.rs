//! Tiled pane layout for background monitor mode.

use ratatui::layout::Rect;

/// Minimum comfortable pane size.
pub const MIN_W: u16 = 60;
pub const MIN_H: u16 = 8;

/// Pane rectangles for `n` panes in `area`, plus how many panes fit per page.
/// Layout rules:
/// - 1 → full area
/// - 2 → side by side when width ≥ 120, else stacked
/// - 3 → one big left + two stacked right when wide enough, else a column/row
/// - otherwise a cols×rows grid chosen to minimize wasted cells while keeping
///   panes ≥ MIN_W × MIN_H (as far as the terminal allows) and close to a
///   pleasant aspect ratio. Panes in the last row stretch to fill the width.
pub fn layout(n: usize, area: Rect) -> Vec<Rect> {
    if n == 0 || area.width == 0 || area.height == 0 {
        return Vec::new();
    }
    let (w, h) = (area.width, area.height);
    let max_cols = (w / MIN_W).max(1) as usize;
    let max_rows = (h / MIN_H).max(1) as usize;
    let cap = max_cols * max_rows;
    let n = n.min(cap.max(1));
    match n {
        1 => return vec![area],
        2 => {
            return if w >= 2 * MIN_W { split_cols(area, 2) } else if h >= 2 * MIN_H { split_rows(area, 2) } else {
                split_cols(area, 2)
            }
        }
        3 if w >= 2 * MIN_W && h >= 2 * MIN_H => {
            let left_w = w / 2;
            let left = Rect { width: left_w, ..area };
            let right = Rect { x: area.x + left_w, width: w - left_w, ..area };
            let mut v = vec![left];
            v.extend(split_rows(right, 2));
            return v;
        }
        _ => {}
    }
    let (cols, rows) = choose_grid(n, w, h);
    grid(area, n, cols, rows)
}

/// How many panes fit on one page in `area`.
pub fn capacity(area: Rect) -> usize {
    ((area.width / MIN_W).max(1) as usize) * ((area.height / MIN_H).max(1) as usize)
}

/// Pick (cols, rows) for n panes. Exposed for tests.
pub fn choose_grid(n: usize, w: u16, h: u16) -> (usize, usize) {
    let max_cols = (w / MIN_W).max(1) as usize;
    let max_rows = (h / MIN_H).max(1) as usize;
    let mut best = (1usize, n.max(1));
    let mut best_score = f64::MAX;
    for cols in 1..=max_cols.min(n) {
        let rows = n.div_ceil(cols);
        if rows > max_rows {
            continue;
        }
        let pw = w as f64 / cols as f64;
        let ph = h as f64 / rows as f64;
        // Terminal cells are ~2x taller than wide; ideal pane aspect (w/h in cells) ≈ 3.
        let aspect = pw / ph;
        let aspect_pen = (aspect / 3.0).ln().abs();
        let waste = (cols * rows - n) as f64 / (cols * rows) as f64;
        let score = aspect_pen + waste * 1.5;
        if score < best_score {
            best_score = score;
            best = (cols, rows);
        }
    }
    if best_score == f64::MAX {
        // Nothing satisfies the minimums: maximal grid.
        let cols = max_cols.min(n).max(1);
        best = (cols, n.div_ceil(cols));
    }
    best
}

fn split_even(total: u16, parts: usize) -> Vec<(u16, u16)> {
    let parts = parts.max(1) as u16;
    let base = total / parts;
    let extra = total % parts;
    let mut out = Vec::new();
    let mut pos = 0;
    for i in 0..parts {
        let len = base + if i < extra { 1 } else { 0 };
        out.push((pos, len));
        pos += len;
    }
    out
}

fn split_cols(area: Rect, n: usize) -> Vec<Rect> {
    split_even(area.width, n).into_iter().map(|(x, w)| Rect { x: area.x + x, width: w, ..area }).collect()
}

fn split_rows(area: Rect, n: usize) -> Vec<Rect> {
    split_even(area.height, n).into_iter().map(|(y, h)| Rect { y: area.y + y, height: h, ..area }).collect()
}

fn grid(area: Rect, n: usize, cols: usize, rows: usize) -> Vec<Rect> {
    let mut out = Vec::new();
    let row_spans = split_even(area.height, rows);
    let mut left = n;
    for (y, rh) in row_spans {
        let in_row = left.min(cols);
        if in_row == 0 {
            break;
        }
        for (x, cw) in split_even(area.width, in_row) {
            out.push(Rect { x: area.x + x, y: area.y + y, width: cw, height: rh });
        }
        left -= in_row;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(w: u16, h: u16) -> Rect {
        Rect::new(0, 0, w, h)
    }

    fn covers(rects: &[Rect], a: Rect) -> bool {
        let total: u32 = rects.iter().map(|r| r.width as u32 * r.height as u32).sum();
        // no overlaps
        for (i, r) in rects.iter().enumerate() {
            for s in &rects[i + 1..] {
                if r.intersects(*s) {
                    return false;
                }
            }
        }
        total == a.width as u32 * a.height as u32
    }

    #[test]
    fn one_and_two() {
        assert_eq!(layout(1, area(100, 40)), vec![area(100, 40)]);
        let v = layout(2, area(200, 50));
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].height, 50);
        assert_eq!(v[0].width, 100);
        let v = layout(2, area(100, 50));
        assert_eq!(v[0].width, 100);
        assert_eq!(v[0].height, 25);
    }

    #[test]
    fn three_big_left() {
        let a = area(200, 50);
        let v = layout(3, a);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].height, 50);
        assert_eq!(v[1].x, 100);
        assert_eq!(v[1].height + v[2].height, 50);
        assert!(covers(&v, a));
    }

    #[test]
    fn four_is_two_by_two() {
        let a = area(200, 50);
        assert_eq!(choose_grid(4, 200, 50), (2, 2));
        let v = layout(4, a);
        assert!(covers(&v, a));
        assert!(v.iter().all(|r| r.width >= MIN_W && r.height >= MIN_H));
    }

    #[test]
    fn many_panes_respect_minimums_and_cover() {
        for (n, w, h) in [(5, 240, 60), (6, 240, 60), (7, 300, 80), (9, 200, 60)] {
            let a = area(w, h);
            let v = layout(n, a);
            assert!(v.len() <= n);
            assert!(covers(&v, a), "n={n} {w}x{h}");
            for r in &v {
                assert!(r.width >= MIN_W && r.height >= MIN_H, "n={n} {r:?}");
            }
        }
    }

    #[test]
    fn overflow_is_capped_to_capacity() {
        let a = area(120, 30); // 2 cols x 3 rows max
        assert_eq!(capacity(a), 6);
        assert_eq!(layout(10, a).len(), 6);
    }

    #[test]
    fn tiny_terminal_still_lays_out() {
        let v = layout(3, area(50, 10));
        assert_eq!(v.len(), 1); // only one pane fits; rest paginated
    }
}
