//! Filesystem discovery: markdown agents/commands, instruction files, skills.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

/// Split `---\nyaml\n---\nbody` → (frontmatter object, body). Missing/invalid frontmatter → empty map.
pub fn parse_frontmatter(text: &str) -> (Map<String, Value>, String) {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.split_inclusive('\n');
    let first = lines.next().unwrap_or("");
    if first.trim_end() != "---" {
        return (Map::new(), text.to_string());
    }
    let mut yaml = String::new();
    let mut consumed = first.len();
    let mut closed = false;
    for line in lines {
        consumed += line.len();
        if line.trim_end() == "---" {
            closed = true;
            break;
        }
        yaml.push_str(line);
    }
    if !closed {
        return (Map::new(), text.to_string());
    }
    let body = text[consumed..].to_string();
    (parse_yaml_map(&yaml), body)
}

fn parse_yaml_map(yaml: &str) -> Map<String, Value> {
    if yaml.trim().is_empty() {
        return Map::new();
    }
    match serde_yaml::from_str::<Value>(yaml) {
        Ok(Value::Object(m)) => m,
        Ok(_) => Map::new(),
        Err(_) => {
            // Retry: quote top-level scalar values that contain ": " (common in descriptions).
            let fixed: String = yaml
                .lines()
                .map(|l| {
                    if l.starts_with(' ') || l.starts_with('#') {
                        return format!("{l}\n");
                    }
                    if let Some((k, v)) = l.split_once(':') {
                        let v = v.trim();
                        if !v.is_empty()
                            && v.contains(": ")
                            && !v.starts_with(['"', '\'', '|', '>', '[', '{'])
                        {
                            let q = serde_json::to_string(v).unwrap_or_default();
                            return format!("{k}: {q}\n");
                        }
                    }
                    format!("{l}\n")
                })
                .collect();
            match serde_yaml::from_str::<Value>(&fixed) {
                Ok(Value::Object(m)) => m,
                _ => {
                    tracing::warn!("invalid frontmatter");
                    Map::new()
                }
            }
        }
    }
}

/// All `*.md` files under `dir` recursively (sorted), with names = relative path minus `.md`.
pub fn md_files(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    fn rec(base: &Path, dir: &Path, depth: usize, out: &mut Vec<(String, PathBuf)>) {
        if depth > 6 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            let fname = p
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default();
            if fname.starts_with('.') {
                continue;
            }
            if p.is_dir() {
                rec(base, &p, depth + 1, out);
            } else if p.extension().is_some_and(|e| e == "md") {
                if let Ok(rel) = p.strip_prefix(base) {
                    let name = rel.with_extension("").to_string_lossy().replace('\\', "/");
                    out.push((name, p.clone()));
                }
            }
        }
    }
    rec(dir, dir, 0, &mut out);
    out
}

/// Config fragment `{agent: {...}, command: {...}}` from markdown files in a config directory.
/// `agent/ agents/` (`mode/ modes/` forced primary), `command/ commands/`.
pub fn md_fragment(dir: &Path) -> Option<Value> {
    let mut agents = Map::new();
    let mut commands = Map::new();
    for (sub, primary) in [
        ("agent", false),
        ("agents", false),
        ("mode", true),
        ("modes", true),
    ] {
        for (name, path) in md_files(&dir.join(sub)) {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let (mut fm, body) = parse_frontmatter(&text);
            fm.insert("prompt".into(), Value::String(body.trim().to_string()));
            if primary {
                fm.insert("mode".into(), Value::String("primary".into()));
            }
            agents.insert(name, Value::Object(fm));
        }
    }
    for sub in ["command", "commands"] {
        for (name, path) in md_files(&dir.join(sub)) {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let (mut fm, body) = parse_frontmatter(&text);
            fm.insert("template".into(), Value::String(body.trim().to_string()));
            commands.insert(name, Value::Object(fm));
        }
    }
    if agents.is_empty() && commands.is_empty() {
        return None;
    }
    let mut root = Map::new();
    if !agents.is_empty() {
        root.insert("agent".into(), Value::Object(agents));
    }
    if !commands.is_empty() {
        root.insert("command".into(), Value::Object(commands));
    }
    Some(Value::Object(root))
}

/// Directories from `cwd` up to the git root (dir containing `.git`), nearest first.
/// If no git root exists: just `cwd`.
pub fn project_dirs(cwd: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut cur = Some(cwd);
    while let Some(d) = cur {
        dirs.push(d.to_path_buf());
        if d.join(".git").exists() {
            return dirs;
        }
        cur = d.parent();
    }
    vec![cwd.to_path_buf()]
}

pub const MAX_INSTRUCTION_BYTES: u64 = 200 * 1024;

fn has_glob_meta(s: &str) -> bool {
    s.contains(['*', '?', '[', '{'])
}

/// Match a glob pattern relative to `base`; returns sorted existing files.
pub fn glob_files(base: &Path, pattern: &str) -> Vec<PathBuf> {
    if !has_glob_meta(pattern) {
        let p = base.join(pattern);
        return if p.is_file() { vec![p] } else { vec![] };
    }
    // Walk from the literal prefix directory.
    let pat_path = Path::new(pattern);
    let mut prefix = PathBuf::new();
    let mut rest = Vec::new();
    let mut in_rest = false;
    for comp in pat_path.components() {
        let s = comp.as_os_str().to_string_lossy();
        if !in_rest && !has_glob_meta(&s) {
            prefix.push(comp);
        } else {
            in_rest = true;
            rest.push(s.into_owned());
        }
    }
    let root = base.join(&prefix);
    let full_pat = root.join(rest.join("/"));
    let Ok(glob) = globset::GlobBuilder::new(&full_pat.to_string_lossy())
        .literal_separator(true)
        .build()
    else {
        tracing::warn!("invalid instructions glob {pattern:?}");
        return vec![];
    };
    let m = glob.compile_matcher();
    let mut out = Vec::new();
    fn rec(dir: &Path, depth: usize, m: &globset::GlobMatcher, out: &mut Vec<PathBuf>) {
        if depth > 10 || out.len() > 1000 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if name == ".git" || name == "node_modules" || name == "target" {
                continue;
            }
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                rec(&p, depth + 1, m, out);
            } else if m.is_match(&p) {
                out.push(p);
            }
        }
    }
    rec(&root, 0, &m, &mut out);
    out.sort();
    out
}

/// Accumulates instruction files, deduped by canonical path.
#[derive(Default)]
pub struct InstructionSet {
    seen: HashSet<PathBuf>,
    pub files: Vec<(PathBuf, String)>,
}

impl InstructionSet {
    pub fn add(&mut self, p: &Path) -> bool {
        let Ok(meta) = std::fs::metadata(p) else {
            return false;
        };
        if !meta.is_file() {
            return false;
        }
        if meta.len() > MAX_INSTRUCTION_BYTES {
            tracing::warn!("skipping instructions {} (> 200KB)", p.display());
            return false;
        }
        let canon = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        if !self.seen.insert(canon) {
            return false;
        }
        match std::fs::read_to_string(p) {
            Ok(s) => {
                self.files.push((p.to_path_buf(), s));
                true
            }
            Err(_) => false,
        }
    }
}

/// Scan a skills root: subdirectories (up to depth 4) containing SKILL.md. A dir with SKILL.md is
/// not descended further. `include_root`: also accept `root/SKILL.md`.
pub fn scan_skills(root: &Path, include_root: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if include_root && root.join("SKILL.md").is_file() {
        out.push(root.join("SKILL.md"));
        return out;
    }
    let mut frontier = vec![root.to_path_buf()];
    for _depth in 0..4 {
        let mut next = Vec::new();
        for dir in frontier {
            let Ok(rd) = std::fs::read_dir(&dir) else {
                continue;
            };
            let mut subs: Vec<PathBuf> = rd
                .flatten()
                .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect();
            subs.sort();
            for s in subs {
                let f = s.join("SKILL.md");
                if f.is_file() {
                    out.push(f);
                } else {
                    next.push(s);
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    out
}
