//! Cross-session tools (in-process, reliable): list_sessions, send_message, message_status,
//! read_session; spawn tools: create_session, resume_session, close_session.

use crate::hub::{Hub, PeerRecord};
use crate::session::{Session, user_message};
use crate::util::lock;
use ref_llm::ToolDef;
use ref_proto::{
    Message, MessageSource, PEER_COMMAND_INBOX, PEER_COMMAND_NAME, PEER_COMMAND_OUTBOX,
    PEER_COMMAND_PEERS, Part, Role, SessionStatus,
};
use ref_tools::ToolOutput;
use serde_json::{Value, json};
use std::sync::Arc;

/// Names of the cross-session tools.
pub(crate) const LIST_SESSIONS: &str = "list_sessions";
pub(crate) const SEND_MESSAGE: &str = "send_message";
pub(crate) const MESSAGE_STATUS: &str = "message_status";
pub(crate) const READ_SESSION: &str = "read_session";
pub(crate) const CREATE_SESSION: &str = "create_session";
pub(crate) const RESUME_SESSION: &str = "resume_session";
pub(crate) const CLOSE_SESSION: &str = "close_session";

/// Max persisted history sessions merged into `list_sessions` when `history` is true.
const HISTORY_LIST_MAX: usize = 200;

/// `read_session` paging: default and largest page size.
const PAGE_SIZE_DEFAULT: u64 = 10;
const PAGE_SIZE_MAX: u64 = 50;

/// How many of the latest messages `/peers-inbox` and `/peers-outbox` list.
const LIST_LAST: usize = 20;

/// Largest message `send_message` accepts.
const MAX_MESSAGE_BYTES: usize = 100_000;

/// Lengths (in characters) at which texts are cut: previews of messages, text parts and tool
/// outputs in `read_session`, and session titles in `/peers`.
const PREVIEW_CHARS: usize = 80;
const READ_TEXT_CHARS: usize = 4000;
const READ_TOOL_OUTPUT_CHARS: usize = 300;
const LIST_TITLE_CHARS: usize = 40;

/// How the header line of a message from another session starts (see [`peer_header`]).
pub(crate) const PEER_HEADER_PREFIX: &str = "[Message from session";
/// How the footer line of a message from another session starts.
pub(crate) const PEER_FOOTER_PREFIX: &str = "[Reply with send_message";
/// How the header line of the prompt of a created session starts (see [`spawn_header`]).
pub(crate) const SPAWN_HEADER_PREFIX: &str = "[Session created by";
/// How the line that asks a created session to report back starts.
pub(crate) const REPORT_REMINDER_PREFIX: &str = "[When you finish or get blocked";

/// Header line of a message from session `name`.
pub(crate) fn peer_header(name: &str, id: &str) -> String {
    format!("{PEER_HEADER_PREFIX} \"{name}\" (id {id})]")
}

/// Header line of the prompt of a session created by session `name`.
pub(crate) fn spawn_header(name: &str, id: &str) -> String {
    format!("{SPAWN_HEADER_PREFIX} \"{name}\" (id {id})]")
}

fn def(name: &str, description: &str, params: Value) -> ToolDef {
    ToolDef {
        name: name.into(),
        description: description.into(),
        parameters: crate::toolset::with_purpose(&params),
    }
}

pub(crate) fn peer_defs() -> Vec<ToolDef> {
    vec![
        def(
            LIST_SESSIONS,
            "List sessions in this backend: name, id, agent, status, foreground/background, directory, title, and creator. By default lists live sessions only; set history: true to include closed history sessions (newest 200), or creator to filter by \"user\" or creator name/id.",
            json!({"type": "object", "properties": {
                "history": {"type": "boolean", "description": "Also include closed history sessions (newest 200; live wins on duplicate id)."},
                "creator": {"type": "string", "description": "Filter by creator: \"user\" for user-created sessions, or a creator session's name or id."}
            }}),
        ),
        def(
            SEND_MESSAGE,
            "Send a plain-text message to another live session, including while it is busy (it is read after its current step). The message arrives as a user message; the recipient can reply with send_message. Resolve the target with list_sessions first if unsure.",
            json!({"type": "object", "properties": {
                "to": {"type": "string", "description": "Session name or id (see list_sessions)."},
                "message": {"type": "string", "description": "Plain-text message body."}
            }, "required": ["to", "message"]}),
        ),
        def(
            MESSAGE_STATUS,
            "Check whether a message sent with send_message is still queued, delivered into the recipient's context, or already read (the recipient responded since).",
            json!({"type": "object", "properties": {
                "message_id": {"type": "string", "description": "Message id returned by send_message."}
            }, "required": ["message_id"]}),
        ),
        def(
            READ_SESSION,
            "Read another session's messages (live or closed history), newest first, paginated. Tool calls are summarized; reasoning is omitted.",
            json!({"type": "object", "properties": {
                "session": {"type": "string", "description": "Session name or id."},
                "page": {"type": "integer", "minimum": 1, "description": "Page number, 1 = newest (default 1)."},
                "page_size": {"type": "integer", "minimum": 1, "maximum": PAGE_SIZE_MAX,
                    "description": format!("Messages per page (default {PAGE_SIZE_DEFAULT}, max {PAGE_SIZE_MAX}).")}
            }, "required": ["session"]}),
        ),
    ]
}

pub(crate) fn spawn_defs() -> Vec<ToolDef> {
    vec![
        def(
            CREATE_SESSION,
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
            CLOSE_SESSION,
            "Stop and close a background session (for example one you created that has finished). Sessions a user is attached to (foreground) cannot be closed. The session stays in history.",
            json!({"type": "object", "properties": {
                "session": {"type": "string", "description": "Session name or id."}
            }, "required": ["session"]}),
        ),
        def(
            RESUME_SESSION,
            "Resume a closed history session so it becomes live again (or report its status if already live). Optionally send it a prompt message.",
            json!({"type": "object", "properties": {
                "session": {"type": "string", "description": "Session name or id (see list_sessions with history: true)."},
                "prompt": {"type": "string", "description": "Optional plain-text prompt message to send to the resumed session."}
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
        SessionStatus::WaitingQuestion => "waiting for an answer",
        SessionStatus::Retrying { .. } => "retrying",
        SessionStatus::Compacting => "compacting",
    }
}

fn resolve_live(hub: &Hub, q: &str, exclude: Option<&str>) -> Result<Option<Arc<Session>>, String> {
    let q = q.trim();
    if q.is_empty() {
        return Err("Error: session name or id must not be empty.".into());
    }
    let all = hub.sessions();
    let me = |s: &Arc<Session>| Some(s.id.as_str()) == exclude;
    let self_err = || "Error: that is your own session; choose another session.".to_string();
    let accept = |s: &Arc<Session>| {
        if me(s) {
            Err(self_err())
        } else {
            Ok(Some(s.clone()))
        }
    };
    if let Some(s) = all.iter().find(|s| s.id == q) {
        return accept(s);
    }
    let infos: Vec<(Arc<Session>, String)> = all
        .iter()
        .map(|s| (s.clone(), s.lock().info.name.clone()))
        .collect();
    if let Some((s, _)) = infos.iter().find(|(_, n)| n == q) {
        return accept(s);
    }
    let lower = q.to_lowercase();
    let ci: Vec<&(Arc<Session>, String)> = infos
        .iter()
        .filter(|(_, n)| n.to_lowercase() == lower)
        .collect();
    match ci.len() {
        1 => accept(&ci[0].0),
        0 => Ok(None),
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

/// Resolve a live session by id, exact name, then unique case-insensitive name.
/// `exclude` = caller id (the caller cannot target itself).
pub fn resolve(hub: &Hub, q: &str, exclude: Option<&str>) -> Result<Arc<Session>, String> {
    match resolve_live(hub, q, exclude)? {
        Some(s) => Ok(s),
        None => {
            let names: Vec<String> = hub
                .sessions()
                .into_iter()
                .filter(|s| Some(s.id.as_str()) != exclude)
                .map(|s| format!("\"{}\" (id {})", s.lock().info.name, s.id))
                .collect();
            let list = if names.is_empty() {
                "(none)".to_string()
            } else {
                names.join(", ")
            };
            Err(format!(
                "Error: no live session named \"{}\". Live sessions: {list}",
                q.trim()
            ))
        }
    }
}

fn resolve_history_id(store: &crate::store::Store, q: &str) -> Result<String, String> {
    let q = q.trim();
    let infos = store.list();
    if let Some(i) = infos.iter().find(|i| i.id == q) {
        return Ok(i.id.clone());
    }
    let exact: Vec<&ref_proto::SessionInfo> = infos.iter().filter(|i| i.name == q).collect();
    match exact.len() {
        1 => return Ok(exact[0].id.clone()),
        n if n > 1 => {
            let c: Vec<String> = exact
                .iter()
                .map(|i| format!("\"{}\" (id {})", i.name, i.id))
                .collect();
            return Err(format!(
                "Error: \"{q}\" matches multiple history sessions. Candidates: {}. Use the id.",
                c.join(", ")
            ));
        }
        _ => {}
    }
    let lower = q.to_lowercase();
    let ci: Vec<&ref_proto::SessionInfo> = infos
        .iter()
        .filter(|i| i.name.to_lowercase() == lower)
        .collect();
    match ci.len() {
        1 => Ok(ci[0].id.clone()),
        0 => Err(format!(
            "Error: no live or history session matching \"{q}\"."
        )),
        _ => {
            let c: Vec<String> = ci
                .iter()
                .map(|i| format!("\"{}\" (id {})", i.name, i.id))
                .collect();
            Err(format!(
                "Error: \"{q}\" is ambiguous in history. Candidates: {}. Use the id.",
                c.join(", ")
            ))
        }
    }
}

pub async fn run(hub: &Arc<Hub>, s: &Arc<Session>, name: &str, input: &Value) -> ToolOutput {
    let r = match name {
        LIST_SESSIONS => list_sessions(hub, s, input).await,
        SEND_MESSAGE => send_message(hub, s, input),
        MESSAGE_STATUS => message_status(hub, input),
        READ_SESSION => read_session(hub, s, input).await,
        CREATE_SESSION => create_session(hub, s, input).await,
        RESUME_SESSION => resume_session(hub, s, input).await,
        CLOSE_SESSION => close_session(hub, s, input).await,
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

async fn list_sessions(hub: &Arc<Hub>, s: &Arc<Session>, input: &Value) -> Result<String, String> {
    let include_history = input
        .get("history")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let creator = input
        .get("creator")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|c| !c.is_empty());
    let live_infos = hub.infos();
    let live_ids: std::collections::HashSet<String> =
        live_infos.iter().map(|i| i.id.clone()).collect();
    let hist_infos = if include_history {
        let store = hub.store.clone();
        tokio::task::spawn_blocking(move || store.list_for(None, Some(HISTORY_LIST_MAX)))
            .await
            .map_err(|e| format!("listing history failed: {e}"))?
    } else {
        Vec::new()
    };
    let mut names: std::collections::HashMap<String, String> = hist_infos
        .iter()
        .map(|i| (i.id.clone(), i.name.clone()))
        .collect();
    for i in &live_infos {
        names.insert(i.id.clone(), i.name.clone());
    }
    let mut combined: Vec<(ref_proto::SessionInfo, bool)> =
        live_infos.into_iter().map(|i| (i, true)).collect();
    for mut i in hist_infos {
        if !live_ids.contains(&i.id) {
            i.live = false;
            combined.push((i, false));
        }
    }
    if let Some(c) = creator {
        if c == "user" {
            combined.retain(|(i, _)| i.parent.is_none());
        } else {
            let c_lower = c.to_lowercase();
            combined.retain(|(i, _)| {
                i.parent.as_deref().is_some_and(|p| {
                    p == c
                        || names
                            .get(p)
                            .is_some_and(|n| n == c || n.to_lowercase() == c_lower)
                })
            });
        }
    }
    let live_count = combined.iter().filter(|(_, live)| *live).count();
    let hist_count = combined.len() - live_count;
    let mut out = if include_history {
        format!(
            "{} session(s) ({live_count} live, {hist_count} history):\n",
            combined.len()
        )
    } else {
        format!("{live_count} live session(s):\n")
    };
    for (i, is_live) in &combined {
        if *is_live {
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
        } else {
            out.push_str(&format!(
                "- \"{}\" (id {}) — agent {} — status closed — history — cwd {} — title \"{}\"",
                i.name, i.id, i.agent, i.cwd, i.title,
            ));
        }
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
    Ok(out)
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
        "{}\n{text}\n{PEER_FOOTER_PREFIX} to \"{from_id}\" if a reply is needed. Do not reply only to acknowledge.]",
        peer_header(&from_name, &from_id)
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
    lock(&hub.peer_msgs).insert(
        id.clone(),
        PeerRecord {
            from_id,
            to_id: target.id.clone(),
            to_name: target.lock().info.name.clone(),
            time: ref_proto::now_ms(),
            preview: preview(text, PREVIEW_CHARS),
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
    if text.len() > MAX_MESSAGE_BYTES {
        return Err(format!("Error: message exceeds {MAX_MESSAGE_BYTES} bytes."));
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
    let rec = lock(&hub.peer_msgs).get(id).cloned();
    let Some(rec) = rec else {
        return Err(format!("Error: unknown message id \"{id}\"."));
    };
    let name = hub
        .session(&rec.to_id)
        .map_or_else(|| rec.to_name.clone(), |t| t.lock().info.name.clone());
    let status = delivery_status(hub, &rec, id);
    Ok(format!("Message {id} to \"{name}\": {status}."))
}

/// Where a sent message is now, as a phrase (without the final period).
fn delivery_status(hub: &Arc<Hub>, rec: &PeerRecord, id: &str) -> String {
    let Some(t) = hub.session(&rec.to_id) else {
        return "the target session is closed".into();
    };
    let st = t.lock();
    if st.queue.iter().any(|m| m.id == id) {
        return "queued".into();
    }
    let Some(i) = st.find_message(id) else {
        return "delivered (no longer in the recipient's context: compacted or undone)".into();
    };
    let responded = st.messages[i + 1..]
        .iter()
        .any(|m| m.role == Role::Assistant && m.time_end.is_some());
    if responded {
        "read (the recipient has responded since)".into()
    } else {
        "delivered (in context)".into()
    }
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

async fn read_session(hub: &Arc<Hub>, s: &Arc<Session>, input: &Value) -> Result<String, String> {
    let q = str_arg(input, "session")?;
    let page = input
        .get("page")
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .max(1) as usize;
    let size = input
        .get("page_size")
        .and_then(Value::as_u64)
        .unwrap_or(PAGE_SIZE_DEFAULT)
        .clamp(1, PAGE_SIZE_MAX) as usize;
    let live_target = if q.trim() == s.id {
        Some(s.clone())
    } else {
        resolve_live(hub, q, None)?
    };
    if let Some(target) = live_target {
        let st = target.lock();
        return format_session_messages(&st.info.name, &st.messages, page, size);
    }
    let store = hub.store.clone();
    let q = q.to_string();
    let persisted = tokio::task::spawn_blocking(move || {
        let id = resolve_history_id(&store, &q)?;
        store.load(&id)
    })
    .await
    .map_err(|e| format!("reading session failed: {e}"))??;
    format_session_messages(&persisted.info.name, &persisted.messages, page, size)
}

fn format_session_messages(
    name: &str,
    messages: &[Message],
    page: usize,
    size: usize,
) -> Result<String, String> {
    let n = messages.len();
    let pages = n.div_ceil(size).max(1);
    let cstart = crate::convert::effective_start(messages);
    let compacted = messages
        .get(cstart)
        .is_some_and(|m| m.source == MessageSource::Compaction);
    if n == 0 {
        return Ok(format!("Session \"{name}\" has no messages."));
    }
    if page > pages {
        return Err(format!("Error: page {page} is out of range (1–{pages})."));
    }
    // Newest first: page 1 = last `size` messages.
    let hi = n - (page - 1) * size; // exclusive
    let lo = hi.saturating_sub(size);
    let mut out = format!(
        "Session \"{}\": messages {}–{} of {} (page {}/{}, newest first)\n",
        name,
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
        let m = &messages[i];
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
                    out.push_str(&crate::util::truncate_chars(text.trim(), READ_TEXT_CHARS));
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
                        let t = crate::util::truncate_chars(o.trim(), READ_TOOL_OUTPUT_CHARS);
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
            let p = std::path::Path::new(&my_cwd).join(p);
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
    // Agent's own model wins; otherwise inherit the creator's model (never a random fallback).
    let has_own_model =
        cfg.agents.get(&agent).is_some_and(|a| a.model.is_some()) || cfg.model.is_some();
    let opts = crate::ops::CreateOpts {
        cwd: cwd.clone(),
        agent: Some(agent.clone()),
        model: if has_own_model { None } else { Some(my_model) },
        name: Some(base),
        unique_suffix: true,
        parent: Some(s.id.clone()),
        must_report,
    };
    let child = crate::ops::create_session(hub, opts)
        .await
        .map_err(|e| e.message)?;
    let mut text = format!("{}\n{}", spawn_header(&my_name, &s.id), prompt.trim_end());
    if must_report {
        text.push_str(&format!(
            "\n{REPORT_REMINDER_PREFIX}, report the result to \"{my_name}\" with send_message (to: \"{}\").]",
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
async fn resume_session(hub: &Arc<Hub>, s: &Arc<Session>, input: &Value) -> Result<String, String> {
    let (agent, can_spawn) = {
        let st = s.lock();
        let a = st.info.agent.clone();
        let ok = st.cfg.agents.get(&a).is_some_and(|ag| ag.can_spawn);
        (a, ok)
    };
    if !can_spawn {
        return Err(format!(
            "Error: agent \"{agent}\" is not allowed to resume sessions."
        ));
    }
    let q = str_arg(input, "session")?.trim();
    if q.is_empty() {
        return Err("Error: session name or id must not be empty.".into());
    }
    let prompt = match input.get("prompt") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let text = v
                .as_str()
                .ok_or_else(|| "Error: parameter `prompt` must be a string.".to_string())?;
            if text.trim().is_empty() {
                None
            } else {
                if text.len() > MAX_MESSAGE_BYTES {
                    return Err(format!("Error: message exceeds {MAX_MESSAGE_BYTES} bytes."));
                }
                Some(text)
            }
        }
    };
    if let Some(target) = resolve_live(hub, q, None)? {
        let (name, status) = {
            let st = target.lock();
            (st.info.name.clone(), st.info.status.clone())
        };
        let mut out = format!(
            "Session \"{name}\" (id {}) is already live (status: {}).",
            target.id,
            status_word(&status),
        );
        if let Some(text) = prompt {
            if target.id == s.id {
                return Err("Error: that is your own session; choose another session.".into());
            }
            let (msg_id, queued) = deliver(hub, s, &target, text);
            out.push_str(&format!(" Delivered prompt (message id {msg_id})."));
            if queued {
                out.push_str(" They are busy; it will be read after their current step.");
            }
        }
        return Ok(out);
    }
    let store = hub.store.clone();
    let q_owned = q.to_string();
    let id = tokio::task::spawn_blocking(move || resolve_history_id(&store, &q_owned))
        .await
        .map_err(|e| format!("finding history session failed: {e}"))??;
    let resumed = crate::ops::get_or_load(hub, &id).await?;
    let (name, r_agent, cwd) = {
        let st = resumed.lock();
        (
            st.info.name.clone(),
            st.info.agent.clone(),
            st.info.cwd.clone(),
        )
    };
    let mut out = format!(
        "Resumed session \"{name}\" (id {}) (agent {r_agent}, cwd {cwd}).",
        resumed.id
    );
    if let Some(text) = prompt {
        let (msg_id, queued) = deliver(hub, s, &resumed, text);
        out.push_str(&format!(" Delivered prompt (message id {msg_id})."));
        if queued {
            out.push_str(" They are busy; it will be read after their current step.");
        }
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
        .is_some_and(|l| l.starts_with(PEER_HEADER_PREFIX))
    {
        lines.remove(0);
    }
    if lines
        .last()
        .is_some_and(|l| l.starts_with(PEER_FOOTER_PREFIX))
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
        PEER_COMMAND_PEERS | "list-agents" => Ok(cmd_peers(hub, s)),
        PEER_COMMAND_NAME => cmd_name(hub, s, args),
        PEER_COMMAND_INBOX => Ok(cmd_inbox(s)),
        PEER_COMMAND_OUTBOX => Ok(cmd_outbox(hub, s)),
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
            crate::util::truncate_chars(&i.title, LIST_TITLE_CHARS),
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
                preview(&peer_body(m), PREVIEW_CHARS)
            ));
        }
    }
    if !received.is_empty() {
        let n = received.len();
        out.push_str(&format!(
            "{n} received{}:\n",
            if n > LIST_LAST {
                format!(" (last {LIST_LAST})")
            } else {
                String::new()
            }
        ));
        for m in received.iter().skip(n.saturating_sub(LIST_LAST)) {
            out.push_str(&format!(
                "  {} · from \"{}\" · {}\n",
                crate::util::fmt_time(m.time),
                from(m),
                preview(&peer_body(m), PREVIEW_CHARS)
            ));
        }
    }
    out.trim_end().to_string()
}

fn cmd_outbox(hub: &Arc<Hub>, s: &Arc<Session>) -> String {
    let mut sent: Vec<(String, PeerRecord)> = lock(&hub.peer_msgs)
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
        if n > LIST_LAST {
            format!(" (last {LIST_LAST})")
        } else {
            String::new()
        }
    );
    for (id, r) in sent.iter().skip(n.saturating_sub(LIST_LAST)) {
        let status = delivery_status(hub, r, id);
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
