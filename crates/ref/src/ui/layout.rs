//! Layout helpers: watch-mode split grid.

use ratatui::layout::Rect;

pub const MIN_PANE_W: u16 = 40;
pub const MIN_PANE_H: u16 = 8;
/// Preferred pane width/height ratio in cells (cells are ~2.2× taller than wide).
pub const TARGET_ASPECT: f64 = 2.2;

/// How many panes fit at most in `area` with the minimum pane size.
pub fn capacity(area: Rect) -> usize {
    let c = (area.width / MIN_PANE_W).max(1) as usize;
    let r = (area.height / MIN_PANE_H).max(1) as usize;
    c * r
}

/// Choose the column count for `n` panes in `area`.
pub fn choose_cols(n: usize, area: Rect) -> usize {
    if n <= 1 {
        return 1;
    }
    let max_cols = (area.width / MIN_PANE_W).max(1) as usize;
    let max_rows = (area.height / MIN_PANE_H).max(1) as usize;
    let mut best = (f64::MAX, 1usize);
    for c in 1..=n.min(max_cols) {
        let r = n.div_ceil(c);
        if r > max_rows {
            continue;
        }
        let pw = area.width as f64 / c as f64;
        let ph = area.height as f64 / r as f64;
        let aspect = pw / ph;
        let mut cost = (aspect / TARGET_ASPECT).ln().abs();
        // Prefer filled grids: the last row gets widened panes, which distorts their shape a bit.
        let empty = c * r - n;
        cost += 0.15 * empty as f64 / c as f64;
        if cost < best.0 {
            best = (cost, c);
        }
    }
    best.1
}

/// Split `area` into `n` pane rects (row-major). The last row may have fewer panes, which then span the
/// full width. Caller must ensure `n <= capacity(area)` (else extra panes are paged).
pub fn grid(n: usize, area: Rect) -> Vec<Rect> {
    if n == 0 {
        return vec![];
    }
    let c = choose_cols(n, area);
    let r = n.div_ceil(c);
    let mut out = Vec::with_capacity(n);
    let rows = split_even(area.y, area.height, r);
    let mut left = n;
    for (ri, (y, h)) in rows.into_iter().enumerate() {
        let in_row = if ri + 1 == r { left } else { c.min(left) };
        for (x, w) in split_even(area.x, area.width, in_row) {
            out.push(Rect { x, y, width: w, height: h });
        }
        left -= in_row;
    }
    out
}

fn split_even(start: u16, len: u16, k: usize) -> Vec<(u16, u16)> {
    let k = k.max(1) as u16;
    let base = len / k;
    let extra = len % k;
    let mut out = Vec::new();
    let mut pos = start;
    for i in 0..k {
        let l = base + if i < extra { 1 } else { 0 };
        out.push((pos, l));
        pos += l;
    }
    out
}

/// Pages for `n` panes: returns (page_size, page_count).
pub fn pages(n: usize, area: Rect) -> (usize, usize) {
    let cap = capacity(area).max(1);
    (cap, n.div_ceil(cap).max(1))
}

pub fn contains(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

/// A rect centered in `area` with the given size (clamped).
pub fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(w: u16, h: u16) -> Rect {
        Rect { x: 0, y: 0, width: w, height: h }
    }

    #[test]
    fn single_and_pair() {
        assert_eq!(grid(1, area(200, 50)), vec![area(200, 50)]);
        // Wide terminal: two side by side.
        let g = grid(2, area(200, 50));
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].height, 50);
        assert_eq!(g[0].width + g[1].width, 200);
        // Tall narrow terminal: stacked.
        let g = grid(2, area(80, 60));
        assert_eq!(g[0].width, 80);
        assert_eq!(g[0].height + g[1].height, 60);
    }

    #[test]
    fn last_row_widened() {
        let g = grid(3, area(160, 50));
        // 3 panes on 160x50: 2 columns, 2 rows; last row single pane spans full width.
        assert_eq!(g.len(), 3);
        assert_eq!(g[2].width, 160);
        assert_eq!(g[0].width + g[1].width, 160);
        // Covers the whole area without overlap.
        let total: u32 = g.iter().map(|r| r.width as u32 * r.height as u32).sum();
        assert_eq!(total, 160 * 50);
    }

    #[test]
    fn respects_minimum() {
        let a = area(100, 20);
        assert_eq!(capacity(a), 2 * 2);
        for n in 1..=capacity(a) {
            for r in grid(n, a) {
                assert!(r.width >= MIN_PANE_W && r.height >= MIN_PANE_H, "n={n} {r:?}");
            }
        }
        assert_eq!(pages(9, a), (4, 3));
    }

    #[test]
    fn many_panes_cover_area() {
        let a = area(240, 64);
        for n in 1..=capacity(a) {
            let g = grid(n, a);
            assert_eq!(g.len(), n);
            let total: u32 = g.iter().map(|r| r.width as u32 * r.height as u32).sum();
            assert_eq!(total, 240 * 64, "n={n}");
        }
    }
}
