//! Integration-style tests for loading + model resolution (no real home dir involved).

use crate::load::{load_with, Dirs};
use crate::tests::TempDir;
use crate::*;
use hn_llm::Protocol;
use serde_json::json;

const GLOBAL: &str = r#"{
  "$schema": "https://opencode.ai/config.json",
  "permission": "allow",
  // comment with "quotes" and a url http://x
  "provider": {
    "hcai": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "HCAI",
      "options": { "baseURL": "https://ai.example.com/v1/", "apiKey": "{env:HN_FIXTURE_KEY}", },
      "models": {
        "gpt-5.5": {
          "name": "gpt-5.5",
          "thinking": true,
          "attachment": true,
          "modalities": { "input": ["text", "image"], "output": ["text"] },
          "options": { "include": ["reasoning.encrypted_content"], "store": false },
          "variants": {
            "xhigh": { "reasoningEffort": "xhigh", "textVerbosity": "medium", "reasoningSummary": "auto" },
            "high": { "reasoningEffort": "high", "textVerbosity": "medium", "reasoningSummary": "auto" },
            "off": { "disabled": true },
          },
        },
      },
    },
    "dgn": {
      "npm": "@ai-sdk/anthropic",
      "name": "dgn",
      "options": { "baseURL": "https://coding.example.cn/v1", "apiKey": "k-dgn", "headers": {"X-A": "1"} },
      "models": {
        "claude-opus-5.5": {
          "name": "Claude Opus 5.5",
          "attachment": true,
          "reasoning": true,
          "limit": { "context": 1000000, "output": 128000 },
          "headers": {"X-B": "2"},
          "variants": { "high": { "thinking": {"type": "enabled", "budgetTokens": 16000} } },
        },
      },
    },
    "tencent2": {
      "npm": "@ai-sdk/openai-compatible",
      "options": { "baseURL": "https://tokenhub.example.com/plan/v3", "apiKey": "k", "chunkTimeout": 60000,
                   "body": {"x": 1}, "setCacheKey": true },
      "models": {
        "glm-5": {
          "options": { "thinking": { "type": "enabled", "budgetTokens": 8192 }, "extraBody": {"y": 2} },
          "limit": { "context": 204800, "input": 150000, "output": 131072, "compact": 120000 },
          "interleaved": {"field": "reasoning_details"},
        },
      },
    },
    "lbc": {
      "npm": "@ai-sdk/openai-compatible",
      "options": { "baseURL": "https://lbc.example/v1", "apiKey": "k" },
      "models": {
        "qwen": {
          "variants": {
            "low": { "reasoningEffort": "low" },
            "off": { "chat_template_kwargs": { "enable_thinking": false } },
          },
        },
      },
    },
    "weird": { "options": "not an object" },
    "halfweird": {
      "options": { "baseURL": "https://h.example/v1" },
      "models": { "good": {}, "bad": { "limit": "huge" } },
    },
  },
  "model": "hcai/gpt-5.5#high",
  "delay": { "bash": 2 },
  "instructions": ["A.md"],
}"#;

fn write(p: &Path, s: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, s).unwrap();
}

struct Fixture {
    _t: TempDir,
    dirs: Dirs,
    proj: PathBuf,
}

fn fixture() -> Fixture {
    let t = TempDir::new("load");
    let home = t.path().join("home");
    let oc = home.join(".config/opencode");
    let hn = home.join(".config/hernees");
    write(&oc.join("opencode.json"), GLOBAL);
    write(
        &oc.join("agents/review.md"),
        "---\ndescription: Reviews code\nmode: subagent\nmodel: dgn/claude-opus-5.5\npermission:\n  edit: deny\n---\nYou are a reviewer.\n",
    );
    write(&oc.join("agent/nested/deep.md"), "---\ndescription: deep one\n---\nDeep.");
    write(&oc.join("modes/focus.md"), "---\ndescription: primary mode\n---\nFocus.");
    write(&oc.join("commands/git/commit.md"), "---\ndescription: Commit stuff\nagent: build\n---\nCommit $ARGUMENTS\n");
    write(&oc.join("agents/broken.md"), "---\ndescription: [unclosed\n---\nx");
    write(
        &hn.join("hernees.jsonc"),
        r#"{
  // hernees global
  "agent": {
    "build": { "delay": { "edit": 1.5 } },
    "helper": { "description": "Spawnable helper", "spawnable": true, "manage_sessions": false },
    "worker": { "description": "opencode subagent", "mode": "subagent", "reasoningEffort": "low" },
  },
  "compaction": { "ratio": 0.5 },
  "retry": { "max_delay": 60 },
}"#,
    );
    let proj = t.path().join("proj");
    std::fs::create_dir_all(proj.join(".git")).unwrap();
    write(&proj.join("A.md"), "a");
    write(
        &proj.join("opencode.json"),
        r#"{"model": "dgn/claude-opus-5.5", "instructions": ["A.md", "B.md"], "default_agent": "helper",
            "mcp": {"m": {"type": "local", "command": ["echo"]}, "bad": {"type": "nope"}}}"#,
    );
    write(&proj.join(".hernees/hernees.json"), r#"{"agent": {"helper": {"color": "accent"}}}"#);
    write(&proj.join(".opencode/command/hello.md"), "Say hello to $1");
    write(&proj.join("sub/opencode.jsonc"), r#"{"small_model": "lbc/qwen", /* c */ }"#);
    let catalog = t.path().join("models.json");
    std::fs::write(
        &catalog,
        json!({
            "anthropic": {"id": "anthropic", "name": "Anthropic", "npm": "@ai-sdk/anthropic",
                "env": ["HN_FIXTURE_ANTHROPIC"],
                "models": {"claude-x": {"id": "claude-x", "name": "Claude X", "reasoning": true,
                    "limit": {"context": 200000, "output": 64000}, "modalities": {"input": ["text", "image"]}}}},
            "hcai": {"id": "hcai", "name": "Cat HCAI", "npm": "@ai-sdk/openai", "env": [],
                "models": {"cat-only": {"limit": {"context": 50000, "output": 1000}, "interleaved": true}}},
            "deepseek": {"id": "deepseek", "npm": "@ai-sdk/openai-compatible", "api": "https://api.deepseek.com",
                "env": ["HN_FIXTURE_DS"], "models": {"deepseek-chat": {}}},
        })
        .to_string(),
    )
    .unwrap();
    let dirs = Dirs { home: Some(home), opencode: oc, hernees: hn, explicit: vec![], catalog: Some(catalog) };
    Fixture { _t: t, dirs, proj }
}

fn env(k: &str) -> Option<String> {
    match k {
        "HN_FIXTURE_KEY" => Some("sk-fixture".into()),
        "HN_FIXTURE_ANTHROPIC" => Some("sk-ant".into()),
        _ => None,
    }
}

fn load(f: &Fixture, sub: &str) -> Config {
    load_with(&f.proj.join(sub), &f.dirs, &env)
}

#[test]
fn loads_and_merges() {
    let f = fixture();
    let c = load(&f, "");
    // Warnings: weird provider, bad model, bad mcp, broken markdown frontmatter.
    let w = c.warnings.join("\n");
    assert!(w.contains("provider.weird"), "{w}");
    assert!(w.contains("provider.halfweird.models.bad"), "{w}");
    assert!(w.contains("mcp.bad"), "{w}");
    assert!(w.contains("broken.md"), "{w}");
    assert_eq!(c.warnings.len(), 4, "{w}");

    assert!(!c.providers.contains_key("weird"));
    assert_eq!(c.providers["halfweird"].models.keys().collect::<Vec<_>>(), ["good"]);
    assert_eq!(c.providers["hcai"].options["apiKey"], "sk-fixture");
    assert_eq!(c.model.as_ref().unwrap().to_string(), "dgn/claude-opus-5.5"); // project wins
    assert_eq!(c.small_model, None);
    assert_eq!(c.default_agent, "helper");
    assert_eq!(c.instructions, ["A.md", "B.md"]);
    assert_eq!(c.compaction.ratio, 0.5);
    assert!(c.compaction.auto);
    assert_eq!(c.retry.max_delay, 60);
    assert_eq!(c.retry.initial_delay, 2.0);
    assert_eq!(c.mcp.keys().collect::<Vec<_>>(), ["m"]);
    assert_eq!(c.delay["bash"], 2.0);
    assert_eq!(c.root, f.proj);

    // Agents: build first, built-in defaults kept under user overrides.
    let names: Vec<_> = c.agents.keys().map(String::as_str).collect();
    assert_eq!(names[0], "build");
    for n in ["review", "nested/deep", "focus", "helper", "worker"] {
        assert!(names.contains(&n), "{n} missing from {names:?}");
    }
    assert!(!names.contains(&"general") && !names.contains(&"explore") && !names.contains(&"plan"));
    let build = &c.agents["build"];
    assert_eq!(build.description.as_deref(), Some("Default agent with full tool access"));
    assert_eq!(build.manage_sessions, Some(true));
    assert!(build.prompt.is_none());
    assert_eq!(c.delay("build", "edit"), Duration::from_millis(1500));
    assert_eq!(c.delay("build", "write"), Duration::from_millis(1500));
    assert_eq!(c.delay("build", "bash"), Duration::from_secs(2));

    let review = &c.agents["review"];
    assert_eq!(review.prompt.as_deref(), Some("You are a reviewer."));
    assert_eq!(review.mode.as_deref(), Some("subagent"));
    assert_eq!(c.permission("review", "edit", "x"), Action::Deny);
    assert_eq!(c.permission("build", "edit", "x"), Action::Allow);
    assert_eq!(c.agents["focus"].mode.as_deref(), Some("primary"));
    assert_eq!(c.agents["helper"].color.as_deref(), Some("accent"));
    assert_eq!(c.agents["worker"].options["reasoningEffort"], "low");

    let infos = c.agent_infos();
    let sp = |n: &str| infos.iter().find(|a| a.name == n).unwrap().spawnable;
    assert!(sp("build") && sp("helper") && sp("worker") && sp("review"));
    assert!(!sp("focus") && !sp("nested/deep"));
    assert!(infos.iter().find(|a| a.name == "build").unwrap().manage_sessions);
    assert_eq!(infos.iter().find(|a| a.name == "review").unwrap().model.as_ref().unwrap().provider, "dgn");

    // Commands from markdown (nested names, project + global).
    assert_eq!(c.commands["git/commit"].template, "Commit $ARGUMENTS");
    assert_eq!(c.commands["git/commit"].agent.as_deref(), Some("build"));
    assert_eq!(c.commands["hello"].template, "Say hello to $1");
    assert!(c.command_infos().iter().any(|ci| ci.name == "git/commit" && ci.source == "custom"));

    // Nearer dir adds keys.
    let c2 = load(&f, "sub");
    assert_eq!(c2.small_model.as_ref().unwrap().to_string(), "lbc/qwen");
    assert!(c2.sources.iter().any(|s| s.ends_with("sub/opencode.jsonc")));
}

#[test]
fn resolves_models() {
    let f = fixture();
    let c = load(&f, "");
    let r = |s: &str| crate::model::resolve_with(&c, &ModelRef::parse(s).unwrap(), &env);

    let s = r("hcai/gpt-5.5#high").unwrap();
    assert_eq!(s.protocol, Some(Protocol::Chat));
    assert_eq!(s.base_url, "https://ai.example.com/v1");
    assert_eq!(s.api_key.as_deref(), Some("sk-fixture"));
    assert_eq!(s.api_model, "gpt-5.5");
    assert_eq!(
        Value::Object(s.options.clone()),
        json!({"include": ["reasoning.encrypted_content"], "store": false,
               "reasoningEffort": "high", "textVerbosity": "medium", "reasoningSummary": "auto"})
    );
    assert!(s.reasoning && s.image_input);
    assert_eq!((s.context, s.output), (128_000, 32_000));
    assert!(r("hcai/gpt-5.5#off").is_err());
    assert!(r("hcai/gpt-5.5#nope").is_err());
    let s = r("hcai/gpt-5.5").unwrap();
    assert!(!s.options.contains_key("reasoningEffort"));

    let s = r("dgn/claude-opus-5.5#high").unwrap();
    assert_eq!(s.protocol, Some(Protocol::Anthropic));
    assert_eq!(s.base_url, "https://coding.example.cn/v1");
    assert_eq!((s.context, s.output), (1_000_000, 128_000));
    assert_eq!(s.headers.get("X-A").map(String::as_str), Some("1"));
    assert_eq!(s.headers.get("X-B").map(String::as_str), Some("2"));
    assert_eq!(s.options["thinking"]["budgetTokens"], 16000);
    assert!(!s.options.contains_key("headers") && !s.options.contains_key("apiKey"));

    let s = r("tencent2/glm-5").unwrap();
    assert_eq!(s.context, 150_000); // input limit caps context
    assert_eq!(s.output, 131_072);
    assert_eq!(s.chunk_timeout, Some(Duration::from_secs(60)));
    assert_eq!(Value::Object(s.body.clone()), json!({"x": 1, "y": 2}));
    assert_eq!(s.options["thinking"], json!({"type": "enabled", "budgetTokens": 8192}));
    assert_eq!(s.options["setCacheKey"], true);
    assert!(!s.options.contains_key("body") && !s.options.contains_key("extraBody"));
    assert_eq!(s.interleaved_field.as_deref(), Some("reasoning_details"));
    assert!(!s.reasoning);
    assert_eq!(c.context_limits(&s, &ModelRef::parse("tencent2/glm-5").unwrap()), (150_000, 120_000));

    let s = r("lbc/qwen#off").unwrap();
    assert_eq!(s.options["chat_template_kwargs"]["enable_thinking"], false);
    assert!(s.reasoning, "variants with reasoningEffort imply reasoning");
    assert_eq!(c.context_limits(&s, &ModelRef::parse("lbc/qwen").unwrap()), (128_000, 64_000));

    // Catalog-only model under a configured provider: config npm/baseURL win, catalog limits fill.
    let s = r("hcai/cat-only").unwrap();
    assert_eq!(s.protocol, Some(Protocol::Chat));
    assert_eq!(s.base_url, "https://ai.example.com/v1");
    assert_eq!((s.context, s.output), (50_000, 1000));
    assert_eq!(s.interleaved_field.as_deref(), Some("reasoning_content"));

    // Catalog-only provider with env key set.
    let s = r("anthropic/claude-x").unwrap();
    assert_eq!(s.protocol, Some(Protocol::Anthropic));
    assert_eq!(s.base_url, "https://api.anthropic.com/v1");
    assert_eq!(s.api_key.as_deref(), Some("sk-ant"));
    assert!(s.reasoning && s.image_input);
    assert_eq!(s.context, 200_000);
    // Catalog provider without key -> error.
    assert!(r("deepseek/deepseek-chat").is_err());
    assert!(r("nope/x").is_err());
    assert!(r("hcai/missing").is_err());

    // Model list: config models only, variants without disabled ones.
    let list = c.models();
    let gpt = list.iter().find(|m| m.provider == "hcai" && m.model == "gpt-5.5").unwrap();
    assert_eq!(gpt.variants, ["xhigh", "high"]);
    assert_eq!(gpt.provider_name, "HCAI");
    assert!(gpt.image_input && gpt.reasoning);
    assert!(!list.iter().any(|m| m.model == "cat-only"));
    let qwen = list.iter().find(|m| m.model == "qwen").unwrap();
    assert_eq!(qwen.provider_name, "lbc");
    assert_eq!(qwen.name, "qwen");

    // Defaults.
    assert_eq!(c.default_model("review").unwrap().to_string(), "dgn/claude-opus-5.5");
    assert_eq!(c.default_model("build").unwrap().to_string(), "dgn/claude-opus-5.5");
    let mut c3 = c.clone();
    c3.model = None;
    assert_eq!(c3.default_model("build").unwrap().to_string(), "hcai/gpt-5.5");
    c3.agents.get_mut("build").unwrap().model = Some("lbc/qwen".into());
    c3.agents.get_mut("build").unwrap().variant = Some("low".into());
    assert_eq!(c3.default_model("build").unwrap().to_string(), "lbc/qwen#low");
}

#[test]
fn explicit_files_and_global_cwd() {
    let f = fixture();
    let extra = f.proj.join("extra.json");
    write(&extra, r#"{"model": "lbc/qwen", "small_model": "lbc/qwen"}"#);
    let mut dirs = f.dirs.clone();
    dirs.explicit = vec![extra, f.proj.join("missing.json")];
    let c = load_with(&f.proj, &dirs, &env);
    // Project file still beats explicit file for `model`.
    assert_eq!(c.model.as_ref().unwrap().to_string(), "dgn/claude-opus-5.5");
    assert_eq!(c.small_model.as_ref().unwrap().to_string(), "lbc/qwen");
    assert!(c.warnings.iter().any(|w| w.contains("missing.json")));

    // cwd == global config dir: no project walk.
    let c = load_with(&f.dirs.opencode, &f.dirs, &env);
    assert_eq!(c.model.as_ref().unwrap().to_string(), "hcai/gpt-5.5#high");
}

#[test]
fn invalid_file_is_a_warning() {
    let f = fixture();
    write(&f.proj.join(".opencode/opencode.jsonc"), "{ \"model\": ");
    let c = load(&f, "");
    assert!(c.warnings.iter().any(|w| w.contains(".opencode/opencode.jsonc") && w.contains("invalid JSON")));
    assert_eq!(c.model.as_ref().unwrap().to_string(), "dgn/claude-opus-5.5");
}

#[test]
fn user_build_overrides_builtin() {
    let mut w = Vec::new();
    let c = crate::load::extract(
        json!({"agent": {"build": {"prompt": "custom", "manage_sessions": false}, "general": {"description": "g"}}}),
        &mut w,
    );
    assert!(w.is_empty());
    let b = &c.agents["build"];
    assert_eq!(b.prompt.as_deref(), Some("custom"));
    assert_eq!(b.manage_sessions, Some(false));
    assert_eq!(b.spawnable, Some(true));
    assert!(c.agents.contains_key("general"));
    assert_eq!(c.default_agent, "build");
}
