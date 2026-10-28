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
pub fn normalize_overlaps(hunks: Vec<Hunk>, warnings: &mut Vec<String>) -> Result<Vec<Hunk>, String> {
    let mut keep = vec![true; hunks.len()];
    for i in 0..hunks.len() {
        let Some(ri) = hunks[i].deleted() else { continue };
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
    let hunks: Vec<Hunk> =
        hunks.into_iter().zip(keep).filter(|(_, k)| *k).map(|(h, _)| h).collect();
    let mut owner: BTreeMap<usize, usize> = BTreeMap::new();
    for h in &hunks {
        if let Some((a, b)) = h.deleted() {
            for l in a..=b {
                if let Some(prior) = owner.get(&l) {
                    return Err(format!(
                        "line {}: anchor line {l} is already targeted by another hunk on line {prior}. Issue ONE hunk per range; payload is only the final desired content, never a before/after pair.",
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
            HunkKind::Put { loc: Loc::Range(a, b), .. } | HunkKind::Cut { start: a, end: b } => {
                s.extend(*a..=*b);
            }
            HunkKind::Put { loc: Loc::Before(n), .. } if *n > 1 => {
                s.insert(*n);
            }
            HunkKind::Put { loc: Loc::After(n), .. } => {
                s.insert(*n);
            }
            _ => {}
        }
    }
    s
}

/// Check every referenced line exists.
pub fn check_bounds(hunks: &[Hunk], nlines: usize) -> Result<(), String> {
    let oob = |n: usize| format!("Line {n} does not exist (file has {nlines} lines)");
    for h in hunks {
        match &h.kind {
            HunkKind::Put { loc: Loc::Range(_, b), .. } | HunkKind::Cut { end: b, .. } => {
                if *b > nlines {
                    return Err(oob(*b));
                }
            }
            HunkKind::Put { loc: Loc::Before(n), .. } => {
                if *n > nlines && !(*n == 1 && nlines == 0) {
                    return Err(oob(*n));
                }
            }
            HunkKind::Put { loc: Loc::After(n), .. } if *n > nlines => return Err(oob(*n)),
            _ => {}
        }
    }
    Ok(())
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
        if let DiffOp::Equal { old_index, new_index, len } = op {
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
                    Loc::Before(n) => Loc::Before(m(*n)?),
                    Loc::After(n) => Loc::After(m(*n)?),
                    Loc::Eof => Loc::Eof,
                };
                HunkKind::Put { loc, body: body.clone() }
            }
            HunkKind::Cut { start, end } => {
                let (a, b) = range(*start, *end)?;
                HunkKind::Cut { start: a, end: b }
            }
            HunkKind::Rem => HunkKind::Rem,
        };
        out.push(Hunk { kind, src_line: h.src_line });
    }
    Some(out)
}

const PREVIEW_CONTEXT: usize = 2;
const COLLAPSE_OVER: usize = 8;
const COLLAPSE_KEEP: usize = 3;
const MAX_PREVIEW_ROWS: usize = 400;

/// Compact preview of `new` relative to `old`: changed rows + context, NEW numbering.
/// Returns (text, displayed new line numbers).
pub fn preview(old: &[&str], new: &[&str]) -> (String, Vec<usize>) {
    let mut show: BTreeSet<usize> = BTreeSet::new(); // 0-based new indices
    let mut hidden: BTreeSet<usize> = BTreeSet::new();
    let ctx = |show: &mut BTreeSet<usize>, from: usize, to: usize| {
        // context rows around new range [from, to)
        let lo = from.saturating_sub(PREVIEW_CONTEXT);
        let hi = (to + PREVIEW_CONTEXT).min(new.len());
        for i in lo..hi {
            show.insert(i);
        }
    };
    for op in capture_diff_slices(Algorithm::Myers, old, new) {
        match op {
            DiffOp::Equal { .. } => {}
            DiffOp::Delete { new_index, .. } => ctx(&mut show, new_index, new_index),
            DiffOp::Insert { new_index, new_len, .. } | DiffOp::Replace { new_index, new_len, .. } => {
                ctx(&mut show, new_index, new_index + new_len);
                if new_len > COLLAPSE_OVER {
                    for i in (new_index + COLLAPSE_KEEP)..(new_index + new_len - COLLAPSE_KEEP) {
                        hidden.insert(i);
                    }
                }
            }
        }
    }
    let mut out = String::new();
    let mut shown = vec![];
    // Index of the previously visited (shown or hidden) row.
    let mut prev: Option<usize> = None;
    let mut last_was_gap = false;
    let mut rows = 0usize;
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
            break;
        }
        let (text, cut) = crate::util::cap_line(new[i], crate::read::MAX_LINE_CHARS);
        out.push_str(&format!("{}:{}\n", i + 1, text));
        if !cut {
            shown.push(i + 1);
        }
        rows += 1;
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
        let out = apply(&t, "PUT 1:\n+A1\n+A2\nCUT 3\nPUT >4:\n+D2\nPUT <2:\n+pre\nPUT >$:\n+z\n");
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
    fn preview_collapse() {
        let old = ["a", "b", "c"];
        let mut new: Vec<String> = vec!["a".into()];
        new.extend((0..20).map(|i| format!("n{i}")));
        new.push("c".into());
        let newr: Vec<&str> = new.iter().map(|s| s.as_str()).collect();
        let (p, shown) = preview(&old, &newr);
        assert!(p.starts_with("1:a\n2:n0\n3:n1\n4:n2\n…\n19:n17"), "{p}");
        assert!(p.ends_with("21:n19\n22:c"), "{p}");
        assert!(!shown.contains(&10));
    }
}
