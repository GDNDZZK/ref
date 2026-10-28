//! AGENTS.md / CLAUDE.md discovery + config `instructions` entries.

use crate::load::{canonical, git_root};
use crate::*;
use std::collections::HashSet;

pub(crate) fn collect(c: &Config, cwd: &Path) -> Vec<(PathBuf, String)> {
    // A hand-built Config has no discovery dirs; fall back to the real ones.
    let fallback;
    let d = if c.dirs.opencode.as_os_str().is_empty() {
        fallback = crate::load::Dirs::from_env();
        &fallback
    } else {
        &c.dirs
    };
    collect_with(c, cwd, d.home.as_deref(), &d.hernees, &d.opencode)
}

pub(crate) fn collect_with(
    c: &Config,
    cwd: &Path,
    home: Option<&Path>,
    hernees_dir: &Path,
    opencode_dir: &Path,
) -> Vec<(PathBuf, String)> {
    let cwd = canonical(cwd);
    let mut files: Vec<PathBuf> = Vec::new();

    // Global: first existing wins.
    let mut global = vec![hernees_dir.join("AGENTS.md"), opencode_dir.join("AGENTS.md")];
    if std::env::var_os("OPENCODE_DISABLE_CLAUDE_CODE_PROMPT").is_none() {
        if let Some(h) = home {
            global.push(h.join(".claude/CLAUDE.md"));
        }
    }
    if let Some(g) = global.into_iter().find(|p| p.is_file()) {
        files.push(g);
    }

    // Project: cwd up to the git root (else home, else filesystem root), farthest first.
    let stop = git_root(&cwd).or_else(|| home.map(canonical).filter(|h| cwd.starts_with(h)));
    let mut project = Vec::new();
    for d in cwd.ancestors() {
        let agents = d.join("AGENTS.md");
        let claude = d.join("CLAUDE.md");
        if agents.is_file() {
            project.push(agents);
        } else if claude.is_file() {
            project.push(claude);
        }
        if Some(d) == stop.as_deref() {
            break;
        }
    }
    project.reverse();
    files.extend(project);

    // Config `instructions`: paths / globs relative to the project root.
    let root = if c.root.as_os_str().is_empty() { cwd.clone() } else { c.root.clone() };
    for entry in &c.instructions {
        let e = entry.trim();
        if e.is_empty() || e.starts_with("http://") || e.starts_with("https://") {
            continue;
        }
        let expanded = crate::jsonc::expand_home(e, home);
        let abs = if expanded.is_absolute() { expanded } else { root.join(expanded) };
        let s = abs.to_string_lossy().into_owned();
        if s.contains(['*', '?', '[', '{']) {
            files.extend(glob(&s));
        } else if abs.is_file() {
            files.push(abs);
        }
    }

    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for f in files {
        let canon = canonical(&f);
        if !seen.insert(canon.clone()) {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&canon) {
            if !text.trim().is_empty() {
                out.push((canon, text));
            }
        }
    }
    out
}

/// Expand an absolute glob pattern (`*` does not cross `/`, `**` does). Sorted results.
fn glob(pattern: &str) -> Vec<PathBuf> {
    // Literal directory prefix before the first glob component.
    let mut base = PathBuf::from("/");
    for comp in Path::new(pattern).components().skip(1) {
        let s = comp.as_os_str().to_string_lossy();
        if s.contains(['*', '?', '[', '{']) {
            break;
        }
        base.push(comp);
    }
    let Ok(g) = globset::GlobBuilder::new(pattern).literal_separator(true).build() else { return Vec::new() };
    let m = g.compile_matcher();
    let mut out: Vec<PathBuf> = ignore::WalkBuilder::new(&base)
        .hidden(false)
        .max_depth(Some(12))
        .build()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .map(|e| e.into_path())
        .filter(|p| m.is_match(p))
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_in_order() {
        let t = crate::tests::TempDir::new("instr");
        let home = t.path().join("home");
        let proj = home.join("proj");
        let sub = proj.join("a/b");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::create_dir_all(proj.join(".git")).unwrap();
        std::fs::create_dir_all(proj.join("docs/x")).unwrap();
        std::fs::create_dir_all(home.join(".config/opencode")).unwrap();
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        let w = |p: PathBuf, s: &str| std::fs::write(p, s).unwrap();
        w(home.join(".config/opencode/AGENTS.md"), "global");
        w(home.join(".claude/CLAUDE.md"), "claude global (shadowed)");
        w(home.join("AGENTS.md"), "above git root (ignored)");
        w(proj.join("AGENTS.md"), "root");
        w(proj.join("CLAUDE.md"), "root claude (shadowed)");
        w(proj.join("a/CLAUDE.md"), "a claude");
        w(sub.join("AGENTS.md"), "b");
        w(proj.join("docs/one.md"), "doc one");
        w(proj.join("docs/x/deep.md"), "deep (not matched by *)");
        w(proj.join("CONTRIBUTING.md"), "contrib");

        let c = Config {
            root: canonical(&proj),
            instructions: vec![
                "CONTRIBUTING.md".into(),
                "docs/*.md".into(),
                "AGENTS.md".into(),
                "https://example.com/x.md".into(),
                "~/AGENTS.md".into(),
            ],
            ..Default::default()
        };
        let got = collect_with(&c, &sub, Some(&home), &home.join(".config/hernees"), &home.join(".config/opencode"));
        let contents: Vec<&str> = got.iter().map(|(_, s)| s.as_str()).collect();
        assert_eq!(contents, ["global", "root", "a claude", "b", "contrib", "doc one", "above git root (ignored)"]);

        // No git root: walk stops at home.
        std::fs::remove_dir_all(proj.join(".git")).unwrap();
        let c = Config::default();
        let got = collect_with(&c, &sub, Some(&home), &home.join("none"), &home.join("none2"));
        let contents: Vec<&str> = got.iter().map(|(_, s)| s.as_str()).collect();
        assert_eq!(contents, ["claude global (shadowed)", "above git root (ignored)", "root", "a claude", "b"]);
    }
}
