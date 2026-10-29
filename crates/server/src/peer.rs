//! Harness tools: cross-session (peer) tools, context tools and `skill`.

use crate::hub::Hub;
use crate::session::{history, Input, Session};
use crate::util::{ellipsize, fmt_time, relative_age, truncate_chars};
use async_trait::async_trait;
use hn_config::Config;
use hn_proto::*;
use hn_tools::{Tool, ToolCtx, ToolOutput};
use serde_json::{json, Value};
use std::sync::{Arc, Weak};

pub const MAX_MESSAGE_BYTES: usize = 64 * 1024;

/// Full, deterministically ordered tool list for a session: built-ins, harness tools, MCP.
pub fn session_tools(hub: &Arc<Hub>, config: &Arc<Config>, agent: &str, mcp: Vec<Arc<dyn Tool>>) -> Vec<Arc<dyn Tool>> {
    let mut tools = hn_tools::builtin_tools();
    let w = Arc::downgrade(hub);
    let manage = config.agent(agent).and_then(|a| a.manage_sessions).unwrap_or(false);
    tools.push(Arc::new(ContextInfo(w.clone())));
    tools.push(Arc::new(CompactContext(w.clone())));
    tools.push(Arc::new(ListSessions(w.clone())));
    tools.push(Arc::new(SendMessage(w.clone())));
    tools.push(Arc::new(SessionMessages(w.clone())));
    tools.push(Arc::new(SessionContext(w.clone())));
    if manage {
        tools.push(Arc::new(CreateSession(w.clone())));
        tools.push(Arc::new(CloseSession(w.clone())));
    }
    if !config.skills.is_empty() {
        tools.push(Arc::new(SkillTool(config.clone())));
    }
    for t in mcp {
        if tools.iter().any(|x| x.name() == t.name()) {
            tracing::warn!("MCP tool {} shadows a built-in tool; skipped", t.name());
            continue;
        }
        tools.push(t);
    }
    tools
}

fn hub_of(w: &Weak<Hub>) -> anyhow::Result<Arc<Hub>> {
    w.upgrade().ok_or_else(|| anyhow::anyhow!("server is shutting down"))
}

fn caller(hub: &Hub, ctx: &ToolCtx) -> anyhow::Result<Arc<Session>> {
    hub.get(&ctx.session_id).ok_or_else(|| anyhow::anyhow!("calling session is not open"))
}

fn str_arg<'a>(input: &'a Value, key: &str) -> anyhow::Result<&'a str> {
    match input.get(key) {
        Some(Value::String(s)) => Ok(s.as_str()),
        None | Some(Value::Null) => anyhow::bail!("missing required field \"{key}\""),
        Some(_) => anyhow::bail!("field \"{key}\" must be a string"),
    }
}

fn opt_str<'a>(input: &'a Value, key: &str) -> Option<&'a str> {
    input.get(key).and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty())
}

fn status_word(s: &SessionStatus) -> &'static str {
    match s {
        SessionStatus::Idle => "idle",
        SessionStatus::Busy => "busy",
        SessionStatus::Retry { .. } => "retry",
        SessionStatus::Permission => "permission",
    }
}

fn context_line(info: &SessionInfo, used: u64) -> String {
    let max = info.context.max;
    let pct = if max > 0 { used as f64 * 100.0 / max as f64 } else { 0.0 };
    format!(
        "Context window: used {used} tokens ({pct:.1}%), max {max}, recommended compaction at {}.",
        info.context.compact_at
    )
}

// --- context_info / compact_context ---------------------------------------------

struct ContextInfo(Weak<Hub>);

#[async_trait]
impl Tool for ContextInfo {
    fn name(&self) -> &str {
        "context_info"
    }
    fn description(&self) -> String {
        "Show how much of the context window this session uses and when compaction is recommended.".into()
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {}})
    }
    async fn run(&self, _input: Value, ctx: &ToolCtx) -> anyhow::Result<ToolOutput> {
        let hub = hub_of(&self.0)?;
        let s = caller(&hub, ctx)?;
        let st = s.lock();
        let used = history::used_tokens(&st.msgs);
        Ok(ToolOutput::text(context_line(&st.info, used)))
    }
}

struct CompactContext(Weak<Hub>);

#[async_trait]
impl Tool for CompactContext {
    fn name(&self) -> &str {
        "compact_context"
    }
    fn description(&self) -> String {
        "Compact your context window. Write a summary of everything you still need: goals, user requirements, decisions, current state, relevant files and next steps. After this step, older messages are dropped from your context and only the summary remains. Use it when the context gets large (see context_info) at a natural boundary in the work.".into()
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "summary": {"type": "string", "description": "The distilled information to keep: goals, decisions, state, relevant files, next steps."}
            },
            "required": ["summary"]
        })
    }
    async fn run(&self, input: Value, ctx: &ToolCtx) -> anyhow::Result<ToolOutput> {
        let summary = str_arg(&input, "summary")?.trim();
        anyhow::ensure!(!summary.is_empty(), "summary must not be empty");
        let hub = hub_of(&self.0)?;
        let s = caller(&hub, ctx)?;
        s.lock().checkpoint = Some(summary.to_string());
        Ok(ToolOutput::text("Context compacted; older messages will be dropped after this step."))
    }
}

// --- list_sessions -----------------------------------------------------------------

struct ListSessions(Weak<Hub>);

#[async_trait]
impl Tool for ListSessions {
    fn name(&self) -> &str {
        "list_sessions"
    }
    fn description(&self) -> String {
        "List the other sessions in this Hernees instance (name, id, agent, status, foreground/background, directory, parent, last activity). Use the name or id with send_message, session_messages or session_context.".into()
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "include_closed": {"type": "boolean", "description": "Also list recent closed sessions from history (default false)."}
            }
        })
    }
    async fn run(&self, input: Value, ctx: &ToolCtx) -> anyhow::Result<ToolOutput> {
        let hub = hub_of(&self.0)?;
        let me = caller(&hub, ctx)?.info();
        let now = now_ms();
        let open = hub.open_list();
        let name_of = |id: &str| open.iter().find(|i| i.id == id).map(|i| i.name.clone());
        let others: Vec<&SessionInfo> = open.iter().filter(|i| i.id != me.id).collect();
        let mut lines = Vec::new();
        if others.is_empty() {
            lines.push("No other open sessions.".to_string());
        } else {
            lines.push(format!("{} other open session(s):", others.len()));
            for i in others {
                let mut seg = vec![
                    format!("\"{}\" (id {})", i.name, i.id),
                    format!("agent {}", i.agent),
                    status_word(&i.status).to_string(),
                    if i.is_foreground() { "foreground".into() } else { "background".into() },
                    i.cwd.clone(),
                ];
                if let Some(p) = &i.parent {
                    let pn = if *p == me.id { "you".to_string() } else { name_of(p).map(|n| format!("\"{n}\"")).unwrap_or_default() };
                    seg.push(format!("parent {pn} ({p})"));
                }
                seg.push(format!("last activity {}", relative_age(i.updated_at, now)));
                lines.push(format!("- {}", seg.join(" — ")));
            }
        }
        if input.get("include_closed").and_then(|v| v.as_bool()).unwrap_or(false) {
            let closed: Vec<SessionInfo> = tokio::task::spawn_blocking(crate::persist::list_infos).await.unwrap_or_default();
            let mut closed: Vec<SessionInfo> = closed.into_iter().filter(|i| !open.iter().any(|o| o.id == i.id)).collect();
            closed.sort_by_key(|i| std::cmp::Reverse(i.updated_at));
            if !closed.is_empty() {
                lines.push(format!("Closed sessions (most recent {} of {}):", closed.len().min(30), closed.len()));
                for i in closed.iter().take(30) {
                    lines.push(format!(
                        "- \"{}\" (id {}) — agent {} — {} — {} messages — last activity {}",
                        i.name,
                        i.id,
                        i.agent,
                        i.cwd,
                        i.message_count,
                        relative_age(i.updated_at, now)
                    ));
                }
            }
        }
        lines.push(format!("You are \"{}\" (id {}).", me.name, me.id));
        Ok(ToolOutput::text(lines.join("\n")))
    }
}

// --- send_message ------------------------------------------------------------------

struct SendMessage(Weak<Hub>);

#[async_trait]
impl Tool for SendMessage {
    fn name(&self) -> &str {
        "send_message"
    }
    fn description(&self) -> String {
        "Send a plain-text message to another open session in this Hernees instance. It arrives as a user message: an idle session starts working on it immediately, a busy one reads it at its next step. Resolve targets with list_sessions if unsure.".into()
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "to": {"type": "string", "description": "Target session name or id (see list_sessions)."},
                "message": {"type": "string", "description": "Plain-text message body."}
            },
            "required": ["to", "message"]
        })
    }
    fn title(&self, input: &Value) -> String {
        opt_str(input, "to").map(|t| format!("→ {t}")).unwrap_or_default()
    }
    async fn run(&self, input: Value, ctx: &ToolCtx) -> anyhow::Result<ToolOutput> {
        let to = str_arg(&input, "to")?.trim().to_string();
        let text = str_arg(&input, "message")?;
        if text.trim().is_empty() {
            return Ok(ToolOutput::error("Error: message must not be empty."));
        }
        if text.len() > MAX_MESSAGE_BYTES {
            return Ok(ToolOutput::error(format!("Error: message exceeds {MAX_MESSAGE_BYTES} bytes.")));
        }
        let hub = hub_of(&self.0)?;
        let me = caller(&hub, ctx)?;
        let target = match hub.resolve_open(&to) {
            Ok(t) => t,
            Err(e) => return Ok(ToolOutput::error(format!("Error: {e}"))),
        };
        if target.id == me.id {
            return Ok(ToolOutput::error("Error: cannot send a message to yourself."));
        }
        let mi = me.info();
        let ti = target.info();
        let input = Input::Peer { from_id: mi.id.clone(), from_name: mi.name.clone(), from_cwd: mi.cwd.clone(), text: text.to_string() };
        let idle = match target.enqueue(input) {
            Ok(idle) => idle,
            Err(e) => return Ok(ToolOutput::error(format!("Error: {e}"))),
        };
        if mi.parent.as_deref() == Some(target.id.as_str()) {
            me.lock().meta.reported = true;
        }
        let how = if idle { "it was idle and is now processing it." } else { "it is busy, the message will be read at its next step." };
        Ok(ToolOutput::text(format!("Delivered to \"{}\" ({}); {how}", ti.name, ti.id)).with_title(format!("→ {}", ti.name)))
    }
}

// --- session_messages / session_context ----------------------------------------------

/// Resolve a session among open ones, else persisted history (id or exact name).
async fn find_any(hub: &Hub, target: &str) -> Result<(SessionInfo, Vec<Message>), String> {
    if let Ok(s) = hub.resolve_open(target) {
        let st = s.lock();
        return Ok((st.info.clone(), st.msgs.clone()));
    }
    let t = target.trim().to_string();
    let r = tokio::task::spawn_blocking(move || {
        if crate::persist::valid_id(&t) {
            if let Ok(x) = crate::persist::load(&t) {
                return Ok(x);
            }
        }
        let mut matches: Vec<SessionInfo> = crate::persist::list_infos().into_iter().filter(|i| i.name.trim() == t).collect();
        match matches.len() {
            0 => Err(format!("no session named or with id \"{t}\"")),
            1 => crate::persist::load(&matches.remove(0).id).map_err(|e| e.to_string()),
            _ => Err(format!(
                "\"{t}\" is ambiguous: {}. Use the session id.",
                matches.iter().map(|i| format!("\"{}\" (id {})", i.name, i.id)).collect::<Vec<_>>().join(", ")
            )),
        }
    })
    .await;
    r.unwrap_or_else(|e| Err(e.to_string()))
}

fn origin_label(m: &Message) -> String {
    match (&m.role, &m.origin) {
        (Role::Assistant, _) => "assistant".into(),
        (_, Origin::Human) => "user".into(),
        (_, Origin::Peer { from_name, .. }) => format!("message from \"{from_name}\""),
        (_, Origin::Spawn { from_name, .. }) => format!("task from \"{from_name}\""),
        (_, Origin::System) => "system reminder".into(),
        (_, Origin::Compaction) => "compaction checkpoint".into(),
    }
}

fn state_word(s: ToolState) -> &'static str {
    match s {
        ToolState::Streaming => "streaming",
        ToolState::Permission => "waiting for permission",
        ToolState::Delayed => "delayed",
        ToolState::Running => "running",
        ToolState::Done => "done",
        ToolState::Error => "error",
        ToolState::Denied => "denied",
        ToolState::Cancelled => "cancelled",
    }
}

pub fn render_message(m: &Message) -> String {
    let mut out = format!("[{}] {}", fmt_time(m.time), origin_label(m));
    if let Some(e) = &m.error {
        out.push_str(&format!(" (error: {})", ellipsize(e, 300)));
    }
    if m.finish.as_deref() == Some("aborted") {
        out.push_str(" (aborted)");
    }
    out.push('\n');
    for p in &m.parts {
        match p {
            Part::Text { text, .. } => {
                let (t, cut) = truncate_chars(text.trim(), 2000);
                out.push_str(&t);
                if cut {
                    out.push_str(&format!("\n[… truncated, {} chars total]", text.chars().count()));
                }
                out.push('\n');
            }
            Part::Tool(tp) => {
                let purpose = if tp.purpose.is_empty() { tp.title.clone() } else { tp.purpose.clone() };
                out.push_str(&format!("→ {}({}) [{}]\n", tp.name, crate::util::one_line(&purpose), state_word(tp.state)));
                if !tp.output.is_empty() {
                    let (o, cut) = truncate_chars(tp.output.trim(), 300);
                    for l in o.lines() {
                        out.push_str("  ");
                        out.push_str(l);
                        out.push('\n');
                    }
                    if cut {
                        out.push_str("  […]\n");
                    }
                }
            }
            Part::Image(_) => out.push_str("[image]\n"),
            Part::Reasoning { .. } => {}
        }
    }
    out
}

struct SessionMessages(Weak<Hub>);

#[async_trait]
impl Tool for SessionMessages {
    fn name(&self) -> &str {
        "session_messages"
    }
    fn description(&self) -> String {
        "Read the transcript of another session (open or closed), paginated: page 1 is the most recent messages. Long texts and tool outputs are truncated.".into()
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "session": {"type": "string", "description": "Session name or id."},
                "page": {"type": "integer", "minimum": 1, "description": "Page number, 1 = most recent (default 1)."},
                "page_size": {"type": "integer", "minimum": 1, "maximum": 100, "description": "Messages per page (default 20, max 100)."}
            },
            "required": ["session"]
        })
    }
    async fn run(&self, input: Value, _ctx: &ToolCtx) -> anyhow::Result<ToolOutput> {
        let target = str_arg(&input, "session")?;
        let page = input.get("page").and_then(|v| v.as_u64()).unwrap_or(1).max(1) as usize;
        let size = input.get("page_size").and_then(|v| v.as_u64()).unwrap_or(20).clamp(1, 100) as usize;
        let hub = hub_of(&self.0)?;
        let (info, msgs) = match find_any(&hub, target).await {
            Ok(x) => x,
            Err(e) => return Ok(ToolOutput::error(format!("Error: {e}"))),
        };
        let n = msgs.len();
        if n == 0 {
            return Ok(ToolOutput::text(format!("Session \"{}\" ({}) has no messages.", info.name, info.id)));
        }
        let pages = n.div_ceil(size);
        if page > pages {
            return Ok(ToolOutput::error(format!("Error: page {page} out of range (1-{pages}).")));
        }
        let end = n - (page - 1) * size;
        let start = end.saturating_sub(size);
        let mut out = format!(
            "Session \"{}\" ({}) — messages {}–{} of {n} (page {page}/{pages})\n\n",
            info.name,
            info.id,
            start + 1,
            end
        );
        for m in &msgs[start..end] {
            out.push_str(&render_message(m));
            out.push('\n');
        }
        if page < pages {
            out.push_str(&format!("(Older messages: page {}.)", page + 1));
        }
        Ok(ToolOutput::text(out.trim_end().to_string()))
    }
}

struct SessionContext(Weak<Hub>);

#[async_trait]
impl Tool for SessionContext {
    fn name(&self) -> &str {
        "session_context"
    }
    fn description(&self) -> String {
        "Show the context window usage and status of another session.".into()
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"session": {"type": "string", "description": "Session name or id."}},
            "required": ["session"]
        })
    }
    async fn run(&self, input: Value, _ctx: &ToolCtx) -> anyhow::Result<ToolOutput> {
        let target = str_arg(&input, "session")?;
        let hub = hub_of(&self.0)?;
        let (info, msgs) = match find_any(&hub, target).await {
            Ok(x) => x,
            Err(e) => return Ok(ToolOutput::error(format!("Error: {e}"))),
        };
        let open = hub.get(&info.id).is_some();
        let status = if open { status_word(&info.status) } else { "closed" };
        Ok(ToolOutput::text(format!(
            "Session \"{}\" ({}) — status {status}, model {}.\n{}",
            info.name,
            info.id,
            info.model,
            context_line(&info, history::used_tokens(&msgs))
        )))
    }
}

// --- create_session / close_session --------------------------------------------------

struct CreateSession(Weak<Hub>);

#[async_trait]
impl Tool for CreateSession {
    fn name(&self) -> &str {
        "create_session"
    }
    fn description(&self) -> String {
        "Start a new background session running one of the spawnable agents with an initial prompt. It works independently and in parallel; communicate with send_message. Set must_report to require it to message you with its result before it stops.".into()
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "Name for the new session."},
                "agent": {"type": "string", "description": "Agent to run (see the list of spawnable agents)."},
                "prompt": {"type": "string", "description": "The task / first message for the new session."},
                "cwd": {"type": "string", "description": "Working directory (default: yours; relative paths resolve against yours)."},
                "must_report": {"type": "boolean", "description": "Require the session to report back to you with send_message before stopping (default false)."},
                "model": {"type": "string", "description": "Optional model override as provider/model."}
            },
            "required": ["name", "agent", "prompt"]
        })
    }
    fn title(&self, input: &Value) -> String {
        opt_str(input, "name").unwrap_or("").to_string()
    }
    async fn run(&self, input: Value, ctx: &ToolCtx) -> anyhow::Result<ToolOutput> {
        let name = str_arg(&input, "name")?.trim().to_string();
        let agent = str_arg(&input, "agent")?.trim().to_string();
        let prompt = str_arg(&input, "prompt")?.to_string();
        anyhow::ensure!(!prompt.trim().is_empty(), "prompt must not be empty");
        let must_report = input.get("must_report").and_then(|v| v.as_bool()).unwrap_or(false);
        let hub = hub_of(&self.0)?;
        let me = caller(&hub, ctx)?.info();
        let cwd = match opt_str(&input, "cwd") {
            Some(c) => {
                let p = std::path::Path::new(c);
                let p = if p.is_absolute() { p.to_path_buf() } else { std::path::Path::new(&me.cwd).join(p) };
                p.to_string_lossy().into_owned()
            }
            None => me.cwd.clone(),
        };
        let config = hub.config(&cwd).await;
        let spawnable = config.spawnable_agents();
        if !spawnable.contains(&agent.as_str()) {
            return Ok(ToolOutput::error(format!(
                "Error: agent \"{agent}\" is not spawnable. Spawnable agents: {}",
                if spawnable.is_empty() { "(none)".to_string() } else { spawnable.join(", ") }
            )));
        }
        let model = match opt_str(&input, "model") {
            Some(m) => match ModelRef::parse(m) {
                Some(r) => {
                    if let Err(e) = config.resolve_model(&r) {
                        return Ok(ToolOutput::error(format!("Error: model {m}: {e:#}")));
                    }
                    Some(r)
                }
                None => return Ok(ToolOutput::error(format!("Error: invalid model \"{m}\" (expected provider/model)"))),
            },
            None => None,
        };
        let s = match hub.create_session(&cwd, Some(agent.clone()), model, Some(name), Some(me.id.clone()), must_report).await {
            Ok(s) => s,
            Err(e) => return Ok(ToolOutput::error(format!("Error: {e:#}"))),
        };
        s.enqueue(Input::Spawn { from_id: me.id.clone(), from_name: me.name.clone(), text: prompt, must_report })?;
        let info = s.info();
        let mut out = format!(
            "Created background session \"{}\" (id {}) running agent {} with model {} in {}. It is working on the prompt now.",
            info.name, info.id, info.agent, info.model, info.cwd
        );
        if must_report {
            out.push_str(" It will report back to you with send_message.");
        }
        Ok(ToolOutput::text(out).with_title(info.name))
    }
}

struct CloseSession(Weak<Hub>);

#[async_trait]
impl Tool for CloseSession {
    fn name(&self) -> &str {
        "close_session"
    }
    fn description(&self) -> String {
        "Close a background session (aborting it if busy). It stays in history. Sessions a user is attached to cannot be closed.".into()
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"session": {"type": "string", "description": "Session name or id."}},
            "required": ["session"]
        })
    }
    async fn run(&self, input: Value, ctx: &ToolCtx) -> anyhow::Result<ToolOutput> {
        let target = str_arg(&input, "session")?;
        let hub = hub_of(&self.0)?;
        let me = caller(&hub, ctx)?;
        let s = match hub.resolve_open(target) {
            Ok(s) => s,
            Err(e) => return Ok(ToolOutput::error(format!("Error: {e}"))),
        };
        if s.id == me.id {
            return Ok(ToolOutput::error("Error: you cannot close your own session."));
        }
        let info = s.info();
        if info.is_foreground() {
            return Ok(ToolOutput::error(format!(
                "Error: Session \"{}\" is a foreground session (a user is attached); only background sessions can be closed.",
                info.name
            )));
        }
        let was_busy = s.is_busy();
        s.close(&hub).await;
        Ok(ToolOutput::text(format!(
            "Closed session \"{}\" ({}){}.",
            info.name,
            info.id,
            if was_busy { " (it was busy and has been aborted)" } else { "" }
        )))
    }
}

// --- skill ---------------------------------------------------------------------------

struct SkillTool(Arc<Config>);

#[async_trait]
impl Tool for SkillTool {
    fn name(&self) -> &str {
        "skill"
    }
    fn description(&self) -> String {
        "Load a specialized skill's instructions and resources into the current conversation when the task at hand matches its description.\n\nThe skill name must match an available skill or a skill explicitly referenced by the user.".into()
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"name": {"type": "string", "description": "Skill name."}},
            "required": ["name"]
        })
    }
    fn title(&self, input: &Value) -> String {
        opt_str(input, "name").unwrap_or("").to_string()
    }
    async fn run(&self, input: Value, _ctx: &ToolCtx) -> anyhow::Result<ToolOutput> {
        let name = str_arg(&input, "name")?.trim();
        match self.0.skill(name) {
            Some(s) => {
                let s = s.clone();
                let text = tokio::task::spawn_blocking(move || hn_config::skill_content(&s)).await?;
                Ok(ToolOutput::text(text).with_title(name.to_string()))
            }
            None => {
                let names: Vec<&str> = self.0.skills.iter().map(|s| s.name.as_str()).collect();
                Ok(ToolOutput::error(format!("Error: unknown skill \"{name}\". Available skills: {}", names.join(", "))))
            }
        }
    }
}
