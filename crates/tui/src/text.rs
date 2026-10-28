//! Display-width aware text helpers: wrapping styled text, truncation, tail fitting,
//! and time/duration formatting.

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Display width of a string (CJK = 2 columns).
pub fn width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Display width of a single grapheme. Control characters count as 0, tabs are
/// expected to be expanded by the caller.
pub fn gwidth(g: &str) -> usize {
    UnicodeWidthStr::width(g)
}

/// Expand tabs to spaces (fixed 4) and strip other control characters that would
/// corrupt the terminal grid.
pub fn sanitize(s: &str) -> std::borrow::Cow<'_, str> {
    if !s.chars().any(|c| c == '\t' || c == '\r' || (c.is_control() && c != '\n')) {
        return std::borrow::Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '\t' => out.push_str("    "),
            '\r' => {}
            '\n' => out.push('\n'),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    std::borrow::Cow::Owned(out)
}

/// Truncate to `max` columns, appending `…` when cut.
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
        let gw = gwidth(g);
        if w + gw > max - 1 {
            break;
        }
        out.push_str(g);
        w += gw;
    }
    out.push('…');
    out
}

/// Collapse all whitespace runs (including newlines) to single spaces.
pub fn one_line(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            space = true;
        } else {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            if !c.is_control() {
                out.push(c);
            }
        }
    }
    out
}

/// The latest characters of `text` that fit in `max` columns (whitespace collapsed,
/// newlines become spaces). Prefixed with `…` when the text was cut.
/// Only scans the end of the text, so it is cheap even for very long reasoning.
pub fn tail_fit(text: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    // Only the last chunk can ever be visible; avoid normalizing megabytes.
    let budget = max * 4 + 64;
    let start = if text.len() > budget {
        let mut i = text.len() - budget;
        while !text.is_char_boundary(i) {
            i += 1;
        }
        i
    } else {
        0
    };
    let norm = one_line(&text[start..]);
    let cut_prefix = start > 0;
    if !cut_prefix && width(&norm) <= max {
        return norm;
    }
    let mut parts: Vec<&str> = Vec::new();
    let mut w = 0;
    for g in norm.graphemes(true).rev() {
        let gw = gwidth(g);
        if w + gw > max - 1 {
            break;
        }
        parts.push(g);
        w += gw;
    }
    let mut out = String::from("…");
    for g in parts.iter().rev() {
        out.push_str(g);
    }
    // Avoid a leading space right after the ellipsis.
    if out.starts_with("… ") {
        out.replace_range(.."… ".len(), "…");
    }
    out
}

/// Is this grapheme a "wide" character that can be broken around (CJK etc.)?
fn breakable_wide(g: &str) -> bool {
    gwidth(g) >= 2
}

#[derive(Clone)]
struct Tok {
    gs: Vec<(String, Style)>,
    w: usize,
    space: bool,
}

fn tokenize(segs: &[(String, Style)]) -> Vec<Tok> {
    let mut toks: Vec<Tok> = Vec::new();
    let mut cur: Option<Tok> = None;
    for (text, style) in segs {
        for g in text.graphemes(true) {
            if g == "\n" {
                continue;
            }
            let gw = gwidth(g);
            let space = g.chars().all(char::is_whitespace);
            let wide = breakable_wide(g);
            let push_new = match &cur {
                None => true,
                Some(t) => t.space != space || wide || t.gs.last().map(|(l, _)| breakable_wide(l)).unwrap_or(false),
            };
            if push_new {
                if let Some(t) = cur.take() {
                    toks.push(t);
                }
                cur = Some(Tok { gs: Vec::new(), w: 0, space });
            }
            let t = cur.as_mut().unwrap();
            t.gs.push((g.to_string(), *style));
            t.w += gw;
        }
    }
    if let Some(t) = cur {
        toks.push(t);
    }
    toks
}

fn push_g(spans: &mut Vec<Span<'static>>, g: &str, style: Style) {
    if let Some(last) = spans.last_mut() {
        if last.style == style {
            last.content.to_mut().push_str(g);
            return;
        }
    }
    spans.push(Span::styled(g.to_string(), style));
}

/// A wrapped visual line: spans + whether it continues the previous logical line.
pub struct Wrapped {
    pub spans: Vec<Span<'static>>,
    #[allow(dead_code)]
    pub cont: bool,
    pub width: usize,
}

/// Word-wrap one logical line of styled segments into lines of at most `width`
/// columns. CJK characters may break anywhere; overlong words are hard-split.
/// Never returns an empty vector (an empty input yields one empty line).
pub fn wrap_segments(segs: &[(String, Style)], width: usize) -> Vec<Wrapped> {
    let width = width.max(1);
    let toks = tokenize(segs);
    let mut out: Vec<Wrapped> = Vec::new();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut w = 0usize;
    let mut cont = false;
    let flush = |out: &mut Vec<Wrapped>, spans: &mut Vec<Span<'static>>, w: &mut usize, cont: &mut bool| {
        // Trim trailing whitespace of wrapped lines.
        while let Some(last) = spans.last_mut() {
            let trimmed_len = last.content.trim_end().len();
            if trimmed_len == last.content.len() {
                break;
            }
            let removed = width_of(&last.content[trimmed_len..]);
            *w -= removed;
            if trimmed_len == 0 {
                spans.pop();
            } else {
                last.content.to_mut().truncate(trimmed_len);
                break;
            }
        }
        out.push(Wrapped { spans: std::mem::take(spans), cont: *cont, width: *w });
        *w = 0;
        *cont = true;
    };
    for tok in toks {
        if tok.space {
            if w == 0 && cont {
                continue; // drop leading whitespace on continuation lines
            }
            if w + tok.w > width {
                flush(&mut out, &mut spans, &mut w, &mut cont);
                continue;
            }
            for (g, s) in &tok.gs {
                push_g(&mut spans, g, *s);
            }
            w += tok.w;
            continue;
        }
        if w + tok.w <= width {
            for (g, s) in &tok.gs {
                push_g(&mut spans, g, *s);
            }
            w += tok.w;
            continue;
        }
        if tok.w <= width && w > 0 {
            flush(&mut out, &mut spans, &mut w, &mut cont);
            for (g, s) in &tok.gs {
                push_g(&mut spans, g, *s);
            }
            w += tok.w;
            continue;
        }
        // Hard split.
        for (g, s) in &tok.gs {
            let gw = gwidth(g);
            if w + gw > width && w > 0 {
                flush(&mut out, &mut spans, &mut w, &mut cont);
            }
            push_g(&mut spans, g, *s);
            w += gw;
        }
    }
    if !spans.is_empty() || out.is_empty() {
        flush(&mut out, &mut spans, &mut w, &mut cont);
    }
    out
}

fn width_of(s: &str) -> usize {
    width(s)
}


/// Pad a line with spaces (in `style`) to exactly `w` columns (truncating if longer).
pub fn pad_line(mut line: Line<'static>, w: usize, style: Style) -> Line<'static> {
    let cur = line.width();
    if cur < w {
        line.spans.push(Span::styled(" ".repeat(w - cur), style));
    }
    line
}

/// Plain text of a line's spans.
pub fn line_text(line: &Line<'_>) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

/// Extract the text between display columns [x0, x1) of a line.
pub fn slice_cols(line: &Line<'_>, x0: usize, x1: usize) -> String {
    let mut out = String::new();
    let mut col = 0;
    for span in &line.spans {
        for g in span.content.graphemes(true) {
            let gw = gwidth(g);
            if col >= x0 && col < x1 {
                out.push_str(g);
            }
            col += gw;
            if col >= x1 {
                return out;
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Time formatting
// ---------------------------------------------------------------------------

/// `HH:MM:SS`, or `MM-DD HH:MM:SS` when not today (with year if not this year).
pub fn fmt_time(ms: i64) -> String {
    use chrono::{Datelike, Local, TimeZone};
    let Some(t) = Local.timestamp_millis_opt(ms).single() else {
        return String::new();
    };
    let now = Local::now();
    if t.date_naive() == now.date_naive() {
        t.format("%H:%M:%S").to_string()
    } else if t.year() == now.year() {
        t.format("%m-%d %H:%M:%S").to_string()
    } else {
        t.format("%Y-%m-%d %H:%M").to_string()
    }
}

/// Compact duration: `0.4s`, `12s`, `1m 05s`, `1h 02m`.
pub fn fmt_dur(ms: i64) -> String {
    let ms = ms.max(0);
    if ms < 10_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else if ms < 60_000 {
        format!("{}s", ms / 1000)
    } else if ms < 3_600_000 {
        format!("{}m {:02}s", ms / 60_000, (ms / 1000) % 60)
    } else {
        format!("{}h {:02}m", ms / 3_600_000, (ms / 60_000) % 60)
    }
}

/// Countdown seconds (rounded up), e.g. `3s`.
pub fn fmt_countdown(ms: i64) -> String {
    let s = (ms.max(0) + 999) / 1000;
    if s >= 60 {
        format!("{}m {:02}s", s / 60, s % 60)
    } else {
        format!("{s}s")
    }
}

/// Human token count: 950, 12.3K, 1.2M.
pub fn fmt_tokens(n: u64) -> String {
    if n < 1000 {
        n.to_string()
    } else if n < 1_000_000 {
        format!("{:.1}K", n as f64 / 1000.0)
    } else {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    }
}

/// Replace $HOME prefix with `~`.
pub fn tilde(path: &str) -> String {
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() && path.starts_with(&home) {
            return format!("~{}", &path[home.len()..]);
        }
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(ws: &[Wrapped]) -> Vec<String> {
        ws.iter().map(|w| w.spans.iter().map(|s| s.content.as_ref()).collect()).collect()
    }

    #[test]
    fn wrap_ascii_words() {
        let segs = vec![("hello world foo bar".to_string(), Style::default())];
        let w = wrap_segments(&segs, 11);
        assert_eq!(plain(&w), vec!["hello world", "foo bar"]);
        assert!(!w[0].cont);
        assert!(w[1].cont);
    }

    #[test]
    fn wrap_cjk_breaks_anywhere() {
        let segs = vec![("中文字符测试换行".to_string(), Style::default())];
        let w = wrap_segments(&segs, 7);
        // 3 chars = 6 columns fit; the 4th would make 8.
        assert_eq!(plain(&w), vec!["中文字", "符测试", "换行"]);
        for l in &w {
            assert!(l.width <= 7);
        }
    }

    #[test]
    fn wrap_mixed_cjk_ascii() {
        let segs = vec![("用 Rust 写一个 TUI 程序".to_string(), Style::default())];
        let w = wrap_segments(&segs, 10);
        for l in &w {
            assert!(l.width <= 10, "{:?}", plain(&w));
        }
        let joined: String = plain(&w).join("");
        assert_eq!(joined.replace(' ', ""), "用Rust写一个TUI程序");
    }

    #[test]
    fn wrap_long_word_hard_split() {
        let segs = vec![("abcdefghijkl".to_string(), Style::default())];
        assert_eq!(plain(&wrap_segments(&segs, 5)), vec!["abcde", "fghij", "kl"]);
    }

    #[test]
    fn wrap_keeps_styles() {
        let a = Style::default().fg(ratatui::style::Color::Red);
        let segs = vec![("foo ".to_string(), Style::default()), ("bar".to_string(), a)];
        let w = wrap_segments(&segs, 20);
        assert_eq!(w[0].spans.len(), 2);
        assert_eq!(w[0].spans[1].style, a);
    }

    #[test]
    fn tail_fit_short_and_long() {
        assert_eq!(tail_fit("hello\nworld", 20), "hello world");
        let t = tail_fit("the quick brown fox jumps", 10);
        assert_eq!(width(&t), 10);
        assert!(t.starts_with('…'));
        assert!(t.ends_with("jumps"));
    }

    #[test]
    fn tail_fit_cjk() {
        let t = tail_fit("思考中文推理过程", 7);
        // ellipsis (1) + 3 CJK chars (6)
        assert_eq!(t, "…理过程");
        assert!(width(&t) <= 7);
        let t = tail_fit("思考中文推理过程", 8);
        assert!(width(&t) <= 8);
        assert!(t.ends_with("过程"));
    }

    #[test]
    fn tail_fit_huge_text() {
        let big = "x".repeat(1_000_000) + " end";
        let t = tail_fit(&big, 12);
        assert_eq!(width(&t), 12);
        assert!(t.ends_with(" end"));
    }

    #[test]
    fn truncate_width() {
        assert_eq!(truncate("hello world", 6), "hello…");
        assert_eq!(truncate("中文中文", 5), "中文…");
        assert_eq!(truncate("ok", 5), "ok");
    }

    #[test]
    fn slice_columns() {
        let line = Line::from(vec![Span::raw("ab"), Span::raw("中文cd")]);
        assert_eq!(slice_cols(&line, 1, 4), "b中");
        assert_eq!(slice_cols(&line, 2, 8), "中文cd");
    }

    #[test]
    fn durations() {
        assert_eq!(fmt_dur(400), "0.4s");
        assert_eq!(fmt_dur(12_300), "12s");
        assert_eq!(fmt_dur(65_000), "1m 05s");
        assert_eq!(fmt_countdown(2100), "3s");
    }
}
