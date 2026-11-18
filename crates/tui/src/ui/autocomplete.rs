//! Autocomplete popup state, 0-flicker directory/binary caches, path & shell token parsing, and rendering.

use std::collections::VecDeque;

use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};

use super::{chat, dialog, theme};
use crate::text::{self, width};

/// Most rows shown at once in the autocomplete popup.
pub const AC_MAX_ROWS: usize = 10;
/// Minimum column where the autocomplete description starts.
const AC_MIN_DESC_COL: usize = 24;
/// Maximum parent directories cached in memory to prevent popup flicker while typing.
const DIR_CACHE_CAP: usize = 24;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcKind {
    Slash,
    SlashArg,
    File,
    ShellCmd,
    ShellPath,
}

#[derive(Debug, Clone)]
pub struct Autocomplete {
    pub kind: AcKind,
    pub title: String,
    /// `(value inserted, label, description)`
    pub items: Vec<(String, String, String)>,
    pub sel: usize,
    pub scroll: usize,
    /// Byte offset where the replaced token starts.
    pub start: usize,
    pub rect: Rect,
    /// When true, plain `Enter` submits the editor unless `explicitly_selected` is set.
    pub freeform_enter: bool,
    /// Set once the user explicitly moves selection in the popup (`↑`/`↓`/`Ctrl+P`/`Ctrl+N`/hover).
    pub explicitly_selected: bool,
}

impl Autocomplete {
    pub fn new(
        kind: AcKind,
        title: impl Into<String>,
        items: Vec<(String, String, String)>,
        start: usize,
        freeform_enter: bool,
        prev: Option<&Autocomplete>,
    ) -> Option<Self> {
        if items.is_empty() {
            return None;
        }
        let title = title.into();
        let same_ctx = prev.filter(|p| p.kind == kind && p.start == start && p.title == title);
        let (sel, scroll, explicitly_selected) = match same_ctx {
            Some(p) if p.explicitly_selected => (p.sel.min(items.len() - 1), p.scroll, true),
            _ => (0, 0, false),
        };
        Some(Autocomplete {
            kind,
            title,
            items,
            sel,
            scroll,
            start,
            rect: Rect::default(),
            freeform_enter,
            explicitly_selected,
        })
    }

    pub fn move_sel(&mut self, delta: i32) {
        let n = self.items.len() as i32;
        if n <= 0 {
            return;
        }
        self.sel = (self.sel as i32 + delta).rem_euclid(n) as usize;
        self.explicitly_selected = true;
    }

    /// Number of visible item rows inside `self.rect` at last draw.
    pub fn visible_rows(&self) -> usize {
        (self.rect.height as usize)
            .saturating_sub(2)
            .min(self.items.len())
    }

    /// Item index at screen coordinates `(x, y)`, ignoring top border and bottom padding row.
    pub fn item_at(&self, x: u16, y: u16) -> Option<usize> {
        let r = self.rect;
        if !r.contains(Position::new(x, y)) || y <= r.y {
            return None;
        }
        let n = self.visible_rows();
        let row_offset = (y - r.y - 1) as usize;
        if row_offset >= n {
            return None;
        }
        let idx = self.scroll + row_offset;
        (idx < self.items.len()).then_some(idx)
    }
}

/// Draws the autocomplete popup directly above `prompt_rect` within `chat_height`.
pub fn draw(buf: &mut Buffer, prompt_rect: Rect, chat_height: u16, ac: &mut Autocomplete) {
    if ac.items.is_empty() || prompt_rect.width < 6 || chat_height < 3 {
        ac.rect = Rect::default();
        return;
    }
    let max_h = AC_MAX_ROWS.min(chat_height.saturating_sub(1) as usize);
    let n = ac.items.len().min(max_h);
    if n == 0 {
        ac.rect = Rect::default();
        return;
    }
    ac.sel = ac.sel.min(ac.items.len() - 1);
    if ac.sel < ac.scroll {
        ac.scroll = ac.sel;
    } else if ac.sel >= ac.scroll + n {
        ac.scroll = ac.sel + 1 - n;
    }
    let max_scroll = ac.items.len().saturating_sub(n);
    ac.scroll = ac.scroll.min(max_scroll);

    let h = n as u16 + 2;
    let x = prompt_rect.x;
    let w = prompt_rect.width;
    let y = prompt_rect.y.saturating_sub(h);
    let r = Rect::new(x, y, w, h);
    ac.rect = r;
    dialog::clear_wide_edges(buf, r);
    let bg = theme::PANEL;
    for yy in r.y..r.y + r.height {
        for xx in r.x..r.x + r.width {
            if let Some(c) = buf.cell_mut((xx, yy)) {
                c.set_symbol(" ");
                c.set_style(Style::new().bg(bg));
            }
        }
    }
    let bst = Style::new().fg(theme::BORDER).bg(bg);
    for xx in r.x..r.x + r.width {
        buf.set_string(xx, r.y, "─", bst);
    }
    if r.width > 4 {
        let title = text::truncate(&ac.title, (r.width - 3) as usize);
        buf.set_string(r.x + 2, r.y, title, Style::new().fg(theme::MUTED).bg(bg));
    }
    let has_bar = max_scroll > 0 && r.width >= 6;
    let right_pad = if has_bar { 5 } else { 4 };
    let lw = (r.width as usize).saturating_sub(right_pad);
    for (row_idx, (i, (_, label, desc))) in ac
        .items
        .iter()
        .enumerate()
        .skip(ac.scroll)
        .take(n)
        .enumerate()
    {
        let yy = r.y + 1 + row_idx as u16;
        let sel = i == ac.sel;
        let rbg = if sel { theme::PRIMARY } else { bg };
        let row_w = if has_bar { r.width - 1 } else { r.width };
        buf.set_style(Rect::new(r.x, yy, row_w, 1), Style::new().bg(rbg));
        if lw == 0 {
            continue;
        }
        let fgc = if sel { theme::PANEL } else { theme::TEXT };
        let label_fit = text::truncate(label, lw);
        let label_w = width(&label_fit).min(lw);
        buf.set_stringn(
            r.x + 2,
            yy,
            &label_fit,
            lw,
            Style::new().fg(fgc).bg(rbg).add_modifier(if sel {
                Modifier::BOLD
            } else {
                Modifier::empty()
            }),
        );
        let dx = (label_w + 2).max(AC_MIN_DESC_COL.min(lw / 2));
        if !desc.is_empty() && dx + 4 < lw {
            buf.set_stringn(
                r.x + 2 + dx as u16,
                yy,
                text::truncate(desc, lw - dx),
                lw - dx,
                Style::new()
                    .fg(if sel { theme::PANEL } else { theme::MUTED })
                    .bg(rbg),
            );
        }
    }
    if has_bar {
        chat::draw_scrollbar(
            buf,
            Rect::new(r.x + r.width - 1, r.y + 1, 1, n as u16),
            ac.items.len(),
            ac.scroll as f64,
            max_scroll as f64,
            ("┃", theme::BORDER_ACTIVE),
            Some(("│", theme::BORDER_SUBTLE)),
        );
    }
}

// ---------------------------------------------------------------------------
// Directory & Binary Caches
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirKey {
    pub cwd: String,
    pub path: String,
    pub include_files: bool,
}

#[derive(Debug, Clone)]
pub struct DirListing {
    pub resolved_path: String,
    pub dirs: Vec<String>,
    pub files: Vec<String>,
    pub not_found: bool,
}

#[derive(Default)]
pub struct DirCache {
    entries: VecDeque<(DirKey, DirListing)>,
}

impl DirCache {
    pub fn get(&self, key: &DirKey) -> Option<&DirListing> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn insert(&mut self, key: DirKey, listing: DirListing) {
        self.entries.retain(|(k, _)| k != &key);
        if self.entries.len() >= DIR_CACHE_CAP {
            self.entries.pop_front();
        }
        self.entries.push_back((key, listing));
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Splits a typed path fragment into `(request_dir, display_prefix, name_prefix, open_quote)`.
pub struct SplitPath {
    /// Directory passed in `Request::ListDirs.path` (never empty; `"."` for current dir).
    pub req_dir: String,
    /// Prefix kept in front of each completed entry name (e.g. `""`, `"./"`, `"../"`, `"~/"`, `"src/"`).
    pub display_prefix: String,
    /// Partial entry name after the last `/` used for client-side filtering.
    pub name_prefix: String,
    /// Leading quote character (`'` or `"`), if the user opened a quoted path.
    pub quote: Option<char>,
}

pub fn split_dir_query(raw: &str) -> SplitPath {
    let mut s = raw;
    let mut quote = None;
    if let Some(rest) = s.strip_prefix('"') {
        quote = Some('"');
        s = rest.trim_end_matches('"');
    } else if let Some(rest) = s.strip_prefix('\'') {
        quote = Some('\'');
        s = rest.trim_end_matches('\'');
    }
    let unescaped = if quote.is_none() && s.contains("\\ ") {
        s.replace("\\ ", " ")
    } else {
        s.to_string()
    };
    if unescaped == "~" {
        return SplitPath {
            req_dir: "~".into(),
            display_prefix: "~/".into(),
            name_prefix: String::new(),
            quote,
        };
    }
    if let Some(slash) = unescaped.rfind('/') {
        let before = &unescaped[..slash];
        let req_dir = if before.is_empty() {
            "/".into()
        } else {
            before.to_string()
        };
        return SplitPath {
            req_dir,
            display_prefix: unescaped[..=slash].to_string(),
            name_prefix: unescaped[slash + 1..].to_string(),
            quote,
        };
    }
    SplitPath {
        req_dir: ".".into(),
        display_prefix: String::new(),
        name_prefix: unescaped,
        quote,
    }
}

/// Formats a completed path entry for insertion into an editor.
pub fn format_path_completion(
    sp: &SplitPath,
    entry: &str,
    is_dir: bool,
    shell_escape: bool,
    trailing_space_on_file: bool,
) -> String {
    let suffix = if is_dir { "/" } else { "" };
    let full = format!("{}{entry}{suffix}", sp.display_prefix);
    if let Some(q) = sp.quote {
        return if is_dir {
            format!("{q}{full}")
        } else if trailing_space_on_file {
            format!("{q}{full}{q} ")
        } else {
            format!("{q}{full}{q}")
        };
    }
    let escaped = if shell_escape && full.contains(' ') {
        full.replace(' ', "\\ ")
    } else {
        full
    };
    if !is_dir && trailing_space_on_file {
        format!("{escaped} ")
    } else {
        escaped
    }
}

/// Builds ranked `(value, label, desc)` items from a `DirListing` and `SplitPath`.
pub fn build_dir_items(
    listing: &DirListing,
    sp: &SplitPath,
    include_files: bool,
    shell_escape: bool,
    trailing_space_on_file: bool,
) -> Vec<(String, String, String)> {
    if listing.not_found {
        return Vec::new();
    }
    let show_hidden = sp.name_prefix.starts_with('.');
    let q_lower = sp.name_prefix.to_lowercase();
    let mut prefix_matches = Vec::new();
    let mut fuzzy_matches = Vec::new();

    // Offer `./` and `../` when typing leading dots without a slash yet.
    if sp.display_prefix.is_empty() && matches!(sp.name_prefix.as_str(), "." | "..") {
        if ".".starts_with(&q_lower) {
            prefix_matches.push((
                "./".to_string(),
                "./".to_string(),
                "current dir".to_string(),
            ));
        }
        if "..".starts_with(&q_lower) {
            prefix_matches.push((
                "../".to_string(),
                "../".to_string(),
                "parent dir".to_string(),
            ));
        }
    }

    for d in &listing.dirs {
        if d.starts_with('.') && !show_hidden {
            continue;
        }
        let val = format_path_completion(sp, d, true, shell_escape, trailing_space_on_file);
        let label = format!("{}{d}/", sp.display_prefix);
        let dl = d.to_lowercase();
        if q_lower.is_empty() || dl.starts_with(&q_lower) {
            prefix_matches.push((val, label, "dir".to_string()));
        } else if let Some(score) = text::fuzzy_score(d, &sp.name_prefix) {
            fuzzy_matches.push((score + 10, (val, label, "dir".to_string())));
        }
    }
    if include_files {
        for f in &listing.files {
            if f.starts_with('.') && !show_hidden {
                continue;
            }
            let val = format_path_completion(sp, f, false, shell_escape, trailing_space_on_file);
            let label = format!("{}{f}", sp.display_prefix);
            let fl = f.to_lowercase();
            if q_lower.is_empty() || fl.starts_with(&q_lower) {
                prefix_matches.push((val, label, String::new()));
            } else if let Some(score) = text::fuzzy_score(f, &sp.name_prefix) {
                fuzzy_matches.push((score, (val, label, String::new())));
            }
        }
    }
    fuzzy_matches.sort_by_key(|x| std::cmp::Reverse(x.0));
    prefix_matches
        .into_iter()
        .chain(fuzzy_matches.into_iter().map(|x| x.1))
        .take(50)
        .collect()
}

/// Steps up one directory segment in `Kind::Cd` when pressing Backspace at a trailing `/` or selecting `../`.
pub fn parent_dir_input(current: &str) -> String {
    let trimmed = current.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/".into();
    }
    if trimmed == "~" {
        return String::new();
    }
    match trimmed.rfind('/') {
        Some(0) => "/".into(),
        Some(idx) => format!("{}/", &trimmed[..idx]),
        None => "../".into(),
    }
}

// ---------------------------------------------------------------------------
// Shell Token Context
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellTokenPos {
    /// First command token (at start of line or after `|`, `||`, `&&`, `;`).
    Command {
        start: usize,
        raw: String,
        at_line_start: bool,
    },
    /// Subsequent argument token after a command word.
    Arg { start: usize, raw: String },
}

/// Parses the current line before `cursor` in `!` shell mode to identify the token being completed.
pub fn shell_token_context(before_cursor: &str) -> ShellTokenPos {
    let line_offset = before_cursor.rfind('\n').map_or(0, |i| i + 1);
    let line = &before_cursor[line_offset..];
    let bytes = line.as_bytes();
    let mut i = 0usize;
    let mut quote: Option<u8> = None;
    let mut seg_start = 0usize;
    let mut at_line_start = true;

    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == b'\\' && q == b'"' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if b == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'\\' => {
                i = (i + 2).min(bytes.len());
            }
            b'\'' | b'"' => {
                quote = Some(b);
                i += 1;
            }
            b';' => {
                i += 1;
                seg_start = i;
                at_line_start = false;
            }
            b'|' => {
                i += if bytes.get(i + 1) == Some(&b'|') {
                    2
                } else {
                    1
                };
                seg_start = i;
                at_line_start = false;
            }
            b'&' if bytes.get(i + 1) == Some(&b'&') => {
                i += 2;
                seg_start = i;
                at_line_start = false;
            }
            _ => {
                i += 1;
            }
        }
    }

    // Within `line[seg_start..]`, scan words (skipping leading `VAR=val` assignments).
    let seg = &line[seg_start..];
    let mut pos = 0usize;
    let mut word_index = 0usize;
    loop {
        while let Some(c) = seg[pos..].chars().next() {
            if c.is_whitespace() {
                pos += c.len_utf8();
            } else {
                break;
            }
        }
        if pos >= seg.len() {
            let abs = line_offset + seg_start + pos;
            return if word_index == 0 {
                ShellTokenPos::Command {
                    start: abs,
                    raw: String::new(),
                    at_line_start,
                }
            } else {
                ShellTokenPos::Arg {
                    start: abs,
                    raw: String::new(),
                }
            };
        }
        let tok_start = pos;
        let mut tq: Option<char> = None;
        let mut esc = false;
        while let Some(c) = seg[pos..].chars().next() {
            if esc {
                esc = false;
                pos += c.len_utf8();
                continue;
            }
            if let Some(q) = tq {
                if c == '\\' && q == '"' {
                    esc = true;
                } else if c == q {
                    tq = None;
                }
                pos += c.len_utf8();
                continue;
            }
            if c.is_whitespace() {
                break;
            }
            if c == '\\' {
                esc = true;
            } else if c == '\'' || c == '"' {
                tq = Some(c);
            }
            pos += c.len_utf8();
        }
        let tok = &seg[tok_start..pos];
        if pos >= seg.len() {
            let abs = line_offset + seg_start + tok_start;
            return if word_index == 0 {
                ShellTokenPos::Command {
                    start: abs,
                    raw: tok.to_string(),
                    at_line_start,
                }
            } else {
                ShellTokenPos::Arg {
                    start: abs,
                    raw: tok.to_string(),
                }
            };
        }
        // Env assignment before command (`FOO=bar cmd`) leaves `word_index` at 0.
        let is_env_assign = word_index == 0
            && tok.split_once('=').is_some_and(|(k, _)| {
                !k.is_empty()
                    && k.bytes().enumerate().all(|(idx, b)| {
                        b == b'_' || b.is_ascii_alphabetic() || (idx > 0 && b.is_ascii_digit())
                    })
            });
        if !is_env_assign {
            word_index += 1;
        }
    }
}

/// Finds the start byte offset and raw text of the whitespace-delimited token at `end` within `s`
/// (starting no earlier than `min_start`), respecting quotes and `\` escapes.
pub fn last_arg_token(s: &str, min_start: usize) -> (usize, String) {
    let slice = &s[min_start..];
    let mut tok_start = 0usize;
    let mut in_tok = false;
    let mut quote: Option<char> = None;
    let mut esc = false;
    for (idx, c) in slice.char_indices() {
        if esc {
            if !in_tok {
                tok_start = idx;
                in_tok = true;
            }
            esc = false;
            continue;
        }
        if let Some(q) = quote {
            if c == '\\' && q == '"' {
                esc = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        if c.is_whitespace() {
            in_tok = false;
            tok_start = idx + c.len_utf8();
        } else {
            if !in_tok {
                tok_start = idx;
                in_tok = true;
            }
            if c == '\\' {
                esc = true;
            } else if c == '\'' || c == '"' {
                quote = Some(c);
            }
        }
    }
    if !in_tok {
        (s.len(), String::new())
    } else {
        (min_start + tok_start, slice[tok_start..].to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_dir_query_cases() {
        let sp = split_dir_query("");
        assert_eq!(
            (
                sp.req_dir.as_str(),
                sp.display_prefix.as_str(),
                sp.name_prefix.as_str()
            ),
            (".", "", "")
        );
        let sp = split_dir_query("src/u");
        assert_eq!(
            (
                sp.req_dir.as_str(),
                sp.display_prefix.as_str(),
                sp.name_prefix.as_str()
            ),
            ("src", "src/", "u")
        );
        let sp = split_dir_query("./sr");
        assert_eq!(
            (
                sp.req_dir.as_str(),
                sp.display_prefix.as_str(),
                sp.name_prefix.as_str()
            ),
            (".", "./", "sr")
        );
        let sp = split_dir_query("../foo");
        assert_eq!(
            (
                sp.req_dir.as_str(),
                sp.display_prefix.as_str(),
                sp.name_prefix.as_str()
            ),
            ("..", "../", "foo")
        );
        let sp = split_dir_query("~/app");
        assert_eq!(
            (
                sp.req_dir.as_str(),
                sp.display_prefix.as_str(),
                sp.name_prefix.as_str()
            ),
            ("~", "~/", "app")
        );
        let sp = split_dir_query("/ho");
        assert_eq!(
            (
                sp.req_dir.as_str(),
                sp.display_prefix.as_str(),
                sp.name_prefix.as_str()
            ),
            ("/", "/", "ho")
        );
    }

    #[test]
    fn shell_token_parsing_pipes_and_args() {
        assert_eq!(
            shell_token_context("car"),
            ShellTokenPos::Command {
                start: 0,
                raw: "car".into(),
                at_line_start: true,
            }
        );
        assert_eq!(
            shell_token_context("cargo test src/"),
            ShellTokenPos::Arg {
                start: 11,
                raw: "src/".into(),
            }
        );
        assert_eq!(
            shell_token_context("cat Cargo.toml | gr"),
            ShellTokenPos::Command {
                start: 17,
                raw: "gr".into(),
                at_line_start: false,
            }
        );
        assert_eq!(
            shell_token_context("RUST_LOG=debug car"),
            ShellTokenPos::Command {
                start: 15,
                raw: "car".into(),
                at_line_start: true,
            }
        );
    }

    #[test]
    fn autocomplete_item_at_ignores_bottom_pad_and_uses_scroll() {
        let items: Vec<_> = (0..15)
            .map(|i| (format!("v{i}"), format!("l{i}"), String::new()))
            .collect();
        let mut ac = Autocomplete::new(AcKind::Slash, " commands ", items, 0, false, None).unwrap();
        let mut buf = Buffer::empty(Rect::new(0, 0, 60, 20));
        draw(&mut buf, Rect::new(1, 15, 50, 4), 15, &mut ac);
        // 10 visible rows at r.y + 1 .. r.y + 10; r.y + 11 is bottom pad!
        assert_eq!(ac.item_at(5, ac.rect.y), None);
        assert_eq!(ac.item_at(5, ac.rect.y + 1), Some(0));
        assert_eq!(ac.item_at(5, ac.rect.y + 10), Some(9));
        assert_eq!(
            ac.item_at(5, ac.rect.y + 11),
            None,
            "bottom pad row is not an item"
        );
    }
}
