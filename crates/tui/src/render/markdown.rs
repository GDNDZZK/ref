//! Markdown → styled, width-wrapped terminal lines (pulldown-cmark based).

use crate::render::highlight::Highlighter;
use crate::text::{self, wrap_segments};
use crate::theme;
use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

type Seg = (String, Style);

struct Prefix {
    first: Vec<Span<'static>>,
    rest: Vec<Span<'static>>,
    used: bool,
}

struct Table {
    aligns: Vec<Alignment>,
    rows: Vec<Vec<Vec<Seg>>>,
    header_rows: usize,
    cur_row: Vec<Vec<Seg>>,
    in_head: bool,
}

struct Renderer {
    width: usize,
    base: Style,
    out: Vec<Line<'static>>,
    /// Logical lines of the current inline block.
    inline: Vec<Vec<Seg>>,
    styles: Vec<Style>,
    prefixes: Vec<Prefix>,
    lists: Vec<Option<u64>>,
    links: Vec<String>,
    code: Option<(String, String)>,
    table: Option<Table>,
    cell: Option<Vec<Seg>>,
    pending_gap: bool,
    heading: Option<HeadingLevel>,
}

/// Render markdown text into lines no wider than `width` columns.
/// `base` is the body text style.
pub fn render(src: &str, width: usize, base: Style) -> Vec<Line<'static>> {
    let width = width.max(8);
    let src = text::sanitize(src);
    let mut r = Renderer {
        width,
        base,
        out: Vec::new(),
        inline: Vec::new(),
        styles: vec![base],
        prefixes: Vec::new(),
        lists: Vec::new(),
        links: Vec::new(),
        code: None,
        table: None,
        cell: None,
        pending_gap: false,
        heading: None,
    };
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    for ev in Parser::new_ext(&src, opts) {
        r.event(ev);
    }
    r.flush_inline();
    if let Some((lang, code)) = r.code.take() {
        r.emit_code(&lang, &code);
    }
    // Trim trailing blank lines.
    while r.out.last().is_some_and(|l| text::line_text(l).trim().is_empty()) {
        r.out.pop();
    }
    r.out
}

impl Renderer {
    fn style(&self) -> Style {
        *self.styles.last().unwrap_or(&self.base)
    }

    fn push_style(&mut self, f: impl FnOnce(Style) -> Style) {
        let s = f(self.style());
        self.styles.push(s);
    }

    fn pop_style(&mut self) {
        if self.styles.len() > 1 {
            self.styles.pop();
        }
    }

    fn push_text(&mut self, t: &str, style: Style) {
        if let Some(cell) = self.cell.as_mut() {
            cell.push((t.replace('\n', " "), style));
            return;
        }
        let mut first = true;
        for piece in t.split('\n') {
            if !first || self.inline.is_empty() {
                self.inline.push(Vec::new());
            }
            first = false;
            if !piece.is_empty() {
                self.inline.last_mut().unwrap().push((piece.to_string(), style));
            }
        }
    }

    fn line_break(&mut self) {
        if self.cell.is_some() {
            self.push_text(" ", self.style());
            return;
        }
        if self.inline.is_empty() {
            self.inline.push(Vec::new());
        }
        self.inline.push(Vec::new());
    }

    fn prefix_spans(&mut self) -> (Vec<Span<'static>>, usize) {
        let mut spans = Vec::new();
        for p in self.prefixes.iter_mut() {
            if p.used {
                spans.extend(p.rest.iter().cloned());
            } else {
                spans.extend(p.first.iter().cloned());
                p.used = true;
            }
        }
        let w = spans.iter().map(|s| s.width()).sum();
        (spans, w)
    }

    fn prefix_width(&self) -> usize {
        self.prefixes.iter().map(|p| p.rest.iter().map(|s| s.width()).sum::<usize>()).sum()
    }

    fn gap(&mut self) {
        if self.pending_gap && !self.out.is_empty() {
            // Blank line keeps only the "rest" bars (e.g. quote bars), never bullets.
            let spans: Vec<Span<'static>> = self
                .prefixes
                .iter()
                .flat_map(|p| p.rest.iter().cloned())
                .map(|s| {
                    let t = s.content.trim_end().to_string();
                    Span::styled(t, s.style)
                })
                .collect();
            self.out.push(Line::from(spans));
        }
        self.pending_gap = false;
    }

    fn emit_line(&mut self, spans: Vec<Span<'static>>) {
        let (mut pre, _) = self.prefix_spans();
        pre.extend(spans);
        self.out.push(Line::from(pre));
    }

    fn flush_inline(&mut self) {
        if self.inline.is_empty() {
            return;
        }
        let lines = std::mem::take(&mut self.inline);
        // Drop trailing empty logical lines.
        let mut lines: Vec<Vec<Seg>> = lines;
        while lines.last().is_some_and(|l| l.iter().all(|s| s.0.trim().is_empty())) {
            lines.pop();
        }
        if lines.is_empty() {
            return;
        }
        self.gap();
        let avail = self.width.saturating_sub(self.prefix_width()).max(4);
        for logical in lines {
            for w in wrap_segments(&logical, avail) {
                self.emit_line(w.spans);
            }
        }
    }

    fn emit_code(&mut self, lang: &str, code: &str) {
        self.gap();
        let bg = theme::PANEL;
        let avail = self.width.saturating_sub(self.prefix_width()).max(6);
        let inner = avail.saturating_sub(2).max(1);
        let base = Style::new().fg(theme::TEXT).bg(bg);
        let pad = |line: Vec<Span<'static>>| -> Vec<Span<'static>> {
            let w: usize = line.iter().map(|s| s.width()).sum();
            let mut v = vec![Span::styled(" ", base)];
            v.extend(line);
            if w + 1 < avail {
                v.push(Span::styled(" ".repeat(avail - w - 1), base));
            }
            v
        };
        let label = lang.split([',', ' ']).next().unwrap_or("").trim();
        if !label.is_empty() {
            let l = text::truncate(label, inner);
            self.emit_line(pad(vec![Span::styled(l, Style::new().fg(theme::MUTED).bg(bg))]));
        }
        let mut hl = Highlighter::new(label);
        let code = code.strip_suffix('\n').unwrap_or(code);
        for raw in code.split('\n') {
            let segs = hl.line(raw, base);
            // Hard-wrap highlighted segments by width (code shouldn't word-wrap).
            let mut cur: Vec<Span<'static>> = Vec::new();
            let mut w = 0;
            let mut rows: Vec<Vec<Span<'static>>> = Vec::new();
            for (s, st) in segs {
                for piece in split_keep(&s, inner, &mut w) {
                    match piece {
                        Piece::Text(t) => {
                            if !t.is_empty() {
                                cur.push(Span::styled(t, st));
                            }
                        }
                        Piece::Break => rows.push(std::mem::take(&mut cur)),
                    }
                }
            }
            rows.push(cur);
            for row in rows {
                self.emit_line(pad(row));
            }
        }
        self.pending_gap = true;
    }

    fn emit_table(&mut self, t: Table) {
        self.gap();
        let ncols = t.rows.iter().map(|r| r.len()).max().unwrap_or(0);
        if ncols == 0 {
            return;
        }
        let cell_w = |c: &Vec<Seg>| c.iter().map(|s| text::width(&s.0)).sum::<usize>();
        let mut widths = vec![1usize; ncols];
        for row in &t.rows {
            for (i, c) in row.iter().enumerate() {
                widths[i] = widths[i].max(cell_w(c));
            }
        }
        let avail = self.width.saturating_sub(self.prefix_width()).max(8);
        let sep = 3; // " │ "
        let total = |w: &Vec<usize>| w.iter().sum::<usize>() + sep * (ncols.saturating_sub(1));
        // Shrink widest columns until it fits.
        while total(&widths) > avail {
            let (i, m) = widths.iter().enumerate().max_by_key(|(_, w)| **w).map(|(i, w)| (i, *w)).unwrap();
            if m <= 3 {
                break;
            }
            widths[i] = m - 1;
        }
        let border = theme::fg(theme::BORDER);
        for (ri, row) in t.rows.iter().enumerate() {
            let is_head = ri < t.header_rows;
            let wrapped: Vec<Vec<text::Wrapped>> = (0..ncols)
                .map(|ci| {
                    let segs: Vec<Seg> = row
                        .get(ci)
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|(s, st)| (s, if is_head { st.add_modifier(Modifier::BOLD) } else { st }))
                        .collect();
                    wrap_segments(&segs, widths[ci])
                })
                .collect();
            let height = wrapped.iter().map(|w| w.len()).max().unwrap_or(1);
            for li in 0..height {
                let mut spans: Vec<Span<'static>> = Vec::new();
                for ci in 0..ncols {
                    if ci > 0 {
                        spans.push(Span::styled(" │ ", border));
                    }
                    let (cell_spans, w) = match wrapped[ci].get(li) {
                        Some(wl) => (wl.spans.clone(), wl.width),
                        None => (Vec::new(), 0),
                    };
                    let free = widths[ci].saturating_sub(w);
                    let (l, r) = match t.aligns.get(ci) {
                        Some(Alignment::Right) => (free, 0),
                        Some(Alignment::Center) => (free / 2, free - free / 2),
                        _ => (0, free),
                    };
                    if l > 0 {
                        spans.push(Span::raw(" ".repeat(l)));
                    }
                    spans.extend(cell_spans);
                    if r > 0 && ci + 1 < ncols {
                        spans.push(Span::raw(" ".repeat(r)));
                    }
                }
                self.emit_line(spans);
            }
            if is_head && ri + 1 == t.header_rows {
                let mut s = String::new();
                for (ci, w) in widths.iter().enumerate() {
                    if ci > 0 {
                        s.push_str("─┼─");
                    }
                    s.push_str(&"─".repeat(*w));
                }
                self.emit_line(vec![Span::styled(s, border)]);
            }
        }
        self.pending_gap = true;
    }

    fn event(&mut self, ev: Event<'_>) {
        if let Some((_, buf)) = self.code.as_mut() {
            match ev {
                Event::Text(t) => {
                    buf.push_str(&t);
                    return;
                }
                Event::End(TagEnd::CodeBlock) => {
                    let (lang, code) = self.code.take().unwrap();
                    self.emit_code(&lang, &code);
                    return;
                }
                _ => return,
            }
        }
        match ev {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => {
                let st = self.style();
                self.push_text(&t, st);
            }
            Event::Code(t) => {
                let st = self.style().fg(theme::MD_CODE);
                self.push_text(&t, st);
            }
            Event::InlineMath(t) | Event::DisplayMath(t) => {
                let st = self.style().fg(theme::MD_CODE);
                self.push_text(&t, st);
            }
            Event::Html(t) | Event::InlineHtml(t) => {
                let st = self.style().fg(theme::MUTED);
                self.push_text(&t, st);
            }
            Event::FootnoteReference(t) => {
                let st = self.style().fg(theme::MUTED);
                self.push_text(&format!("[{t}]"), st);
            }
            Event::SoftBreak | Event::HardBreak => self.line_break(),
            Event::Rule => {
                self.flush_inline();
                self.gap();
                let w = self.width.saturating_sub(self.prefix_width());
                self.emit_line(vec![Span::styled("─".repeat(w), theme::fg(theme::BORDER))]);
                self.pending_gap = true;
            }
            Event::TaskListMarker(done) => {
                if let Some(p) = self.prefixes.last_mut() {
                    if !p.used {
                        let w: usize = p.first.iter().map(|s| s.width()).sum();
                        let (mark, color) = if done { ("[✓] ", theme::SUCCESS) } else { ("[ ] ", theme::MUTED) };
                        let indent = w.saturating_sub(2);
                        p.first = vec![Span::raw(" ".repeat(indent)), Span::styled(mark, theme::fg(color))];
                        p.rest = vec![Span::raw(" ".repeat(indent + 4))];
                    }
                }
            }
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {
                self.flush_inline();
            }
            Tag::Heading { level, .. } => {
                self.flush_inline();
                self.heading = Some(level);
                let color = theme::MD_HEADING;
                self.push_style(|s| {
                    let s = s.fg(color).add_modifier(Modifier::BOLD);
                    if level == HeadingLevel::H1 {
                        s.add_modifier(Modifier::UNDERLINED)
                    } else {
                        s
                    }
                });
            }
            Tag::BlockQuote(_) => {
                self.flush_inline();
                self.gap();
                let bar = Span::styled("│ ", theme::fg(theme::MD_QUOTE));
                self.prefixes.push(Prefix { first: vec![bar.clone()], rest: vec![bar], used: false });
                self.push_style(|s| s.fg(theme::MD_QUOTE).add_modifier(Modifier::ITALIC));
            }
            Tag::CodeBlock(kind) => {
                self.flush_inline();
                let lang = match kind {
                    CodeBlockKind::Fenced(l) => l.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((lang, String::new()));
            }
            Tag::List(start) => {
                self.flush_inline();
                if self.lists.is_empty() {
                    self.gap();
                }
                self.lists.push(start);
            }
            Tag::Item => {
                self.flush_inline();
                let depth = self.lists.len().saturating_sub(1);
                let marker = match self.lists.last_mut() {
                    Some(Some(n)) => {
                        let m = format!("{n}. ");
                        *n += 1;
                        Span::styled(m, theme::fg(theme::MD_ENUM))
                    }
                    _ => {
                        let b = ["• ", "◦ ", "▪ "][depth % 3];
                        Span::styled(b, theme::fg(theme::MD_LIST))
                    }
                };
                let w = marker.width();
                self.prefixes.push(Prefix { first: vec![marker], rest: vec![Span::raw(" ".repeat(w))], used: false });
            }
            Tag::Emphasis => self.push_style(|s| s.fg(theme::MD_EMPH).add_modifier(Modifier::ITALIC)),
            Tag::Strong => self.push_style(|s| s.fg(theme::MD_STRONG).add_modifier(Modifier::BOLD)),
            Tag::Strikethrough => self.push_style(|s| s.add_modifier(Modifier::CROSSED_OUT)),
            Tag::Link { dest_url, .. } => {
                self.links.push(dest_url.to_string());
                self.push_style(|s| s.fg(theme::MD_LINK_TEXT).add_modifier(Modifier::UNDERLINED));
            }
            Tag::Image { dest_url, .. } => {
                self.links.push(dest_url.to_string());
                let st = self.style().fg(theme::PRIMARY);
                self.push_text("🖼 ", st);
                self.push_style(|s| s.fg(theme::MD_LINK_TEXT));
            }
            Tag::Table(aligns) => {
                self.flush_inline();
                self.table =
                    Some(Table { aligns, rows: Vec::new(), header_rows: 0, cur_row: Vec::new(), in_head: false });
            }
            Tag::TableHead => {
                if let Some(t) = self.table.as_mut() {
                    t.in_head = true;
                    t.cur_row.clear();
                }
            }
            Tag::TableRow => {
                if let Some(t) = self.table.as_mut() {
                    t.cur_row.clear();
                }
            }
            Tag::TableCell => self.cell = Some(Vec::new()),
            Tag::FootnoteDefinition(name) => {
                self.flush_inline();
                let st = self.style().fg(theme::MUTED);
                self.push_text(&format!("[{name}]: "), st);
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.flush_inline();
                self.pending_gap = true;
            }
            TagEnd::Heading(_) => {
                self.pop_style();
                self.flush_inline();
                self.heading = None;
                self.pending_gap = true;
            }
            TagEnd::BlockQuote(_) => {
                self.flush_inline();
                self.prefixes.pop();
                self.pop_style();
                self.pending_gap = true;
            }
            TagEnd::List(_) => {
                self.flush_inline();
                self.lists.pop();
                if self.lists.is_empty() {
                    self.pending_gap = true;
                }
            }
            TagEnd::Item => {
                self.flush_inline();
                // An item with no content still shows its bullet.
                if self.prefixes.last().is_some_and(|p| !p.used) {
                    self.gap();
                    self.emit_line(Vec::new());
                }
                self.prefixes.pop();
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => self.pop_style(),
            TagEnd::Link | TagEnd::Image => {
                self.pop_style();
                if let Some(url) = self.links.pop() {
                    let shown: String = match &self.cell {
                        Some(c) => c.iter().map(|s| s.0.as_str()).collect(),
                        None => self.inline.last().map(|l| l.iter().map(|s| s.0.as_str()).collect()).unwrap_or_default(),
                    };
                    if !url.is_empty() && !shown.ends_with(url.as_str()) && !url.starts_with('#') {
                        let st = self.style().fg(theme::MUTED);
                        self.push_text(&format!(" ({url})"), st);
                    }
                }
            }
            TagEnd::TableCell => {
                if let (Some(cell), Some(t)) = (self.cell.take(), self.table.as_mut()) {
                    t.cur_row.push(cell);
                }
            }
            TagEnd::TableHead => {
                if let Some(t) = self.table.as_mut() {
                    let row = std::mem::take(&mut t.cur_row);
                    t.rows.push(row);
                    t.header_rows = t.rows.len();
                    t.in_head = false;
                }
            }
            TagEnd::TableRow => {
                if let Some(t) = self.table.as_mut() {
                    let row = std::mem::take(&mut t.cur_row);
                    t.rows.push(row);
                }
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    self.emit_table(t);
                }
            }
            TagEnd::FootnoteDefinition => {
                self.flush_inline();
                self.pending_gap = true;
            }
            _ => {}
        }
    }
}

enum Piece {
    Text(String),
    Break,
}

/// Split `s` into pieces so that the running column `w` never exceeds `max`.
fn split_keep(s: &str, max: usize, w: &mut usize) -> Vec<Piece> {
    use unicode_segmentation::UnicodeSegmentation;
    let mut out = Vec::new();
    let mut cur = String::new();
    for g in s.graphemes(true) {
        let gw = text::gwidth(g);
        if *w + gw > max && *w > 0 {
            out.push(Piece::Text(std::mem::take(&mut cur)));
            out.push(Piece::Break);
            *w = 0;
        }
        cur.push_str(g);
        *w += gw;
    }
    out.push(Piece::Text(cur));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(lines: &[Line<'_>]) -> Vec<String> {
        lines.iter().map(text::line_text).collect()
    }

    #[test]
    fn paragraph_wraps_cjk_by_width() {
        let lines = render("这是一个很长的中文段落，用来测试换行是否按照显示宽度进行。", 20, Style::default());
        assert!(lines.len() > 1);
        for l in &lines {
            assert!(l.width() <= 20, "{:?}", plain(&lines));
        }
        let joined: String = plain(&lines).concat();
        assert_eq!(joined, "这是一个很长的中文段落，用来测试换行是否按照显示宽度进行。");
    }

    #[test]
    fn mixed_cjk_list_wraps_with_indent() {
        let lines = render("- 第一项 item with English words 以及中文混排内容\n- second", 18, Style::default());
        let p = plain(&lines);
        assert!(p[0].starts_with("• "));
        assert!(p[1].starts_with("  "), "{p:?}");
        for l in &lines {
            assert!(l.width() <= 18, "{p:?}");
        }
        assert!(p.last().unwrap().starts_with("• second"));
    }

    #[test]
    fn heading_and_inline_styles() {
        let lines = render("# Title\n\nsome **bold** and `code`", 40, Style::default());
        let p = plain(&lines);
        assert_eq!(p[0], "Title");
        assert_eq!(p[1], "");
        assert_eq!(p[2], "some bold and code");
        let bold = lines[2].spans.iter().find(|s| s.content == "bold").unwrap();
        assert!(bold.style.add_modifier.contains(Modifier::BOLD));
        let code = lines[2].spans.iter().find(|s| s.content == "code").unwrap();
        assert_eq!(code.style.fg, Some(theme::MD_CODE));
    }

    #[test]
    fn code_block_has_panel_bg_and_full_width() {
        let lines = render("```rust\nfn main() {}\n```", 30, Style::default());
        assert_eq!(lines.len(), 2); // label + code
        for l in &lines {
            assert_eq!(l.width(), 30);
            assert!(l.spans.iter().all(|s| s.style.bg == Some(theme::PANEL)));
        }
        assert!(text::line_text(&lines[1]).contains("fn main() {}"));
    }

    #[test]
    fn code_block_hard_wraps_cjk() {
        let lines = render("```\n中文中文中文中文中文\n```", 12, Style::default());
        for l in &lines {
            assert_eq!(l.width(), 12, "{:?}", plain(&lines));
        }
    }

    #[test]
    fn unclosed_fence_while_streaming() {
        let lines = render("text\n\n```py\nprint(1)", 30, Style::default());
        assert!(plain(&lines).iter().any(|l| l.contains("print(1)")));
    }

    #[test]
    fn table_renders() {
        let lines = render("| a | 中文 |\n|---|---:|\n| 1 | 2 |", 40, Style::default());
        let p = plain(&lines);
        assert_eq!(p.len(), 3);
        assert!(p[0].contains("│"));
        assert!(p[1].contains("┼"));
        assert_eq!(text::width(&p[0]), text::width(&p[2]));
    }

    #[test]
    fn quote_prefix() {
        let lines = render("> quoted text here", 40, Style::default());
        assert!(plain(&lines)[0].starts_with("│ quoted"));
    }

    #[test]
    fn soft_breaks_kept() {
        let lines = render("line one\nline two", 40, Style::default());
        assert_eq!(plain(&lines), vec!["line one", "line two"]);
    }
}
