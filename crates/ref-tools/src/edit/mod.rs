//! `edit` tool: hashline patches (and a str_replace fallback).

mod apply;
mod parse;
mod replace;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde_json::Value;

use self::apply::{anchored_lines, check_bounds, line_map, materialize, normalize_overlaps, preview, rebase};
use self::parse::{Hunk, HunkKind};
use crate::hash::file_hash;
use crate::read::{load_text, not_found, MAX_LINE_CHARS};
use crate::store::Seen;
use crate::util::{
    cap_line, canonical_key, display_path, format_line_list, join_lines, resolve_path, split_lines,
    TextFile,
};
use crate::{ToolCtx, ToolOutput};

const MISMATCH_CONTEXT: usize = 2;
const SEEN_LINE_REVEAL_CAP: usize = 40;

pub fn run(input: &Value, ctx: &ToolCtx) -> ToolOutput {
    match input.get("input") {
        Some(Value::String(patch)) => match run_patch(patch, ctx) {
            Ok(o) => o,
            Err(e) => ToolOutput::err(e),
        },
        Some(Value::Null) | None => {
            if input.get("old_string").is_some() || input.get("new_string").is_some() {
                match replace::run(input, ctx) {
                    Ok(o) => o,
                    Err(e) => ToolOutput::err(e),
                }
            } else {
                ToolOutput::err("missing required parameter `input`")
            }
        }
        Some(_) => ToolOutput::err("parameter `input` must be a string"),
    }
}

/// A file section after validation, ready to be written.
struct Planned {
    path: PathBuf,
    key: PathBuf,
    shown: String,
    file: TextFile,
    old_text: String,
    /// None = delete the file.
    new_text: Option<String>,
    warnings: Vec<String>,
}

fn run_patch(patch: &str, ctx: &ToolCtx) -> Result<ToolOutput, String> {
    let parsed = parse::parse(patch)?;
    let mut warnings = parsed.warnings;

    // Merge same-file sections (by canonical path) preserving order.
    let mut order: Vec<PathBuf> = vec![];
    let mut merged: BTreeMap<PathBuf, (String, Option<String>, Vec<Hunk>, PathBuf)> = BTreeMap::new();
    for s in parsed.sections {
        let path = resolve_path(&ctx.cwd, &s.path);
        let key = canonical_key(&path);
        match merged.get_mut(&key) {
            Some((name, tag, hunks, _)) => {
                if tag != &s.tag {
                    return Err(format!(
                        "Two sections for {name} carry different tags (#{} and #{}); use one section per file with its latest tag.",
                        tag.as_deref().unwrap_or("none"),
                        s.tag.as_deref().unwrap_or("none")
                    ));
                }
                hunks.extend(s.hunks);
            }
            None => {
                order.push(key.clone());
                merged.insert(key, (s.path, s.tag, s.hunks, path));
            }
        }
    }

    let mut plans = vec![];
    for key in order {
        let Some((_, tag, hunks, path)) = merged.remove(&key) else { continue };
        plans.push(plan_section(ctx, path, key, tag, hunks)?);
    }

    // Everything validated in memory; now write.
    let mut out = String::new();
    let mut all_noop = true;
    for (i, p) in plans.iter().enumerate() {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        let written_note = || {
            let done: Vec<&str> = plans[..i].iter().map(|p| p.shown.as_str()).collect();
            if done.is_empty() {
                String::new()
            } else {
                format!(" (already written: {})", done.join(", "))
            }
        };
        match &p.new_text {
            None => {
                std::fs::remove_file(&p.path)
                    .map_err(|e| format!("Failed to delete {}: {e}{}", p.shown, written_note()))?;
                ctx.store.lock().remove(&p.key);
                out.push_str(&format!("Deleted {}", p.shown));
                all_noop = false;
            }
            Some(new_text) if *new_text == p.old_text => {
                out.push_str(&format!(
                    "Edits to {} parsed and applied cleanly, but produced no change: your body row(s) are byte-identical to the file at the targeted lines. The bug is somewhere else — re-read the file before issuing another edit. Do NOT widen the payload or add lines; verify the anchor first.",
                    p.shown
                ));
            }
            Some(new_text) => {
                std::fs::write(&p.path, p.file.encode(new_text))
                    .map_err(|e| format!("Failed to write {}: {e}{}", p.shown, written_note()))?;
                out.push_str(&finish_file(ctx, &p.key, &p.shown, &p.old_text, new_text));
                all_noop = false;
            }
        }
        warnings.extend(p.warnings.iter().cloned());
    }
    if !warnings.is_empty() {
        out.push_str("\n\nWarnings:\n");
        out.push_str(&warnings.join("\n"));
    }
    Ok(if all_noop { ToolOutput::err(out) } else { ToolOutput::ok(out) })
}

/// Record the new snapshot (carrying over seen lines) and render `[path#TAG]` + preview.
pub(crate) fn finish_file(ctx: &ToolCtx, key: &std::path::Path, shown: &str, old_text: &str, new_text: &str) -> String {
    let old_lines = split_lines(old_text);
    let new_lines = split_lines(new_text);
    let (pv, shown_rows) = preview(&old_lines, &new_lines);
    let mut store = ctx.store.lock();
    let old_hash = file_hash(old_text);
    let mut seen: BTreeSet<usize> = shown_rows.into_iter().collect();
    let all_seen = match store.by_hash(key, &old_hash).map(|s| s.seen.clone()) {
        Some(Seen::All) => true,
        Some(Seen::Lines(prev)) => {
            let map = line_map(&old_lines, &new_lines);
            seen.extend(prev.iter().filter_map(|l| map.get(l).copied()));
            false
        }
        None => false,
    };
    // Lines inserted by the model are known to it even if collapsed in the preview only when
    // the whole file was writable before; otherwise only shown rows count.
    let seen = if all_seen { Seen::All } else { Seen::Lines(seen) };
    let tag = store.record(key, new_text, seen);
    if pv.is_empty() {
        format!("[{shown}#{tag}]\n(file is now empty)")
    } else {
        format!("[{shown}#{tag}]\n{pv}")
    }
}

fn plan_section(
    ctx: &ToolCtx,
    path: PathBuf,
    key: PathBuf,
    tag: Option<String>,
    hunks: Vec<Hunk>,
) -> Result<Planned, String> {
    let shown = display_path(&ctx.cwd, &path);
    if !path.exists() {
        let base = not_found(&ctx.cwd, &path, &shown);
        return Err(format!("{base}\n`edit` only patches existing files; use `write` to create a new file."));
    }
    let file = load_text(&path, &shown)?;
    let text = file.text.clone();
    let cur_hash = file_hash(&text);
    let cur_lines = split_lines(&text);
    let mut warnings = vec![];

    let has_rem = hunks.iter().any(|h| h.kind == HunkKind::Rem);
    if has_rem && hunks.len() > 1 {
        return Err(format!(
            "Section for {shown} combines `REM` with other hunks; `REM` deletes the whole file and must be the only hunk."
        ));
    }
    let Some(tag) = tag else {
        return Err(format!(
            "Edit rejected for {shown}: the header needs this file's current tag, #{cur_hash}. Use `[{shown}#{cur_hash}]` after reading the lines you change."
        ));
    };
    if has_rem {
        if tag != cur_hash {
            return Err(format!(
                "Edit rejected for {shown}: `REM` requires the current tag #{cur_hash}, but the section is bound to #{tag}. Re-read the file before deleting it."
            ));
        }
        return Ok(Planned { path, key, shown, file, old_text: text, new_text: None, warnings });
    }

    let hunks = normalize_overlaps(hunks, &mut warnings)?;
    let anchors = anchored_lines(&hunks);

    let final_hunks = if tag == cur_hash {
        check_bounds(&hunks, cur_lines.len())?;
        let seen = ctx.store.lock().by_hash(&key, &tag).map(|s| s.seen.clone()).unwrap_or_else(Seen::none);
        let unseen: Vec<usize> = anchors.iter().copied().filter(|l| !seen.contains(*l)).collect();
        if !unseen.is_empty() {
            return Err(unseen_error(ctx, &key, &shown, &tag, &text, &cur_lines, &unseen));
        }
        hunks
    } else {
        let old = ctx.store.lock().by_hash(&key, &tag).cloned();
        match old {
            Some(old) => {
                let old_lines = split_lines(&old.text);
                let head_tail_only = anchors.is_empty();
                let rebased = if head_tail_only {
                    warnings.push(format!(
                        "{shown} changed since #{tag} was issued; the head/tail insertion was applied to the current file (#{cur_hash})."
                    ));
                    Some(hunks.clone())
                } else if check_bounds(&hunks, old_lines.len()).is_ok()
                    && anchors.iter().all(|l| old.seen.contains(*l))
                {
                    let map = line_map(&old_lines, &cur_lines);
                    rebase(&hunks, &map)
                } else {
                    None
                };
                match rebased {
                    Some(h) => {
                        if !head_tail_only {
                            warnings.push(format!(
                                "Recovered by remapping stale line anchors of {shown} (#{tag}) to unchanged current lines (#{cur_hash}); the file changed since the tagged read. Verify the diff matches your intent."
                            ));
                        }
                        check_bounds(&h, cur_lines.len())?;
                        h
                    }
                    None => {
                        return Err(mismatch_error(ctx, &key, &shown, &tag, &cur_hash, &text, &cur_lines, &anchors, false))
                    }
                }
            }
            None => {
                return Err(mismatch_error(ctx, &key, &shown, &tag, &cur_hash, &text, &cur_lines, &anchors, true))
            }
        }
    };

    let new_lines = materialize(&cur_lines, &final_hunks);
    let trailing_nl = text.is_empty() || text.ends_with('\n');
    let new_text = join_lines(&new_lines, trailing_nl);
    Ok(Planned { path, key, shown, file, old_text: text, new_text: Some(new_text), warnings })
}

/// Context rows (`*N:` anchors, ` N:` others, `...` between runs); returns rows + displayed lines.
fn anchored_context(lines: &[&str], anchors: &BTreeSet<usize>) -> (String, Vec<usize>) {
    let mut display = BTreeSet::new();
    for &a in anchors {
        if a == 0 || a > lines.len() {
            continue;
        }
        let lo = a.saturating_sub(MISMATCH_CONTEXT).max(1);
        let hi = (a + MISMATCH_CONTEXT).min(lines.len());
        display.extend(lo..=hi);
    }
    if display.is_empty() && !lines.is_empty() {
        // Anchors beyond the current end: show the tail.
        let lo = lines.len().saturating_sub(MISMATCH_CONTEXT * 2).max(1);
        display.extend(lo..=lines.len());
    }
    let mut out = String::new();
    let mut seen = vec![];
    let mut prev: Option<usize> = None;
    for (count, n) in display.into_iter().enumerate() {
        if count >= 200 {
            out.push_str("...\n");
            break;
        }
        if prev.is_some_and(|p| n > p + 1) {
            out.push_str("...\n");
        }
        prev = Some(n);
        let marker = if anchors.contains(&n) { '*' } else { ' ' };
        let (t, cut) = cap_line(lines[n - 1], MAX_LINE_CHARS);
        out.push_str(&format!("{marker}{n}:{t}\n"));
        if !cut {
            seen.push(n);
        }
    }
    if out.ends_with('\n') {
        out.pop();
    }
    (out, seen)
}

#[allow(clippy::too_many_arguments)]
fn mismatch_error(
    ctx: &ToolCtx,
    key: &std::path::Path,
    shown: &str,
    tag: &str,
    cur_hash: &str,
    text: &str,
    lines: &[&str],
    anchors: &BTreeSet<usize>,
    unknown: bool,
) -> String {
    let (rows, seen) = anchored_context(lines, anchors);
    let mut store = ctx.store.lock();
    let other = if unknown { store.path_with_hash(tag, key) } else { None };
    store.record(key, text, Seen::from(seen));
    drop(store);
    let mut msg = if unknown {
        let mut m = format!("Edit rejected for {shown}: hash #{tag} is not from this session.");
        if let Some(o) = other {
            m.push_str(&format!(" Hash #{tag} was issued in this session for {}.", display_path(&ctx.cwd, &o)));
        }
        m.push_str(&format!(
            "\nThe current file hashes to #{cur_hash}. Re-read the file with `read` to copy a current [path#tag] header — never invent the tag and never reuse one from a prior session."
        ));
        m
    } else {
        format!(
            "Edit rejected for {shown}: file changed between read and edit.\nSection is bound to #{tag}, but the current file hashes to #{cur_hash}. If a prior edit in this session modified this file, copy the [path#newhash] header from that edit's response; otherwise re-read the file with `read` to refresh the tag before retrying."
        )
    };
    if !rows.is_empty() {
        msg.push_str("\n\n");
        msg.push_str(&rows);
    }
    msg
}

fn unseen_error(
    ctx: &ToolCtx,
    key: &std::path::Path,
    shown: &str,
    tag: &str,
    text: &str,
    lines: &[&str],
    unseen: &[usize],
) -> String {
    let list = format_line_list(unseen);
    let mut msg = format!(
        "This edit anchors to lines {list} of {shown} that [{shown}#{tag}] never displayed (it showed a partial range, a search hit, or a cut line)."
    );
    if unseen.len() > SEEN_LINE_REVEAL_CAP {
        msg.push_str(" Re-read them in full first with `read` (offset/limit), then re-issue the edit.");
        return msg;
    }
    msg.push_str(" Actual file content at those lines:\n");
    let mut revealed = vec![];
    let mut any_cut = false;
    for &n in unseen {
        let (t, cut) = cap_line(lines[n - 1], MAX_LINE_CHARS);
        msg.push_str(&format!("{n}:{t}\n"));
        if cut {
            any_cut = true;
        } else {
            revealed.push(n);
        }
    }
    ctx.store.lock().record(key, text, Seen::from(revealed));
    if any_cut {
        msg.push_str("Some of these lines are too long to display whole; use `bash` or the `old_string`/`new_string` form to change them.");
    } else {
        msg.push_str(
            "Verify the content matches what you intend to touch, then re-issue the edit with the same [path#tag] header — a straight retry now succeeds without a re-read. If the content does NOT match, fix your line numbers.",
        );
    }
    msg
}

