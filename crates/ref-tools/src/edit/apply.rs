//! Lowering hunks to primitive edits and applying them in one ascending pass, plus diff helpers
//! (line mapping, compact preview).

use std::collections::{BTreeMap, BTreeSet};

use similar::{capture_diff_slices, Algorithm, DiffOp};

use super::parse::{Hunk, HunkKind, Loc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Slot {
    Before = 0,
    Replace = 1,
    After = 2,
}

/// Remove exact duplicate ranges (keep last, warn) and reject overlapping deletions.
pub fn normalize_overlaps(
    hunks: Vec<Hunk>,
    warnings: &mut Vec<String>,
) -> Result<Vec<Hunk>, String> {
    let mut keep = vec![true; hunks.len()];
    for i in 0..hunks.len() {
        let Some(ri) = hunks[i].deleted() else {
            continue;
        };
        for later in &hunks[i + 1..] {
            if later.deleted() == Some(ri) {
                keep[i] = false;
                warnings.push(
                    "Multiple hunks targeted the same exact range; kept only the last. Issue one `PUT` or `CUT` hunk per range."
                        .into(),
                );
                break;
            }
        }
    }
    let hunks: Vec<Hunk> = hunks
        .into_iter()
        .zip(keep)
        .filter(|(_, k)| *k)
        .map(|(h, _)| h)
        .collect();
    let mut owner: BTreeMap<usize, usize> = BTreeMap::new();
    for h in &hunks {
        if let Some((a, b)) = h.deleted() {
            for l in a..=b {
                if let Some(prior) = owner.get(&l) {
                    return Err(format!(
                        "line {}: anchor line {l} is already targeted by another hunk on line {prior}. Issue ONE hunk per range (merge overlapping changes into a single `PUT N.=M:`); payload is only the final desired content, never a before/after pair.",
                        h.src_line
                    ));
                }
                owner.insert(l, h.src_line);
            }
        }
    }
    Ok(hunks)
}

/// Lines a hunk anchors to (must have been seen). `<1` and `>$` are exempt.
pub fn anchored_lines(hunks: &[Hunk]) -> BTreeSet<usize> {
    let mut s = BTreeSet::new();
    for h in hunks {
        match &h.kind {
            HunkKind::Put {
                loc: Loc::Range(a, b),
                ..
            }
            | HunkKind::Cut { start: a, end: b } => {
                s.extend(*a..=*b);
            }
            HunkKind::Put {
                loc: Loc::Before(n),
                ..
            } if *n > 1 => {
                s.insert(*n);
            }
            HunkKind::Put {
                loc: Loc::After(n), ..
            } => {
                s.insert(*n);
            }
            _ => {}
        }
    }
    s
}

/// Check every referenced line exists.
pub fn check_bounds(hunks: &[Hunk], nlines: usize) -> Result<(), String> {
    let oob = |n: usize| {
        format!("Line {n} does not exist (file has {nlines} lines). To add lines at the end use `PUT >$:`.")
    };
    for h in hunks {
        match &h.kind {
            HunkKind::Put {
                loc: Loc::Range(_, b),
                ..
            }
            | HunkKind::Cut { end: b, .. } => {
                if *b > nlines {
                    return Err(oob(*b));
                }
            }
            HunkKind::Put {
                loc: Loc::Before(n),
                ..
            } => {
                // `<N+1` (just past the end) is an append.
                if *n > nlines + 1 {
                    return Err(oob(*n));
                }
            }
            HunkKind::Put {
                loc: Loc::After(n), ..
            } if *n > nlines => return Err(oob(*n)),
            _ => {}
        }
    }
    Ok(())
}

/// A line distinctive enough that repeating it right next to itself is almost surely an echo
/// (not `}`, `end`, blank lines or other short punctuation).
fn distinctive(l: &str) -> bool {
    let t = l.trim();
    t.chars().filter(|c| c.is_alphanumeric()).count() >= 3
}

/// Drop body rows that restate the unchanged line just outside a replaced range (or the anchor
/// line of an insertion) — a frequent model mistake that would duplicate that line.
pub fn trim_echoes(hunks: &mut [Hunk], lines: &[&str], warnings: &mut Vec<String>) {
    let line = |n: usize| -> Option<&str> { n.checked_sub(1).and_then(|i| lines.get(i)).copied() };
    let deleted: BTreeSet<usize> = hunks
        .iter()
        .filter_map(|h| h.deleted())
        .flat_map(|(a, b)| a..=b)
        .collect();
    for h in hunks.iter_mut() {
        let src = h.src_line;
        let HunkKind::Put { loc, body } = &mut h.kind else {
            continue;
        };
        let mut dropped: Vec<(usize, &str)> = vec![];
        match *loc {
            Loc::Range(a, b) => {
                if body.len() >= 2 {
                    if let Some(prev) =
                        line(a - 1).filter(|p| !deleted.contains(&(a - 1)) && distinctive(p))
                    {
                        if body[0] == prev && line(a) != Some(prev) {
                            body.remove(0);
                            dropped.push((a - 1, prev));
                        }
                    }
                }
                if body.len() >= 2 {
                    if let Some(next) =
                        line(b + 1).filter(|n| !deleted.contains(&(b + 1)) && distinctive(n))
                    {
                        if body.last().map(String::as_str) == Some(next) && line(b) != Some(next) {
                            body.pop();
                            dropped.push((b + 1, next));
                        }
                    }
                }
            }
            Loc::After(n) => {
                if let Some(anchor) = line(n).filter(|l| !deleted.contains(&n) && distinctive(l)) {
                    if body.len() >= 2 && body[0] == anchor && line(n + 1) != Some(anchor) {
                        body.remove(0);
                        dropped.push((n, anchor));
                    }
                }
            }
            Loc::Before(n) => {
                if let Some(anchor) = line(n).filter(|l| !deleted.contains(&n) && distinctive(l)) {
                    if body.len() >= 2
                        && body.last().map(String::as_str) == Some(anchor)
                        && line(n - 1) != Some(anchor)
                    {
                        body.pop();
                        dropped.push((n, anchor));
                    }
                }
            }
            Loc::Eof => {}
        }
        for (n, l) in dropped {
            warnings.push(format!(
                "line {src}: dropped body row `{}`: it repeats unchanged line {n}, which the hunk does not replace. Body rows are only the new content of the targeted lines.",
                crate::util::cap_line(l, 80).0
            ));
        }
    }
}

/// Word tokens used for rough line similarity: identifiers/numbers of 2+ chars, and each
/// non-ASCII letter (CJK) on its own.
fn tokens(s: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut BTreeSet<String>| {
        if cur.chars().count() >= 2 {
            out.insert(std::mem::take(cur));
        } else {
            cur.clear();
        }
    };
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            cur.push(c);
        } else {
            flush(&mut cur, &mut out);
            if c.is_alphanumeric() {
                out.insert(c.to_string());
            }
        }
    }
    flush(&mut cur, &mut out);
    out
}

/// Fraction of `line`'s tokens that occur in `other` (1.0 when `line` has none).
fn containment(line: &BTreeSet<String>, other: &BTreeSet<String>) -> f64 {
    if line.is_empty() {
        return 1.0;
    }
    line.iter().filter(|t| other.contains(*t)).count() as f64 / line.len() as f64
}

/// Character similarity in [0, 1].
fn similarity(a: &str, b: &str) -> f32 {
    similar::TextDiff::from_chars(a.trim(), b.trim()).ratio()
}

/// Identifier-like words: ASCII words of 3+ chars and runs of 2+ non-ASCII letters (CJK).
fn words(s: &str) -> Vec<&str> {
    // 0 = separator, 1 = ASCII word char, 2 = other letter
    let class = |c: char| -> u8 {
        if c.is_ascii_alphanumeric() || c == '_' {
            1
        } else if !c.is_ascii() && c.is_alphanumeric() {
            2
        } else {
            0
        }
    };
    let mut out = vec![];
    let mut run: Option<(usize, u8)> = None;
    for (i, c) in s.char_indices().chain(std::iter::once((s.len(), ' '))) {
        let k = class(c);
        if let Some((a, rk)) = run {
            if rk == k {
                continue;
            }
            let w = &s[a..i];
            if w.chars().count() >= if rk == 1 { 3 } else { 2 } {
                out.push(w);
            }
        }
        run = (k != 0).then_some((i, k));
    }
    out
}

/// Pair the replaced lines a..=b greedily with body rows by similarity: line → (score, row).
fn pair_rows(lines: &[&str], a: usize, b: usize, body: &[String]) -> BTreeMap<usize, (f32, usize)> {
    let mut pairs: Vec<(f32, usize, usize)> = vec![];
    for n in a..=b {
        for (j, row) in body.iter().enumerate() {
            pairs.push((similarity(lines[n - 1], row), n, j));
        }
    }
    pairs.sort_by(|x, y| y.0.total_cmp(&x.0));
    let (mut used_line, mut used_row) = (BTreeSet::new(), BTreeSet::new());
    let mut best = BTreeMap::new();
    for (score, n, j) in pairs {
        if !used_line.contains(&n) && !used_row.contains(&j) {
            used_line.insert(n);
            used_row.insert(j);
            best.insert(n, (score, j));
        }
    }
    best
}

/// Replaced lines whose current content the body demonstrably knew: a paired body row is a
/// close variant of the line AND resembles it more than the neighbouring lines outside the
/// range (so an off-by-one number is not "verified"). Insertions and CUTs verify nothing.
pub fn content_verified(hunks: &[Hunk], lines: &[&str]) -> BTreeSet<usize> {
    const MIN: f32 = 0.75;
    let mut out = BTreeSet::new();
    for h in hunks {
        let HunkKind::Put {
            loc: Loc::Range(a, b),
            body,
        } = &h.kind
        else {
            continue;
        };
        let (a, b) = (*a, *b);
        if b > lines.len() {
            continue;
        }
        for (n, (score, j)) in pair_rows(lines, a, b, body) {
            if score < MIN || !distinctive(lines[n - 1]) {
                continue;
            }
            let row = &body[j];
            let beats = |m: usize| {
                (a..=b).contains(&m)
                    || m == 0
                    || m > lines.len()
                    || similarity(lines[m - 1], row) < score
            };
            if beats(n - 1) && beats(n + 1) {
                out.insert(n);
            }
        }
    }
    out
}

/// Warn when a short replacement drops a line that no body row resembles and whose words no
/// longer occur anywhere in the file: usually the model meant to insert (`PUT >N:`) rather
/// than replace. Body rows are paired greedily with the replaced lines by similarity.
pub fn lost_line_warnings(hunks: &[Hunk], lines: &[&str], new_lines: &[String]) -> Vec<String> {
    let mut out = vec![];
    let mut new_words: Option<BTreeSet<&str>> = None;
    for h in hunks {
        let HunkKind::Put {
            loc: Loc::Range(a, b),
            body,
        } = &h.kind
        else {
            continue;
        };
        let (a, b) = (*a, *b);
        if b - a + 1 > 3 || body.len() < b - a + 1 || b > lines.len() {
            continue;
        }
        let best = pair_rows(lines, a, b, body);
        for n in a..=b {
            let l = lines[n - 1];
            if !distinctive(l) || best.get(&n).map_or(0.0, |p| p.0) >= 0.7 {
                continue;
            }
            let nw =
                new_words.get_or_insert_with(|| new_lines.iter().flat_map(|l| words(l)).collect());
            if !words(l).iter().any(|w| !nw.contains(w)) {
                continue;
            }
            let range = if a == b {
                format!("{a}")
            } else {
                format!("{a}.={b}")
            };
            out.push(format!(
                "line {}: `PUT {range}:` removed line {n} `{}` and no body row resembles it. If that line should stay, restore it: PUT replaces its range; `PUT <N:`/`PUT >N:` insert without removing anything.",
                h.src_line,
                crate::util::cap_line(l.trim(), 80).0
            ));
        }
    }
    out
}

/// After a stale-tag remap moved a replacement, check whether its body was really written for
/// the CURRENT lines at the authored numbers (the model mixed an old tag with fresh numbers).
pub fn aimed_at_current(authored: &[Hunk], remapped: &[Hunk], lines: &[&str]) -> bool {
    let range_tokens = |a: usize, b: usize| -> BTreeSet<String> {
        (a..=b)
            .filter_map(|n| lines.get(n.wrapping_sub(1)))
            .flat_map(|l| tokens(l))
            .collect()
    };
    for (h, r) in authored.iter().zip(remapped) {
        let (
            HunkKind::Put {
                loc: Loc::Range(a, b),
                body,
            },
            HunkKind::Put {
                loc: Loc::Range(ra, rb),
                ..
            },
        ) = (&h.kind, &r.kind)
        else {
            continue;
        };
        if (a, b) == (ra, rb) || *b > lines.len() {
            continue;
        }
        let body_t: BTreeSet<String> = body.iter().flat_map(|l| tokens(l)).collect();
        if body_t.is_empty() {
            continue;
        }
        let at_authored = containment(&range_tokens(*a, *b), &body_t);
        let at_remapped = containment(&range_tokens(*ra, *rb), &body_t);
        if at_authored >= 0.5 && at_authored > at_remapped + 0.25 {
            return true;
        }
    }
    false
}

/// Apply hunks (original numbering) to `lines`. Bounds must already be checked.
pub fn materialize(lines: &[&str], hunks: &[Hunk]) -> Vec<String> {
    let mut bof: Vec<&str> = vec![];
    let mut eof: Vec<&str> = vec![];
    // (line, slot, authored order) -> rows; stable by construction.
    let mut inserts: BTreeMap<(usize, Slot, usize), &[String]> = BTreeMap::new();
    let mut deleted: BTreeSet<usize> = BTreeSet::new();
    for (order, h) in hunks.iter().enumerate() {
        match &h.kind {
            HunkKind::Put { loc, body } => match loc {
                Loc::Range(a, b) => {
                    inserts.insert((*a, Slot::Replace, order), body);
                    deleted.extend(*a..=*b);
                }
                Loc::Before(n) if lines.is_empty() || *n == 0 => {
                    bof.extend(body.iter().map(|s| s.as_str()))
                }
                Loc::Before(n) if *n > lines.len() => eof.extend(body.iter().map(|s| s.as_str())),
                Loc::Before(n) => {
                    inserts.insert((*n, Slot::Before, order), body);
                }
                Loc::After(n) => {
                    inserts.insert((*n, Slot::After, order), body);
                }
                Loc::Eof => eof.extend(body.iter().map(|s| s.as_str())),
            },
            HunkKind::Cut { start, end } => deleted.extend(*start..=*end),
            HunkKind::Rem => {}
        }
    }
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 16);
    out.extend(bof.iter().map(|s| s.to_string()));
    let mut it = inserts.into_iter().peekable();
    for (i, line) in lines.iter().enumerate() {
        let n = i + 1;
        while let Some(((ln, slot, _), _)) = it.peek() {
            if *ln != n || *slot == Slot::After {
                break;
            }
            if let Some((_, rows)) = it.next() {
                out.extend(rows.iter().cloned());
            }
        }
        if !deleted.contains(&n) {
            out.push(line.to_string());
        }
        while let Some(((ln, _, _), _)) = it.peek() {
            if *ln != n {
                break;
            }
            if let Some((_, rows)) = it.next() {
                out.extend(rows.iter().cloned());
            }
        }
    }
    out.extend(eof.iter().map(|s| s.to_string()));
    out
}

/// Map 1-based old line numbers to 1-based new line numbers for unchanged lines.
pub fn line_map(old: &[&str], new: &[&str]) -> BTreeMap<usize, usize> {
    let mut map = BTreeMap::new();
    for op in capture_diff_slices(Algorithm::Myers, old, new) {
        if let DiffOp::Equal {
            old_index,
            new_index,
            len,
        } = op
        {
            for k in 0..len {
                map.insert(old_index + k + 1, new_index + k + 1);
            }
        }
    }
    map
}

/// Remap hunks from an old snapshot onto the current text. `None` if any anchor is not an
/// unchanged line (or a range is no longer contiguous).
pub fn rebase(hunks: &[Hunk], map: &BTreeMap<usize, usize>) -> Option<Vec<Hunk>> {
    let m = |n: usize| map.get(&n).copied();
    let range = |a: usize, b: usize| -> Option<(usize, usize)> {
        let (na, nb) = (m(a)?, m(b)?);
        // Every line of the range must be unchanged and still contiguous.
        for l in a..=b {
            if m(l)? != na + (l - a) {
                return None;
            }
        }
        Some((na, nb))
    };
    let mut out = Vec::with_capacity(hunks.len());
    for h in hunks {
        let kind = match &h.kind {
            HunkKind::Put { loc, body } => {
                let loc = match loc {
                    Loc::Range(a, b) => {
                        let (na, nb) = range(*a, *b)?;
                        Loc::Range(na, nb)
                    }
                    Loc::Before(1) => Loc::Before(1),
                    // `<N` == `>N-1`; this also covers `<len+1` (append), which has no mapping.
                    Loc::Before(n) => match m(*n) {
                        Some(x) => Loc::Before(x),
                        None => Loc::After(m(*n - 1)?),
                    },
                    Loc::After(n) => Loc::After(m(*n)?),
                    Loc::Eof => Loc::Eof,
                };
                HunkKind::Put {
                    loc,
                    body: body.clone(),
                }
            }
            HunkKind::Cut { start, end } => {
                let (a, b) = range(*start, *end)?;
                HunkKind::Cut { start: a, end: b }
            }
            HunkKind::Rem => HunkKind::Rem,
        };
        out.push(Hunk {
            kind,
            src_line: h.src_line,
        });
    }
    Some(out)
}

const PREVIEW_CONTEXT: usize = 2;
const COLLAPSE_OVER: usize = 8;
const COLLAPSE_KEEP: usize = 3;
const MAX_PREVIEW_ROWS: usize = 400;

/// Removed lines shown per deletion run (the rest is summarized).
const REMOVED_SHOWN: usize = 3;
const REMOVED_CHARS: usize = 160;

/// Compact preview of `new` relative to `old`: changed rows + context in NEW numbering, with
/// removed lines as unnumbered `-text` rows so unintended deletions are visible.
/// Returns (text, displayed new line numbers).
pub fn preview(old: &[&str], new: &[&str]) -> (String, Vec<usize>) {
    let mut show: BTreeSet<usize> = BTreeSet::new(); // 0-based new indices
    let mut hidden: BTreeSet<usize> = BTreeSet::new();
    // new index before which the old lines [a, b) were removed
    let mut removed: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
    let ctx = |show: &mut BTreeSet<usize>, from: usize, to: usize| {
        // context rows around new range [from, to)
        let lo = from.saturating_sub(PREVIEW_CONTEXT);
        let hi = (to + PREVIEW_CONTEXT).min(new.len());
        for i in lo..hi {
            show.insert(i);
        }
    };
    for op in capture_diff_slices(Algorithm::Patience, old, new) {
        match op {
            DiffOp::Equal { .. } => {}
            DiffOp::Delete {
                old_index,
                old_len,
                new_index,
            } => {
                ctx(&mut show, new_index, new_index);
                removed.insert(new_index, (old_index, old_index + old_len));
            }
            DiffOp::Insert {
                new_index, new_len, ..
            } => {
                ctx(&mut show, new_index, new_index + new_len);
                if new_len > COLLAPSE_OVER {
                    hidden
                        .extend((new_index + COLLAPSE_KEEP)..(new_index + new_len - COLLAPSE_KEEP));
                }
            }
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                ctx(&mut show, new_index, new_index + new_len);
                removed.insert(new_index, (old_index, old_index + old_len));
                if new_len > COLLAPSE_OVER {
                    hidden
                        .extend((new_index + COLLAPSE_KEEP)..(new_index + new_len - COLLAPSE_KEEP));
                }
            }
        }
    }
    let render_removed = |out: &mut String, (a, b): (usize, usize)| {
        for (k, l) in old[a..b].iter().enumerate() {
            if k == REMOVED_SHOWN && b - a > REMOVED_SHOWN + 1 {
                out.push_str(&format!(
                    "-… ({} more removed lines)\n",
                    b - a - REMOVED_SHOWN
                ));
                break;
            }
            out.push_str(&format!("-{}\n", crate::util::cap_line(l, REMOVED_CHARS).0));
        }
    };
    let mut out = String::new();
    let mut shown = vec![];
    // Index of the previously visited (shown or hidden) row.
    let mut prev: Option<usize> = None;
    let mut last_was_gap = false;
    let mut rows = 0usize;
    let mut removed_it = removed.into_iter().peekable();
    let mut truncated = false;
    for i in show {
        let gap = prev.is_some_and(|p| p + 1 != i);
        prev = Some(i);
        if hidden.contains(&i) || gap {
            if !last_was_gap {
                out.push_str("…\n");
                last_was_gap = true;
            }
            if hidden.contains(&i) {
                continue;
            }
        }
        last_was_gap = false;
        if rows >= MAX_PREVIEW_ROWS {
            out.push_str("… (preview truncated; read the file to see the rest)\n");
            truncated = true;
            break;
        }
        while let Some((_, r)) = removed_it.next_if(|(k, _)| *k <= i) {
            render_removed(&mut out, r);
        }
        let (text, cut) = crate::util::cap_line(new[i], crate::read::MAX_LINE_CHARS);
        out.push_str(&format!("{}:{}\n", i + 1, text));
        if !cut {
            shown.push(i + 1);
        }
        rows += 1;
    }
    if !truncated {
        // Deletions at the very end of the file.
        for (_, r) in removed_it {
            render_removed(&mut out, r);
        }
    }
    if out.ends_with('\n') {
        out.pop();
    }
    (out, shown)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::parse::parse;

    fn apply(text: &[&str], patch: &str) -> Vec<String> {
        let p = parse(&format!("[f#0000]\n{patch}")).unwrap();
        let mut w = vec![];
        let hunks = normalize_overlaps(p.sections[0].hunks.clone(), &mut w).unwrap();
        check_bounds(&hunks, text.len()).unwrap();
        materialize(text, &hunks)
    }

    #[test]
    fn non_shifting() {
        let t = ["a", "b", "c", "d", "e"];
        let out = apply(
            &t,
            "PUT 1:\n+A1\n+A2\nCUT 3\nPUT >4:\n+D2\nPUT <2:\n+pre\nPUT >$:\n+z\n",
        );
        assert_eq!(out, vec!["A1", "A2", "pre", "b", "d", "D2", "e", "z"]);
        let out = apply(&[], "PUT <1:\n+x\nPUT >$:\n+y\n");
        assert_eq!(out, vec!["x", "y"]);
    }

    #[test]
    fn overlap() {
        let p = parse("[f#0000]\nPUT 1.=3:\n+x\nCUT 3\n").unwrap();
        let err = normalize_overlaps(p.sections[0].hunks.clone(), &mut vec![]).unwrap_err();
        assert!(err.contains("anchor line 3 is already targeted"), "{err}");
        let p = parse("[f#0000]\nPUT 2:\n+x\nPUT 2:\n+y\n").unwrap();
        let mut w = vec![];
        let h = normalize_overlaps(p.sections[0].hunks.clone(), &mut w).unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(w.len(), 1);
    }

    #[test]
    fn preview_block_delete_shows_the_block() {
        // Bodies repeat across classes; the removed lines must be the deleted class, not a
        // mix of the end of C1 and the head of C2.
        let block = |i: usize| {
            vec![
                format!("class C{i}:"),
                "    def run(self):".into(),
                format!("        return {}", i % 2),
                "".into(),
                "    def name(self):".into(),
                format!("        return 'c{i}'"),
                "".into(),
                "".into(),
            ]
        };
        let old: Vec<String> = (0..4).flat_map(block).collect();
        let new: Vec<String> = [0, 1, 3].into_iter().flat_map(block).collect();
        let (o, n): (Vec<&str>, Vec<&str>) = (
            old.iter().map(|s| s.as_str()).collect(),
            new.iter().map(|s| s.as_str()).collect(),
        );
        let (p, _) = preview(&o, &n);
        assert!(
            p.contains("-class C2:\n-    def run(self):\n-        return 0\n"),
            "{p}"
        );
    }

    #[test]
    fn preview_collapse() {
        let old = ["a", "b", "c"];
        let mut new: Vec<String> = vec!["a".into()];
        new.extend((0..20).map(|i| format!("n{i}")));
        new.push("c".into());
        let newr: Vec<&str> = new.iter().map(|s| s.as_str()).collect();
        let (p, shown) = preview(&old, &newr);
        assert!(p.starts_with("1:a\n-b\n2:n0\n3:n1\n4:n2\n…\n19:n17"), "{p}");
        assert!(p.ends_with("21:n19\n22:c"), "{p}");
        assert!(!shown.contains(&10));
    }
}
