//! Multi-line prompt editor: CJK-aware cursor, soft wrap, selection, paste tokens, image
//! attachments, history and undo.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ref_proto::ImageData;
use unicode_segmentation::UnicodeSegmentation;

use super::text::{gwidth, width};
use super::theme;

#[derive(Debug, Clone)]
pub struct Paste {
    pub token: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct Attachment {
    pub token: String,
    pub image: ImageData,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Editor {
    pub text: String,
    /// Byte offset of the cursor (always on a grapheme boundary).
    pub cursor: usize,
    /// Selection anchor (byte offset), when selecting with shift+arrows.
    pub anchor: Option<usize>,
    pub pastes: Vec<Paste>,
    pub images: Vec<Attachment>,
    paste_seq: usize,
    image_seq: usize,
    /// Preferred visual column for up/down.
    pref_col: Option<usize>,
    undo: Vec<(String, usize)>,
    redo: Vec<(String, usize)>,
    /// Width used by the last layout (for up/down).
    pub last_width: usize,
    /// Scroll offset of visible rows.
    pub scroll: usize,
}

/// A visual row: byte range `[start, end)` of `text`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VRow {
    pub start: usize,
    pub end: usize,
    /// Last row of its logical line.
    pub last: bool,
}

pub struct Submit {
    pub text: String,
    pub images: Vec<(ImageData, Option<String>)>,
}

impl Editor {
    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && self.images.is_empty()
    }

    fn snapshot(&mut self) {
        if self.undo.last().is_some_and(|(t, _)| *t == self.text) {
            return;
        }
        self.undo.push((self.text.clone(), self.cursor));
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self) {
        if let Some((t, c)) = self.undo.pop() {
            self.redo.push((std::mem::replace(&mut self.text, t), self.cursor));
            self.cursor = c.min(self.text.len());
            self.anchor = None;
        }
    }

    pub fn redo(&mut self) {
        if let Some((t, c)) = self.redo.pop() {
            self.undo.push((std::mem::replace(&mut self.text, t), self.cursor));
            self.cursor = c.min(self.text.len());
            self.anchor = None;
        }
    }

    pub fn set_text(&mut self, t: &str) {
        self.snapshot();
        self.text = t.to_string();
        self.cursor = self.text.len();
        self.anchor = None;
        self.pref_col = None;
        self.gc_tokens();
    }

    pub fn clear(&mut self) {
        self.snapshot();
        self.text.clear();
        self.cursor = 0;
        self.anchor = None;
        self.pastes.clear();
        self.images.clear();
        self.pref_col = None;
        self.scroll = 0;
    }

    // -- selection ----------------------------------------------------------

    pub fn selection(&self) -> Option<(usize, usize)> {
        let a = self.anchor?;
        if a == self.cursor {
            return None;
        }
        Some((a.min(self.cursor), a.max(self.cursor)))
    }

    pub fn selected_text(&self) -> Option<String> {
        self.selection().map(|(a, b)| self.text[a..b].to_string())
    }

    fn delete_selection(&mut self) -> bool {
        if let Some((a, b)) = self.selection() {
            self.snapshot();
            self.text.replace_range(a..b, "");
            self.cursor = a;
            self.anchor = None;
            self.gc_tokens();
            return true;
        }
        self.anchor = None;
        false
    }

    /// Begin/extend (`extend = true`) or clear the selection before a movement.
    pub fn select_mode(&mut self, extend: bool) {
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
    }

    // -- editing ------------------------------------------------------------

    pub fn insert(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        if !self.delete_selection() {
            self.snapshot_coalesced(s);
        }
        let s = s.replace("\r\n", "\n").replace('\r', "\n").replace('\t', "    ");
        self.text.insert_str(self.cursor, &s);
        self.cursor += s.len();
        self.pref_col = None;
    }

    /// Snapshot only at word boundaries so undo is not per-keystroke.
    fn snapshot_coalesced(&mut self, s: &str) {
        if s.len() > 1 || s == " " || s == "\n" || self.undo.is_empty() {
            self.snapshot();
        }
    }

    pub fn newline(&mut self) {
        self.insert("\n");
    }

    fn prev_boundary(&self, i: usize) -> usize {
        self.text[..i].grapheme_indices(true).next_back().map(|(j, _)| j).unwrap_or(0)
    }

    fn next_boundary(&self, i: usize) -> usize {
        self.text[i..].graphemes(true).next().map(|g| i + g.len()).unwrap_or(i)
    }

    /// Token (paste / image) ending exactly at `i` or containing it.
    fn token_around(&self, i: usize) -> Option<(usize, usize)> {
        let toks = self.pastes.iter().map(|p| p.token.as_str()).chain(self.images.iter().map(|a| a.token.as_str()));
        for t in toks {
            let mut from = 0;
            while let Some(p) = self.text[from..].find(t) {
                let s = from + p;
                let e = s + t.len();
                if i > s && i <= e {
                    return Some((s, e));
                }
                from = e;
            }
        }
        None
    }

    pub fn backspace(&mut self) {
        if self.delete_selection() {
            return;
        }
        if self.cursor == 0 {
            return;
        }
        self.snapshot();
        let (s, e) = self.token_around(self.cursor).unwrap_or((self.prev_boundary(self.cursor), self.cursor));
        self.text.replace_range(s..e, "");
        self.cursor = s;
        self.pref_col = None;
        self.gc_tokens();
    }

    pub fn delete(&mut self) {
        if self.delete_selection() {
            return;
        }
        if self.cursor >= self.text.len() {
            return;
        }
        self.snapshot();
        let n = self.next_boundary(self.cursor);
        let (s, e) = self.token_around(n).filter(|(s, _)| *s >= self.cursor).unwrap_or((self.cursor, n));
        self.text.replace_range(s..e, "");
        self.gc_tokens();
    }

    fn word_left_pos(&self) -> usize {
        let before = &self.text[..self.cursor];
        let mut idx = self.cursor;
        let mut seen_word = false;
        for (i, w) in before.split_word_bound_indices().rev() {
            let is_space = w.chars().all(char::is_whitespace);
            if is_space && seen_word {
                break;
            }
            if !is_space {
                if seen_word && w.chars().all(|c| !c.is_alphanumeric()) {
                    break;
                }
                seen_word = true;
            }
            idx = i;
            if seen_word && !w.chars().any(char::is_alphanumeric) {
                break;
            }
        }
        idx
    }

    fn word_right_pos(&self) -> usize {
        let after = &self.text[self.cursor..];
        let mut idx = self.cursor;
        let mut seen_word = false;
        for (i, w) in after.split_word_bound_indices() {
            let is_space = w.chars().all(char::is_whitespace);
            if is_space && seen_word {
                break;
            }
            idx = self.cursor + i + w.len();
            if !is_space {
                seen_word = true;
                if !w.chars().any(char::is_alphanumeric) {
                    break;
                }
            }
        }
        idx
    }

    pub fn delete_word_back(&mut self) {
        if self.delete_selection() {
            return;
        }
        let p = self.word_left_pos();
        if p < self.cursor {
            self.snapshot();
            self.text.replace_range(p..self.cursor, "");
            self.cursor = p;
            self.gc_tokens();
        }
    }

    pub fn delete_word_forward(&mut self) {
        if self.delete_selection() {
            return;
        }
        let p = self.word_right_pos();
        if p > self.cursor {
            self.snapshot();
            self.text.replace_range(self.cursor..p, "");
            self.gc_tokens();
        }
    }

    fn line_start(&self) -> usize {
        self.text[..self.cursor].rfind('\n').map(|i| i + 1).unwrap_or(0)
    }

    fn line_end(&self) -> usize {
        self.text[self.cursor..].find('\n').map(|i| self.cursor + i).unwrap_or(self.text.len())
    }

    pub fn kill_to_start(&mut self) {
        let s = self.line_start();
        let s = if s == self.cursor && s > 0 { s - 1 } else { s };
        if s < self.cursor {
            self.snapshot();
            self.text.replace_range(s..self.cursor, "");
            self.cursor = s;
            self.anchor = None;
            self.gc_tokens();
        }
    }

    pub fn kill_to_end(&mut self) {
        let e = self.line_end();
        let e = if e == self.cursor && e < self.text.len() { e + 1 } else { e };
        if e > self.cursor {
            self.snapshot();
            self.text.replace_range(self.cursor..e, "");
            self.anchor = None;
            self.gc_tokens();
        }
    }

    // -- movement ------------------------------------------------------------

    pub fn left(&mut self) {
        self.cursor = self.prev_boundary(self.cursor);
        self.pref_col = None;
    }

    pub fn right(&mut self) {
        self.cursor = self.next_boundary(self.cursor);
        self.pref_col = None;
    }

    pub fn word_left(&mut self) {
        self.cursor = self.word_left_pos();
        self.pref_col = None;
    }

    pub fn word_right(&mut self) {
        self.cursor = self.word_right_pos();
        self.pref_col = None;
    }

    pub fn home(&mut self) {
        self.cursor = self.line_start();
        self.pref_col = None;
    }

    pub fn end(&mut self) {
        self.cursor = self.line_end();
        self.pref_col = None;
    }

    pub fn buf_start(&mut self) {
        self.cursor = 0;
    }

    pub fn buf_end(&mut self) {
        self.cursor = self.text.len();
    }

    /// Moves one visual row up; returns false if already on the first row.
    pub fn up(&mut self) -> bool {
        self.vmove(-1)
    }

    pub fn down(&mut self) -> bool {
        self.vmove(1)
    }

    fn vmove(&mut self, dir: i32) -> bool {
        let rows = self.layout(self.last_width.max(1));
        let (r, col) = self.cursor_rc(&rows);
        let nr = r as i32 + dir;
        if nr < 0 || nr as usize >= rows.len() {
            return false;
        }
        let want = *self.pref_col.get_or_insert(col);
        let row = rows[nr as usize];
        let mut x = 0;
        let mut pos = row.start;
        for (i, g) in self.text[row.start..row.end].grapheme_indices(true) {
            if g == "\n" {
                break;
            }
            let w = gwidth(g);
            if x + w > want {
                break;
            }
            x += w;
            pos = row.start + i + g.len();
        }
        self.cursor = pos;
        true
    }

    pub fn on_first_row(&self) -> bool {
        let rows = self.layout(self.last_width.max(1));
        self.cursor_rc(&rows).0 == 0
    }

    pub fn on_last_row(&self) -> bool {
        let rows = self.layout(self.last_width.max(1));
        self.cursor_rc(&rows).0 + 1 >= rows.len()
    }

    // -- tokens ----------------------------------------------------------------

    /// Inserts a large paste as a collapsed token (or literally if small).
    pub fn paste(&mut self, s: &str) {
        let s = s.replace("\r\n", "\n").replace('\r', "\n");
        let s = super::text::sanitize(&s);
        let lines = s.lines().count();
        if lines >= 3 || s.chars().count() > 150 {
            self.paste_seq += 1;
            let token = if lines > 1 {
                format!("[Pasted ~{lines} lines]")
            } else {
                format!("[Pasted {} chars]", s.chars().count())
            };
            // Disambiguate identical tokens.
            let token = if self.pastes.iter().any(|p| p.token == token) {
                format!("{} #{}]", token.trim_end_matches(']'), self.paste_seq)
            } else {
                token
            };
            self.pastes.push(Paste { token: token.clone(), text: s });
            self.insert(&token);
        } else {
            self.insert(&s);
        }
    }

    pub fn attach_image(&mut self, image: ImageData, name: Option<String>) {
        self.image_seq += 1;
        let token = format!("[Image {}]", self.image_seq);
        self.images.push(Attachment { token: token.clone(), image, name });
        let pad = if self.cursor > 0 && !self.text[..self.cursor].ends_with([' ', '\n']) { " " } else { "" };
        self.insert(&format!("{pad}{token} "));
    }

    /// Puts a previously sent prompt back: fills an empty editor, otherwise appends it after a
    /// blank line. `[Image N]` tokens in `text` are bound to `images` in token-number order (the
    /// order the editor created them) and renumbered after the editor's own images; images
    /// without a token get one at the end. Cursor at the end.
    pub fn restore(&mut self, text: &str, images: Vec<(ImageData, Option<String>)>) {
        if text.trim().is_empty() && images.is_empty() {
            return;
        }
        self.snapshot();
        let mut nums: Vec<usize> = image_token_numbers(text);
        nums.sort_unstable();
        nums.dedup();
        let mut images = images.into_iter();
        // Two passes (via placeholders) so renumbering never hits a token renamed earlier.
        let mut restored = text.to_string();
        let mut bound = Vec::new();
        for (i, n) in nums.into_iter().enumerate() {
            let Some(img) = images.next() else { break };
            restored = restored.replace(&format!("[Image {n}]"), &format!("\u{0}{i}\u{0}"));
            bound.push(img);
        }
        for (i, (image, name)) in bound.into_iter().enumerate() {
            self.image_seq += 1;
            let token = format!("[Image {}]", self.image_seq);
            restored = restored.replace(&format!("\u{0}{i}\u{0}"), &token);
            self.images.push(Attachment { token, image, name });
        }
        let keep = self.text.trim_end();
        self.text = if keep.is_empty() { restored } else { format!("{keep}\n\n{restored}") };
        self.cursor = self.text.len();
        self.anchor = None;
        self.pref_col = None;
        for (image, name) in images {
            self.attach_image(image, name);
        }
    }

    /// Drops tokens no longer present in the text.
    fn gc_tokens(&mut self) {
        let t = &self.text;
        self.pastes.retain(|p| t.contains(&p.token));
        self.images.retain(|a| t.contains(&a.token));
        if self.images.is_empty() {
            self.image_seq = 0;
        }
        self.cursor = self.cursor.min(self.text.len());
        while !self.text.is_char_boundary(self.cursor) {
            self.cursor -= 1;
        }
    }

    /// Final text (paste tokens expanded) and images (in token order). Image tokens stay in the
    /// text so the model can refer to them.
    pub fn submit(&mut self) -> Submit {
        let mut text = self.text.clone();
        for p in &self.pastes {
            text = text.replacen(&p.token, &p.text, 1);
        }
        let images = self.images.iter().filter(|a| self.text.contains(&a.token)).map(|a| (a.image.clone(), a.name.clone())).collect();
        self.clear();
        self.undo.clear();
        Submit { text, images }
    }

    /// Ranges of tokens in the text (for highlighting).
    fn token_ranges(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for t in self.pastes.iter().map(|p| p.token.as_str()).chain(self.images.iter().map(|a| a.token.as_str())) {
            let mut from = 0;
            while let Some(p) = self.text[from..].find(t) {
                out.push((from + p, from + p + t.len()));
                from += p + t.len();
            }
        }
        out
    }

    // -- layout & drawing --------------------------------------------------------

    /// Soft-wrapped visual rows for `w` columns (word wrap, CJK aware).
    pub fn layout(&self, w: usize) -> Vec<VRow> {
        let w = w.max(2);
        let mut rows = Vec::new();
        let mut line_start = 0;
        for line in self.text.split('\n') {
            let base = line_start;
            let mut start = 0usize;
            let mut x = 0usize;
            let mut last_space: Option<usize> = None;
            for (i, g) in line.grapheme_indices(true) {
                let gw = gwidth(g);
                // Spaces may hang past the edge (they become the break point).
                if g == " " && x + gw > w - 1 {
                    x += gw;
                    last_space = Some(i + 1);
                    continue;
                }
                // Reserve one column for the cursor at the end of a full row.
                if x + gw > w - 1 && i > start {
                    let brk = match last_space {
                        Some(s) if s > start => s,
                        _ => i,
                    };
                    rows.push(VRow { start: base + start, end: base + brk, last: false });
                    start = brk;
                    x = width(&line[start..i]);
                    last_space = None;
                }
                x += gw;
                if g == " " {
                    last_space = Some(i + 1);
                }
            }
            rows.push(VRow { start: base + start, end: base + line.len(), last: true });
            line_start += line.len() + 1;
        }
        rows
    }

    /// (visual row, column) of the cursor.
    pub fn cursor_rc(&self, rows: &[VRow]) -> (usize, usize) {
        for (ri, r) in rows.iter().enumerate() {
            let inside = self.cursor >= r.start && (self.cursor < r.end || (self.cursor == r.end && (r.last || ri + 1 == rows.len())));
            if inside {
                return (ri, width(&self.text[r.start..self.cursor]));
            }
        }
        (rows.len().saturating_sub(1), 0)
    }

    /// Height (rows) needed for width `w`.
    pub fn height(&self, w: usize) -> usize {
        self.layout(w).len()
    }

    /// Draws into `area`; returns the screen cursor position.
    pub fn draw(&mut self, buf: &mut Buffer, area: Rect, placeholder: &str, focused: bool) -> Option<(u16, u16)> {
        let w = area.width as usize;
        self.last_width = w;
        if area.height == 0 || w == 0 {
            return None;
        }
        if self.text.is_empty() {
            let ph = super::text::truncate(placeholder, w.saturating_sub(1));
            buf.set_string(area.x, area.y, ph, theme::fg(theme::DIM));
            return focused.then_some((area.x, area.y));
        }
        let rows = self.layout(w);
        let (cr, cc) = self.cursor_rc(&rows);
        let h = area.height as usize;
        if cr < self.scroll {
            self.scroll = cr;
        }
        if cr >= self.scroll + h {
            self.scroll = cr + 1 - h;
        }
        self.scroll = self.scroll.min(rows.len().saturating_sub(h));
        let sel = self.selection();
        let toks = self.token_ranges();
        let base = theme::text();
        for (i, r) in rows.iter().enumerate().skip(self.scroll).take(h) {
            let y = area.y + (i - self.scroll) as u16;
            let mut x = area.x;
            for (gi, g) in self.text[r.start..r.end].grapheme_indices(true) {
                let off = r.start + gi;
                let gw = gwidth(g) as u16;
                if g == "\n" || gw == 0 {
                    continue;
                }
                let mut st = base;
                if toks.iter().any(|(a, b)| off >= *a && off < *b) {
                    st = Style::new().fg(theme::ACCENT).add_modifier(Modifier::BOLD);
                }
                if sel.is_some_and(|(a, b)| off >= a && off < b) {
                    st = st.bg(theme::SELECTION);
                }
                if x + gw > area.x + area.width {
                    break;
                }
                buf.set_string(x, y, g, st);
                x += gw;
            }
        }
        if !focused || cr < self.scroll {
            return None;
        }
        let y = area.y + (cr - self.scroll) as u16;
        Some(((area.x + cc as u16).min(area.x + area.width.saturating_sub(1)), y))
    }

    /// The `@word` being typed at the cursor: (byte start of `@`, query).
    pub fn mention(&self) -> Option<(usize, String)> {
        let before = &self.text[..self.cursor];
        let at = before.rfind('@')?;
        let q = &before[at + 1..];
        if q.contains(char::is_whitespace) {
            return None;
        }
        if at > 0 && !before[..at].ends_with(char::is_whitespace) {
            return None;
        }
        Some((at, q.to_string()))
    }

    /// Replace `[start, cursor)` with `s`.
    pub fn replace_before_cursor(&mut self, start: usize, s: &str) {
        if start > self.cursor {
            return;
        }
        self.snapshot();
        self.text.replace_range(start..self.cursor, s);
        self.cursor = start + s.len();
        self.anchor = None;
    }
}

// ---------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct History {
    pub items: Vec<String>,
    /// Index while browsing (None = editing a fresh draft).
    pos: Option<usize>,
    draft: String,
    path: Option<std::path::PathBuf>,
}

const HISTORY_MAX: usize = 500;

impl History {
    pub fn load(path: std::path::PathBuf) -> Self {
        let mut items = Vec::new();
        if let Ok(s) = std::fs::read_to_string(&path) {
            for l in s.lines() {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(l)
                    && let Some(t) = v.get("text").and_then(|t| t.as_str())
                {
                    items.push(t.to_string());
                }
            }
        }
        let n = items.len();
        if n > HISTORY_MAX {
            items.drain(..n - HISTORY_MAX);
        }
        History { items, pos: None, draft: String::new(), path: Some(path) }
    }

    pub fn push(&mut self, t: &str) {
        self.pos = None;
        let t = t.trim_end();
        if t.is_empty() || self.items.last().is_some_and(|l| l == t) {
            return;
        }
        self.items.push(t.to_string());
        if let Some(p) = &self.path {
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            use std::io::Write;
            let rewrite = self.items.len() > HISTORY_MAX;
            if rewrite {
                let n = self.items.len();
                self.items.drain(..n - HISTORY_MAX);
                let body: String = self.items.iter().map(|i| format!("{}\n", serde_json::json!({ "text": i }))).collect();
                let _ = std::fs::write(p, body);
            } else if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
                let _ = writeln!(f, "{}", serde_json::json!({ "text": t }));
            }
        }
    }

    pub fn prev(&mut self, current: &str) -> Option<String> {
        if self.items.is_empty() {
            return None;
        }
        let i = match self.pos {
            None => {
                self.draft = current.to_string();
                self.items.len() - 1
            }
            Some(0) => return None,
            Some(i) => i - 1,
        };
        self.pos = Some(i);
        self.items.get(i).cloned()
    }

    pub fn next(&mut self) -> Option<String> {
        let i = self.pos?;
        if i + 1 >= self.items.len() {
            self.pos = None;
            return Some(std::mem::take(&mut self.draft));
        }
        self.pos = Some(i + 1);
        self.items.get(i + 1).cloned()
    }

    pub fn reset(&mut self) {
        self.pos = None;
    }

    pub fn browsing(&self) -> bool {
        self.pos.is_some()
    }
}

/// Numbers `N` of the `[Image N]` tokens in `text` (in order of appearance).
fn image_token_numbers(text: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("[Image ") {
        rest = &rest[i + 7..];
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0
            && rest[digits..].starts_with(']')
            && let Ok(n) = rest[..digits].parse()
        {
            out.push(n);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(data: &str) -> ImageData {
        ImageData { mime: "image/png".into(), data: data.into() }
    }

    #[test]
    fn restore_fills_or_appends_and_rebinds_images() {
        let mut e = Editor::default();
        e.restore("fix it", vec![]);
        assert_eq!((e.text.as_str(), e.cursor), ("fix it", 6));
        // Appended after a blank line; image tokens renumbered after the editor's own.
        let mut e = Editor::default();
        e.insert("draft ");
        e.attach_image(img("a"), None);
        e.restore("see [Image 2] and [Image 1]", vec![(img("x"), None), (img("y"), Some("y.png".into()))]);
        assert_eq!(e.text, "draft [Image 1]\n\nsee [Image 3] and [Image 2]");
        assert_eq!(e.cursor, e.text.len());
        let data: Vec<(&str, &str)> = e.images.iter().map(|a| (a.token.as_str(), a.image.data.as_str())).collect();
        assert_eq!(data, [("[Image 1]", "a"), ("[Image 2]", "x"), ("[Image 3]", "y")]);
        let sub = e.submit();
        assert_eq!(sub.images.len(), 3);
        // An image without a token in the text still comes back.
        let mut e = Editor::default();
        e.restore("look", vec![(img("z"), None)]);
        assert_eq!(e.text, "look [Image 1] ");
        assert_eq!(e.images.len(), 1);
    }

    #[test]
    fn insert_delete_cjk() {
        let mut e = Editor::default();
        e.insert("你好a");
        assert_eq!(e.cursor, e.text.len());
        e.left();
        e.backspace();
        assert_eq!(e.text, "你a");
        e.home();
        e.right();
        assert_eq!(e.cursor, "你".len());
        let rows = e.layout(20);
        assert_eq!(e.cursor_rc(&rows), (0, 2));
        e.end();
        e.insert("👍🏽");
        e.backspace();
        assert_eq!(e.text, "你a");
    }

    #[test]
    fn words_and_kill() {
        let mut e = Editor::default();
        e.insert("hello brave world");
        e.delete_word_back();
        assert_eq!(e.text, "hello brave ");
        e.word_left();
        assert_eq!(e.cursor, 6);
        e.kill_to_end();
        assert_eq!(e.text, "hello ");
        e.kill_to_start();
        assert_eq!(e.text, "");
    }

    #[test]
    fn wrap_and_vertical_move() {
        let mut e = Editor::default();
        e.insert("aaaa bbbb cccc\nxy");
        e.last_width = 10;
        let rows = e.layout(10);
        assert_eq!(rows.len(), 3);
        assert_eq!(&e.text[rows[0].start..rows[0].end], "aaaa bbbb ");
        assert!(e.up());
        assert!(e.up());
        assert!(!e.up());
        assert!(e.on_first_row());
    }

    #[test]
    fn paste_tokens() {
        let mut e = Editor::default();
        e.paste("line1\nline2\nline3\n");
        assert!(e.text.starts_with("[Pasted ~3 lines]"));
        e.insert(" end");
        let s = e.submit();
        assert_eq!(s.text, "line1\nline2\nline3\n end");
        // Backspace removes a token as a unit.
        let mut e = Editor::default();
        e.attach_image(ImageData { mime: "image/png".into(), data: "AA".into() }, None);
        e.backspace(); // trailing space
        e.backspace(); // token
        assert_eq!(e.text, "");
        assert!(e.images.is_empty());
    }

    #[test]
    fn selection_and_undo() {
        let mut e = Editor::default();
        e.insert("abc def");
        e.select_mode(true);
        e.word_left();
        assert_eq!(e.selected_text().as_deref(), Some("def"));
        e.insert("X");
        assert_eq!(e.text, "abc X");
        e.undo();
        assert_eq!(e.text, "abc def");
    }

    #[test]
    fn mention_detection() {
        let mut e = Editor::default();
        e.insert("look at @src/ma");
        assert_eq!(e.mention(), Some((8, "src/ma".into())));
        e.insert(" x");
        assert_eq!(e.mention(), None);
    }

    #[test]
    fn history_cycle() {
        let mut h = History::default();
        h.push("one");
        h.push("two");
        assert_eq!(h.prev("draft").as_deref(), Some("two"));
        assert_eq!(h.prev("").as_deref(), Some("one"));
        assert_eq!(h.prev(""), None);
        assert_eq!(h.next().as_deref(), Some("two"));
        assert_eq!(h.next().as_deref(), Some("draft"));
    }
}
