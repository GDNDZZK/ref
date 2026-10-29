//! Text selection inside the transcript (own implementation; nothing is auto-copied).

use unicode_segmentation::UnicodeSegmentation;

use super::Row;
use super::text::gwidth;

/// Position in content coordinates: (global row, column).
pub type Pos = (usize, u16);

#[derive(Debug, Clone, Default)]
pub struct Selection {
    pub anchor: Pos,
    pub head: Pos,
    /// Mouse button is held.
    pub dragging: bool,
    /// Something has actually been selected (moved / word / line).
    pub active: bool,
}

impl Selection {
    pub fn range(&self) -> (Pos, Pos) {
        if self.anchor <= self.head { (self.anchor, self.head) } else { (self.head, self.anchor) }
    }
}

/// Graphemes of a row with their starting column.
fn cells(row: &Row) -> Vec<(u16, &str)> {
    let mut out = Vec::new();
    let mut x = 0u16;
    for sp in &row.line.spans {
        for g in sp.content.graphemes(true) {
            let w = gwidth(g) as u16;
            if w == 0 {
                continue;
            }
            out.push((x, g));
            x = x.saturating_add(w);
        }
    }
    out
}

/// Text of `row` between columns `from..=to` (copyable part only), trailing spaces trimmed.
fn row_text(row: &Row, from: u16, to: u16) -> String {
    if row.decor {
        return String::new();
    }
    let lo = from.max(row.skip);
    let hi = match row.end {
        Some(e) if e > 0 => to.min(e.saturating_sub(1)),
        Some(_) => return String::new(),
        None => to,
    };
    let mut s = String::new();
    for (x, g) in cells(row) {
        if x >= lo && x <= hi {
            s.push_str(g);
        }
    }
    s.trim_end().to_string()
}

/// Extracts selected text. `get(i)` returns global row `i`.
pub fn extract<'a>(sel: &Selection, get: impl Fn(usize) -> Option<&'a Row>) -> String {
    let ((r0, c0), (r1, c1)) = sel.range();
    let mut out = String::new();
    let mut first = true;
    for r in r0..=r1 {
        let Some(row) = get(r) else { continue };
        let from = if r == r0 { c0 } else { 0 };
        let to = if r == r1 { c1 } else { u16::MAX };
        let t = row_text(row, from, to);
        if row.decor {
            continue;
        }
        if !first {
            if row.soft {
                // Soft-wrapped continuation: join; restore the space consumed by wrapping.
                if !out.ends_with(' ') && !t.is_empty() && needs_space(&out, &t) {
                    out.push(' ');
                }
            } else {
                out.push('\n');
            }
        }
        first = false;
        out.push_str(&t);
    }
    // Collapse leading/trailing blank lines.
    out.trim_matches('\n').to_string()
}

fn needs_space(prev: &str, next: &str) -> bool {
    let a = prev.chars().last();
    let b = next.chars().next();
    match (a, b) {
        (Some(a), Some(b)) => !(is_cjk(a) || is_cjk(b)),
        _ => false,
    }
}

fn is_cjk(c: char) -> bool {
    gwidth(c.encode_utf8(&mut [0; 4])) >= 2
}

/// Word boundaries around column `col` (inclusive columns).
pub fn word_at(row: &Row, col: u16) -> (u16, u16) {
    let cs = cells(row);
    let idx = cs.iter().rposition(|(x, _)| *x <= col);
    let Some(i) = idx else { return (col, col) };
    let is_word = |g: &str| g.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.' || c == '/' || is_cjk(c));
    if !is_word(cs[i].1) {
        return (cs[i].0, cs[i].0);
    }
    let mut a = i;
    while a > 0 && is_word(cs[a - 1].1) && cs[a - 1].0 >= row.skip {
        a -= 1;
    }
    let mut b = i;
    while b + 1 < cs.len() && is_word(cs[b + 1].1) {
        b += 1;
    }
    let end = cs[b].0 + gwidth(cs[b].1) as u16 - 1;
    (cs[a].0, end)
}

/// Last column with content in a row.
pub fn row_end(row: &Row) -> u16 {
    let cs = cells(row);
    cs.iter().rev().find(|(_, g)| g.trim() != "").map(|(x, g)| x + gwidth(g) as u16 - 1).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::text::{Line, Span};

    fn row(s: &str) -> Row {
        Row::new(Line::from(Span::raw(s.to_string())))
    }

    #[test]
    fn extract_multi_row() {
        let rows = vec![row("  hello world"), row("  wrapped").soft(true).skip(2), row("  next para")];
        let rows: Vec<Row> = rows.into_iter().map(|r| if r.skip == 0 { r.skip(2) } else { r }).collect();
        let sel = Selection { anchor: (0, 8), head: (2, 5), dragging: false, active: true };
        let t = extract(&sel, |i| rows.get(i));
        assert_eq!(t, "world wrapped\nnext");
    }

    #[test]
    fn decor_and_end_excluded() {
        let rows = [row("text  ⧉ copy").end(4), row("────").decor(), row("more")];
        let sel = Selection { anchor: (0, 0), head: (2, 10), dragging: false, active: true };
        assert_eq!(extract(&sel, |i| rows.get(i)), "text\nmore");
    }

    #[test]
    fn cjk_columns() {
        let rows = [row("你好世界")];
        let sel = Selection { anchor: (0, 2), head: (0, 5), dragging: false, active: true };
        assert_eq!(extract(&sel, |i| rows.get(i)), "好世");
        assert_eq!(word_at(&row("foo bar_baz qux"), 6), (4, 10));
    }
}
