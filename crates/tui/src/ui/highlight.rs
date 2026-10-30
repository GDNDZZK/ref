//! Syntax highlighting for code blocks (syntect) with an opencode-like palette and an
//! incremental cache so growing (streaming) code blocks only highlight new lines.

use std::sync::{Mutex, OnceLock};

use ratatui::style::{Color, Modifier, Style};
use syntect::highlighting::{
    Color as SColor, FontStyle, HighlightState, Highlighter, RangedHighlightIterator,
    ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSettings,
};
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet};

use super::theme;

fn syntaxes() -> &'static SyntaxSet {
    static S: OnceLock<SyntaxSet> = OnceLock::new();
    S.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn scolor(c: Color) -> SColor {
    match c {
        Color::Rgb(r, g, b) => SColor { r, g, b, a: 255 },
        _ => SColor::WHITE,
    }
}

fn hl_theme() -> &'static Theme {
    static T: OnceLock<Theme> = OnceLock::new();
    T.get_or_init(|| {
        let item = |sel: &str, c: Color, fs: Option<FontStyle>| -> Option<ThemeItem> {
            Some(ThemeItem {
                scope: sel.parse::<ScopeSelectors>().ok()?,
                style: StyleModifier { foreground: Some(scolor(c)), background: None, font_style: fs },
            })
        };
        let yellow = theme::rgb(0xe5c07b);
        let scopes = [
            item("comment, punctuation.definition.comment", theme::MUTED, Some(FontStyle::ITALIC)),
            item("keyword, storage, keyword.control, storage.type, storage.modifier", theme::ACCENT, None),
            item("keyword.operator", theme::INFO, None),
            item("string, string.quoted, punctuation.definition.string", theme::SUCCESS, None),
            item("constant.numeric, constant.language, constant.character", theme::WARNING, None),
            item("entity.name.function, support.function, meta.function-call variable.function", theme::PRIMARY, None),
            item("entity.name.type, entity.name.class, support.type, support.class, entity.other.inherited-class", yellow, None),
            item("variable.parameter, variable.other.member", theme::ERROR, None),
            item("entity.name.tag", theme::ERROR, None),
            item("entity.other.attribute-name", theme::WARNING, None),
            item("markup.heading", theme::ACCENT, Some(FontStyle::BOLD)),
            item("markup.inserted", theme::DIFF_ADDED, None),
            item("markup.deleted", theme::DIFF_REMOVED, None),
            item("meta.diff.range, meta.diff.header", theme::INFO, None),
            item("support.constant, variable.language", theme::WARNING, None),
            item("entity.name.namespace, entity.name.module", yellow, None),
            item("punctuation", theme::TEXT, None),
        ];
        Theme {
            name: Some("ref".into()),
            author: None,
            settings: ThemeSettings { foreground: Some(scolor(theme::TEXT)), ..Default::default() },
            scopes: scopes.into_iter().flatten().collect(),
        }
    })
}

fn find_syntax(lang: &str) -> Option<&'static SyntaxReference> {
    let ss = syntaxes();
    let l = lang.trim().to_ascii_lowercase();
    let l = l
        .split(|c: char| c == ',' || c.is_whitespace())
        .next()
        .unwrap_or("");
    if l.is_empty() {
        return None;
    }
    let alias = match l {
        "rs" => "rust",
        "ts" | "tsx" | "typescript" | "jsx" | "mjs" | "cjs" => "js",
        "py" | "python3" => "python",
        "sh" | "shell" | "zsh" | "console" | "shellsession" => "bash",
        "yml" => "yaml",
        "c++" | "hpp" | "cc" => "cpp",
        "golang" => "go",
        "md" => "markdown",
        "jsonc" | "json5" => "json",
        "toml" => "toml",
        other => other,
    };
    ss.find_syntax_by_token(alias)
        .or_else(|| ss.find_syntax_by_extension(alias))
}

pub type HlLine = Vec<(Style, String)>;

struct Entry {
    lang: String,
    lines: Vec<String>,
    /// State after each line.
    states: Vec<(ParseState, HighlightState)>,
    out: Vec<HlLine>,
}

fn cache() -> &'static Mutex<Vec<Entry>> {
    static C: OnceLock<Mutex<Vec<Entry>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(Vec::new()))
}

fn conv(s: syntect::highlighting::Style) -> Style {
    let mut st = Style::new().fg(Color::Rgb(s.foreground.r, s.foreground.g, s.foreground.b));
    if s.font_style.contains(FontStyle::BOLD) {
        st = st.add_modifier(Modifier::BOLD);
    }
    if s.font_style.contains(FontStyle::ITALIC) {
        st = st.add_modifier(Modifier::ITALIC);
    }
    st
}

/// Highlights `code` (lines without trailing newline). Returns None if the language is unknown.
pub fn highlight(lang: &str, lines: &[&str]) -> Option<Vec<HlLine>> {
    let syntax = find_syntax(lang)?;
    // Very large blocks: skip highlighting for responsiveness.
    if lines.len() > 3000 || lines.iter().any(|l| l.len() > 4000) {
        return None;
    }
    let th = hl_theme();
    let highlighter = Highlighter::new(th);
    let mut guard = cache().lock().ok()?;
    // Best entry: same lang with the longest common prefix.
    let mut best: Option<(usize, usize)> = None;
    for (i, e) in guard.iter().enumerate() {
        if e.lang != lang {
            continue;
        }
        let common = e
            .lines
            .iter()
            .zip(lines.iter())
            .take_while(|(a, b)| a.as_str() == **b)
            .count();
        if best.is_none_or(|(_, c)| common > c) {
            best = Some((i, common));
        }
    }
    let (mut entry, common) = match best {
        Some((i, c)) if c > 0 => (guard.swap_remove(i), c),
        _ => (
            Entry {
                lang: lang.to_string(),
                lines: Vec::new(),
                states: Vec::new(),
                out: Vec::new(),
            },
            0,
        ),
    };
    entry.lines.truncate(common);
    entry.states.truncate(common);
    entry.out.truncate(common);
    let (mut ps, mut hs) = match entry.states.last() {
        Some((p, h)) => (p.clone(), h.clone()),
        None => (
            ParseState::new(syntax),
            HighlightState::new(&highlighter, ScopeStack::new()),
        ),
    };
    for line in &lines[common..] {
        let with_nl = format!("{line}\n");
        let ops = ps.parse_line(&with_nl, syntaxes()).unwrap_or_default();
        let mut hl: HlLine = Vec::new();
        for (style, piece, _) in RangedHighlightIterator::new(&mut hs, &ops, &with_nl, &highlighter)
        {
            let piece = piece.trim_end_matches('\n');
            if !piece.is_empty() {
                hl.push((conv(style), piece.to_string()));
            }
        }
        entry.lines.push(line.to_string());
        entry.states.push((ps.clone(), hs.clone()));
        entry.out.push(hl);
    }
    let out = entry.out.clone();
    guard.push(entry);
    if guard.len() > 24 {
        guard.remove(0);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn highlights_rust_incrementally() {
        let a = super::highlight("rust", &["fn main() {", "    let x = 1;"]).unwrap();
        assert_eq!(a.len(), 2);
        let b = super::highlight("rust", &["fn main() {", "    let x = 1;", "}"]).unwrap();
        assert_eq!(b.len(), 3);
        assert_eq!(a[0], b[0]);
        assert!(super::highlight("nosuchlang", &["x"]).is_none());
    }
}
