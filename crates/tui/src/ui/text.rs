//! Width-aware text helpers: sanitising, wrapping, truncation, formatting.

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub fn width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Width of one grapheme cluster (control chars count as 0, wide emoji as 2).
pub fn gwidth(g: &str) -> usize {
    let w = UnicodeWidthStr::width(g);
    if w > 2 { 2 } else { w }
}

/// Strip ANSI escape sequences, expand tabs to 4 spaces, drop other control chars (keeps `\n`).
pub fn sanitize(s: &str) -> String {
    if !s.bytes().any(|b| b < 0x20 && b != b'\n' || b == 0x7f) {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\x1b' => {
                // CSI: ESC [ ... final byte (0x40..=0x7e); OSC: ESC ] ... BEL | ESC \
                match it.peek() {
                    Some('[') => {
                        it.next();
                        for d in it.by_ref() {
                            if ('\x40'..='\x7e').contains(&d) {
                                break;
                            }
                        }
                    }
                    Some(']') => {
                        it.next();
                        while let Some(d) = it.next() {
                            if d == '\x07' {
                                break;
                            }
                            if d == '\x1b' {
                                if it.peek() == Some(&'\\') {
                                    it.next();
                                }
                                break;
                            }
                        }
                    }
                    Some(_) => {
                        it.next();
                    }
                    None => {}
                }
            }
            '\t' => out.push_str("    "),
            '\n' => out.push('\n'),
            '\r' => {
                // CRLF → LF; lone CR dropped.
            }
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// Single-line version: newlines and tabs become spaces, runs of whitespace collapse.
pub fn one_line(s: &str) -> String {
    let mut out = String::with_capacity(s.len().min(4096));
    let mut space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            space = true;
            continue;
        }
        if c.is_control() {
            continue;
        }
        if space && !out.is_empty() {
            out.push(' ');
        }
        space = false;
        out.push(c);
    }
    out
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
        w += gw;
        out.push_str(g);
    }
    out.push('…');
    out
}

/// The LAST graphemes of `s` that fit in `max` columns; prefixed with `…` when cut.
pub fn tail_fit(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if width(s) <= max {
        return s.to_string();
    }
    let mut parts: Vec<&str> = Vec::new();
    let mut w = 0;
    for g in s.graphemes(true).rev() {
        let gw = gwidth(g);
        if w + gw > max - 1 {
            break;
        }
        w += gw;
        parts.push(g);
    }
    let mut out = String::from("…");
    for g in parts.iter().rev() {
        out.push_str(g);
    }
    // Avoid starting with a space right after the ellipsis.
    if out.len() > 3 && out[3..].starts_with(' ') {
        out = format!("…{}", out[3..].trim_start());
    }
    out
}

/// Punctuation that must not begin a wrapped row (simplified kinsoku shori).
fn no_line_start(g: &str) -> bool {
    g.chars()
        .next()
        .is_some_and(|c| ",.:;!?)]}%、。，．：；！？）】》」』〉〕”’…·ー々".contains(c))
}

fn is_wide(g: &str) -> bool {
    gwidth(g) >= 2
}

/// Greedy word wrap of a styled logical line into rows of at most `width` columns.
/// Breaks at spaces and around wide (CJK) characters; hard-breaks words that don't fit.
/// Returns at least one (possibly empty) line.
pub fn wrap_spans(spans: &[Span<'_>], width: usize) -> Vec<Line<'static>> {
    wrap_spans_flags(spans, width)
        .into_iter()
        .map(|(l, _)| l)
        .collect()
}

/// Like [`wrap_spans`], also reporting for each row whether the break before it swallowed
/// whitespace (so copying can restore exactly one space when joining soft-wrapped rows).
pub fn wrap_spans_flags(spans: &[Span<'_>], width: usize) -> Vec<(Line<'static>, bool)> {
    let width = width.max(1);
    // Flatten to graphemes.
    let mut gs: Vec<(&str, usize, Style)> = Vec::new();
    for sp in spans {
        for g in sp.content.graphemes(true) {
            let w = gwidth(g);
            if w == 0 && g.chars().all(|c| c.is_control()) {
                continue;
            }
            gs.push((g, w, sp.style));
        }
    }
    type Cell<'a> = (&'a str, usize, Style);
    let mut rows: Vec<(Vec<Cell<'_>>, bool)> = Vec::new();
    let mut cur: Vec<Cell<'_>> = Vec::new();
    // Whether the break that started `cur` consumed whitespace.
    let mut cur_sp = false;
    let mut cur_w = 0usize;
    // Index in `cur` where the line could be broken (start of next row).
    let mut brk: Option<usize> = None;
    for &(g, w, st) in &gs {
        let space = g == " ";
        if cur_w + w > width {
            if space {
                rows.push((std::mem::take(&mut cur), cur_sp));
                cur_sp = true;
                cur_w = 0;
                brk = None;
                continue;
            }
            // Never start a row with closing punctuation: carry the preceding wide glyph along.
            if no_line_start(g)
                && brk == Some(cur.len())
                && cur.len() > 1
                && cur.last().is_some_and(|x| is_wide(x.0))
            {
                brk = Some(cur.len() - 1);
            }
            if let Some(b) = brk.filter(|&b| b > 0) {
                let tail: Vec<_> = cur.split_off(b);
                let mut head = std::mem::take(&mut cur);
                let mut ate = false;
                while head.last().is_some_and(|x| x.0 == " ") {
                    head.pop();
                    ate = true;
                }
                rows.push((head, cur_sp));
                let n = tail.len();
                cur = tail.into_iter().skip_while(|x| x.0 == " ").collect();
                cur_sp = ate || cur.len() < n;
                cur_w = cur.iter().map(|x| x.1).sum();
            } else {
                rows.push((std::mem::take(&mut cur), cur_sp));
                cur_sp = false;
                cur_w = 0;
            }
            brk = None;
            if cur_w + w > width && !cur.is_empty() {
                rows.push((std::mem::take(&mut cur), cur_sp));
                cur_sp = false;
                cur_w = 0;
            }
        }
        if is_wide(g) && !cur.is_empty() {
            brk = Some(cur.len());
        }
        cur.push((g, w, st));
        cur_w += w;
        if space || is_wide(g) {
            brk = Some(cur.len());
        }
    }
    rows.push((cur, cur_sp));
    rows.into_iter().map(|(r, sp)| (merge_row(r), sp)).collect()
}

fn merge_row(row: Vec<(&str, usize, Style)>) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut buf = String::new();
    let mut style: Option<Style> = None;
    for (g, _, st) in row {
        if style != Some(st) {
            if let Some(s) = style
                && !buf.is_empty()
            {
                spans.push(Span::styled(std::mem::take(&mut buf), s));
            }
            style = Some(st);
        }
        buf.push_str(g);
    }
    if let Some(s) = style
        && !buf.is_empty()
    {
        spans.push(Span::styled(buf, s));
    }
    Line::from(spans)
}

/// Hard wrap (character granularity, no word logic) — for code and raw output.
pub fn wrap_hard(spans: &[Span<'_>], width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut rows: Vec<Vec<(&str, usize, Style)>> = vec![Vec::new()];
    let mut w = 0;
    for sp in spans {
        for g in sp.content.graphemes(true) {
            let gw = gwidth(g);
            if w + gw > width && w > 0 {
                rows.push(Vec::new());
                w = 0;
            }
            if let Some(r) = rows.last_mut() {
                r.push((g, gw, sp.style));
            }
            w += gw;
        }
    }
    rows.into_iter().map(merge_row).collect()
}

/// Width of a line in columns.
pub fn line_width(l: &Line<'_>) -> usize {
    l.spans.iter().map(|s| width(&s.content)).sum()
}

/// Plain text of a line.
pub fn line_text(l: &Line<'_>) -> String {
    l.spans.iter().map(|s| s.content.as_ref()).collect()
}

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

/// `HH:MM:SS` for today, else `MM-DD HH:MM:SS` (or `YYYY-MM-DD HH:MM` for other years).
pub fn fmt_time(ms: i64) -> String {
    use chrono::{Datelike, Local, TimeZone};
    let Some(t) = Local.timestamp_millis_opt(ms).single() else {
        return "--:--:--".into();
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

/// Short clock time `HH:MM:SS` (no date).
pub fn fmt_clock(ms: i64) -> String {
    use chrono::{Local, TimeZone};
    match Local.timestamp_millis_opt(ms).single() {
        Some(t) => t.format("%H:%M:%S").to_string(),
        None => "--:--:--".into(),
    }
}

/// Duration: `850ms`, `1.4s`, `12s`, `2m 03s`, `1h 02m`.
pub fn fmt_dur(ms: i64) -> String {
    let ms = ms.max(0);
    if ms < 1000 {
        format!("{ms}ms")
    } else if ms < 10_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else if ms < 60_000 {
        format!("{}s", ms / 1000)
    } else if ms < 3_600_000 {
        format!("{}m {:02}s", ms / 60_000, (ms / 1000) % 60)
    } else {
        format!("{}h {:02}m", ms / 3_600_000, (ms / 60_000) % 60)
    }
}

/// Coarse elapsed for live counters: `3s`, `1m 05s`.
pub fn fmt_elapsed(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m {:02}s", s / 60, s % 60)
    } else {
        format!("{}h {:02}m", s / 3600, (s / 60) % 60)
    }
}

/// `950`, `12.3k`, `1.0M`.
pub fn fmt_tokens(n: u64) -> String {
    if n >= 999_950 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

/// Relative time for lists: `just now`, `5m ago`, `3h ago`, `2d ago`, else date.
pub fn fmt_ago(ms: i64, now: i64) -> String {
    let d = (now - ms).max(0) / 1000;
    if d < 45 {
        "just now".into()
    } else if d < 3600 {
        format!("{}m ago", (d / 60).max(1))
    } else if d < 86_400 {
        format!("{}h ago", d / 3600)
    } else if d < 86_400 * 14 {
        format!("{}d ago", d / 86_400)
    } else {
        fmt_time(ms)
    }
}

/// Replace `$HOME` prefix with `~`.
pub fn tilde(path: &str) -> String {
    if let Some(h) = std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(dirs::home_dir)
    {
        let h = h.to_string_lossy();
        if !h.is_empty() && h != "/" {
            if path == h {
                return "~".into();
            }
            if let Some(rest) = path.strip_prefix(h.as_ref())
                && rest.starts_with(std::path::is_separator)
            {
                return format!("~{rest}");
            }
        }
    }
    path.to_string()
}

/// Simple fuzzy subsequence score (higher is better), None if no match.
pub fn fuzzy_score(hay: &str, needle: &str) -> Option<i64> {
    if needle.is_empty() {
        return Some(0);
    }
    let h: Vec<char> = hay.to_lowercase().chars().collect();
    let n: Vec<char> = needle.to_lowercase().chars().collect();
    let mut score = 0i64;
    let mut hi = 0usize;
    let mut last: Option<usize> = None;
    for &c in &n {
        if c == ' ' {
            continue;
        }
        let mut found = None;
        while hi < h.len() {
            if h[hi] == c {
                found = Some(hi);
                hi += 1;
                break;
            }
            hi += 1;
        }
        let i = found?;
        score += 10;
        if let Some(l) = last {
            if i == l + 1 {
                score += 15;
            } else {
                score -= (i - l) as i64 / 2;
            }
        } else {
            score -= i as i64;
        }
        if i == 0 || matches!(h.get(i.wrapping_sub(1)), Some('/' | '-' | '_' | ' ' | '.')) {
            score += 12;
        }
        last = Some(i);
    }
    if h.iter()
        .collect::<String>()
        .starts_with(&n.iter().collect::<String>())
    {
        score += 40;
    }
    Some(score)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(s: &str, w: usize) -> Vec<String> {
        wrap_spans(&[Span::raw(s.to_string())], w)
            .iter()
            .map(line_text)
            .collect()
    }

    #[test]
    fn wrap_words() {
        assert_eq!(rows("hello world foo", 11), vec!["hello world", "foo"]);
        assert_eq!(rows("hello world foo", 5), vec!["hello", "world", "foo"]);
        assert_eq!(rows("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
        assert_eq!(rows("", 4), vec![""]);
    }

    #[test]
    fn wrap_flags_record_swallowed_spaces() {
        let f = |s: &str, w: usize| {
            wrap_spans_flags(&[Span::raw(s.to_string())], w)
                .into_iter()
                .map(|(l, sp)| (line_text(&l), sp))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            f("hello world", 5),
            vec![("hello".into(), false), ("world".into(), true)]
        );
        assert_eq!(
            f("abcdefgh", 4),
            vec![("abcd".into(), false), ("efgh".into(), false)]
        );
        // Break after a CJK run at a space keeps the space; between CJK glyphs it doesn't.
        assert_eq!(
            f("中文 abc", 5),
            vec![("中文".into(), false), ("abc".into(), true)]
        );
        assert_eq!(
            f("中文中文", 4),
            vec![("中文".into(), false), ("中文".into(), false)]
        );
    }

    #[test]
    fn wrap_cjk() {
        let r = rows("你好世界你好", 5);
        assert_eq!(r, vec!["你好", "世界", "你好"]);
        for row in &r {
            assert!(width(row) <= 5);
        }
        let r = rows("abc 中文混合 text", 8);
        for row in &r {
            assert!(width(row) <= 8, "{row:?}");
        }
    }

    #[test]
    fn wrap_never_starts_with_punctuation() {
        assert_eq!(rows("中文中文，好", 8), vec!["中文中", "文，好"]);
        assert_eq!(rows("你好世界,abc", 8), vec!["你好世", "界,abc"]);
        assert_eq!(rows("abc def,", 7), vec!["abc", "def,"]);
    }

    #[test]
    fn tail_fit_cjk() {
        assert_eq!(tail_fit("abcdef", 10), "abcdef");
        assert_eq!(tail_fit("abcdef", 4), "…def");
        let t = tail_fit("思考中的内容很长很长", 7);
        assert!(width(&t) <= 7, "{t}");
        assert!(t.ends_with("很长"));
        assert_eq!(tail_fit("x", 0), "");
    }

    #[test]
    fn truncate_works() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 6), "hello…");
        assert!(width(&truncate("中文中文中文", 5)) <= 5);
    }

    #[test]
    fn sanitize_strips_ansi() {
        assert_eq!(sanitize("\x1b[31mred\x1b[0m\tx\r\n"), "red    x\n");
        assert_eq!(sanitize("a\x1b]0;title\x07b"), "ab");
    }

    #[test]
    fn formats() {
        assert_eq!(fmt_dur(850), "850ms");
        assert_eq!(fmt_dur(1400), "1.4s");
        assert_eq!(fmt_dur(125_000), "2m 05s");
        assert_eq!(fmt_tokens(12_345), "12.3k");
        assert_eq!(fmt_tokens(1_000_000), "1.0M");
        assert!(fuzzy_score("claude-opus", "opus").is_some());
        assert!(fuzzy_score("claude", "xyz").is_none());
    }
}
