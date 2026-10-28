use std::fs;
use std::path::Path;

use ref_proto::spec::ApiKind;
use serde_json::json;

use super::*;

struct Env {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    paths: ConfigPaths,
}

impl Env {
    fn new() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        let home = root.join("home");
        fs::create_dir_all(&home).unwrap();
        let paths = ConfigPaths {
            global_dir: home.join(".config/opencode"),
            ref_global_dir: home.join(".config/ref"),
            catalog_path: home.join(".cache/opencode/models.json"),
            auth_path: home.join(".local/share/opencode/auth.json"),
            home,
            ..Default::default()
        };
        Env {
            _tmp: tmp,
            root,
            paths,
        }
    }

    fn write(&self, rel: impl AsRef<Path>, content: &str) -> PathBuf {
        let p = self.root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, content).unwrap();
        p
    }

    /// Project dir `proj` with `.git`, cwd `proj/sub`.
    fn project(&self) -> PathBuf {
        fs::create_dir_all(self.root.join("proj/.git")).unwrap();
        fs::create_dir_all(self.root.join("proj/sub")).unwrap();
        self.root.join("proj/sub")
    }

    fn load(&self, cwd: &Path) -> Config {
        Config::load_with(&self.paths, cwd).unwrap()
    }
}

#[test]
fn jsonc_comments_and_trailing_commas() {
    let src = r#"{
        // line comment
        "a": "http://x//y", /* block
        comment */ "b": [1, 2, 3,],
        "c": {"d": "a,}", "e": "q\"//",},
    }"#;
    let v: Value = serde_json::from_str(&jsonc::strip_jsonc(src)).unwrap();
    assert_eq!(
        v,
        json!({"a": "http://x//y", "b": [1,2,3], "c": {"d": "a,}", "e": "q\"//"}})
    );
}

#[test]
fn env_and_file_substitution() {
    let mut e = Env::new();
    e.paths.env.insert("MY_KEY".into(), "sk-test".into());
    e.write("home/.config/opencode/prompt.txt", "  line1\n\"quoted\"\n ");
    e.write(
        "home/.config/opencode/opencode.json",
        r#"{
  // "x": "{file:missing.txt}",
  "provider": {"p": {"npm": "@ai-sdk/openai-compatible", "options": {"baseURL": "http://h/v1/", "apiKey": "{env:MY_KEY}", "headers": {"X-Unset": "{env:NOPE}"}}, "models": {"m": {}}}},
  "agent": {"build": {"prompt": "{file:prompt.txt}"}},
}"#,
    );
    let c = e.load(&e.root);
    assert!(c.warnings.is_empty(), "{:?}", c.warnings);
    assert_eq!(c.agents["build"].prompt, "line1\n\"quoted\"");
    let s = c.resolve_model("p/m", None).unwrap();
    assert_eq!(s.api_key.as_deref(), Some("sk-test"));
    assert_eq!(s.base_url, "http://h/v1");
    assert_eq!(s.headers["X-Unset"], "");
}

#[test]
fn merge_precedence() {
    let mut e = Env::new();
    let cwd = e.project();
    e.write("home/.config/opencode/opencode.json", r#"{"model": "g/1", "small_model": "g/s", "instructions": ["a.md"], "compaction": {"auto": false, "threshold": 0.5}}"#);
    e.write(
        "explicit.json",
        r#"{"model": "x/1", "default_agent": "plan"}"#,
    );
    e.paths.explicit_config = Some(e.root.join("explicit.json"));
    e.write(
        "proj/opencode.jsonc",
        r#"{"model": "p/far", /* c */ "username": "far",}"#,
    );
    e.write("proj/sub/opencode.json", r#"{"model": "p/near"}"#);
    e.write(
        "proj/.opencode/opencode.json",
        r#"{"username": "dotopencode", "instructions": ["b.md"]}"#,
    );
    e.paths.config_content = Some(r#"{"compaction": {"threshold": 0.6}}"#.into());
    e.write(
        "home/.config/ref/ref.json",
        r#"{"model": "r/global", "tool_output_limit": 1000}"#,
    );
    e.write(
        "proj/sub/.ref/ref.jsonc",
        r#"{"model": "r/proj", "delay": {"bash": 2},}"#,
    );
    // Broken file: warned, ignored.
    e.write("proj/.opencode/opencode.jsonc", r#"{"model": "#);
    let c = e.load(&cwd);
    assert_eq!(c.model.as_deref(), Some("r/proj"));
    assert_eq!(c.small_model.as_deref(), Some("g/s"));
    assert_eq!(c.default_agent, "plan");
    assert_eq!(c.raw["username"], "dotopencode");
    assert_eq!(c.instructions, vec!["a.md", "b.md"]);
    assert!(!c.compaction.auto);
    assert_eq!(c.compaction.threshold, 0.6);
    assert_eq!(c.tool_output_limit, 1000);
    assert_eq!(c.delay("build", "bash"), Duration::from_secs(2));
    assert_eq!(c.warnings.len(), 1);
    assert_eq!(c.sources.len(), 8);
}

#[test]
fn project_walk_stops_at_cwd_without_git() {
    let e = Env::new();
    e.write("a/opencode.json", r#"{"model": "outer/m"}"#);
    e.write("a/b/opencode.json", r#"{"username": "inner"}"#);
    let c = e.load(&e.root.join("a/b"));
    assert_eq!(c.model, None);
    assert_eq!(c.raw["username"], "inner");
}

fn provider_config() -> &'static str {
    r#"{
  "provider": {
    "anth": {
      "npm": "@ai-sdk/anthropic",
      "options": {"apiKey": "k1", "headers": {"A": "1"}, "timeout": false, "chunkTimeout": 5000, "setCacheKey": true, "custom": 1},
      "models": {
        "claude": {
          "id": "claude-x", "reasoning": true, "attachment": true,
          "limit": {"context": 200000, "output": 64000, "compact": 150000},
          "options": {"thinking": {"type": "enabled", "budgetTokens": 1000}},
          "headers": {"B": "2"},
          "variants": {"high": {"thinking": {"budgetTokens": 9000}}, "none": {"thinking": {"type": "disabled"}}, "gone": {"disabled": true}}
        },
        "plain": {"limit": {"context": 1000, "output": 5000}}
      }
    },
    "oai": {
      "npm": "@ai-sdk/openai",
      "options": {"apiKey": "k2", "body": {"service_tier": "flex"}},
      "models": {"gpt": {"thinking": true, "options": {"store": false}}}
    },
    "compat": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "Compat",
      "options": {"baseURL": "https://c.example/v1", "headerTimeout": 1000},
      "models": {
        "org/model-a": {"modalities": {"input": ["text", "image"]}, "variants": {"off": {"chat_template_kwargs": {"enable_thinking": false}}}},
        "resp": {"provider": {"npm": "@ai-sdk/openai"}},
        "forced": {"api_format": "anthropic"}
      }
    },
    "fmt": {"api_format": "responses", "options": {"baseURL": "https://f/v1"}, "models": {"m": {}}}
  }
}"#
}

#[test]
fn resolve_models_by_npm() {
    let e = Env::new();
    e.write("home/.config/opencode/opencode.json", provider_config());
    let c = e.load(&e.root);

    let s = c.resolve_model("anth/claude", None).unwrap();
    assert_eq!(s.api, ApiKind::Anthropic);
    assert_eq!(s.base_url, "https://api.anthropic.com/v1");
    assert_eq!(s.api_model, "claude-x");
    assert_eq!(s.full_id(), "anth/claude");
    assert_eq!(s.api_key.as_deref(), Some("k1"));
    assert_eq!(s.headers.get("A").map(String::as_str), Some("1"));
    assert_eq!(s.headers.get("B").map(String::as_str), Some("2"));
    assert_eq!(
        s.options,
        json!({"custom": 1, "thinking": {"type": "enabled", "budgetTokens": 1000}})
            .as_object()
            .unwrap()
            .clone()
    );
    assert_eq!((s.context, s.max_output), (200000, 64000));
    assert!(s.reasoning && s.image_input);
    assert_eq!(s.temperature, None);
    assert_eq!(s.header_timeout, Duration::from_secs(300));
    assert_eq!(s.chunk_timeout, Duration::from_secs(5));
    assert_eq!(c.compact_at(&s), 150000);

    let s = c.resolve_model("anth/claude#high", None).unwrap();
    assert_eq!(
        s.options["thinking"],
        json!({"type": "enabled", "budgetTokens": 9000})
    );
    let s = c.resolve_model("anth/claude", Some("none")).unwrap();
    assert_eq!(s.options["thinking"]["type"], "disabled");
    let err = c
        .resolve_model("anth/claude", Some("gone"))
        .unwrap_err()
        .to_string();
    assert!(err.ends_with("available: high, none"), "{err}");

    let s = c.resolve_model("anth/plain", None).unwrap();
    assert_eq!((s.context, s.max_output), (1000, 250));
    assert_eq!(c.compact_at(&s), 800);

    let s = c.resolve_model("oai/gpt", Some("high")).unwrap();
    assert_eq!(s.api, ApiKind::OpenAIResponses);
    assert_eq!(s.base_url, "https://api.openai.com/v1");
    assert_eq!(
        s.extra_body,
        json!({"service_tier": "flex"}).as_object().unwrap().clone()
    );
    assert_eq!(
        s.options,
        json!({"store": false, "reasoningEffort": "high", "reasoningSummary": "auto"})
            .as_object()
            .unwrap()
            .clone()
    );
    assert_eq!((s.context, s.max_output), (128000, 32000));

    let s = c.resolve_model("compat/org/model-a#off", None).unwrap();
    assert_eq!(s.api, ApiKind::OpenAIChat);
    assert_eq!(s.model_key, "org/model-a");
    assert_eq!(s.api_model, "org/model-a");
    assert_eq!(s.base_url, "https://c.example/v1");
    assert!(s.image_input && !s.reasoning);
    assert_eq!(s.options["chat_template_kwargs"]["enable_thinking"], false);
    assert_eq!(s.header_timeout, Duration::from_secs(1));
    assert_eq!(s.api_key, None);

    assert_eq!(
        c.resolve_model("compat/resp", None).unwrap().api,
        ApiKind::OpenAIResponses
    );
    assert_eq!(
        c.resolve_model("compat/forced", None).unwrap().api,
        ApiKind::Anthropic
    );
    assert_eq!(
        c.resolve_model("fmt/m", None).unwrap().api,
        ApiKind::OpenAIResponses
    );

    let err = c
        .resolve_model("compat/model-a", None)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("did you mean") && err.contains("compat/org/model-a"),
        "{err}"
    );
    assert!(c.resolve_model("nope/x", None).is_err());
    assert!(c.resolve_model("noslash", None).is_err());

    let models = c.models();
    let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "anth/claude",
            "anth/plain",
            "oai/gpt",
            "compat/org/model-a",
            "compat/resp",
            "compat/forced",
            "fmt/m"
        ]
    );
    assert_eq!(models[0].variants, ["high", "none"]);
    assert_eq!(models[2].variants, ["low", "medium", "high", "xhigh"]);
    assert_eq!(models[3].variants, ["off"]);
    assert_eq!(models[3].provider_name, "Compat");
    assert!(models[1].variants.is_empty());

    // First configured model is the fallback default.
    assert_eq!(
        c.default_model_for("build"),
        Some(("anth/claude".into(), None))
    );
}

#[test]
fn catalog_and_credentials() {
    let mut e = Env::new();
    e.write(
        "home/.cache/opencode/models.json",
        &json!({
            "anthropic": {"id": "anthropic", "env": ["ANTHROPIC_API_KEY"], "npm": "@ai-sdk/anthropic", "name": "Anthropic",
                "models": {"claude-z": {"id": "claude-z", "name": "Claude Z", "reasoning": true, "temperature": false, "attachment": false,
                    "modalities": {"input": ["text", "image"]}, "limit": {"context": 500000, "output": 64000},
                    "cost": {"input": 3, "output": 15, "cache_read": 0.3}}}},
            "zai": {"id": "zai", "env": ["ZAI_KEY"], "npm": "@ai-sdk/openai-compatible", "api": "https://z/v4", "name": "Z",
                "models": {"glm": {"id": "glm", "name": "GLM", "limit": {"context": 200000, "output": 100000}}}},
            "nokey": {"id": "nokey", "env": ["NOKEY_KEY"], "api": "https://n", "models": {"x": {}}},
            "authd": {"id": "authd", "env": [], "api": "https://a/", "name": "Authd", "models": {"y": {"name": "Y"}}},
        })
        .to_string(),
    );
    e.write(
        "home/.local/share/opencode/auth.json",
        r#"{"authd": {"type": "api", "key": "from-auth"}, "zai": {"type": "oauth"}}"#,
    );
    e.paths
        .env
        .insert("ANTHROPIC_API_KEY".into(), "env-key".into());
    e.write(
        "home/.config/opencode/opencode.json",
        r#"{"provider": {"zai": {"options": {"apiKey": "cfg"}, "models": {"glm": {"name": "GLM custom"}}}},
           "agent": {"build": {"temperature": 0.3, "top_p": 0.9}, "plan": {"temperature": 0.5, "model": "zai/glm#x"}}}"#,
    );
    let c = e.load(&e.root);
    let ids: Vec<String> = c.models().into_iter().map(|m| m.id).collect();
    assert_eq!(ids, ["zai/glm", "anthropic/claude-z", "authd/y"]);

    let s = c.resolve_model("zai/glm", None).unwrap();
    assert_eq!(s.base_url, "https://z/v4");
    assert_eq!(s.api_key.as_deref(), Some("cfg"));
    assert_eq!((s.context, s.max_output), (200000, 100000));
    let s = c.resolve_for_agent("plan", "zai/glm", None).unwrap();
    assert_eq!(s.temperature, Some(0.5));

    let s = c.resolve_model("anthropic/claude-z", Some("max")).unwrap();
    assert_eq!(s.api, ApiKind::Anthropic);
    assert_eq!(s.api_key.as_deref(), Some("env-key"));
    assert_eq!(s.base_url, "https://api.anthropic.com/v1");
    assert_eq!(s.options["effort"], "max");
    assert!(s.image_input && s.reasoning);
    assert_eq!(s.cost.cache_read, 0.3);
    // Catalog says temperature: false → agent temperature is not applied.
    let s = c
        .resolve_for_agent("build", "anthropic/claude-z", None)
        .unwrap();
    assert_eq!((s.temperature, s.top_p), (None, Some(0.9)));

    let s = c.resolve_model("authd/y", None).unwrap();
    assert_eq!(
        (s.api_key.as_deref(), s.base_url.as_str()),
        (Some("from-auth"), "https://a")
    );
    assert!(c.resolve_model("nokey/x", None).is_err());

    assert_eq!(
        c.default_model_for("plan"),
        Some(("zai/glm".into(), Some("x".into())))
    );
    assert_eq!(c.default_model_for("build"), Some(("zai/glm".into(), None)));

    // disabled / enabled lists
    e.paths.config_content =
        Some(r#"{"disabled_providers": ["zai"], "enabled_providers": ["zai", "authd"]}"#.into());
    let c = e.load(&e.root);
    let ids: Vec<String> = c.models().into_iter().map(|m| m.id).collect();
    assert_eq!(ids, ["authd/y"]);
}

#[test]
fn permissions_and_overrides() {
    let e = Env::new();
    e.write(
        "home/.config/opencode/opencode.json",
        r#"{
  "permission": {
    "*": "allow",
    "bash": {"*": "ask", "git *": "allow", "git push *": "deny", "ls?": "allow"},
    "edit": {"*": "allow", "*.lock": "deny", "~/secret/*": "deny"},
    "glob": "ask",
    "web_*": "deny",
    "doom_loop": "ask",
    "external_directory": "deny"
  },
  "delay": {"*": 1, "bash": 3, "web_*": 0.5},
  "agent": {
    "review": {"permission": {"bash": "deny", "web_search": "allow"}, "delay": {"bash": 0}, "tools": {"write": false}},
    "plan": {"permission": {"read": "deny"}}
  }
}"#,
    );
    let c = e.load(&e.root);
    let p = |a: &str, t: &str, s: Option<&str>| c.permission(a, t, s);
    assert_eq!(p("build", "bash", Some("git")), Action::Allow);
    assert_eq!(p("build", "bash", Some("git status")), Action::Allow);
    assert_eq!(p("build", "bash", Some("git push origin")), Action::Deny);
    assert_eq!(p("build", "bash", Some("gitk")), Action::Ask);
    assert_eq!(p("build", "bash", Some("lsx")), Action::Allow);
    assert_eq!(p("build", "bash", Some("rm -rf /")), Action::Ask);
    assert_eq!(p("build", "edit", Some("Cargo.lock")), Action::Deny);
    assert_eq!(p("build", "write", Some("a/b.lock")), Action::Deny);
    assert_eq!(p("build", "delete", Some("src/x.rs")), Action::Allow);
    let home = c.paths.home.to_string_lossy().into_owned();
    assert_eq!(
        p("build", "write", Some(&format!("{home}/secret/k"))),
        Action::Deny
    );
    assert_eq!(p("build", "find", None), Action::Ask);
    assert_eq!(p("build", "web_search", None), Action::Deny);
    assert_eq!(p("build", "read", Some("x")), Action::Allow);
    assert_eq!(p("review", "bash", Some("git status")), Action::Deny);
    assert_eq!(p("review", "web_search", None), Action::Allow);
    assert_eq!(p("review", "write", Some("x")), Action::Deny);
    assert_eq!(p("review", "edit", Some("x")), Action::Allow);
    // Built-in plan rules + config override; global bash patterns stay below agent rules.
    assert_eq!(p("plan", "edit", Some("x")), Action::Ask);
    assert_eq!(p("plan", "bash", Some("git status")), Action::Ask);
    assert_eq!(p("plan", "read", Some("x")), Action::Deny);
    assert_eq!(p("unknown-agent", "grep", None), Action::Allow);

    assert_eq!(c.delay("build", "bash"), Duration::from_secs(3));
    assert_eq!(c.delay("build", "read"), Duration::from_secs(1));
    assert_eq!(c.delay("build", "web_reader"), Duration::from_millis(500));
    assert_eq!(c.delay("review", "bash"), Duration::ZERO);
    assert_eq!(c.delay("review", "read"), Duration::from_secs(1));

    // String permission applies to everything; no config → allow.
    let e2 = Env::new();
    e2.write(
        "home/.config/opencode/opencode.json",
        r#"{"permission": "ask"}"#,
    );
    assert_eq!(
        e2.load(&e2.root).permission("build", "read", None),
        Action::Ask
    );
    let e3 = Env::new();
    assert_eq!(
        e3.load(&e3.root).permission("build", "bash", Some("rm")),
        Action::Allow
    );
}

#[test]
fn wildcard_semantics() {
    use perm::wildcard_match as m;
    assert!(m("git *", "git"));
    assert!(m("git *", "git log -1"));
    assert!(!m("git *", "gitx"));
    assert!(m("*.rs", "src/a.rs"));
    assert!(m("a?c", "abc"));
    assert!(!m("a?c", "abbc"));
    assert!(m("echo *", "echo a\nb"));
    assert!(m("a.b", "a.b") && !m("a.b", "axb"));
}

#[test]
fn agents_from_markdown_and_config() {
    let e = Env::new();
    let cwd = e.project();
    e.write(
        "home/.config/opencode/agents/reviewer.md",
        "---\ndescription: Reviews code: carefully\nmode: subagent\nmodel: p/m\nvariant: high\ncolor: \"#ff0000\"\n---\n\nYou review code.\n",
    );
    e.write(
        "home/.config/opencode/agent/nested/deep.md",
        "---\ndescription: deep\n---\nbody",
    );
    e.write(
        "proj/.opencode/agents/primary.md",
        "---\ndescription: P\nmode: primary\n---\nPrimary prompt",
    );
    e.write(
        "home/.config/ref/agents/refagent.md",
        "---\ndescription: R\ncan_spawn: false\ndelay:\n  bash: 2\n---\nR prompt",
    );
    e.write(
        "proj/.ref/agents/reviewer.md",
        "---\nspawnable: false\n---\nOverridden prompt",
    );
    e.write("home/.config/opencode/modes/legacy.md", "Legacy");
    e.write(
        "home/.config/opencode/opencode.json",
        r#"{"agent": {"plan": {"disable": true}, "x": {"description": "X", "model": "a/b#low", "reasoningEffort": "low", "steps": 3, "hidden": true}}, "default_agent": "x"}"#,
    );
    let c = e.load(&cwd);
    let names: Vec<&str> = c.agents.keys().map(String::as_str).collect();
    assert_eq!(
        names,
        [
            "build",
            "legacy",
            "nested/deep",
            "primary",
            "refagent",
            "reviewer",
            "x"
        ]
    );
    let r = &c.agents["reviewer"];
    assert_eq!(r.description, "Reviews code: carefully");
    assert_eq!(r.prompt, "Overridden prompt");
    assert_eq!(
        (r.model.as_deref(), r.variant.as_deref()),
        (Some("p/m"), Some("high"))
    );
    assert_eq!(r.color.as_deref(), Some("#ff0000"));
    assert!(!r.spawnable && r.can_spawn);
    assert!(!c.agents["primary"].spawnable);
    assert!(!c.agents["legacy"].spawnable);
    assert_eq!(c.agents["refagent"].delay["bash"], 2.0);
    assert!(!c.agents["refagent"].can_spawn);
    let x = &c.agents["x"];
    assert!(x.hidden && x.spawnable);
    assert_eq!(
        x.options,
        json!({"reasoningEffort": "low"})
            .as_object()
            .unwrap()
            .clone()
    );
    assert_eq!(
        (x.model.as_deref(), x.variant.as_deref()),
        (Some("a/b"), Some("low"))
    );
    assert_eq!(c.default_agent, "x");
    let b = &c.agents["build"];
    assert_eq!(b.description, "Default agent with all tools");
    assert!(b.prompt.is_empty() && b.spawnable && b.can_spawn);
    let entries = c.agent_entries();
    let re = entries.iter().find(|a| a.name == "reviewer").unwrap();
    assert_eq!(
        (re.model.as_str(), re.variant.as_deref()),
        ("p/m", Some("high"))
    );
}

#[test]
fn builtin_plan_agent() {
    let e = Env::new();
    let c = e.load(&e.root);
    assert_eq!(c.default_agent, "build");
    let p = &c.agents["plan"];
    assert!(p.prompt.starts_with("Plan mode:"));
    for t in ["edit", "write", "delete", "bash"] {
        assert_eq!(c.permission("plan", t, Some("x")), Action::Ask, "{t}");
    }
    assert_eq!(c.permission("plan", "read", Some("x")), Action::Allow);
    assert_eq!(c.default_model_for("build"), None);
    assert!(c.models().is_empty());
}

#[test]
fn commands() {
    let e = Env::new();
    let cwd = e.project();
    e.write(
        "home/.config/opencode/commands/test.md",
        "---\ndescription: Run tests\nagent: build\n---\nRun $ARGUMENTS\n",
    );
    e.write("proj/.opencode/command/review.md", "Review it");
    e.write(
        "home/.config/opencode/opencode.json",
        r#"{"command": {"fix": {"template": "Fix $1", "description": "Fix", "model": "a/b"}, "bad": {"description": "no template"}}}"#,
    );
    let c = e.load(&cwd);
    let names: Vec<&str> = c.commands.keys().map(String::as_str).collect();
    assert_eq!(names, ["fix", "review", "test"]);
    assert_eq!(c.commands["test"].template, "Run $ARGUMENTS");
    assert_eq!(c.commands["test"].agent.as_deref(), Some("build"));
    assert_eq!(c.commands["fix"].model.as_deref(), Some("a/b"));
    assert_eq!(c.commands["review"].description, "");
    assert_eq!(c.command_entries().len(), 3);
}

#[test]
fn instructions_order() {
    let e = Env::new();
    let cwd = e.project();
    e.write("home/.config/opencode/AGENTS.md", "global");
    e.write("home/.claude/CLAUDE.md", "claude-global");
    e.write("proj/AGENTS.md", "root");
    e.write("proj/sub/AGENTS.md", "sub");
    e.write("proj/sub/CLAUDE.md", "ignored claude");
    e.write("proj/docs/a.md", "doc a");
    e.write("proj/docs/b.md", "doc b");
    e.write("abs/rules.md", "abs");
    e.write("home/notes.md", "home note");
    e.write("proj/big.md", &"x".repeat(210 * 1024));
    let cfg = format!(
        r#"{{"instructions": ["docs/*.md", "AGENTS.md", "{}/abs/*.md", "~/notes.md", "big.md", "https://example.com/x.md"]}}"#,
        e.root.display()
    );
    e.write("home/.config/opencode/opencode.json", &cfg);
    let c = e.load(&cwd);
    let got: Vec<String> = instructions(&c, &cwd).into_iter().map(|(_, s)| s).collect();
    assert_eq!(
        got,
        [
            "global",
            "root",
            "sub",
            "doc a",
            "doc b",
            "abs",
            "home note"
        ]
    );

    // CLAUDE.md fallback when no AGENTS.md anywhere; ~/.claude/CLAUDE.md when no global AGENTS.md.
    let e = Env::new();
    let cwd = e.project();
    e.write("home/.claude/CLAUDE.md", "claude-global");
    e.write("proj/CLAUDE.md", "claude-root");
    e.write("outside/AGENTS.md", "outside git root");
    let c = e.load(&cwd);
    let got: Vec<String> = instructions(&c, &cwd).into_iter().map(|(_, s)| s).collect();
    assert_eq!(got, ["claude-global", "claude-root"]);
}

#[test]
fn skills_discovery() {
    let e = Env::new();
    let cwd = e.project();
    let skill =
        |name: &str, desc: &str| format!("---\nname: {name}\ndescription: {desc}\n---\nbody");
    e.write(
        "home/.config/opencode/skills/alpha/SKILL.md",
        &skill("alpha", "global alpha"),
    );
    e.write(
        "home/.claude/skills/beta/SKILL.md",
        &skill("beta", "claude beta"),
    );
    e.write(
        "home/.agents/skills/group/deep/gamma/SKILL.md",
        &skill("gamma", "deep gamma"),
    );
    e.write(
        "home/.config/ref/skills/nodesc/SKILL.md",
        "---\nname: nodesc\n---\n",
    );
    e.write(
        "home/.config/ref/skills/dirname/SKILL.md",
        "---\ndescription: uses dir name\n---\n",
    );
    e.write(
        "proj/.opencode/skill/alpha/SKILL.md",
        &skill("alpha", "project alpha"),
    );
    e.write(
        "proj/sub/.ref/skills/x/SKILL.md",
        &skill("beta", "ref beta"),
    );
    e.write(
        "proj/.claude/skills/too/deep/a/b/c/SKILL.md",
        &skill("toodeep", "x"),
    );
    e.write("extra/one/SKILL.md", &skill("extra", "from config"));
    e.write(
        "home/.config/opencode/opencode.json",
        &format!(r#"{{"skills": ["{}/extra"]}}"#, e.root.display()),
    );
    let c = e.load(&cwd);
    let got: Vec<(String, String)> = skills(&c, &cwd)
        .into_iter()
        .map(|s| (s.name, s.description))
        .collect();
    let want = [
        ("alpha", "project alpha"),
        ("beta", "ref beta"),
        ("dirname", "uses dir name"),
        ("extra", "from config"),
        ("gamma", "deep gamma"),
    ];
    let want: Vec<(String, String)> = want
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
    assert_eq!(got, want);
}

#[test]
fn compaction_threshold_absolute() {
    let e = Env::new();
    e.write(
        "home/.config/opencode/opencode.json",
        r#"{"compaction": {"threshold": 50000, "reserved": 1000}, "provider": {"p": {"options": {"baseURL": "http://x"}, "models": {"m": {"limit": {"context": 100000, "output": 0}}}}}}"#,
    );
    let c = e.load(&e.root);
    let s = c.resolve_model("p/m", None).unwrap();
    assert_eq!(s.max_output, 32000);
    assert_eq!(c.compact_at(&s), 50000);
    assert_eq!(c.reserved_tokens(&s), 1000);
}

#[test]
fn debug_output_hides_secrets() {
    let mut e = Env::new();
    e.paths.env.insert("SECRET".into(), "sk-very-secret".into());
    e.write("home/.config/opencode/opencode.json", r#"{"provider": {"p": {"options": {"apiKey": "{env:SECRET}", "baseURL": "http://x"}, "models": {"m": {}}}}}"#);
    let c = e.load(&e.root);
    let dbg = format!("{c:?}");
    assert!(
        !dbg.contains("sk-very-secret"),
        "debug output leaks the key"
    );
}

/// Loads the real user config and prints only model ids. Run manually:
/// `cargo test -p ref-config real_user_config -- --ignored --nocapture`
#[test]
#[ignore]
fn real_user_config() {
    let cwd = std::env::current_dir().unwrap();
    let t = std::time::Instant::now();
    let c = Config::load(&cwd).unwrap();
    println!(
        "loaded in {:?}; sources: {:?}; warnings: {}",
        t.elapsed(),
        c.sources,
        c.warnings.len()
    );
    for w in &c.warnings {
        // Warnings contain only file paths and parse errors, never values.
        println!("warning: {}", w.lines().next().unwrap_or(""));
    }
    let models = c.models();
    for m in &models {
        println!(
            "{} variants={:?} ctx={} img={} reasoning={}",
            m.id, m.variants, m.context, m.image_input, m.reasoning
        );
    }
    for m in &models {
        let spec = c
            .resolve_model(&m.id, m.variants.first().map(String::as_str))
            .unwrap();
        assert!(!spec.base_url.is_empty());
    }
    println!(
        "agents: {:?}; default: {:?}",
        c.agents.keys().collect::<Vec<_>>(),
        c.default_model_for(&c.default_agent)
    );
    println!("mcp: {:?}", c.mcp.keys().collect::<Vec<_>>());
    println!(
        "skills: {:?}",
        skills(&c, &cwd).iter().map(|s| &s.name).collect::<Vec<_>>()
    );
    println!(
        "instructions: {:?}",
        instructions(&c, &cwd)
            .iter()
            .map(|s| &s.0)
            .collect::<Vec<_>>()
    );
    println!("total {:?}", t.elapsed());
    assert!(!models.is_empty());
}
