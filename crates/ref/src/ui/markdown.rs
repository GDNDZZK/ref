//! Markdown → wrapped ratatui lines (pulldown-cmark), unicode-width aware.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use super::text::{pad_line, sanitize, width, wrap_spans};
use super::theme::*;

enum Container {
    Quote,
    Item { marker: String, first: bool },
}

struct List {
    next: Option<u64>,
}

struct Table {
    aligns: Vec<pulldown_cmark::Alignment>,
    rows: Vec<Vec<Vec<Span<'static>>>>,
    head_rows: usize,
    in_head: bool,
}

struct R {
    width: usize,
    base: Style,
    highlight: bool,
    out: Vec<Line<'static>>,
    containers: Vec<Container>,
    lists: Vec<List>,
    inline: Vec<Span<'static>>,
    styles: Vec<Style>,
    pending_blank: bool,
    heading: Option<HeadingLevel>,
    code: Option<(String, String)>,
    link: Vec<String>,
    table: Option<Table>,
    cell: Vec<Span<'static>>,
}

impl R {
    fn style(&self) -> Style {
        *self.styles.last().unwrap_or(&self.base)
    }

    fn push_style(&mut self, f: impl FnOnce(Style) -> Style) {
        let s = f(self.style());
        self.styles.push(s);
    }

    fn push_text(&mut self, t: &str, style: Style) {
        let t = sanitize(t);
        if t.is_empty() {
            return;
        }
        let target = if self.table.is_some() { &mut self.cell } else { &mut self.inline };
        if let Some(last) = target.last_mut() {
            if last.style == style {
                let mut c = last.content.to_string();
                c.push_str(&t);
                last.content = c.into();
                return;
            }
        }
        target.push(Span::styled(t, style));
    }

    /// Prefix spans for the next line: (first-line prefix, continuation prefix). Consumes item markers.
    fn prefixes(&mut self) -> (Vec<Span<'static>>, Vec<Span<'static>>) {
        let mut first = Vec::new();
        let mut cont = Vec::new();
        for c in self.containers.iter_mut() {
            match c {
                Container::Quote => {
                    let s = Span::styled("│ ", fg(MD_QUOTE));
                    first.push(s.clone());
                    cont.push(s);
                }
                Container::Item { marker, first: f } => {
                    let pad = " ".repeat(width(marker));
                    if *f {
                        let col = if marker.trim_end().ends_with('.') { MD_ENUM } else { MD_LIST };
                        first.push(Span::styled(marker.clone(), fg(col)));
                        *f = false;
                    } else {
                        first.push(Span::raw(pad.clone()));
                    }
                    cont.push(Span::raw(pad));
                }
            }
        }
        (first, cont)
    }

    /// Prefix used for blank separator lines (quote bars only).
    fn blank_prefix(&self) -> Vec<Span<'static>> {
        let mut v = Vec::new();
        for c in &self.containers {
            match c {
                Container::Quote => v.push(Span::styled("│", fg(MD_QUOTE))),
                Container::Item { marker, .. } => v.push(Span::raw(" ".repeat(width(marker)))),
            }
        }
        v
    }

    fn start_block(&mut self) {
        if self.pending_blank && !self.out.is_empty() {
            let mut p = self.blank_prefix();
            // Trim trailing raw padding.
            while matches!(p.last(), Some(s) if s.content.trim().is_empty()) {
                p.pop();
            }
            self.out.push(Line::from(p));
        }
        self.pending_blank = false;
    }

    fn flush_inline(&mut self) {
        if self.inline.is_empty() {
            return;
        }
        self.start_block();
        let spans = std::mem::take(&mut self.inline);
        // Split on explicit newlines (hard breaks are inserted as '\n').
        let mut logical: Vec<Vec<Span<'static>>> = vec![vec![]];
        for s in spans {
            let parts: Vec<&str> = s.content.split('\n').collect();
            for (i, p) in parts.iter().enumerate() {
                if i > 0 {
                    logical.push(vec![]);
                }
                if !p.is_empty() {
                    logical.last_mut().unwrap().push(Span::styled(p.to_string(), s.style));
                }
            }
        }
        for l in logical {
            let (first, cont) = self.prefixes();
            let lines = wrap_spans(&l, self.width, &first, &cont);
            self.out.extend(lines);
        }
    }

    /// Fenced/indented code: a panel with a left bar, the language label on the top padding row, the code
    /// (syntax highlighted, hard-wrapped) indented by one column, and a bottom padding row.
    fn render_code(&mut self, lang: &str, code: &str) {
        self.start_block();
        let (first, cont) = self.prefixes();
        let pw: usize = first.iter().map(|s| width(&s.content)).sum();
        let inner = self.width.saturating_sub(pw).max(6);
        let bg = Style::new().bg(ELEMENT);
        let bar = Span::styled("▎", Style::new().fg(BORDER_ACTIVE).bg(ELEMENT));
        let total = self.width.max(pw + 6);
        let code = code.strip_suffix('\n').unwrap_or(code);
        let mut header = first.clone();
        header.push(bar.clone());
        if !lang.is_empty() {
            header.push(Span::styled(format!(" {}", super::text::truncate(lang, inner.saturating_sub(3))), bg.fg(MUTED)));
        }
        self.out.push(pad_line(Line::from(header), total, bg));
        let hl = if self.highlight { highlight(lang, code) } else { None };
        // Columns used by the bar + left padding; one column of right padding is kept free.
        let lead = 2usize;
        let avail = inner.saturating_sub(lead + 1).max(1);
        for (i, l) in code.split('\n').enumerate() {
            let segs: Vec<(String, Color)> = match &hl {
                Some(h) => h.get(i).cloned().unwrap_or_default(),
                None => vec![(l.to_string(), MD_CODE_BLOCK)],
            };
            // Hard-wrap the styled segments.
            let start = || vec![bar.clone(), Span::styled(" ", bg)];
            let mut lines: Vec<Vec<Span<'static>>> = vec![start()];
            let mut w = 0usize;
            for (s, c) in segs {
                for g in unicode_segmentation::UnicodeSegmentation::graphemes(sanitize(&s).as_str(), true) {
                    let gw = width(g);
                    if w + gw > avail && w > 0 {
                        lines.push(start());
                        w = 0;
                    }
                    let st = bg.fg(c);
                    let cur = lines.last_mut().unwrap();
                    match cur.last_mut() {
                        Some(sp) if sp.style == st => {
                            let mut t = sp.content.to_string();
                            t.push_str(g);
                            sp.content = t.into();
                        }
                        _ => cur.push(Span::styled(g.to_string(), st)),
                    }
                    w += gw;
                }
            }
            for spans in lines {
                let mut l = cont.clone();
                l.extend(spans);
                self.out.push(pad_line(Line::from(l), total, bg));
            }
        }
        let mut footer = cont.clone();
        footer.push(bar);
        self.out.push(pad_line(Line::from(footer), total, bg));
        self.pending_blank = true;
    }

    fn render_table(&mut self, t: Table) {
        self.start_block();
        let ncols = t.rows.iter().map(|r| r.len()).max().unwrap_or(0);
        if ncols == 0 {
            return;
        }
        let cell_w = |c: &Vec<Span<'static>>| c.iter().map(|s| width(&s.content)).sum::<usize>();
        let mut natural = vec![0usize; ncols];
        for r in &t.rows {
            for (i, c) in r.iter().enumerate() {
                natural[i] = natural[i].max(cell_w(c));
            }
        }
        let (first, cont) = self.prefixes();
        let pw: usize = first.iter().map(|s| width(&s.content)).sum();
        let chrome = 3 * ncols + 1;
        let avail = self.width.saturating_sub(pw + chrome);
        let bst = fg(BORDER);
        let Some(widths) = fit_columns(&natural, avail) else {
            // Hopelessly narrow: raw pipe rows, wrapped.
            for (ri, r) in t.rows.iter().enumerate() {
                let mut spans = vec![Span::styled("| ", bst)];
                for c in r {
                    spans.extend(c.iter().cloned());
                    spans.push(Span::styled(" | ", bst));
                }
                let (f, c) = if ri == 0 { (first.clone(), cont.clone()) } else { (cont.clone(), cont.clone()) };
                self.out.extend(wrap_spans(&spans, self.width, &f, &c));
            }
            self.pending_blank = true;
            return;
        };
        let border = |l: &str, m: &str, r: &str| -> Line<'static> {
            let mut s = String::from(l);
            for (i, w) in widths.iter().enumerate() {
                s.push_str(&"─".repeat(w + 2));
                s.push_str(if i + 1 == ncols { r } else { m });
            }
            let mut v = cont.clone();
            v.push(Span::styled(s, bst));
            Line::from(v)
        };
        let mut top = border("┌", "┬", "┐");
        top.spans.splice(0..cont.len(), first.clone());
        self.out.push(top);
        let empty = vec![];
        for (ri, r) in t.rows.iter().enumerate() {
            // Wrap every cell to its column width; the row is as tall as its tallest cell.
            let cells: Vec<Vec<Line<'static>>> = widths
                .iter()
                .enumerate()
                .map(|(i, w)| {
                    let mut c: Vec<Span<'static>> = r.get(i).unwrap_or(&empty).clone();
                    if ri < t.head_rows {
                        for s in c.iter_mut() {
                            s.style = s.style.add_modifier(Modifier::BOLD);
                        }
                    }
                    if cell_w(&c) <= *w {
                        vec![Line::from(c)]
                    } else {
                        wrap_spans(&c, *w, &[], &[])
                    }
                })
                .collect();
            let h = cells.iter().map(|c| c.len()).max().unwrap_or(1).max(1);
            for li in 0..h {
                let mut v = cont.clone();
                v.push(Span::styled("│", bst));
                for (i, w) in widths.iter().enumerate() {
                    let line = cells[i].get(li);
                    let cw = line.map(|l| l.spans.iter().map(|s| width(&s.content)).sum::<usize>()).unwrap_or(0);
                    let pad = w.saturating_sub(cw);
                    let (lp, rp) = match t.aligns.get(i) {
                        Some(pulldown_cmark::Alignment::Right) => (pad, 0),
                        Some(pulldown_cmark::Alignment::Center) => (pad / 2, pad - pad / 2),
                        _ => (0, pad),
                    };
                    v.push(Span::raw(" ".repeat(lp + 1)));
                    if let Some(l) = line {
                        v.extend(l.spans.iter().cloned());
                    }
                    v.push(Span::raw(" ".repeat(rp + 1)));
                    v.push(Span::styled("│", bst));
                }
                self.out.push(Line::from(v));
            }
            if ri + 1 == t.head_rows && t.rows.len() > t.head_rows {
                self.out.push(border("├", "┼", "┤"));
            }
        }
        self.out.push(border("└", "┴", "┘"));
        self.pending_blank = true;
    }
}

/// Column widths for a table with `natural` content widths in `avail` columns of content space.
/// Narrow columns keep their natural width; wide ones share the rest (water-filling). None if even
/// 3 columns per cell don't fit.
fn fit_columns(natural: &[usize], avail: usize) -> Option<Vec<usize>> {
    let n = natural.len();
    let natural: Vec<usize> = natural.iter().map(|w| (*w).max(1)).collect();
    if natural.iter().sum::<usize>() <= avail {
        return Some(natural);
    }
    let min = 3usize;
    if avail < n * min {
        return None;
    }
    let mut out = vec![0usize; n];
    let mut fixed = vec![false; n];
    let mut left = avail;
    loop {
        let open: Vec<usize> = (0..n).filter(|i| !fixed[*i]).collect();
        if open.is_empty() {
            break;
        }
        let share = left / open.len();
        let mut changed = false;
        for &i in &open {
            if natural[i] <= share {
                out[i] = natural[i];
                fixed[i] = true;
                left -= natural[i];
                changed = true;
            }
        }
        if !changed {
            let share = (left / open.len()).max(min);
            let mut extra = left.saturating_sub(share * open.len());
            for &i in &open {
                out[i] = share + if extra > 0 { 1 } else { 0 };
                extra = extra.saturating_sub(1);
            }
            break;
        }
    }
    Some(out)
}

/// For text that is still streaming: close inline markers (`**`, `` ` ``, `~~`) left open in the
/// trailing paragraph so partially received emphasis renders styled instead of flashing raw markers.
/// Text inside an unclosed code fence is left untouched.
pub fn close_partial(src: &str) -> std::borrow::Cow<'_, str> {
    let fences = src.lines().filter(|l| {
        let t = l.trim_start();
        t.starts_with("```") || t.starts_with("~~~")
    });
    if fences.count() % 2 == 1 {
        return src.into();
    }
    let para = &src[src.rfind("\n\n").map(|p| p + 2).unwrap_or(0)..];
    if para.trim_start().starts_with('|') || para.trim_start().starts_with("    ") {
        return src.into();
    }
    // Inline code spans first: markers inside them don't count.
    let ticks = para.matches('`').count();
    let mut suffix = String::new();
    let outside: String = para.split('`').step_by(2).collect();
    if ticks % 2 == 1 {
        suffix.push('`');
    } else {
        if outside.matches("**").count() % 2 == 1 && !para.trim_end().ends_with("**") {
            suffix.push_str("**");
        }
        if outside.matches("~~").count() % 2 == 1 {
            suffix.push_str("~~");
        }
    }
    if suffix.is_empty() || para.trim().is_empty() {
        return src.into();
    }
    format!("{src}{suffix}").into()
}

/// Render markdown into lines that fit `width` columns.
pub fn render(src: &str, w: usize, base: Style, highlight: bool) -> Vec<Line<'static>> {
    let w = w.max(8);
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    let mut r = R {
        width: w,
        base,
        highlight,
        out: Vec::new(),
        containers: Vec::new(),
        lists: Vec::new(),
        inline: Vec::new(),
        styles: Vec::new(),
        pending_blank: false,
        heading: None,
        code: None,
        link: Vec::new(),
        table: None,
        cell: Vec::new(),
    };
    for ev in Parser::new_ext(src, opts) {
        match ev {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {
                    r.flush_inline();
                }
                Tag::Heading { level, .. } => {
                    r.flush_inline();
                    r.heading = Some(level);
                    let st = match level {
                        HeadingLevel::H1 => bold(MD_HEADING).add_modifier(Modifier::UNDERLINED),
                        HeadingLevel::H2 => bold(MD_HEADING),
                        _ => bold(TEXT),
                    };
                    r.styles.push(st);
                }
                Tag::BlockQuote(_) => {
                    r.flush_inline();
                    r.start_block();
                    r.containers.push(Container::Quote);
                    r.push_style(|s| s.fg(MD_QUOTE).add_modifier(Modifier::ITALIC));
                }
                Tag::CodeBlock(kind) => {
                    r.flush_inline();
                    let lang = match kind {
                        CodeBlockKind::Fenced(l) => l.split([',', ' ']).next().unwrap_or("").to_string(),
                        CodeBlockKind::Indented => String::new(),
                    };
                    r.code = Some((lang, String::new()));
                }
                Tag::List(start) => {
                    r.flush_inline();
                    if r.lists.is_empty() {
                        r.start_block();
                    }
                    r.lists.push(List { next: start });
                }
                Tag::Item => {
                    r.flush_inline();
                    let depth = r.lists.len();
                    let marker = match r.lists.last_mut() {
                        Some(List { next: Some(n) }) => {
                            let m = format!("{n}. ");
                            *n += 1;
                            m
                        }
                        _ => match depth {
                            0 | 1 => "• ".to_string(),
                            2 => "◦ ".to_string(),
                            _ => "▪ ".to_string(),
                        },
                    };
                    r.containers.push(Container::Item { marker, first: true });
                }
                Tag::Emphasis => r.push_style(|s| s.fg(MD_EMPH).add_modifier(Modifier::ITALIC)),
                Tag::Strong => r.push_style(|s| s.fg(MD_STRONG).add_modifier(Modifier::BOLD)),
                Tag::Strikethrough => r.push_style(|s| s.add_modifier(Modifier::CROSSED_OUT)),
                Tag::Link { dest_url, .. } => {
                    r.link.push(dest_url.to_string());
                    r.push_style(|s| s.fg(MD_LINK_TEXT).add_modifier(Modifier::UNDERLINED));
                }
                Tag::Image { dest_url, .. } => {
                    r.link.push(dest_url.to_string());
                    let st = fg(MD_IMAGE_TEXT);
                    r.push_text("[image: ", st);
                    r.styles.push(st);
                }
                Tag::Table(aligns) => {
                    r.flush_inline();
                    r.table = Some(Table { aligns, rows: vec![], head_rows: 0, in_head: false });
                }
                Tag::TableHead => {
                    if let Some(t) = r.table.as_mut() {
                        t.in_head = true;
                        t.rows.push(vec![]);
                    }
                }
                Tag::TableRow => {
                    if let Some(t) = r.table.as_mut() {
                        t.rows.push(vec![]);
                    }
                }
                Tag::TableCell => r.cell.clear(),
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph => {
                    r.flush_inline();
                    r.pending_blank = true;
                }
                TagEnd::Heading(_) => {
                    r.flush_inline();
                    r.styles.pop();
                    r.heading = None;
                    r.pending_blank = true;
                }
                TagEnd::BlockQuote(_) => {
                    r.flush_inline();
                    r.containers.pop();
                    r.styles.pop();
                    r.pending_blank = true;
                }
                TagEnd::CodeBlock => {
                    if let Some((lang, code)) = r.code.take() {
                        r.render_code(&lang, &code);
                    }
                }
                TagEnd::List(_) => {
                    r.flush_inline();
                    r.lists.pop();
                    if r.lists.is_empty() {
                        r.pending_blank = true;
                    }
                }
                TagEnd::Item => {
                    r.flush_inline();
                    r.containers.pop();
                    // Loose items set pending_blank via paragraphs; tight items should not.
                }
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                    r.styles.pop();
                }
                TagEnd::Link => {
                    r.styles.pop();
                    if let Some(url) = r.link.pop() {
                        let text_now: String = r.inline.iter().map(|s| s.content.as_ref()).collect();
                        if !url.is_empty() && !text_now.ends_with(&url) {
                            r.push_text(&format!(" ({url})"), fg(MUTED));
                        }
                    }
                }
                TagEnd::Image => {
                    r.styles.pop();
                    let url = r.link.pop().unwrap_or_default();
                    r.push_text("]", fg(MD_IMAGE_TEXT));
                    if !url.is_empty() {
                        r.push_text(&format!(" ({url})"), fg(MUTED));
                    }
                }
                TagEnd::TableHead => {
                    if let Some(t) = r.table.as_mut() {
                        t.in_head = false;
                        t.head_rows = t.rows.len();
                    }
                }
                TagEnd::TableCell => {
                    let cell = std::mem::take(&mut r.cell);
                    if let Some(t) = r.table.as_mut() {
                        if let Some(row) = t.rows.last_mut() {
                            row.push(cell);
                        }
                    }
                }
                TagEnd::Table => {
                    if let Some(t) = r.table.take() {
                        r.render_table(t);
                    }
                }
                _ => {}
            },
            Event::Text(t) => {
                if let Some((_, code)) = r.code.as_mut() {
                    code.push_str(&t);
                } else {
                    let st = r.style();
                    // Inside tables, newlines aren't meaningful.
                    r.push_text(&t, st);
                }
            }
            Event::Code(t) => {
                let st = Style::new().fg(MD_CODE).bg(ELEMENT);
                r.push_text(&t, st);
            }
            Event::InlineMath(t) | Event::DisplayMath(t) => {
                let st = fg(MD_CODE);
                r.push_text(&t, st);
            }
            Event::Html(t) | Event::InlineHtml(t) => {
                if r.code.is_none() {
                    let st = fg(MUTED);
                    let t = t.trim_end_matches('\n').to_string();
                    r.push_text(&t, st);
                }
            }
            Event::SoftBreak => {
                // Chat output: a single newline is meant as a line break (lists of lines, poems, logs).
                let st = r.style();
                r.push_text(if r.table.is_some() { " " } else { "\n" }, st);
            }
            Event::HardBreak => {
                let st = r.style();
                r.push_text("\n", st);
            }
            Event::Rule => {
                r.flush_inline();
                r.start_block();
                let (first, _) = r.prefixes();
                let pw: usize = first.iter().map(|s| width(&s.content)).sum();
                let mut v = first;
                v.push(Span::styled("─".repeat(r.width.saturating_sub(pw)), fg(MD_HR)));
                r.out.push(Line::from(v));
                r.pending_blank = true;
            }
            Event::TaskListMarker(done) => {
                let (s, c) = if done { ("[✓] ", SUCCESS) } else { ("[ ] ", MUTED) };
                r.push_text(s, fg(c));
            }
            Event::FootnoteReference(n) => {
                r.push_text(&format!("[^{n}]"), fg(MUTED));
            }
        }
    }
    // Unclosed code block while streaming.
    if let Some((lang, code)) = r.code.take() {
        // Still streaming: keep highlighting small blocks (so colors don't pop in when the fence closes);
        // skip it for big ones, which would be re-highlighted on every delta.
        if code.len() > 4 * 1024 {
            r.highlight = false;
        }
        r.render_code(&lang, &code);
    }
    if let Some(t) = r.table.take() {
        r.render_table(t);
    }
    r.flush_inline();
    r.out
}

const MD_IMAGE_TEXT: Color = INFO;

// ───────────────────────── syntax highlighting ─────────────────────────

type Highlighted = Vec<Vec<(String, Color)>>;

struct Hl {
    ss: syntect::parsing::SyntaxSet,
    theme: syntect::highlighting::Theme,
}

/// Load the syntax set in the background so the first code block doesn't stall a frame.
pub fn prewarm() {
    std::thread::spawn(|| {
        let _ = hl();
    });
}

fn hl() -> &'static Hl {
    static H: OnceLock<Hl> = OnceLock::new();
    H.get_or_init(|| {
        use std::str::FromStr;
        use syntect::highlighting::{Color as SColor, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSettings};
        let c = |col: Color| match col {
            Color::Rgb(r, g, b) => SColor { r, g, b, a: 255 },
            _ => SColor { r: 238, g: 238, b: 238, a: 255 },
        };
        let item = |sel: &str, col: Color| ThemeItem {
            scope: ScopeSelectors::from_str(sel).unwrap(),
            style: StyleModifier { foreground: Some(c(col)), background: None, font_style: None },
        };
        let theme = Theme {
            name: Some("opencode".into()),
            author: None,
            settings: ThemeSettings { foreground: Some(c(TEXT)), ..Default::default() },
            scopes: vec![
                item("comment, punctuation.definition.comment", SYN_COMMENT),
                item("keyword, storage.modifier, storage.type.function, keyword.control", SYN_KEYWORD),
                item("keyword.operator", SYN_OPERATOR),
                item("entity.name.function, support.function, meta.function-call", SYN_FUNCTION),
                item("variable, variable.parameter", SYN_VARIABLE),
                item("string, string.quoted", SYN_STRING),
                item("constant.numeric, constant.language, constant.character", SYN_NUMBER),
                item("entity.name.type, storage.type, support.type, entity.name.class, entity.name.struct", SYN_TYPE),
                item("entity.name.tag", SYN_KEYWORD),
                item("entity.other.attribute-name", SYN_FUNCTION),
            ],
        };
        Hl { ss: syntect::parsing::SyntaxSet::load_defaults_newlines(), theme }
    })
}

fn highlight(lang: &str, code: &str) -> Option<Highlighted> {
    if lang.is_empty() || code.len() > 200_000 {
        return None;
    }
    static CACHE: OnceLock<Mutex<HashMap<(String, u64), Highlighted>>> = OnceLock::new();
    let key = (lang.to_string(), hash_str(code));
    let cache = CACHE.get_or_init(Default::default);
    if let Some(h) = cache.lock().unwrap().get(&key) {
        return Some(h.clone());
    }
    let h = hl();
    let syn = h.ss.find_syntax_by_token(lang)?;
    let mut hi = syntect::easy::HighlightLines::new(syn, &h.theme);
    let mut out = Vec::new();
    for line in syntect::util::LinesWithEndings::from(code) {
        let ranges = hi.highlight_line(line, &h.ss).ok()?;
        out.push(
            ranges
                .into_iter()
                .map(|(st, s)| {
                    let f = st.foreground;
                    (s.trim_end_matches('\n').to_string(), Color::Rgb(f.r, f.g, f.b))
                })
                .collect(),
        );
    }
    let mut c = cache.lock().unwrap();
    if c.len() > 512 {
        c.clear();
    }
    c.insert(key, out.clone());
    Some(out)
}

pub fn hash_str(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::super::text::{line_text, line_width};
    use super::*;

    fn r(s: &str, w: usize) -> Vec<String> {
        render(s, w, text(), false).iter().map(line_text).collect()
    }

    #[test]
    fn paragraphs_and_headings() {
        let out = r("# Title\n\nHello **world**.\n\nSecond para", 40);
        assert_eq!(out, vec!["Title", "", "Hello world.", "", "Second para"]);
    }

    #[test]
    fn lists_nested() {
        let out = r("- a\n- b\n  - c\n1. one\n2. two", 40);
        assert_eq!(out, vec!["• a", "• b", "  ◦ c", "", "1. one", "2. two"]);
    }

    #[test]
    fn list_wrap_indent() {
        let out = r("- aaaa bbbb cccc", 8);
        assert_eq!(out, vec!["• aaaa", "  bbbb", "  cccc"]);
    }

    #[test]
    fn cjk_wrap_width() {
        let src = "这是一个很长的中文段落，用于测试换行是否按照显示宽度正确计算。Mixed with English words too.";
        for w in 8..40 {
            for l in render(src, w, text(), false) {
                assert!(line_width(&l) <= w, "w={w}: {:?}", line_text(&l));
            }
        }
    }

    #[test]
    fn code_block() {
        let out = render("```rust\nfn main() {}\n```", 30, text(), true);
        let t: Vec<String> = out.iter().map(line_text).collect();
        assert!(t[0].contains("rust"));
        assert!(t[1].contains("fn main() {}"));
        for l in &out {
            assert_eq!(line_width(l), 30);
        }
    }

    #[test]
    fn table_box() {
        let out = r("| a | bb |\n|---|---|\n| 1 | 你好 |", 40);
        assert_eq!(out[0], "┌───┬──────┐");
        assert_eq!(out[1], "│ a │ bb   │");
        assert_eq!(out[2], "├───┼──────┤");
        assert_eq!(out[3], "│ 1 │ 你好 │");
        assert_eq!(out[4], "└───┴──────┘");
    }

    #[test]
    fn table_too_wide_raw() {
        let out = r("| aaaaaaaa | bbbbbbbbbb |\n|---|---|\n| 1 | 2 |", 10);
        assert!(out[0].starts_with("| aaaa"), "{out:?}");
    }

    #[test]
    fn table_narrow_wraps_cells() {
        let src = "| key | description |\n|---|---|\n| a | a fairly long description that must wrap |\n| b | 短 |";
        let out = render(src, 30, text(), false);
        for l in &out {
            assert!(line_width(l) <= 30, "{:?}", line_text(l));
        }
        let t: Vec<String> = out.iter().map(line_text).collect();
        assert!(t[0].starts_with('┌'), "{t:?}");
        assert!(t.last().unwrap().starts_with('└'));
        // Every row line has the same width (box stays aligned).
        let w0 = line_width(&out[0]);
        assert!(out.iter().all(|l| line_width(l) == w0), "{t:?}");
        assert!(t.iter().any(|l| l.contains("must wrap")), "{t:?}");
    }

    #[test]
    fn soft_breaks_kept() {
        assert_eq!(r("1\n2\n3", 40), vec!["1", "2", "3"]);
        assert_eq!(r("> a\n> b", 40), vec!["│ a", "│ b"]);
    }

    #[test]
    fn partial_markers_closed() {
        assert_eq!(close_partial("1. **If the image is"), "1. **If the image is**");
        assert_eq!(close_partial("use `foo"), "use `foo`");
        assert_eq!(close_partial("**done** and more"), "**done** and more");
        assert_eq!(close_partial("```rust\nlet **x"), "```rust\nlet **x");
        assert_eq!(close_partial("a **b**\n\nc ~~d"), "a **b**\n\nc ~~d~~");
        assert_eq!(close_partial("list **"), "list **");
        let out = r(&close_partial("Hello **wor"), 40);
        assert_eq!(out, vec!["Hello wor"]);
    }

    #[test]
    fn code_block_panel() {
        let out = render("```rust\nlet x = 1;\n```", 24, text(), false);
        let t: Vec<String> = out.iter().map(line_text).collect();
        assert_eq!(t[0].trim_end(), "▎ rust");
        assert_eq!(t[1].trim_end(), "▎ let x = 1;");
        assert_eq!(t[2].trim_end(), "▎");
        assert!(out.iter().all(|l| line_width(l) == 24));
        // Long lines wrap inside the panel and keep the bar.
        let out = render("```\n0123456789012345678901234567890\n```", 16, text(), false);
        let t: Vec<String> = out.iter().map(line_text).collect();
        assert!(t[1].starts_with("▎ 0123456789012"), "{t:?}");
        assert!(t[2].starts_with("▎ 345"), "{t:?}");
    }

    #[test]
    fn links_and_quote() {
        let out = r("see [docs](https://x.y) now\n\n> quoted", 60);
        assert_eq!(out[0], "see docs (https://x.y) now");
        assert_eq!(out[2], "│ quoted");
    }

    #[test]
    fn inline_code_and_rule() {
        let out = r("use `foo()`\n\n---", 10);
        assert_eq!(out[0], "use foo()");
        assert_eq!(out[2], "──────────");
    }
}
