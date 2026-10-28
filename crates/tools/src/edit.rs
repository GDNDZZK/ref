//! `edit`: apply line-addressed edits against a hashline snapshot.

use crate::hashline::{self, LineSet, Seen, Snapshot, format_header};
use crate::util::{self, TextFile};
use crate::{ToolCtx, ToolOutput, ToolSpec};
use anyhow::{Result, anyhow, bail};
use regex::Regex;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;
use std::sync::LazyLock;

/// Context rows around each change in the success preview.
const PREVIEW_CONTEXT: usize = 2;
/// Context rows around each anchor in mismatch diagnostics.
const MISMATCH_CONTEXT: usize = 2;
/// Max rows in the success preview.
const PREVIEW_MAX_ROWS: usize = 200;
/// Max lines revealed inline by the seen-line guard.
const REVEAL_MAX_LINES: usize = 40;
const REVEAL_MAX_CHARS: usize = 512;
/// Max chars per preview / context row.
const ROW_MAX_CHARS: usize = 500;

pub fn spec() -> ToolSpec {
    ToolSpec {
        name: "edit",
        description: "Edit an existing text file by line number. Create new files with `write`.\n\
\n\
- `tag`: the 4-hex TAG from the `[path#TAG]` header of the LATEST `read`, `grep`, `write` or `edit` output for this file. Never invent it.\n\
- Line numbers are the `N` of the `N:content` rows in that output, i.e. they refer to the file version identified by `tag`. All edits in one call are applied simultaneously against that version, so never shift numbers to account for earlier edits in the same call.\n\
- Ops:\n\
  {\"op\":\"replace\",\"start\":N,\"end\":M,\"lines\":[...]} replaces lines N..M inclusive (omit `end` for one line); `lines` may have any length.\n\
  {\"op\":\"delete\",\"start\":N,\"end\":M} deletes lines N..M.\n\
  {\"op\":\"insert_after\",\"line\":N,\"lines\":[...]} inserts after line N (N=0 = top of file).\n\
  {\"op\":\"insert_before\",\"line\":N,\"lines\":[...]} inserts before line N.\n\
- `lines` are complete raw lines WITHOUT the `N:` prefix and without trailing newlines; indentation must be exact. Use `[\"\"]` for one blank line.\n\
- Ranges must not overlap; use one replace per contiguous region and only touch lines you saw in full.\n\
- On success the output shows the new `[path#TAG]` header and the changed region with new line numbers. Use that new tag (and those numbers) for the next edit; the old tag is stale.\n\
- If rejected because the file changed, use the current lines shown in the error or re-read the file.\n\
- Line endings, BOM and the final newline of the file are preserved."
            .to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File to edit."},
                "tag": {"type": "string", "description": "4-hex TAG from the file's latest [path#TAG] header."},
                "edits": {
                    "type": "array",
                    "minItems": 1,
                    "description": "Edits, all addressed by line numbers of the tagged version.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "op": {"type": "string", "enum": ["replace", "delete", "insert_before", "insert_after"]},
                            "start": {"type": "integer", "minimum": 1, "description": "replace/delete: first line of the range."},
                            "end": {"type": "integer", "minimum": 1, "description": "replace/delete: last line of the range, inclusive (default = start)."},
                            "line": {"type": "integer", "minimum": 0, "description": "insert_before/insert_after: anchor line (insert_after 0 = top of file)."},
                            "lines": {"type": "array", "items": {"type": "string"}, "description": "replace/insert: new lines, verbatim, without N: prefixes."}
                        },
                        "required": ["op"]
                    }
                }
            },
            "required": ["path", "tag", "edits"]
        }),
    }
}

pub async fn run(input: Value, ctx: &ToolCtx) -> Result<ToolOutput> {
    let ctx = ctx.clone();
    util::blocking(move || run_sync(&input, &ctx)).await
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    /// Replace inclusive `start..=end` (empty `lines` = delete).
    Replace { start: usize, end: usize },
    /// Insert into gap `g` (after line g; 0 = top).
    Insert { gap: usize, before: bool, anchor: usize },
}

#[derive(Debug, Clone)]
struct Op {
    /// Index in the input array (for messages).
    idx: usize,
    kind: Kind,
    lines: Vec<String>,
}

impl Op {
    /// Original lines this op is anchored to (must exist / be seen).
    fn anchor_lines(&self, total: usize) -> Vec<usize> {
        match self.kind {
            Kind::Replace { start, end } => (start..=end).collect(),
            Kind::Insert { anchor, .. } => {
                if anchor >= 1 && anchor <= total { vec![anchor] } else { vec![] }
            }
        }
    }

    fn describe(&self) -> String {
        match &self.kind {
            Kind::Replace { start, end } if self.lines.is_empty() => range_label("delete", *start, *end),
            Kind::Replace { start, end } => range_label("replace", *start, *end),
            Kind::Insert { before: true, anchor, .. } => format!("insert_before line {anchor}"),
            Kind::Insert { anchor, .. } => format!("insert_after line {anchor}"),
        }
    }
}

fn range_label(op: &str, s: usize, e: usize) -> String {
    if s == e { format!("{op} line {s}") } else { format!("{op} lines {s}-{e}") }
}

static PREFIX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(?:[*+-]\s*)?(\d+):").expect("valid regex"));
static HEADER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*\[[^#\r\n]+#[0-9a-fA-F]{4}\]\s*$").expect("valid regex"));

/// Strips `N:` prefixes when the model obviously pasted read output: every non-empty row
/// carries a prefix and the numbers count up by one (2+ rows), or a single row is numbered
/// exactly like the line it replaces.
fn strip_pasted_prefixes(lines: &mut Vec<String>, replace_start: Option<usize>) -> bool {
    let rows: Vec<&String> = lines.iter().filter(|l| !l.is_empty() && !HEADER_RE.is_match(l)).collect();
    if rows.is_empty() {
        return false;
    }
    let mut prev: Option<u64> = None;
    let mut first = None;
    for r in &rows {
        let Some(n) = PREFIX_RE.captures(r).and_then(|c| c[1].parse::<u64>().ok()) else {
            return false;
        };
        if prev.is_some_and(|p| p + 1 != n) {
            return false;
        }
        first.get_or_insert(n);
        prev = Some(n);
    }
    if rows.len() == 1 && replace_start.map(|s| s as u64) != first {
        return false;
    }
    let stripped: Vec<String> = lines
        .iter()
        .filter(|l| !HEADER_RE.is_match(l))
        .map(|l| PREFIX_RE.replace(l, "").into_owned())
        .collect();
    *lines = stripped;
    true
}

fn parse_lines(v: Option<&Value>, idx: usize) -> Result<Option<Vec<String>>> {
    let push = |out: &mut Vec<String>, s: &str| {
        for part in util::normalize_to_lf(s).split('\n') {
            out.push(part.to_string());
        }
    };
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => {
            let s = s.strip_suffix('\n').unwrap_or(s);
            let mut out = Vec::new();
            push(&mut out, s);
            Ok(Some(out))
        }
        Some(Value::Array(items)) => {
            let mut out = Vec::new();
            for it in items {
                match it {
                    Value::String(s) => push(&mut out, s),
                    Value::Null => out.push(String::new()),
                    Value::Number(_) | Value::Bool(_) => out.push(it.to_string()),
                    _ => bail!("edits[{idx}]: `lines` must be an array of strings"),
                }
            }
            Ok(Some(out))
        }
        Some(_) => bail!("edits[{idx}]: `lines` must be an array of strings"),
    }
}

fn parse_edits(input: &Value, warnings: &mut Vec<String>) -> Result<Vec<Op>> {
    let edits = match input.get("edits") {
        Some(Value::String(s)) => serde_json::from_str::<Value>(s)
            .map_err(|e| anyhow!("`edits` must be an array of edit objects ({e})"))?,
        Some(v) => v.clone(),
        None => bail!("missing required parameter `edits`"),
    };
    let edits = match edits {
        Value::Array(a) => a,
        Value::Object(_) => vec![edits],
        _ => bail!("`edits` must be an array of edit objects"),
    };
    if edits.is_empty() {
        bail!("`edits` is empty; nothing to do");
    }
    let mut ops = Vec::new();
    let mut stripped_any = false;
    for (idx, e) in edits.iter().enumerate() {
        if !e.is_object() {
            bail!("edits[{idx}] must be an object");
        }
        let op = util::opt_str(e, "op")?.ok_or_else(|| anyhow!("edits[{idx}]: missing `op`"))?;
        let op = op.trim().to_ascii_lowercase().replace('-', "_");
        let num = |k: &str| util::value_u64(e.get(k), &format!("edits[{idx}].{k}")).map(|v| v.map(|n| n as usize));
        let lines = parse_lines(e.get("lines"), idx)?;
        let kind = match op.as_str() {
            "replace" | "delete" => {
                let start = num("start")?
                    .or(num("line")?)
                    .ok_or_else(|| anyhow!("edits[{idx}]: `{op}` requires `start`"))?;
                let end = num("end")?.unwrap_or(start);
                if start == 0 {
                    bail!("edits[{idx}]: line numbers start at 1");
                }
                if end < start {
                    bail!("edits[{idx}]: `end` ({end}) is before `start` ({start})");
                }
                Kind::Replace { start, end }
            }
            "insert_before" | "insert_after" => {
                let line = num("line")?
                    .or(num("start")?)
                    .ok_or_else(|| anyhow!("edits[{idx}]: `{op}` requires `line`"))?;
                if op == "insert_before" {
                    if line == 0 {
                        bail!("edits[{idx}]: insert_before needs line >= 1 (use insert_after with line 0 for the top of the file)");
                    }
                    Kind::Insert { gap: line - 1, before: true, anchor: line }
                } else {
                    Kind::Insert { gap: line, before: false, anchor: line }
                }
            }
            other => bail!(
                "edits[{idx}]: unknown op \"{other}\" (expected replace, delete, insert_before or insert_after)"
            ),
        };
        let mut lines = match (&kind, op.as_str(), lines) {
            (_, "delete", _) => Vec::new(),
            (_, _, Some(l)) => l,
            (Kind::Replace { .. }, _, None) => {
                bail!("edits[{idx}]: replace requires `lines` (use op \"delete\" to remove lines)")
            }
            (_, _, None) => bail!("edits[{idx}]: `{op}` requires `lines`"),
        };
        let replace_start = match kind {
            Kind::Replace { start, .. } => Some(start),
            _ => None,
        };
        if !lines.is_empty() && strip_pasted_prefixes(&mut lines, replace_start) {
            stripped_any = true;
        }
        if matches!(kind, Kind::Insert { .. }) && lines.is_empty() {
            bail!("edits[{idx}]: `{op}` with empty `lines` does nothing");
        }
        ops.push(Op { idx, kind, lines });
    }
    if stripped_any {
        warnings.push("Stripped `N:` display prefixes from `lines` (they looked like pasted read output). `lines` must contain raw file content only.".to_string());
    }
    Ok(ops)
}

/// Bounds and overlap validation against a file with `total` lines.
fn validate(ops: &[Op], total: usize) -> Result<(), String> {
    for op in ops {
        let i = op.idx;
        match op.kind {
            Kind::Replace { start, end } => {
                if total == 0 {
                    return Err(format!("edits[{i}]: the file is empty; use insert_after with line 0."));
                }
                if end > total {
                    return Err(format!("edits[{i}]: line {end} does not exist (file has {total} lines)."));
                }
                let _ = start;
            }
            Kind::Insert { gap, anchor, before } => {
                let ok = if before { anchor <= total + 1 } else { gap <= total };
                if !ok {
                    return Err(format!("edits[{i}]: line {anchor} does not exist (file has {total} lines)."));
                }
            }
        }
    }
    let ranges: Vec<(&Op, usize, usize)> = ops
        .iter()
        .filter_map(|o| match o.kind {
            Kind::Replace { start, end } => Some((o, start, end)),
            _ => None,
        })
        .collect();
    for (a, (oa, sa, ea)) in ranges.iter().enumerate() {
        for (ob, sb, eb) in ranges.iter().skip(a + 1) {
            if sa <= eb && sb <= ea {
                return Err(format!(
                    "edits[{}] ({}) and edits[{}] ({}) overlap. Ranges in one call must not overlap; use a single replace covering the whole region. Line numbers always refer to the tagged version, never to the result of other edits in the same call.",
                    oa.idx,
                    oa.describe(),
                    ob.idx,
                    ob.describe()
                ));
            }
        }
    }
    for o in ops {
        if let Kind::Insert { gap, .. } = o.kind {
            for (or, s, e) in &ranges {
                // Gap g lies strictly inside [s, e] when s <= g < e.
                if *s <= gap && gap < *e {
                    return Err(format!(
                        "edits[{}] ({}) points inside the range of edits[{}] ({}). Put the new lines into that replace instead.",
                        o.idx,
                        o.describe(),
                        or.idx,
                        or.describe()
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Applies validated ops simultaneously to `lines` (1-based addressing).
fn apply(lines: &[&str], ops: &[Op]) -> Vec<String> {
    let total = lines.len();
    let mut gaps: Vec<Vec<&str>> = vec![Vec::new(); total + 1];
    let mut replace_at: Vec<Option<&Op>> = vec![None; total + 1];
    let mut deleted = vec![false; total + 1];
    for op in ops {
        match op.kind {
            Kind::Insert { gap, .. } => gaps[gap].extend(op.lines.iter().map(String::as_str)),
            Kind::Replace { start, end } => {
                replace_at[start] = Some(op);
                for d in deleted.iter_mut().take(end + 1).skip(start) {
                    *d = true;
                }
            }
        }
    }
    let mut out: Vec<String> = Vec::with_capacity(total + 16);
    out.extend(gaps[0].iter().map(|s| s.to_string()));
    for n in 1..=total {
        if let Some(op) = replace_at[n] {
            out.extend(op.lines.iter().cloned());
        }
        if !deleted[n] {
            out.push(lines[n - 1].to_string());
        }
        out.extend(gaps[n].iter().map(|s| s.to_string()));
    }
    out
}

// ---------------------------------------------------------------------------
// Line mapping through diffs
// ---------------------------------------------------------------------------

/// Maps 1-based lines of `old` to 1-based lines of `new` through unchanged runs.
struct LineMap {
    map: Vec<Option<usize>>, // index = old line (1-based); [0] unused
    new_total: usize,
}

impl LineMap {
    fn build(old: &[&str], new: &[&str]) -> LineMap {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let ops = similar::capture_diff_slices_deadline(similar::Algorithm::Myers, old, new, Some(deadline));
        let mut map = vec![None; old.len() + 1];
        for op in ops {
            if let similar::DiffOp::Equal { old_index, new_index, len } = op {
                for k in 0..len {
                    map[old_index + k + 1] = Some(new_index + k + 1);
                }
            }
        }
        LineMap { map, new_total: new.len() }
    }

    fn get(&self, line: usize) -> Option<usize> {
        self.map.get(line).copied().flatten()
    }

    /// Remaps one op; `None` if any anchor does not map cleanly.
    fn remap_op(&self, op: &Op, old_total: usize) -> Option<Op> {
        let kind = match op.kind {
            Kind::Replace { start, end } => {
                let ns = self.get(start)?;
                for (k, l) in (start..=end).enumerate() {
                    if self.get(l)? != ns + k {
                        return None;
                    }
                }
                Kind::Replace { start: ns, end: ns + (end - start) }
            }
            Kind::Insert { before, anchor, .. } => {
                let na = if anchor == 0 {
                    0
                } else if anchor > old_total {
                    self.new_total + 1
                } else {
                    self.get(anchor)?
                };
                if before {
                    Kind::Insert { gap: na - 1, before, anchor: na }
                } else {
                    Kind::Insert { gap: na, before, anchor: na }
                }
            }
        };
        Some(Op { idx: op.idx, kind, lines: op.lines.clone() })
    }

    fn map_seen(&self, seen: &Seen) -> Seen {
        match seen {
            Seen::All => Seen::All,
            Seen::Lines(s) => Seen::Lines(s.iter().filter_map(|l| self.get(l as usize)).map(|l| l as u32).collect()),
        }
    }
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

/// `*N:text` for anchors, ` N:text` for neighbours, `...` between runs.
fn anchored_context(lines: &[&str], anchors: &BTreeSet<usize>) -> (String, Vec<u32>) {
    let total = lines.len();
    let mut show = BTreeSet::new();
    for &a in anchors {
        if a == 0 || a > total {
            continue;
        }
        for l in a.saturating_sub(MISMATCH_CONTEXT).max(1)..=(a + MISMATCH_CONTEXT).min(total) {
            show.insert(l);
        }
    }
    let mut out = String::new();
    let mut seen = Vec::new();
    let mut prev = 0;
    for l in show {
        if prev != 0 && l != prev + 1 {
            out.push_str("...\n");
        }
        let marker = if anchors.contains(&l) { '*' } else { ' ' };
        let (text, cut) = util::truncate_chars(lines[l - 1], ROW_MAX_CHARS);
        let _ = writeln!(out, "{marker}{l}:{text}");
        if !cut {
            seen.push(l as u32);
        }
        prev = l;
    }
    if out.ends_with('\n') {
        out.pop();
    }
    (out, seen)
}

fn all_anchors(ops: &[Op], total: usize) -> BTreeSet<usize> {
    let mut s = BTreeSet::new();
    for op in ops {
        match op.kind {
            Kind::Replace { start, end } => {
                s.insert(start);
                s.insert(end);
            }
            Kind::Insert { anchor, .. } => {
                s.insert(anchor.clamp(1, total.max(1)));
            }
        }
    }
    s
}

/// Seen-line guard: rejects ops touching lines never displayed from `snap`.
fn check_seen(ctx: &ToolCtx, abs: &Path, display: &str, snap: &Snapshot, ops: &[Op]) -> Option<ToolOutput> {
    let (lines, _) = util::split_lines(&snap.text);
    let mut unseen = BTreeSet::new();
    for op in ops {
        for l in op.anchor_lines(lines.len()) {
            if !snap.seen.contains(l as u32) {
                unseen.insert(l);
            }
        }
    }
    if unseen.is_empty() {
        return None;
    }
    let list: Vec<u32> = unseen.iter().map(|&l| l as u32).collect();
    let mut msg = format!(
        "Edit rejected: it touches lines {} of {display} that [{display}#{}] never displayed in full (you saw a partial range, a search hit or a cut line). Actual content at those lines:\n",
        util::format_line_list(&list),
        snap.tag
    );
    let mut revealed = Vec::new();
    let mut over_cap = false;
    for (i, &l) in unseen.iter().enumerate() {
        if i >= REVEAL_MAX_LINES {
            over_cap = true;
            break;
        }
        let (text, cut) = util::truncate_chars(lines[l - 1], REVEAL_MAX_CHARS);
        let _ = writeln!(msg, "{l}:{text}");
        if !cut {
            revealed.push(l as u32);
        }
    }
    if over_cap {
        let rest: Vec<u32> = unseen.iter().skip(REVEAL_MAX_LINES).map(|&l| l as u32).collect();
        let _ = writeln!(msg, "…\n(The remaining lines {} were not shown; read them first.)", util::format_line_list(&rest));
    }
    msg.push_str(
        "Verify this is the content you intend to change, then re-issue the same edit with the same tag (a straight retry now succeeds). If it is not, fix your line numbers.",
    );
    ctx.store.mark_seen(abs, &snap.tag, revealed);
    Some(ToolOutput::err(msg))
}

// ---------------------------------------------------------------------------
// Preview
// ---------------------------------------------------------------------------

/// Compact view of the new file around changes: `N:text` rows, `…` between runs.
/// Returns (preview, displayed full rows).
fn build_preview(old: &[&str], new: &[String]) -> (String, Vec<u32>) {
    let new_refs: Vec<&str> = new.iter().map(String::as_str).collect();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let ops = similar::capture_diff_slices_deadline(similar::Algorithm::Myers, old, &new_refs, Some(deadline));
    // Changed regions in new-file coordinates, 0-based half-open.
    let mut regions: Vec<(usize, usize)> = Vec::new();
    for op in ops {
        match op {
            similar::DiffOp::Equal { .. } => {}
            similar::DiffOp::Delete { new_index, .. } => regions.push((new_index, new_index)),
            similar::DiffOp::Insert { new_index, new_len, .. } | similar::DiffOp::Replace { new_index, new_len, .. } => {
                regions.push((new_index, new_index + new_len))
            }
        }
    }
    let total = new.len();
    // Long added runs collapse to first 3 + `…` + last 2 (the gap produces the `…` row).
    let mut show: BTreeSet<usize> = BTreeSet::new();
    for &(a, b) in &regions {
        for l in a.saturating_sub(PREVIEW_CONTEXT)..a {
            show.insert(l);
        }
        for l in b..(b + PREVIEW_CONTEXT).min(total) {
            show.insert(l);
        }
        let len = b - a;
        if len > 5 {
            for l in a..a + 3 {
                show.insert(l);
            }
            for l in b - 2..b {
                show.insert(l);
            }
        } else {
            for l in a..b {
                show.insert(l);
            }
        }
    }
    let mut out = String::new();
    let mut seen = Vec::new();
    let mut prev: Option<usize> = None;
    let mut rows = 0;
    for l in show {
        if l >= total {
            continue;
        }
        if rows >= PREVIEW_MAX_ROWS {
            out.push_str("…\n[preview truncated; read the file to see the rest]\n");
            break;
        }
        if let Some(p) = prev
            && l != p + 1 {
                out.push_str("…\n");
            }
        let (text, cut) = util::truncate_chars(&new[l], ROW_MAX_CHARS);
        let _ = writeln!(out, "{}:{text}", l + 1);
        if !cut {
            seen.push((l + 1) as u32);
        }
        prev = Some(l);
        rows += 1;
    }
    if out.ends_with('\n') {
        out.pop();
    }
    if out.is_empty() {
        out = if total == 0 { "(file is now empty)".to_string() } else { "(no visible change)".to_string() };
    }
    (out, seen)
}

/// Lines of `new` that are new content (inserted/replaced), 1-based.
fn added_lines(old: &[&str], new: &[String]) -> Vec<u32> {
    let new_refs: Vec<&str> = new.iter().map(String::as_str).collect();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let mut out = Vec::new();
    for op in similar::capture_diff_slices_deadline(similar::Algorithm::Myers, old, &new_refs, Some(deadline)) {
        if let similar::DiffOp::Insert { new_index, new_len, .. } | similar::DiffOp::Replace { new_index, new_len, .. } = op {
            out.extend((new_index..new_index + new_len).map(|l| (l + 1) as u32));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Main flow
// ---------------------------------------------------------------------------

fn run_sync(input: &Value, ctx: &ToolCtx) -> Result<ToolOutput> {
    let raw_path = util::req_str(input, "path")?;
    let (raw_path, header_tag) = util::unwrap_header(&raw_path);
    if raw_path.is_empty() {
        bail!("`path` must not be empty");
    }
    let tag_raw = util::opt_str(input, "tag")?.or(header_tag.clone());
    let mut warnings = Vec::new();
    let ops = parse_edits(input, &mut warnings)?;

    let abs = util::resolve_path(&ctx.cwd, &raw_path);
    let display = util::display_path(&ctx.cwd, &abs);
    let meta = match std::fs::metadata(&abs) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ToolOutput::err(format!(
                "File not found: {display}. Use the `write` tool to create new files."
            )));
        }
        Err(e) => bail!("cannot access {display}: {e}"),
    };
    if meta.is_dir() {
        return Ok(ToolOutput::err(format!("{display} is a directory, not a file.")));
    }
    let bytes = std::fs::read(&abs)?;
    if util::looks_binary(&bytes) {
        return Ok(ToolOutput::err(format!("Cannot edit binary file {display}.")));
    }
    let raw = String::from_utf8(bytes).map_err(|_| anyhow!("cannot edit {display}: not valid UTF-8 text"))?;
    let file = TextFile::decode(&raw);
    let current = file.text.as_str();
    let current_tag = hashline::file_hash(current);
    let (cur_lines, trailing) = util::split_lines(current);

    let Some(tag) = tag_raw.as_deref().and_then(hashline::normalize_tag) else {
        let got = tag_raw.map(|t| format!(" (got {t:?})")).unwrap_or_default();
        ctx.store.record(&abs, current, Seen::lines([]));
        return Ok(ToolOutput::err(format!(
            "Edit rejected for {display}: `tag` must be the 4-hex TAG from the file's [path#TAG] header{got}. Read the file with `read` first and copy the tag from its header; never invent it."
        )));
    };

    // Resolve the ops against the current file.
    let snapshot = ctx.store.get(&abs, &tag);
    let (ops, base_seen): (Vec<Op>, Seen) = if tag == current_tag {
        if let Err(msg) = validate(&ops, cur_lines.len()) {
            return Ok(ToolOutput::err(format!("Edit rejected for {display}: {msg}")));
        }
        match &snapshot {
            Some(snap) => {
                if let Some(rejection) = check_seen(ctx, &abs, &display, snap, &ops) {
                    return Ok(rejection);
                }
                (ops, snap.seen.clone())
            }
            // Tag matches the live content but was not issued in this session (e.g. after
            // a restart): the content is what the model saw, so allow it.
            None => (ops, Seen::All),
        }
    } else {
        match &snapshot {
            Some(snap) => match try_remap(&snap.text, &cur_lines, &ops, &snap.seen) {
                Some((remapped, seen)) => {
                    warnings.push("Recovered: file changed since read; line numbers remapped.".to_string());
                    (remapped, seen)
                }
                None => {
                    return Ok(mismatch(ctx, &abs, &display, &tag, &current_tag, current, &ops, Some(snap)));
                }
            },
            None => return Ok(mismatch(ctx, &abs, &display, &tag, &current_tag, current, &ops, None)),
        }
    };

    let new_lines = apply(&cur_lines, &ops);
    let new_trailing = if cur_lines.is_empty() { !new_lines.is_empty() } else { trailing };
    let new_text = util::join_lines(&new_lines, new_trailing);
    if new_text == current {
        ctx.store.record(&abs, current, base_seen);
        return Ok(ToolOutput::err(format!(
            "Edits to {display} applied cleanly but produced no change: the new lines are identical to the file at the targeted lines. Re-read the file and verify the line numbers before issuing another edit."
        )));
    }
    std::fs::write(&abs, file.encode(&new_text)).map_err(|e| anyhow!("failed to write {display}: {e}"))?;

    // New snapshot: previously seen lines carried through the diff + everything displayed or authored now.
    let map = LineMap::build(&cur_lines, &new_lines.iter().map(String::as_str).collect::<Vec<_>>());
    let (preview, preview_seen) = build_preview(&cur_lines, &new_lines);
    let seen = match map.map_seen(&base_seen) {
        Seen::All => Seen::All,
        Seen::Lines(mut s) => {
            let extra: LineSet = preview_seen.into_iter().chain(added_lines(&cur_lines, &new_lines)).collect();
            s.union_with(&extra);
            Seen::Lines(s)
        }
    };
    let new_tag = ctx.store.record(&abs, &new_text, seen);

    let (diff, added, removed) = util::unified_diff(current, &new_text, &display);
    let mut text = format!("{}\n{preview}", format_header(&display, &new_tag));
    if !warnings.is_empty() {
        text.push_str("\n\nWarnings:\n");
        text.push_str(&warnings.join("\n"));
    }
    let mut out = ToolOutput::ok(text);
    out.meta = Some(json!({
        "path": abs.to_string_lossy(),
        "diff": diff,
        "added": added,
        "removed": removed,
    }));
    Ok(out)
}

/// Remaps ops from snapshot text onto the current lines. `None` if any anchor is ambiguous,
/// changed, unseen or out of range.
fn try_remap(snap_text: &str, cur_lines: &[&str], ops: &[Op], seen: &Seen) -> Option<(Vec<Op>, Seen)> {
    let (old_lines, _) = util::split_lines(snap_text);
    validate(ops, old_lines.len()).ok()?;
    for op in ops {
        if op.anchor_lines(old_lines.len()).iter().any(|&l| !seen.contains(l as u32)) {
            return None;
        }
    }
    let map = LineMap::build(&old_lines, cur_lines);
    let remapped: Vec<Op> = ops.iter().map(|o| map.remap_op(o, old_lines.len())).collect::<Option<_>>()?;
    validate(&remapped, cur_lines.len()).ok()?;
    // Inserts must still sit next to the same neighbour lines (avoid landing in a rewritten region).
    for (o, r) in ops.iter().zip(&remapped) {
        if let (Kind::Insert { gap: og, .. }, Kind::Insert { gap: ng, .. }) = (&o.kind, &r.kind) {
            // Head/tail inserts are content-independent.
            if *og == 0 || *og == old_lines.len() {
                continue;
            }
            if *og >= 1 && *og <= old_lines.len() && map.get(*og) != Some(*ng) {
                return None;
            }
            if *og < old_lines.len() && map.get(og + 1) != Some(ng + 1) {
                return None;
            }
        }
    }
    Some((remapped, map.map_seen(seen)))
}

#[allow(clippy::too_many_arguments)]
fn mismatch(
    ctx: &ToolCtx,
    abs: &Path,
    display: &str,
    tag: &str,
    current_tag: &str,
    current: &str,
    ops: &[Op],
    snap: Option<&Snapshot>,
) -> ToolOutput {
    let (cur_lines, _) = util::split_lines(current);
    // Anchors in current coordinates: remapped where possible, else the raw numbers.
    let map = snap.map(|s| {
        let (old, _) = util::split_lines(&s.text);
        LineMap::build(&old, &cur_lines)
    });
    let raw_anchors = all_anchors(ops, cur_lines.len());
    let anchors: BTreeSet<usize> = raw_anchors
        .iter()
        .map(|&a| map.as_ref().and_then(|m| m.get(a)).unwrap_or(a))
        .filter(|&a| a >= 1 && a <= cur_lines.len())
        .collect();
    let (context, shown) = anchored_context(&cur_lines, &anchors);
    let mut seen = match (&map, snap) {
        (Some(m), Some(s)) => match m.map_seen(&s.seen) {
            Seen::All => LineSet::new(),
            Seen::Lines(l) => l,
        },
        _ => LineSet::new(),
    };
    seen.union_with(&shown.iter().copied().collect());
    ctx.store.record(abs, current, Seen::Lines(seen));

    let mut msg = if snap.is_some() {
        format!(
            "Edit rejected for {display}: file changed since it was read (tag #{tag}, current #{current_tag}).\n\
The changes touch the lines you are editing, so line numbers could not be remapped safely."
        )
    } else {
        let mut m = format!(
            "Edit rejected for {display}: tag #{tag} was not issued for this file in this session; the current file hashes to #{current_tag}."
        );
        let others: Vec<String> = ctx
            .store
            .paths_for_tag(tag)
            .into_iter()
            .filter(|p| p.as_path() != util::store_key(abs))
            .map(|p| util::display_path(&ctx.cwd, &p))
            .collect();
        if !others.is_empty() {
            let _ = write!(m, "\nTag #{tag} belongs to: {}.", others.join(", "));
        }
        m.push_str("\nRead the file with `read` first and copy the tag from its [path#TAG] header; never invent a tag.");
        m
    };
    if !context.is_empty() {
        let _ = write!(msg, "\n\nCurrent content around your anchors ([{display}#{current_tag}]):\n{context}");
    }
    let _ = write!(
        msg,
        "\n\nIf a previous edit in this session changed the file, use the [path#TAG] header from that edit's output. Otherwise re-read the file (or use the current lines above with tag {current_tag}) and retry."
    );
    ToolOutput::err(msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hashline::file_hash;
    use crate::util::test_support::{ctx, temp_dir};

    fn setup(content: &str) -> (crate::util::test_support::TempDir, ToolCtx) {
        let d = temp_dir("edit");
        std::fs::write(d.join("f.txt"), content).unwrap();
        let c = ctx(&d);
        (d, c)
    }

    async fn read_tag(c: &ToolCtx, path: &str) -> String {
        let o = crate::execute("read", json!({"path": path}), c).await;
        let header = o.text.lines().next().unwrap().to_string();
        header.rsplit_once('#').unwrap().1.trim_end_matches(']').to_string()
    }

    async fn edit(c: &ToolCtx, tag: &str, edits: Value) -> ToolOutput {
        crate::execute("edit", json!({"path": "f.txt", "tag": tag, "edits": edits}), c).await
    }

    fn file(d: &Path) -> String {
        std::fs::read_to_string(d.join("f.txt")).unwrap()
    }

    #[tokio::test]
    async fn multi_edit_simultaneous() {
        let (d, c) = setup("a\nb\nc\nd\ne\n");
        let tag = read_tag(&c, "f.txt").await;
        let o = edit(
            &c,
            &tag,
            json!([
                {"op": "replace", "start": 2, "lines": ["B1", "B2"]},
                {"op": "delete", "start": 4},
                {"op": "insert_after", "line": 0, "lines": ["top"]},
                {"op": "insert_before", "line": 5, "lines": ["before-e"]},
                {"op": "insert_after", "line": 5, "lines": ["end"]}
            ]),
        )
        .await;
        assert!(!o.is_error, "{}", o.text);
        let expected = "top\na\nB1\nB2\nc\nbefore-e\ne\nend\n";
        assert_eq!(file(&d), expected);
        let new_tag = file_hash(expected);
        assert!(o.text.starts_with(&format!("[f.txt#{new_tag}]\n")), "{}", o.text);
        assert!(o.text.contains("3:B1\n4:B2"));
        let m = o.meta.unwrap();
        assert_eq!(m["added"], json!(5));
        assert_eq!(m["removed"], json!(2));
        assert!(m["diff"].as_str().unwrap().contains("+B1"));

        // Chain: new tag works immediately, old tag is stale.
        let o = edit(&c, &new_tag, json!([{"op": "replace", "start": 1, "lines": ["TOP"]}])).await;
        assert!(!o.is_error, "{}", o.text);
        assert!(file(&d).starts_with("TOP\na\n"));
    }

    #[tokio::test]
    async fn overlap_and_bounds_errors() {
        let (d, c) = setup("a\nb\nc\nd\n");
        let tag = read_tag(&c, "f.txt").await;
        let o = edit(&c, &tag, json!([
            {"op": "replace", "start": 1, "end": 3, "lines": ["x"]},
            {"op": "delete", "start": 3, "end": 4}
        ])).await;
        assert!(o.is_error && o.text.contains("overlap"), "{}", o.text);
        let o = edit(&c, &tag, json!([
            {"op": "replace", "start": 1, "end": 3, "lines": ["x"]},
            {"op": "insert_after", "line": 2, "lines": ["y"]}
        ])).await;
        assert!(o.is_error && o.text.contains("inside the range"), "{}", o.text);
        let o = edit(&c, &tag, json!([{"op": "replace", "start": 9, "lines": ["x"]}])).await;
        assert!(o.is_error && o.text.contains("does not exist (file has 4 lines)"), "{}", o.text);
        let o = edit(&c, &tag, json!([{"op": "bogus", "start": 1}])).await;
        assert!(o.is_error && o.text.contains("unknown op"));
        // Adjacent ranges and inserts at range boundaries are fine.
        let o = edit(&c, &tag, json!([
            {"op": "replace", "start": 1, "lines": ["A"]},
            {"op": "replace", "start": 2, "lines": ["B"]},
            {"op": "insert_after", "line": 2, "lines": ["mid"]},
            {"op": "insert_before", "line": 1, "lines": ["first"]}
        ])).await;
        assert!(!o.is_error, "{}", o.text);
        assert_eq!(file(&d), "first\nA\nB\nmid\nc\nd\n");
    }

    #[tokio::test]
    async fn stale_tag_recovered_by_remap() {
        let (d, c) = setup("one\ntwo\nthree\nfour\n");
        let tag = read_tag(&c, "f.txt").await;
        // External change above the edit target.
        std::fs::write(d.join("f.txt"), "zero\none\ntwo\nthree\nfour\n").unwrap();
        let o = edit(&c, &tag, json!([{"op": "replace", "start": 3, "lines": ["THREE"]}])).await;
        assert!(!o.is_error, "{}", o.text);
        assert!(o.text.contains("Recovered: file changed since read; line numbers remapped."));
        assert_eq!(file(&d), "zero\none\ntwo\nTHREE\nfour\n");
    }

    #[tokio::test]
    async fn stale_tag_conflict_error() {
        let (d, c) = setup("one\ntwo\nthree\nfour\nfive\nsix\n");
        let tag = read_tag(&c, "f.txt").await;
        std::fs::write(d.join("f.txt"), "one\ntwo\nTHREE!\nfour\nfive\nsix\n").unwrap();
        let o = edit(&c, &tag, json!([{"op": "replace", "start": 3, "lines": ["3"]}])).await;
        assert!(o.is_error);
        let cur = file_hash("one\ntwo\nTHREE!\nfour\nfive\nsix\n");
        assert!(o.text.starts_with(&format!(
            "Edit rejected for f.txt: file changed since it was read (tag #{tag}, current #{cur})."
        )), "{}", o.text);
        assert!(o.text.contains(" 1:one\n 2:two\n*3:THREE!\n 4:four\n 5:five"), "{}", o.text);
        // Retry with the current tag and shown lines works.
        let o = edit(&c, &cur, json!([{"op": "replace", "start": 3, "lines": ["3"]}])).await;
        assert!(!o.is_error, "{}", o.text);
        assert_eq!(file(&d), "one\ntwo\n3\nfour\nfive\nsix\n");
    }

    #[tokio::test]
    async fn unknown_tag_and_missing_file() {
        let (_d, c) = setup("x\n");
        let o = edit(&c, "0000", json!([{"op": "replace", "start": 1, "lines": ["y"]}])).await;
        assert!(o.is_error && o.text.contains("was not issued"), "{}", o.text);
        assert!(o.text.contains("*1:x"));
        let o = edit(&c, "zz", json!([{"op": "replace", "start": 1, "lines": ["y"]}])).await;
        assert!(o.is_error && o.text.contains("4-hex TAG"));
        let o = crate::execute("edit", json!({"path": "nope.txt", "tag": "0000", "edits": [{"op":"delete","start":1}]}), &c).await;
        assert!(o.is_error && o.text.contains("Use the `write` tool"));
    }

    #[tokio::test]
    async fn unseen_lines_rejected_then_retry() {
        let content: String = (1..=20).map(|i| format!("l{i}\n")).collect();
        let (d, c) = setup(&content);
        let o = crate::execute("read", json!({"path": "f.txt", "offset": 1, "limit": 5}), &c).await;
        let tag = file_hash(&content);
        assert!(o.text.starts_with(&format!("[f.txt#{tag}]")));
        let o = edit(&c, &tag, json!([{"op": "replace", "start": 10, "end": 11, "lines": ["x"]}])).await;
        assert!(o.is_error && o.text.contains("never displayed"), "{}", o.text);
        assert!(o.text.contains("10:l10\n11:l11"));
        let o = edit(&c, &tag, json!([{"op": "replace", "start": 10, "end": 11, "lines": ["x"]}])).await;
        assert!(!o.is_error, "{}", o.text);
        assert!(file(&d).contains("l9\nx\nl12\n"));
    }

    #[tokio::test]
    async fn crlf_bom_and_no_trailing_newline_preserved() {
        let (d, c) = setup("\u{FEFF}a\r\nb\r\nc");
        let tag = read_tag(&c, "f.txt").await;
        assert_eq!(tag, file_hash("a\nb\nc"));
        let o = edit(&c, &tag, json!([{"op": "replace", "start": 2, "lines": ["B", "B2"]}])).await;
        assert!(!o.is_error, "{}", o.text);
        assert_eq!(file(&d), "\u{FEFF}a\r\nB\r\nB2\r\nc");
    }

    #[tokio::test]
    async fn write_then_edit_and_prefix_stripping() {
        let d = temp_dir("edit-write");
        let c = ctx(&d);
        let o = crate::execute("write", json!({"path": "f.txt", "content": "fn a() {\n    1\n}\n"}), &c).await;
        let tag = o.text.lines().next().unwrap().rsplit_once('#').unwrap().1.trim_end_matches(']').to_string();
        let o = edit(&c, &tag, json!([{"op": "replace", "start": 2, "end": 3, "lines": ["2:    2", "3:}"]}])).await;
        assert!(!o.is_error, "{}", o.text);
        assert!(o.text.contains("Stripped `N:` display prefixes"));
        assert_eq!(file(&d), "fn a() {\n    2\n}\n");
        // A lone `N:`-looking line that is real content is kept.
        let tag = file_hash(&file(&d));
        let o = edit(&c, &tag, json!([{"op": "insert_after", "line": 3, "lines": ["10:30 meeting"]}])).await;
        assert!(!o.is_error);
        assert!(file(&d).ends_with("}\n10:30 meeting\n"));
    }

    #[tokio::test]
    async fn empty_file_and_noop() {
        let (d, c) = setup("");
        let tag = read_tag(&c, "f.txt").await;
        assert_eq!(tag, "5D05");
        let o = edit(&c, &tag, json!([{"op": "insert_after", "line": 0, "lines": ["hello"]}])).await;
        assert!(!o.is_error, "{}", o.text);
        assert_eq!(file(&d), "hello\n");
        let tag = file_hash("hello\n");
        let o = edit(&c, &tag, json!([{"op": "replace", "start": 1, "lines": ["hello"]}])).await;
        assert!(o.is_error && o.text.contains("no change"));
    }

    #[test]
    fn preview_collapses_long_runs() {
        let old = vec!["a", "b"];
        let new: Vec<String> = ["a"].iter().map(|s| s.to_string())
            .chain((1..=10).map(|i| format!("n{i}")))
            .chain(["b".to_string()])
            .collect();
        let (p, _) = build_preview(&old, &new);
        assert_eq!(p, "1:a\n2:n1\n3:n2\n4:n3\n…\n10:n9\n11:n10\n12:b");
    }
}
