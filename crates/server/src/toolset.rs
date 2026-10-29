//! Tool list sent to the model (fixed order, `purpose` injected into every schema) and
//! dispatch of a single tool call to builtin / harness / peer / MCP implementations.

use crate::hub::Hub;
use crate::mcp::McpServer;
use crate::session::Session;
use ref_config::Config;
use ref_llm::ToolDef;
use ref_proto::{Todo, TodoStatus};
use ref_tools::ToolOutput;
use serde_json::{Map, Value, json};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub const PURPOSE_DESC: &str =
    "One short sentence saying why you are calling this tool; shown to the user. Use the language of the user's messages.";

/// Max MCP text output returned to the model.
const MCP_MAX_BYTES: usize = 50 * 1024;

#[derive(Clone)]
pub enum Kind {
    Builtin,
    Harness,
    Peer,
    Spawn,
    Mcp { server: Arc<McpServer>, tool: String },
}

#[derive(Clone)]
pub struct Entry {
    pub def: ToolDef,
    pub kind: Kind,
}

/// Inserts the required `purpose` property as the FIRST property of a JSON schema.
pub fn with_purpose(schema: &Value) -> Value {
    let mut obj = schema.as_object().cloned().unwrap_or_default();
    obj.entry("type").or_insert_with(|| json!("object"));
    let mut props = Map::new();
    props.insert("purpose".into(), json!({"type": "string", "description": PURPOSE_DESC}));
    if let Some(Value::Object(p)) = obj.get("properties") {
        for (k, v) in p {
            if k != "purpose" {
                props.insert(k.clone(), v.clone());
            }
        }
    }
    let mut required = vec![json!("purpose")];
    if let Some(Value::Array(r)) = obj.get("required") {
        required.extend(r.iter().filter(|x| x.as_str() != Some("purpose")).cloned());
    }
    // Rebuild with a stable key order: type, properties, required, then the rest.
    let mut out = Map::new();
    out.insert("type".into(), obj.remove("type").unwrap_or(json!("object")));
    obj.remove("properties");
    obj.remove("required");
    out.insert("properties".into(), Value::Object(props));
    out.insert("required".into(), Value::Array(required));
    for (k, v) in obj {
        out.insert(k, v);
    }
    Value::Object(out)
}

/// `<server>_<tool>` with chars outside `[A-Za-z0-9_-]` replaced by `_`, max 64 chars.
pub fn mcp_tool_name(server: &str, tool: &str) -> String {
    let raw = format!("{server}_{tool}");
    raw.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .take(64)
        .collect()
}

fn def(name: &str, description: &str, params: Value) -> ToolDef {
    ToolDef { name: name.into(), description: description.into(), parameters: with_purpose(&params) }
}

pub fn harness_defs() -> Vec<ToolDef> {
    vec![
        def(
            "context_window",
            "Show how much of the context window is used (tokens used / max), the recommended compaction point and the automatic compaction threshold. Pass `session` (name or id) to inspect another live session.",
            json!({"type": "object", "properties": {
                "session": {"type": "string", "description": "Name or id of another live session (default: this session)."}
            }}),
        ),
        def(
            "todo",
            "Maintain a todo list for multi-step work; the user sees it live. Each call REPLACES the whole list, so always send every item. Keep exactly one item in_progress while working, mark items completed as soon as they are done, and add follow-ups when discovered. Skip it for trivial single-step tasks.",
            json!({"type": "object", "properties": {
                "todos": {"type": "array", "description": "The complete, updated list.", "items": {
                    "type": "object",
                    "properties": {
                        "content": {"type": "string", "description": "Short imperative description."},
                        "status": {"type": "string", "enum": ["pending", "in_progress", "completed", "cancelled"]}
                    },
                    "required": ["content", "status"]
                }}
            }, "required": ["todos"]}),
        ),
        def(
            "compact_context",
            "Drop the conversation history from your context and continue with only the given summary (the user still sees the full transcript). Use when the context is large (see context_window) or the history is no longer relevant. The summary must contain everything needed to continue: the user's goals and constraints, decisions made, current state, files touched, and next steps.",
            json!({"type": "object", "properties": {
                "summary": {"type": "string", "description": "Complete summary that replaces the conversation history in your context."}
            }, "required": ["summary"]}),
        ),
    ]
}

/// Full tool list for a session in fixed order.
pub fn assemble(cfg: &Config, agent: &str, mcp: &[Arc<McpServer>]) -> Vec<Entry> {
    let mut out = Vec::new();
    let push = |d: ToolDef, kind: Kind, out: &mut Vec<Entry>| {
        if !cfg.tool_disabled(agent, &d.name) && !out.iter().any(|e: &Entry| e.def.name == d.name) {
            out.push(Entry { def: d, kind });
        }
    };
    for s in ref_tools::specs() {
        push(def(s.name, &s.description, s.parameters), Kind::Builtin, &mut out);
    }
    for d in harness_defs() {
        push(d, Kind::Harness, &mut out);
    }
    for d in crate::peers::peer_defs() {
        push(d, Kind::Peer, &mut out);
    }
    if cfg.agents.get(agent).is_some_and(|a| a.can_spawn) {
        for d in crate::peers::spawn_defs() {
            push(d, Kind::Spawn, &mut out);
        }
    }
    let mut mcp_entries = Vec::new();
    for server in mcp {
        for t in server.tools() {
            let name = mcp_tool_name(&server.name, &t.name);
            let desc = if t.description.trim().is_empty() { format!("MCP tool {} from server {}", t.name, server.name) } else { t.description.clone() };
            mcp_entries.push(Entry {
                def: ToolDef { name, description: desc, parameters: with_purpose(&t.input_schema) },
                kind: Kind::Mcp { server: server.clone(), tool: t.name.clone() },
            });
        }
    }
    mcp_entries.sort_by(|a, b| a.def.name.cmp(&b.def.name));
    for e in mcp_entries {
        push(e.def, e.kind, &mut out);
    }
    out
}

/// Permission subject for a call.
pub fn subject(tool: &str, input: &Value) -> String {
    let get = |k: &str| input.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    match tool {
        "bash" => get("command"),
        "read" | "edit" | "write" | "delete" | "grep" | "find" => get("path"),
        "webfetch" => get("url"),
        _ => String::new(),
    }
}

pub type OutputSink = Option<Arc<dyn Fn(&str) + Send + Sync>>;

/// Execute one tool call (permission / delay already handled by the engine).
pub async fn dispatch(
    hub: Arc<Hub>,
    s: Arc<Session>,
    kind: Kind,
    name: String,
    input: Value,
    cancel: CancellationToken,
    on_output: OutputSink,
) -> ToolOutput {
    match kind {
        Kind::Builtin => {
            let cwd = s.lock().info.cwd.clone();
            let ctx = ref_tools::ToolCtx { cwd: cwd.into(), store: s.hashes.clone(), cancel, on_output };
            ref_tools::execute(&name, input, &ctx).await
        }
        Kind::Harness => harness(&hub, &s, &name, &input),
        Kind::Peer | Kind::Spawn => crate::peers::run(&hub, &s, &name, &input).await,
        Kind::Mcp { server, tool } => {
            let Some(client) = server.client() else {
                return ToolOutput::err(format!("MCP server {} is not connected.", server.name));
            };
            match client.call_tool(&tool, input, &cancel).await {
                Ok(r) => {
                    let mut text = r.text;
                    if text.len() > MCP_MAX_BYTES {
                        let total = text.len();
                        text = crate::util::truncate_middle(&text, MCP_MAX_BYTES);
                        text.push_str(&format!("\n\n[Output truncated: {total} bytes total.]"));
                    }
                    ToolOutput { text, images: r.images, meta: None, is_error: r.is_error }
                }
                Err(e) => ToolOutput::err(format!("MCP error: {e:#}")),
            }
        }
    }
}

fn harness(hub: &Arc<Hub>, s: &Arc<Session>, name: &str, input: &Value) -> ToolOutput {
    match name {
        "context_window" => {
            let target = match input.get("session").and_then(Value::as_str).map(str::trim).filter(|x| !x.is_empty()) {
                Some(q) => match crate::peers::resolve(hub, q, None) {
                    Ok(t) => t,
                    Err(e) => return ToolOutput::err(e),
                },
                None => s.clone(),
            };
            let info = target.info();
            let c = info.context;
            let pct = |n: u64| if c.max > 0 { n as f64 * 100.0 / c.max as f64 } else { 0.0 };
            ToolOutput::ok(format!(
                "Context window for \"{}\": used {} tokens of {} ({:.0}%). Recommended compaction at {} tokens ({:.0}%). Automatic compaction at {} tokens.",
                info.name,
                c.used,
                c.max,
                pct(c.used),
                c.recommend,
                pct(c.recommend),
                c.auto
            ))
        }
        "todo" => {
            let Some(items) = input.get("todos").and_then(Value::as_array) else {
                return ToolOutput::err("Error: `todos` must be an array of {content, status}.");
            };
            let mut todos = Vec::with_capacity(items.len());
            for (i, it) in items.iter().enumerate() {
                let content = it.get("content").and_then(Value::as_str).map(str::trim).unwrap_or("");
                if content.is_empty() {
                    return ToolOutput::err(format!("Error: todos[{i}].content must be a non-empty string."));
                }
                let status = match it.get("status").and_then(Value::as_str).unwrap_or("pending") {
                    "pending" => TodoStatus::Pending,
                    "in_progress" => TodoStatus::InProgress,
                    "completed" => TodoStatus::Completed,
                    "cancelled" => TodoStatus::Cancelled,
                    other => {
                        return ToolOutput::err(format!(
                            "Error: todos[{i}].status {other:?} is invalid; use pending, in_progress, completed or cancelled."
                        ))
                    }
                };
                todos.push(Todo { content: content.to_string(), status });
            }
            let text = format_todos(&todos);
            {
                let mut st = s.lock();
                st.info.todos = todos;
                hub.emit_info(&st);
            }
            ToolOutput::ok(text)
        }
        "compact_context" => {
            let summary = input.get("summary").and_then(Value::as_str).unwrap_or("").trim().to_string();
            if summary.is_empty() {
                return ToolOutput::err("Error: summary must not be empty.");
            }
            s.lock().pending_compact = Some(summary);
            ToolOutput::ok("After this batch of tool calls your context will be replaced by your summary.")
        }
        _ => ToolOutput::err(format!("Unknown tool: {name}")),
    }
}

/// Text echoed to the model after a `todo` update.
pub fn format_todos(todos: &[Todo]) -> String {
    if todos.is_empty() {
        return "Todo list cleared.".into();
    }
    let done = todos.iter().filter(|t| t.status == TodoStatus::Completed).count();
    let mut out = format!("Todo list updated ({done}/{} completed):\n", todos.len());
    for t in todos {
        let mark = match t.status {
            TodoStatus::Pending => "[ ]",
            TodoStatus::InProgress => "[~]",
            TodoStatus::Completed => "[x]",
            TodoStatus::Cancelled => "[-]",
        };
        out.push_str(&format!("{mark} {}\n", t.content));
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn purpose_first() {
        let s = with_purpose(&json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}));
        let keys: Vec<&String> = s["properties"].as_object().unwrap().keys().collect();
        assert_eq!(keys, ["purpose", "path"]);
        assert_eq!(s["required"], json!(["purpose", "path"]));
        assert_eq!(s["additionalProperties"], json!(false));
        let e = with_purpose(&json!({}));
        assert_eq!(e["required"], json!(["purpose"]));
    }

    #[test]
    fn mcp_names() {
        assert_eq!(mcp_tool_name("my server", "do.it"), "my_server_do_it");
        assert_eq!(mcp_tool_name("s", &"x".repeat(100)).len(), 64);
    }

    #[test]
    fn order_and_spawn_gating() {
        let mut cfg = Config::default();
        cfg.agents.insert("a".into(), ref_config::AgentConfig { name: "a".into(), can_spawn: true, ..Default::default() });
        cfg.agents.insert("b".into(), ref_config::AgentConfig { name: "b".into(), ..Default::default() });
        let a: Vec<String> = assemble(&cfg, "a", &[]).into_iter().map(|e| e.def.name).collect();
        let b: Vec<String> = assemble(&cfg, "b", &[]).into_iter().map(|e| e.def.name).collect();
        assert_eq!(&a[..7], ["read", "edit", "write", "delete", "grep", "find", "bash"]);
        assert!(a.contains(&"create_session".to_string()));
        assert!(!b.contains(&"create_session".to_string()));
        assert!(b.contains(&"send_message".to_string()));
    }
}
