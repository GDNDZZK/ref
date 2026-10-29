//! System prompt assembly: agent prompt, spawnable agents, environment, instructions,
//! skills, MCP instructions. Built once per session and cached (stable bytes => prompt cache hits).

use ref_config::Config;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct PromptInput<'a> {
    pub cfg: &'a Config,
    pub agent: &'a str,
    pub cwd: &'a Path,
    pub home: Option<&'a Path>,
    pub date: &'a str,
    /// (server name, instructions) of connected MCP servers, sorted by name.
    pub mcp_instructions: Vec<(String, String)>,
}

pub fn build(inp: &PromptInput) -> Vec<String> {
    // No built-in base prompt: tool descriptions carry everything the model needs.
    let mut parts = Vec::new();
    let agent = inp.cfg.agents.get(inp.agent);
    if let Some(a) = agent
        && !a.prompt.trim().is_empty()
    {
        parts.push(a.prompt.trim().to_string());
    }
    if agent.is_some_and(|a| a.can_spawn) {
        let list: Vec<String> = inp
            .cfg
            .agents
            .values()
            .filter(|a| a.creatable && !a.hidden)
            .map(|a| format!("- {}: {}", a.name, one_line(&a.description)))
            .collect();
        if !list.is_empty() {
            parts.push(format!("Agents available for create_session:\n{}", list.join("\n")));
        }
    }
    let git = git_root(inp.cwd).is_some();
    parts.push(format!(
        "<env>\nWorking directory: {}\nPlatform: {}\nShell: bash\nGit repository: {}\nToday's date: {}\n</env>",
        inp.cwd.display(),
        std::env::consts::OS,
        if git { "yes" } else { "no" },
        inp.date
    ));
    let instr = instructions(inp.cfg, inp.cwd, inp.home);
    if !instr.is_empty() {
        parts.push(
            instr
                .iter()
                .map(|(p, c)| format!("Instructions from: {}\n{}", p.display(), c.trim_end()))
                .collect::<Vec<_>>()
                .join("\n\n"),
        );
    }
    let skills = skills(inp.cfg, inp.cwd, inp.home);
    if !skills.is_empty() {
        let mut s = String::from("<available_skills>\n");
        for sk in &skills {
            s.push_str(&format!(
                "<skill name=\"{}\" path=\"{}\">{}</skill>\n",
                xml_escape(&sk.name),
                xml_escape(&sk.path.display().to_string()),
                xml_escape(&one_line(&sk.description))
            ));
        }
        s.push_str("</available_skills>\nWhen a task matches a skill, read its SKILL.md first and follow it.");
        parts.push(s);
    }
    for (name, text) in &inp.mcp_instructions {
        parts.push(format!("<mcp_instructions server=\"{}\">\n{}\n</mcp_instructions>", xml_escape(name), text.trim()));
    }
    parts
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

pub fn git_root(cwd: &Path) -> Option<PathBuf> {
    let mut d = Some(cwd);
    while let Some(x) = d {
        if x.join(".git").exists() {
            return Some(x.to_path_buf());
        }
        d = x.parent();
    }
    None
}

/// Directories from the git root (or cwd) down to cwd.
fn project_dirs(cwd: &Path) -> Vec<PathBuf> {
    let root = git_root(cwd).unwrap_or_else(|| cwd.to_path_buf());
    let mut dirs = Vec::new();
    let mut d = Some(cwd);
    while let Some(x) = d {
        dirs.push(x.to_path_buf());
        if x == root {
            break;
        }
        d = x.parent();
    }
    dirs.reverse();
    dirs
}

fn read_text(p: &Path) -> Option<String> {
    let meta = std::fs::metadata(p).ok()?;
    if !meta.is_file() || meta.len() > 512 * 1024 {
        return None;
    }
    std::fs::read_to_string(p).ok().filter(|s| !s.trim().is_empty())
}

/// Instruction files in order: global, project (root -> cwd), configured.
pub fn instructions(cfg: &Config, cwd: &Path, home: Option<&Path>) -> Vec<(PathBuf, String)> {
    let mut out: Vec<(PathBuf, String)> = Vec::new();
    let push = |p: PathBuf, out: &mut Vec<(PathBuf, String)>| {
        let canon = std::fs::canonicalize(&p).unwrap_or(p.clone());
        if out.iter().any(|(q, _)| std::fs::canonicalize(q).unwrap_or(q.clone()) == canon) {
            return;
        }
        if let Some(t) = read_text(&p) {
            out.push((p, t));
        }
    };
    if let Some(h) = home {
        push(h.join(".config/opencode/AGENTS.md"), &mut out);
        push(h.join(".config/ref/AGENTS.md"), &mut out);
    }
    for d in project_dirs(cwd) {
        let a = d.join("AGENTS.md");
        if a.is_file() {
            push(a, &mut out);
        } else {
            push(d.join("CLAUDE.md"), &mut out);
        }
    }
    for pat in &cfg.instructions {
        if pat.starts_with("http://") || pat.starts_with("https://") {
            continue;
        }
        let expanded = crate::util::expand_home(pat, home);
        let abs = if expanded.is_absolute() { expanded } else { cwd.join(expanded) };
        let s = abs.to_string_lossy().to_string();
        if !s.contains(['*', '?', '[', '{']) {
            push(abs, &mut out);
            continue;
        }
        for p in glob_files(&s) {
            push(p, &mut out);
        }
    }
    out
}

/// Expand an absolute glob pattern into matching files (sorted).
fn glob_files(pattern: &str) -> Vec<PathBuf> {
    let Ok(glob) = globset::GlobBuilder::new(pattern).literal_separator(true).build() else {
        return Vec::new();
    };
    let m = glob.compile_matcher();
    // Base directory = longest prefix without glob characters.
    let idx = pattern.find(['*', '?', '[', '{']).unwrap_or(pattern.len());
    let base = match pattern[..idx].rfind('/') {
        Some(i) => &pattern[..i.max(1)],
        None => "/",
    };
    let mut out: Vec<PathBuf> = ignore::WalkBuilder::new(base)
        .max_depth(Some(8))
        .hidden(false)
        .build()
        .flatten()
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .map(|e| e.into_path())
        .filter(|p| m.is_match(p))
        .take(100)
        .collect();
    out.sort();
    out
}

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
}

/// Discover skills (later roots override earlier ones by name), sorted by name.
pub fn skills(cfg: &Config, cwd: &Path, home: Option<&Path>) -> Vec<Skill> {
    let mut roots: Vec<PathBuf> = Vec::new();
    let pdirs = project_dirs(cwd);
    for d in &cfg.config_dirs {
        // Plain project directories are in config_dirs too; only scan dedicated config dirs.
        if cwd.starts_with(d) {
            continue;
        }
        roots.push(d.join("skill"));
        roots.push(d.join("skills"));
    }
    if let Some(h) = home {
        roots.push(h.join(".claude/skills"));
        roots.push(h.join(".agents/skills"));
    }
    for d in &pdirs {
        for sub in [".opencode", ".ref"] {
            let c = d.join(sub);
            if !cfg.config_dirs.contains(&c) {
                roots.push(c.join("skill"));
                roots.push(c.join("skills"));
            }
        }
        roots.push(d.join(".claude/skills"));
        roots.push(d.join(".agents/skills"));
    }
    roots.extend(cfg.skill_paths.iter().cloned());
    let mut seen_roots = Vec::new();
    let mut by_name: BTreeMap<String, Skill> = BTreeMap::new();
    for r in roots {
        if !r.is_dir() || seen_roots.contains(&r) {
            continue;
        }
        seen_roots.push(r.clone());
        for s in scan_skills(&r) {
            by_name.insert(s.name.clone(), s);
        }
    }
    by_name.into_values().collect()
}

fn scan_skills(root: &Path) -> Vec<Skill> {
    let mut out = Vec::new();
    let mut queue = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = queue.pop() {
        let file = dir.join("SKILL.md");
        if file.is_file() {
            if let Some(s) = parse_skill(&file) {
                out.push(s);
            }
            continue;
        }
        if depth >= 6 {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut subs: Vec<PathBuf> = rd
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir() || t.is_symlink()))
            .map(|e| e.path())
            .filter(|p| {
                let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                !n.starts_with('.') && n != "node_modules" && p.is_dir()
            })
            .collect();
        subs.sort();
        subs.reverse();
        for s in subs {
            queue.push((s, depth + 1));
        }
    }
    out
}

fn parse_skill(file: &Path) -> Option<Skill> {
    let text = read_text(file)?;
    let (fm, _) = ref_config::split_frontmatter(&text);
    let description = fm.get("description").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if description.is_empty() {
        return None;
    }
    if fm.get("disable-model-invocation").and_then(|v| v.as_bool()) == Some(true) {
        return None;
    }
    let dir_name = file.parent().and_then(|p| p.file_name()).and_then(|n| n.to_str()).unwrap_or("skill");
    let name = fm
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(dir_name)
        .to_string();
    let path = std::fs::canonicalize(file).unwrap_or(file.to_path_buf());
    Some(Skill { name, description, path })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_parts_and_skills() {
        let dir = std::env::temp_dir().join(format!("ref-prompt-{}", crate::util::random_hex(8)));
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::create_dir_all(dir.join("sub/.agents/skills/foo")).unwrap();
        std::fs::write(dir.join("AGENTS.md"), "root rules").unwrap();
        std::fs::write(dir.join("sub/CLAUDE.md"), "sub rules").unwrap();
        std::fs::write(
            dir.join("sub/.agents/skills/foo/SKILL.md"),
            "---\nname: foo\ndescription: Does foo things\n---\nbody",
        )
        .unwrap();
        let cwd = dir.join("sub");
        let mut cfg = Config::default();
        cfg.agents.insert(
            "build".into(),
            ref_config::AgentConfig { name: "build".into(), can_spawn: true, creatable: true, description: "Builder".into(), ..Default::default() },
        );
        let parts = build(&PromptInput { cfg: &cfg, agent: "build", cwd: &cwd, home: None, date: "2026-01-02", mcp_instructions: vec![] });
        let all = parts.join("\n---\n");
        assert!(all.contains("- build: Builder"));
        assert!(all.contains("Git repository: yes"));
        assert!(all.contains("Today's date: 2026-01-02"));
        let ri = all.find("root rules").unwrap();
        let si = all.find("sub rules").unwrap();
        assert!(ri < si);
        assert!(all.contains("<skill name=\"foo\""));
        let _ = std::fs::remove_dir_all(dir);
    }
}
