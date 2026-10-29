//! Session-level tools implemented by refd (not ref-tools).

use std::sync::Arc;

use ref_proto::spec::ToolDef;
use ref_proto::{Message, MessageOrigin, Part, Role, SessionInfo, SessionStatus};
use serde_json::{json, Value};

use super::engine::StepEnv;
use super::exec::ToolResult;
use super::manage::CreateOpts;
use super::{store, user_message, SessionCore, Submitted};
use crate::peers::{self, Candidate};
use crate::util;

pub const SESSION_TOOLS: &[&str] = &[
    "context_info",
    "compact",
    "skill",
    "list_sessions",
    "send_message",
    "read_messages",
    "session_context",
    "create_session",
    "close_session",
];

fn def(name: &str, description: &str, parameters: Value) -> ToolDef {
    ToolDef { name: name.into(), description: description.into(), parameters }
}

/// Session tool definitions (deterministic order, without `purpose`).
pub fn defs(can_spawn: bool, has_skills: bool) -> Vec<ToolDef> {
    let mut v = vec![
        def(
            "context_info",
            "Show how much of your context window is used and when compaction is recommended.",
            json!({"type": "object", "properties": {}, "required": []}),
        ),
        def(
            "compact",
            "Replace your whole conversation history with `summary` to free context. The new context starts after the current step; write a complete summary (goal, decisions, work state, next steps, relevant files) since earlier messages will no longer be visible.",
            json!({"type": "object", "properties": {"summary": {"type": "string", "description": "Complete summary to continue from"}}, "required": ["summary"]}),
        ),
    ];
    if has_skills {
        v.push(def(
            "skill",
            "Load a skill's instructions (SKILL.md) by name; see the skills list in the system prompt.",
            json!({"type": "object", "properties": {"name": {"type": "string", "description": "Skill name"}}, "required": ["name"]}),
        ));
    }
    v.extend([
        def(
            "list_sessions",
            "List the other open sessions (agents) on this machine that you can message.",
            json!({"type": "object", "properties": {}, "required": []}),
        ),
        def(
            "send_message",
            "Send a plain-text message to another open session (by name or id). It is delivered immediately if the session is idle, otherwise after its current step.",
            json!({"type": "object", "properties": {
                "to": {"type": "string", "description": "Session name or id (see list_sessions)"},
                "message": {"type": "string", "description": "Message text"}
            }, "required": ["to", "message"]}),
        ),
        def(
            "read_messages",
            "Read the transcript of another session (text and tool calls; reasoning omitted). Newest last; `offset` counts messages from the end (default 0), `limit` defaults to 20.",
            json!({"type": "object", "properties": {
                "session": {"type": "string", "description": "Session name or id"},
                "offset": {"type": "integer", "description": "Messages to skip from the end"},
                "limit": {"type": "integer", "description": "Number of messages (default 20)"}
            }, "required": ["session"]}),
        ),
        def(
            "session_context",
            "Show the context window usage of another session.",
            json!({"type": "object", "properties": {"session": {"type": "string", "description": "Session name or id"}}, "required": ["session"]}),
        ),
    ]);
    if can_spawn {
        v.push(def(
            "create_session",
            "Create a new background session (a separate agent with its own context) that starts working on `prompt` immediately. Use send_message / read_messages to coordinate with it.",
            json!({"type": "object", "properties": {
                "name": {"type": "string", "description": "Short unique name for the session"},
                "agent": {"type": "string", "description": "Agent to use (see the list in the system prompt)"},
                "prompt": {"type": "string", "description": "Complete task description"},
                "cwd": {"type": "string", "description": "Working directory (default: yours)"},
                "must_report": {"type": "boolean", "description": "Require the session to report back to you with send_message before it stops"}
            }, "required": ["name", "agent", "prompt"]}),
        ));
        v.push(def(
            "close_session",
            "Close (abort and unload) a background session. Foreground sessions (with a user attached) and yourself cannot be closed.",
            json!({"type": "object", "properties": {"session": {"type": "string", "description": "Session name or id"}}, "required": ["session"]}),
        ));
    }
    v
}

fn s<'a>(args: &'a Value, k: &str) -> Option<&'a str> {
    args.get(k).and_then(Value::as_str)
}

fn status_word(i: &SessionInfo) -> &'static str {
    if i.pending_permissions > 0 {
        return "waiting-permission";
    }
    match i.status {
        SessionStatus::Idle => "idle",
        SessionStatus::Busy => "busy",
        SessionStatus::Retrying { .. } => "retrying",
    }
}

pub fn context_line(c: &ref_proto::ContextInfo) -> String {
    let pct = if c.max > 0 { (c.used as f64 * 100.0 / c.max as f64).round() as u64 } else { 0 };
    format!("Context: used {} tokens of {} ({}%). Recommended compaction at {} tokens.", c.used, c.max, pct, c.compact_at)
}

pub(crate) async fn execute(core: &Arc<SessionCore>, env: &StepEnv, name: &str, args: Value) -> ToolResult {
    match name {
        "context_info" => {
            let mut c = core.st.lock().info.context.clone();
            if c.used == 0 {
                c.used = core.st.lock().estimate_context();
            }
            ToolResult::ok(context_line(&c))
        }
        "compact" => {
            let Some(summary) = s(&args, "summary").map(str::trim).filter(|x| !x.is_empty()) else {
                return ToolResult::err("Error: summary must not be empty.");
            };
            core.st.lock().compact_summary = Some(summary.to_string());
            ToolResult::ok("Compaction scheduled: after this step your context will be replaced by the summary.")
        }
        "skill" => skill(core, &args),
        "list_sessions" => ToolResult::ok(list_sessions(core)),
        "send_message" => send_message(core, &args),
        "read_messages" => read_messages(core, &args),
        "session_context" => match resolve_any(core, s(&args, "session").unwrap_or("")) {
            Ok(info) => ToolResult::ok(format!("Session \"{}\" ({}): {}", info.name, info.id, context_line(&info.context))),
            Err(e) => ToolResult::err(e),
        },
        "create_session" => create_session(core, env, &args),
        "close_session" => close_session(core, &args).await,
        other => ToolResult::err(format!("Unknown tool: {other}")),
    }
}

fn skill(core: &SessionCore, args: &Value) -> ToolResult {
    let want = s(args, "name").unwrap_or("").trim();
    let skills = core.st.lock().skills.clone();
    let Some(sk) = skills.iter().find(|k| k.name == want).or_else(|| skills.iter().find(|k| k.name.eq_ignore_ascii_case(want)))
    else {
        let names: Vec<&str> = skills.iter().map(|k| k.name.as_str()).collect();
        return ToolResult::err(format!("Error: unknown skill \"{want}\". Available: {}", names.join(", ")));
    };
    match std::fs::read_to_string(&sk.path) {
        Ok(text) => {
            let dir = sk.path.parent().map(|p| p.display().to_string()).unwrap_or_default();
            ToolResult::ok(format!("Skill \"{}\" (base directory: {dir}):\n\n{text}", sk.name))
        }
        Err(e) => ToolResult::err(format!("Error: cannot read {}: {e}", sk.path.display())),
    }
}

fn candidates(core: &SessionCore) -> (Vec<Candidate>, Vec<Candidate>) {
    let open: Vec<Candidate> =
        core.hub.open_sessions().iter().map(|s| Candidate { id: s.id.clone(), name: s.name() }).collect();
    let closed: Vec<Candidate> =
        core.hub.history_all().into_iter().map(|h| Candidate { id: h.id, name: h.name }).collect();
    (open, closed)
}

/// Resolve among open sessions (not self).
fn resolve_open(core: &SessionCore, to: &str) -> Result<Arc<SessionCore>, String> {
    let (open, closed) = candidates(core);
    let id = peers::resolve(to, &open, &closed)?;
    if id == core.id {
        return Err("Error: that is your own session.".into());
    }
    core.hub.session(&id).ok_or_else(|| format!("Error: session {id} is no longer open."))
}

/// Resolve among open and closed sessions (for read-only tools).
fn resolve_any(core: &SessionCore, to: &str) -> Result<SessionInfo, String> {
    let (mut open, closed) = candidates(core);
    open.extend(closed);
    let id = peers::resolve(to, &open, &[])?;
    match core.hub.session(&id) {
        Some(s) => Ok(s.info()),
        None => core.hub.history_info(&id).ok_or_else(|| format!("Error: unknown session {id}")),
    }
}

fn list_sessions(core: &SessionCore) -> String {
    let me = core.info();
    let mut out = String::new();
    let others: Vec<SessionInfo> = core.hub.open_infos().into_iter().filter(|i| i.id != me.id).collect();
    if others.is_empty() {
        out.push_str("No other open sessions.\n");
    } else {
        out.push_str(&format!("{} other open session(s):\n", others.len()));
    }
    for i in others {
        out.push_str(&format!(
            "- \"{}\" id={} agent={} status={} {} cwd={}",
            i.name,
            i.id,
            i.agent,
            status_word(&i),
            if i.attached > 0 { "foreground" } else { "background" },
            i.cwd
        ));
        if let Some(p) = &i.parent {
            out.push_str(&format!(" parent={p}"));
            if *p == me.id {
                out.push_str(" (your child)");
            }
        }
        out.push('\n');
    }
    out.push_str(&format!("You are \"{}\" (id {}).", me.name, me.id));
    out
}

fn send_message(core: &Arc<SessionCore>, args: &Value) -> ToolResult {
    let text = s(args, "message").unwrap_or("");
    if text.trim().is_empty() {
        return ToolResult::err("Error: message must not be empty.");
    }
    if text.len() > peers::MAX_MESSAGE_BYTES {
        return ToolResult::err(format!("Error: message exceeds {} bytes.", peers::MAX_MESSAGE_BYTES));
    }
    let to = s(args, "to").unwrap_or("");
    let target = match resolve_open(core, to) {
        Ok(t) => t,
        Err(e) => return ToolResult::err(e),
    };
    let me = core.info();
    let wrapped = peers::wrap_peer_message(&me.name, &me.id, &me.cwd, text);
    let msg = user_message(MessageOrigin::Peer { from_session: me.id.clone(), from_name: me.name.clone() }, wrapped);
    if me.parent.as_deref() == Some(target.id.as_str()) {
        core.st.lock().reported = true;
    }
    let tname = target.name();
    match target.submit(msg) {
        Submitted::Started => ToolResult::ok(format!("Delivered to \"{tname}\" ({}); it is idle and now processing it.", target.id)),
        Submitted::Queued => {
            ToolResult::ok(format!("Delivered to \"{tname}\" ({}); it is busy and will see it after its current step.", target.id))
        }
    }
}

fn origin_label(m: &Message) -> String {
    match (&m.role, &m.origin) {
        (Role::Assistant, _) => "assistant".into(),
        (_, MessageOrigin::User) => "user".into(),
        (_, MessageOrigin::Peer { from_name, .. }) => format!("message from \"{from_name}\""),
        (_, MessageOrigin::Spawn { from_name, .. }) => format!("task from \"{from_name}\""),
        (_, MessageOrigin::Reminder) => "reminder".into(),
        (_, MessageOrigin::Compaction) => "compaction summary".into(),
    }
}

pub fn format_transcript(info: &SessionInfo, messages: &[Message], offset: usize, limit: usize) -> String {
    let n = messages.len();
    let end = n.saturating_sub(offset);
    let start = end.saturating_sub(limit);
    let mut out = format!("Session \"{}\" ({}), status {}: messages {}-{} of {}", info.name, info.id, status_word(info), start + 1, end, n);
    if start >= end {
        out = format!("Session \"{}\" ({}): no messages in that range ({} total).", info.name, info.id, n);
        return out;
    }
    if start > 0 {
        out.push_str(&format!(" (use offset={} for older)", n - start));
    }
    out.push('\n');
    for (i, m) in messages[start..end].iter().enumerate() {
        out.push_str(&format!("\n[{}] {}:\n", start + i + 1, origin_label(m)));
        for p in &m.parts {
            match p {
                Part::Text(t) if !t.text.trim().is_empty() => {
                    out.push_str(&util::ellipsize(t.text.trim(), 8000));
                    out.push('\n');
                }
                Part::Tool(t) => {
                    let purpose = if t.purpose.is_empty() { String::new() } else { format!(": {}", t.purpose) };
                    out.push_str(&format!("(tool {}{} [{}])\n", t.name, purpose, state_word(t.state)));
                }
                Part::Image(_) => out.push_str("(image)\n"),
                _ => {}
            }
        }
    }
    out
}

fn read_messages(core: &SessionCore, args: &Value) -> ToolResult {
    let info = match resolve_any(core, s(args, "session").unwrap_or("")) {
        Ok(i) => i,
        Err(e) => return ToolResult::err(e),
    };
    let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(20).clamp(1, 200) as usize;
    let messages = match core.hub.session(&info.id) {
        Some(t) => t.st.lock().messages.clone(),
        None => store::load_messages(&core.hub.sessions_dir, &info.id).0,
    };
    ToolResult::ok(format_transcript(&info, &messages, offset, limit))
}

fn create_session(core: &Arc<SessionCore>, env: &StepEnv, args: &Value) -> ToolResult {
    let name = s(args, "name").unwrap_or("").trim();
    let agent = s(args, "agent").unwrap_or("").trim();
    let task = s(args, "prompt").unwrap_or("");
    if task.trim().is_empty() {
        return ToolResult::err("Error: prompt must not be empty.");
    }
    let spawnable: Vec<&str> =
        env.cfg.agents.values().filter(|a| a.spawnable && !a.hidden).map(|a| a.name.as_str()).collect();
    if !spawnable.contains(&agent) {
        return ToolResult::err(format!(
            "Error: agent \"{agent}\" cannot be used for create_session. Valid agents: {}",
            if spawnable.is_empty() { "(none)".to_string() } else { spawnable.join(", ") }
        ));
    }
    let me = core.info();
    let cwd = match s(args, "cwd").map(str::trim).filter(|c| !c.is_empty()) {
        Some(c) => {
            let p = std::path::Path::new(c);
            if p.is_absolute() {
                c.to_string()
            } else {
                std::path::Path::new(&me.cwd).join(p).to_string_lossy().into_owned()
            }
        }
        None => me.cwd.clone(),
    };
    let must_report = args.get("must_report").and_then(Value::as_bool).unwrap_or(false);
    let child = match core.hub.create_session(CreateOpts {
        cwd,
        agent: Some(agent.to_string()),
        name: Some(if name.is_empty() { agent.to_string() } else { name.to_string() }),
        parent: Some(me.id.clone()),
        must_report,
        ..Default::default()
    }) {
        Ok(c) => c,
        Err(e) => return ToolResult::err(format!("Error: {e}")),
    };
    let info = child.info();
    let mut text = format!("[Task from session \"{}\" (id: {}, cwd: {})]\n{}", me.name, me.id, me.cwd, task);
    if must_report {
        text.push_str(&format!("\n\n(When done, report back with send_message to \"{}\".)", me.id));
    }
    child.submit(user_message(MessageOrigin::Spawn { from_session: me.id.clone(), from_name: me.name.clone() }, text));
    let mut out = format!(
        "Created background session \"{}\" (id {}, agent {}, cwd {}). It is now working on the prompt.",
        info.name, info.id, info.agent, info.cwd
    );
    if info.name != name && !name.is_empty() {
        out.push_str(&format!(" (The name \"{name}\" was taken.)"));
    }
    if must_report {
        out.push_str(" It will report back to you with send_message.");
    }
    ToolResult::ok(out)
}

async fn close_session(core: &Arc<SessionCore>, args: &Value) -> ToolResult {
    let to = s(args, "session").unwrap_or("");
    let (open, closed) = candidates(core);
    let id = match peers::resolve(to, &open, &closed) {
        Ok(id) => id,
        Err(e) => return ToolResult::err(e),
    };
    if id == core.id {
        return ToolResult::err("Error: you cannot close your own session.");
    }
    let Some(target) = core.hub.session(&id) else {
        return ToolResult::err(format!("Error: session {id} is not open."));
    };
    if core.hub.attached_count(&id) > 0 {
        return ToolResult::err("Cannot close a foreground session (a user is attached to it)");
    }
    let name = target.name();
    drop(target);
    // Run the close in its own task so aborting this tool call can't leave it half done.
    let hub = core.hub.clone();
    let id2 = id.clone();
    let res = tokio::spawn(async move { hub.close_session(&id2).await })
        .await
        .unwrap_or_else(|e| Err(format!("close failed: {e}")));
    match res {
        Ok(()) => ToolResult::ok(format!("Closed session \"{name}\" ({id}). It remains in the history.")),
        Err(e) => ToolResult::err(format!("Error: {e}")),
    }
}

fn state_word(s: ref_proto::ToolState) -> &'static str {
    use ref_proto::ToolState::*;
    match s {
        Streaming => "streaming",
        AwaitingPermission => "awaiting permission",
        Delayed => "delayed",
        Running => "running",
        Done => "done",
        Error => "error",
        Denied => "denied",
        Cancelled => "cancelled",
    }
}
