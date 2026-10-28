use crate::util::{self, canonical_key, display_path, resolve_path, Args};
use crate::*;

pub struct DeleteTool;

const DESCRIPTION: &str = "Delete a file, or a directory with `recursive: true`. Refuses `/`, the home directory, and the working directory itself.";

fn count_entries(dir: &std::path::Path) -> (usize, usize) {
    let (mut files, mut dirs) = (0, 0);
    for e in ignore::WalkBuilder::new(dir).standard_filters(false).build().flatten() {
        if e.depth() == 0 {
            continue;
        }
        if e.file_type().is_some_and(|t| t.is_dir()) {
            dirs += 1;
        } else {
            files += 1;
        }
    }
    (files, dirs)
}

#[async_trait]
impl Tool for DeleteTool {
    fn name(&self) -> &str {
        "delete"
    }

    fn description(&self) -> String {
        DESCRIPTION.to_string()
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File or directory to delete." },
                "recursive": { "type": "boolean", "description": "Required to delete a non-empty directory." }
            },
            "required": ["path"],
            "additionalProperties": false
        })
    }

    fn permission_subject(&self, input: &Value) -> String {
        util::expand_tilde(Args::lenient(input).s("path")).to_string_lossy().to_string()
    }

    fn title(&self, input: &Value) -> String {
        Args::lenient(input).s("path").to_string()
    }

    async fn run(&self, input: Value, ctx: &ToolCtx) -> anyhow::Result<ToolOutput> {
        let args = Args::new(&input)?;
        let raw = args.str_req("path")?;
        let recursive = args.bool_opt("recursive")?.unwrap_or(false);
        let abs = resolve_path(&ctx.cwd, raw);
        let display = display_path(&ctx.cwd, &abs);
        let canon = canonical_key(&abs);
        // Refuse `/`, home, cwd, and anything containing home or cwd (e.g. `..`).
        let protected = [util::home_dir().map(|h| canonical_key(&h)), Some(canonical_key(&ctx.cwd))];
        if abs.parent().is_none() || protected.iter().flatten().any(|p| p.starts_with(&canon)) {
            return Ok(ToolOutput::error(format!(
                "Refusing to delete {}: protected path (root, home, the working directory, or one of their parents).",
                abs.display()
            ))
            .with_title(display));
        }
        let meta = match std::fs::symlink_metadata(&abs) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ToolOutput::error(format!("Not found: {display}")).with_title(display));
            }
            Err(e) => return Ok(ToolOutput::error(format!("Cannot access {display}: {e}")).with_title(display)),
        };
        let _lock = util::lock_files(std::slice::from_ref(&abs)).await;
        let text = if meta.is_dir() {
            let (files, dirs) = count_entries(&abs);
            if files + dirs > 0 && !recursive {
                return Ok(ToolOutput::error(format!(
                    "{display} is a directory with {files} file(s) and {dirs} subdirectorie(s). Pass recursive=true to delete it."
                ))
                .with_title(display));
            }
            let a = abs.clone();
            if let Err(e) = tokio::task::spawn_blocking(move || std::fs::remove_dir_all(&a)).await? {
                return Ok(ToolOutput::error(format!("Failed to delete {display}: {e}")).with_title(display));
            }
            format!("Deleted directory {display}/ ({files} file(s), {dirs} subdirectorie(s)).")
        } else {
            if let Err(e) = tokio::fs::remove_file(&abs).await {
                return Ok(ToolOutput::error(format!("Failed to delete {display}: {e}")).with_title(display));
            }
            ctx.store.invalidate(&canon);
            if meta.file_type().is_symlink() {
                format!("Deleted symlink {display}.")
            } else {
                format!("Deleted {display} ({}).", util::human_size(meta.len()))
            }
        };
        Ok(ToolOutput::text(text).with_title(display))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ctx, run_tool};
    use serde_json::json;

    #[tokio::test]
    async fn delete_files_and_dirs() {
        let (dir, c) = ctx();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        std::fs::create_dir_all(dir.path().join("d/e")).unwrap();
        std::fs::write(dir.path().join("d/e/f.txt"), "x").unwrap();
        let out = run_tool(&DeleteTool, json!({"path": "a.txt"}), &c).await;
        assert_eq!(out.text, "Deleted a.txt (1B).");
        let out = run_tool(&DeleteTool, json!({"path": "d"}), &c).await;
        assert!(out.is_error && out.text.contains("recursive=true"), "{}", out.text);
        let out = run_tool(&DeleteTool, json!({"path": "d", "recursive": true}), &c).await;
        assert!(!out.is_error && out.text.contains("1 file(s), 1 subdir"), "{}", out.text);
        assert!(!dir.path().join("d").exists());
        for p in [".", "/", "~", "..", "./"] {
            let out = run_tool(&DeleteTool, json!({"path": p, "recursive": true}), &c).await;
            assert!(out.is_error && out.text.contains("Refusing"), "{p}: {}", out.text);
        }
        let out = run_tool(&DeleteTool, json!({"path": "nope"}), &c).await;
        assert!(out.is_error && out.text.contains("Not found"));
    }
}
