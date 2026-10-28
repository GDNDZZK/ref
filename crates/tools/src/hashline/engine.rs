//! Patch driver: tag validation, seen-line guard, stale recovery, atomic multi-file write.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use super::apply::{apply_ops, finish_clipboard, line_map, preview, remap_ops, Clipboard};
use super::parse::{parse_patch, FileOp, Gap, HeaderTag, Op, OpKind, Section};
use super::{Origin, Seen};
use super::{anchored_context, format_line_ranges, header, split_lines, EditStore, TextFile, MAX_COLUMNS, SEEN_REVEAL_CAP};
use crate::util::{canonical_key, display_path, resolve_path};

const WIDE_LINE: usize = 512;
const HEADTAIL_DRIFT_WARNING: &str = "Applied the `PUT <1:`/`PUT >$:` edit despite a stale snapshot tag (file changed since your read) — head/tail position is content-independent. Re-read if the drift was unexpected.";
const RECOVERY_EXTERNAL_WARNING: &str = "Recovered from a stale file hash using a previous read snapshot (file changed externally between read and edit).";
const RECOVERY_SESSION_CHAIN_WARNING: &str = "Recovered from a stale file hash using an earlier in-session snapshot (a prior edit in this session advanced the hash).";
const RECOVERY_LINE_REMAP_WARNING: &str = "Recovered by remapping stale line anchors to unchanged current lines (file changed since the tagged read). Verify the diff matches your intent.";

/// Outcome of a whole patch call.
#[derive(Debug, Clone)]
pub struct PatchResult {
    pub text: String,
    pub is_error: bool,
}

impl PatchResult {
    fn err(text: impl Into<String>) -> Self {
        PatchResult { text: text.into(), is_error: true }
    }
}

/// Paths a patch touches (sources and MV destinations), for locking and titles. Best-effort.
pub fn patch_paths(input: &str, cwd: &Path) -> Vec<PathBuf> {
    let Ok(sections) = parse_patch(input) else { return Vec::new() };
    let mut out = Vec::new();
    for s in &sections {
        let abs = resolve_path(cwd, &s.path);
        // MV destinations resolve against the session cwd, like every other path.
        if let Some(FileOp::Move(dest)) = &s.file_op {
            out.push(resolve_path(cwd, dest));
        }
        out.push(abs);
    }
    out
}

fn current_tag_advice(tag: &str) -> String {
    format!("The current file hashes to #{tag}. Re-read the file with `read` to copy a current [path#tag] header — never invent the tag and never reuse one from a prior session.")
}

/// Record the current content with the rows a reply displayed (unless already unrestricted).
fn record_reply(store: &EditStore, key: &Path, text: &str, displayed: &[u32]) -> String {
    if let Some(s) = store.by_content(key, text) {
        if s.seen == Seen::All {
            return s.tag;
        }
    }
    let mut lines = displayed.to_vec();
    lines.push(0);
    store.record(key, text, Seen::lines(lines), Origin::Display)
}

fn guarded_lines(ops: &[Op]) -> Vec<u32> {
    let mut v: Vec<u32> = ops.iter().flat_map(|o| o.anchors()).map(|l| l as u32).collect();
    v.sort_unstable();
    v.dedup();
    v
}

fn only_head_tail(ops: &[Op]) -> bool {
    !ops.is_empty()
        && ops.iter().all(|o| match &o.kind {
            OpKind::Insert { gap, .. } | OpKind::PasteGap { gap, .. } => matches!(gap, Gap::Bof | Gap::Eof),
            _ => false,
        })
}

struct Prepared {
    abs: PathBuf,
    key: PathBuf,
    display: String,
    file: Option<TextFile>,
    new_text: Option<String>,
    file_op: Option<FileOp>,
    dest: Option<(PathBuf, PathBuf, String)>,
    warnings: Vec<String>,
    old_seen: Option<Seen>,
}

#[allow(clippy::too_many_arguments)]
fn mismatch_message(
    display: &str,
    expected: &HeaderTag,
    actual: &str,
    recognized: bool,
    origins: &[String],
    anchors: &[u32],
    lines: &[&str],
) -> (String, Vec<u32>) {
    let mut out: Vec<String> = Vec::new();
    match expected {
        HeaderTag::Tag(t) if recognized => {
            out.push(format!("Edit rejected for {display}: file changed between read and edit."));
            out.push(format!("Section is bound to #{t}, but the current file hashes to #{actual}. If a prior edit in this session modified this file, copy the [path#newhash] header from that edit's response; otherwise re-read the file with `read` to refresh the tag before retrying."));
        }
        HeaderTag::Tag(t) => {
            out.push(format!("Edit rejected for {display}: hash #{t} is not from this session."));
            for o in origins {
                out.push(format!("Hash #{t} was issued in this session for {o}."));
            }
            out.push(current_tag_advice(actual));
        }
        _ => {
            out.push(format!("Edit rejected for {display}: the header needs this file's current tag, #{actual}."));
            out.push(current_tag_advice(actual));
        }
    }
    let (rows, shown) = anchored_context(anchors, lines);
    if !rows.is_empty() {
        out.push(String::new());
        out.extend(rows);
    }
    (out.join("\n"), shown)
}

/// Seen-line guard. Err = rejection message (lines revealed inline are recorded as seen).
fn check_seen(store: &EditStore, key: &Path, display: &str, tag: &str, ops: &[Op], lines: &[&str]) -> Result<Option<Seen>, String> {
    let guarded = guarded_lines(ops);
    let Some(snap) = store.by_tag(key, tag) else {
        // No retained snapshot (e.g. restart) but the tag matches: only reject wide lines.
        let wide: Vec<u32> = guarded
            .iter()
            .copied()
            .filter(|&l| lines.get(l as usize - 1).is_some_and(|t| t.chars().count() > MAX_COLUMNS))
            .collect();
        if !wide.is_empty() {
            return Err(format!(
                "This edit anchors to lines {} of {display} that were not fully displayed (longer than {MAX_COLUMNS} columns). Read them with `read` (offset/limit) and use `bash` to inspect full content if needed, then re-issue the edit.",
                format_line_ranges(&wide)
            ));
        }
        return Ok(None);
    };
    let unseen: Vec<u32> = guarded.iter().copied().filter(|&l| !snap.seen.contains(l)).collect();
    if unseen.is_empty() {
        return Ok(Some(snap.seen));
    }
    let ranges = format_line_ranges(&unseen);
    let wide = unseen.iter().any(|&l| lines.get(l as usize - 1).is_some_and(|t| t.chars().count() > WIDE_LINE));
    if wide {
        return Err(format!(
            "This edit anchors to lines {ranges} of {display} that were not fully displayed (some exceed {WIDE_LINE} chars). Inspect their full content (e.g. `bash` with `sed -n`), then rewrite the affected region with a range that you have seen, or use `write` for the whole file."
        ));
    }
    let head = format!(
        "This edit anchors to lines {ranges} of {display} that {} never displayed (it showed a partial range, a search hit, or a truncated line).",
        header(display, tag)
    );
    if unseen.len() > SEEN_REVEAL_CAP {
        let first = unseen[0];
        return Err(format!(
            "{head} Re-read them in full first with `read` (offset={first}, limit={}), then re-issue the edit.",
            unseen[unseen.len() - 1] - first + 1
        ));
    }
    let preview: Vec<String> = unseen.iter().map(|&l| format!("  {l}:{}", lines[l as usize - 1])).collect();
    store.add_seen(key, tag, &unseen);
    Err(format!(
        "{head} Actual file content at those lines:\n{}\nVerify the content matches what you intend to touch, then re-issue the edit with the same [path#tag] header — a straight retry now succeeds without a re-read. If the content does NOT match, fix your line numbers.",
        preview.join("\n")
    ))
}

fn prepare(section: &Section, cwd: &Path, store: &EditStore, clip: &mut Clipboard) -> Result<Prepared, String> {
    let mut abs = resolve_path(cwd, &section.path);
    let mut warnings = section.warnings.clone();
    if !abs.exists() {
        // Recover a mistyped path via a unique snapshot with the same tag + basename.
        let base = abs.file_name().map(|s| s.to_os_string());
        let cands: Vec<PathBuf> = section
            .tag()
            .map(|t| store.paths_with_tag(t))
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p.file_name().map(|s| s.to_os_string()) == base && p.exists())
            .collect();
        if cands.len() == 1 {
            warnings.push(format!(
                "Path {} does not exist; applied to {} (the file this tag was issued for).",
                section.path,
                display_path(cwd, &cands[0])
            ));
            abs = cands[0].clone();
        } else {
            return Err(format!("File not found: {}. Use the write tool to create new files.", section.path));
        }
    }
    if abs.is_dir() {
        return Err(format!("{} is a directory; the edit tool patches files.", section.path));
    }
    let key = canonical_key(&abs);
    let display = display_path(cwd, &abs);
    let bytes = std::fs::read(&abs).map_err(|e| format!("Cannot read {display}: {e}"))?;
    let file = TextFile::decode(&bytes);
    let mut prepared = Prepared {
        abs: abs.clone(),
        key: key.clone(),
        display: display.clone(),
        file: None,
        new_text: None,
        file_op: section.file_op.clone(),
        dest: None,
        warnings,
        old_seen: None,
    };
    let Some(file) = file else {
        if matches!(section.file_op, Some(FileOp::Rem)) && section.tag().is_some() {
            return Ok(prepared);
        }
        return Err(format!("Cannot edit {display}: not valid UTF-8 text (binary file)."));
    };
    let live = file.text.clone();
    let lines = split_lines(&live);
    let actual = file.tag();

    // Missing / placeholder tag.
    let Some(tag) = section.tag() else {
        let anchors = guarded_lines(&section.ops);
        let (msg, shown) = mismatch_message(&display, &section.tag, &actual, false, &[], &anchors, &lines);
        record_reply(store, &key, &live, &shown);
        return Err(msg);
    };
    if section.ops.is_empty() && section.file_op.is_none() {
        record_reply(store, &key, &live, &[]);
        return Err(format!(
            "Edit rejected for {display}: no ops follow the header; put them under this file's current tag, #{actual}.\n{}",
            current_tag_advice(&actual)
        ));
    }

    let mut ops: Vec<Op> = section.ops.clone();
    if tag == actual {
        if let Some(&bad) = guarded_lines(&ops).iter().find(|&&l| l as usize > lines.len()) {
            return Err(format!("{display}: Line {bad} does not exist (file has {} lines)", lines.len()));
        }
        prepared.old_seen = check_seen(store, &key, &display, tag, &ops, &lines)?;
    } else if only_head_tail(&ops) {
        prepared.warnings.push(HEADTAIL_DRIFT_WARNING.into());
        prepared.old_seen = store.by_content(&key, &live).map(|s| s.seen);
    } else {
        let snap = store.by_tag(&key, tag);
        let recovered = snap.as_ref().filter(|_| !matches!(section.file_op, Some(FileOp::Rem))).and_then(|s| {
            let guarded = guarded_lines(&ops);
            if !guarded.iter().all(|&l| s.seen.contains(l)) {
                return None;
            }
            remap_ops(&s.text, &live, &ops).map(|m| (m, s))
        });
        match recovered {
            Some((mapped, s)) => {
                let shifted = mapped.iter().zip(&ops).any(|(a, b)| a.anchors() != b.anchors());
                let chain = store.by_content(&key, &live).is_some_and(|c| c.origin == Origin::Written);
                prepared.warnings.push(
                    if chain {
                        RECOVERY_SESSION_CHAIN_WARNING
                    } else if shifted {
                        RECOVERY_LINE_REMAP_WARNING
                    } else {
                        RECOVERY_EXTERNAL_WARNING
                    }
                    .into(),
                );
                // Seen lines carry over through the line map.
                let map = line_map(&s.text, &live);
                prepared.old_seen = Some(match &s.seen {
                    Seen::All => Seen::All,
                    Seen::Lines(set) => Seen::lines(set.iter().filter_map(|l| map.get(&(*l as usize)).map(|v| *v as u32))),
                });
                ops = mapped;
            }
            None => {
                let anchors = guarded_lines(&ops);
                let origins: Vec<String> = if snap.is_none() {
                    store.paths_with_tag(tag).iter().filter(|p| **p != key).map(|p| display_path(cwd, p)).collect()
                } else {
                    Vec::new()
                };
                let (msg, shown) = mismatch_message(&display, &section.tag, &actual, snap.is_some(), &origins, &anchors, &lines);
                record_reply(store, &key, &live, &shown);
                return Err(msg);
            }
        }
    }

    if matches!(section.file_op, Some(FileOp::Rem)) {
        prepared.file = Some(file);
        return Ok(prepared);
    }

    let applied = apply_ops(&live, &ops, clip).map_err(|e| format!("{display}: {e}"))?;
    prepared.warnings.extend(applied.warnings);
    if let Some(FileOp::Move(dest)) = &section.file_op {
        let dabs = resolve_path(cwd, dest);
        let dkey = canonical_key(&dabs);
        if dkey != key && dabs.exists() {
            return Err(format!("Cannot move {display} to {dest}: destination already exists. Delete it first or pick another path."));
        }
        prepared.dest = Some((dabs.clone(), dkey, display_path(cwd, &dabs)));
    }
    if applied.text == live && section.file_op.is_none() {
        let mut h = DefaultHasher::new();
        format!("{:?}", section.ops).hash(&mut h);
        let count = store.note_noop(&key, h.finish());
        if count >= 3 {
            return Err(format!("STOP. Edits to {display} have been a byte-identical no-op {count} times in a row. Your anchors point at lines that already contain your payload. Re-read the file with `read`, locate the real problem, and change approach instead of resending."));
        }
        return Err(format!("Edits to {display} parsed and applied cleanly, but produced no change: your body row(s) are byte-identical to the file at the targeted lines. The bug is somewhere else — re-read the file before issuing another edit. Do NOT widen the payload or add lines; verify the anchor first."));
    }
    prepared.new_text = Some(applied.text);
    prepared.file = Some(file);
    Ok(prepared)
}

fn write_atomic(path: &Path, content: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Preserve permissions by writing in place via a temp sibling + rename.
    let tmp = path.with_file_name(format!(
        ".{}.hn-tmp-{}",
        path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
        std::process::id()
    ));
    std::fs::write(&tmp, content)?;
    if let Ok(meta) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Parse, validate every section, then write all files. Nothing is written if any section fails.
pub fn run_patch(input: &str, cwd: &Path, store: &EditStore) -> PatchResult {
    let sections = match parse_patch(input) {
        Ok(s) => s,
        Err(e) => return PatchResult::err(format!("Patch parse error: {e}")),
    };
    let mut clip = Clipboard::new(store.registers());
    let mut prepared = Vec::new();
    let mut errors = Vec::new();
    for s in &sections {
        match prepare(s, cwd, store, &mut clip) {
            Ok(p) => prepared.push(p),
            Err(e) => errors.push(e),
        }
    }
    finish_clipboard(&mut clip);
    if !errors.is_empty() {
        let mut text = errors.join("\n\n");
        if sections.len() > 1 {
            text.push_str("\n\nNo files were modified (all sections must validate before any write).");
        }
        return PatchResult::err(text);
    }

    let mut outputs = Vec::new();
    let mut write_errors = Vec::new();
    for p in prepared {
        store.clear_noop(&p.key);
        if matches!(p.file_op, Some(FileOp::Rem)) {
            match std::fs::remove_file(&p.abs) {
                Ok(()) => {
                    store.invalidate(&p.key);
                    outputs.push(format!("Deleted {}", p.display));
                }
                Err(e) => write_errors.push(format!("Failed to delete {}: {e}", p.display)),
            }
            continue;
        }
        let (Some(file), Some(new_text)) = (p.file.as_ref(), p.new_text.as_ref()) else { continue };
        let encoded = file.encode(new_text);
        let (target_abs, target_key, target_display) = match &p.dest {
            Some((a, k, d)) => (a.clone(), k.clone(), d.clone()),
            None => (p.abs.clone(), p.key.clone(), p.display.clone()),
        };
        if let Err(e) = write_atomic(&target_abs, &encoded) {
            write_errors.push(format!("Failed to write {target_display}: {e}"));
            continue;
        }
        if p.dest.is_some() && target_key != p.key {
            if let Err(e) = std::fs::remove_file(&p.abs) {
                write_errors.push(format!("Wrote {target_display} but failed to remove {}: {e}", p.display));
            }
            store.relocate(&p.key, &target_key);
        }
        let old_text = &file.text;
        let (rows, shown) = preview(old_text, new_text);
        // Carry over seen lines whose number and content are unchanged, plus preview rows.
        let seen = match &p.old_seen {
            Some(Seen::All) => Seen::All,
            other => {
                let map = line_map(old_text, new_text);
                let mut set: Vec<u32> = match other {
                    Some(Seen::Lines(s)) => s.iter().copied().filter(|l| map.get(&(*l as usize)) == Some(&(*l as usize))).collect(),
                    _ => Vec::new(),
                };
                set.extend(shown);
                set.push(0);
                Seen::lines(set)
            }
        };
        let tag = store.record(&target_key, new_text, seen, Origin::Written);
        let mut out = vec![header(&target_display, &tag)];
        if p.dest.is_some() {
            out.push(format!("Moved {} to {target_display}", p.display));
        }
        out.extend(rows);
        if !p.warnings.is_empty() {
            out.push(String::new());
            out.push("Warnings:".into());
            out.extend(p.warnings.iter().map(|w| format!("- {w}")));
        }
        outputs.push(out.join("\n"));
    }
    store.commit_registers(std::mem::take(&mut clip.dirty));
    let mut text = outputs.join("\n\n");
    if !write_errors.is_empty() {
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(&write_errors.join("\n"));
    }
    PatchResult { text, is_error: !write_errors.is_empty() }
}
