//! Apply parsed hashline ops to a file's lines; build the post-edit preview; remap stale anchors.

use std::collections::{BTreeSet, HashMap, HashSet};

use similar::{capture_diff_slices, group_diff_ops, Algorithm, DiffOp};

use super::parse::{Gap, Op, OpKind};

const EMPTY_PASTE: &str = "Nothing to paste: no unlabeled `CUT` precedes this `PUT` in this call, and the anonymous register never carries across calls. Put `CUT N.=M` above it, or use named registers (`CUT … @name` → `PUT … @name`) for cross-call moves.";

/// Clipboard state for one call: persisted named registers (forked) + per-call anonymous cuts.
#[derive(Debug, Default, Clone)]
pub struct Clipboard {
    pub named: HashMap<String, Vec<String>>,
    /// Named registers written during this call (committed after the write lands).
    pub dirty: HashMap<String, Vec<String>>,
    anon_pending: Vec<(usize, Vec<String>)>,
}

impl Clipboard {
    pub fn new(named: HashMap<String, Vec<String>>) -> Self {
        Clipboard { named, ..Default::default() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// New LF text.
    pub text: String,
    pub warnings: Vec<String>,
}

fn has_content(s: &str) -> bool {
    !s.trim().is_empty()
}

/// Rows at the start of `body` restating the lines just above `start`.
fn restated_above(body: &[String], start: usize, lines: &[&str]) -> usize {
    let max = body.len().min(start.saturating_sub(1));
    for count in (1..=max).rev() {
        let src = start - 1 - count;
        let cand = &body[..count];
        if cand.iter().zip(&lines[src..src + count]).all(|(a, b)| a == b) && cand.iter().any(|l| has_content(l)) {
            return count;
        }
    }
    0
}

/// Rows at the end of `body` restating the lines just below `end`.
fn restated_below(body: &[String], end: usize, lines: &[&str]) -> usize {
    let max = body.len().min(lines.len().saturating_sub(end));
    for count in (1..=max).rev() {
        let cand = &body[body.len() - count..];
        if cand.iter().zip(&lines[end..end + count]).all(|(a, b)| a == b) && cand.iter().any(|l| has_content(l)) {
            return count;
        }
    }
    0
}

fn echo_warning(start: usize, lead: usize, trail: usize) -> String {
    let mut parts = Vec::new();
    if lead > 0 {
        parts.push(format!("{lead} leading"));
    }
    if trail > 0 {
        parts.push(format!("{trail} trailing"));
    }
    format!(
        "Auto-repaired a replacement boundary echo at line {start}: dropped {} body line(s) already present outside the range. Issue the body as final content for the selected range only.",
        parts.join(" and ")
    )
}

fn short_echo_error(start: usize, end: usize, side: &str, count: usize) -> String {
    format!(
        "`PUT {start}.={end}:` rejected: the body {side} the {count} line(s) just {} the range, but is too short to be the full final content of the selected range. Re-issue with the range covering exactly the lines that change and the body as their complete final content.",
        if side.starts_with("opens") { "above" } else { "below" }
    )
}

/// Drop body rows that duplicate the unchanged neighbours of a replaced range.
fn repair_echo(start: usize, end: usize, body: &mut Vec<String>, lines: &[&str]) -> Result<Option<String>, String> {
    let range_len = end - start + 1;
    let lead = restated_above(body, start, lines);
    let trail = restated_below(body, end, lines);
    let (mut dl, mut dt) = (0, 0);
    if lead > 0 && trail > 0 {
        if body.len() >= lead + trail && body.len() - lead - trail == range_len {
            dl = lead;
            dt = trail;
        }
    } else if lead > 0 && range_len > 1 {
        if body.len() - lead >= range_len {
            dl = lead;
        } else {
            return Err(short_echo_error(start, end, "opens by restating", lead));
        }
    } else if trail > 0 && range_len > 1 {
        if body.len() - trail >= range_len {
            dt = trail;
        } else {
            return Err(short_echo_error(start, end, "ends by restating", trail));
        }
    }
    if dl == 0 && dt == 0 {
        return Ok(None);
    }
    body.drain(..dl);
    body.truncate(body.len() - dt);
    Ok(Some(echo_warning(start, dl, dt)))
}

fn check_line(n: usize, count: usize) -> Result<(), String> {
    if n < 1 || n > count {
        return Err(format!("Line {n} does not exist (file has {count} lines)"));
    }
    Ok(())
}

/// Resolve registers and lower ops to (deletes, inserts by slot). Validates bounds.
pub fn apply_ops(text: &str, ops: &[Op], clip: &mut Clipboard) -> Result<Applied, String> {
    let lines = super::split_lines(text);
    let n = lines.len();
    let trailing_nl = text.ends_with('\n') || text.is_empty();
    let mut warnings = Vec::new();

    for op in ops {
        for a in op.anchors() {
            check_line(a, n)?;
        }
    }

    // Lowered edits.
    #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Slot {
        Before,
        Replacement,
        After,
    }
    let mut deleted: BTreeSet<usize> = BTreeSet::new();
    let mut inserts: Vec<(usize, Slot, String)> = Vec::new();
    let mut bof: Vec<String> = Vec::new();
    let mut eof: Vec<String> = Vec::new();

    let mut push_gap = |gap: Gap, body: Vec<String>, inserts: &mut Vec<(usize, Slot, String)>| match gap {
        Gap::Bof => bof.extend(body),
        Gap::Eof => eof.extend(body),
        Gap::Before(l) => inserts.extend(body.into_iter().map(|t| (l, Slot::Before, t))),
        Gap::After(l) => inserts.extend(body.into_iter().map(|t| (l, Slot::After, t))),
    };

    let take_named = |clip: &Clipboard, name: &str| -> Option<Vec<String>> {
        clip.dirty.get(name).or_else(|| clip.named.get(name)).cloned()
    };
    let known = |clip: &Clipboard| -> String {
        let mut k: Vec<&String> = clip.named.keys().chain(clip.dirty.keys()).collect();
        k.sort();
        k.dedup();
        if k.is_empty() {
            String::new()
        } else {
            format!(" Available registers: {}.", k.iter().map(|s| format!("`@{s}`")).collect::<Vec<_>>().join(", "))
        }
    };

    for op in ops {
        match &op.kind {
            OpKind::Replace { start, end, body } => {
                let mut body = body.clone();
                if let Some(w) = repair_echo(*start, *end, &mut body, &lines)? {
                    warnings.push(w);
                }
                deleted.extend(*start..=*end);
                inserts.extend(body.into_iter().map(|t| (*start, Slot::Replacement, t)));
            }
            OpKind::Insert { gap, body } => push_gap(*gap, body.clone(), &mut inserts),
            OpKind::Cut { start, end, register } => {
                let captured: Vec<String> = lines[start - 1..*end].iter().map(|s| s.to_string()).collect();
                match register {
                    Some(name) => {
                        clip.dirty.insert(name.clone(), captured);
                    }
                    None => clip.anon_pending.push((op.line, captured)),
                }
                deleted.extend(*start..=*end);
            }
            OpKind::PasteGap { gap, register } => {
                let body = match register {
                    Some(name) => match take_named(clip, name) {
                        Some(b) => b,
                        None => {
                            warnings.push(format!(
                                "`@{name}` was empty — no `CUT … @{name}` precedes this op in this call and no persisted register has that name — so nothing was pasted.{}",
                                known(clip)
                            ));
                            Vec::new()
                        }
                    },
                    None => match clip.anon_pending.len() {
                        0 => return Err(format!("line {}: {EMPTY_PASTE}", op.line)),
                        1 => clip.anon_pending.remove(0).1,
                        k => {
                            let at: Vec<String> = clip.anon_pending.iter().map(|(l, _)| format!("line {l}")).collect();
                            return Err(format!(
                                "line {}: {k} unlabeled `CUT`s are pending ({}) — an unlabeled paste cannot tell which one you meant. Label the moves (`CUT … @name` → `PUT … @name`).",
                                op.line,
                                at.join(", ")
                            ));
                        }
                    },
                };
                push_gap(*gap, body, &mut inserts);
            }
            OpKind::PasteSpan { start, end, register } => {
                let Some(body) = take_named(clip, register) else {
                    return Err(format!(
                        "line {}: `@{register}` is empty — no `CUT … @{register}` precedes this op in this call and no persisted register has that name — so pasting it over a range would delete those lines and write nothing back.{}",
                        op.line,
                        known(clip)
                    ));
                };
                deleted.extend(*start..=*end);
                inserts.extend(body.into_iter().map(|t| (*start, Slot::Replacement, t)));
            }
        }
    }

    // Materialize: per line → [Before][Replacement][line unless deleted][After].
    let mut by_line: HashMap<usize, Vec<(Slot, String)>> = HashMap::new();
    for (l, slot, t) in inserts {
        by_line.entry(l).or_default().push((slot, t));
    }
    let mut out: Vec<String> = Vec::with_capacity(n + 8);
    out.extend(bof);
    for (i, line) in lines.iter().enumerate() {
        let ln = i + 1;
        let group = by_line.remove(&ln).unwrap_or_default();
        for slot in [Slot::Before, Slot::Replacement] {
            out.extend(group.iter().filter(|(s, _)| *s == slot).map(|(_, t)| t.clone()));
        }
        if !deleted.contains(&ln) {
            out.push(line.to_string());
        }
        out.extend(group.iter().filter(|(s, _)| *s == Slot::After).map(|(_, t)| t.clone()));
    }
    out.extend(eof);

    let mut new_text = out.join("\n");
    if trailing_nl && !out.is_empty() {
        new_text.push('\n');
    }
    Ok(Applied { text: new_text, warnings })
}

/// Pending anonymous cuts never pasted are fine (they're deletions).
pub fn finish_clipboard(clip: &mut Clipboard) {
    clip.anon_pending.clear();
}

// ---------------------------------------------------------------------------
// Preview
// ---------------------------------------------------------------------------

/// Compact post-edit view: changed regions in new numbering with ±2 context.
/// Returns (rows, line numbers displayed in full).
pub fn preview(old: &str, new: &str) -> (Vec<String>, Vec<u32>) {
    let old_lines = super::split_lines(old);
    let new_lines = super::split_lines(new);
    let ops = capture_diff_slices(Algorithm::Myers, &old_lines, &new_lines);
    let groups = group_diff_ops(ops, 2);
    let mut rows = Vec::new();
    let mut shown = Vec::new();
    let emit = |rows: &mut Vec<String>, shown: &mut Vec<u32>, idx: usize| {
        let ln = idx + 1;
        let (t, cut) = crate::util::truncate_chars(new_lines[idx], super::MAX_COLUMNS);
        if !cut {
            shown.push(ln as u32);
        }
        rows.push(format!("{ln}:{t}"));
    };
    for (gi, group) in groups.iter().enumerate() {
        if gi > 0 {
            rows.push("…".to_string());
        }
        for op in group {
            match *op {
                DiffOp::Equal { new_index, len, .. } => {
                    for i in new_index..new_index + len {
                        emit(&mut rows, &mut shown, i);
                    }
                }
                DiffOp::Delete { .. } => {}
                DiffOp::Insert { new_index, new_len, .. } | DiffOp::Replace { new_index, new_len, .. } => {
                    if new_len > 5 {
                        for i in new_index..new_index + 2 {
                            emit(&mut rows, &mut shown, i);
                        }
                        rows.push("…".to_string());
                        for i in new_index + new_len - 2..new_index + new_len {
                            emit(&mut rows, &mut shown, i);
                        }
                    } else {
                        for i in new_index..new_index + new_len {
                            emit(&mut rows, &mut shown, i);
                        }
                    }
                }
            }
        }
    }
    (rows, shown)
}

/// Map old line numbers (1-based) to new ones through unchanged runs.
pub fn line_map(old: &str, new: &str) -> HashMap<usize, usize> {
    let old_lines = super::split_lines(old);
    let new_lines = super::split_lines(new);
    let mut map = HashMap::new();
    for op in capture_diff_slices(Algorithm::Myers, &old_lines, &new_lines) {
        if let DiffOp::Equal { old_index, new_index, len } = op {
            for k in 0..len {
                map.insert(old_index + k + 1, new_index + k + 1);
            }
        }
    }
    map
}

// ---------------------------------------------------------------------------
// Stale-anchor recovery
// ---------------------------------------------------------------------------

fn duplicated<'a>(lines: &[&'a str]) -> HashSet<&'a str> {
    let mut seen = HashSet::new();
    let mut dup = HashSet::new();
    for l in lines {
        if !seen.insert(*l) {
            dup.insert(*l);
        }
    }
    dup
}

/// Remap ops authored against `previous` onto `current`. Every anchor must sit in an unchanged
/// run, keep a consistent offset within its op, and have a neighbour that maps with the same
/// offset (both neighbours for lines that are not unique).
pub fn remap_ops(previous: &str, current: &str, ops: &[Op]) -> Option<Vec<Op>> {
    let map = line_map(previous, current);
    let prev_lines = super::split_lines(previous);
    let cur_lines = super::split_lines(current);
    let prev_dup = duplicated(&prev_lines);
    let cur_dup = duplicated(&cur_lines);
    let anchors: BTreeSet<usize> = ops.iter().flat_map(|o| o.anchors()).collect();

    // Neighbours of each contiguous run of anchors.
    let sorted: Vec<usize> = anchors.iter().copied().collect();
    let mut i = 0;
    while i < sorted.len() {
        let mut j = i;
        while j + 1 < sorted.len() && sorted[j + 1] == sorted[j] + 1 {
            j += 1;
        }
        let before = (sorted[i] > 1).then(|| sorted[i] - 1);
        let after = (sorted[j] < prev_lines.len()).then(|| sorted[j] + 1);
        for &line in &sorted[i..=j] {
            let mapped = *map.get(&line)?;
            let off = mapped as i64 - line as i64;
            let ok = |nb: Option<usize>| {
                nb.and_then(|b| map.get(&b).map(|&v| v as i64 - b as i64 == off))
            };
            let pv = prev_lines[line - 1];
            let cv = cur_lines[mapped - 1];
            if !prev_dup.contains(pv) && !cur_dup.contains(cv) {
                if ok(before) != Some(true) && ok(after) != Some(true) {
                    return None;
                }
            } else {
                let b = ok(before);
                let a = ok(after);
                if b == Some(false) || a == Some(false) || (b.is_none() && a.is_none()) {
                    return None;
                }
            }
        }
        i = j + 1;
    }

    let mut out = Vec::with_capacity(ops.len());
    for op in ops {
        let anchors = op.anchors();
        let offs: Vec<i64> = anchors.iter().map(|a| map[a] as i64 - *a as i64).collect();
        if offs.windows(2).any(|w| w[0] != w[1]) {
            return None;
        }
        let m = |l: usize| map[&l];
        let kind = match &op.kind {
            OpKind::Replace { start, end, body } => OpKind::Replace { start: m(*start), end: m(*end), body: body.clone() },
            OpKind::Cut { start, end, register } => OpKind::Cut { start: m(*start), end: m(*end), register: register.clone() },
            OpKind::PasteSpan { start, end, register } => OpKind::PasteSpan { start: m(*start), end: m(*end), register: register.clone() },
            OpKind::Insert { gap, body } => OpKind::Insert { gap: map_gap(*gap, &m), body: body.clone() },
            OpKind::PasteGap { gap, register } => OpKind::PasteGap { gap: map_gap(*gap, &m), register: register.clone() },
        };
        out.push(Op { kind, line: op.line });
    }
    Some(out)
}

fn map_gap(gap: Gap, m: &impl Fn(usize) -> usize) -> Gap {
    match gap {
        Gap::Before(l) => Gap::Before(m(l)),
        Gap::After(l) => Gap::After(m(l)),
        g => g,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hashline::parse::parse_patch;

    fn run(text: &str, patch: &str) -> Result<Applied, String> {
        let secs = parse_patch(patch)?;
        let mut clip = Clipboard::default();
        apply_ops(text, &secs[0].ops, &mut clip)
    }

    const SRC: &str = "one\ntwo\nthree\nfour\nfive\n";

    #[test]
    fn replace_insert_delete() {
        let r = run(SRC, "[f#AAAA]\nPUT 2.=3:\n+TWO\nPUT <1:\n+zero\nPUT >5:\n+six\nPUT >$:\n+seven\nPUT <4:\n+3.5\n").unwrap();
        assert_eq!(r.text, "zero\none\nTWO\n3.5\nfour\nfive\nsix\nseven\n");
        let r = run(SRC, "[f#AAAA]\nCUT 1.=2\nCUT 5.=5\n").unwrap();
        assert_eq!(r.text, "three\nfour\n");
    }

    #[test]
    fn numbers_are_original() {
        // Second hunk's numbers ignore the first hunk's growth.
        let r = run(SRC, "[f#AAAA]\nPUT 1.=1:\n+a\n+b\n+c\nPUT 4.=4:\n+FOUR\n").unwrap();
        assert_eq!(r.text, "a\nb\nc\ntwo\nthree\nFOUR\nfive\n");
    }

    #[test]
    fn registers_move_lines() {
        let r = run(SRC, "[f#AAAA]\nCUT 1.=2\nPUT >5\n").unwrap();
        assert_eq!(r.text, "three\nfour\nfive\none\ntwo\n");
        let r = run(SRC, "[f#AAAA]\nCUT 4.=5 @m\nPUT <1 @m\n").unwrap();
        assert_eq!(r.text, "four\nfive\none\ntwo\nthree\n");
        let e = run(SRC, "[f#AAAA]\nPUT >5\n").unwrap_err();
        assert!(e.contains("Nothing to paste"));
        let e = run(SRC, "[f#AAAA]\nPUT 1.=1 @nope\n").unwrap_err();
        assert!(e.contains("is empty"));
        // span paste with named register
        let r = run(SRC, "[f#AAAA]\nCUT 5.=5 @x\nPUT 1.=1 @x\n").unwrap();
        assert_eq!(r.text, "five\ntwo\nthree\nfour\n");
    }

    #[test]
    fn bounds_and_no_trailing_newline() {
        let e = run(SRC, "[f#AAAA]\nPUT 6.=6:\n+x\n").unwrap_err();
        assert_eq!(e, "Line 6 does not exist (file has 5 lines)");
        let r = run("a\nb", "[f#AAAA]\nPUT >$:\n+c\n").unwrap();
        assert_eq!(r.text, "a\nb\nc");
        let r = run("", "[f#AAAA]\nPUT >$:\n+c\n").unwrap();
        assert_eq!(r.text, "c\n");
        let r = run("", "[f#AAAA]\nPUT <1:\n+c\n").unwrap();
        assert_eq!(r.text, "c\n");
    }

    #[test]
    fn boundary_echo_repaired() {
        // Body restates line 1 (above) and line 5 (below) around a 3-line range.
        let r = run(SRC, "[f#AAAA]\nPUT 2.=4:\n+one\n+2\n+3\n+4\n+five\n").unwrap();
        assert_eq!(r.text, "one\n2\n3\n4\nfive\n");
        assert!(r.warnings[0].contains("boundary echo"));
        // Leading echo on a multi-line range.
        let r = run(SRC, "[f#AAAA]\nPUT 2.=3:\n+one\n+2\n+3\n").unwrap();
        assert_eq!(r.text, "one\n2\n3\nfour\nfive\n");
        // Too short → refused.
        let e = run(SRC, "[f#AAAA]\nPUT 2.=4:\n+one\n+x\n").unwrap_err();
        assert!(e.contains("too short"), "{e}");
    }

    #[test]
    fn preview_rows() {
        let old = "a\nb\nc\nd\ne\nf\ng\nh\n";
        let new = "a\nb\nc\nX\ne\nf\ng\nh\n";
        let (rows, shown) = preview(old, new);
        assert_eq!(rows, vec!["2:b", "3:c", "4:X", "5:e", "6:f"]);
        assert_eq!(shown, vec![2, 3, 4, 5, 6]);
    }

    #[test]
    fn remap_through_inserted_lines() {
        let prev = "a\nb\nc\nd\n";
        let cur = "new1\nnew2\na\nb\nc\nd\n";
        let ops = parse_patch("[f#AAAA]\nPUT 3.=3:\n+C\n").unwrap().remove(0).ops;
        let mapped = remap_ops(prev, cur, &ops).unwrap();
        assert_eq!(mapped[0].span(), Some((5, 5)));
        // Anchor line changed → no remap.
        let cur2 = "a\nb\nCHANGED\nd\n";
        assert!(remap_ops(prev, cur2, &ops).is_none());
    }
}
