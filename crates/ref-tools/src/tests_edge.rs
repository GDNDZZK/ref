//! Edge cases: line endings, empty files, EOF handling, unicode, paths, repairs, bash behavior.

use serde_json::json;

use crate::file_hash;
use crate::tests::{call, edit, err, get, ok, put, rt, setup, tag_of, ABC};

fn read_tag(ctx: &crate::ToolCtx, path: &str) -> String {
    tag_of(&ok(ctx, "read", json!({ "path": path })))
}

fn patch_ok(ctx: &crate::ToolCtx, patch: &str) -> String {
    let o = edit(ctx, patch);
    assert!(!o.is_error, "edit failed: {}\n--- patch:\n{patch}", o.text);
    o.text
}

// ───────────────────────────── EOF / empty files ─────────────────────────────

#[test]
fn no_trailing_newline_append_and_insert_after_last() {
    let (d, ctx) = setup();
    put(d.path(), "n.txt", "a\nb");
    let t = read_tag(&ctx, "n.txt");
    patch_ok(&ctx, &format!("[n.txt#{t}]\nPUT >2:\n+c\n"));
    assert_eq!(
        get(d.path(), "n.txt"),
        "a\nb\nc",
        "no trailing newline must stay absent"
    );
    let t = read_tag(&ctx, "n.txt");
    patch_ok(&ctx, &format!("[n.txt#{t}]\nPUT >$:\n+d\n"));
    assert_eq!(get(d.path(), "n.txt"), "a\nb\nc\nd");
    let t = read_tag(&ctx, "n.txt");
    patch_ok(&ctx, &format!("[n.txt#{t}]\nPUT 4:\n+D\n"));
    assert_eq!(get(d.path(), "n.txt"), "a\nb\nc\nD");
}

#[test]
fn insert_before_one_past_end_appends() {
    let (d, ctx) = setup();
    put(d.path(), "a.txt", "x\ny\n");
    let t = read_tag(&ctx, "a.txt");
    patch_ok(&ctx, &format!("[a.txt#{t}]\nPUT <3:\n+z\n"));
    assert_eq!(get(d.path(), "a.txt"), "x\ny\nz\n");
    let t = read_tag(&ctx, "a.txt");
    let o = edit(&ctx, &format!("[a.txt#{t}]\nPUT <5:\n+w\n"));
    assert!(o.is_error && o.text.contains("PUT >$:"), "{}", o.text);
}

#[test]
fn empty_file_insert_and_cut_everything() {
    let (d, ctx) = setup();
    put(d.path(), "e.txt", "");
    let t = read_tag(&ctx, "e.txt");
    assert_eq!(t, "5D05");
    patch_ok(&ctx, &format!("[e.txt#{t}]\nPUT <1:\n+first\n"));
    assert_eq!(get(d.path(), "e.txt"), "first\n");
    put(d.path(), "f.txt", "");
    let t = read_tag(&ctx, "f.txt");
    patch_ok(&ctx, &format!("[f.txt#{t}]\nPUT >$:\n+x\n"));
    assert_eq!(get(d.path(), "f.txt"), "x\n");
    // PUT >0 means insert at the top.
    let t = read_tag(&ctx, "f.txt");
    patch_ok(&ctx, &format!("[f.txt#{t}]\nPUT >0:\n+top\n"));
    assert_eq!(get(d.path(), "f.txt"), "top\nx\n");
    // CUT the whole file: empty file, not deleted.
    let t = read_tag(&ctx, "f.txt");
    let out = patch_ok(&ctx, &format!("[f.txt#{t}]\nCUT 1.=2\n"));
    assert!(out.contains("(file is now empty)"), "{out}");
    assert_eq!(get(d.path(), "f.txt"), "");
}

#[test]
fn put_with_empty_body_is_cut() {
    let (d, ctx) = setup();
    put(d.path(), "a.txt", ABC);
    let t = read_tag(&ctx, "a.txt");
    let out = patch_ok(&ctx, &format!("[a.txt#{t}]\nPUT 2.=3:\nPUT 5:\n+E\n"));
    assert!(out.contains("treated as `CUT 2.=3`"), "{out}");
    assert_eq!(get(d.path(), "a.txt"), "alpha\ndelta\nE\nfoxtrot\ngolf\n");
}

// ───────────────────────────── line endings / unicode ─────────────────────────────

#[test]
fn crlf_without_trailing_newline_and_mixed_endings() {
    let (d, ctx) = setup();
    std::fs::write(d.path().join("w.ini"), b"[a]\r\nx = 1\r\ny = 2").unwrap();
    let t = read_tag(&ctx, "w.ini");
    patch_ok(
        &ctx,
        &format!("[w.ini#{t}]\nPUT 2:\n+x = 9\nPUT >3:\n+z = 3\n"),
    );
    assert_eq!(
        std::fs::read(d.path().join("w.ini")).unwrap(),
        b"[a]\r\nx = 9\r\ny = 2\r\nz = 3"
    );
    // Mixed endings: untouched lines keep their exact bytes.
    std::fs::write(d.path().join("m.txt"), b"one\r\ntwo\nthree\r\n").unwrap();
    let t = read_tag(&ctx, "m.txt");
    patch_ok(&ctx, &format!("[m.txt#{t}]\nPUT 2:\n+TWO\n"));
    assert_eq!(
        std::fs::read(d.path().join("m.txt")).unwrap(),
        b"one\r\nTWO\nthree\r\n"
    );
}

#[test]
fn lone_cr_is_content_not_a_line_break() {
    let (d, ctx) = setup();
    std::fs::write(d.path().join("cr.txt"), b"a\rb\nc\n").unwrap();
    let out = ok(&ctx, "read", json!({"path": "cr.txt"}));
    assert!(out.contains("1:a\rb\n2:c"), "{out:?}");
    let t = tag_of(&out);
    patch_ok(&ctx, &format!("[cr.txt#{t}]\nPUT 2:\n+C\n"));
    assert_eq!(
        std::fs::read(d.path().join("cr.txt")).unwrap(),
        b"a\rb\nC\n"
    );
}

#[test]
fn unicode_lines_and_paths() {
    let (d, ctx) = setup();
    let text = "首行 🎉\ne\u{301}tude\n第三行\n";
    put(d.path(), "目录 a/文件 名.md", text);
    let out = ok(&ctx, "read", json!({"path": "目录 a/文件 名.md"}));
    assert!(
        out.starts_with(&format!(
            "[目录 a/文件 名.md#{}]\n1:首行 🎉\n2:e\u{301}tude",
            file_hash(text)
        )),
        "{out}"
    );
    let t = tag_of(&out);
    patch_ok(
        &ctx,
        &format!("[目录 a/文件 名.md#{t}]\nPUT 3:\n+第三行 ✓\n"),
    );
    assert_eq!(
        get(d.path(), "目录 a/文件 名.md"),
        "首行 🎉\ne\u{301}tude\n第三行 ✓\n"
    );
    // Long CJK line: cut by chars, never mid-codepoint.
    put(d.path(), "long.txt", &"汉".repeat(2100));
    let out = ok(&ctx, "read", json!({"path": "long.txt"}));
    assert!(
        out.contains('…') && out.contains("Lines 1 are longer"),
        "{}",
        &out[out.len() - 200..]
    );
}

// ───────────────────────────── paths ─────────────────────────────

#[test]
fn relative_dotdot_symlink_and_permissions() {
    let (d, ctx) = setup();
    put(d.path(), "sub/x.txt", "x\n");
    let out = ok(&ctx, "read", json!({"path": "sub/../sub/./x.txt"}));
    assert!(out.starts_with("[sub/x.txt#"), "{out}");
    std::os::unix::fs::symlink(d.path().join("sub/x.txt"), d.path().join("link.txt")).unwrap();
    let t = read_tag(&ctx, "link.txt");
    patch_ok(&ctx, &format!("[link.txt#{t}]\nPUT 1:\n+X\n"));
    assert_eq!(get(d.path(), "sub/x.txt"), "X\n");
    assert!(std::fs::symlink_metadata(d.path().join("link.txt"))
        .unwrap()
        .file_type()
        .is_symlink());
    // The same file through two names shares one snapshot: the target's tag is current.
    let t2 = read_tag(&ctx, "sub/x.txt");
    patch_ok(
        &ctx,
        &format!("[sub/x.txt#{t2}]\nPUT 1:\n+Y\n[link.txt#{t2}]\nPUT >1:\n+Z\n"),
    );
    assert_eq!(get(d.path(), "sub/x.txt"), "Y\nZ\n");
    // Deleting a symlink removes the link, not the target.
    ok(&ctx, "delete", json!({"path": "link.txt"}));
    assert!(d.path().join("sub/x.txt").exists());
    // Missing directory.
    let e = err(&ctx, "read", json!({"path": "nope/x.txt"}));
    assert!(e.contains("File not found: nope/x.txt"), "{e}");
    let e = err(&ctx, "grep", json!({"pattern": "x", "path": "nope"}));
    assert!(e.contains("not found"), "{e}");
    // Permission denied (skipped when running as root).
    use std::os::unix::fs::PermissionsExt;
    put(d.path(), "secret.txt", "s\n");
    std::fs::set_permissions(
        d.path().join("secret.txt"),
        std::fs::Permissions::from_mode(0o000),
    )
    .unwrap();
    if std::fs::read(d.path().join("secret.txt")).is_err() {
        let e = err(&ctx, "read", json!({"path": "secret.txt"}));
        assert!(e.contains("Permission denied: secret.txt"), "{e}");
    }
}

#[test]
fn tilde_paths() {
    let Some(home) = dirs::home_dir() else { return };
    let (_d, ctx) = setup();
    let dir = tempfile::tempdir_in(&home).unwrap();
    let name = dir
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    std::fs::write(dir.path().join("t.txt"), "t\n").unwrap();
    let out = ok(&ctx, "read", json!({"path": format!("~/{name}/t.txt")}));
    assert!(out.starts_with(&format!("[~/{name}/t.txt#")), "{out}");
    let t = tag_of(&out);
    patch_ok(&ctx, &format!("[~/{name}/t.txt#{t}]\nPUT 1:\n+T\n"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("t.txt")).unwrap(),
        "T\n"
    );
}

// ───────────────────────────── multi-hunk / multi-section ─────────────────────────────

#[test]
fn adjacent_hunks_and_same_line_inserts() {
    let (d, ctx) = setup();
    put(d.path(), "a.txt", ABC);
    let t = read_tag(&ctx, "a.txt");
    // Adjacent replaced ranges, an insert after the first range's end and before the next.
    patch_ok(
        &ctx,
        &format!("[a.txt#{t}]\nPUT 2:\n+B\nPUT >2:\n+after-b\nPUT <3:\n+before-c\nPUT 3.=4:\n+CD\nPUT >7:\n+end\n"),
    );
    assert_eq!(
        get(d.path(), "a.txt"),
        "alpha\nB\nafter-b\nbefore-c\nCD\necho\nfoxtrot\ngolf\nend\n"
    );
}

#[test]
fn same_file_twice_merges_and_conflicting_tags_rejected() {
    let (d, ctx) = setup();
    put(d.path(), "a.txt", ABC);
    let t = read_tag(&ctx, "a.txt");
    patch_ok(
        &ctx,
        &format!("[a.txt#{t}]\nPUT 1:\n+A\n[./a.txt#{t}]\nPUT 7:\n+G\n"),
    );
    assert_eq!(
        get(d.path(), "a.txt"),
        ABC.replace("alpha", "A").replace("golf", "G")
    );
    let t2 = read_tag(&ctx, "a.txt");
    let o = edit(
        &ctx,
        &format!("[a.txt#{t2}]\nPUT 1:\n+x\n[a.txt#{t}]\nPUT 2:\n+y\n"),
    );
    assert!(
        o.is_error && o.text.contains("different tags"),
        "{}",
        o.text
    );
}

#[test]
fn multi_file_failure_writes_nothing_and_names_section() {
    let (d, ctx) = setup();
    put(d.path(), "a.txt", "a\n");
    put(d.path(), "b.txt", "b\n");
    let ta = read_tag(&ctx, "a.txt");
    let o = edit(
        &ctx,
        &format!("[a.txt#{ta}]\nPUT 1:\n+A\n[b.txt#BEEF]\nPUT 1:\n+B\n"),
    );
    assert!(
        o.is_error && o.text.contains("Edit rejected for b.txt"),
        "{}",
        o.text
    );
    assert!(o.text.contains("No file was changed"), "{}", o.text);
    assert_eq!(get(d.path(), "a.txt"), "a\n");
}

// ───────────────────────────── parser tolerance ─────────────────────────────

#[test]
fn tolerant_range_syntax_and_missing_colon() {
    let (d, ctx) = setup();
    for (hdr, want) in [
        ("PUT 2..=3:", "alpha\nX\ndelta\n"),
        ("PUT 2:=3:", "alpha\nX\ndelta\n"),
        ("PUT 2..3:", "alpha\nX\ndelta\n"),
        ("PUT 2.:=3:", "alpha\nX\ndelta\n"),
        ("PUT 2:3:", "alpha\nX\ndelta\n"),
        ("PUT 2-3:", "alpha\nX\ndelta\n"),
        ("PUT 2.=3", "alpha\nX\ndelta\n"),
        ("PUT 2", "alpha\nX\ncharlie\ndelta\n"),
        ("PUT 2>:", "alpha\nbravo\nX\ncharlie\ndelta\n"),
    ] {
        put(d.path(), "a.txt", "alpha\nbravo\ncharlie\ndelta\n");
        let t = read_tag(&ctx, "a.txt");
        let out = patch_ok(&ctx, &format!("[a.txt#{t}]\n{hdr}\n+X\n"));
        assert!(out.contains("Warnings:"), "{hdr}: {out}");
        assert_eq!(get(d.path(), "a.txt"), want, "{hdr}");
    }
    put(d.path(), "a.txt", ABC);
    let t = read_tag(&ctx, "a.txt");
    let o = edit(&ctx, &format!("[a.txt#{t}]\nPUT 2.=\n+X\n"));
    assert!(o.is_error && o.text.contains("no end line"), "{}", o.text);
    let o = edit(&ctx, &format!("[a.txt#{t}]\nPUT 2\nCUT 3\n"));
    assert!(
        o.is_error && o.text.contains("must end with `:`"),
        "{}",
        o.text
    );
}

#[test]
fn bracket_lines_in_body_are_content() {
    let (d, ctx) = setup();
    put(d.path(), "s.ini", "[a]\nx = 1\n");
    let t = read_tag(&ctx, "s.ini");
    // `[b]` without `+` inside a body is an INI section, not a file header.
    patch_ok(&ctx, &format!("[s.ini#{t}]\nPUT >2:\n+\n[b]\n+y = 2\n"));
    assert_eq!(get(d.path(), "s.ini"), "[a]\nx = 1\n\n[b]\ny = 2\n");
}

#[test]
fn numbered_prefix_strip_needs_matching_numbers() {
    let (d, ctx) = setup();
    put(d.path(), "t.yml", "a: 1\nb: 2\nc: 3\n");
    let t = read_tag(&ctx, "t.yml");
    // `10:30` style content is kept when the numbers do not match the target lines.
    patch_ok(&ctx, &format!("[t.yml#{t}]\nPUT 2:\n+10:30\n+11:00\n"));
    assert_eq!(get(d.path(), "t.yml"), "a: 1\n10:30\n11:00\nc: 3\n");
    // Pasted read rows with the right numbers are stripped.
    let t = read_tag(&ctx, "t.yml");
    patch_ok(&ctx, &format!("[t.yml#{t}]\nPUT 1.=2:\n+1:A\n+2:B\n"));
    assert_eq!(get(d.path(), "t.yml"), "A\nB\n11:00\nc: 3\n");
}

// ───────────────────────────── auto-repairs ─────────────────────────────

#[test]
fn boundary_echo_is_trimmed() {
    let (d, ctx) = setup();
    put(
        d.path(),
        "p.py",
        "def f():\n    x = compute()\n    return x\n\nprint(f())\n",
    );
    let t = read_tag(&ctx, "p.py");
    // Body restates line 1 (above the range) and line 3 (below it).
    let out = patch_ok(
        &ctx,
        &format!("[p.py#{t}]\nPUT 2:\n+def f():\n+    x = compute() + 1\n+    return x\n"),
    );
    assert!(
        out.contains("repeats unchanged line 1") && out.contains("repeats unchanged line 3"),
        "{out}"
    );
    assert_eq!(
        get(d.path(), "p.py"),
        "def f():\n    x = compute() + 1\n    return x\n\nprint(f())\n"
    );
    // Insert after N that starts by restating N.
    let t = read_tag(&ctx, "p.py");
    patch_ok(
        &ctx,
        &format!("[p.py#{t}]\nPUT >3:\n+    return x\n+\n+def g():\n+    pass\n"),
    );
    assert_eq!(
        get(d.path(), "p.py"),
        "def f():\n    x = compute() + 1\n    return x\n\ndef g():\n    pass\n\nprint(f())\n"
    );
    // Short, non-distinctive lines (`}`) are never treated as echoes.
    put(d.path(), "b.rs", "fn a() {\n    1\n}\n");
    let t = read_tag(&ctx, "b.rs");
    patch_ok(&ctx, &format!("[b.rs#{t}]\nPUT 2:\n+    2\n+}}\n"));
    assert_eq!(get(d.path(), "b.rs"), "fn a() {\n    2\n}\n}\n");
}

#[test]
fn replace_that_drops_a_line_warns() {
    let (d, ctx) = setup();
    put(
        d.path(),
        "t.md",
        "| a |\n| --- |\n| `alpha(x)` | first |\n| `beta(y)` | second |\n",
    );
    let t = read_tag(&ctx, "t.md");
    // Meant to insert after 3, but replaced it.
    let out = patch_ok(
        &ctx,
        &format!("[t.md#{t}]\nPUT 3:\n+| `gamma(z)` | third |\n"),
    );
    assert!(
        out.contains("removed line 3") && out.contains("-| `alpha(x)` | first |"),
        "{out}"
    );
    // A normal in-place modification does not warn.
    let t = read_tag(&ctx, "t.md");
    let out = patch_ok(&ctx, &format!("[t.md#{t}]\nPUT 4:\n+| `beta(y)` | 2nd |\n"));
    assert!(!out.contains("Warnings"), "{out}");
}

#[test]
fn stale_tag_with_current_numbers_is_refused() {
    let (d, ctx) = setup();
    put(d.path(), "a.txt", ABC);
    let t = read_tag(&ctx, "a.txt");
    patch_ok(&ctx, &format!("[a.txt#{t}]\nPUT <1:\n+top1\n+top2\n"));
    // Old tag, but numbers (and body) aimed at the CURRENT file: line 4 is now `bravo`.
    let o = edit(&ctx, &format!("[a.txt#{t}]\nPUT 4:\n+bravo-changed\n"));
    assert!(
        o.is_error && o.text.contains("match the CURRENT lines"),
        "{}",
        o.text
    );
    assert_eq!(get(d.path(), "a.txt"), format!("top1\ntop2\n{ABC}"));
    // Old tag with old numbers still rebases.
    patch_ok(&ctx, &format!("[a.txt#{t}]\nPUT 2:\n+BRAVO\n"));
    assert_eq!(
        get(d.path(), "a.txt"),
        format!("top1\ntop2\n{}", ABC.replace("bravo", "BRAVO"))
    );
}

#[test]
fn seen_guard_grep_partial_read_full_write_all() {
    let (d, ctx) = setup();
    put(d.path(), "a.txt", ABC);
    let g = ok(
        &ctx,
        "grep",
        json!({"pattern": "delta", "path": "a.txt", "context": 1}),
    );
    let t = tag_of(&g);
    // grep showed 3-5 only.
    patch_ok(&ctx, &format!("[a.txt#{t}]\nPUT 4:\n+DELTA\n"));
    let t = read_tag(&ctx, "a.txt"); // full read
    let partial = ok(&ctx, "grep", json!({"pattern": "golf", "path": "a.txt"}));
    assert_eq!(tag_of(&partial), t);
    // A later partial view must not shrink what the full read made editable.
    patch_ok(&ctx, &format!("[a.txt#{t}]\nPUT 1:\n+ALPHA\n"));
    let w = ok(
        &ctx,
        "write",
        json!({"path": "a.txt", "content": "1\n2\n3\n"}),
    );
    patch_ok(&ctx, &format!("[a.txt#{}]\nPUT 2:\n+two\n", tag_of(&w)));
    assert_eq!(get(d.path(), "a.txt"), "1\ntwo\n3\n");
}

#[test]
fn mismatch_context_marks_rows_seen_for_retry() {
    let (d, ctx) = setup();
    let text: String = (1..=40).map(|i| format!("line {i}\n")).collect();
    put(d.path(), "a.txt", &text);
    let t = tag_of(&ok(&ctx, "read", json!({"path": "a.txt", "limit": 5})));
    put(
        d.path(),
        "a.txt",
        &text.replace("line 30\n", "line thirty\n"),
    );
    let o = edit(&ctx, &format!("[a.txt#{t}]\nPUT 30:\n+x\n"));
    assert!(
        o.is_error && o.text.contains("*30:line thirty"),
        "{}",
        o.text
    );
    let cur = file_hash(&get(d.path(), "a.txt"));
    patch_ok(&ctx, &format!("[a.txt#{cur}]\nPUT 30:\n+line 30 again\n"));
}

// ───────────────────────────── read / bash ─────────────────────────────

#[test]
fn read_notices() {
    let (d, ctx) = setup();
    let text: String = (1..=2500).map(|i| format!("{i}\n")).collect();
    put(d.path(), "big.txt", &text);
    let out = ok(&ctx, "read", json!({"path": "big.txt"}));
    assert!(
        out.ends_with("2000:2000\n[500 more lines. Use offset=2001 to continue]"),
        "{}",
        &out[out.len() - 80..]
    );
    let out = ok(&ctx, "read", json!({"path": "big.txt", "offset": 3000}));
    assert!(
        out.contains("beyond end of file (2500 lines total)"),
        "{out}"
    );
}

#[test]
fn bash_stdin_is_eof_and_non_utf8_and_exit_codes() {
    let (_d, ctx) = setup();
    let t0 = std::time::Instant::now();
    let out = ok(
        &ctx,
        "bash",
        json!({"command": "read x; echo \"got:$x:$?\"; cat; echo done"}),
    );
    assert_eq!(out, "got::1\ndone\n");
    assert!(t0.elapsed() < std::time::Duration::from_secs(5));
    let out = ok(&ctx, "bash", json!({"command": "printf 'a\\xffb\\n'"}));
    assert_eq!(out, "a\u{fffd}b\n");
    let o = call(&ctx, "bash", json!({"command": "kill -9 $$"}));
    assert!(
        o.is_error && o.text.contains("[killed by signal 9]"),
        "{}",
        o.text
    );
    // Progress redraws with bare CR collapse to the final state of the line.
    let out = ok(
        &ctx,
        "bash",
        json!({"command": "printf '10%%\\r50%%\\r100%%\\n'"}),
    );
    assert_eq!(out, "100%\n");
}

#[test]
fn bash_huge_output_bounded() {
    let (_d, ctx) = setup();
    let t0 = std::time::Instant::now();
    let out = rt().block_on(crate::execute(
        "bash",
        json!({"command": "yes | head -c 10000000"}),
        &ctx,
    ));
    assert!(!out.is_error, "{}", out.text);
    assert!(out.text.len() < 60 * 1024, "{}", out.text.len());
    assert!(t0.elapsed() < std::time::Duration::from_secs(20));
    let i = out.text.find("full output saved to ").unwrap();
    let rest = &out.text[i + "full output saved to ".len()..];
    let path = &rest[..rest.find(';').unwrap()];
    assert_eq!(std::fs::metadata(path).unwrap().len(), 10_000_000);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn bash_streams_before_exit() {
    let (_d, ctx) = setup();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let ctx = crate::ToolCtx {
        progress: Some(tx),
        ..ctx
    };
    let rt = rt();
    let first_at = rt.block_on(async {
        let t0 = std::time::Instant::now();
        let run = crate::execute("bash", json!({"command": "echo one; sleep 1; echo two"}), &ctx);
        tokio::pin!(run);
        let mut first = None;
        loop {
            tokio::select! {
                out = &mut run => { assert_eq!(out.text, "one\ntwo\n"); break; }
                Some(chunk) = rx.recv() => { if first.is_none() { assert_eq!(chunk, "one\n"); first = Some(t0.elapsed()); } }
            }
        }
        first.unwrap()
    });
    assert!(
        first_at < std::time::Duration::from_millis(800),
        "{first_at:?}"
    );
}

#[test]
fn unseen_blank_lines_next_to_seen_ones_are_allowed() {
    let (d, ctx) = setup();
    put(
        d.path(),
        "p.py",
        "a = 1\n\n\ndef old():\n    pass\n\n\nb = 2\n",
    );
    let t = tag_of(&ok(&ctx, "read", json!({"path": "p.py", "limit": 5})));
    // Lines 6-7 are blank and were not displayed; deleting them with the function is fine.
    patch_ok(&ctx, &format!("[p.py#{t}]\nCUT 4.=7\n"));
    assert_eq!(get(d.path(), "p.py"), "a = 1\n\n\nb = 2\n");
    // A non-blank unseen line is still refused.
    put(d.path(), "q.txt", "1\n2\n3\n4\n");
    let t = tag_of(&ok(&ctx, "read", json!({"path": "q.txt", "limit": 2})));
    let o = edit(&ctx, &format!("[q.txt#{t}]\nCUT 2.=3\n"));
    assert!(
        o.is_error && o.text.contains("never displayed"),
        "{}",
        o.text
    );
}

#[test]
fn write_strips_pasted_read_output_but_not_yaml() {
    let (d, ctx) = setup();
    let o = ok(
        &ctx,
        "write",
        json!({"path": "y.yml", "content": "1: one\n2: two\n"}),
    );
    assert!(!o.contains("stripped"), "{o}");
    assert_eq!(get(d.path(), "y.yml"), "1: one\n2: two\n");
    let o = ok(
        &ctx,
        "write",
        json!({"path": "c.py", "content": "[c.py#ABCD]\n1:def f():\n2:    return 1\n"}),
    );
    assert!(o.contains("stripped"), "{o}");
    assert_eq!(get(d.path(), "c.py"), "def f():\n    return 1\n");
    // Latin-1 file: clear message.
    std::fs::write(d.path().join("l1.txt"), b"caf\xe9\n").unwrap();
    let e = err(&ctx, "read", json!({"path": "l1.txt"}));
    assert!(e.contains("non-UTF-8 encoding"), "{e}");
}

#[test]
fn fifo_is_refused_without_blocking() {
    let (d, ctx) = setup();
    let fifo = d.path().join("fifo");
    let c = std::ffi::CString::new(fifo.to_string_lossy().as_bytes()).unwrap();
    // SAFETY: valid NUL-terminated path.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    assert!(err(&ctx, "read", json!({"path": "fifo"})).contains("not a regular file"));
    assert!(
        err(&ctx, "write", json!({"path": "fifo", "content": "x"})).contains("not a regular file")
    );
    assert!(edit(&ctx, "[fifo#AAAA]\nPUT 1:\n+x\n")
        .text
        .contains("not a regular file"));
}

#[test]
fn unseen_replacement_verified_by_content() {
    let (d, ctx) = setup();
    put(d.path(), "s.ini", "[server]\nhost = 127.0.0.1\nport = 8080\n# worker threads\nworkers = 4\n\n[network]\ntimeout = 30\n");
    let t = tag_of(&ok(&ctx, "read", json!({"path": "s.ini", "limit": 1})));
    // Off-by-one guesses are refused: line 4 is the comment, not `workers`.
    let o = edit(&ctx, &format!("[s.ini#{t}]\nPUT 4:\n+workers = 8\n"));
    assert!(
        o.is_error && o.text.contains("never displayed"),
        "{}",
        o.text
    );
    // Insertions anchored to unseen lines are refused (nothing proves the anchor).
    let o = edit(
        &ctx,
        &format!(
            "[s.ini#{t2}]\nPUT >8:\n+retries = 3\n",
            t2 = tag_of(&ok(&ctx, "read", json!({"path": "s.ini", "limit": 1})))
        ),
    );
    assert!(o.is_error, "{}", o.text);
    // A replacement whose body is a close variant of the current line is accepted.
    let t = tag_of(&ok(&ctx, "read", json!({"path": "s.ini", "limit": 1})));
    let out = patch_ok(
        &ctx,
        &format!("[s.ini#{t}]\nPUT 5:\n+workers = 8\nPUT 3:\n+port = 9090\n"),
    );
    assert!(
        out.contains("accepted because the body rows match"),
        "{out}"
    );
    assert!(get(d.path(), "s.ini").contains("port = 9090\n# worker threads\nworkers = 8\n"));
    // Similar neighbours: the body must match the target better than the lines around it.
    put(
        d.path(),
        "c.rs",
        "fn f() {\n    one(a);\n    one(b);\n    one(c);\n}\n",
    );
    let t = tag_of(&ok(&ctx, "read", json!({"path": "c.rs", "limit": 1})));
    let o = edit(&ctx, &format!("[c.rs#{t}]\nPUT 3:\n+    one(c, x);\n"));
    assert!(o.is_error, "{}", o.text);
}

#[test]
fn wrong_files_tag_accepted_only_when_unambiguous() {
    let (d, ctx) = setup();
    put(d.path(), "a.txt", ABC);
    put(d.path(), "b.txt", "b\n");
    read_tag(&ctx, "a.txt");
    let tb = read_tag(&ctx, "b.txt");
    let out = patch_ok(&ctx, &format!("[a.txt#{tb}]\nPUT 1:\n+A\n"));
    assert!(out.contains("does not belong to a.txt"), "{out}");
    // Now a.txt has two snapshots: a foreign tag is ambiguous and refused.
    let o = edit(&ctx, &format!("[a.txt#{tb}]\nPUT 2:\n+B\n"));
    assert!(
        o.is_error && o.text.contains("was issued in this session for b.txt"),
        "{}",
        o.text
    );
    // `N.M` single-dot range.
    let t = read_tag(&ctx, "a.txt");
    patch_ok(&ctx, &format!("[a.txt#{t}]\nPUT 2.3:\n+BC\n"));
    assert!(get(d.path(), "a.txt").starts_with("A\nBC\ndelta\n"));
}
