//! Multi-line prompt editor with display-width aware cursor math and atomic chips
//! (`[Image N]`, `[Pasted ~N lines]`).

use crate::text::gwidth;
use hn_proto::ImageData;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone)]
pub enum ChipKind {
    Image(ImageData),
    /// Collapsed large paste; expanded on submit.
    Paste(String),
}

#[derive(Debug, Clone)]
pub struct Chip {
    pub label: String,
    pub kind: ChipKind,
}

#[derive(Debug, Default, Clone)]
pub struct Editor {
    text: String,
    /// Byte offset, always on a grapheme boundary.
    cursor: usize,
    pub chips: Vec<Chip>,
    image_seq: usize,
    undo: Vec<(String, usize)>,
    /// Desired display column for vertical movement.
    goal_col: Option<usize>,
}

/// A visual (wrapped) row: byte range into the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    pub start: usize,
    pub end: usize,
}

impl Editor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }


    pub fn set_text(&mut self, t: &str) {
        self.snapshot();
        self.text = t.to_string();
        self.cursor = self.text.len();
        self.chips.retain(|c| t.contains(&c.label));
        self.goal_col = None;
    }

    pub fn clear(&mut self) {
        self.snapshot();
        self.text.clear();
        self.cursor = 0;
        self.chips.clear();
        self.goal_col = None;
    }

    fn snapshot(&mut self) {
        if self.undo.last().map(|(t, _)| t != &self.text).unwrap_or(true) {
            self.undo.push((self.text.clone(), self.cursor));
            if self.undo.len() > 200 {
                self.undo.remove(0);
            }
        }
    }

    pub fn undo(&mut self) {
        if let Some((t, c)) = self.undo.pop() {
            self.text = t;
            self.cursor = c.min(self.text.len());
            self.goal_col = None;
        }
    }

    // --- chips --------------------------------------------------------------

    /// Ranges of chips present in the text.
    fn chip_ranges(&self) -> Vec<(usize, usize)> {
        let mut v = Vec::new();
        for c in &self.chips {
            if let Some(p) = self.text.find(&c.label) {
                v.push((p, p + c.label.len()));
            }
        }
        v
    }

    /// Chip label ranges, for styling.
    pub fn chip_spans(&self) -> Vec<(usize, usize)> {
        self.chip_ranges()
    }

    pub fn add_image(&mut self, img: ImageData) {
        self.image_seq += 1;
        let label = format!("[Image {}]", self.image_seq);
        self.chips.push(Chip { label: label.clone(), kind: ChipKind::Image(img) });
        self.insert_str(&format!("{label} "));
    }

    pub fn add_paste(&mut self, content: String) {
        let lines = content.lines().count().max(1);
        let mut label = format!("[Pasted ~{lines} lines]");
        // Keep labels unique.
        let mut n = 2;
        while self.chips.iter().any(|c| c.label == label) {
            label = format!("[Pasted ~{lines} lines #{n}]");
            n += 1;
        }
        self.chips.push(Chip { label: label.clone(), kind: ChipKind::Paste(content) });
        self.insert_str(&format!("{label} "));
    }

    /// Final text (pastes expanded) and images to send.
    pub fn take_submission(&mut self) -> (String, Vec<ImageData>) {
        let mut text = self.text.clone();
        let mut images = Vec::new();
        for c in &self.chips {
            match &c.kind {
                ChipKind::Paste(content) => text = text.replacen(&c.label, content, 1),
                ChipKind::Image(img) => {
                    if text.contains(&c.label) {
                        images.push(img.clone());
                    }
                }
            }
        }
        self.clear();
        self.undo.clear();
        self.image_seq = 0;
        (text.trim_end().to_string(), images)
    }

    // --- editing ------------------------------------------------------------

    pub fn insert_str(&mut self, s: &str) {
        self.snapshot_if_word_boundary(s);
        let s = s.replace("\r\n", "\n").replace('\r', "\n").replace('\t', "    ");
        self.text.insert_str(self.cursor, &s);
        self.cursor += s.len();
        self.goal_col = None;
    }

    fn snapshot_if_word_boundary(&mut self, s: &str) {
        if s.len() > 1 || s.chars().any(|c| c.is_whitespace()) || self.undo.is_empty() {
            self.snapshot();
        }
    }

    pub fn insert_char(&mut self, c: char) {
        let mut b = [0u8; 4];
        self.insert_str(c.encode_utf8(&mut b));
    }

    pub fn newline(&mut self) {
        self.insert_str("\n");
    }

    fn prev_boundary(&self, pos: usize) -> usize {
        for (s, e) in self.chip_ranges() {
            if pos > s && pos <= e {
                return s;
            }
        }
        self.text[..pos].grapheme_indices(true).next_back().map(|(i, _)| i).unwrap_or(0)
    }

    fn next_boundary(&self, pos: usize) -> usize {
        for (s, e) in self.chip_ranges() {
            if pos >= s && pos < e {
                return e;
            }
        }
        self.text[pos..].graphemes(true).next().map(|g| pos + g.len()).unwrap_or(pos)
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.snapshot();
        let p = self.prev_boundary(self.cursor);
        self.text.replace_range(p..self.cursor, "");
        self.cursor = p;
        self.gc_chips();
        self.goal_col = None;
    }

    pub fn delete(&mut self) {
        if self.cursor >= self.text.len() {
            return;
        }
        self.snapshot();
        let n = self.next_boundary(self.cursor);
        self.text.replace_range(self.cursor..n, "");
        self.gc_chips();
        self.goal_col = None;
    }

    fn gc_chips(&mut self) {
        let text = &self.text;
        self.chips.retain(|c| text.contains(&c.label));
    }

    pub fn left(&mut self) {
        self.cursor = self.prev_boundary(self.cursor);
        self.goal_col = None;
    }

    pub fn right(&mut self) {
        self.cursor = self.next_boundary(self.cursor);
        self.goal_col = None;
    }

    fn line_start(&self) -> usize {
        self.text[..self.cursor].rfind('\n').map(|i| i + 1).unwrap_or(0)
    }

    fn line_end(&self) -> usize {
        self.text[self.cursor..].find('\n').map(|i| self.cursor + i).unwrap_or(self.text.len())
    }

    pub fn home(&mut self) {
        self.cursor = self.line_start();
        self.goal_col = None;
    }

    pub fn end(&mut self) {
        self.cursor = self.line_end();
        self.goal_col = None;
    }

    fn word_left_pos(&self) -> usize {
        let before = &self.text[..self.cursor];
        let mut pos = self.cursor;
        let mut seen_word = false;
        for (i, g) in before.grapheme_indices(true).rev() {
            let ws = g.chars().all(|c| c.is_whitespace() || c.is_ascii_punctuation());
            if ws && seen_word {
                break;
            }
            if !ws {
                seen_word = true;
            }
            pos = i;
        }
        pos
    }

    fn word_right_pos(&self) -> usize {
        let after = &self.text[self.cursor..];
        let mut seen_word = false;
        for (i, g) in after.grapheme_indices(true) {
            let ws = g.chars().all(|c| c.is_whitespace() || c.is_ascii_punctuation());
            if ws && seen_word {
                return self.cursor + i;
            }
            if !ws {
                seen_word = true;
            }
        }
        self.text.len()
    }

    pub fn word_left(&mut self) {
        self.cursor = self.word_left_pos();
        self.goal_col = None;
    }

    pub fn word_right(&mut self) {
        self.cursor = self.word_right_pos();
        self.goal_col = None;
    }

    pub fn delete_word_back(&mut self) {
        let p = self.word_left_pos();
        if p < self.cursor {
            self.snapshot();
            self.text.replace_range(p..self.cursor, "");
            self.cursor = p;
            self.gc_chips();
        }
    }

    pub fn delete_word_forward(&mut self) {
        let p = self.word_right_pos();
        if p > self.cursor {
            self.snapshot();
            self.text.replace_range(self.cursor..p, "");
            self.gc_chips();
        }
    }

    pub fn kill_to_end(&mut self) {
        let e = self.line_end();
        self.snapshot();
        if e == self.cursor && e < self.text.len() {
            self.text.remove(e); // join with next line
        } else {
            self.text.replace_range(self.cursor..e, "");
        }
        self.gc_chips();
    }

    pub fn kill_to_start(&mut self) {
        let s = self.line_start();
        if s < self.cursor {
            self.snapshot();
            self.text.replace_range(s..self.cursor, "");
            self.cursor = s;
            self.gc_chips();
        }
    }

    // --- layout -------------------------------------------------------------

    /// Wrap the text into visual rows of at most `width` columns.
    pub fn rows(&self, width: usize) -> Vec<Row> {
        let width = width.max(2);
        let mut rows = Vec::new();
        let mut line_start = 0;
        for line in self.text.split('\n') {
            let mut start = line_start;
            let mut w = 0;
            let mut last_space: Option<usize> = None;
            for (i, g) in line.grapheme_indices(true) {
                let abs = line_start + i;
                let gw = gwidth(g);
                if w + gw > width && abs > start {
                    // Prefer breaking after the last space in this row.
                    let brk = match last_space {
                        Some(sp) if sp > start => sp,
                        _ => abs,
                    };
                    rows.push(Row { start, end: brk });
                    start = brk;
                    w = crate::text::width(&self.text[start..abs]);
                    last_space = None;
                }
                w += gw;
                if g == " " {
                    last_space = Some(abs + 1);
                }
            }
            rows.push(Row { start, end: line_start + line.len() });
            line_start += line.len() + 1;
        }
        rows
    }

    /// Visual (row, col) of the cursor for the given wrap width.
    pub fn cursor_pos(&self, width: usize) -> (usize, usize) {
        let rows = self.rows(width);
        Self::pos_in(&rows, &self.text, self.cursor)
    }

    fn pos_in(rows: &[Row], text: &str, cursor: usize) -> (usize, usize) {
        for (ri, r) in rows.iter().enumerate() {
            let next_continues = rows.get(ri + 1).is_some_and(|n| n.start == r.end);
            if cursor >= r.start && (cursor < r.end || (cursor == r.end && !next_continues)) {
                return (ri, crate::text::width(&text[r.start..cursor]));
            }
        }
        let last = rows.len().saturating_sub(1);
        (last, rows.last().map(|r| crate::text::width(&text[r.start..r.end])).unwrap_or(0))
    }

    fn byte_at_col(&self, r: Row, col: usize) -> usize {
        let mut w = 0;
        for (i, g) in self.text[r.start..r.end].grapheme_indices(true) {
            let gw = gwidth(g);
            if w + gw > col {
                return r.start + i;
            }
            w += gw;
        }
        r.end
    }

    /// Move the cursor one visual row up. Returns false when already on the first row.
    pub fn up(&mut self, width: usize) -> bool {
        self.vmove(width, -1)
    }

    /// Move the cursor one visual row down. Returns false when already on the last row.
    pub fn down(&mut self, width: usize) -> bool {
        self.vmove(width, 1)
    }

    fn vmove(&mut self, width: usize, dir: i32) -> bool {
        let rows = self.rows(width);
        let (r, c) = Self::pos_in(&rows, &self.text, self.cursor);
        let target = r as i64 + dir as i64;
        if target < 0 || target as usize >= rows.len() {
            return false;
        }
        let goal = *self.goal_col.get_or_insert(c);
        let mut p = self.byte_at_col(rows[target as usize], goal);
        // Snap out of chips.
        for (s, e) in self.chip_ranges() {
            if p > s && p < e {
                p = e;
            }
        }
        self.cursor = p;
        true
    }

    /// Place the cursor at a visual (row, col), e.g. from a mouse click.
    pub fn click(&mut self, width: usize, row: usize, col: usize) {
        let rows = self.rows(width);
        if let Some(r) = rows.get(row.min(rows.len().saturating_sub(1))) {
            self.cursor = self.byte_at_col(*r, col);
            self.goal_col = None;
        }
    }

    /// The word being typed at cursor if it starts with `/` at the very beginning.
    pub fn slash_query(&self) -> Option<&str> {
        let t = &self.text;
        if !t.starts_with('/') || t.contains('\n') {
            return None;
        }
        let first_space = t.find(' ').unwrap_or(t.len());
        if self.cursor > first_space {
            return None;
        }
        Some(&t[1..first_space])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cjk_cursor_columns() {
        let mut e = Editor::new();
        e.insert_str("ab中文");
        assert_eq!(e.cursor_pos(40), (0, 6));
        e.left();
        assert_eq!(e.cursor_pos(40), (0, 4));
        e.left();
        e.left();
        assert_eq!(e.cursor_pos(40), (0, 1));
    }

    #[test]
    fn wrap_rows_and_cursor_at_wrap() {
        let mut e = Editor::new();
        e.insert_str("中文中文中"); // 10 cols
        let rows = e.rows(4);
        assert_eq!(rows.len(), 3);
        // cursor at end -> last row
        assert_eq!(e.cursor_pos(4), (2, 2));
        // cursor exactly at a wrap boundary belongs to the next row
        e.home();
        e.right();
        e.right();
        assert_eq!(e.cursor_pos(4), (1, 0));
    }

    #[test]
    fn word_wrap_prefers_spaces() {
        let mut e = Editor::new();
        e.insert_str("hello world foo");
        let rows = e.rows(8);
        let t = e.text();
        let strs: Vec<&str> = rows.iter().map(|r| &t[r.start..r.end]).collect();
        assert_eq!(strs, vec!["hello ", "world ", "foo"]);
    }

    #[test]
    fn vertical_movement_keeps_column() {
        let mut e = Editor::new();
        e.insert_str("abcdef\n中文\nxyz");
        assert_eq!(e.cursor_pos(40), (2, 3));
        assert!(e.up(40));
        // goal col 3 on "中文" -> inside 2nd char boundary: col 2
        assert_eq!(e.cursor_pos(40), (1, 2));
        assert!(e.up(40));
        assert_eq!(e.cursor_pos(40), (0, 3));
        assert!(!e.up(40));
        assert!(e.down(40));
        assert!(e.down(40));
        assert_eq!(e.cursor_pos(40), (2, 3));
        assert!(!e.down(40));
    }

    #[test]
    fn chips_are_atomic() {
        let mut e = Editor::new();
        e.insert_str("see ");
        e.add_image(ImageData { mime: "image/png".into(), data: "AAAA".into() });
        assert_eq!(e.text(), "see [Image 1] ");
        e.backspace(); // remove trailing space
        e.backspace(); // remove the whole chip
        assert_eq!(e.text(), "see ");
        assert!(e.chips.is_empty());
    }

    #[test]
    fn paste_chip_expands_on_submit() {
        let mut e = Editor::new();
        e.insert_str("look: ");
        e.add_paste("a\nb\nc".into());
        assert!(e.text().contains("[Pasted ~3 lines]"));
        e.left(); // past trailing space
        e.left(); // jumps over chip
        assert_eq!(e.cursor(), "look: ".len());
        let (t, imgs) = e.take_submission();
        assert_eq!(t, "look: a\nb\nc");
        assert!(imgs.is_empty());
        assert!(e.is_empty());
    }

    #[test]
    fn word_ops_and_undo() {
        let mut e = Editor::new();
        e.insert_str("foo bar baz");
        e.delete_word_back();
        assert_eq!(e.text(), "foo bar ");
        e.undo();
        assert_eq!(e.text(), "foo bar baz");
        e.home();
        e.word_right();
        assert_eq!(e.cursor(), 3);
        e.kill_to_end();
        assert_eq!(e.text(), "foo");
    }

    #[test]
    fn slash_query() {
        let mut e = Editor::new();
        e.insert_str("/mod");
        assert_eq!(e.slash_query(), Some("mod"));
        e.insert_str(" x");
        assert_eq!(e.slash_query(), None);
    }
}
