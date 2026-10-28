//! Hashline patch parser (subset: PUT / CUT / REM, no blocks, no registers, no MV).

use crate::util::{parse_num_prefix, strip_numbered_prefixes};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loc {
    /// Inclusive original line range.
    Range(usize, usize),
    Before(usize),
    After(usize),
    Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HunkKind {
    Put { loc: Loc, body: Vec<String> },
    Cut { start: usize, end: usize },
    Rem,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub kind: HunkKind,
    /// 1-based line in the patch input (for error messages).
    pub src_line: usize,
}

impl Hunk {
    /// Lines deleted by this hunk (original numbering).
    pub fn deleted(&self) -> Option<(usize, usize)> {
        match self.kind {
            HunkKind::Put { loc: Loc::Range(a, b), .. } => Some((a, b)),
            HunkKind::Cut { start, end } => Some((start, end)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Section {
    pub path: String,
    pub tag: Option<String>,
    pub hunks: Vec<Hunk>,
    pub src_line: usize,
}

#[derive(Debug, Default)]
pub struct Parsed {
    pub sections: Vec<Section>,
    pub warnings: Vec<String>,
}

fn is_hunk_line(l: &str) -> bool {
    l == "REM" || l.starts_with("PUT ") || l.starts_with("CUT ") || l == "PUT" || l == "CUT"
}

/// `[path#TAG]` → (path, tag). Also accepts grep's `## path#TAG` (flagged by the bool).
fn parse_header(l: &str) -> Option<(String, Option<String>, bool)> {
    let (inner, from_grep) = if l.starts_with('[') && l.ends_with(']') && l.len() >= 2 {
        (&l[1..l.len() - 1], false)
    } else {
        // `## path#TAG` from grep, only when it carries a tag (not a markdown heading).
        let rest = l.strip_prefix("## ")?;
        let (_, t) = rest.rsplit_once('#')?;
        is_tag(t).then_some((rest, true))?
    };
    let inner = inner.trim();
    if inner.is_empty() {
        return None;
    }
    match inner.rsplit_once('#') {
        Some((p, t)) if is_tag(t.trim()) => {
            Some((unquote(p.trim()), Some(t.trim().to_ascii_uppercase()), from_grep))
        }
        Some((p, t)) if t.trim().is_empty() => Some((unquote(p.trim()), None, from_grep)),
        _ => Some((unquote(inner), None, from_grep)),
    }
}

fn unquote(p: &str) -> String {
    let p = p.trim();
    if p.len() >= 2
        && ((p.starts_with('"') && p.ends_with('"')) || (p.starts_with('\'') && p.ends_with('\'')))
    {
        p[1..p.len() - 1].to_string()
    } else {
        p.to_string()
    }
}

fn is_tag(t: &str) -> bool {
    t.len() == 4 && t.bytes().all(|b| b.is_ascii_hexdigit())
}

fn parse_lid(s: &str, src: usize) -> Result<usize, String> {
    let s = s.trim();
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("line {src}: expected a line number, got `{s}`"));
    }
    let n: usize = s.parse().map_err(|_| format!("line {src}: line number `{s}` is too large"))?;
    if n == 0 {
        return Err(format!("line {src}: Line 0 does not exist; lines are 1-indexed."));
    }
    Ok(n)
}

fn range_err(src: usize, a: usize, b: usize) -> String {
    format!(
        "line {src}: Invalid absolute range: start {a}, end {b}. The value after `.=` is an absolute source line, not a line count or replacement length. For one line use `PUT {a}:`. For {b} lines starting at {a}, use `PUT {a}.={}:`.",
        a + b - 1
    )
}

/// Parse a range `N.=M` / `N` (also tolerates `N-M`). Returns (start, end, used_dash).
fn parse_range(s: &str, src: usize) -> Result<(usize, usize, bool), String> {
    let s = s.trim();
    if s.ends_with('*') {
        return Err(format!(
            "line {src}: block hunks (`N*`) are not supported; use an explicit range like `PUT N.=M:`."
        ));
    }
    if s.contains('@') {
        return Err(format!(
            "line {src}: registers (`@name`) are not supported; write the content as `+` body rows instead."
        ));
    }
    let (a, b, dash) = if let Some((a, b)) = s.split_once(".=") {
        (a, Some(b), false)
    } else if let Some((a, b)) = s.split_once('-') {
        (a, Some(b), true)
    } else {
        (s, None, false)
    };
    let a = parse_lid(a, src)?;
    let b = match b {
        Some(b) => parse_lid(b, src)?,
        None => a,
    };
    if b < a {
        return Err(range_err(src, a, b));
    }
    Ok((a, b, dash))
}

fn parse_put_loc(s: &str, src: usize) -> Result<(Loc, bool), String> {
    let s = s.trim();
    if s == ">$" || s == "$" {
        return Ok((Loc::Eof, false));
    }
    if let Some(r) = s.strip_prefix('<') {
        if r.trim_end().ends_with('*') {
            return parse_range(r, src).map(|_| unreachable_loc());
        }
        return Ok((Loc::Before(parse_lid(r, src)?), false));
    }
    if let Some(r) = s.strip_prefix('>') {
        if r.trim() == "$" {
            return Ok((Loc::Eof, false));
        }
        if r.trim_end().ends_with('*') {
            return parse_range(r, src).map(|_| unreachable_loc());
        }
        return Ok((Loc::After(parse_lid(r, src)?), false));
    }
    let (a, b, dash) = parse_range(s, src)?;
    Ok((Loc::Range(a, b), dash))
}

// parse_range always errors on `*`; this only satisfies the types.
fn unreachable_loc() -> (Loc, bool) {
    (Loc::Eof, false)
}

struct Builder {
    put: Option<(Loc, usize, Vec<String>, Vec<usize>)>,
    pending_blank: usize,
    repaired_bare: bool,
    dropped_minus: bool,
}

pub fn parse(input: &str) -> Result<Parsed, String> {
    let input = crate::util::normalize_eol(input);
    let mut out = Parsed::default();
    let mut cur: Option<Section> = None;
    let mut b = Builder { put: None, pending_blank: 0, repaired_bare: false, dropped_minus: false };
    let mut dash_warned = false;

    for (i, raw) in input.split('\n').enumerate() {
        let src = i + 1;
        let line = raw;
        let trimmed = line.trim_end();
        if trimmed == "*** Begin Patch" || trimmed == "*** End Patch" || trimmed.starts_with("*** End of File") {
            flush_put(&mut b, &mut cur, &mut out)?;
            continue;
        }
        if trimmed == "*** Abort" {
            break;
        }
        // Body rows.
        if let Some(rest) = line.strip_prefix('+') {
            match &mut b.put {
                Some((_, _, body, kinds)) => {
                    for _ in 0..b.pending_blank {
                        body.push(String::new());
                        kinds.push(0);
                    }
                    b.pending_blank = 0;
                    body.push(rest.to_string());
                    kinds.push(1);
                }
                None => {
                    return Err(format!(
                        "line {src}: body row `{}` is not inside a `PUT …:` hunk. Body rows follow `PUT N.=M:`, `PUT <N:`, `PUT >N:` or `PUT >$:`.",
                        clip(line)
                    ))
                }
            }
            continue;
        }
        if trimmed.is_empty() {
            if b.put.is_some() {
                b.pending_blank += 1;
            }
            continue;
        }
        if let Some((path, tag, from_grep)) = parse_header(trimmed) {
            flush_put(&mut b, &mut cur, &mut out)?;
            if let Some(s) = cur.take() {
                out.sections.push(s);
            }
            if from_grep {
                out.warnings.push(format!(
                    "Section header `{trimmed}` was copied from grep output; use `[path#TAG]` headers."
                ));
            }
            cur = Some(Section { path, tag, hunks: vec![], src_line: src });
            continue;
        }
        if is_hunk_line(trimmed) {
            flush_put(&mut b, &mut cur, &mut out)?;
            let Some(sec) = cur.as_mut() else {
                return Err(format!(
                    "line {src}: hunk `{}` appears before any `[PATH#TAG]` section header.",
                    clip(trimmed)
                ));
            };
            if trimmed == "REM" {
                sec.hunks.push(Hunk { kind: HunkKind::Rem, src_line: src });
            } else if let Some(rest) = trimmed.strip_prefix("CUT") {
                let rest = rest.trim().trim_end_matches(':');
                let (a, e, dash) = parse_range(rest, src)?;
                if dash && !dash_warned {
                    dash_warned = true;
                    out.warnings.push("Ranges are written `N.=M`, not `N-M`; interpreted as inclusive.".into());
                }
                sec.hunks.push(Hunk { kind: HunkKind::Cut { start: a, end: e }, src_line: src });
            } else {
                let rest = trimmed.strip_prefix("PUT").unwrap_or("").trim();
                let Some(locs) = rest.strip_suffix(':') else {
                    return Err(format!(
                        "line {src}: `{}` must end with `:` and be followed by `+` body rows (e.g. `PUT 5:` then `+new text`). To delete lines use `CUT N.=M`.",
                        clip(trimmed)
                    ));
                };
                let (loc, dash) = parse_put_loc(locs, src)?;
                if dash && !dash_warned {
                    dash_warned = true;
                    out.warnings.push("Ranges are written `N.=M`, not `N-M`; interpreted as inclusive.".into());
                }
                b.put = Some((loc, src, vec![], vec![]));
            }
            continue;
        }
        // Bare line: inside a PUT body it is a row missing its `+` prefix.
        if let Some((_, _, body, kinds)) = &mut b.put {
            for _ in 0..b.pending_blank {
                body.push(String::new());
                kinds.push(0);
            }
            b.pending_blank = 0;
            if line.starts_with('-') && !line.starts_with("--") {
                // Diff-style `-old` row; dropped below if the body has explicit `+` rows.
                body.push(line.to_string());
                kinds.push(2);
            } else {
                body.push(line.to_string());
                kinds.push(3);
            }
            continue;
        }
        if cur.is_none() {
            return Err(format!(
                "line {src}: expected a `[PATH#TAG]` section header, got `{}`. Copy the header from `read`/`grep` output.",
                clip(trimmed)
            ));
        }
        return Err(format!(
            "line {src}: unrecognized line `{}`; expected a `PUT`/`CUT`/`REM` hunk, a `+` body row, or a `[PATH#TAG]` header.",
            clip(trimmed)
        ));
    }
    flush_put(&mut b, &mut cur, &mut out)?;
    if let Some(s) = cur.take() {
        out.sections.push(s);
    }
    if b.repaired_bare {
        out.warnings.push("Some body rows were missing the `+` prefix and were treated as body rows; prefix every body row with `+`.".into());
    }
    if b.dropped_minus {
        out.warnings.push("Dropped diff-style `-old` rows; bodies contain only the new content as `+` rows.".into());
    }
    if out.sections.is_empty() {
        return Err("empty patch: expected at least one `[PATH#TAG]` section followed by hunks.".into());
    }
    for s in &out.sections {
        if s.hunks.is_empty() {
            return Err(format!(
                "line {}: section `[{}]` has no hunks.",
                s.src_line,
                s.path
            ));
        }
    }
    Ok(out)
}

fn clip(s: &str) -> String {
    crate::util::cap_line(s, 80).0
}

/// kinds: 0 = blank between rows, 1 = `+` row, 2 = bare `-` row, 3 = bare row.
fn flush_put(b: &mut Builder, cur: &mut Option<Section>, out: &mut Parsed) -> Result<(), String> {
    b.pending_blank = 0;
    let Some((loc, src, mut body, mut kinds)) = b.put.take() else { return Ok(()) };
    let Some(sec) = cur.as_mut() else { return Ok(()) };
    let has_plus = kinds.contains(&1);
    if kinds.contains(&2) {
        if has_plus {
            let mut i = 0;
            body.retain(|_| {
                let keep = kinds[i] != 2;
                i += 1;
                keep
            });
            kinds.retain(|k| *k != 2);
            b.dropped_minus = true;
        } else {
            b.repaired_bare = true;
        }
    }
    if kinds.contains(&3) {
        b.repaired_bare = true;
    }
    // Rows pasted from read output (`N:text`): strip when every row carries consecutive numbers.
    if !body.is_empty() {
        let refs: Vec<&str> = body.iter().map(|s| s.as_str()).collect();
        let all_numbered = refs.iter().all(|r| parse_num_prefix(r).is_some());
        if all_numbered {
            if let Some(stripped) = strip_numbered_prefixes(&refs) {
                let first = parse_num_prefix(refs[0]).map(|(n, _)| n as usize).unwrap_or(0);
                let plausible = refs.len() >= 2
                    || match loc {
                        Loc::Range(a, e) => first + 1 >= a && first <= e + 1,
                        Loc::Before(n) | Loc::After(n) => first + 1 >= n && first <= n + 1,
                        Loc::Eof => false,
                    };
                if plausible {
                    body = stripped;
                    out.warnings.push(format!(
                        "line {src}: stripped `N:` line-number prefixes copied from read output; body rows are `+TEXT` without numbers."
                    ));
                }
            }
        }
    }
    if body.is_empty() {
        match loc {
            Loc::Range(a, e) => {
                out.warnings.push(format!(
                    "line {src}: `PUT` with an empty body was treated as `CUT {}`; use CUT to delete lines.",
                    if a == e { a.to_string() } else { format!("{a}.={e}") }
                ));
                sec.hunks.push(Hunk { kind: HunkKind::Cut { start: a, end: e }, src_line: src });
                return Ok(());
            }
            _ => {
                return Err(format!(
                    "line {src}: insertion hunk has no body rows; add `+TEXT` rows (a lone `+` inserts an empty line)."
                ))
            }
        }
    }
    sec.hunks.push(Hunk { kind: HunkKind::Put { loc, body }, src_line: src });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic() {
        let p = parse("*** Begin Patch\n[a.txt#00AB]\nPUT 2.=3:\n+x\n+\nCUT 5\nPUT >$:\n+end\n*** End Patch\n").unwrap();
        assert_eq!(p.sections.len(), 1);
        let s = &p.sections[0];
        assert_eq!(s.tag.as_deref(), Some("00AB"));
        assert_eq!(s.hunks.len(), 3);
        assert_eq!(
            s.hunks[0].kind,
            HunkKind::Put { loc: Loc::Range(2, 3), body: vec!["x".into(), "".into()] }
        );
        assert_eq!(s.hunks[1].kind, HunkKind::Cut { start: 5, end: 5 });
        assert_eq!(s.hunks[2].kind, HunkKind::Put { loc: Loc::Eof, body: vec!["end".into()] });
    }

    #[test]
    fn repairs() {
        let p = parse("[a#ABCD]\nPUT 3:\n  bare\n+ok\n").unwrap();
        assert_eq!(
            p.sections[0].hunks[0].kind,
            HunkKind::Put { loc: Loc::Range(3, 3), body: vec!["  bare".into(), "ok".into()] }
        );
        assert!(!p.warnings.is_empty());
        let p = parse("[a#ABCD]\nPUT 3.=4:\n3:x\n4:y\n").unwrap();
        assert_eq!(
            p.sections[0].hunks[0].kind,
            HunkKind::Put { loc: Loc::Range(3, 4), body: vec!["x".into(), "y".into()] }
        );
        let p = parse("[a#ABCD]\nPUT 3:\n+3:x\n+5:y\n").unwrap();
        assert_eq!(
            p.sections[0].hunks[0].kind,
            HunkKind::Put { loc: Loc::Range(3, 3), body: vec!["3:x".into(), "5:y".into()] }
        );
    }

    #[test]
    fn errors() {
        assert!(parse("PUT 1:\n+x").unwrap_err().contains("before any"));
        assert!(parse("[a#ABCD]\nPUT 5.=3:\n+x").unwrap_err().contains("Invalid absolute range"));
        assert!(parse("[a#ABCD]\nPUT 5*:\n+x").unwrap_err().contains("not supported"));
        assert!(parse("[a#ABCD]\nPUT 0:\n+x").unwrap_err().contains("1-indexed"));
        assert!(parse("[a#ABCD]\nPUT >2:\n").unwrap_err().contains("no body"));
    }
}
