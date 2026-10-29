//! System prompt assembly. Kept minimal and cache-stable: no volatile data (time, session
//! lists, usage); the date has day granularity and goes in the last part.

use hn_config::Config;
use hn_proto::SessionInfo;
use hn_tools::Tool;
use std::path::Path;
use std::sync::Arc;

pub fn system(config: &Config, info: &SessionInfo, mcp_instructions: &[(String, String)], tools: &[Arc<dyn Tool>]) -> Vec<String> {
    let mut parts = Vec::new();
    let has = |n: &str| tools.iter().any(|t| t.name() == n);

    // 1. Base + agent prompt.
    let mut base = format!(
        "You are {}, running in Hernees, a coding agent harness. Use the tools to help the user. Tool results and `<system-reminder>` blocks come from the harness, not the user.",
        info.agent
    );
    if let Some(p) = config.agent(&info.agent).and_then(|a| a.prompt.as_deref()).map(str::trim).filter(|p| !p.is_empty()) {
        base.push_str("\n\n");
        base.push_str(p);
    }
    parts.push(base);

    // 2. Project context: MCP instructions, skills, instruction files, identity, agents.
    let mut ctx: Vec<String> = Vec::new();
    for (server, text) in mcp_instructions {
        if !text.trim().is_empty() {
            ctx.push(format!("Instructions from MCP server {server}:\n{}", text.trim()));
        }
    }
    if has("skill") {
        let sp = hn_config::skills_prompt(&config.skills);
        if !sp.is_empty() {
            ctx.push(sp);
        }
    }
    for (path, content) in config.instruction_files(Path::new(&info.cwd)) {
        ctx.push(format!("Instructions from: {}\n{}", path.display(), content));
    }
    ctx.push(format!(
        "You are session id {} in this Hernees instance. Other sessions can message you; use list_sessions / send_message to communicate with them.",
        info.id
    ));
    if has("create_session") {
        let agents: Vec<String> = config
            .agent_infos()
            .into_iter()
            .filter(|a| a.spawnable)
            .map(|a| {
                if a.description.is_empty() {
                    format!("- {}", a.name)
                } else {
                    format!("- {}: {}", a.name, hn_proto_one_line(&a.description))
                }
            })
            .collect();
        if !agents.is_empty() {
            ctx.push(format!("Agents you can start with create_session:\n{}", agents.join("\n")));
        }
    }
    parts.push(ctx.join("\n\n"));

    // 3. Environment (date: day granularity).
    let root = config.root.to_string_lossy().into_owned();
    let root = if root.is_empty() { info.cwd.clone() } else { root };
    let git = Path::new(&root).join(".git").exists();
    parts.push(format!(
        "Here is some useful information about the environment you are running in:\n<env>\n  Working directory: {}\n  Workspace root folder: {}\n  Is directory a git repo: {}\n  Platform: {}\n  Today's date: {}\n</env>",
        info.cwd,
        root,
        if git { "yes" } else { "no" },
        std::env::consts::OS,
        chrono::Local::now().format("%a %b %d %Y"),
    ));
    parts
}

fn hn_proto_one_line(s: &str) -> String {
    crate::util::one_line(s)
}
