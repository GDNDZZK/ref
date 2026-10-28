//! Skill discovery (`SKILL.md` files) and prompt rendering.

use crate::load::{canonical, Dirs};
use indexmap::IndexMap;
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    /// Path to SKILL.md
    pub path: PathBuf,
    /// False when frontmatter has `disable-model-invocation: true`: the skill stays available as
    /// a `/` command but is not advertised to the model in [`skills_prompt`].
    pub model_invocable: bool,
}

impl Skill {
    /// Directory containing SKILL.md.
    pub fn dir(&self) -> &Path {
        self.path.parent().unwrap_or(Path::new("/"))
    }
}

const MAX_DEPTH: usize = 6;
const IGNORED_DIRS: &[&str] = &["node_modules", "target", "dist", "build", "__pycache__", "vendor"];

/// Skill roots in priority order (lowest first; a later skill with the same name wins).
pub(crate) fn roots(dirs: &Dirs, walk: &[PathBuf]) -> Vec<PathBuf> {
    let compat = std::env::var_os("OPENCODE_DISABLE_CLAUDE_CODE").is_none()
        && std::env::var_os("OPENCODE_DISABLE_CLAUDE_CODE_SKILLS").is_none();
    let mut v = Vec::new();
    if let Some(h) = &dirs.home {
        if compat {
            v.push(h.join(".claude/skills"));
        }
        v.push(h.join(".agents/skills"));
    }
    for g in dirs.global_dirs() {
        v.push(g.join("skill"));
        v.push(g.join("skills"));
    }
    for d in walk {
        if compat {
            v.push(d.join(".claude/skills"));
        }
        v.push(d.join(".agents/skills"));
        for sub in [".opencode", ".hernees"] {
            v.push(d.join(sub).join("skill"));
            v.push(d.join(sub).join("skills"));
        }
    }
    v
}

pub(crate) fn discover(dirs: &Dirs, walk: &[PathBuf]) -> Vec<Skill> {
    let mut seen_roots = HashSet::new();
    let mut by_name: IndexMap<String, Skill> = IndexMap::new();
    for root in roots(dirs, walk) {
        if !root.is_dir() || !seen_roots.insert(canonical(&root)) {
            continue;
        }
        for file in scan(&root) {
            if let Some(s) = read_skill(&file) {
                by_name.shift_remove(&s.name);
                by_name.insert(s.name.clone(), s);
            }
        }
    }
    let mut v: Vec<Skill> = by_name.into_values().collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

/// BFS for `SKILL.md`; a dir containing one is not descended into.
fn scan(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut q = VecDeque::from([(root.to_path_buf(), 0usize)]);
    while let Some((dir, depth)) = q.pop_front() {
        let skill = dir.join("SKILL.md");
        if skill.is_file() {
            out.push(skill);
            continue;
        }
        if depth >= MAX_DEPTH {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut subs: Vec<PathBuf> = rd
            .filter_map(|e| e.ok())
            .filter(|e| {
                let n = e.file_name();
                let n = n.to_string_lossy();
                !n.starts_with('.') && !IGNORED_DIRS.contains(&n.as_ref())
            })
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        subs.sort();
        q.extend(subs.into_iter().map(|p| (p, depth + 1)));
    }
    out
}

fn read_skill(path: &Path) -> Option<Skill> {
    let text = std::fs::read_to_string(path).ok()?;
    let (fm, _) = crate::markdown::parse(&text).ok()?;
    let description = fm.get("description").and_then(|d| d.as_str()).map(str::trim).unwrap_or("");
    if description.is_empty() {
        return None;
    }
    let name = fm
        .get("name")
        .and_then(|n| n.as_str())
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_string)
        .or_else(|| path.parent()?.file_name().map(|n| n.to_string_lossy().into_owned()))?;
    let model_invocable = !fm.get("disable-model-invocation").and_then(|v| v.as_bool()).unwrap_or(false);
    Some(Skill { name, description: description.to_string(), path: path.to_path_buf(), model_invocable })
}

/// Up to `limit` files in the skill dir (recursive, sorted, excluding SKILL.md and ignored dirs).
fn skill_files(dir: &Path, limit: usize) -> Vec<PathBuf> {
    let mut files = Vec::new();
    fn rec(d: &Path, depth: usize, files: &mut Vec<PathBuf>) {
        if depth > MAX_DEPTH || files.len() > 1000 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd.filter_map(|e| e.ok()) {
            let p = e.path();
            let n = e.file_name().to_string_lossy().into_owned();
            if p.is_dir() {
                if n != ".git" && !IGNORED_DIRS.contains(&n.as_str()) {
                    rec(&p, depth + 1, files);
                }
            } else if p.is_file() {
                files.push(p);
            }
        }
    }
    rec(dir, 0, &mut files);
    let skill_md = dir.join("SKILL.md");
    files.retain(|f| *f != skill_md);
    files.sort();
    files.truncate(limit);
    files
}

/// The `<skill_content>` block returned by the skill tool / injected by `/skill-name`.
pub fn skill_content(s: &Skill) -> String {
    let body = std::fs::read_to_string(&s.path)
        .ok()
        .map(|t| match crate::markdown::parse(&t) {
            Ok((_, body)) => body,
            Err(_) => t,
        })
        .unwrap_or_default();
    let dir = s.dir();
    let mut lines = vec![
        format!("<skill_content name=\"{}\">", s.name),
        format!("# Skill: {}", s.name),
        String::new(),
        body.trim().to_string(),
        String::new(),
        format!("Base directory for this skill: {}", dir.display()),
        "Relative paths in this skill (e.g., scripts/, reference/) are relative to this base directory.".into(),
        "Note: file list is sampled.".into(),
        String::new(),
        "<skill_files>".into(),
    ];
    for f in skill_files(dir, 10) {
        lines.push(format!("<file>{}</file>", f.display()));
    }
    lines.push("</skill_files>".into());
    lines.push("</skill_content>".into());
    lines.join("\n")
}

/// System-prompt block advertising skills; empty string when there are none.
pub fn skills_prompt(skills: &[Skill]) -> String {
    let mut listed: Vec<&Skill> = skills.iter().filter(|s| s.model_invocable).collect();
    if listed.is_empty() {
        return String::new();
    }
    listed.sort_by(|a, b| a.name.cmp(&b.name));
    let mut lines = vec![
        "Skills provide specialized instructions and workflows for specific tasks.".to_string(),
        "Use the skill tool to load a skill when a task matches its description.".into(),
        "The user may also invoke a skill directly. When that happens, its instructions appear in the conversation as a <skill_content> block, the same shape the skill tool returns. A skill that is already present this way does not need to be invoked again.".into(),
        "<available_skills>".into(),
    ];
    for s in listed {
        lines.push("  <skill>".into());
        lines.push(format!("    <name>{}</name>", s.name));
        lines.push(format!("    <description>{}</description>", s.description));
        lines.push("  </skill>".into());
    }
    lines.push("</available_skills>".into());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_and_render() {
        let t = crate::tests::TempDir::new("skills");
        let home = t.path().join("home");
        let proj = t.path().join("proj");
        let w = |p: PathBuf, s: &str| {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, s).unwrap();
        };
        w(home.join(".claude/skills/a/SKILL.md"), "---\nname: alpha\ndescription: global alpha\n---\nGlobal body");
        w(home.join(".config/opencode/skills/x/y/SKILL.md"), "---\ndescription: nested, name from dir\n---\nY");
        w(home.join(".config/opencode/skills/nodesc/SKILL.md"), "---\nname: nodesc\n---\nno description");
        w(proj.join(".opencode/skill/a2/SKILL.md"), "---\nname: alpha\ndescription: project alpha\n---\n\n  Project body\n");
        w(proj.join(".opencode/skill/a2/scripts/run.sh"), "echo");
        w(proj.join(".opencode/skill/a2/ref.md"), "ref");
        w(proj.join(".agents/skills/hidden/SKILL.md"), "---\ndescription: h\ndisable-model-invocation: true\n---\n");
        std::fs::create_dir_all(proj.join(".git")).unwrap();
        let dirs = Dirs {
            home: Some(home.clone()),
            opencode: home.join(".config/opencode"),
            hernees: home.join(".config/hernees"),
            ..Default::default()
        };
        let walk = crate::load::walk_dirs(&proj, Some(&proj));
        let skills = discover(&dirs, &walk);
        let names: Vec<_> = skills.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["alpha", "hidden", "y"]);
        let alpha = &skills[0];
        assert_eq!(alpha.description, "project alpha");

        let c = skill_content(alpha);
        assert!(c.starts_with("<skill_content name=\"alpha\">\n# Skill: alpha\n\nProject body\n\nBase directory for this skill: "));
        assert!(c.contains("/ref.md</file>"));
        assert!(c.contains("/scripts/run.sh</file>"));
        assert!(!c.contains("SKILL.md</file>"));

        let p = skills_prompt(&skills);
        assert!(p.contains("<name>alpha</name>"));
        assert!(p.contains("<description>nested, name from dir</description>"));
        assert!(!p.contains("hidden"));
        assert_eq!(skills_prompt(&[]), "");
    }
}
