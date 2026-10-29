//! Markdown → styled, wrapped rows (pulldown-cmark + syntect for code blocks).

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::text::{self, width};
use super::{Row, highlight, theme};

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

struct R {
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
    let width = width.max(4);
    let src = text::sanitize(src);
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    let mut r = R {
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
    while r.rows.last().is_some_and(|x| text::line_width(&x.line) == 0 && x.hits.is_empty()) && r.rows.len() > 1 {
        r.rows.pop();
    }
    r.rows
}

impl R {
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
                Container::Quote => out.push(Span::styled("│ ", theme::fg(theme::MD_QUOTE))),
                Container::Item { marker, used } => {
                    let w = width(marker);
                    if first && !*used {
                        *used = true;
                        let st = if marker.trim_end().ends_with('.') { theme::fg(theme::MD_ENUM) } else { theme::fg(theme::MD_LIST) };
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
                Container::Quote => 2,
                Container::Item { marker, .. } => width(marker),
            })
            .sum()
    }

    fn quote_only_prefix(&self) -> Vec<Span<'static>> {
        self.containers
            .iter()
            .map(|c| match c {
                Container::Quote => Span::styled("│ ", theme::fg(theme::MD_QUOTE)),
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
            if self.containers.iter().any(|c| matches!(c, Container::Quote)) {
                row = row.skip(self.prefix_width() as u16);
            }
            self.rows.push(row);
        }
    }

    fn emit_wrapped(&mut self, lines: Vec<Vec<Span<'static>>>, hard: bool) {
        let avail = self.width.saturating_sub(self.prefix_width()).max(1);
        for logical in lines {
            let wrapped = if hard { text::wrap_hard(&logical, avail) } else { text::wrap_spans(&logical, avail) };
            for (i, l) in wrapped.into_iter().enumerate() {
                let first = i == 0;
                let p = self.prefix(true);
                let pw: usize = p.iter().map(|s| width(&s.content)).sum();
                let mut spans = p;
                spans.extend(l.spans);
                let mut row = Row::new(Line::from(spans)).soft(!first);
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
        let lines = std::mem::take(&mut self.inline);
        // Drop a trailing empty logical line produced by a final newline.
        let mut lines = lines;
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
                if ev_kind_is_block(&t) {
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
                let (s, st) = if done { ("[✓] ", theme::fg(theme::SUCCESS)) } else { ("[ ] ", theme::muted()) };
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
                    HeadingLevel::H1 => theme::bold(theme::MD_HEADING).add_modifier(Modifier::UNDERLINED),
                    HeadingLevel::H2 | HeadingLevel::H3 => theme::bold(theme::MD_HEADING),
                    _ => theme::bold(theme::MD_HEADING).remove_modifier(Modifier::BOLD).add_modifier(Modifier::ITALIC),
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
                    _ => match depth {
                        1 => "• ".to_string(),
                        2 => "◦ ".to_string(),
                        _ => "▪ ".to_string(),
                    },
                };
                self.containers.push(Container::Item { marker, used: false });
                self.suppress_blank = true;
            }
            Tag::Emphasis => self.push_style(|s| s.fg(theme::MD_EMPH).add_modifier(Modifier::ITALIC)),
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
                self.table = Some(Table { aligns, rows: Vec::new(), head_rows: 0, cur_row: Vec::new() });
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
            Tag::FootnoteDefinition(_) | Tag::DefinitionList | Tag::DefinitionListTitle | Tag::DefinitionListDefinition => {
                self.block_start()
            }
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
                if let Some(i) = self.containers.iter().rposition(|c| matches!(c, Container::Quote)) {
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
                if let Some(i) = self.containers.iter().rposition(|c| matches!(c, Container::Item { .. })) {
                    // Item with no content still shows its marker.
                    if let Container::Item { used: false, .. } = &self.containers[i] {
                        let p = self.prefix(true);
                        self.rows.push(Row::new(Line::from(p)));
                    }
                    self.containers.truncate(i);
                }
                self.suppress_blank = false;
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Superscript | TagEnd::Subscript => {
                self.pop_style()
            }
            TagEnd::Link => {
                self.pop_style();
                let url = self.link.pop().unwrap_or_default();
                let shown: String = self.inline.last().map(|l| l.iter().map(|s| s.content.as_ref()).collect()).unwrap_or_default();
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
            TagEnd::FootnoteDefinition | TagEnd::DefinitionList | TagEnd::DefinitionListTitle | TagEnd::DefinitionListDefinition => {
                self.flush()
            }
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
                Some(segs) => segs.iter().map(|(st, s)| Span::styled(s.clone(), st.bg(bg))).collect(),
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
                if i == 0 && j == 0 && !lang_label.is_empty() && fill > width(&lang_label) + 2 {
                    fill -= width(&lang_label);
                    label = Some(Span::styled(lang_label.clone(), Style::new().fg(theme::DIM).bg(bg)));
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
        let cell_w = |c: &Vec<Span<'static>>| c.iter().map(|s| width(&s.content)).sum::<usize>();
        let mut widths = vec![1usize; ncols];
        for r in &t.rows {
            for (i, c) in r.iter().enumerate() {
                widths[i] = widths[i].max(cell_w(c).max(1));
            }
        }
        let pw = self.prefix_width();
        // Borders: "│ a │ b │" = 3*ncols + 1.
        let avail = self.width.saturating_sub(pw + 3 * ncols + 1).max(ncols);
        let total: usize = widths.iter().sum();
        if total > avail {
            // Shrink the widest columns first.
            let mut w = widths.clone();
            while w.iter().sum::<usize>() > avail {
                let (mi, _) = w.iter().enumerate().max_by_key(|(_, v)| **v).unwrap_or((0, &0));
                if w[mi] <= 2 {
                    break;
                }
                w[mi] -= 1;
            }
            widths = w;
        }
        if widths.iter().sum::<usize>() > avail || self.width < pw + 3 * ncols + 1 + ncols {
            // Too narrow for a grid: one line per row, cells separated by " │ ".
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
            return;
        }
        let bst = theme::fg(theme::BORDER);
        let border = |l: &str, m: &str, r: &str, widths: &[usize]| -> Vec<Span<'static>> {
            let mut s = String::from(l);
            for (i, w) in widths.iter().enumerate() {
                s.push_str(&"─".repeat(w + 2));
                s.push_str(if i + 1 == widths.len() { r } else { m });
            }
            vec![Span::styled(s, bst)]
        };
        let top = border("┌", "┬", "┐", &widths);
        let mut p = self.prefix(true);
        p.extend(top);
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
            let h = cells.iter().map(|c| c.len()).max().unwrap_or(1);
            for li in 0..h {
                let mut spans = self.prefix(false);
                for (ci, c) in cells.iter().enumerate() {
                    spans.push(Span::styled(if ci == 0 { "│ " } else { " │ " }, bst));
                    let line = c.get(li).cloned().unwrap_or_default();
                    let lw = text::line_width(&line);
                    let pad = widths[ci].saturating_sub(lw);
                    let (lp, rp) = match t.aligns.get(ci) {
                        Some(pulldown_cmark::Alignment::Right) => (pad, 0),
                        Some(pulldown_cmark::Alignment::Center) => (pad / 2, pad - pad / 2),
                        _ => (0, pad),
                    };
                    spans.push(Span::raw(" ".repeat(lp)));
                    spans.extend(line.spans);
                    spans.push(Span::raw(" ".repeat(rp)));
                }
                spans.push(Span::styled(" │", bst));
                self.rows.push(Row::new(Line::from(spans)).soft(li > 0));
            }
            if head && ri + 1 == t.head_rows {
                let mut p = self.prefix(false);
                p.extend(border("├", "┼", "┤", &widths));
                self.rows.push(Row::new(Line::from(p)).decor());
            }
        }
        let mut p = self.prefix(false);
        p.extend(border("└", "┴", "┘", &widths));
        self.rows.push(Row::new(Line::from(p)).decor());
    }
}

fn ev_kind_is_block(t: &str) -> bool {
    t.ends_with('\n')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(src: &str, w: usize) -> Vec<String> {
        render(src, w, theme::text()).iter().map(|r| text::line_text(&r.line).trim_end().to_string()).collect()
    }

    #[test]
    fn headings_and_paragraphs() {
        let out = plain("# Title\n\nHello **world**, `code`.\n\nSecond para", 40);
        assert_eq!(out, vec!["Title", "", "Hello world, code.", "", "Second para"]);
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
                assert!(text::line_width(&r.line) <= w, "w={w} {:?}", text::line_text(&r.line));
            }
        }
    }
}
