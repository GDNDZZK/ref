//! Config file discovery, substitution, layering and merging.

use crate::markdown;
use crate::{Config, PermissionRules, parse};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Global config file names tried (in order) in every global config dir.
const GLOBAL_FILES: &[&str] = &[
    "opencode.json",
    "opencode.jsonc",
    "config.json",
    "ref.json",
    "ref.jsonc",
];

/// Environment lookup used during loading (injectable for tests).
pub type EnvFn<'a> = &'a dyn Fn(&str) -> Option<String>;

/// Load config for `cwd` using the real home directory and process environment.
pub fn load(cwd: &Path) -> Config {
    let home = dirs::home_dir();
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|h| h.join(".config")));
    let mut global = Vec::new();
    if let Some(dir) = std::env::var_os("OPENCODE_CONFIG_DIR").filter(|v| !v.is_empty()) {
        global.push(PathBuf::from(dir));
    } else if let Some(b) = &base {
        global.push(b.join("opencode"));
    }
    if let Some(b) = &base {
        global.push(b.join("ref"));
    }
    let env = |k: &str| std::env::var(k).ok();
    load_with(home.as_deref(), &global, cwd, &env)
}

/// One config document (a JSON file or a converted markdown file).
pub(crate) struct Layer {
    pub source: PathBuf,
    pub value: Map<String, Value>,
}

/// Loading context: environment, home dir, warnings sink.
pub(crate) struct Ctx<'a> {
    pub home: Option<&'a Path>,
    pub env: EnvFn<'a>,
    pub warnings: Vec<String>,
}

impl Ctx<'_> {
    pub fn warn(&mut self, msg: impl Into<String>) {
        self.warnings.push(msg.into());
    }
    pub fn env_nonempty(&self, k: &str) -> Option<String> {
        (self.env)(k).filter(|v| !v.trim().is_empty())
    }
}

/// Testable loader. `home` is used for `~` expansion; `global_dirs` are the global config
/// directories (e.g. `~/.config/opencode`, `~/.config/ref`); `env` provides env vars.
pub fn load_with(home: Option<&Path>, global_dirs: &[PathBuf], cwd: &Path, env: EnvFn) -> Config {
    load_in(home, global_dirs, None, cwd, env)
}

/// Like [`load_with`], but the project starts at `project_root` (an ancestor of `cwd`, or
/// `cwd` itself) instead of the git worktree root, so nothing above it is read.
pub fn load_in(
    home: Option<&Path>,
    global_dirs: &[PathBuf],
    project_root: Option<&Path>,
    cwd: &Path,
    env: EnvFn,
) -> Config {
    let mut ctx = Ctx {
        home,
        env,
        warnings: Vec::new(),
    };
    let mut layers: Vec<Layer> = Vec::new();
    let mut config_dirs: Vec<PathBuf> = Vec::new();

    // 1-2. Global dirs.
    for dir in global_dirs {
        if !dir.is_dir() {
            continue;
        }
        push_dir(&mut config_dirs, dir);
        for f in GLOBAL_FILES {
            load_file(&mut ctx, &dir.join(f), &mut layers);
        }
        load_markdown(&mut ctx, dir, &mut layers);
    }

    // 3. Explicit config files.
    for var in ["OPENCODE_CONFIG", "REF_CONFIG"] {
        if let Some(p) = ctx.env_nonempty(var) {
            let p = expand_home(&p, home, cwd);
            if p.is_file() {
                load_file(&mut ctx, &p, &mut layers);
            } else {
                ctx.warn(format!("{var}={}: file not found", p.display()));
            }
        }
    }

    // 4. Project directories, root-most first.
    for dir in project_dirs(cwd, project_root) {
        push_dir(&mut config_dirs, &dir);
        // Order: opencode.json(c), .opencode/opencode.json(c), ref.json(c), .ref/ref.json(c).
        for (stem, sub) in [("opencode", ".opencode"), ("ref", ".ref")] {
            let names = [format!("{stem}.json"), format!("{stem}.jsonc")];
            for n in &names {
                load_file(&mut ctx, &dir.join(n), &mut layers);
            }
            let d = dir.join(sub);
            if !d.is_dir() || global_dirs.iter().any(|g| same_dir(g, &d)) {
                continue;
            }
            push_dir(&mut config_dirs, &d);
            for n in names {
                load_file(&mut ctx, &d.join(n), &mut layers);
            }
            load_markdown(&mut ctx, &d, &mut layers);
        }
    }

    let mut cfg = parse::build(&mut ctx, &layers);
    cfg.config_dirs = config_dirs;
    cfg.project_root = project_root.map(Path::to_path_buf);
    cfg.warnings = ctx.warnings;
    cfg
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

fn push_dir(list: &mut Vec<PathBuf>, d: &Path) {
    if !list.iter().any(|x| x == d) {
        list.push(d.to_path_buf());
    }
}

/// Directories from the project root down to `cwd`. The root is `project_root` if given, else
/// the git worktree root (nearest ancestor containing `.git`); just `[cwd]` when there is none.
pub(crate) fn project_dirs(cwd: &Path, project_root: Option<&Path>) -> Vec<PathBuf> {
    let cwd = if cwd.is_absolute() {
        cwd.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|d| d.join(cwd))
            .unwrap_or_else(|_| cwd.to_path_buf())
    };
    let mut up = Vec::new();
    let mut found_root = false;
    for anc in cwd.ancestors() {
        up.push(anc.to_path_buf());
        let is_root = match project_root {
            Some(root) => anc == root,
            None => anc.join(".git").exists(),
        };
        if is_root {
            found_root = true;
            break;
        }
    }
    if !found_root {
        return vec![cwd];
    }
    up.reverse();
    up
}

fn load_file(ctx: &mut Ctx, path: &Path, layers: &mut Vec<Layer>) {
    if !path.is_file() {
        return;
    }
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => return ctx.warn(format!("{}: {e}", path.display())),
    };
    let value = match crate::parse_jsonc(&text) {
        Ok(Value::Object(m)) => m,
        Ok(_) => return ctx.warn(format!("{}: config must be a JSON object", path.display())),
        Err(e) => return ctx.warn(format!("{}: {e}", path.display())),
    };
    let base = path.parent().unwrap_or(Path::new("/")).to_path_buf();
    let mut v = Value::Object(value);
    substitute_value(ctx, &mut v, &base, path);
    let Value::Object(mut value) = v else {
        unreachable!()
    };
    normalize_paths(ctx, &mut value, &base);
    layers.push(Layer {
        source: path.to_path_buf(),
        value,
    });
}

/// Markdown agents/commands of one config directory, converted to config layers.
fn load_markdown(ctx: &mut Ctx, dir: &Path, layers: &mut Vec<Layer>) {
    let (agents, w1) = markdown::collect(dir, &["agent", "agents", "mode", "modes"]);
    let (commands, w2) = markdown::collect(dir, &["command", "commands"]);
    ctx.warnings.extend(w1);
    ctx.warnings.extend(w2);
    for doc in agents {
        let base = doc.path.parent().unwrap_or(dir).to_path_buf();
        let mut fm = Value::Object(doc.frontmatter);
        substitute_value(ctx, &mut fm, &base, &doc.path);
        let Value::Object(mut fm) = fm else {
            unreachable!()
        };
        fm.remove("name");
        fm.insert("prompt".into(), Value::String(doc.body));
        let key = if doc.legacy_mode { "mode" } else { "agent" };
        let mut entry = Map::new();
        entry.insert(doc.name, Value::Object(fm));
        let mut value = Map::new();
        value.insert(key.into(), Value::Object(entry));
        layers.push(Layer {
            source: doc.path,
            value,
        });
    }
    for doc in commands {
        let base = doc.path.parent().unwrap_or(dir).to_path_buf();
        let mut fm = Value::Object(doc.frontmatter);
        substitute_value(ctx, &mut fm, &base, &doc.path);
        let Value::Object(mut fm) = fm else {
            unreachable!()
        };
        fm.insert("template".into(), Value::String(doc.body));
        let mut entry = Map::new();
        entry.insert(doc.name, Value::Object(fm));
        let mut value = Map::new();
        value.insert("command".into(), Value::Object(entry));
        layers.push(Layer {
            source: doc.path,
            value,
        });
    }
}

/// Make `skills` paths absolute relative to the config file's directory.
fn normalize_paths(ctx: &mut Ctx, v: &mut Map<String, Value>, base: &Path) {
    let Some(skills) = v.get_mut("skills") else {
        return;
    };
    let list = match skills {
        Value::Array(a) => a.clone(),
        Value::String(_) => vec![skills.clone()],
        Value::Object(o) => match o.get("paths") {
            Some(Value::Array(a)) => a.clone(),
            Some(s @ Value::String(_)) => vec![s.clone()],
            _ => vec![],
        },
        _ => vec![],
    };
    let mut out = Vec::new();
    for item in list {
        match item.as_str() {
            Some(s) if !s.trim().is_empty() => out.push(Value::String(
                expand_home(s.trim(), ctx.home, base)
                    .to_string_lossy()
                    .into_owned(),
            )),
            _ => ctx.warn(format!("invalid skills path entry {item}")),
        }
    }
    *skills = Value::Array(out);
}

/// Expand `~` / `~/x` and resolve relative paths against `base`.
pub(crate) fn expand_home(p: &str, home: Option<&Path>, base: &Path) -> PathBuf {
    if let Some(h) = home {
        if p == "~" {
            return h.to_path_buf();
        }
        if let Some(rest) = p.strip_prefix("~/") {
            return h.join(rest);
        }
    }
    let pb = PathBuf::from(p);
    if pb.is_absolute() { pb } else { base.join(pb) }
}

/// Apply `{env:VAR}` / `{file:path}` substitution to every string in `v`.
pub(crate) fn substitute_value(ctx: &mut Ctx, v: &mut Value, base: &Path, source: &Path) {
    match v {
        Value::String(s) => {
            if s.contains("{env:") || s.contains("{file:") {
                *s = substitute(ctx, s, base, source);
            }
        }
        Value::Array(a) => a
            .iter_mut()
            .for_each(|x| substitute_value(ctx, x, base, source)),
        Value::Object(m) => m
            .values_mut()
            .for_each(|x| substitute_value(ctx, x, base, source)),
        _ => {}
    }
}

/// Substitute `{env:VAR}` (env value or "") and `{file:path}` (trimmed contents or "").
pub(crate) fn substitute(ctx: &mut Ctx, s: &str, base: &Path, source: &Path) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    loop {
        let next = [rest.find("{env:"), rest.find("{file:")]
            .into_iter()
            .flatten()
            .min();
        let Some(start) = next else {
            out.push_str(rest);
            break;
        };
        let Some(end_rel) = rest[start..].find('}') else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..start]);
        let token = &rest[start + 1..start + end_rel];
        if let Some(var) = token.strip_prefix("env:") {
            out.push_str(&(ctx.env)(var.trim()).unwrap_or_default());
        } else if let Some(p) = token.strip_prefix("file:") {
            let path = expand_home(p.trim(), ctx.home, base);
            match std::fs::read_to_string(&path) {
                Ok(t) => out.push_str(t.trim()),
                Err(e) => ctx.warn(format!("{}: {{file:{}}}: {e}", source.display(), p.trim())),
            }
        }
        rest = &rest[start + end_rel + 1..];
    }
    out
}

/// Deep merge `src` into `dst`: objects merged recursively, everything else replaced.
pub(crate) fn deep_merge(dst: &mut Map<String, Value>, src: &Map<String, Value>) {
    for (k, v) in src {
        match (dst.get_mut(k), v) {
            (Some(Value::Object(d)), Value::Object(s)) => deep_merge(d, s),
            _ => {
                dst.insert(k.clone(), v.clone());
            }
        }
    }
}

/// Rules from one document's `tools` (legacy) then `permission` keys.
pub(crate) fn layer_rules(
    ctx: &mut Ctx,
    m: &Map<String, Value>,
    source: &Path,
    what: &str,
) -> PermissionRules {
    let mut rules = PermissionRules::default();
    if let Some(t) = m.get("tools") {
        rules.extend(PermissionRules::from_tools_map(t));
    }
    if let Some(p) = m.get("permission") {
        let (r, w) = PermissionRules::from_value(p);
        for x in w {
            ctx.warn(format!("{}: {what}: {x}", source.display()));
        }
        rules.extend(r);
    }
    rules
}

/// Accumulated per-document data that must not be deep-merged.
#[derive(Default)]
pub(crate) struct Accum {
    pub permission: PermissionRules,
    pub agent_permission: BTreeMap<String, PermissionRules>,
    pub instructions: Vec<String>,
    pub skills: Vec<PathBuf>,
}

pub(crate) fn accumulate(ctx: &mut Ctx, layers: &[Layer]) -> (Map<String, Value>, Accum) {
    let mut merged = Map::new();
    let mut acc = Accum::default();
    for l in layers {
        acc.permission
            .extend(layer_rules(ctx, &l.value, &l.source, "permission"));
        for key in ["mode", "agent"] {
            if let Some(Value::Object(agents)) = l.value.get(key) {
                for (name, a) in agents {
                    if let Value::Object(a) = a {
                        let r = layer_rules(ctx, a, &l.source, &format!("agent {name}"));
                        acc.agent_permission
                            .entry(name.clone())
                            .or_default()
                            .extend(r);
                    }
                }
            }
        }
        match l.value.get("instructions") {
            Some(Value::Array(a)) => {
                for x in a {
                    match x.as_str() {
                        Some(s) if !acc.instructions.iter().any(|y| y == s) => {
                            acc.instructions.push(s.to_string())
                        }
                        Some(_) => {}
                        None => ctx.warn(format!(
                            "{}: invalid instructions entry {x}",
                            l.source.display()
                        )),
                    }
                }
            }
            Some(Value::String(s)) if !acc.instructions.contains(s) => {
                acc.instructions.push(s.clone())
            }
            Some(Value::String(_)) | None => {}
            Some(_) => ctx.warn(format!(
                "{}: `instructions` must be an array of strings",
                l.source.display()
            )),
        }
        if let Some(Value::Array(a)) = l.value.get("instructions") {
            // `~` expansion only; relative entries are resolved later against the project.
            for s in a.iter().filter_map(Value::as_str) {
                if s.starts_with('~') {
                    let e = expand_home(s, ctx.home, Path::new(""))
                        .to_string_lossy()
                        .into_owned();
                    if let Some(x) = acc.instructions.iter_mut().find(|x| x.as_str() == s) {
                        *x = e;
                    }
                }
            }
        }
        if let Some(Value::Array(a)) = l.value.get("skills") {
            for p in a.iter().filter_map(|x| x.as_str()) {
                let p = PathBuf::from(p);
                if !acc.skills.contains(&p) {
                    acc.skills.push(p);
                }
            }
        }
        deep_merge(&mut merged, &l.value);
    }
    (merged, acc)
}
