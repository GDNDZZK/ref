//! `delete`: remove a file or directory.

use crate::util;
use crate::{ToolCtx, ToolOutput, ToolSpec};
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::path::Path;

/// Name the model calls the tool by.
pub(crate) const NAME: &str = "delete";

pub fn spec() -> ToolSpec {
    ToolSpec {
        name: NAME,
        description: "Delete a file or directory. Non-empty directories require `recursive: true`. \
Refuses to delete the working directory itself, `/` or the home directory. Symlinks are removed, not followed."
            .to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File or directory to delete."},
                "recursive": {"type": "boolean", "description": "Required to delete a non-empty directory (default false)."}
            },
            "required": ["path"]
        }),
    }
}

pub async fn run(input: Value, ctx: &ToolCtx) -> Result<ToolOutput> {
    let ctx = ctx.clone();
    util::blocking(move || run_sync(&input, &ctx)).await
}

/// Safeguards: compares both the lexical and the canonical forms of the paths.
fn is_protected_target(abs: &Path, canon: &Path, is_symlink: bool, ctx: &ToolCtx) -> bool {
    let cwd_lex = util::normalize_lexical(&ctx.cwd);
    let cwd_canon = std::fs::canonicalize(&ctx.cwd).unwrap_or_else(|_| cwd_lex.clone());
    let home = util::home_dir();
    let home_canon = home.as_deref().map(util::store_key);
    let protected = |p: &Path| {
        let is_root = p.parent().is_none();
        let is_cwd = p == cwd_lex || p == cwd_canon;
        let is_home = home.as_deref() == Some(p) || home_canon.as_deref() == Some(p);
        let contains_cwd = cwd_canon.starts_with(p);
        is_root || is_cwd || is_home || contains_cwd
    };
    protected(abs) || (!is_symlink && protected(canon))
}

/// The refusal to return when `abs` must not be deleted.
fn check_protected(
    abs: &Path,
    canon: &Path,
    is_symlink: bool,
    ctx: &ToolCtx,
) -> Option<ToolOutput> {
    is_protected_target(abs, canon, is_symlink, ctx).then(|| {
        ToolOutput::err(format!(
            "Refusing to delete {}: it is the working directory, one of its parents, the home directory or the filesystem root.",
            abs.display()
        ))
    })
}

/// Removes the target and returns what it was; `None` for a non-empty directory without `recursive`.
fn remove_target(
    abs: &Path,
    link_meta: &std::fs::Metadata,
    recursive: bool,
) -> Result<Option<&'static str>> {
    let is_symlink = link_meta.file_type().is_symlink();
    if is_symlink || !link_meta.is_dir() {
        std::fs::remove_file(abs)?;
        return Ok(Some(if is_symlink { "symlink" } else { "file" }));
    }

    let non_empty = std::fs::read_dir(abs)?.next().is_some();
    if non_empty && !recursive {
        return Ok(None);
    }
    if non_empty {
        std::fs::remove_dir_all(abs)?
    } else {
        std::fs::remove_dir(abs)?
    }
    Ok(Some("directory"))
}

fn run_sync(input: &Value, ctx: &ToolCtx) -> Result<ToolOutput> {
    let raw = util::req_str(input, "path")?;
    let (raw, _) = util::unwrap_header(&raw);
    if raw.is_empty() {
        bail!("`path` must not be empty");
    }
    let recursive = util::opt_bool(input, "recursive")?.unwrap_or(false);
    let abs = util::resolve_path(&ctx.cwd, &raw);
    let display = util::display_path(&ctx.cwd, &abs);

    let canon = util::store_key(&abs);
    let link_meta = match std::fs::symlink_metadata(&abs) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ToolOutput::err(format!("Path not found: {display}")));
        }
        Err(e) => bail!("cannot access {display}: {e}"),
    };
    let is_symlink = link_meta.file_type().is_symlink();
    if let Some(refusal) = check_protected(&abs, &canon, is_symlink, ctx) {
        return Ok(refusal);
    }

    let Some(what) = remove_target(&abs, &link_meta, recursive)? else {
        return Ok(ToolOutput::err(format!(
            "{display}/ is a non-empty directory. Pass `recursive: true` to delete it and everything inside."
        )));
    };
    ctx.store.forget(&abs);
    if canon != abs {
        ctx.store.forget(&canon);
    }
    let mut out = ToolOutput::ok(format!("Deleted {what} {display}"));
    out.meta = Some(json!({"path": abs.to_string_lossy(), "kind": what}));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_support::{ctx, temp_dir};

    #[tokio::test]
    async fn delete_file_and_dirs() {
        let d = temp_dir("delete");
        let c = ctx(&d);
        std::fs::write(d.join("f.txt"), "x\n").unwrap();
        crate::execute("read", json!({"path": "f.txt"}), &c).await;
        assert_eq!(c.store.path_count(), 1);
        let o = crate::execute("delete", json!({"path": "f.txt"}), &c).await;
        assert_eq!(o.text, "Deleted file f.txt");
        assert!(!d.join("f.txt").exists());
        assert_eq!(c.store.path_count(), 0);

        std::fs::create_dir_all(d.join("dir/sub")).unwrap();
        std::fs::write(d.join("dir/sub/x"), "").unwrap();
        let o = crate::execute("delete", json!({"path": "dir"}), &c).await;
        assert!(o.is_error && o.text.contains("recursive"));
        let o = crate::execute("delete", json!({"path": "dir", "recursive": true}), &c).await;
        assert!(!o.is_error, "{}", o.text);
        assert!(!d.join("dir").exists());

        std::fs::create_dir(d.join("empty")).unwrap();
        let o = crate::execute("delete", json!({"path": "empty"}), &c).await;
        assert_eq!(o.text, "Deleted directory empty");
        let o = crate::execute("delete", json!({"path": "missing"}), &c).await;
        assert!(o.is_error);
    }

    #[tokio::test]
    async fn safeguards() {
        let d = temp_dir("delete-guard");
        let c = ctx(&d);
        for p in [".", "/", "..", "~", d.to_str().unwrap()] {
            let o = crate::execute("delete", json!({"path": p, "recursive": true}), &c).await;
            assert!(o.is_error && o.text.contains("Refusing"), "{p}: {}", o.text);
        }
        assert!(d.exists());
    }
}
