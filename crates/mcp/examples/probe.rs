//! Connect to the MCP servers configured in `~/.config/opencode/opencode.json` and list
//! their tools. Header values are never printed.
//!
//!   cargo run -p hn-mcp --example probe [server-name ...]
//!   cargo run -p hn-mcp --example probe -- --call <server> <tool> '<json args>'

use hn_config::McpConfig;
use indexmap::IndexMap;
use serde_json::Value;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let path = std::env::var_os("HN_PROBE_CONFIG")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".config/opencode/opencode.json")))
        .ok_or_else(|| anyhow::anyhow!("no home dir"))?;
    let text = std::fs::read_to_string(&path)?;
    let doc: Value = json5::from_str(&text).map_err(|e| anyhow::anyhow!("parse {}: {e}", path.display()))?;
    let servers: IndexMap<String, McpConfig> = serde_json::from_value(doc.get("mcp").cloned().unwrap_or(Value::Null))
        .map_err(|e| anyhow::anyhow!("invalid `mcp` section: {e}"))?;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let cwd = std::env::current_dir()?;

    if args.first().map(String::as_str) == Some("--call") {
        let [_, server, tool, json_args] = args.as_slice() else {
            anyhow::bail!("usage: --call <server> <tool> '<json args>'");
        };
        let cfg = servers.get(server).ok_or_else(|| anyhow::anyhow!("no server `{server}`"))?;
        let client = hn_mcp::connect(server, cfg, &cwd).await?;
        let r = client.call(tool, serde_json::from_str(json_args)?, &Default::default()).await?;
        println!("is_error={} images={}\n{}", r.is_error, r.images.len(), r.text);
        client.shutdown().await;
        return Ok(());
    }

    for (name, cfg) in &servers {
        if !args.is_empty() && !args.contains(name) {
            continue;
        }
        let target = match cfg {
            McpConfig::Local { command, .. } => format!("local `{}`", command.first().map(String::as_str).unwrap_or("")),
            McpConfig::Remote { url, headers, .. } => {
                format!("remote {url} (headers: {})", headers.keys().cloned().collect::<Vec<_>>().join(", "))
            }
        };
        println!("== {name}: {target}");
        if !cfg.enabled() {
            println!("   disabled");
            continue;
        }
        let start = std::time::Instant::now();
        match hn_mcp::connect(name, cfg, &cwd).await {
            Ok(c) => {
                println!(
                    "   connected via {} in {:?}, protocol {}, server {}",
                    c.transport(),
                    start.elapsed(),
                    c.protocol_version,
                    c.server_info.as_ref().map(|v| v.to_string()).unwrap_or_default()
                );
                if let Some(i) = &c.instructions {
                    println!("   instructions: {} chars", i.chars().count());
                }
                for t in c.tools() {
                    let props = t.schema.get("properties").and_then(Value::as_object).map(|p| p.keys().cloned().collect::<Vec<_>>().join(",")).unwrap_or_default();
                    let desc: String = t.description.chars().take(70).collect();
                    println!("   - {} ({}) [{props}]: {}", t.name, t.tool, desc.replace('\n', " "));
                }
                c.shutdown().await;
            }
            Err(e) => println!("   FAILED: {e:#}"),
        }
    }
    Ok(())
}
