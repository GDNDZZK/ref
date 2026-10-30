//! Cross-session tools (in-process, reliable): list_sessions, send_message, message_status,
//! read_session; spawn tools: create_session, close_session.

use crate::hub::{Hub, PeerRecord};
use crate::session::{Session, user_message};
use ref_llm::ToolDef;
use ref_proto::{Message, MessageSource, Part, Role, SessionStatus};
use ref_tools::ToolOutput;
use serde_json::{Value, json};
use std::sync::Arc;

fn def(name: &str, description: &str, params: Value) -> ToolDef {
    ToolDef {
        name: name.into(),
        description: description.into(),
        parameters: crate::toolset::with_purpose(&params),
    }
}

pub fn peer_defs() -> Vec<ToolDef> {
    vec![
        def(
            "list_sessions",
            "List the live sessions (other agents) in this backend that you can exchange messages with: name, id, agent, status, foreground/background, directory and title.",
            json!({"type": "object", "properties": {}}),
        ),
        def(
            "send_message",
            "Send a plain-text message to another live session, including while it is busy (it is read after its current step). The message arrives as a user message; the recipient can reply with send_message. Resolve the target with list_sessions first if unsure.",
            json!({"type": "object", "properties": {
                "to": {"type": "string", "description": "Session name or id (see list_sessions)."},
                "message": {"type": "string", "description": "Plain-text message body."}
            }, "required": ["to", "message"]}),
        ),
        def(
            "message_status",
            "Check whether a message sent with send_message is still queued, delivered into the recipient's context, or already read (the recipient responded since).",
            json!({"type": "object", "properties": {
                "message_id": {"type": "string", "description": "Message id returned by send_message."}
            }, "required": ["message_id"]}),
        ),
        def(
            "read_session",
            "Read another live session's messages, newest first, paginated. Tool calls are summarized; reasoning is omitted.",
            json!({"type": "object", "properties": {
                "session": {"type": "string", "description": "Session name or id."},
                "page": {"type": "integer", "minimum": 1, "description": "Page number, 1 = newest (default 1)."},
                "page_size": {"type": "integer", "minimum": 1, "maximum": 50, "description": "Messages per page (default 10, max 50)."}
            }, "required": ["session"]}),
        ),
    ]
}

pub fn spawn_defs() -> Vec<ToolDef> {
    vec![
        def(
            "create_session",
            "Create a new background session running another agent (see the list of available agents in the system prompt) and give it a task. It starts working immediately in parallel with you. With must_report (default true) it is required to report its result back to you with send_message.",
            json!({"type": "object", "properties": {
                "name": {"type": "string", "description": "Peer name for the new session (any language; made unique automatically)."},
                "agent": {"type": "string", "description": "Agent to run in the new session."},
                "prompt": {"type": "string", "description": "Complete task description; the new session has none of your context."},
                "cwd": {"type": "string", "description": "Working directory (default: yours; relative paths resolve against yours)."},
                "must_report": {"type": "boolean", "description": "Require the session to report back to you with send_message (default true)."}
            }, "required": ["name", "agent", "prompt"]}),
        ),
        def(
            "close_session",
            "Stop and close a background session (for example one you created that has finished). Sessions a user is attached to (foreground) cannot be closed. The session stays in history.",
            json!({"type": "object", "properties": {
                "session": {"type": "string", "description": "Session name or id."}
            }, "required": ["session"]}),
        ),
    ]
}

fn status_word(s: &SessionStatus) -> &'static str {
    match s {
        SessionStatus::Idle => "idle",
        SessionStatus::Busy => "busy",
        SessionStatus::Running => "running tools",
        SessionStatus::WaitingPermission => "waiting for permission",
        SessionStatus::Retrying { .. } => "retrying",
        SessionStatus::Compacting => "compacting",
    }
}

/// Resolve a session by id, exact name, then unique case-insensitive name.
/// `exclude` = caller id (the caller cannot target itself).
pub fn resolve(hub: &Hub, q: &str, exclude: Option<&str>) -> Result<Arc<Session>, String> {
    let q = q.trim();
    if q.is_empty() {
        return Err("Error: session name or id must not be empty.".into());
    }
    let all = hub.sessions();
    let me = |s: &Arc<Session>| Some(s.id.as_str()) == exclude;
    let self_err = || "Error: that is your own session; choose another session.".to_string();
    if let Some(s) = all.iter().find(|s| s.id == q) {
        return if me(s) {
            Err(self_err())
        } else {
            Ok(s.clone())
        };
    }
    let infos: Vec<(Arc<Session>, String)> = all
        .iter()
        .map(|s| (s.clone(), s.lock().info.name.clone()))
        .collect();
    if let Some((s, _)) = infos.iter().find(|(_, n)| n == q) {
        return if me(s) {
            Err(self_err())
        } else {
            Ok(s.clone())
        };
    }
    let lower = q.to_lowercase();
    let ci: Vec<&(Arc<Session>, String)> = infos
        .iter()
        .filter(|(_, n)| n.to_lowercase() == lower)
        .collect();
    match ci.len() {
        1 => {
            let s = &ci[0].0;
            if me(s) {
                Err(self_err())
            } else {
                Ok(s.clone())
            }
        }
        0 => {
            let names: Vec<String> = infos
                .iter()
                .filter(|(s, _)| !me(s))
                .map(|(s, n)| format!("\"{n}\" (id {})", s.id))
                .collect();
            let list = if names.is_empty() {
                "(none)".to_string()
            } else {
                names.join(", ")
            };
            Err(format!(
                "Error: no live session named \"{q}\". Live sessions: {list}"
            ))
        }
        _ => {
            let c: Vec<String> = ci
                .iter()
                .map(|(s, n)| format!("\"{n}\" (id {})", s.id))
                .collect();
            Err(format!(
                "Error: \"{q}\" is ambiguous. Candidates: {}. Use the id.",
                c.join(", ")
            ))
        }
    }
}

pub async fn run(hub: &Arc<Hub>, s: &Arc<Session>, name: &str, input: &Value) -> ToolOutput {
    let r = match name {
        "list_sessions" => Ok(list_sessions(hub, s)),
        "send_message" => send_message(hub, s, input),
        "message_status" => message_status(hub, input),
        "read_session" => read_session(hub, s, input),
        "create_session" => create_session(hub, s, input).await,
        "close_session" => close_session(hub, s, input).await,
        _ => Err(format!("Unknown tool: {name}")),
    };
    match r {
        Ok(t) => ToolOutput::ok(t),
        Err(e) => ToolOutput::err(e),
    }
}

fn str_arg<'a>(input: &'a Value, k: &str) -> Result<&'a str, String> {
    input
        .get(k)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("Error: missing required string parameter `{k}`."))
}

fn list_sessions(hub: &Arc<Hub>, s: &Arc<Session>) -> String {
    let infos = hub.infos();
    let names: std::collections::HashMap<String, String> = infos
        .iter()
        .map(|i| (i.id.clone(), i.name.clone()))
        .collect();
    let mut out = format!("{} live session(s):\n", infos.len());
    for i in &infos {
        out.push_str(&format!(
            "- \"{}\" (id {}){} — agent {} — status {} — {} — cwd {} — title \"{}\"",
            i.name,
            i.id,
            if i.id == s.id { " (you)" } else { "" },
            i.agent,
            status_word(&i.status),
            if i.attached > 0 {
                "foreground"
            } else {
                "background"
            },
            i.cwd,
            i.title
        ));
        if let Some(p) = &i.parent {
            match names.get(p) {
                Some(n) => out.push_str(&format!(" — created by \"{n}\"")),
                None => out.push_str(&format!(" — created by {p}")),
            }
        }
        out.push('\n');
    }
    let me = s.lock().info.name.clone();
    out.push_str(&format!("You are \"{me}\" (id {}).", s.id));
    out
}

/// Deliver a peer message from `from` to `target`. Returns (message id, queued).
pub fn deliver(
    hub: &Arc<Hub>,
    from: &Arc<Session>,
    target: &Arc<Session>,
    text: &str,
) -> (String, bool) {
    let (from_name, from_id) = (from.lock().info.name.clone(), from.id.clone());
    let body = format!(
        "[Message from session \"{from_name}\" (id {from_id})]\n{text}\n[Reply with send_message to \"{from_id}\" if a reply is needed. Do not reply only to acknowledge.]"
    );
    let msg = user_message(
        MessageSource::Peer {
            from_id: from_id.clone(),
            from_name,
            msg_id: String::new(),
        },
        body,
        vec![],
    );
    let mut msg = msg;
    let id = msg.id.clone();
    if let MessageSource::Peer { msg_id, .. } = &mut msg.source {
        *msg_id = id.clone();
    }
    hub.peer_msgs
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(
            id.clone(),
            PeerRecord {
                from_id,
                to_id: target.id.clone(),
                to_name: target.lock().info.name.clone(),
                time: ref_proto::now_ms(),
                preview: preview(text, 80),
            },
        );
    let queued = crate::engine::submit(hub, target, msg);
    (id, queued)
}

fn send_message(hub: &Arc<Hub>, s: &Arc<Session>, input: &Value) -> Result<String, String> {
    let to = str_arg(input, "to")?;
    let text = str_arg(input, "message")?;
    if text.trim().is_empty() {
        return Err("Error: message must not be empty.".into());
    }
    if text.len() > 100_000 {
        return Err("Error: message exceeds 100000 bytes.".into());
    }
    let target = resolve(hub, to, Some(&s.id))?;
    let (id, queued) = deliver(hub, s, &target, text);
    s.lock().reported = true;
    let name = target.lock().info.name.clone();
    let mut out = format!("Delivered to \"{name}\" ({}). Message id {id}.", target.id);
    if queued {
        out.push_str(" They are busy; it will be read after their current step.");
    }
    Ok(out)
}

fn message_status(hub: &Arc<Hub>, input: &Value) -> Result<String, String> {
    let id = str_arg(input, "message_id")?.trim();
    let rec = hub
        .peer_msgs
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(id)
        .cloned();
    let Some(rec) = rec else {
        return Err(format!("Error: unknown message id \"{id}\"."));
    };
    let Some(t) = hub.session(&rec.to_id) else {
        return Ok(format!(
            "Message {id} to \"{}\": the target session is closed.",
            rec.to_name
        ));
    };
    let st = t.lock();
    let status = if st.queue.iter().any(|m| m.id == id) {
        "queued".to_string()
    } else if let Some(i) = st.find_message(id) {
        let responded = st.messages[i + 1..]
            .iter()
            .any(|m| m.role == Role::Assistant && m.time_end.is_some());
        if responded {
            "read (the recipient has responded since)".into()
        } else {
            "delivered (in context)".into()
        }
    } else {
        "delivered (no longer in the recipient's context: compacted or undone)".into()
    };
    Ok(format!("Message {id} to \"{}\": {status}.", st.info.name))
}

fn role_label(m: &Message) -> String {
    match (&m.role, &m.source) {
        (Role::Assistant, _) => format!(
            "assistant{}",
            m.agent
                .as_ref()
                .map(|a| format!(" ({a})"))
                .unwrap_or_default()
        ),
        (Role::User, MessageSource::Human) => "user".into(),
        (Role::User, MessageSource::Peer { from_name, .. }) => {
            format!("message from \"{from_name}\"")
        }
        (Role::User, MessageSource::Spawn { from_name, .. }) => {
            format!("task from \"{from_name}\"")
        }
        (Role::User, MessageSource::Reminder) => "reminder".into(),
        (Role::User, MessageSource::Compaction) => "compaction summary".into(),
        (Role::User, MessageSource::Shell) => "user shell".into(),
    }
}

fn read_session(hub: &Arc<Hub>, s: &Arc<Session>, input: &Value) -> Result<String, String> {
    let q = str_arg(input, "session")?;
    let target = if q.trim() == s.id {
        s.clone()
    } else {
        resolve(hub, q, None)?
    };
    let page = input
        .get("page")
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .max(1) as usize;
    let size = input
        .get("page_size")
        .and_then(Value::as_u64)
        .unwrap_or(10)
        .clamp(1, 50) as usize;
    let st = target.lock();
    let n = st.messages.len();
    let pages = n.div_ceil(size).max(1);
    let cstart = crate::convert::effective_start(&st.messages);
    let compacted = st
        .messages
        .get(cstart)
        .is_some_and(|m| m.source == MessageSource::Compaction);
    if n == 0 {
        return Ok(format!("Session \"{}\" has no messages.", st.info.name));
    }
    if page > pages {
        return Err(format!("Error: page {page} is out of range (1–{pages})."));
    }
    // Newest first: page 1 = last `size` messages.
    let hi = n - (page - 1) * size; // exclusive
    let lo = hi.saturating_sub(size);
    let mut out = format!(
        "Session \"{}\": messages {}–{} of {} (page {}/{}, newest first)\n",
        st.info.name,
        lo + 1,
        hi,
        n,
        page,
        pages
    );
    if compacted {
        out.push_str(&format!(
            "Context was compacted at #{}: messages before it are no longer in the model's context.\n",
            cstart + 1
        ));
    }
    for i in (lo..hi).rev() {
        let m = &st.messages[i];
        let mark = if compacted && i < cstart {
            " (before compaction)"
        } else {
            ""
        };
        out.push_str(&format!(
            "\n#{} [{}] {}{mark}:\n",
            i + 1,
            crate::util::fmt_time(m.time),
            role_label(m)
        ));
        for p in &m.parts {
            match p {
                Part::Text { text, .. } if !text.trim().is_empty() => {
                    out.push_str(&crate::util::truncate_chars(text.trim(), 4000));
                    out.push('\n');
                }
                Part::Image { .. } => out.push_str("[image]\n"),
                Part::Tool {
                    name,
                    purpose,
                    state,
                    output,
                    ..
                } => {
                    out.push_str(&format!(
                        "  → {name}({purpose}) [{}]\n",
                        serde_json::to_string(state)
                            .unwrap_or_default()
                            .trim_matches('"')
                    ));
                    if let Some(o) = output.as_deref().filter(|o| !o.trim().is_empty()) {
                        let t = crate::util::truncate_chars(o.trim(), 300);
                        for l in t.lines() {
                            out.push_str(&format!("    {l}\n"));
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(e) = &m.error {
            out.push_str(&format!("  (error: {e})\n"));
        }
    }
    Ok(out.trim_end().to_string())
}

async fn create_session(hub: &Arc<Hub>, s: &Arc<Session>, input: &Value) -> Result<String, String> {
    let name = str_arg(input, "name")?;
    let agent = str_arg(input, "agent")?.trim().to_string();
    let prompt = str_arg(input, "prompt")?;
    if prompt.trim().is_empty() {
        return Err("Error: prompt must not be empty.".into());
    }
    let must_report = input
        .get("must_report")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let (cfg, my_cwd, my_name, my_model) = {
        let st = s.lock();
        (
            st.cfg.clone(),
            st.info.cwd.clone(),
            st.info.name.clone(),
            st.info.model.clone(),
        )
    };
    match cfg.agents.get(&agent) {
        Some(a) if a.creatable => {}
        Some(_) => {
            return Err(format!(
                "Error: agent \"{agent}\" cannot be created by other sessions."
            ));
        }
        None => {
            let list: Vec<&str> = cfg
                .agents
                .values()
                .filter(|a| a.creatable && !a.hidden)
                .map(|a| a.name.as_str())
                .collect();
            return Err(format!(
                "Error: unknown agent \"{agent}\". Available: {}",
                list.join(", ")
            ));
        }
    }
    let cwd = match input
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|c| !c.is_empty())
    {
        Some(c) => {
            let p = crate::util::expand_home(c, hub.opts.home.as_deref());
            let p = if p.is_absolute() {
                p
            } else {
                std::path::Path::new(&my_cwd).join(p)
            };
            let p = dunce::canonicalize(&p)
                .map_err(|_| format!("Error: directory does not exist: {}", p.display()))?;
            if !p.is_dir() {
                return Err(format!("Error: not a directory: {}", p.display()));
            }
            p.to_string_lossy().to_string()
        }
        None => my_cwd,
    };
    let base = crate::util::normalize_name(name).map_err(|e| format!("Error: {e}"))?;
    let opts = crate::ops::CreateOpts {
        cwd: cwd.clone(),
        agent: Some(agent.clone()),
        // Agent's own model wins; otherwise inherit the creator's model (never a random fallback).
        model: if cfg.agents.get(&agent).is_some_and(|a| a.model.is_some()) || cfg.model.is_some() {
            None
        } else {
            Some(my_model)
        },
        name: Some(base),
        unique_suffix: true,
        parent: Some(s.id.clone()),
        must_report,
    };
    let child = crate::ops::create_session(hub, opts).await?;
    let mut text = format!(
        "[Session created by \"{my_name}\" (id {})]\n{}",
        s.id,
        prompt.trim_end()
    );
    if must_report {
        text.push_str(&format!(
            "\n[When you finish or get blocked, report the result to \"{my_name}\" with send_message (to: \"{}\").]",
            s.id
        ));
    }
    let msg = user_message(
        MessageSource::Spawn {
            from_id: s.id.clone(),
            from_name: my_name,
        },
        text,
        vec![],
    );
    crate::engine::submit(hub, &child, msg);
    let cname = child.lock().info.name.clone();
    let mut out = format!(
        "Created session \"{cname}\" (id {}) with agent {agent} in {cwd}. It runs in the background",
        child.id
    );
    if must_report {
        out.push_str("; it will report back via a message.");
    } else {
        out.push('.');
    }
    Ok(out)
}

async fn close_session(hub: &Arc<Hub>, s: &Arc<Session>, input: &Value) -> Result<String, String> {
    let q = str_arg(input, "session")?;
    let target = resolve(hub, q, Some(&s.id))?;
    let (name, attached) = {
        let st = target.lock();
        (st.info.name.clone(), st.info.attached)
    };
    if attached > 0 {
        return Err(format!(
            "Cannot close \"{name}\": it is a foreground session (a user is attached)."
        ));
    }
    crate::ops::close_session(hub, &target).await;
    Ok(format!(
        "Closed session \"{name}\" ({}). It remains in history.",
        target.id
    ))
}

// ---------------------------------------------------------------------------
// Slash commands for the user (/peers, /peers-name, /peers-inbox, /peers-outbox).
// They run directly in the backend — no LLM turn, nothing is added to the transcript.
// ---------------------------------------------------------------------------

fn preview(text: &str, max: usize) -> String {
    let one: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    crate::util::truncate_chars(&one, max)
}

fn ago(ms: i64) -> String {
    let secs = (ref_proto::now_ms() - ms).max(0) / 1000;
    if secs < 60 {
        "just now".into()
    } else {
        format!("{} ago", crate::util::fmt_duration(secs))
    }
}

/// Peer message body without the harness header/footer lines.
fn peer_body(m: &Message) -> String {
    let raw = m.raw_text();
    let mut lines: Vec<&str> = raw.lines().collect();
    if lines
        .first()
        .is_some_and(|l| l.starts_with("[Message from session"))
    {
        lines.remove(0);
    }
    if lines
        .last()
        .is_some_and(|l| l.starts_with("[Reply with send_message"))
    {
        lines.pop();
    }
    lines.join("\n")
}

pub fn user_command(
    hub: &Arc<Hub>,
    s: &Arc<Session>,
    command: &str,
    args: &str,
) -> Result<String, String> {
    match command {
        "peers" | "list-agents" => Ok(cmd_peers(hub, s)),
        "peers-name" => cmd_name(hub, s, args),
        "peers-inbox" => Ok(cmd_inbox(s)),
        "peers-outbox" => Ok(cmd_outbox(hub, s)),
        other => Err(format!("Unknown peer command /{other}")),
    }
}

fn cmd_peers(hub: &Arc<Hub>, s: &Arc<Session>) -> String {
    let infos = hub.infos();
    let me = infos
        .iter()
        .find(|i| i.id == s.id)
        .map(|i| i.name.clone())
        .unwrap_or_default();
    let others: Vec<_> = infos.iter().filter(|i| i.id != s.id).collect();
    let mut out = format!("You are \"{me}\" ({}).\n", s.id);
    if others.is_empty() {
        out.push_str("No other live sessions.");
        return out;
    }
    out.push_str(&format!("Other live sessions ({}):\n", others.len()));
    for i in others {
        let queued = hub
            .session(&i.id)
            .map(|t| t.lock().queue.len())
            .unwrap_or(0);
        out.push_str(&format!(
            "  [{}] {} · \"{}\" · {} · {} · {} · started {}{}\n",
            status_word(&i.status),
            i.name,
            crate::util::truncate_chars(&i.title, 40),
            i.agent,
            if i.attached > 0 {
                "foreground"
            } else {
                "background"
            },
            i.cwd,
            ago(i.created_at),
            if queued > 0 {
                format!(" · {queued} queued")
            } else {
                String::new()
            },
        ));
    }
    out.trim_end().to_string()
}

fn cmd_name(hub: &Arc<Hub>, s: &Arc<Session>, args: &str) -> Result<String, String> {
    let args = args.trim();
    if args.is_empty() {
        return Ok(format!("Current name: \"{}\"", s.lock().info.name));
    }
    let name = crate::util::normalize_name(args)?;
    if hub.name_taken(&name, Some(&s.id)) {
        return Err(format!("A live session named \"{name}\" already exists."));
    }
    {
        let mut st = s.lock();
        st.info.name = name.clone();
        hub.emit_info(&st);
    }
    crate::ops::persist(hub, s);
    Ok(format!("Renamed to \"{name}\"."))
}

fn cmd_inbox(s: &Arc<Session>) -> String {
    let st = s.lock();
    let is_peer = |m: &&Message| {
        matches!(
            m.source,
            MessageSource::Peer { .. } | MessageSource::Spawn { .. }
        )
    };
    let from = |m: &Message| match &m.source {
        MessageSource::Peer { from_name, .. } | MessageSource::Spawn { from_name, .. } => {
            from_name.clone()
        }
        _ => String::new(),
    };
    let queued: Vec<&Message> = st.queue.iter().filter(is_peer).collect();
    let received: Vec<&Message> = st.messages.iter().filter(is_peer).collect();
    if queued.is_empty() && received.is_empty() {
        return "Inbox is empty: no messages from other sessions.".into();
    }
    let mut out = String::new();
    if !queued.is_empty() {
        out.push_str(&format!(
            "{} queued (read after the current step):\n",
            queued.len()
        ));
        for m in &queued {
            out.push_str(&format!(
                "  {} · from \"{}\" · {}\n",
                crate::util::fmt_time(m.time),
                from(m),
                preview(&peer_body(m), 80)
            ));
        }
    }
    if !received.is_empty() {
        let n = received.len();
        out.push_str(&format!(
            "{n} received{}:\n",
            if n > 20 { " (last 20)" } else { "" }
        ));
        for m in received.iter().skip(n.saturating_sub(20)) {
            out.push_str(&format!(
                "  {} · from \"{}\" · {}\n",
                crate::util::fmt_time(m.time),
                from(m),
                preview(&peer_body(m), 80)
            ));
        }
    }
    out.trim_end().to_string()
}

fn cmd_outbox(hub: &Arc<Hub>, s: &Arc<Session>) -> String {
    let mut sent: Vec<(String, PeerRecord)> = hub
        .peer_msgs
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|(_, r)| r.from_id == s.id)
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if sent.is_empty() {
        return "Outbox is empty: this session has not sent any messages.".into();
    }
    sent.sort_by_key(|(_, r)| r.time);
    let n = sent.len();
    let mut out = format!(
        "{n} sent message(s){}:\n",
        if n > 20 { " (last 20)" } else { "" }
    );
    for (id, r) in sent.iter().skip(n.saturating_sub(20)) {
        let status = message_status(hub, &json!({ "message_id": id }))
            .ok()
            .and_then(|t| {
                t.rsplit_once(": ")
                    .map(|(_, st)| st.trim_end_matches('.').to_string())
            })
            .unwrap_or_else(|| "unknown".into());
        out.push_str(&format!(
            "  {} · to \"{}\" · {} · {}\n",
            crate::util::fmt_time(r.time),
            r.to_name,
            status,
            r.preview
        ));
    }
    out.trim_end().to_string()
}
