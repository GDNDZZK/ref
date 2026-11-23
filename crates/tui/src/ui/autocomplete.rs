//! Autocomplete popup state, 0-flicker directory/binary caches, path & shell token parsing, and rendering.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

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
    ShellArg,
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

const SHELL_CACHE_CAP: usize = 48;
const SHELL_CACHE_TTL: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellCompleteKey {
    pub cwd: String,
    pub line: String,
}

#[derive(Default)]
pub struct ShellCompleteCache {
    entries: VecDeque<(
        ShellCompleteKey,
        Instant,
        Vec<ref_proto::ShellCompletionItem>,
    )>,
}

impl ShellCompleteCache {
    pub fn get(&self, key: &ShellCompleteKey) -> Option<&[ref_proto::ShellCompletionItem]> {
        self.entries
            .iter()
            .find(|(k, at, _)| k == key && at.elapsed() < SHELL_CACHE_TTL)
            .map(|(_, _, v)| v.as_slice())
    }

    pub fn insert(&mut self, key: ShellCompleteKey, items: Vec<ref_proto::ShellCompletionItem>) {
        self.entries.retain(|(k, _, _)| k != &key);
        if self.entries.len() >= SHELL_CACHE_CAP {
            self.entries.pop_front();
        }
        self.entries.push_back((key, Instant::now(), items));
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Longest non-expired cached entry in `cwd` whose `line` is a prefix within the same token.
    pub fn longest_prefix(
        &self,
        cwd: &str,
        line: &str,
        min_len: usize,
    ) -> Option<&[ref_proto::ShellCompletionItem]> {
        self.entries
            .iter()
            .filter(|(k, at, _)| {
                at.elapsed() < SHELL_CACHE_TTL
                    && k.cwd == cwd
                    && k.line.len() >= min_len
                    && line.starts_with(&k.line)
                    && !line[k.line.len()..].contains(char::is_whitespace)
            })
            .max_by_key(|(k, _, _)| k.line.len())
            .map(|(_, _, v)| v.as_slice())
    }
}

/// Formats `CompleteShell` items for `AcKind::ShellArg`, appending a space unless ending with `/`, `\`, `=`, or `:`.
pub fn build_shell_arg_items(
    items: &[ref_proto::ShellCompletionItem],
    raw_prefix: &str,
    filter_prefix: bool,
    limit: usize,
) -> Vec<(String, String, String)> {
    let p_lower = raw_prefix.trim_start_matches(['"', '\'']).to_lowercase();
    items
        .iter()
        .filter(|it| {
            !filter_prefix || p_lower.is_empty() || it.value.to_lowercase().starts_with(&p_lower)
        })
        .take(limit)
        .map(|it| {
            let insert = if it.value.ends_with(['/', '\\', '=', ':', ' ']) {
                it.value.clone()
            } else {
                format!("{} ", it.value)
            };
            (insert, it.value.clone(), it.description.clone())
        })
        .collect()
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
    /// First command token (at start of line or after `|`, `||`, `&&`, `;`, or a wrapper).
    Command {
        start: usize,
        raw: String,
        at_line_start: bool,
    },
    /// Subsequent argument token after a command word, or a wrapper flag / flag-arg.
    Arg { start: usize, raw: String },
}

struct WrapperSpec {
    name: &'static str,
    arg_flags: &'static [&'static str],
    positional_args: usize,
}

const WRAPPERS: &[WrapperSpec] = &[
    WrapperSpec {
        name: "sudo",
        arg_flags: &[
            "-C",
            "--close-from",
            "-c",
            "--login-class",
            "-D",
            "--chdir",
            "-g",
            "--group",
            "-h",
            "--host",
            "-p",
            "--prompt",
            "-R",
            "--chroot",
            "-r",
            "--role",
            "-t",
            "--type",
            "-T",
            "--command-timeout",
            "-U",
            "--other-user",
            "-u",
            "--user",
        ],
        positional_args: 0,
    },
    WrapperSpec {
        name: "doas",
        arg_flags: &["-a", "-C", "-u"],
        positional_args: 0,
    },
    WrapperSpec {
        name: "env",
        arg_flags: &[
            "-C",
            "--chdir",
            "-P",
            "-S",
            "--split-string",
            "-u",
            "--unset",
            "--default-signal",
            "--ignore-signal",
            "--block-signal",
        ],
        positional_args: 0,
    },
    WrapperSpec {
        name: "nohup",
        arg_flags: &[],
        positional_args: 0,
    },
    WrapperSpec {
        name: "time",
        arg_flags: &["-f", "--format", "-o", "--output"],
        positional_args: 0,
    },
    WrapperSpec {
        name: "nice",
        arg_flags: &["-n", "--adjustment"],
        positional_args: 0,
    },
    WrapperSpec {
        name: "ionice",
        arg_flags: &[
            "-c",
            "--class",
            "-n",
            "--classdata",
            "-P",
            "--pgid",
            "-p",
            "--pid",
            "-u",
            "--uid",
        ],
        positional_args: 0,
    },
    WrapperSpec {
        name: "timeout",
        arg_flags: &["-k", "--kill-after", "-s", "--signal"],
        positional_args: 1,
    },
    WrapperSpec {
        name: "setsid",
        arg_flags: &[],
        positional_args: 0,
    },
    WrapperSpec {
        name: "strace",
        arg_flags: &[
            "-A", "-b", "-E", "-e", "-I", "-o", "-P", "-p", "-S", "-s", "-U", "-u",
        ],
        positional_args: 0,
    },
    WrapperSpec {
        name: "ltrace",
        arg_flags: &[
            "-A", "-a", "-e", "-F", "-l", "-n", "-o", "-p", "-s", "-u", "-x",
        ],
        positional_args: 0,
    },
    WrapperSpec {
        name: "chrt",
        arg_flags: &["-D", "-P", "-T"],
        positional_args: 1,
    },
    WrapperSpec {
        name: "taskset",
        arg_flags: &[],
        positional_args: 1,
    },
    WrapperSpec {
        name: "stdbuf",
        arg_flags: &["-e", "--error", "-i", "--input", "-o", "--output"],
        positional_args: 0,
    },
    WrapperSpec {
        name: "watch",
        arg_flags: &["-n", "--interval"],
        positional_args: 0,
    },
    WrapperSpec {
        name: "command",
        arg_flags: &[],
        positional_args: 0,
    },
    WrapperSpec {
        name: "exec",
        arg_flags: &["-a"],
        positional_args: 0,
    },
    WrapperSpec {
        name: "builtin",
        arg_flags: &[],
        positional_args: 0,
    },
    WrapperSpec {
        name: "xargs",
        arg_flags: &[
            "-a",
            "--arg-file",
            "-d",
            "--delimiter",
            "-E",
            "-e",
            "-I",
            "-L",
            "-l",
            "-n",
            "--max-args",
            "-P",
            "--max-procs",
            "-s",
            "--max-chars",
        ],
        positional_args: 0,
    },
    WrapperSpec {
        name: "caffeinate",
        arg_flags: &["-t", "-w"],
        positional_args: 0,
    },
    WrapperSpec {
        name: "pkexec",
        arg_flags: &["--user"],
        positional_args: 0,
    },
];

fn find_wrapper(tok: &str) -> Option<&'static WrapperSpec> {
    let base = tok.rsplit(['/', '\\']).next().unwrap_or(tok);
    WRAPPERS.iter().find(|w| w.name == base)
}

/// True when a wrapper option token consumes the following whitespace-separated token as its value.
fn wrapper_flag_takes_next_arg(w: &WrapperSpec, tok: &str) -> bool {
    if tok.contains('=') {
        return false;
    }
    if tok.starts_with("--") {
        return w.arg_flags.contains(&tok);
    }
    let Some(short) = tok.strip_prefix('-') else {
        return false;
    };
    for (idx, ch) in short.char_indices() {
        let is_arg_flag = w
            .arg_flags
            .iter()
            .any(|f| f.len() == 2 && f.starts_with('-') && f.as_bytes()[1] as char == ch);
        if is_arg_flag {
            // Remaining characters in the same token are an attached value (e.g. `-Euroot`, `-vk5s`).
            return idx + ch.len_utf8() >= short.len();
        }
    }
    false
}

fn is_env_assign(tok: &str) -> bool {
    tok.split_once('=').is_some_and(|(k, _)| {
        !k.is_empty()
            && k.bytes().enumerate().all(|(idx, b)| {
                b == b'_' || b.is_ascii_alphabetic() || (idx > 0 && b.is_ascii_digit())
            })
    })
}

#[derive(Default)]
struct SegScan {
    in_args: bool,
    wrapped: bool,
    wrapper: Option<&'static WrapperSpec>,
    expect_flag_arg: bool,
    end_of_opts: bool,
    positional_left: usize,
}

impl SegScan {
    fn enter_wrapper(&mut self, w: &'static WrapperSpec) {
        self.wrapped = true;
        self.wrapper = Some(w);
        self.expect_flag_arg = false;
        self.end_of_opts = false;
        self.positional_left = w.positional_args;
    }

    /// Updates segment state after a completed whitespace-terminated token `tok`.
    fn advance(&mut self, tok: &str) {
        if self.in_args {
            return;
        }
        if self.expect_flag_arg {
            self.expect_flag_arg = false;
            return;
        }
        let Some(w) = self.wrapper else {
            if is_env_assign(tok) {
                return;
            }
            if let Some(w) = find_wrapper(tok) {
                self.enter_wrapper(w);
            } else {
                self.in_args = true;
            }
            return;
        };
        if !self.end_of_opts && tok == "--" {
            self.end_of_opts = true;
            return;
        }
        if !self.end_of_opts && tok.starts_with('-') {
            if wrapper_flag_takes_next_arg(w, tok) {
                self.expect_flag_arg = true;
            }
            return;
        }
        if self.positional_left > 0 {
            self.positional_left -= 1;
            return;
        }
        if is_env_assign(tok) {
            return;
        }
        if let Some(next_w) = find_wrapper(tok) {
            self.enter_wrapper(next_w);
            return;
        }
        self.in_args = true;
    }

    /// Classifies the current (possibly empty) token being typed at the cursor.
    fn classify(&self, start: usize, raw: String, at_line_start: bool) -> ShellTokenPos {
        let typing_wrapper_flag =
            self.wrapper.is_some() && !self.end_of_opts && raw.starts_with('-');
        let is_arg =
            self.in_args || self.expect_flag_arg || self.positional_left > 0 || typing_wrapper_flag;
        if is_arg {
            ShellTokenPos::Arg { start, raw }
        } else {
            ShellTokenPos::Command {
                start,
                raw,
                at_line_start: at_line_start && !self.wrapped,
            }
        }
    }
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

    // Within `line[seg_start..]`, scan words (skipping `VAR=val` assignments and command wrappers).
    let seg = &line[seg_start..];
    let mut pos = 0usize;
    let mut scan = SegScan::default();
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
            return scan.classify(abs, String::new(), at_line_start);
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
            return scan.classify(abs, tok.to_string(), at_line_start);
        }
        scan.advance(tok);
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

    #[test]
    fn shell_token_context_wrappers_flags_durations_and_chains() {
        assert_eq!(
            shell_token_context("sudo car"),
            ShellTokenPos::Command {
                start: 5,
                raw: "car".into(),
                at_line_start: false,
            }
        );
        assert_eq!(
            shell_token_context("sudo -u root car"),
            ShellTokenPos::Command {
                start: "sudo -u root ".len(),
                raw: "car".into(),
                at_line_start: false,
            }
        );
        // Typing wrapper `-` flag or flag argument stays in `Arg` (no premature binary list).
        assert_eq!(
            shell_token_context("sudo -"),
            ShellTokenPos::Arg {
                start: 5,
                raw: "-".into(),
            }
        );
        assert_eq!(
            shell_token_context("sudo -u "),
            ShellTokenPos::Arg {
                start: 8,
                raw: String::new(),
            }
        );
        assert_eq!(
            shell_token_context("sudo -u ro"),
            ShellTokenPos::Arg {
                start: 8,
                raw: "ro".into(),
            }
        );
        assert_eq!(
            shell_token_context("sudo --user=root car"),
            ShellTokenPos::Command {
                start: "sudo --user=root ".len(),
                raw: "car".into(),
                at_line_start: false,
            }
        );
        assert_eq!(
            shell_token_context("sudo -- car"),
            ShellTokenPos::Command {
                start: "sudo -- ".len(),
                raw: "car".into(),
                at_line_start: false,
            }
        );

        // `timeout` consumes 1 positional duration before the command.
        assert_eq!(
            shell_token_context("timeout "),
            ShellTokenPos::Arg {
                start: 8,
                raw: String::new(),
            }
        );
        assert_eq!(
            shell_token_context("timeout 5s"),
            ShellTokenPos::Arg {
                start: 8,
                raw: "5s".into(),
            }
        );
        assert_eq!(
            shell_token_context("timeout 5s "),
            ShellTokenPos::Command {
                start: 11,
                raw: String::new(),
                at_line_start: false,
            }
        );
        assert_eq!(
            shell_token_context("timeout -s TERM 5s car"),
            ShellTokenPos::Command {
                start: "timeout -s TERM 5s ".len(),
                raw: "car".into(),
                at_line_start: false,
            }
        );

        // Chained wrappers + `VAR=val` assignments + arguments after wrapped command.
        assert_eq!(
            shell_token_context("sudo env -u PATH RUST_LOG=debug car"),
            ShellTokenPos::Command {
                start: "sudo env -u PATH RUST_LOG=debug ".len(),
                raw: "car".into(),
                at_line_start: false,
            }
        );
        assert_eq!(
            shell_token_context("sudo cargo test src/"),
            ShellTokenPos::Arg {
                start: "sudo cargo test ".len(),
                raw: "src/".into(),
            }
        );
        assert_eq!(
            shell_token_context("echo hi | sudo gr"),
            ShellTokenPos::Command {
                start: "echo hi | sudo ".len(),
                raw: "gr".into(),
                at_line_start: false,
            }
        );

        // Remaining wrappers.
        for prefix in [
            "doas -u admin ",
            "env -i FOO=bar ",
            "nohup ",
            "time -p ",
            "nice -n 10 ",
            "nice -10 ",
            "ionice -c 2 -n 0 ",
            "setsid -w ",
            "strace -e trace=file ",
            "ltrace -o out.log ",
            "chrt -f 99 ",
            "taskset -c 0-3 ",
            "stdbuf -o L ",
            "watch -n 1 ",
            "command -p ",
            "exec -a name ",
            "builtin ",
            "xargs -I {} ",
            "caffeinate -t 5 ",
            "pkexec --user root ",
        ] {
            let line = format!("{prefix}car");
            assert_eq!(
                shell_token_context(&line),
                ShellTokenPos::Command {
                    start: prefix.len(),
                    raw: "car".into(),
                    at_line_start: false,
                },
                "line={line:?}"
            );
        }
    }

    #[test]
    fn shell_token_context_path_prefixed_and_clustered_short_flags() {
        // Path-prefixed wrappers match by basename.
        for prefix in [
            "/usr/bin/sudo ",
            "/bin/env FOO=bar ",
            "/usr/bin/timeout 5s ",
            "/usr/bin/time -f '%e' ",
            r"C:\tools\sudo -u root ",
        ] {
            let line = format!("{prefix}car");
            assert_eq!(
                shell_token_context(&line),
                ShellTokenPos::Command {
                    start: prefix.len(),
                    raw: "car".into(),
                    at_line_start: false,
                },
                "line={line:?}"
            );
        }

        // Clustered short flags ending in an arg-taking flag consume the next token.
        for prefix in [
            "sudo -Eu root ",
            "sudo -iu postgres ",
            "env -iC /tmp ",
            "xargs -0n 1 ",
            "xargs -0I {} ",
            // Attached values within the same token do NOT consume the next token.
            "sudo -Euroot ",
            "timeout -vk5s 10s ",
            "stdbuf -oL ",
        ] {
            let line = format!("{prefix}car");
            assert_eq!(
                shell_token_context(&line),
                ShellTokenPos::Command {
                    start: prefix.len(),
                    raw: "car".into(),
                    at_line_start: false,
                },
                "line={line:?}"
            );
        }

        // Mid-argument while typing after a clustered arg-taking short flag stays in `Arg`.
        assert_eq!(
            shell_token_context("sudo -Eu "),
            ShellTokenPos::Arg {
                start: "sudo -Eu ".len(),
                raw: String::new(),
            }
        );
        assert_eq!(
            shell_token_context("sudo -Eu ro"),
            ShellTokenPos::Arg {
                start: "sudo -Eu ".len(),
                raw: "ro".into(),
            }
        );
        assert_eq!(
            shell_token_context("env -iC "),
            ShellTokenPos::Arg {
                start: "env -iC ".len(),
                raw: String::new(),
            }
        );
    }
}
