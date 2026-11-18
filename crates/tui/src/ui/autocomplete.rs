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
/// Popup chrome rows: top border + bottom padding row.
const AC_CHROME_ROWS: u16 = 2;
/// Bonus added to directory fuzzy-match scores so directories rank ahead of files.
const DIR_FUZZY_SCORE_BONUS: i64 = 10;
/// Maximum entries returned by `build_dir_items`.
const MAX_DIR_ITEMS: usize = 50;

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
    let max_h = AC_MAX_ROWS.min(chat_height.saturating_sub(AC_CHROME_ROWS) as usize);
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

    let h = n as u16 + AC_CHROME_ROWS;
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

/// True when `p` begins with an ASCII drive prefix like `C:`.
pub(crate) fn has_drive_prefix(p: &str) -> bool {
    let b = p.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

/// True when `p` is an exact bare drive specifier like `C:` (no trailing separator).
pub(crate) fn is_bare_drive(p: &str) -> bool {
    p.len() == 2 && has_drive_prefix(p)
}

/// True when `p` begins with a rooted Windows drive prefix (`C:/` or `C:\`).
pub(crate) fn has_drive_root_prefix(p: &str) -> bool {
    has_drive_prefix(p) && matches!(p.as_bytes().get(2), Some(b'/' | b'\\'))
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
    let win_style = has_drive_prefix(s)
        || s.starts_with("~\\")
        || s.starts_with(".\\")
        || s.starts_with("..\\");
    let unescaped = if quote.is_none() && !win_style && s.contains("\\ ") {
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
    if is_bare_drive(&unescaped) {
        let root = format!("{unescaped}/");
        return SplitPath {
            req_dir: root.clone(),
            display_prefix: root,
            name_prefix: String::new(),
            quote,
        };
    }
    let sep_idx = if win_style || unescaped.contains('\\') {
        unescaped.rfind(['/', '\\'])
    } else {
        unescaped.rfind('/')
    };
    if let Some(idx) = sep_idx {
        let before = &unescaped[..idx];
        let with_sep = &unescaped[..=idx];
        let req_dir = if before.is_empty() || is_bare_drive(before) {
            with_sep.to_string()
        } else {
            before.to_string()
        };
        return SplitPath {
            req_dir,
            display_prefix: with_sep.to_string(),
            name_prefix: unescaped[idx + 1..].to_string(),
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
fn format_path_completion(
    sp: &SplitPath,
    entry: &str,
    is_dir: bool,
    shell_escape: bool,
    trailing_space_on_file: bool,
) -> String {
    let sep = if sp.display_prefix.ends_with('\\') {
        '\\'
    } else {
        '/'
    };
    let full = if is_dir {
        format!("{}{entry}{sep}", sp.display_prefix)
    } else {
        format!("{}{entry}", sp.display_prefix)
    };
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
        if cfg!(windows) || sep == '\\' || has_drive_prefix(&full) {
            return if is_dir {
                format!("\"{full}")
            } else if trailing_space_on_file {
                format!("\"{full}\" ")
            } else {
                format!("\"{full}\"")
            };
        }
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

    let sep = if sp.display_prefix.ends_with('\\') {
        '\\'
    } else {
        '/'
    };
    for d in &listing.dirs {
        if d.starts_with('.') && !show_hidden {
            continue;
        }
        let val = format_path_completion(sp, d, true, shell_escape, trailing_space_on_file);
        let label = format!("{}{d}{sep}", sp.display_prefix);
        let dl = d.to_lowercase();
        if q_lower.is_empty() || dl.starts_with(&q_lower) {
            prefix_matches.push((val, label, "dir".to_string()));
        } else if let Some(score) = text::fuzzy_score(d, &sp.name_prefix) {
            fuzzy_matches.push((
                score + DIR_FUZZY_SCORE_BONUS,
                (val, label, "dir".to_string()),
            ));
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
        .take(MAX_DIR_ITEMS)
        .collect()
}

/// Pops one trailing directory segment on Backspace at `/` or `\` when an earlier separator exists.
/// Returns `None` at `/`, `C:/`, `~/`, `./`, `../`, or single-segment `foo/` so normal backspace runs.
pub fn parent_dir_input(current: &str) -> Option<String> {
    if !current.ends_with(['/', '\\']) {
        return None;
    }
    let trimmed = current.trim_end_matches(['/', '\\']);
    let sep_idx = if cfg!(windows) || has_drive_prefix(trimmed) || current.contains('\\') {
        trimmed.rfind(['/', '\\'])
    } else {
        trimmed.rfind('/')
    };
    let idx = sep_idx?;
    Some(trimmed[..=idx].to_string())
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
    let mut it = line.char_indices().peekable();
    let mut quote: Option<char> = None;
    let mut seg_start = 0usize;
    let mut at_line_start = true;

    while let Some((idx, c)) = it.next() {
        if let Some(q) = quote {
            if c == '\\' && q == '"' {
                it.next();
                continue;
            }
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\\' => {
                it.next();
            }
            '\'' | '"' => {
                quote = Some(c);
            }
            ';' => {
                seg_start = idx + c.len_utf8();
                at_line_start = false;
            }
            '|' => {
                seg_start = if it.next_if(|&(_, nc)| nc == '|').is_some() {
                    idx + 2
                } else {
                    idx + 1
                };
                at_line_start = false;
            }
            '&' if it.next_if(|&(_, nc)| nc == '&').is_some() => {
                seg_start = idx + 2;
                at_line_start = false;
            }
            _ => {}
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

    #[test]
    fn windows_drive_backslash_tilde_and_parent_dir_input() {
        let check = |raw: &str, req: &str, pfx: &str, name: &str| {
            let sp = split_dir_query(raw);
            assert_eq!(
                (
                    sp.req_dir.as_str(),
                    sp.display_prefix.as_str(),
                    sp.name_prefix.as_str()
                ),
                (req, pfx, name),
                "raw={raw:?}"
            );
            sp
        };
        check("C:", "C:/", "C:/", "");
        check("C:/", "C:/", "C:/", "");
        check(r"C:\", r"C:\", r"C:\", "");
        let sp_win = check(r"C:\Users\al", r"C:\Users", r"C:\Users\", "al");
        check(r"~\app", "~", r"~\", "app");
        check(r"src\ui", "src", r"src\", "ui");
        check(r"my\ dir/sub", "my dir", "my dir/", "sub");

        // `format_path_completion` preserves backslash separator style and quotes spaces.
        assert_eq!(
            format_path_completion(&sp_win, "alice", true, false, false),
            r"C:\Users\alice\"
        );
        assert_eq!(
            format_path_completion(&sp_win, "Program Files", true, true, false),
            "\"C:\\Users\\Program Files\\"
        );

        // `parent_dir_input` pops only when an earlier separator exists; keeps drive roots intact.
        assert_eq!(parent_dir_input("~/app/").as_deref(), Some("~/"));
        assert_eq!(parent_dir_input(r"~\app\").as_deref(), Some(r"~\"));
        assert_eq!(parent_dir_input("/a/b/").as_deref(), Some("/a/"));
        assert_eq!(parent_dir_input("/a/").as_deref(), Some("/"));
        assert_eq!(parent_dir_input("a/b/").as_deref(), Some("a/"));
        assert_eq!(parent_dir_input("../../").as_deref(), Some("../"));
        assert_eq!(parent_dir_input("C:/Users/").as_deref(), Some("C:/"));
        assert_eq!(parent_dir_input(r"C:\Users\").as_deref(), Some(r"C:\"));
        for single in ["/", "C:/", r"C:\", "~/", r"~\", "./", "../", "foo/"] {
            assert_eq!(parent_dir_input(single), None, "single={single:?}");
        }
    }

    #[test]
    fn shell_token_context_utf8_escaped_multibyte_and_short_terminal_popup() {
        // `\中` must step over the full multibyte char without splitting UTF-8 bytes.
        assert_eq!(
            shell_token_context(r"echo \中 | gr"),
            ShellTokenPos::Command {
                start: r"echo \中 | ".len(),
                raw: "gr".into(),
                at_line_start: false,
            }
        );
        assert_eq!(
            shell_token_context("echo \"\\中\" arg"),
            ShellTokenPos::Arg {
                start: "echo \"\\中\" ".len(),
                raw: "arg".into(),
            }
        );

        // Short terminals: popup height `n + 2 <= chat_height` never overwrites `prompt_rect.y`.
        let items: Vec<_> = (0..15)
            .map(|i| (format!("v{i}"), format!("l{i}"), String::new()))
            .collect();
        for chat_h in [3u16, 4, 5, 6] {
            let mut ac =
                Autocomplete::new(AcKind::Slash, " commands ", items.clone(), 0, false, None)
                    .unwrap();
            let prompt_rect = Rect::new(1, chat_h, 40, 4);
            let mut buf = Buffer::empty(Rect::new(0, 0, 50, chat_h + 6));
            buf.set_string(2, prompt_rect.y, "PROMPT_TOP", Style::default());
            draw(&mut buf, prompt_rect, chat_h, &mut ac);
            assert!(
                ac.rect.height <= chat_h,
                "chat_h={chat_h} rect={:?}",
                ac.rect
            );
            assert!(
                ac.rect.y + ac.rect.height <= prompt_rect.y,
                "popup overlapped prompt_rect: rect={:?} prompt={prompt_rect:?}",
                ac.rect
            );
            assert_eq!(buf[(2, prompt_rect.y)].symbol(), "P");
        }
    }
}
