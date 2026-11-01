//! Markdown → styled, wrapped rows (pulldown-cmark + syntect for code blocks).

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::text::{self, width};
use super::{Row, highlight, theme};

/// Narrowest width anything is rendered at.
const MIN_WIDTH: usize = 4;
/// Prefix of a quoted line and its width in columns.
const QUOTE_PREFIX: &str = "│ ";
const QUOTE_PREFIX_WIDTH: usize = 2;
/// Table columns are not shrunk below this width.
const MIN_COLUMN_WIDTH: usize = 2;
/// List bullets by nesting depth; deeper levels reuse the last one.
const BULLETS: [&str; 3] = ["• ", "◦ ", "▪ "];

#[derive(Clone)]
enum Container {
    Quote,
    Item { marker: String, used: bool },
}

struct Table {
    aligns: Vec<pulldown_cmark::Alignment>,
    rows: Vec<Vec<Vec<Span<'static>>>>,
    head_rows: usize,
    cur_row: Vec<Vec<Span<'static>>>,
}

struct MarkdownRenderer {
    width: usize,
    base: Style,
    rows: Vec<Row>,
    /// Logical lines of the current inline run.
    inline: Vec<Vec<Span<'static>>>,
    styles: Vec<Style>,
    containers: Vec<Container>,
    lists: Vec<Option<u64>>,
    suppress_blank: bool,
    code: Option<(String, String)>,
    heading: Option<HeadingLevel>,
    link: Vec<String>,
    table: Option<Table>,
}

/// Renders markdown into rows of at most `width` columns using `base` as the text style.
pub fn render(src: &str, width: usize, base: Style) -> Vec<Row> {
    let width = width.max(MIN_WIDTH);
    let src = text::sanitize(src);
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    let mut r = MarkdownRenderer {
        width,
        base,
        rows: Vec::new(),
        inline: Vec::new(),
        styles: vec![base],
        containers: Vec::new(),
        lists: Vec::new(),
        suppress_blank: false,
        code: None,
        heading: None,
        link: Vec::new(),
        table: None,
    };
    for ev in Parser::new_ext(&src, opts) {
        r.event(ev);
    }
    r.flush();
    if let Some((lang, code)) = r.code.take() {
        r.code_block(&lang, &code);
    }
    let is_trailing_blank = |rows: &[Row]| {
        rows.last()
            .is_some_and(|x| text::line_width(&x.line) == 0 && x.hits.is_empty())
    };
    while is_trailing_blank(&r.rows) && r.rows.len() > 1 {
        r.rows.pop();
    }
    r.rows
}

impl MarkdownRenderer {
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
        if let Some(tb) = self.table.as_mut() {
            if tb.cur_row.is_empty() {
                tb.cur_row.push(Vec::new());
            }
            if let Some(cell) = tb.cur_row.last_mut() {
                cell.push(Span::styled(t.replace('\n', " "), style));
            }
            return;
        }
        if self.inline.is_empty() {
            self.inline.push(Vec::new());
        }
        let mut first = true;
        for piece in t.split('\n') {
            if !first {
                self.inline.push(Vec::new());
            }
            first = false;
            if !piece.is_empty()
                && let Some(l) = self.inline.last_mut()
            {
                l.push(Span::styled(piece.to_string(), style));
            }
        }
    }

    /// Prefix spans for the next row; `first` = first row of a block (list marker shown).
    fn prefix(&mut self, first: bool) -> Vec<Span<'static>> {
        let mut out = Vec::new();
        let n = self.containers.len();
        for i in 0..n {
            match &mut self.containers[i] {
                Container::Quote => {
                    out.push(Span::styled(QUOTE_PREFIX, theme::fg(theme::MD_QUOTE)))
                }
                Container::Item { marker, used } => {
                    let w = width(marker);
                    if first && !*used {
                        *used = true;
                        let st = if marker.trim_end().ends_with('.') {
                            theme::fg(theme::MD_ENUM)
                        } else {
                            theme::fg(theme::MD_LIST)
                        };
                        out.push(Span::styled(marker.clone(), st));
                    } else {
                        out.push(Span::raw(" ".repeat(w)));
                    }
                }
            }
        }
        out
    }

    fn prefix_width(&self) -> usize {
        self.containers
            .iter()
            .map(|c| match c {
                Container::Quote => QUOTE_PREFIX_WIDTH,
                Container::Item { marker, .. } => width(marker),
            })
            .sum()
    }

    fn quote_only_prefix(&self) -> Vec<Span<'static>> {
        self.containers
            .iter()
            .map(|c| match c {
                Container::Quote => Span::styled(QUOTE_PREFIX, theme::fg(theme::MD_QUOTE)),
                Container::Item { marker, .. } => Span::raw(" ".repeat(width(marker))),
            })
            .collect()
    }

    fn block_start(&mut self) {
        self.flush();
        if self.suppress_blank {
            self.suppress_blank = false;
            return;
        }
        if !self.rows.is_empty() {
            let p = self.quote_only_prefix();
            let mut row = Row::new(Line::from(p));
            if self
                .containers
                .iter()
                .any(|c| matches!(c, Container::Quote))
            {
                row = row.skip(self.prefix_width() as u16);
            }
            self.rows.push(row);
        }
    }

    fn emit_wrapped(&mut self, lines: Vec<Vec<Span<'static>>>, hard: bool) {
        let avail = self.width.saturating_sub(self.prefix_width()).max(1);
        for logical in lines {
            let wrapped: Vec<(Line<'static>, bool)> = if hard {
                text::wrap_hard(&logical, avail)
                    .into_iter()
                    .map(|l| (l, false))
                    .collect()
            } else {
                text::wrap_spans_flags(&logical, avail)
            };
            for (i, (l, sp)) in wrapped.into_iter().enumerate() {
                let first = i == 0;
                let p = self.prefix(true);
                let pw: usize = text::spans_width(&p);
                let mut spans = p;
                spans.extend(l.spans);
                let mut row = Row::new(Line::from(spans)).soft_sp(!first, sp);
                if !first {
                    row = row.skip(pw as u16);
                }
                self.rows.push(row);
            }
        }
    }

    fn flush(&mut self) {
        if self.inline.is_empty() {
            return;
        }
        let mut lines = std::mem::take(&mut self.inline);
        // Drop a trailing empty logical line produced by a final newline.
        while lines.len() > 1 && lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        self.emit_wrapped(lines, false);
    }

    fn event(&mut self, ev: Event<'_>) {
        if let Some((_, code)) = self.code.as_mut() {
            match ev {
                Event::Text(t) => {
                    code.push_str(&t);
                    return;
                }
                Event::End(TagEnd::CodeBlock) => {
                    if let Some((lang, code)) = self.code.take() {
                        self.code_block(&lang, &code);
                    }
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
                let st = Style::new().fg(theme::MD_CODE);
                self.push_text(&t, st);
            }
            Event::InlineMath(t) | Event::DisplayMath(t) => {
                let st = Style::new().fg(theme::MD_CODE);
                self.push_text(&t, st);
            }
            Event::Html(t) | Event::InlineHtml(t) => {
                let st = theme::muted();
                self.push_text(t.trim_end_matches('\n'), st);
                let ends_block = t.ends_with('\n');
                if ends_block {
                    self.flush();
                }
            }
            Event::SoftBreak => {
                let st = self.style();
                self.push_text(" ", st);
            }
            Event::HardBreak => {
                let st = self.style();
                self.push_text("\n", st);
            }
            Event::Rule => {
                self.block_start();
                let w = self.width.saturating_sub(self.prefix_width());
                let mut spans = self.prefix(true);
                spans.push(Span::styled("─".repeat(w), theme::fg(theme::BORDER)));
                self.rows.push(Row::new(Line::from(spans)));
            }
            Event::TaskListMarker(done) => {
                let (s, st) = if done {
                    ("[✓] ", theme::fg(theme::SUCCESS))
                } else {
                    ("[ ] ", theme::muted())
                };
                self.push_text(s, st);
            }
            Event::FootnoteReference(n) => {
                let st = theme::muted();
                self.push_text(&format!("[^{n}]"), st);
            }
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => self.block_start(),
            Tag::Heading { level, .. } => {
                self.block_start();
                self.heading = Some(level);
                let st = match level {
                    HeadingLevel::H1 => {
                        theme::bold(theme::MD_HEADING).add_modifier(Modifier::UNDERLINED)
                    }
                    HeadingLevel::H2 | HeadingLevel::H3 => theme::bold(theme::MD_HEADING),
                    _ => theme::bold(theme::MD_HEADING)
                        .remove_modifier(Modifier::BOLD)
                        .add_modifier(Modifier::ITALIC),
                };
                self.styles.push(st);
            }
            Tag::BlockQuote(_) => {
                self.block_start();
                self.containers.push(Container::Quote);
                self.suppress_blank = true;
                self.push_style(|s| s.fg(theme::MD_QUOTE).add_modifier(Modifier::ITALIC));
            }
            Tag::CodeBlock(kind) => {
                self.block_start();
                let lang = match kind {
                    CodeBlockKind::Fenced(l) => l.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((lang, String::new()));
            }
            Tag::HtmlBlock => self.block_start(),
            Tag::List(start) => {
                if self.lists.is_empty() {
                    self.block_start();
                } else {
                    self.flush();
                }
                self.lists.push(start);
            }
            Tag::Item => {
                self.flush();
                let depth = self.lists.len();
                let marker = match self.lists.last_mut() {
                    Some(Some(n)) => {
                        let m = format!("{n}. ");
                        *n += 1;
                        m
                    }
                    _ => BULLETS[depth.saturating_sub(1).min(BULLETS.len() - 1)].to_string(),
                };
                self.containers.push(Container::Item {
                    marker,
                    used: false,
                });
                self.suppress_blank = true;
            }
            Tag::Emphasis => {
                self.push_style(|s| s.fg(theme::MD_EMPH).add_modifier(Modifier::ITALIC))
            }
            Tag::Strong => self.push_style(|s| s.fg(theme::MD_STRONG).add_modifier(Modifier::BOLD)),
            Tag::Strikethrough => self.push_style(|s| s.add_modifier(Modifier::CROSSED_OUT)),
            Tag::Superscript | Tag::Subscript => self.push_style(|s| s),
            Tag::Link { dest_url, .. } => {
                self.link.push(dest_url.to_string());
                self.push_style(|s| s.fg(theme::MD_LINK_TEXT).add_modifier(Modifier::UNDERLINED));
            }
            Tag::Image { dest_url, .. } => {
                self.link.push(dest_url.to_string());
                let st = theme::muted();
                self.push_text("[image: ", st);
                self.push_style(|_| theme::muted());
            }
            Tag::Table(aligns) => {
                self.block_start();
                self.table = Some(Table {
                    aligns,
                    rows: Vec::new(),
                    head_rows: 0,
                    cur_row: Vec::new(),
                });
            }
            Tag::TableHead | Tag::TableRow => {
                if let Some(t) = self.table.as_mut() {
                    t.cur_row.clear();
                }
            }
            Tag::TableCell => {
                if let Some(t) = self.table.as_mut() {
                    t.cur_row.push(Vec::new());
                }
            }
            Tag::FootnoteDefinition(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition => self.block_start(),
            Tag::MetadataBlock(_) => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.flush(),
            TagEnd::Heading(_) => {
                self.flush();
                self.heading = None;
                self.pop_style();
            }
            TagEnd::BlockQuote(_) => {
                self.flush();
                self.pop_style();
                if let Some(i) = self
                    .containers
                    .iter()
                    .rposition(|c| matches!(c, Container::Quote))
                {
                    self.containers.truncate(i);
                }
                self.suppress_blank = false;
            }
            TagEnd::CodeBlock => {}
            TagEnd::HtmlBlock => self.flush(),
            TagEnd::List(_) => {
                self.flush();
                self.lists.pop();
                self.suppress_blank = false;
            }
            TagEnd::Item => {
                self.flush();
                if let Some(i) = self
                    .containers
                    .iter()
                    .rposition(|c| matches!(c, Container::Item { .. }))
                {
                    // Item with no content still shows its marker.
                    if let Container::Item { used: false, .. } = &self.containers[i] {
                        let p = self.prefix(true);
                        self.rows.push(Row::new(Line::from(p)));
                    }
                    self.containers.truncate(i);
                }
                self.suppress_blank = false;
            }
            TagEnd::Emphasis
            | TagEnd::Strong
            | TagEnd::Strikethrough
            | TagEnd::Superscript
            | TagEnd::Subscript => self.pop_style(),
            TagEnd::Link => {
                self.pop_style();
                let url = self.link.pop().expect("a link start pushed its url");
                let shown: String = self
                    .inline
                    .last()
                    .map(|l| l.iter().map(|s| s.content.as_ref()).collect())
                    .unwrap_or_default();
                if !url.is_empty() && !shown.ends_with(&url) && !url.starts_with('#') {
                    let st = theme::fg(theme::MD_LINK).add_modifier(Modifier::DIM);
                    self.push_text(&format!(" ({url})"), st);
                }
            }
            TagEnd::Image => {
                self.pop_style();
                self.link.pop();
                let st = theme::muted();
                self.push_text("]", st);
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    self.render_table(t);
                }
            }
            TagEnd::TableHead => {
                if let Some(t) = self.table.as_mut() {
                    let r = std::mem::take(&mut t.cur_row);
                    t.rows.push(r);
                    t.head_rows = t.rows.len();
                }
            }
            TagEnd::TableRow => {
                if let Some(t) = self.table.as_mut() {
                    let r = std::mem::take(&mut t.cur_row);
                    t.rows.push(r);
                }
            }
            TagEnd::TableCell => {}
            TagEnd::FootnoteDefinition
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition => self.flush(),
            TagEnd::MetadataBlock(_) => {}
        }
    }

    fn code_block(&mut self, lang: &str, code: &str) {
        let code = code.strip_suffix('\n').unwrap_or(code);
        let lines: Vec<&str> = code.split('\n').collect();
        let hl = highlight::highlight(lang, &lines);
        let pw = self.prefix_width();
        let inner = self.width.saturating_sub(pw + 2).max(1);
        let bg = theme::ELEMENT;
        let base = Style::new().fg(theme::TEXT).bg(bg);
        let lang_label = lang.split_whitespace().next().unwrap_or("").to_string();
        for (i, line) in lines.iter().enumerate() {
            let spans: Vec<Span<'static>> = match hl.as_ref().and_then(|h| h.get(i)) {
                Some(segs) => segs
                    .iter()
                    .map(|(st, s)| Span::styled(s.clone(), st.bg(bg)))
                    .collect(),
                None => vec![Span::styled(line.to_string(), base)],
            };
            let wrapped = text::wrap_hard(&spans, inner);
            for (j, l) in wrapped.into_iter().enumerate() {
                let mut out = self.prefix(true);
                out.push(Span::styled("  ", base));
                let lw = text::line_width(&l);
                out.extend(l.spans);
                let mut fill = inner.saturating_sub(lw);
                // Language label on the first row, right aligned, if there's room.
                let mut label = None;
                let is_first_row = i == 0 && j == 0;
                let label_fits = fill > width(&lang_label) + 2;
                if is_first_row && !lang_label.is_empty() && label_fits {
                    fill -= width(&lang_label);
                    label = Some(Span::styled(
                        lang_label.clone(),
                        Style::new().fg(theme::DIM).bg(bg),
                    ));
                }
                out.push(Span::styled(" ".repeat(fill), base));
                let mut row = Row::new(Line::from(out)).soft(j > 0).skip((pw + 2) as u16);
                if let Some(lb) = label {
                    row = row.end((pw + 2 + lw + fill) as u16);
                    row.line.spans.push(lb);
                }
                self.rows.push(row);
            }
        }
    }

    fn render_table(&mut self, t: Table) {
        let ncols = t.rows.iter().map(|r| r.len()).max().unwrap_or(0);
        if ncols == 0 {
            return;
        }
        let mut widths = vec![1usize; ncols];
        for r in &t.rows {
            for (i, c) in r.iter().enumerate() {
                widths[i] = widths[i].max(text::spans_width(c).max(1));
            }
        }
        let pw = self.prefix_width();
        let chrome = grid_chrome_width(ncols);
        let avail = self.width.saturating_sub(pw + chrome).max(ncols);
        let widths = fit_column_widths(widths, avail);
        let too_narrow_for_grid =
            widths.iter().sum::<usize>() > avail || self.width < pw + chrome + ncols;
        if too_narrow_for_grid {
            self.render_compact_table(&t);
        } else {
            self.render_grid_table(&t, &widths);
        }
    }

    /// Too narrow for a grid: one line per row, cells separated by " │ ".
    fn render_compact_table(&mut self, t: &Table) {
        for r in &t.rows {
            let mut spans: Vec<Span<'static>> = Vec::new();
            for (i, c) in r.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::styled(" │ ", theme::fg(theme::BORDER)));
                }
                spans.extend(c.iter().cloned());
            }
            self.emit_wrapped(vec![spans], false);
        }
    }

    fn render_grid_table(&mut self, t: &Table, widths: &[usize]) {
        let ncols = widths.len();
        let bst = theme::fg(theme::BORDER);
        let border = |l: &str, m: &str, r: &str| -> Vec<Span<'static>> {
            let mut s = String::from(l);
            for (i, w) in widths.iter().enumerate() {
                s.push_str(&"─".repeat(w + 2));
                s.push_str(if i + 1 == widths.len() { r } else { m });
            }
            vec![Span::styled(s, bst)]
        };
        let mut p = self.prefix(true);
        p.extend(border("┌", "┬", "┐"));
        self.rows.push(Row::new(Line::from(p)).decor());
        for (ri, r) in t.rows.iter().enumerate() {
            let head = ri < t.head_rows;
            let cells: Vec<Vec<Line<'static>>> = (0..ncols)
                .map(|i| {
                    let mut spans = r.get(i).cloned().unwrap_or_default();
                    if head {
                        for s in &mut spans {
                            s.style = s.style.add_modifier(Modifier::BOLD);
                        }
                    }
                    text::wrap_spans(&spans, widths[i])
                })
                .collect();
            let h = cells
                .iter()
                .map(|c| c.len())
                .max()
                .expect("a table has at least one column");
            for li in 0..h {
                let mut spans = self.prefix(false);
                for (ci, c) in cells.iter().enumerate() {
                    spans.push(Span::styled(if ci == 0 { "│ " } else { " │ " }, bst));
                    let line = c.get(li).cloned().unwrap_or_default();
                    let pad = widths[ci].saturating_sub(text::line_width(&line));
                    let (lp, rp) = align_padding(t.aligns.get(ci), pad);
                    spans.push(Span::raw(" ".repeat(lp)));
                    spans.extend(line.spans);
                    spans.push(Span::raw(" ".repeat(rp)));
                }
                spans.push(Span::styled(" │", bst));
                self.rows
                    .push(Row::new(Line::from(spans)).soft_sp(li > 0, true));
            }
            if head && ri + 1 == t.head_rows {
                let mut p = self.prefix(false);
                p.extend(border("├", "┼", "┤"));
                self.rows.push(Row::new(Line::from(p)).decor());
            }
        }
        let mut p = self.prefix(false);
        p.extend(border("└", "┴", "┘"));
        self.rows.push(Row::new(Line::from(p)).decor());
    }
}

/// Columns taken by the borders of a grid table: "│ a │ b │" = 3 per column + 1.
fn grid_chrome_width(ncols: usize) -> usize {
    3 * ncols + 1
}

/// Shrinks the widest columns first until the widths sum to at most `avail`
/// (or every column is down to `MIN_COLUMN_WIDTH`).
fn fit_column_widths(mut widths: Vec<usize>, avail: usize) -> Vec<usize> {
    while widths.iter().sum::<usize>() > avail {
        let (widest, _) = widths
            .iter()
            .enumerate()
            .max_by_key(|(_, v)| **v)
            .expect("a table has at least one column");
        if widths[widest] <= MIN_COLUMN_WIDTH {
            break;
        }
        widths[widest] -= 1;
    }
    widths
}

/// Left and right padding that aligns a cell of `pad` spare columns.
fn align_padding(align: Option<&pulldown_cmark::Alignment>, pad: usize) -> (usize, usize) {
    match align {
        Some(pulldown_cmark::Alignment::Right) => (pad, 0),
        Some(pulldown_cmark::Alignment::Center) => (pad / 2, pad - pad / 2),
        _ => (0, pad),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(src: &str, w: usize) -> Vec<String> {
        render(src, w, theme::text())
            .iter()
            .map(|r| text::line_text(&r.line).trim_end().to_string())
            .collect()
    }

    #[test]
    fn headings_and_paragraphs() {
        let out = plain("# Title\n\nHello **world**, `code`.\n\nSecond para", 40);
        assert_eq!(
            out,
            vec!["Title", "", "Hello world, code.", "", "Second para"]
        );
    }

    #[test]
    fn lists() {
        let out = plain("- one\n- two\n  - nested\n1. first\n2. second", 40);
        assert_eq!(out[0], "• one");
        assert_eq!(out[1], "• two");
        assert_eq!(out[2], "  ◦ nested");
        assert!(out.iter().any(|l| l == "1. first"));
        assert!(out.iter().any(|l| l == "2. second"));
    }

    #[test]
    fn list_wrap_indents() {
        let rows = render("- aaaa bbbb cccc", 8, theme::text());
        let t: Vec<String> = rows.iter().map(|r| text::line_text(&r.line)).collect();
        assert_eq!(t[0], "• aaaa");
        assert_eq!(t[1], "  bbbb");
        assert!(rows[1].soft);
        assert_eq!(rows[1].skip, 2);
    }

    #[test]
    fn code_block() {
        let out = plain("```rust\nfn main() {}\n```", 30);
        assert_eq!(out.len(), 1);
        assert!(out[0].starts_with("  fn main() {}"));
        assert!(out[0].ends_with("rust"));
        // Unclosed (streaming) fence still renders.
        let out = plain("text\n```py\nprint(1)", 30);
        assert!(out.iter().any(|l| l.contains("print(1)")));
    }

    #[test]
    fn table() {
        let out = plain("| a | bb |\n|---|---:|\n| 1 | 2 |", 40);
        assert_eq!(out[0], "┌───┬────┐");
        assert_eq!(out[1], "│ a │ bb │");
        assert_eq!(out[2], "├───┼────┤");
        assert_eq!(out[3], "│ 1 │  2 │");
        assert_eq!(out[4], "└───┴────┘");
    }

    #[test]
    fn quote_and_link() {
        let out = plain("> quoted text\n\n[site](https://x.y)", 40);
        assert_eq!(out[0], "│ quoted text");
        assert_eq!(out[2], "site (https://x.y)");
    }

    #[test]
    fn never_exceeds_width() {
        let src = "# 标题很长很长很长很长很长\n\n中文段落，包含很多很多字符以测试换行是否正确处理宽字符。\n\n| 列一 | 列二 |\n|---|---|\n| 很长的单元格内容很长 | x |\n\n```\nlong code line that goes on and on and on\n```";
        for w in [10usize, 17, 25] {
            for r in render(src, w, theme::text()) {
                assert!(
                    text::line_width(&r.line) <= w,
                    "w={w} {:?}",
                    text::line_text(&r.line)
                );
            }
        }
    }
}
