//! Integration-style tests through `execute`.

use std::path::Path;

use serde_json::json;

use crate::{builtin_defs, execute, file_hash, ToolCtx, ToolOutput, BUILTIN_TOOLS};

pub(crate) fn setup() -> (tempfile::TempDir, ToolCtx) {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ToolCtx::new(dir.path().canonicalize().unwrap());
    (dir, ctx)
}

pub(crate) fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
}

pub(crate) fn call(ctx: &ToolCtx, name: &str, input: serde_json::Value) -> ToolOutput {
    rt().block_on(execute(name, input, ctx))
}

pub(crate) fn ok(ctx: &ToolCtx, name: &str, input: serde_json::Value) -> String {
    let o = call(ctx, name, input);
    assert!(!o.is_error, "{name} failed: {}", o.text);
    o.text
}

pub(crate) fn err(ctx: &ToolCtx, name: &str, input: serde_json::Value) -> String {
    let o = call(ctx, name, input);
    assert!(o.is_error, "{name} unexpectedly succeeded: {}", o.text);
    o.text
}

pub(crate) fn put(dir: &Path, name: &str, content: &str) {
    let p = dir.join(name);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(p, content).unwrap();
}

pub(crate) fn get(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(name)).unwrap()
}

/// Tag from the first `[path#TAG]` header in `out`.
pub(crate) fn tag_of(out: &str) -> String {
    let l = out
        .lines()
        .find(|l| l.starts_with('[') || l.starts_with("## "))
        .unwrap();
    let i = l.rfind('#').unwrap();
    l[i + 1..i + 5].to_string()
}

pub(crate) fn edit(ctx: &ToolCtx, patch: &str) -> ToolOutput {
    call(ctx, "edit", json!({ "input": patch }))
}

pub(crate) const ABC: &str = "alpha\nbravo\ncharlie\ndelta\necho\nfoxtrot\ngolf\n";

#[test]
fn defs_order_and_schema() {
    let defs = builtin_defs();
    let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, BUILTIN_TOOLS);
    for d in &defs {
        assert_eq!(d.parameters["type"], "object");
        assert!(d.parameters["required"].is_array());
        assert!(d.parameters.get("additionalProperties").is_none());
    }
    assert_eq!(defs[1].parameters["required"], json!(["input"]));
    assert_eq!(
        serde_json::to_string(&builtin_defs()).unwrap(),
        serde_json::to_string(&defs).unwrap()
    );
}

#[test]
fn params_validation() {
    let (_d, ctx) = setup();
    assert_eq!(
        err(&ctx, "read", json!({})),
        "missing required parameter `path`"
    );
    assert!(err(&ctx, "read", json!({"path": 3})).contains("must be a string"));
    assert!(err(&ctx, "nope", json!({})).contains("unknown tool"));
    assert!(err(&ctx, "edit", json!({})).contains("missing required parameter `input`"));
    assert!(err(&ctx, "bash", json!({"command": "true", "timeout": -1})).contains("non-negative"));
}

#[test]
fn read_format_offset_limit() {
    let (d, ctx) = setup();
    put(d.path(), "src/a.txt", ABC);
    let out = ok(&ctx, "read", json!({"path": "src/a.txt"}));
    let tag = file_hash(ABC);
    assert_eq!(
        out,
        format!(
            "[src/a.txt#{tag}]\n1:alpha\n2:bravo\n3:charlie\n4:delta\n5:echo\n6:foxtrot\n7:golf"
        )
    );
    let out = ok(
        &ctx,
        "read",
        json!({"path": "src/a.txt", "offset": 2, "limit": 2}),
    );
    assert_eq!(
        out,
        format!("[src/a.txt#{tag}]\n2:bravo\n3:charlie\n[4 more lines. Use offset=4 to continue]")
    );
    // omp-style suffix
    let out = ok(&ctx, "read", json!({"path": "src/a.txt:6-7"}));
    assert_eq!(out, format!("[src/a.txt#{tag}]\n6:foxtrot\n7:golf"));
    // absolute path inside cwd displays relative
    let abs = d.path().canonicalize().unwrap().join("src/a.txt");
    let out = ok(
        &ctx,
        "read",
        json!({"path": abs.to_string_lossy(), "limit": 1}),
    );
    assert!(out.starts_with("[src/a.txt#"));
    put(d.path(), "empty.txt", "");
    assert_eq!(
        ok(&ctx, "read", json!({"path": "empty.txt"})),
        "[empty.txt#5D05]\n(empty file)"
    );
}

#[test]
fn read_long_line_not_seen() {
    let (d, ctx) = setup();
    let long = "x".repeat(2500);
    put(d.path(), "l.txt", &format!("short\n{long}\n"));
    let out = ok(&ctx, "read", json!({"path": "l.txt"}));
    assert!(out.contains("2:xxx") && out.contains('…') && out.contains("cut"));
    let tag = tag_of(&out);
    let e = edit(&ctx, &format!("[l.txt#{tag}]\nPUT 2:\n+y\n"));
    assert!(
        e.is_error && e.text.contains("never displayed"),
        "{}",
        e.text
    );
    // line 1 editable
    let e = edit(&ctx, &format!("[l.txt#{tag}]\nPUT 1:\n+S\n"));
    assert!(!e.is_error, "{}", e.text);
}

#[test]
fn read_dir_binary_missing() {
    let (d, ctx) = setup();
    put(d.path(), "b.txt", "x");
    put(d.path(), "sub/c.txt", "x");
    put(d.path(), ".git/HEAD", "x");
    let out = ok(&ctx, "read", json!({"path": "."}));
    assert_eq!(out, "./\nb.txt\nsub/");
    std::fs::write(d.path().join("bin.dat"), b"ab\0cd").unwrap();
    assert!(err(&ctx, "read", json!({"path": "bin.dat"})).contains("binary"));
    std::fs::write(d.path().join("bad.txt"), b"ab\xff\xfecd").unwrap();
    assert!(err(&ctx, "read", json!({"path": "bad.txt"})).contains("UTF-8"));
    put(d.path(), "config.toml", "x");
    let e = err(&ctx, "read", json!({"path": "confg.toml"}));
    assert!(
        e.contains("File not found: confg.toml") && e.contains("config.toml"),
        "{e}"
    );
}

#[test]
fn read_image() {
    let (d, mut ctx) = setup();
    std::fs::write(d.path().join("i.png"), b"\x89PNG\r\n\x1a\n0000").unwrap();
    let o = call(&ctx, "read", json!({"path": "i.png"}));
    assert!(!o.is_error && o.images.is_empty() && o.text.contains("does not accept image"));
    ctx.image_input = true;
    let o = call(&ctx, "read", json!({"path": "i.png"}));
    assert_eq!(o.images.len(), 1);
    assert_eq!(o.images[0].mime, "image/png");
    assert!(o.images[0].id.starts_with("prt_"));
}

#[test]
fn edit_ops_non_shifting() {
    let (d, ctx) = setup();
    put(d.path(), "a.txt", ABC);
    let tag = tag_of(&ok(&ctx, "read", json!({"path": "a.txt"})));
    let patch = format!(
        "*** Begin Patch\n[a.txt#{tag}]\nPUT 1:\n+ALPHA\n+ALPHA2\nCUT 3\nPUT <5:\n+pre-echo\nPUT >5:\n+post-echo\nPUT 6.=7:\n+FG\nPUT >$:\n+\n+end\n*** End Patch\n"
    );
    let o = edit(&ctx, &patch);
    assert!(!o.is_error, "{}", o.text);
    assert_eq!(
        get(d.path(), "a.txt"),
        "ALPHA\nALPHA2\nbravo\ndelta\npre-echo\necho\npost-echo\nFG\n\nend\n"
    );
    let new_tag = tag_of(&o.text);
    assert_eq!(new_tag, file_hash(&get(d.path(), "a.txt")));
    assert!(
        o.text.starts_with(&format!(
            "[a.txt#{new_tag}]\n-alpha\n1:ALPHA\n2:ALPHA2\n3:bravo\n-charlie\n4:delta"
        )),
        "{}",
        o.text
    );
    // follow-up edit with the new tag, lines shown in the preview
    let o = edit(&ctx, &format!("[a.txt#{new_tag}]\nCUT 9.=10\n"));
    assert!(!o.is_error, "{}", o.text);
    assert_eq!(
        get(d.path(), "a.txt"),
        "ALPHA\nALPHA2\nbravo\ndelta\npre-echo\necho\npost-echo\nFG\n"
    );
}

#[test]
fn edit_errors() {
    let (d, ctx) = setup();
    put(d.path(), "a.txt", ABC);
    let tag = tag_of(&ok(&ctx, "read", json!({"path": "a.txt"})));
    let o = edit(&ctx, &format!("[a.txt#{tag}]\nPUT 2.=4:\n+x\nCUT 4\n"));
    assert!(
        o.is_error && o.text.contains("anchor line 4 is already targeted"),
        "{}",
        o.text
    );
    let o = edit(&ctx, &format!("[a.txt#{tag}]\nPUT 9:\n+x\n"));
    assert!(
        o.is_error && o.text.contains("Line 9 does not exist (file has 7 lines)"),
        "{}",
        o.text
    );
    let o = edit(&ctx, &format!("[a.txt#{tag}]\nPUT 2:\n+bravo\n"));
    assert!(
        o.is_error && o.text.contains("produced no change"),
        "{}",
        o.text
    );
    let o = edit(&ctx, &format!("[new.txt#{tag}]\nPUT >$:\n+x\n"));
    assert!(o.is_error && o.text.contains("use `write`"), "{}", o.text);
    let o = edit(&ctx, "[a.txt#BEEF]\nPUT 2:\n+x\n");
    assert!(
        o.is_error && o.text.contains("hash #BEEF is not from this session"),
        "{}",
        o.text
    );
    assert!(
        o.text.contains(&format!("#{tag}")) && o.text.contains("*2:bravo"),
        "{}",
        o.text
    );
    assert_eq!(get(d.path(), "a.txt"), ABC);
}

#[test]
fn edit_mismatch_then_resend() {
    let (d, ctx) = setup();
    put(d.path(), "a.txt", ABC);
    let old_tag = tag_of(&ok(&ctx, "read", json!({"path": "a.txt"})));
    // External change to line 5 itself (the anchor) → cannot rebase.
    let changed = ABC.replace("echo", "ECHO!");
    put(d.path(), "a.txt", &changed);
    let new_tag = file_hash(&changed);
    let o = edit(&ctx, &format!("[a.txt#{old_tag}]\nPUT 5:\n+echo2\n"));
    assert!(o.is_error);
    let expected = format!(
        "Edit rejected for a.txt: file changed between read and edit.\nSection is bound to #{old_tag}, but the current file hashes to #{new_tag}. If a prior edit in this session modified this file, copy the [path#newhash] header from that edit's response; otherwise re-read the file with `read` to refresh the tag before retrying.\n\n 3:charlie\n 4:delta\n*5:ECHO!\n 6:foxtrot\n 7:golf"
    );
    assert_eq!(o.text, expected);
    // Resend with the new tag works without a re-read.
    let o = edit(&ctx, &format!("[a.txt#{new_tag}]\nPUT 5:\n+echo2\n"));
    assert!(!o.is_error, "{}", o.text);
    assert_eq!(get(d.path(), "a.txt"), ABC.replace("echo", "echo2"));
}

#[test]
fn edit_rebase_after_external_change() {
    let (d, ctx) = setup();
    put(d.path(), "a.txt", ABC);
    let old_tag = tag_of(&ok(&ctx, "read", json!({"path": "a.txt"})));
    // Insert two lines at the top externally.
    put(d.path(), "a.txt", &format!("new1\nnew2\n{ABC}"));
    let o = edit(
        &ctx,
        &format!("[a.txt#{old_tag}]\nPUT 6:\n+FOXTROT\nCUT 1\n"),
    );
    assert!(!o.is_error, "{}", o.text);
    assert!(o.text.contains("Recovered by remapping"), "{}", o.text);
    assert_eq!(
        get(d.path(), "a.txt"),
        "new1\nnew2\nbravo\ncharlie\ndelta\necho\nFOXTROT\ngolf\n"
    );
}

#[test]
fn edit_chain_with_old_tag_rebases() {
    // Second edit reusing the first read's tag after our own edit elsewhere.
    let (d, ctx) = setup();
    put(d.path(), "a.txt", ABC);
    let tag = tag_of(&ok(&ctx, "read", json!({"path": "a.txt"})));
    assert!(!edit(&ctx, &format!("[a.txt#{tag}]\nPUT <1:\n+top\n")).is_error);
    let o = edit(&ctx, &format!("[a.txt#{tag}]\nPUT 7:\n+GOLF\n"));
    assert!(!o.is_error, "{}", o.text);
    assert_eq!(
        get(d.path(), "a.txt"),
        format!("top\n{}", ABC.replace("golf", "GOLF"))
    );
}

#[test]
fn edit_unseen_guard() {
    let (d, ctx) = setup();
    put(d.path(), "a.txt", ABC);
    let tag = tag_of(&ok(&ctx, "read", json!({"path": "a.txt", "limit": 2})));
    let o = edit(&ctx, &format!("[a.txt#{tag}]\nPUT 5.=6:\n+x\n"));
    assert!(o.is_error);
    assert!(
        o.text.contains("lines 5-6 of a.txt") && o.text.contains("5:echo\n6:foxtrot\n"),
        "{}",
        o.text
    );
    assert!(o.text.contains("re-send the SAME edit unchanged"));
    let o = edit(&ctx, &format!("[a.txt#{tag}]\nPUT 5.=6:\n+x\n"));
    assert!(!o.is_error, "{}", o.text);
    assert_eq!(
        get(d.path(), "a.txt"),
        "alpha\nbravo\ncharlie\ndelta\nx\ngolf\n"
    );
}

#[test]
fn edit_crlf_bom_preserved() {
    let (d, ctx) = setup();
    std::fs::write(
        d.path().join("w.txt"),
        b"\xef\xbb\xbfone\r\ntwo\r\nthree\r\n",
    )
    .unwrap();
    let out = ok(&ctx, "read", json!({"path": "w.txt"}));
    assert!(out.contains("1:one\n2:two"));
    let tag = tag_of(&out);
    assert_eq!(tag, file_hash("one\ntwo\nthree\n"));
    let o = edit(&ctx, &format!("[w.txt#{tag}]\nPUT 2:\n+TWO\n+2b\n"));
    assert!(!o.is_error, "{}", o.text);
    assert_eq!(
        std::fs::read(d.path().join("w.txt")).unwrap(),
        b"\xef\xbb\xbfone\r\nTWO\r\n2b\r\nthree\r\n"
    );
}

#[test]
fn edit_repairs() {
    let (d, ctx) = setup();
    put(d.path(), "a.txt", ABC);
    let tag = tag_of(&ok(&ctx, "read", json!({"path": "a.txt"})));
    let o = edit(
        &ctx,
        &format!("[a.txt#{tag}]\nPUT 2.=3:\n2:B\n3:C\nPUT 5:\nE\n"),
    );
    assert!(!o.is_error, "{}", o.text);
    assert!(o.text.contains("Warnings:"));
    assert_eq!(
        get(d.path(), "a.txt"),
        "alpha\nB\nC\ndelta\nE\nfoxtrot\ngolf\n"
    );
}

#[test]
fn edit_multi_file_atomic_and_rem() {
    let (d, ctx) = setup();
    put(d.path(), "a.txt", "a\n");
    put(d.path(), "b.txt", "b\n");
    let ta = tag_of(&ok(&ctx, "read", json!({"path": "a.txt"})));
    let tb = tag_of(&ok(&ctx, "read", json!({"path": "b.txt"})));
    // second section invalid → nothing written
    let o = edit(
        &ctx,
        &format!("[a.txt#{ta}]\nPUT 1:\n+A\n[b.txt#{tb}]\nPUT 5:\n+B\n"),
    );
    assert!(o.is_error);
    assert_eq!(get(d.path(), "a.txt"), "a\n");
    let o = edit(
        &ctx,
        &format!("[a.txt#{ta}]\nPUT 1:\n+A\n[b.txt#{tb}]\nREM\n"),
    );
    assert!(!o.is_error, "{}", o.text);
    assert_eq!(get(d.path(), "a.txt"), "A\n");
    assert!(!d.path().join("b.txt").exists());
    assert!(o.text.contains("Deleted b.txt"));
}

#[test]
fn edit_large_insert_collapsed() {
    let (d, ctx) = setup();
    put(d.path(), "a.txt", "a\nb\n");
    let tag = tag_of(&ok(&ctx, "read", json!({"path": "a.txt"})));
    let body: String = (0..20).map(|i| format!("+n{i}\n")).collect();
    let o = edit(&ctx, &format!("[a.txt#{tag}]\nPUT >1:\n{body}"));
    assert!(!o.is_error, "{}", o.text);
    assert!(o.text.contains("4:n2\n…\n19:n17"), "{}", o.text);
    // collapsed lines are not seen
    let t2 = tag_of(&o.text);
    let o = edit(&ctx, &format!("[a.txt#{t2}]\nPUT 10:\n+x\n"));
    assert!(
        o.is_error && o.text.contains("never displayed"),
        "{}",
        o.text
    );
}

#[test]
fn str_replace_fallback() {
    let (d, ctx) = setup();
    put(
        d.path(),
        "f.rs",
        "fn a() {\n    let x = 1;\n    let x = 1;\n}\n",
    );
    let e = err(
        &ctx,
        "edit",
        json!({"path": "f.rs", "old_string": "let x = 1;", "new_string": "let y = 2;"}),
    );
    assert!(e.contains("matches 2 times"), "{e}");
    let o = ok(
        &ctx,
        "edit",
        json!({"path": "f.rs", "old_string": "let x = 1;", "new_string": "let y = 2;", "replace_all": true}),
    );
    assert!(o.starts_with("[f.rs#"));
    assert_eq!(
        get(d.path(), "f.rs"),
        "fn a() {\n    let y = 2;\n    let y = 2;\n}\n"
    );
    let o = ok(
        &ctx,
        "edit",
        json!({"path": "f.rs", "old_string": "  let y = 2;   \n  let y = 2;", "new_string": "  z();"}),
    );
    assert!(o.contains("ignoring trailing whitespace"), "{o}");
    assert_eq!(get(d.path(), "f.rs"), "fn a() {\n    z();\n}\n");
    let e = err(
        &ctx,
        "edit",
        json!({"path": "f.rs", "old_string": "nothing", "new_string": "x"}),
    );
    assert!(e.contains("not found"));
}

#[test]
fn write_and_delete() {
    let (d, ctx) = setup();
    let o = ok(
        &ctx,
        "write",
        json!({"path": "deep/x/new.txt", "content": "hello\n"}),
    );
    assert_eq!(
        o,
        "[deep/x/new.txt#5BF9]\nCreated: wrote 1 line (6 bytes) to deep/x/new.txt"
    );
    // editable immediately (all lines seen)
    let e = edit(&ctx, "[deep/x/new.txt#5BF9]\nPUT 1:\n+bye\n");
    assert!(!e.is_error, "{}", e.text);
    let o = ok(
        &ctx,
        "write",
        json!({"path": "deep/x/new.txt", "content": "1:a\n2:b\n"}),
    );
    assert!(o.contains("Overwrote") && o.contains("stripped"), "{o}");
    assert_eq!(get(d.path(), "deep/x/new.txt"), "a\nb\n");
    // CRLF kept
    std::fs::write(d.path().join("crlf.txt"), b"x\r\ny\r\n").unwrap();
    ok(
        &ctx,
        "write",
        json!({"path": "crlf.txt", "content": "p\nq\n"}),
    );
    assert_eq!(
        std::fs::read(d.path().join("crlf.txt")).unwrap(),
        b"p\r\nq\r\n"
    );

    assert!(err(&ctx, "delete", json!({"path": "deep"})).contains("recursive"));
    assert!(err(&ctx, "delete", json!({"path": ".", "recursive": true})).contains("Refusing"));
    assert!(err(&ctx, "delete", json!({"path": "..", "recursive": true})).contains("Refusing"));
    assert!(err(&ctx, "delete", json!({"path": "/", "recursive": true})).contains("Refusing"));
    let o = ok(&ctx, "delete", json!({"path": "deep", "recursive": true}));
    assert_eq!(o, "Deleted directory deep (1 file)");
    assert_eq!(
        ok(&ctx, "delete", json!({"path": "crlf.txt"})),
        "Deleted crlf.txt"
    );
    assert!(err(&ctx, "delete", json!({"path": "crlf.txt"})).contains("not found"));
}

#[test]
fn grep_output_and_edit_from_grep() {
    let (d, ctx) = setup();
    put(
        d.path(),
        "src/a.rs",
        "fn main() {\n    needle();\n    other();\n    needle();\n}\n",
    );
    put(d.path(), "src/b.rs", "// needle here\n");
    put(d.path(), "ignored/c.rs", "needle\n");
    put(d.path(), ".gitignore", "ignored/\n");
    put(d.path(), ".hidden/d.rs", "needle\n");
    std::fs::write(d.path().join("bin.dat"), b"needle\0").unwrap();
    let out = ok(&ctx, "grep", json!({"pattern": "needle"}));
    let ta = file_hash(&get(d.path(), "src/a.rs"));
    let tb = file_hash(&get(d.path(), "src/b.rs"));
    assert_eq!(
        out,
        format!("[src/a.rs#{ta}]\n*2:    needle();\n...\n*4:    needle();\n\n[src/b.rs#{tb}]\n*1:// needle here\n\n[3 matches in 2 files]")
    );
    let out = ok(
        &ctx,
        "grep",
        json!({"pattern": "NEEDLE", "path": "src/a.rs", "ignore_case": true, "context": 1}),
    );
    assert!(
        out.contains(" 1:fn main() {\n*2:    needle();\n 3:    other();\n*4:    needle();\n 5:}"),
        "{out}"
    );
    // Edit straight from the grep tag.
    let o = edit(&ctx, &format!("[src/a.rs#{ta}]\nPUT 4:\n+    found();\n"));
    assert!(!o.is_error, "{}", o.text);
    assert!(get(d.path(), "src/a.rs").contains("found();"));
    let out = ok(&ctx, "grep", json!({"pattern": "zzz"}));
    assert!(out.starts_with("No matches for /zzz/ in ."), "{out}");
    assert!(err(&ctx, "grep", json!({"pattern": "("})).contains("invalid regex"));
    let out = ok(&ctx, "grep", json!({"pattern": "needle", "glob": "b.*"}));
    assert!(out.contains("src/b.rs") && !out.contains("src/a.rs"));
    let out = ok(&ctx, "grep", json!({"pattern": "(", "literal": true}));
    assert!(out.contains("src/a.rs"));
}

#[test]
fn grep_limits() {
    let (d, ctx) = setup();
    let many: String = (0..50).map(|i| format!("hit {i}\n")).collect();
    put(d.path(), "a.txt", &many);
    put(d.path(), "b.txt", &many);
    let out = ok(&ctx, "grep", json!({"pattern": "hit"}));
    assert!(out.contains("[30 more matches in this file"), "{out}");
    assert!(
        out.contains("[100 matches in 2 files; showing 40]"),
        "{out}"
    );
    let out = ok(
        &ctx,
        "grep",
        json!({"pattern": "hit", "path": "a.txt", "limit": 5}),
    );
    assert!(out.contains("*5:hit 4") && !out.contains("*6:"), "{out}");
    assert!(out.contains("Match limit 5"), "{out}");
}

#[test]
fn find_files() {
    let (d, ctx) = setup();
    put(d.path(), "src/a.rs", "");
    put(d.path(), "src/deep/b.rs", "");
    put(d.path(), "README.md", "");
    put(d.path(), ".env", "");
    put(d.path(), "target/x.rs", "");
    put(d.path(), ".gitignore", "target/\n");
    let out = ok(&ctx, "find", json!({"pattern": "*.rs"}));
    let mut lines: Vec<&str> = out.lines().collect();
    lines.sort();
    assert_eq!(lines, vec!["src/a.rs", "src/deep/b.rs"]);
    let out = ok(&ctx, "find", json!({"pattern": "src/*.rs"}));
    assert_eq!(out, "src/a.rs");
    let out = ok(&ctx, "find", json!({"pattern": "src/**/*.rs", "limit": 1}));
    assert!(out.contains("[Showing 1 of 2 results"), "{out}");
    let out = ok(&ctx, "find", json!({"pattern": ".env"}));
    assert_eq!(out, ".env");
    let out = ok(&ctx, "find", json!({"type": "dir"}));
    let mut lines: Vec<&str> = out.lines().collect();
    lines.sort();
    assert_eq!(lines, vec!["src/", "src/deep/"]);
    let out = ok(&ctx, "find", json!({"path": "src"}));
    assert!(
        out.contains("src/a.rs") && out.contains("src/deep/b.rs") && !out.contains("src/deep/\n")
    );
    assert!(ok(&ctx, "find", json!({"pattern": "*.zig"})).starts_with("No files found"));
}

#[test]
fn bash_basic() {
    let (d, ctx) = setup();
    let out = ok(
        &ctx,
        "bash",
        json!({"command": "echo hi; echo err >&2; printf '\\033[31mred\\033[0m'"}),
    );
    assert_eq!(out, "hi\nerr\nred");
    let o = call(&ctx, "bash", json!({"command": "echo x; exit 3"}));
    assert!(o.is_error && o.text == "x\n\n[exit code 3]", "{}", o.text);
    assert_eq!(ok(&ctx, "bash", json!({"command": "true"})), "(no output)");
    std::fs::create_dir(d.path().join("sub")).unwrap();
    assert!(ok(&ctx, "bash", json!({"command": "pwd", "cwd": "sub"}))
        .trim_end()
        .ends_with("/sub"));
    assert_eq!(
        ok(
            &ctx,
            "bash",
            json!({"command": "echo $TERM $NO_COLOR $PAGER"})
        ),
        "dumb 1 cat\n"
    );
}

#[test]
fn bash_big_output_spills() {
    let (_d, ctx) = setup();
    let out = ok(&ctx, "bash", json!({"command": "seq 1 100000"}));
    assert!(out.len() < 60 * 1024);
    assert!(out.starts_with("1\n2\n"));
    assert!(out.trim_end().ends_with("100000"));
    let i = out.find("full output saved to ").unwrap();
    let rest = &out[i + "full output saved to ".len()..];
    let path = &rest[..rest.find(';').unwrap()];
    let full = std::fs::read_to_string(path).unwrap();
    assert_eq!(full.lines().count(), 100000);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn bash_timeout_kills_group() {
    let (d, ctx) = setup();
    let marker = d.path().join("pid");
    let start = std::time::Instant::now();
    let o = call(
        &ctx,
        "bash",
        json!({"command": format!("sleep 100 & echo $! > {}; sleep 100", marker.display()), "timeout": 1}),
    );
    assert!(
        o.is_error && o.text.contains("[timed out after 1 s"),
        "{}",
        o.text
    );
    assert!(start.elapsed() < std::time::Duration::from_secs(8));
    let pid: i32 = std::fs::read_to_string(&marker)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(!alive(pid), "background sleep survived");
}

pub(crate) fn alive(pid: i32) -> bool {
    // Zombies count as dead.
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(s) => !s.split_whitespace().nth(2).is_some_and(|st| st == "Z"),
        Err(_) => false,
    }
}

#[test]
fn bash_abort_kills_grandchild() {
    let (d, ctx) = setup();
    let marker = d.path().join("pid");
    let abort = ctx.abort.clone();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let ctx = ToolCtx {
        progress: Some(tx),
        ..ctx
    };
    let rt = rt();
    let start = std::time::Instant::now();
    let o = rt.block_on(async {
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(700)).await;
            abort.cancel();
        });
        execute("bash", json!({"command": format!("echo started; sleep 100 & echo $! > {}; sleep 100", marker.display())}), &ctx).await
    });
    assert!(
        o.is_error && o.text.contains("started") && o.text.ends_with("[aborted]"),
        "{}",
        o.text
    );
    assert!(start.elapsed() < std::time::Duration::from_secs(8));
    assert_eq!(rx.try_recv().unwrap(), "started\n");
    let pid: i32 = std::fs::read_to_string(&marker)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(!alive(pid), "background sleep survived");
}

#[test]
fn bash_background_holding_pipes() {
    let (_d, ctx) = setup();
    let start = std::time::Instant::now();
    let o = call(&ctx, "bash", json!({"command": "(sleep 5 &) ; echo done"}));
    assert!(!o.is_error, "{}", o.text);
    assert!(o.text.starts_with("done\n"));
    assert!(start.elapsed() < std::time::Duration::from_secs(4));
}
