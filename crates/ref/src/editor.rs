//! Multi-line text area with atomic "chips" (collapsed pastes, images), unicode-width aware cursor.

use std::collections::HashMap;

use ref_proto::ImageInput;
use unicode_segmentation::UnicodeSegmentation;

use crate::ui::text::width;

/// Chips are stored inline as single private-use characters so cursor movement treats them atomically.
const CHIP_BASE: u32 = 0xF0000;

#[derive(Debug, Clone)]
pub enum Chip {
    Paste { text: String, lines: usize },
    Image { n: usize, image: ImageInput },
}

#[derive(Debug, Clone, Default)]
pub struct Editor {
    text: String,
    /// Byte offset of the cursor (always on a grapheme boundary).
    cursor: usize,
    chips: HashMap<u32, Chip>,
    next_chip: u32,
    image_count: usize,
    /// Preferred visual column for up/down movement.
    goal_col: Option<usize>,
}

/// A visual row of the wrapped buffer: byte range and starting column offset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Row {
    pub start: usize,
    pub end: usize,
}

pub fn chip_id(c: char) -> Option<u32> {
    let v = c as u32;
    (CHIP_BASE..CHIP_BASE + 0xFFFD).contains(&v).then(|| v - CHIP_BASE)
}

impl Editor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.chips.clear();
        self.image_count = 0;
        self.goal_col = None;
    }

    pub fn set_text(&mut self, s: &str) {
        self.clear();
        self.text = s.replace("\r\n", "\n").replace('\r', "\n");
        self.cursor = self.text.len();
    }

    pub fn chip_label(&self, id: u32) -> String {
        match self.chips.get(&id) {
            Some(Chip::Paste { lines, text }) => {
                if *lines > 1 {
                    format!("[Pasted ~{lines} lines]")
                } else {
                    format!("[Pasted {} chars]", text.chars().count())
                }
            }
            Some(Chip::Image { n, image }) => match &image.name {
                Some(name) => format!("[Image {n}: {name}]"),
                None => format!("[Image {n}]"),
            },
            None => "[?]".into(),
        }
    }

    /// Display width of a grapheme in the editor (chips expand to their label).
    pub fn gwidth(&self, g: &str) -> usize {
        let mut cs = g.chars();
        if let (Some(c), None) = (cs.next(), cs.next()) {
            if let Some(id) = chip_id(c) {
                return width(&self.chip_label(id));
            }
        }
        width(g)
    }

    /// Text to send: chips expanded (pastes inline, images removed), plus images.
    pub fn submission(&self) -> (String, Vec<ImageInput>) {
        let mut out = String::new();
        let mut images = Vec::new();
        for c in self.text.chars() {
            match chip_id(c).and_then(|id| self.chips.get(&id)) {
                Some(Chip::Paste { text, .. }) => out.push_str(text),
                Some(Chip::Image { image, .. }) => images.push(image.clone()),
                None if chip_id(c).is_some() => {}
                None => out.push(c),
            }
        }
        (out.trim_end().to_string(), images)
    }

    /// Plain text with chips replaced by labels (for history / display).
    pub fn display_text(&self) -> String {
        let mut out = String::new();
        for c in self.text.chars() {
            match chip_id(c) {
                Some(id) => out.push_str(&self.chip_label(id)),
                None => out.push(c),
            }
        }
        out
    }

    pub fn insert_str(&mut self, s: &str) {
        let s = s.replace("\r\n", "\n").replace('\r', "\n").replace('\t', "    ");
        self.text.insert_str(self.cursor, &s);
        self.cursor += s.len();
        self.goal_col = None;
    }

    pub fn insert_char(&mut self, c: char) {
        let mut b = [0u8; 4];
        self.insert_str(c.encode_utf8(&mut b));
    }

    fn insert_chip(&mut self, chip: Chip) {
        let id = self.next_chip;
        self.next_chip += 1;
        self.chips.insert(id, chip);
        let c = char::from_u32(CHIP_BASE + id).unwrap_or('?');
        self.insert_char(c);
    }

    /// Insert pasted text; large pastes become a collapsed chip.
    pub fn paste(&mut self, s: &str) {
        let s = s.replace("\r\n", "\n").replace('\r', "\n");
        let lines = s.lines().count();
        if lines > 5 || s.chars().count() > 1000 {
            self.insert_chip(Chip::Paste { text: s, lines });
        } else {
            self.insert_str(&s);
        }
    }

    pub fn add_image(&mut self, image: ImageInput) {
        self.image_count += 1;
        let n = self.image_count;
        self.insert_chip(Chip::Image { n, image });
    }

    pub fn image_count(&self) -> usize {
        self.chips.values().filter(|c| matches!(c, Chip::Image { .. })).count()
    }

    // ── movement ──

    fn prev_boundary(&self, i: usize) -> usize {
        self.text[..i].grapheme_indices(true).next_back().map(|(j, _)| j).unwrap_or(0)
    }

    fn next_boundary(&self, i: usize) -> usize {
        self.text[i..].graphemes(true).next().map(|g| i + g.len()).unwrap_or(self.text.len())
    }

    pub fn left(&mut self) {
        self.cursor = self.prev_boundary(self.cursor);
        self.goal_col = None;
    }

    pub fn right(&mut self) {
        self.cursor = self.next_boundary(self.cursor);
        self.goal_col = None;
    }

    fn line_start(&self, i: usize) -> usize {
        self.text[..i].rfind('\n').map(|p| p + 1).unwrap_or(0)
    }

    fn line_end(&self, i: usize) -> usize {
        self.text[i..].find('\n').map(|p| i + p).unwrap_or(self.text.len())
    }

    pub fn home(&mut self) {
        self.cursor = self.line_start(self.cursor);
        self.goal_col = None;
    }

    pub fn end(&mut self) {
        self.cursor = self.line_end(self.cursor);
        self.goal_col = None;
    }

    pub fn buffer_start(&mut self) {
        self.cursor = 0;
    }

    pub fn buffer_end(&mut self) {
        self.cursor = self.text.len();
    }

    fn is_word(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    fn word_left_pos(&self) -> usize {
        let before = &self.text[..self.cursor];
        let mut it = before.char_indices().rev().peekable();
        // skip non-word
        let mut pos = self.cursor;
        while let Some(&(i, c)) = it.peek() {
            if Self::is_word(c) {
                break;
            }
            pos = i;
            it.next();
        }
        while let Some(&(i, c)) = it.peek() {
            if !Self::is_word(c) {
                break;
            }
            pos = i;
            it.next();
        }
        pos
    }

    fn word_right_pos(&self) -> usize {
        let after = &self.text[self.cursor..];
        let mut pos = self.cursor;
        let mut it = after.char_indices().peekable();
        while let Some(&(_, c)) = it.peek() {
            if Self::is_word(c) {
                break;
            }
            pos += c.len_utf8();
            it.next();
        }
        while let Some(&(_, c)) = it.peek() {
            if !Self::is_word(c) {
                break;
            }
            pos += c.len_utf8();
            it.next();
        }
        pos
    }

    pub fn word_left(&mut self) {
        self.cursor = self.word_left_pos();
        self.goal_col = None;
    }

    pub fn word_right(&mut self) {
        self.cursor = self.word_right_pos();
        self.goal_col = None;
    }

    // ── deletion ──

    fn remove_range(&mut self, a: usize, b: usize) {
        if a >= b {
            return;
        }
        let removed: String = self.text[a..b].to_string();
        for c in removed.chars() {
            if let Some(id) = chip_id(c) {
                self.chips.remove(&id);
            }
        }
        self.text.replace_range(a..b, "");
        self.cursor = a;
        self.goal_col = None;
    }

    pub fn backspace(&mut self) {
        let p = self.prev_boundary(self.cursor);
        self.remove_range(p, self.cursor);
    }

    pub fn delete(&mut self) {
        let n = self.next_boundary(self.cursor);
        let c = self.cursor;
        self.remove_range(c, n);
    }

    pub fn delete_word_back(&mut self) {
        let p = self.word_left_pos();
        self.remove_range(p, self.cursor);
    }

    pub fn delete_word_fwd(&mut self) {
        let p = self.word_right_pos();
        let c = self.cursor;
        self.remove_range(c, p);
    }

    /// ctrl+u: delete to line start (or the newline before if at line start).
    pub fn kill_to_start(&mut self) {
        let s = self.line_start(self.cursor);
        if s == self.cursor && s > 0 {
            self.remove_range(s - 1, s);
        } else {
            self.remove_range(s, self.cursor);
        }
    }

    /// ctrl+k: delete to line end (or the newline if at line end).
    pub fn kill_to_end(&mut self) {
        let e = self.line_end(self.cursor);
        let c = self.cursor;
        if e == c && e < self.text.len() {
            self.remove_range(c, c + 1);
        } else {
            self.remove_range(c, e);
        }
    }

    // ── layout ──

    /// Wrap into visual rows of at most `w` columns (word wrap when possible). Each logical line yields at
    /// least one row. Row byte ranges exclude the '\n'.
    pub fn rows(&self, w: usize) -> Vec<Row> {
        let w = w.max(2);
        let mut rows = Vec::new();
        let mut line_start = 0;
        for line in self.text.split('\n') {
            let base = line_start;
            line_start += line.len() + 1;
            if line.is_empty() {
                rows.push(Row { start: base, end: base });
                continue;
            }
            let mut row_start = base;
            let mut col = 0;
            let mut last_space: Option<usize> = None; // byte idx after a space in current row
            for (i, g) in line.grapheme_indices(true) {
                let gi = base + i;
                let gw = self.gwidth(g);
                // Reserve one column so the cursor can sit after the last char.
                if col + gw > w - 1 && gi > row_start {
                    let brk = match last_space {
                        Some(s) if s > row_start && s <= gi => s,
                        _ => gi,
                    };
                    rows.push(Row { start: row_start, end: brk });
                    row_start = brk;
                    col = self.width_of(row_start, gi);
                    last_space = None;
                }
                col += gw;
                if g == " " {
                    last_space = Some(gi + 1);
                }
            }
            rows.push(Row { start: row_start, end: base + line.len() });
        }
        rows
    }

    pub fn width_of(&self, a: usize, b: usize) -> usize {
        self.text[a..b].graphemes(true).map(|g| self.gwidth(g)).sum()
    }

    /// Visual (row, col) of the cursor.
    pub fn cursor_pos(&self, rows: &[Row]) -> (usize, usize) {
        for (ri, r) in rows.iter().enumerate() {
            let next_starts_here = rows.get(ri + 1).map(|n| n.start == self.cursor && n.start == r.end).unwrap_or(false);
            if self.cursor >= r.start && self.cursor <= r.end && !next_starts_here {
                return (ri, self.width_of(r.start, self.cursor));
            }
        }
        let last = rows.len().saturating_sub(1);
        (last, rows.last().map(|r| self.width_of(r.start, r.end)).unwrap_or(0))
    }

    fn pos_at_col(&self, r: Row, col: usize) -> usize {
        let mut c = 0;
        for (i, g) in self.text[r.start..r.end].grapheme_indices(true) {
            let gw = self.gwidth(g);
            if c + gw > col {
                return r.start + i;
            }
            c += gw;
        }
        r.end
    }

    /// Move up one visual row; returns false if already on the first row.
    pub fn up(&mut self, w: usize) -> bool {
        let rows = self.rows(w);
        let (r, c) = self.cursor_pos(&rows);
        if r == 0 {
            return false;
        }
        let goal = *self.goal_col.get_or_insert(c);
        self.cursor = self.pos_at_col(rows[r - 1], goal);
        true
    }

    pub fn down(&mut self, w: usize) -> bool {
        let rows = self.rows(w);
        let (r, c) = self.cursor_pos(&rows);
        if r + 1 >= rows.len() {
            return false;
        }
        let goal = *self.goal_col.get_or_insert(c);
        self.cursor = self.pos_at_col(rows[r + 1], goal);
        true
    }

    /// Place the cursor at a visual position (mouse click).
    pub fn click(&mut self, w: usize, row: usize, col: usize) {
        let rows = self.rows(w);
        if let Some(r) = rows.get(row.min(rows.len().saturating_sub(1))) {
            self.cursor = self.pos_at_col(*r, col);
        }
        self.goal_col = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_move_delete() {
        let mut e = Editor::new();
        e.insert_str("hello world");
        e.word_left();
        assert_eq!(e.cursor, 6);
        e.delete_word_back();
        assert_eq!(e.text(), "world");
        e.end();
        e.insert_str(" 你好");
        e.left();
        e.backspace();
        assert_eq!(e.text(), "world 好");
        e.home();
        e.kill_to_end();
        assert_eq!(e.text(), "");
    }

    #[test]
    fn multiline_kill() {
        let mut e = Editor::new();
        e.insert_str("ab\ncd");
        e.kill_to_start();
        assert_eq!(e.text(), "ab\n");
        e.kill_to_start();
        assert_eq!(e.text(), "ab");
        e.home();
        e.kill_to_end();
        assert_eq!(e.text(), "");
    }

    #[test]
    fn cjk_cursor_and_wrap() {
        let mut e = Editor::new();
        e.insert_str("你好世界");
        let rows = e.rows(10);
        assert_eq!(rows.len(), 1);
        assert_eq!(e.cursor_pos(&rows), (0, 8));
        let rows = e.rows(6);
        assert_eq!(rows.len(), 2);
        assert_eq!(e.cursor_pos(&rows), (1, 4));
        e.left();
        assert_eq!(e.cursor_pos(&e.rows(10)), (0, 6));
    }

    #[test]
    fn up_down_goal_col() {
        let mut e = Editor::new();
        e.insert_str("abcdef\nxy\n123456");
        assert!(e.up(40));
        assert_eq!(e.cursor_pos(&e.rows(40)), (1, 2));
        assert!(e.up(40));
        assert_eq!(e.cursor_pos(&e.rows(40)), (0, 6));
        assert!(!e.up(40));
        assert!(e.down(40));
        assert!(e.down(40));
        assert_eq!(e.cursor_pos(&e.rows(40)), (2, 6));
    }

    #[test]
    fn word_wrap_rows() {
        let mut e = Editor::new();
        e.insert_str("aaa bbb ccc");
        let rows = e.rows(7);
        let t: Vec<&str> = rows.iter().map(|r| &e.text()[r.start..r.end]).collect();
        assert_eq!(t, vec!["aaa ", "bbb ", "ccc"]);
    }

    #[test]
    fn paste_chip() {
        let mut e = Editor::new();
        e.insert_str("see: ");
        let big = (0..10).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        e.paste(&big);
        assert_eq!(e.display_text(), "see: [Pasted ~10 lines]");
        let (t, imgs) = e.submission();
        assert!(t.starts_with("see: line 0\nline 1"));
        assert!(imgs.is_empty());
        e.backspace();
        assert_eq!(e.text(), "see: ");
        e.paste("small");
        assert_eq!(e.text(), "see: small");
    }

    #[test]
    fn image_chip() {
        let mut e = Editor::new();
        e.add_image(ImageInput { mime: "image/png".into(), data: "AAA".into(), name: None });
        e.insert_str(" what is this");
        assert_eq!(e.display_text(), "[Image 1] what is this");
        let (t, imgs) = e.submission();
        assert_eq!(t, " what is this");
        assert_eq!(imgs.len(), 1);
        let rows = e.rows(80);
        assert_eq!(e.cursor_pos(&rows), (0, 22));
    }
}
