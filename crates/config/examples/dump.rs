//! Load the config for the current directory and print a summary with secrets redacted.
//!
//! `cargo run -p hn-config --example dump [-- <dir>]`

use hn_config::{agent_spawnable, Config};
use serde_json::Value;

fn redact(s: &str) -> String {
    if s.is_empty() {
        "<empty>".into()
    } else {
        format!("<redacted, {} chars>", s.chars().count())
    }
}

fn redact_value(v: &Value) -> Value {
    const SECRET: &[&str] = &["apikey", "api_key", "authorization", "x-api-key", "api-key", "token", "access_token"];
    match v {
        Value::Object(m) => Value::Object(
            m.iter()
                .map(|(k, v)| {
                    let lk = k.to_ascii_lowercase();
                    if SECRET.contains(&lk.as_str()) || lk.contains("secret") || lk.contains("password") {
                        (k.clone(), Value::String(redact(v.as_str().unwrap_or("?"))))
                    } else {
                        (k.clone(), redact_value(v))
                    }
                })
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(redact_value).collect()),
        v => v.clone(),
    }
}

fn main() -> anyhow::Result<()> {
    let cwd = match std::env::args().nth(1) {
        Some(d) => d.into(),
        None => std::env::current_dir()?,
    };
    let c = Config::load(&cwd)?;
    println!("root: {}", c.root.display());
    println!("sources:");
    for s in &c.sources {
        println!("  {}", s.display());
    }
    println!("warnings: {}", c.warnings.len());
    for w in &c.warnings {
        println!("  ! {w}");
    }
    println!("catalog providers: {}", c.catalog.0.len());
    println!("model: {:?}  small_model: {:?}  default_agent: {}", c.model.as_ref().map(|m| m.to_string()), c.small_model.as_ref().map(|m| m.to_string()), c.default_agent);

    println!("\nproviders:");
    for (id, p) in &c.providers {
        let opts = redact_value(&Value::Object(p.options.clone()));
        println!("  {id} ({}) npm={:?} options={opts}", p.name.as_deref().unwrap_or(id), p.npm);
    }

    println!("\nmodels:");
    for m in c.models() {
        let r = hn_proto::ModelRef { provider: m.provider.clone(), model: m.model.clone(), variant: m.variants.first().cloned() };
        match c.resolve_model(&r) {
            Ok(s) => {
                let (max, compact) = c.context_limits(&s, &r);
                println!(
                    "  {r} [{}] {} ctx={} out={} reasoning={} image={} key={} compact_at={compact}/{max}",
                    s.protocol().as_str(),
                    s.base_url,
                    s.context,
                    s.output,
                    s.reasoning,
                    s.image_input,
                    s.api_key.as_deref().map(redact).unwrap_or_else(|| "none".into()),
                );
                println!("      variants={:?} options={}", m.variants, redact_value(&Value::Object(s.options)));
                if !s.body.is_empty() {
                    println!("      body={}", redact_value(&Value::Object(s.body)));
                }
                if let Some(f) = s.interleaved_field {
                    println!("      interleaved={f}");
                }
            }
            Err(e) => println!("  {r}: ERROR {e}"),
        }
    }

    println!("\nagents:");
    for (n, a) in &c.agents {
        println!(
            "  {n}: {:?} mode={:?} model={:?} spawnable={} manage_sessions={} prompt={} delay={:?}",
            a.description.as_deref().unwrap_or(""),
            a.mode,
            a.model,
            agent_spawnable(a),
            a.manage_sessions.unwrap_or(false),
            a.prompt.as_ref().map(|p| p.len()).unwrap_or(0),
            a.delay,
        );
    }
    println!("default model for {}: {:?}", c.default_agent, c.default_model(&c.default_agent).map(|m| m.to_string()));

    println!("\nmcp:");
    for (n, m) in &c.mcp {
        let v = redact_value(&serde_json::to_value(m)?);
        println!("  {n}: enabled={} {v}", m.enabled());
    }
    println!("\npermission rules: {:?}", c.permission.rules);
    println!("delay: {:?}", c.delay);
    println!("commands: {:?}", c.commands.keys().collect::<Vec<_>>());
    println!("skills: {:?}", c.skills.iter().map(|s| &s.name).collect::<Vec<_>>());
    println!("instruction files:");
    for (p, t) in c.instruction_files(&cwd) {
        println!("  {} ({} bytes)", p.display(), t.len());
    }
    Ok(())
}
