use crate::hashline::engine::{patch_paths, run_patch};
use crate::hashline::parse::parse_patch;
use crate::util::{display_path, Args};
use crate::*;

pub struct EditTool;

const DESCRIPTION: &str = r#"Patch existing files with hashline ops (new files: `write`). Each file section starts with `[PATH#TAG]` copied from the latest `read`/`grep`/`edit` output; TAG is required and never invented. Line numbers are the ORIGINAL `N:` numbers from that output, never shifted by earlier hunks in the same patch.

Ops:
- `PUT N.=M:` replace lines N..M (inclusive; `N.=N` = one line) with the `+` rows below.
- `PUT <N:` / `PUT >N:` insert before/after line N (`PUT <1:` file head, `PUT >$:` file end).
- `CUT N.=M` delete lines; `CUT N.=M @name` also captures them.
- `PUT <N` / `PUT >N` (no colon, no body) paste the last unlabeled CUT; `PUT >N @name` / `PUT N.=M @name` paste a named register (persists across calls).
- `REM` delete the file; `MV DEST` move/rename after the section's edits.

Rules:
- Body rows are `+TEXT`, verbatim incl. indentation; lone `+` = blank line; literal leading `-`/`+` → `+- x`/`++ x`. Never send `-` rows or unchanged context; delete with CUT.
- Only touch lines you were shown in full. Keep ranges tight; split non-adjacent changes into separate hunks; pure additions use `PUT <N:`/`PUT >N:`.
- After EVERY edit the tag and numbers change: use the `[PATH#NEWTAG]` header and rows from the edit response, or re-read. On a rejection, read the message: it shows the current tag and lines.

Example:
[src/app.py#1A2B]
PUT 3.=4:
+def greet(name):
+    print(f"hi {name}")
PUT >$:
+greet("x")
[README.md#3C4D]
PUT >2:
+- new item"#;

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &str {
        "edit"
    }

    fn description(&self) -> String {
        DESCRIPTION.to_string()
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "input": {
                    "type": "string",
                    "description": "Hashline patch: one or more `[PATH#TAG]` sections with PUT/CUT/REM/MV ops."
                }
            },
            "required": ["input"],
            "additionalProperties": false
        })
    }

    fn permission_subject(&self, input: &Value) -> String {
        // Without a cwd here, report the header paths verbatim (joined).
        let args = Args::lenient(input);
        match parse_patch(args.s("input")) {
            Ok(secs) => secs.iter().map(|s| s.path.clone()).collect::<Vec<_>>().join(" "),
            Err(_) => "*".into(),
        }
    }

    fn title(&self, input: &Value) -> String {
        let args = Args::lenient(input);
        match parse_patch(args.s("input")) {
            Ok(secs) => secs.iter().map(|s| s.path.clone()).collect::<Vec<_>>().join(", "),
            Err(_) => String::new(),
        }
    }

    async fn run(&self, input: Value, ctx: &ToolCtx) -> anyhow::Result<ToolOutput> {
        let args = Args::new(&input)?;
        let patch = args.str_req("input")?.to_string();
        let paths = patch_paths(&patch, &ctx.cwd);
        let title = paths
            .iter()
            .map(|p| display_path(&ctx.cwd, p))
            .collect::<Vec<_>>()
            .join(", ");
        // Serialize concurrent edits/writes to the same files.
        let _locks = util::lock_files(&paths).await;
        if ctx.cancel.is_cancelled() {
            return Ok(ToolOutput::error("Edit cancelled.").with_title(title));
        }
        let cwd = ctx.cwd.clone();
        let store = ctx.store.clone();
        let res = tokio::task::spawn_blocking(move || run_patch(&patch, &cwd, &store)).await?;
        Ok(ToolOutput { text: res.text, is_error: res.is_error, title, images: Vec::new() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ctx, run_tool};

    async fn read(c: &ToolCtx, path: &str) -> String {
        run_tool(&crate::read::ReadTool, serde_json::json!({ "path": path }), c).await.text
    }

    async fn edit(c: &ToolCtx, input: &str) -> ToolOutput {
        run_tool(&EditTool, serde_json::json!({ "input": input }), c).await
    }

    fn tag_of(out: &str) -> String {
        let h = out.lines().next().unwrap();
        h[h.rfind('#').unwrap() + 1..h.len() - 1].to_string()
    }

    #[tokio::test]
    async fn read_edit_roundtrip_and_chain() {
        let (dir, c) = ctx();
        std::fs::write(dir.path().join("a.txt"), "one\ntwo\nthree\nfour\nfive\n").unwrap();
        let r = read(&c, "a.txt").await;
        let tag = tag_of(&r);
        let out = edit(&c, &format!("[a.txt#{tag}]\nPUT 2.=2:\n+TWO\nPUT >$:\n+six\n")).await;
        assert!(!out.is_error, "{}", out.text);
        assert_eq!(std::fs::read_to_string(dir.path().join("a.txt")).unwrap(), "one\nTWO\nthree\nfour\nfive\nsix\n");
        let new_tag = tag_of(&out.text);
        assert_ne!(new_tag, tag);
        assert!(out.text.contains("2:TWO") && out.text.contains("6:six"), "{}", out.text);
        assert_eq!(out.title, "a.txt");

        // New tag from the response works for a follow-up edit on previewed lines.
        let out2 = edit(&c, &format!("[a.txt#{new_tag}]\nCUT 6.=6\n")).await;
        assert!(!out2.is_error, "{}", out2.text);

        // Reusing the ORIGINAL tag: anchors map cleanly → session-chain recovery.
        let out3 = edit(&c, &format!("[a.txt#{tag}]\nPUT 4.=4:\n+FOUR\n")).await;
        assert!(!out3.is_error, "{}", out3.text);
        assert!(out3.text.contains("earlier in-session snapshot"), "{}", out3.text);
        assert_eq!(std::fs::read_to_string(dir.path().join("a.txt")).unwrap(), "one\nTWO\nthree\nFOUR\nfive\n");
    }

    #[tokio::test]
    async fn stale_tag_mismatch_reply() {
        let (dir, c) = ctx();
        let p = dir.path().join("a.txt");
        std::fs::write(&p, "a\nb\nc\nd\ne\nf\n").unwrap();
        let tag = tag_of(&read(&c, "a.txt").await);
        std::fs::write(&p, "a\nb\nC!\nd\ne\nf\n").unwrap();
        let out = edit(&c, &format!("[a.txt#{tag}]\nPUT 3.=3:\n+X\n")).await;
        assert!(out.is_error);
        assert!(out.text.contains("file changed between read and edit"), "{}", out.text);
        assert!(out.text.contains("*3:C!") && out.text.contains(" 1:a") && out.text.contains(" 5:e"), "{}", out.text);
        let cur = crate::hashline::file_tag("a\nb\nC!\nd\ne\nf\n");
        assert!(out.text.contains(&format!("#{cur}")));
        // Context rows were recorded as seen → retry with current tag succeeds.
        let out = edit(&c, &format!("[a.txt#{cur}]\nPUT 3.=3:\n+X\n")).await;
        assert!(!out.is_error, "{}", out.text);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "a\nb\nX\nd\ne\nf\n");
    }

    #[tokio::test]
    async fn unknown_and_missing_tag() {
        let (dir, c) = ctx();
        std::fs::write(dir.path().join("a.txt"), "a\n").unwrap();
        let out = edit(&c, "[a.txt#0000]\nPUT 1.=1:\n+b\n").await;
        assert!(out.is_error && out.text.contains("is not from this session"), "{}", out.text);
        let out = edit(&c, "[a.txt]\nPUT 1.=1:\n+b\n").await;
        assert!(out.is_error && out.text.contains("needs this file's current tag"), "{}", out.text);
        let out = edit(&c, "[missing.txt#ABCD]\nPUT 1.=1:\n+b\n").await;
        assert!(out.is_error && out.text.contains("File not found"), "{}", out.text);
    }

    #[tokio::test]
    async fn stale_bof_eof_insert_applies() {
        let (dir, c) = ctx();
        let p = dir.path().join("a.txt");
        std::fs::write(&p, "a\nb\n").unwrap();
        let tag = tag_of(&read(&c, "a.txt").await);
        std::fs::write(&p, "a\nB\n").unwrap();
        let out = edit(&c, &format!("[a.txt#{tag}]\nPUT >$:\n+z\n")).await;
        assert!(!out.is_error, "{}", out.text);
        assert!(out.text.contains("despite a stale snapshot tag"));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "a\nB\nz\n");
    }

    #[tokio::test]
    async fn external_insert_remaps() {
        let (dir, c) = ctx();
        let p = dir.path().join("a.txt");
        std::fs::write(&p, "alpha\nbeta\ngamma\ndelta\n").unwrap();
        let tag = tag_of(&read(&c, "a.txt").await);
        std::fs::write(&p, "new0\nalpha\nbeta\ngamma\ndelta\n").unwrap();
        let out = edit(&c, &format!("[a.txt#{tag}]\nPUT 3.=3:\n+GAMMA\n")).await;
        assert!(!out.is_error, "{}", out.text);
        assert!(out.text.contains("remapping stale line anchors"), "{}", out.text);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "new0\nalpha\nbeta\nGAMMA\ndelta\n");
    }

    #[tokio::test]
    async fn seen_line_guard_reveals_then_allows() {
        let (dir, c) = ctx();
        let p = dir.path().join("a.txt");
        let body: String = (1..=50).map(|i| format!("line{i}\n")).collect();
        std::fs::write(&p, &body).unwrap();
        let r = run_tool(&crate::read::ReadTool, serde_json::json!({"path": "a.txt", "offset": 1, "limit": 10}), &c).await;
        let tag = tag_of(&r.text);
        let out = edit(&c, &format!("[a.txt#{tag}]\nPUT 30.=30:\n+X\n")).await;
        assert!(out.is_error);
        assert!(out.text.contains("never displayed") && out.text.contains("  30:line30"), "{}", out.text);
        let out = edit(&c, &format!("[a.txt#{tag}]\nPUT 30.=30:\n+X\n")).await;
        assert!(!out.is_error, "{}", out.text);
        assert!(std::fs::read_to_string(&p).unwrap().contains("line29\nX\nline31"));
    }

    #[tokio::test]
    async fn noop_detected() {
        let (dir, c) = ctx();
        std::fs::write(dir.path().join("a.txt"), "a\nb\n").unwrap();
        let tag = tag_of(&read(&c, "a.txt").await);
        let p = format!("[a.txt#{tag}]\nPUT 1.=1:\n+a\n");
        let out = edit(&c, &p).await;
        assert!(out.is_error && out.text.contains("produced no change"), "{}", out.text);
        edit(&c, &p).await;
        let out = edit(&c, &p).await;
        assert!(out.text.starts_with("STOP."), "{}", out.text);
    }

    #[tokio::test]
    async fn crlf_and_bom_preserved() {
        let (dir, c) = ctx();
        let p = dir.path().join("w.txt");
        std::fs::write(&p, "\u{feff}a\r\nb\r\nc\r\n").unwrap();
        let tag = tag_of(&read(&c, "w.txt").await);
        let out = edit(&c, &format!("[w.txt#{tag}]\nPUT 2.=2:\n+B\n+B2\n")).await;
        assert!(!out.is_error, "{}", out.text);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "\u{feff}a\r\nB\r\nB2\r\nc\r\n");
    }

    #[tokio::test]
    async fn multi_section_atomic() {
        let (dir, c) = ctx();
        std::fs::write(dir.path().join("a.txt"), "a\n").unwrap();
        std::fs::write(dir.path().join("b.txt"), "b\n").unwrap();
        let ta = tag_of(&read(&c, "a.txt").await);
        let tb = tag_of(&read(&c, "b.txt").await);
        // Second section is bad → nothing written.
        let out = edit(&c, &format!("[a.txt#{ta}]\nPUT 1.=1:\n+A\n[b.txt#{tb}]\nPUT 9.=9:\n+B\n")).await;
        assert!(out.is_error && out.text.contains("No files were modified"), "{}", out.text);
        assert_eq!(std::fs::read_to_string(dir.path().join("a.txt")).unwrap(), "a\n");
        // Both good → both written, cross-file move via named register.
        let out = edit(&c, &format!("[a.txt#{ta}]\nCUT 1.=1 @x\nPUT <1:\n+A\n[b.txt#{tb}]\nPUT >1 @x\n")).await;
        assert!(!out.is_error, "{}", out.text);
        assert_eq!(std::fs::read_to_string(dir.path().join("a.txt")).unwrap(), "A\n");
        assert_eq!(std::fs::read_to_string(dir.path().join("b.txt")).unwrap(), "b\na\n");
    }

    #[tokio::test]
    async fn rem_and_mv() {
        let (dir, c) = ctx();
        std::fs::write(dir.path().join("a.txt"), "a\n").unwrap();
        std::fs::write(dir.path().join("b.txt"), "b\n").unwrap();
        let ta = tag_of(&read(&c, "a.txt").await);
        let tb = tag_of(&read(&c, "b.txt").await);
        let out = edit(&c, &format!("[a.txt#{ta}]\nPUT 1.=1:\n+A\nMV sub/c.txt\n[b.txt#{tb}]\nREM\n")).await;
        assert!(!out.is_error, "{}", out.text);
        assert!(out.text.contains("[sub/c.txt#") && out.text.contains("Moved a.txt to sub/c.txt"), "{}", out.text);
        assert!(out.text.contains("Deleted b.txt"));
        assert!(!dir.path().join("a.txt").exists() && !dir.path().join("b.txt").exists());
        assert_eq!(std::fs::read_to_string(dir.path().join("sub/c.txt")).unwrap(), "A\n");
    }

    #[tokio::test]
    async fn concurrent_edits_serialize() {
        let (dir, c) = ctx();
        let p = dir.path().join("a.txt");
        std::fs::write(&p, "x\n").unwrap();
        let tag = tag_of(&read(&c, "a.txt").await);
        let c = std::sync::Arc::new(c);
        let mut handles = Vec::new();
        for i in 0..4 {
            let c = c.clone();
            let tag = tag.clone();
            handles.push(tokio::spawn(async move {
                edit(&c, &format!("[a.txt#{tag}]\nPUT >$:\n+v{i}\n")).await
            }));
        }
        for h in handles {
            let out = h.await.unwrap();
            assert!(!out.is_error, "{}", out.text);
        }
        // All four EOF appends landed (stale-tag head/tail exception), none lost.
        let text = std::fs::read_to_string(&p).unwrap();
        assert_eq!(text.lines().count(), 5, "{text}");
    }
}
