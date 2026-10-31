//! Integration-style tests for loading (use temp dirs, never the real home config).

use crate::*;
use std::collections::HashMap;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> TempDir {
        static N: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "ref-config-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        TempDir(p.canonicalize().unwrap())
    }
    fn path(&self) -> &Path {
        &self.0
    }
    fn write(&self, rel: &str, content: &str) -> PathBuf {
        let p = self.0.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, content).unwrap();
        p
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Test fixture: `<tmp>/home` (home + `.config/opencode`, `.config/ref`), `<tmp>/proj` (git root).
struct Fx {
    t: TempDir,
    env: HashMap<String, String>,
}

impl Fx {
    fn new() -> Fx {
        let t = TempDir::new();
        fs::create_dir_all(t.path().join("home/.config/opencode")).unwrap();
        fs::create_dir_all(t.path().join("proj/.git")).unwrap();
        Fx {
            t,
            env: HashMap::new(),
        }
    }
    fn home(&self) -> PathBuf {
        self.t.path().join("home")
    }
    fn proj(&self) -> PathBuf {
        self.t.path().join("proj")
    }
    fn write(&self, rel: &str, content: &str) -> PathBuf {
        self.t.write(rel, content)
    }
    fn load_at(&self, cwd: &Path) -> Config {
        let home = self.home();
        let globals = [home.join(".config/opencode"), home.join(".config/ref")];
        let env = |k: &str| self.env.get(k).cloned();
        load_with(Some(&home), &globals, cwd, &env)
    }
    fn load(&self) -> Config {
        self.load_at(&self.proj())
    }
}

#[test]
fn empty_config_has_builtins() {
    let fx = Fx::new();
    let c = fx.load();
    assert!(c.warnings.is_empty(), "{:?}", c.warnings);
    assert_eq!(c.default_agent, "build");
    let names: Vec<_> = c.agents.keys().cloned().collect();
    assert_eq!(names, ["build", "compaction", "explore", "general", "plan"]);
    let build = &c.agents["build"];
    assert!(build.can_spawn && build.creatable);
    assert!(!c.agents["plan"].can_spawn);
    assert!(c.providers.is_empty());
    assert_eq!(c.retry_max_delay, 600);
    assert!(c.images_auto_resize);
    assert!(c.compaction.auto);
    assert!(c.config_dirs.contains(&fx.home().join(".config/opencode")));
    assert!(c.config_dirs.contains(&fx.proj()));
}

#[test]
fn providers_models_variants_images() {
    let fx = Fx::new();
    fx.write(
        "home/.config/opencode/opencode.json",
        r#"{
  // comment
  "model": "zc/claude",
  "small_model": "hc/gpt-5.5",
  "provider": {
    "zc": {
      "npm": "@ai-sdk/anthropic",
      "name": "ZeroCat",
      "options": {"baseURL": "https://zero.cat/v1", "apiKey": "sk-test", "headers": {"X-A": "1"}, "timeout": 1000},
      "models": {
        "claude": {"name": "Claude", "attachment": true, "reasoning": true,
                   "limit": {"context": 1000000, "output": 128000, "compact": 500000},
                   "temperature": true,
                   "variants": {"high": {"reasoningEffort": "high"}, "off": {"disabled": true}}}
      }
    },
    "hc": {
      "npm": "@ai-sdk/openai-compatible",
      "options": {"baseUrl": "https://hc/v1", "apiKey": "{env:HC_KEY}"},
      "models": {
        "gpt-5.5": {"id": "gpt-5.5-wire", "thinking": true, "temperature": 0.3, "top_p": 0.9,
                    "modalities": {"input": ["text", "image"], "output": ["text"]},
                    "options": {"store": false}, "headers": {"X-M": "m"}},
        "text-only": {}
      }
    },
  },
}"#,
    );
    let mut fx = fx;
    fx.env.insert("HC_KEY".into(), "from-env".into());
    let c = fx.load();
    assert!(c.warnings.is_empty(), "{:?}", c.warnings);
    assert_eq!(c.model, ModelRef::parse("zc/claude"));
    assert_eq!(c.small_model.as_ref().unwrap().key(), "hc/gpt-5.5");

    let zc = &c.providers["zc"];
    assert_eq!(zc.name, "ZeroCat");
    assert_eq!(zc.npm.as_deref(), Some("@ai-sdk/anthropic"));
    assert_eq!(zc.base_url.as_deref(), Some("https://zero.cat/v1"));
    assert_eq!(zc.api_key.as_deref(), Some("sk-test"));
    assert_eq!(zc.headers["X-A"], "1");
    assert_eq!(zc.options.get("timeout"), Some(&serde_json::json!(1000)));
    assert!(!zc.options.contains_key("apiKey") && !zc.options.contains_key("baseURL"));
    let m = &zc.models["claude"];
    assert_eq!((m.context, m.output, m.input), (1_000_000, 128_000, 0));
    assert_eq!(m.compact_at, Some(500_000));
    assert!(m.images && m.reasoning);
    assert_eq!(
        m.temperature, None,
        "temperature: true is a capability flag"
    );
    assert_eq!(m.variants.keys().collect::<Vec<_>>(), ["high"]);
    assert_eq!(m.variants["high"]["reasoningEffort"], "high");

    let hc = &c.providers["hc"];
    assert_eq!(hc.name, "hc");
    assert_eq!(hc.base_url.as_deref(), Some("https://hc/v1"));
    assert_eq!(hc.api_key.as_deref(), Some("from-env"));
    let g = &hc.models["gpt-5.5"];
    assert_eq!(g.id, "gpt-5.5-wire");
    assert_eq!(g.name, "gpt-5.5");
    assert!(g.images && g.reasoning);
    assert_eq!(g.temperature, Some(0.3));
    assert_eq!(g.top_p, Some(0.9));
    assert_eq!(g.headers["X-M"], "m");
    assert_eq!(g.options["store"], false);
    let t = &hc.models["text-only"];
    assert!(!t.images && !t.reasoning && t.context == 0);

    let (p, m) = c.model(&ModelRef::parse("hc/gpt-5.5").unwrap()).unwrap();
    assert_eq!((p.id.as_str(), m.key.as_str()), ("hc", "gpt-5.5"));
    assert_eq!(c.default_model_for("build").unwrap().key(), "zc/claude");
}

#[test]
fn api_key_from_env_list_and_conventional() {
    let mut fx = Fx::new();
    fx.write(
        "home/.config/opencode/opencode.json",
        r#"{"provider": {
            "a": {"npm": "@ai-sdk/openai-compatible", "env": ["EMPTY_KEY", "A_KEY"], "models": {"m": {}}},
            "b": {"npm": "@ai-sdk/anthropic", "models": {"m": {}}},
            "c": {"npm": "@ai-sdk/openai", "options": {"apiKey": "{env:MISSING}"}, "models": {"m": {}}}
        }}"#,
    );
    fx.env.insert("EMPTY_KEY".into(), "".into());
    fx.env.insert("A_KEY".into(), "ka".into());
    fx.env.insert("ANTHROPIC_API_KEY".into(), "kb".into());
    fx.env.insert("OPENAI_API_KEY".into(), "kc".into());
    let c = fx.load();
    assert_eq!(c.providers["a"].api_key.as_deref(), Some("ka"));
    assert_eq!(c.providers["b"].api_key.as_deref(), Some("kb"));
    assert_eq!(c.providers["c"].api_key.as_deref(), Some("kc"));
    // Built-in catalog entries are added for env keys of unconfigured providers.
    assert!(c.providers.contains_key("anthropic"));
    assert!(c.providers.contains_key("openai"));
    assert!(!c.providers.contains_key("deepseek"));
}

#[test]
fn builtin_catalog_only_with_env() {
    let mut fx = Fx::new();
    assert!(fx.load().providers.is_empty());
    fx.env.insert("DEEPSEEK_API_KEY".into(), "k".into());
    fx.env.insert("OPENAI_API_KEY".into(), "k2".into());
    let c = fx.load();
    let ds = &c.providers["deepseek"];
    assert_eq!(ds.base_url.as_deref(), Some("https://api.deepseek.com/v1"));
    assert_eq!(ds.api.as_deref(), Some("chat"));
    assert_eq!(ds.api_key.as_deref(), Some("k"));
    assert!(ds.models.contains_key("deepseek-chat") && ds.models["deepseek-reasoner"].reasoning);
    let oa = &c.providers["openai"];
    assert_eq!(oa.npm.as_deref(), Some("@ai-sdk/openai"));
    assert_eq!(
        oa.models["gpt-5"].variants.keys().collect::<Vec<_>>(),
        ["high", "low", "medium"]
    );

    // A configured provider with the same id is not replaced.
    fx.write(
        "home/.config/opencode/opencode.json",
        r#"{"provider": {"deepseek": {"models": {"custom": {}}}}}"#,
    );
    let c = fx.load();
    let ds = &c.providers["deepseek"];
    assert_eq!(ds.models.keys().collect::<Vec<_>>(), ["custom"]);
    // ...but known defaults (base url, env key) still apply.
    assert_eq!(ds.base_url.as_deref(), Some("https://api.deepseek.com/v1"));
    assert_eq!(ds.api_key.as_deref(), Some("k"));
}

#[test]
fn substitution_env_and_file() {
    let mut fx = Fx::new();
    fx.write(
        "home/.config/opencode/prompts/rev.md",
        "  Review carefully.\n\n",
    );
    fx.write("home/secret.txt", "tok\n");
    fx.write(
        "home/.config/opencode/opencode.json",
        r#"{
          "agent": {"rev": {"prompt": "{file:./prompts/rev.md}", "description": "by {env:WHO}{env:NOPE}!"}},
          "mcp": {"s": {"url": "https://x", "headers": {"Authorization": "Bearer {file:~/secret.txt}"}},
                  "bad": {"url": "https://y", "headers": {"A": "{file:./missing.txt}"}}}
        }"#,
    );
    fx.env.insert("WHO".into(), "me".into());
    let c = fx.load();
    assert_eq!(c.agents["rev"].prompt, "Review carefully.");
    assert_eq!(c.agents["rev"].description, "by me!");
    match &c.mcp["s"] {
        McpConfig::Remote { headers, .. } => assert_eq!(headers["Authorization"], "Bearer tok"),
        _ => panic!(),
    }
    assert_eq!(c.warnings.len(), 1, "{:?}", c.warnings);
    assert!(c.warnings[0].contains("missing.txt"));
}

#[test]
fn merge_order_and_project_walk() {
    let fx = Fx::new();
    fx.write(
        "home/.config/opencode/opencode.json",
        r#"{"model": "a/global", "tui": {"theme": "x", "keep": 1}, "instructions": ["g.md"],
            "provider": {"p": {"options": {"baseURL": "https://g"}, "models": {"m1": {}, "m2": {}}}}}"#,
    );
    fx.write("home/.config/ref/ref.json", r#"{"small_model": "a/small"}"#);
    fx.write(
        "proj/opencode.json",
        r#"{"model": "a/root", "tui": {"theme": "y"}, "provider": {"p": {"models": {"m3": {}}}}}"#,
    );
    fx.write(
        "proj/.opencode/opencode.jsonc",
        r#"{"default_agent": "plan",}"#,
    );
    fx.write(
        "proj/sub/ref.json",
        r#"{"model": "a/sub", "instructions": ["s.md"]}"#,
    );
    fx.write(
        "proj/sub/.ref/ref.jsonc",
        r#"{"retry": {"max_delay": 30}, "images": {"auto_resize": false}}"#,
    );
    fx.write("proj/other/opencode.json", r#"{"model": "a/other"}"#);
    // Above the git root: ignored.
    fx.write("opencode.json", r#"{"model": "a/outside"}"#);

    let c = fx.load_at(&fx.proj().join("sub"));
    assert!(c.warnings.is_empty(), "{:?}", c.warnings);
    assert_eq!(c.model.unwrap().key(), "a/sub");
    assert_eq!(c.small_model.unwrap().key(), "a/small");
    assert_eq!(c.default_agent, "plan");
    assert_eq!(c.retry_max_delay, 30);
    assert!(!c.images_auto_resize);
    assert_eq!(c.tui, serde_json::json!({"theme": "y", "keep": 1}));
    assert_eq!(c.instructions, ["g.md", "s.md"]);
    let p = &c.providers["p"];
    assert_eq!(p.models.keys().collect::<Vec<_>>(), ["m1", "m2", "m3"]);
    assert_eq!(p.base_url.as_deref(), Some("https://g"));
    assert!(c.config_dirs.contains(&fx.proj().join(".opencode")));
    assert!(c.config_dirs.contains(&fx.proj().join("sub/.ref")));
    assert!(!c.config_dirs.contains(&fx.proj().join("other")));

    let c = fx.load_at(&fx.proj());
    assert_eq!(c.model.unwrap().key(), "a/root");
}

#[test]
fn no_git_root_uses_only_cwd() {
    let t = TempDir::new();
    t.write("opencode.json", r#"{"model": "a/up"}"#);
    t.write("work/opencode.json", r#"{"small_model": "a/s"}"#);
    let env = |_: &str| None;
    let c = load_with(None, &[], &t.path().join("work"), &env);
    assert_eq!(c.model, None);
    assert_eq!(c.small_model.unwrap().key(), "a/s");
}

#[test]
fn parse_errors_become_warnings() {
    let fx = Fx::new();
    fx.write("home/.config/opencode/opencode.json", "{ \"model\": ");
    fx.write("proj/opencode.json", "[1, 2]");
    fx.write(
        "proj/.opencode/opencode.json",
        r#"{"permission": {"bash": "maybe"}, "model": 5}"#,
    );
    let c = fx.load();
    assert_eq!(c.warnings.len(), 4, "{:?}", c.warnings);
    // Windows paths mix `\` and `/`; compare with one separator.
    assert!(
        c.warnings[0]
            .replace('\\', "/")
            .contains("home/.config/opencode/opencode.json")
    );
    assert!(c.warnings.iter().any(|w| w.contains("JSON object")));
    assert!(c.warnings.iter().any(|w| w.contains("maybe")));
}

#[test]
fn compaction_agent_builtin_and_overrides() {
    let fx = Fx::new();
    let c = fx.load();
    let a = &c.agents[COMPACTION_AGENT];
    assert!(a.hidden && !a.creatable && !a.can_spawn);
    assert_eq!(a.prompt, COMPACTION_PROMPT);
    assert!(a.model.is_none() && a.variant.is_none());

    // JSON override of model / variant / options keeps the built-in prompt and flags.
    fx.write(
        "home/.config/opencode/opencode.json",
        r#"{"agent": {"compaction": {"model": "p/small", "variant": "low", "options": {"k": 1}}}}"#,
    );
    let c = fx.load();
    let a = &c.agents[COMPACTION_AGENT];
    assert_eq!(a.model.as_ref().unwrap().key(), "p/small");
    assert_eq!(a.variant.as_deref(), Some("low"));
    assert_eq!(a.options["k"], 1);
    assert_eq!(a.prompt, COMPACTION_PROMPT);
    assert!(a.hidden && !a.creatable);

    // Markdown agent file replaces the prompt.
    fx.write(
        "proj/.opencode/agent/compaction.md",
        "---\ntemperature: 0.1\n---\nSummarize tersely.\n",
    );
    let c = fx.load();
    let a = &c.agents[COMPACTION_AGENT];
    assert_eq!(a.prompt.trim(), "Summarize tersely.");
    assert_eq!(a.temperature, Some(0.1));
    assert_eq!(a.model.as_ref().unwrap().key(), "p/small");
    assert_eq!(c.default_agent, "build");
}

#[test]
fn agents_merge_and_disable() {
    let fx = Fx::new();
    fx.write(
        "home/.config/opencode/opencode.json",
        r##"{"agent": {
            "build": {"model": "p/m", "variant": "high", "temperature": 0.2, "reasoningEffort": "low",
                      "options": {"x": 1}},
            "general": {"disable": true},
            "rev": {"description": "Reviewer", "mode": "subagent", "permission": {"edit": "deny"},
                    "spawn": true, "creatable": false, "color": "#00ff00", "hidden": true,
                    "delay": {"bash": 4}},
            "plain": {}
        },
        "mode": {"old": {"prompt": "legacy"}}}"##,
    );
    let c = fx.load();
    assert!(c.warnings.is_empty(), "{:?}", c.warnings);
    let b = &c.agents["build"];
    assert_eq!(b.description, "Default agent with full tool access.");
    assert!(b.can_spawn, "unspecified fields keep built-in values");
    assert_eq!(b.temperature, Some(0.2));
    assert_eq!(b.options["x"], 1);
    assert_eq!(b.options["reasoningEffort"], "low");
    let dm = c.default_model_for("build").unwrap();
    assert_eq!(
        (dm.key().as_str(), dm.variant.as_deref()),
        ("p/m", Some("high"))
    );
    assert!(!c.agents.contains_key("general"));
    let r = &c.agents["rev"];
    assert_eq!(r.description, "Reviewer");
    assert_eq!(r.mode, "subagent");
    assert!(r.can_spawn && !r.creatable && r.hidden);
    assert_eq!(r.color.as_deref(), Some("#00ff00"));
    assert_eq!(r.delay, vec![("bash".to_string(), 4.0)]);
    let p = &c.agents["plain"];
    assert!(p.creatable && !p.can_spawn && p.model.is_none());
    assert_eq!(c.agents["old"].mode, "primary");
    assert_eq!(c.agents["old"].prompt, "legacy");
    assert!(c.default_model_for("plain").is_none());
}

#[test]
fn markdown_agents_and_commands() {
    let fx = Fx::new();
    fx.write(
        "home/.config/opencode/agent/review.md",
        "---\ndescription: Reviews code\nmode: subagent\nmodel: p/m\ntemperature: 0.1\nreasoningEffort: high\n\
         permission:\n  edit: deny\n  bash:\n    \"*\": ask\n    \"git diff\": allow\n\
         tools:\n  webfetch: false\ndelay:\n  bash: 1.5\ncan_spawn: true\n---\nYou review code.\n",
    );
    fx.write(
        "home/.config/opencode/agents/team/lead.md",
        "Lead prompt only.",
    );
    fx.write(
        "proj/.opencode/agents/review.md",
        "---\ndescription: Project reviewer\n---\nProject prompt.",
    );
    fx.write("proj/.ref/modes/fast.md", "---\nmode: subagent\n---\nFast.");
    fx.write(
        "proj/.opencode/command/test.md",
        "---\ndescription: Run tests\nagent: build\nmodel: p/m\n---\nRun the tests for $ARGUMENTS\n",
    );
    fx.write("proj/.ref/commands/git/sum.md", "Summarize $1");
    fx.write(
        "proj/opencode.json",
        r#"{"command": {"hello": {"template": "Say hi", "description": "greet"}, "bad": {}}}"#,
    );
    let c = fx.load();
    assert_eq!(c.warnings.len(), 1, "{:?}", c.warnings);
    assert!(c.warnings[0].contains("command.bad"));

    let r = &c.agents["review"];
    assert_eq!(r.description, "Project reviewer");
    assert_eq!(r.prompt, "Project prompt.");
    assert_eq!(r.mode, "subagent", "frontmatter fields merged across files");
    assert_eq!(r.model.as_ref().unwrap().key(), "p/m");
    assert_eq!(r.temperature, Some(0.1));
    assert_eq!(r.options["reasoningEffort"], "high");
    assert!(r.can_spawn);
    assert_eq!(r.delay, vec![("bash".to_string(), 1.5)]);
    assert_eq!(c.permission_for("review", "edit", "a.rs"), Action::Deny);
    assert_eq!(c.permission_for("review", "bash", "ls"), Action::Ask);
    assert_eq!(
        c.permission_for("review", "bash", "git diff"),
        Action::Allow
    );
    assert_eq!(c.permission_for("review", "webfetch", ""), Action::Deny);
    assert!(c.tool_disabled("review", "webfetch"));

    assert_eq!(c.agents["team/lead"].prompt, "Lead prompt only.");
    assert_eq!(c.agents["fast"].mode, "primary");

    let t = &c.commands["test"];
    assert_eq!(t.description, "Run tests");
    assert_eq!(t.agent.as_deref(), Some("build"));
    assert_eq!(t.model.as_ref().unwrap().key(), "p/m");
    assert_eq!(t.template, "Run the tests for $ARGUMENTS");
    assert_eq!(c.commands["git/sum"].template, "Summarize $1");
    assert_eq!(c.commands["hello"].template, "Say hi");
}

#[test]
fn permission_precedence() {
    let fx = Fx::new();
    fx.write(
        "home/.config/opencode/opencode.json",
        r#"{"tools": {"webfetch": false, "bash": false}, "permission": {"bash": "ask", "*": "allow"}}"#,
    );
    fx.write(
        "proj/opencode.json",
        r#"{"permission": {"bash": {"*": "ask", "git *": "allow", "rm *": "deny"}},
            "agent": {"build": {"permission": {"bash": {"git push*": "ask"}}},
                      "ro": {"tools": {"bash": false}}}}"#,
    );
    let c = fx.load();
    assert!(c.warnings.is_empty(), "{:?}", c.warnings);
    // `*: allow` comes after the legacy tools map, so it re-enables webfetch globally.
    assert_eq!(c.permission_for("build", "webfetch", ""), Action::Allow);
    assert_eq!(c.permission_for("build", "bash", "ls"), Action::Ask);
    assert_eq!(
        c.permission_for("build", "bash", "git status"),
        Action::Allow
    );
    assert_eq!(
        c.permission_for("build", "bash", "git push origin"),
        Action::Ask
    );
    assert_eq!(c.permission_for("build", "bash", "rm -rf x"), Action::Deny);
    assert_eq!(c.permission_for("nobody", "bash", "git log"), Action::Allow);
    // Agent rules override global ones.
    assert_eq!(c.permission_for("ro", "bash", "git log"), Action::Deny);
    assert!(c.tool_disabled("ro", "bash"));
    assert!(!c.tool_disabled("build", "bash"));
    // Built-in plan agent denies edits even though global allows everything.
    assert_eq!(c.permission_for("plan", "edit", "x"), Action::Deny);
    assert_eq!(c.permission_for("plan", "write", "x"), Action::Deny);
    assert_eq!(c.permission_for("plan", "bash", "rm -rf x"), Action::Ask);
    assert_eq!(c.permission_for("plan", "read", "x"), Action::Allow);
    assert!(c.tool_disabled("explore", "edit"));
}

#[test]
fn global_permission_string() {
    let fx = Fx::new();
    fx.write(
        "home/.config/opencode/opencode.json",
        r#"{"permission": "ask"}"#,
    );
    let c = fx.load();
    assert_eq!(c.permission_for("build", "anything", ""), Action::Ask);
    assert_eq!(c.permission_for("plan", "edit", "x"), Action::Deny);
}

#[test]
fn delay_precedence() {
    let fx = Fx::new();
    fx.write(
        "home/.config/opencode/opencode.json",
        r#"{"delay": {"*": 1, "bash": 3, "mcp_*": 2},
            "agent": {"slow": {"delay": {"bash": 10}}, "fast": {"delay": {"*": 0}}}}"#,
    );
    let c = fx.load();
    assert!(c.warnings.is_empty(), "{:?}", c.warnings);
    assert_eq!(c.delay_for("build", "bash"), 3.0);
    assert_eq!(c.delay_for("build", "read"), 1.0);
    assert_eq!(c.delay_for("build", "mcp_search"), 2.0);
    assert_eq!(c.delay_for("slow", "bash"), 10.0);
    assert_eq!(c.delay_for("slow", "read"), 1.0);
    assert_eq!(c.delay_for("fast", "bash"), 0.0);
    assert_eq!(c.delay_for("unknown", "bash"), 3.0);
}

#[test]
fn compaction_thresholds() {
    let fx = Fx::new();
    let c = fx.load();
    assert_eq!(c.compaction.recommend_tokens(100_000, None), 60_000);
    assert_eq!(c.compaction.recommend_tokens(100_000, Some(42)), 42);
    assert_eq!(c.compaction.auto_tokens(100_000, None), 90_000);

    fx.write(
        "home/.config/opencode/opencode.json",
        r#"{"compaction": {"auto": false, "recommend": 50000, "threshold": 0.8}}"#,
    );
    let c = fx.load();
    assert!(!c.compaction.auto);
    assert_eq!(c.compaction.recommend_tokens(200_000, None), 50_000);
    assert_eq!(
        c.compaction.recommend_tokens(10_000, None),
        10_000,
        "absolute capped at window"
    );
    assert_eq!(c.compaction.auto_tokens(200_000, None), 160_000);
}

#[test]
fn model_threshold_overrides_the_global_one() {
    let fx = Fx::new();
    fx.write(
        "home/.config/opencode/opencode.json",
        r#"{"compaction": {"threshold": 0.8},
            "provider": {"p": {"models": {
              "ratio": {"limit": {"context": 200000, "threshold": 0.5}},
              "tokens": {"limit": {"context": 1000000, "threshold": 250000, "compact": 100000}},
              "plain": {"limit": {"context": 200000}},
              "bad": {"limit": {"context": 200000, "threshold": -1}}}}}}"#,
    );
    let c = fx.load();
    let model = |key: &str| &c.providers["p"].models[key];
    let auto = |key: &str| {
        let m = model(key);
        c.compaction.auto_tokens(m.context, m.threshold)
    };
    assert_eq!(auto("ratio"), 100_000, "fraction of the window");
    assert_eq!(auto("tokens"), 250_000, "absolute tokens");
    assert_eq!(auto("plain"), 160_000, "falls back to the global threshold");
    assert_eq!(auto("bad"), 160_000, "an invalid value is ignored");
    // `limit.compact` only moves the recommended point.
    let tokens = model("tokens");
    let recommend = c
        .compaction
        .recommend_tokens(tokens.context, tokens.compact_at);
    assert_eq!(recommend, 100_000);
    assert_eq!(model("ratio").compact_at, None);
}

#[test]
fn explicit_project_root_hides_the_directories_above() {
    let fx = Fx::new();
    fx.write("proj/opencode.json", r#"{"model": "outer/model"}"#);
    fx.write(
        "proj/pkg/opencode.json",
        r#"{"small_model": "inner/model"}"#,
    );
    let pkg = fx.proj().join("pkg");

    // Without a root the git worktree (proj) is the top: both files count.
    let c = fx.load_at(&pkg);
    assert_eq!(c.model, ModelRef::parse("outer/model"));
    assert_eq!(c.small_model, ModelRef::parse("inner/model"));
    assert_eq!(c.project_root, None);

    let home = fx.home();
    let globals = [home.join(".config/opencode")];
    let env = |_: &str| None;
    let c = load_in(Some(&home), &globals, Some(&pkg), &pkg, &env);
    assert_eq!(c.model, None, "proj/opencode.json is above the root");
    assert_eq!(c.small_model, ModelRef::parse("inner/model"));
    assert_eq!(c.project_root, Some(pkg));
}

#[test]
fn mcp_parsing() {
    let fx = Fx::new();
    fx.write(
        "home/.config/opencode/opencode.json",
        r#"{"mcp": {
            "loc": {"type": "local", "command": ["npx", "-y", "srv"], "environment": {"A": "1"},
                    "cwd": "tools", "timeout": 5000},
            "inferred": {"command": ["srv"], "env": {"B": "2"}, "enabled": false},
            "rem": {"type": "remote", "url": "https://x/mcp", "headers": {"H": "v"}, "oauth": false},
            "url-only": {"url": "https://y"},
            "toggle": {"enabled": false},
            "broken": {"type": "local"}
        }}"#,
    );
    let c = fx.load();
    assert_eq!(c.warnings.len(), 1, "{:?}", c.warnings);
    assert_eq!(
        c.mcp.keys().collect::<Vec<_>>(),
        ["inferred", "loc", "rem", "url-only"]
    );
    match &c.mcp["loc"] {
        McpConfig::Local {
            command,
            environment,
            cwd,
            enabled,
            timeout_ms,
        } => {
            assert_eq!(command, &["npx", "-y", "srv"]);
            assert_eq!(environment["A"], "1");
            assert_eq!(cwd.as_deref(), Some("tools"));
            assert!(*enabled);
            assert_eq!(*timeout_ms, Some(5000));
        }
        _ => panic!(),
    }
    match &c.mcp["inferred"] {
        McpConfig::Local { environment, .. } => assert_eq!(environment["B"], "2"),
        _ => panic!(),
    }
    assert!(!c.mcp["inferred"].enabled());
    match &c.mcp["rem"] {
        McpConfig::Remote { url, headers, .. } => {
            assert_eq!(url, "https://x/mcp");
            assert_eq!(headers["H"], "v");
        }
        _ => panic!(),
    }
    assert!(matches!(c.mcp["url-only"], McpConfig::Remote { .. }));
}

#[test]
fn skills_paths() {
    let fx = Fx::new();
    fx.write(
        "home/.config/opencode/opencode.json",
        r#"{"skills": ["~/sk", "./rel"]}"#,
    );
    fx.write("proj/opencode.json", r#"{"skills": {"paths": ["more"]}}"#);
    let c = fx.load();
    assert!(c.warnings.is_empty(), "{:?}", c.warnings);
    assert_eq!(
        c.skill_paths,
        vec![
            fx.home().join("sk"),
            fx.home().join(".config/opencode/./rel"),
            fx.proj().join("more")
        ]
    );
}

/// Manual sanity check against the real user config. Prints no secrets.
/// Run: `cargo test -p ref-config -- --ignored --nocapture real_config_sanity`
#[test]
#[ignore]
fn real_config_sanity() {
    let c = Config::load(Path::new("/home/zzk/app/ref"));
    for (id, p) in &c.providers {
        let img = p.models.values().filter(|m| m.images).count();
        let rsn = p.models.values().filter(|m| m.reasoning).count();
        let var: usize = p.models.values().map(|m| m.variants.len()).sum();
        println!(
            "provider {id}: npm={:?} models={} images={img} reasoning={rsn} variants={var} key={} ids={:?}",
            p.npm,
            p.models.len(),
            if p.api_key.is_some() { "set" } else { "none" },
            p.models.keys().collect::<Vec<_>>()
        );
    }
    println!(
        "model={:?} small_model={:?}",
        c.model.as_ref().map(|m| m.key()),
        c.small_model.as_ref().map(|m| m.key())
    );
    println!(
        "agents={} {:?} default={}",
        c.agents.len(),
        c.agents.keys().collect::<Vec<_>>(),
        c.default_agent
    );
    println!("mcp={:?}", c.mcp.keys().collect::<Vec<_>>());
    println!(
        "permission rules={} commands={}",
        c.permission.rules.len(),
        c.commands.len()
    );
    println!("config_dirs={:?}", c.config_dirs);
    println!("warnings={:?}", c.warnings);
}

fn question_state(c: &Config, agent: &str) -> (bool, Action) {
    (
        c.tool_disabled(agent, "question"),
        c.permission_for(agent, "question", ""),
    )
}

#[test]
fn question_is_allowed_for_build_and_plan_only() {
    let fx = Fx::new();
    fx.write(
        "home/.config/opencode/opencode.json",
        r#"{"agent": {"helper": {"mode": "subagent"}}}"#,
    );
    let c = fx.load();
    assert_eq!(question_state(&c, "build"), (false, Action::Allow));
    assert_eq!(question_state(&c, "plan"), (false, Action::Allow));
    for denied in ["general", "explore", "compaction", "helper"] {
        assert_eq!(question_state(&c, denied), (true, Action::Deny), "{denied}");
    }
}

#[test]
fn a_global_question_rule_applies_to_every_agent() {
    let fx = Fx::new();
    fx.write(
        "home/.config/opencode/opencode.json",
        r#"{"permission": {"question": "allow"}}"#,
    );
    let c = fx.load();
    assert_eq!(question_state(&c, "general"), (false, Action::Allow));
    fx.write(
        "home/.config/opencode/opencode.json",
        r#"{"permission": {"question": "deny"}}"#,
    );
    let c = fx.load();
    assert_eq!(question_state(&c, "build"), (true, Action::Deny));
    fx.write(
        "home/.config/opencode/opencode.json",
        r#"{"permission": "allow"}"#,
    );
    let c = fx.load();
    assert_eq!(question_state(&c, "explore"), (false, Action::Allow));
}

#[test]
fn an_agent_rule_for_question_wins() {
    let fx = Fx::new();
    fx.write(
        "home/.config/opencode/opencode.json",
        r#"{"agent": {
            "general": {"permission": {"question": "allow"}},
            "build": {"permission": {"question": "deny"}},
            "plan": {"permission": {"question": "ask"}}
        }}"#,
    );
    let c = fx.load();
    assert_eq!(question_state(&c, "general"), (false, Action::Allow));
    assert_eq!(question_state(&c, "build"), (true, Action::Deny));
    assert_eq!(question_state(&c, "plan"), (false, Action::Ask));
}
