//! Unicode-width aware text helpers: measuring, truncating, and word-wrapping styled spans.

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub fn width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

pub fn line_width(l: &Line) -> usize {
    l.spans.iter().map(|s| width(&s.content)).sum()
}

/// Expand tabs and drop control characters (they would corrupt the terminal grid).
pub fn sanitize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\t' => out.push_str("    "),
            '\r' => {}
            c if c.is_control() && c != '\n' => {}
            c => out.push(c),
        }
    }
    out
}

/// Truncate to at most `max` columns, appending `…` if cut.
pub fn truncate(s: &str, max: usize) -> String {
    if width(s) <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut w = 0;
    for g in s.graphemes(true) {
        let gw = width(g);
        if w + gw > max - 1 {
            break;
        }
        out.push_str(g);
        w += gw;
    }
    out.push('…');
    out
}

/// The longest suffix of `s` (newlines/tabs folded to spaces, whitespace runs collapsed) that fits in
/// `max` columns. Used for the live "Thinking" tail.
pub fn tail_fit(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    // Only look at a bounded suffix for speed on long reasoning streams.
    let start = s.len().saturating_sub(max * 4 + 16);
    let mut start = start;
    while !s.is_char_boundary(start) {
        start += 1;
    }
    let mut flat = String::new();
    let mut last_space = false;
    for c in s[start..].chars() {
        let c = if c.is_whitespace() { ' ' } else { c };
        if c.is_control() {
            continue;
        }
        if c == ' ' {
            if last_space {
                continue;
            }
            last_space = true;
        } else {
            last_space = false;
        }
        flat.push(c);
    }
    let flat = flat.trim();
    let gs: Vec<&str> = flat.graphemes(true).collect();
    let mut w = 0;
    let mut i = gs.len();
    while i > 0 {
        let gw = width(gs[i - 1]);
        if w + gw > max {
            break;
        }
        w += gw;
        i -= 1;
    }
    let mut out: String = gs[i..].concat();
    if i > 0 {
        // Replace the first char with an ellipsis to show it's cut.
        let mut it = out.graphemes(true);
        if let Some(first) = it.next() {
            let rest: String = it.collect();
            let fw = width(first);
            out = format!("…{}{}", " ".repeat(fw.saturating_sub(1)), rest);
        }
    }
    out
}

fn is_wide(g: &str) -> bool {
    width(g) >= 2
}

#[derive(Debug, Clone)]
struct Token {
    segs: Vec<(String, Style)>,
    width: usize,
    space: bool,
}

impl Token {
    fn push(&mut self, g: &str, style: Style) {
        if let Some(last) = self.segs.last_mut() {
            if last.1 == style {
                last.0.push_str(g);
                self.width += width(g);
                return;
            }
        }
        self.segs.push((g.to_string(), style));
        self.width += width(g);
    }
}

fn tokenize(spans: &[Span]) -> Vec<Token> {
    let mut toks: Vec<Token> = Vec::new();
    for sp in spans {
        for g in sp.content.graphemes(true) {
            let space = g.chars().all(|c| c == ' ');
            let wide = is_wide(g);
            let start_new = match toks.last() {
                None => true,
                Some(t) => t.space != space || wide || t.segs.last().map(|s| is_wide_last(&s.0)).unwrap_or(false),
            };
            if start_new {
                toks.push(Token { segs: vec![], width: 0, space });
            }
            toks.last_mut().unwrap().push(g, sp.style);
        }
    }
    toks
}

fn is_wide_last(s: &str) -> bool {
    s.graphemes(true).next_back().map(is_wide).unwrap_or(false)
}

/// Word-wrap a logical line of styled spans to `width` columns. `first` is prepended to the first output
/// line and `cont` to continuation lines (their widths are subtracted from the available width).
pub fn wrap_spans(spans: &[Span<'static>], width: usize, first: &[Span<'static>], cont: &[Span<'static>]) -> Vec<Line<'static>> {
    let fw = first.iter().map(|s| self::width(&s.content)).sum::<usize>();
    let cw = cont.iter().map(|s| self::width(&s.content)).sum::<usize>();
    let avail_first = width.saturating_sub(fw).max(1);
    let avail_cont = width.saturating_sub(cw).max(1);
    let toks = tokenize(spans);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut cur: Vec<Span<'static>> = first.to_vec();
    let mut cur_w = 0usize;
    let mut avail = avail_first;
    let mut is_first = true;

    let push_segs = |cur: &mut Vec<Span<'static>>, segs: &[(String, Style)]| {
        for (s, st) in segs {
            if let Some(last) = cur.last_mut() {
                if last.style == *st {
                    let mut c = last.content.to_string();
                    c.push_str(s);
                    last.content = c.into();
                    continue;
                }
            }
            cur.push(Span::styled(s.clone(), *st));
        }
    };

    macro_rules! newline {
        () => {{
            lines.push(Line::from(std::mem::take(&mut cur)));
            cur = cont.to_vec();
            cur_w = 0;
            avail = avail_cont;
            is_first = false;
        }};
    }

    for tok in toks.iter() {
        if tok.width <= avail - cur_w.min(avail) {
            if tok.space && cur_w == 0 && !is_first {
                continue; // drop leading space on wrapped lines
            }
            push_segs(&mut cur, &tok.segs);
            cur_w += tok.width;
            continue;
        }
        if tok.space {
            newline!();
            continue;
        }
        if tok.width <= avail_cont && cur_w > 0 {
            newline!();
            push_segs(&mut cur, &tok.segs);
            cur_w += tok.width;
            continue;
        }
        // Hard-break a long word grapheme by grapheme.
        for (s, st) in &tok.segs {
            for g in s.graphemes(true) {
                let gw = self::width(g);
                if cur_w + gw > avail && cur_w > 0 {
                    newline!();
                }
                push_segs(&mut cur, &[(g.to_string(), *st)]);
                cur_w += gw;
            }
        }
    }
    let _ = is_first;
    // Trim trailing spaces of each line (keep styles).
    lines.push(Line::from(cur));
    for l in lines.iter_mut() {
        trim_end_line(l);
    }
    lines
}

fn trim_end_line(l: &mut Line<'static>) {
    while let Some(last) = l.spans.last_mut() {
        let t = last.content.trim_end_matches(' ');
        if t.is_empty() && last.style.bg.is_none() {
            l.spans.pop();
            continue;
        }
        if t.len() != last.content.len() && last.style.bg.is_none() {
            last.content = t.to_string().into();
        }
        break;
    }
}

/// Wrap plain text (may contain newlines) with one style.
pub fn wrap_text(s: &str, width: usize, style: Style, first: &[Span<'static>], cont: &[Span<'static>]) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    for (i, l) in sanitize(s).split('\n').enumerate() {
        let f = if i == 0 { first } else { cont };
        out.extend(wrap_spans(&[Span::styled(l.to_string(), style)], width, f, cont));
    }
    out
}

/// Hard-wrap (character based, no word boundaries) — for raw tool output / code.
pub fn hard_wrap(s: &str, width: usize, style: Style, prefix: &[Span<'static>]) -> Vec<Line<'static>> {
    let pw: usize = prefix.iter().map(|s| self::width(&s.content)).sum();
    let avail = width.saturating_sub(pw).max(1);
    let mut out = Vec::new();
    for l in sanitize(s).split('\n') {
        let mut cur = String::new();
        let mut w = 0;
        let mut any = false;
        for g in l.graphemes(true) {
            let gw = self::width(g);
            if w + gw > avail {
                let mut spans = prefix.to_vec();
                spans.push(Span::styled(std::mem::take(&mut cur), style));
                out.push(Line::from(spans));
                w = 0;
                any = true;
            }
            cur.push_str(g);
            w += gw;
        }
        if !cur.is_empty() || !any {
            let mut spans = prefix.to_vec();
            spans.push(Span::styled(cur.trim_end().to_string(), style));
            out.push(Line::from(spans));
        }
    }
    out
}

/// Pad a line with spaces (in `style`) to exactly `width` columns (used for background-filled blocks).
pub fn pad_line(mut l: Line<'static>, width: usize, style: Style) -> Line<'static> {
    let w = line_width(&l);
    if w < width {
        l.spans.push(Span::styled(" ".repeat(width - w), style));
    }
    l
}

/// Plain text content of a line.
pub fn line_text(l: &Line) -> String {
    l.spans.iter().map(|s| s.content.as_ref()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(ls: &[Line]) -> Vec<String> {
        ls.iter().map(line_text).collect()
    }

    #[test]
    fn wraps_words() {
        let ls = wrap_text("hello world foo bar", 11, Style::default(), &[], &[]);
        assert_eq!(texts(&ls), vec!["hello world", "foo bar"]);
    }

    #[test]
    fn wraps_cjk_by_width() {
        let ls = wrap_text("你好世界你好世界", 6, Style::default(), &[], &[]);
        assert_eq!(texts(&ls), vec!["你好世", "界你好", "世界"]);
        for l in &ls {
            assert!(line_width(l) <= 6);
        }
    }

    #[test]
    fn wraps_mixed_cjk_ascii_never_exceeds() {
        let s = "这是一个test的句子，包含English words和中文字符混合在一起的情况。";
        for w in 3..30 {
            let ls = wrap_text(s, w, Style::default(), &[], &[]);
            for l in &ls {
                assert!(line_width(l) <= w, "w={w} line={:?}", line_text(l));
            }
            let joined: String = texts(&ls).concat().replace(' ', "");
            assert_eq!(joined, s.replace(' ', ""));
        }
    }

    #[test]
    fn prefixes() {
        let p = vec![Span::raw("- ")];
        let c = vec![Span::raw("  ")];
        let ls = wrap_text("aaa bbb ccc", 7, Style::default(), &p, &c);
        assert_eq!(texts(&ls), vec!["- aaa", "  bbb", "  ccc"]);
    }

    #[test]
    fn long_word_breaks() {
        let ls = wrap_text("abcdefghij", 4, Style::default(), &[], &[]);
        assert_eq!(texts(&ls), vec!["abcd", "efgh", "ij"]);
    }

    #[test]
    fn tail() {
        assert_eq!(tail_fit("hello\nworld", 20), "hello world");
        let t = tail_fit("abcdefghij", 5);
        assert_eq!(t, "…ghij");
        assert_eq!(width(&t), 5);
        let t = tail_fit("前面的思考内容然后继续", 7);
        assert!(width(&t) <= 7, "{t}");
        assert!(t.ends_with("继续"));
        assert_eq!(tail_fit("x", 0), "");
    }

    #[test]
    fn truncates() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 6), "hello…");
        assert_eq!(width(&truncate("你好世界", 5)), 5);
    }

    #[test]
    fn hard_wrap_cjk() {
        let ls = hard_wrap("ab你好cd", 4, Style::default(), &[]);
        assert_eq!(texts(&ls), vec!["ab你", "好cd"]);
    }
}
