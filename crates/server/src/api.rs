//! Request dispatch for one frontend connection.

use crate::hub::{Conn, Hub};
use crate::session::{PermReply, REMINDER_PREFIX, Session, user_message};
use crate::util::lock;
use ref_proto::{
    AgentInfo, CommandInfo, Message, MessageSource, ModelInfo, Part, Request, Role,
    ServerConfigInfo, SessionInfo, SessionSnapshot,
};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

type Reply = Result<Value, String>;

/// Sessions `ListHistory` returns when the client sets no limit.
const HISTORY_DEFAULT_LIMIT: usize = 100;

/// Files `ListFiles` returns by default, and at most.
const FILES_DEFAULT_LIMIT: usize = 50;
const FILES_MAX_LIMIT: usize = 500;

/// Timeout of a `Shell` request (the `bash` tool takes seconds).
const SHELL_TIMEOUT_SECS: u64 = 600;

/// Time a ``!`cmd` `` in a command template may run.
const EXPAND_SHELL_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a directory's file list is reused, and how many directories are cached.
const FILES_CACHE_TTL: Duration = Duration::from_secs(10);
const FILES_CACHE_MAX_DIRS: usize = 32;

fn live(hub: &Arc<Hub>, id: &str) -> Result<Arc<Session>, String> {
    hub.session(id)
        .ok_or_else(|| format!("Session {id} is not live (attach to it first)."))
}

pub async fn handle(hub: &Arc<Hub>, conn: &Arc<Conn>, req: Request) -> Reply {
    match req {
        Request::Hello { .. } => Err("Hello was already received.".into()),
        Request::Ping => Ok(json!({})),
        Request::SetNotify { kinds } => {
            conn.lock().notify = kinds.into_iter().collect();
            Ok(json!({}))
        }
        Request::SetFocus { focused, sessions } => {
            let stamp = focused.then(|| hub.next_stamp());
            let mut st = conn.lock();
            st.focused = focused;
            st.shown = sessions.into_iter().collect();
            if let Some(stamp) = stamp {
                st.focus_stamp = stamp;
            }
            Ok(json!({}))
        }
        Request::ListDirs { path } => {
            let cwd = conn.lock().cwd.clone();
            list_dirs(hub, &cwd, &path).await
        }
        Request::Shutdown => {
            tracing::info!("shutdown requested by a client");
            hub.shutdown.cancel();
            Ok(json!({}))
        }
        Request::GetConfig => {
            let cwd = conn.lock().cwd.clone();
            Ok(json!({ "config": config_info(hub, &cwd).await? }))
        }
        Request::ListSessions => Ok(json!({ "sessions": hub.infos() })),
        Request::McpStatus { cwd } => {
            let cwd = cwd
                .filter(|c| !c.trim().is_empty())
                .map(|c| hub.effective_cwd(&c))
                .unwrap_or_else(|| conn.lock().cwd.clone());
            let cfg = crate::ops::load_config(hub, &cwd).await?;
            Ok(json!({ "servers": hub.mcp.status(&cfg) }))
        }
        Request::ListHistory { cwd, limit } => list_history(hub, cwd, limit).await,
        Request::CreateSession {
            cwd,
            agent,
            model,
            name,
        } => {
            let s = crate::ops::create_session(
                hub,
                crate::ops::CreateOpts {
                    cwd,
                    agent,
                    model,
                    name,
                    unique_suffix: false,
                    parent: None,
                    must_report: false,
                },
            )
            .await?;
            Ok(json!({ "session": s.info() }))
        }
        Request::Attach {
            session_id,
            foreground,
            limit,
        } => attach(hub, conn, session_id, foreground, limit).await,
        Request::Messages {
            session_id,
            before,
            limit,
        } => {
            let s = live(hub, &session_id)?;
            let st = s.lock();
            let end = st
                .find_message(&before)
                .ok_or_else(|| "That message is no longer in the conversation.".to_string())?;
            let (messages, more_before) = crate::session::page(&st.messages, end, limit);
            Ok(json!({ "messages": messages, "more_before": more_before }))
        }
        Request::GetImage {
            session_id,
            image_id,
        } => get_image(hub, session_id, image_id).await,
        Request::Detach { session_id } => {
            if let Some(s) = hub.session(&session_id) {
                let mut st = s.lock();
                let was_fg = {
                    let mut c = conn.lock();
                    c.watch.remove(&session_id);
                    c.fg.remove(&session_id)
                };
                if was_fg {
                    st.info.attached = st.info.attached.saturating_sub(1);
                    hub.emit_info(&st);
                }
            }
            Ok(json!({}))
        }
        Request::WatchAll { enabled } => {
            conn.lock().watch_all = enabled;
            Ok(json!({ "sessions": hub.infos() }))
        }
        Request::Prompt {
            session_id,
            text,
            images,
        } => {
            let s = live(hub, &session_id)?;
            if text.trim().is_empty() && images.is_empty() {
                return Err("Prompt is empty.".into());
            }
            if images.iter().any(ref_proto::ImageData::is_stub) {
                return Err("An attached image has no data.".into());
            }
            reload_config(hub, &s).await;
            let images = if s.lock().cfg.images_auto_resize {
                fit_images(images).await?
            } else {
                images
            };
            let msg = user_message(MessageSource::Human, text, images);
            let id = msg.id.clone();
            let queued = crate::engine::submit(hub, &s, msg);
            Ok(json!({ "queued": queued, "message_id": id }))
        }
        Request::RemoveQueued {
            session_id,
            message_id,
        } => {
            let s = live(hub, &session_id)?;
            Ok(match crate::engine::remove_queued(hub, &s, &message_id) {
                Some(m) => {
                    json!({ "removed": true, "text": m.raw_text(), "images": m.prompt_images() })
                }
                None => json!({ "removed": false }),
            })
        }
        Request::Shell {
            session_id,
            command,
        } => {
            let s = live(hub, &session_id)?;
            let (hub2, s2) = (hub.clone(), s.clone());
            tokio::spawn(async move { run_shell(hub2, s2, command).await });
            Ok(json!({}))
        }
        Request::Abort { session_id } => {
            let s = live(hub, &session_id)?;
            crate::engine::abort(hub, &s, true);
            Ok(json!({}))
        }
        Request::PermissionReply {
            session_id,
            call_id,
            allow,
            always,
        } => {
            let s = live(hub, &session_id)?;
            let tx = {
                let mut st = s.lock();
                st.perms
                    .iter_mut()
                    .find(|p| p.request.call_id == call_id)
                    .and_then(|p| p.tx.take())
            };
            match tx {
                Some(tx) => {
                    // A gone receiver means the waiting tool call already ended.
                    let _ = tx.send(PermReply { allow, always });
                    Ok(json!({}))
                }
                None => Err("No pending permission request with that id.".into()),
            }
        }
        Request::QuestionReply {
            session_id,
            call_id,
            answers,
        } => {
            let s = live(hub, &session_id)?;
            crate::question::reply(&s, &call_id, answers)?;
            Ok(json!({}))
        }
        Request::SetModel { session_id, model } => {
            let s = live(hub, &session_id)?;
            let mut st = s.lock();
            if st.cfg.model(&model).is_none() {
                return Err(format!("Model {} is not configured.", model.key()));
            }
            st.info.context =
                crate::models::context_keeping_used(&st.cfg, &model, st.info.context.used);
            st.info.model = model;
            hub.emit_info(&st);
            Ok(json!({ "session": st.info.clone() }))
        }
        Request::SetAgent { session_id, agent } => set_agent(hub, session_id, agent),
        Request::SetVariant {
            session_id,
            variant,
        } => set_variant(hub, session_id, variant),
        Request::Rename { session_id, name } => {
            let s = live(hub, &session_id)?;
            let name = crate::util::normalize_name(&name)?;
            if hub.name_taken(&name, Some(&session_id)) {
                return Err(format!("A live session named \"{name}\" already exists."));
            }
            {
                let mut st = s.lock();
                st.info.name = name;
                hub.emit_info(&st);
            }
            crate::ops::persist(hub, &s);
            Ok(json!({ "session": s.info() }))
        }
        Request::SetCwd { session_id, path } => {
            let s = live(hub, &session_id)?;
            set_cwd(hub, &s, &path).await?;
            Ok(json!({ "session": s.info() }))
        }
        Request::Compact { session_id } => {
            let s = live(hub, &session_id)?;
            crate::engine::start_custom_run(hub, &s, crate::compaction::manual)?;
            Ok(json!({}))
        }
        Request::PeerCommand {
            session_id,
            command,
            args,
        } => {
            let s = live(hub, &session_id)?;
            let text = crate::peers::user_command(hub, &s, &command, &args)?;
            Ok(json!({ "text": text }))
        }
        Request::Undo {
            session_id,
            message_id,
        } => undo(hub, session_id, message_id).await,
        Request::Close { session_id } => {
            let s = live(hub, &session_id)?;
            crate::ops::close_session(hub, &s).await;
            Ok(json!({}))
        }
        Request::Delete { session_id } => {
            if let Some(s) = hub.session(&session_id) {
                crate::ops::close_session(hub, &s).await;
            }
            let store = hub.store.clone();
            tokio::task::spawn_blocking(move || store.delete(&session_id))
                .await
                .map_err(|e| e.to_string())??;
            Ok(json!({}))
        }
        Request::ExpandCommand { name, args, cwd } => {
            let cwd = hub.effective_cwd(&cwd);
            let cfg = crate::ops::load_config(hub, &cwd).await?;
            let Some(cmd) = cfg.commands.get(&name) else {
                return Err(format!("Unknown command: /{name}"));
            };
            let text = expand_template(&cmd.template, &args);
            let text = expand_shell(&text, &cwd).await;
            Ok(json!({ "text": text }))
        }
        Request::ListFiles { cwd, query, limit } => {
            let cwd = hub.effective_cwd(&cwd);
            let files = list_files(hub, &cwd).await?;
            let limit = limit
                .unwrap_or(FILES_DEFAULT_LIMIT)
                .clamp(1, FILES_MAX_LIMIT);
            let res = tokio::task::spawn_blocking(move || fuzzy_filter(&files, &query, limit))
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!({ "files": res }))
        }
        Request::Export { session_id, path } => export(hub, session_id, path).await,
    }
}

/// Persisted and live sessions, newest first.
async fn list_history(hub: &Arc<Hub>, cwd: Option<String>, limit: Option<usize>) -> Reply {
    let store = hub.store.clone();
    let cwd = cwd.filter(|c| !c.is_empty());
    let (c2, limit) = (cwd.clone(), limit.unwrap_or(HISTORY_DEFAULT_LIMIT));
    let mut list = tokio::task::spawn_blocking(move || store.list_for(c2, Some(limit)))
        .await
        .map_err(|e| e.to_string())?;
    let live: Vec<SessionInfo> = hub.infos();
    for i in &mut list {
        if let Some(l) = live.iter().find(|l| l.id == i.id) {
            *i = l.clone();
        }
    }
    for l in live {
        if !list.iter().any(|i| i.id == l.id) {
            list.push(l);
        }
    }
    if let Some(c) = cwd {
        list.retain(|i| i.cwd == c);
    }
    list.sort_by_key(|i| std::cmp::Reverse(i.updated_at));
    list.truncate(limit);
    Ok(json!({ "sessions": list }))
}

/// Attaches the connection to a session (loading it if needed) and returns its snapshot.
async fn attach(
    hub: &Arc<Hub>,
    conn: &Arc<Conn>,
    session_id: String,
    foreground: bool,
    limit: Option<u32>,
) -> Reply {
    let s = crate::ops::get_or_load(hub, &session_id).await?;
    let snapshot = {
        let mut st = s.lock();
        {
            let mut c = conn.lock();
            if foreground {
                c.watch.remove(&session_id);
                if c.fg.insert(session_id.clone()) {
                    st.info.attached += 1;
                }
            } else if !c.fg.contains(&session_id) {
                c.watch.insert(session_id.clone());
            }
        }
        hub.emit_info(&st);
        let (messages, more_before) = crate::session::page(&st.messages, st.messages.len(), limit);
        let mut queued: Vec<Message> = st.queue.iter().cloned().collect();
        queued.iter_mut().for_each(Message::strip_image_data);
        SessionSnapshot {
            info: st.info.clone(),
            messages,
            more_before,
            pending_permissions: st.perms.iter().map(|p| p.request.clone()).collect(),
            pending_questions: st.questions.iter().map(|p| p.request.clone()).collect(),
            queued,
        }
    };
    Ok(json!({ "snapshot": snapshot }))
}

/// The full data of an image that history pages carry as a stub.
async fn get_image(hub: &Arc<Hub>, session_id: String, image_id: String) -> Reply {
    let mid = ref_proto::image_message_id(&image_id)
        .ok_or_else(|| format!("Invalid image id: {image_id}"))?
        .to_string();
    fn find<'a>(
        mut msgs: impl Iterator<Item = &'a Message>,
        mid: &str,
        image_id: &str,
    ) -> Option<ref_proto::ImageData> {
        msgs.find(|m| m.id == mid)
            .and_then(|m| m.image(image_id))
            .filter(|i| !i.is_stub())
            .cloned()
    }
    let image = match hub.session(&session_id) {
        Some(s) => {
            let st = s.lock();
            find(
                st.messages.iter().rev().chain(st.queue.iter()),
                &mid,
                &image_id,
            )
        }
        None => {
            let store = hub.store.clone();
            let id = session_id.clone();
            let p = tokio::task::spawn_blocking(move || store.load(&id))
                .await
                .map_err(|e| e.to_string())??;
            find(p.messages.iter().rev(), &mid, &image_id)
        }
    };
    let image =
        image.ok_or_else(|| "Image not found (the message may have been removed).".to_string())?;
    Ok(json!({ "image": image }))
}

/// Switches the agent of a live session.
fn set_agent(hub: &Arc<Hub>, session_id: String, agent: String) -> Reply {
    let s = live(hub, &session_id)?;
    let mut st = s.lock();
    if agent == ref_config::COMPACTION_AGENT {
        return Err(format!(
            "Agent {agent} is internal and cannot run a session."
        ));
    }
    let Some(a) = st.cfg.agents.get(&agent) else {
        return Err(format!("Unknown agent: {agent}"));
    };
    if a.model.is_some()
        && let Some(m) = st.cfg.default_model_for(&agent)
    {
        st.info.context = crate::models::context_keeping_used(&st.cfg, &m, st.info.context.used);
        st.info.model = m;
    }
    let old = st.info.agent.clone();
    let changed = old != agent;
    if changed {
        st.info.agent = agent;
        st.system = None;
    }
    if changed {
        crate::agent_switch::switched(hub, &mut st, &old);
    }
    hub.emit_info(&st);
    drop(st);
    if changed {
        crate::ops::persist(hub, &s);
    }
    Ok(json!({ "session": s.info() }))
}

/// Sets or cycles the model variant of a live session.
fn set_variant(hub: &Arc<Hub>, session_id: String, variant: Option<String>) -> Reply {
    let s = live(hub, &session_id)?;
    let mut st = s.lock();
    let list = crate::models::variants(&st.cfg, &st.info.model);
    let next = match variant {
        Some(v) if v.is_empty() => None,
        Some(v) if list.contains(&v) => Some(v),
        Some(v) => {
            return Err(format!("Model {} has no variant {v}.", st.info.model.key()));
        }
        None => crate::models::next_variant(&list, st.info.model.variant.as_deref()),
    };
    st.info.model.variant = next;
    hub.emit_info(&st);
    Ok(json!({ "session": st.info.clone() }))
}

/// Drops the conversation from a message on and returns the prompt to edit again.
async fn undo(hub: &Arc<Hub>, session_id: String, message_id: Option<String>) -> Reply {
    let s = live(hub, &session_id)?;
    if s.lock().run.is_some() {
        crate::engine::abort(hub, &s, true);
        if !crate::engine::wait_idle(&s, crate::ops::SESSION_STOP_WAIT).await {
            return Err("The session did not stop in time.".into());
        }
    }
    let (text, images) = {
        let mut st = s.lock();
        let i = match &message_id {
            Some(id) => st
                .find_message(id)
                .ok_or_else(|| "That message is no longer in the conversation.".to_string())?,
            None => st
                .messages
                .iter()
                .rposition(|m| m.role == Role::User && m.source == MessageSource::Human)
                .ok_or_else(|| "Nothing to undo.".to_string())?,
        };
        // Only human prompts go back into the editor.
        let m = &st.messages[i];
        let (text, images) = if m.source == MessageSource::Human {
            (m.raw_text(), m.prompt_images())
        } else {
            (String::new(), Vec::new())
        };
        st.messages.truncate(i);
        crate::agent_switch::truncated(&mut st);
        if !st.queue.is_empty() {
            st.queue.clear();
            crate::engine::emit_queue(hub, &s, &st);
        }
        st.info.updated_at = ref_proto::now_ms();
        hub.emit(&s.id, st.reset_event());
        hub.emit_info(&st);
        (text, images)
    };
    crate::ops::persist(hub, &s);
    Ok(json!({ "text": text, "images": images }))
}

/// Writes the session as Markdown to `path` (relative to its working directory).
async fn export(hub: &Arc<Hub>, session_id: String, path: String) -> Reply {
    let s = crate::ops::get_or_load(hub, &session_id).await?;
    let (info, messages) = {
        let st = s.lock();
        (st.info.clone(), st.messages.clone())
    };
    let p = std::path::PathBuf::from(&path);
    let p = Path::new(&info.cwd).join(p);
    let md = export_markdown(&info, &messages);
    std::fs::write(&p, md).map_err(|e| format!("Cannot write {}: {e}", p.display()))?;
    Ok(json!({ "path": p.to_string_lossy() }))
}

/// Reload the session config before a human prompt; invalidate the cached system prompt
/// only when the config actually changed (keeps prompt cache hits).
const BUSY_CWD: &str = "The session is busy; wait for it or interrupt it (esc esc) before changing the working directory.";

/// `/cd`: switch the session to another directory and tell the model with a reminder.
async fn set_cwd(hub: &Arc<Hub>, s: &Arc<Session>, path: &str) -> Result<(), String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("Usage: /cd <directory>".into());
    }
    let (old, busy) = {
        let st = s.lock();
        (st.info.cwd.clone(), st.run.is_some())
    };
    if busy {
        return Err(BUSY_CWD.into());
    }
    let home = hub.opts.home.clone().or_else(dirs::home_dir);
    let p = crate::util::expand_home(path, home.as_deref());
    let p = Path::new(&old).join(p);
    let new = crate::ops::validate_cwd(&hub.effective_cwd(&p.to_string_lossy()))?;
    if new == old {
        return Ok(());
    }
    // Instructions (AGENTS.md), skills and project config depend on the directory.
    let cfg = crate::ops::load_config(hub, &new).await?;
    {
        let mut st = s.lock();
        if st.run.is_some() {
            return Err(BUSY_CWD.into());
        }
        st.info.cwd = new.clone();
        st.cfg = Arc::new(cfg);
        st.system = None;
        hub.mcp.start_servers(&st.cfg);
        hub.emit_info(&st);
    }
    let text = format!(
        "{REMINDER_PREFIX} The user changed the working directory from {old} to {new}. Relative paths and commands now resolve against {new}."
    );
    crate::engine::append_only(hub, s, user_message(MessageSource::Reminder, text, vec![]));
    Ok(())
}

/// `ListDirs`: the normalized absolute path and the names of its sub-directories (links to
/// directories included), case-insensitively sorted. In mock mode `~` is the sandbox and
/// nothing outside it is listed.
async fn list_dirs(hub: &Arc<Hub>, cwd: &str, path: &str) -> Reply {
    let path = path.trim();
    if path.is_empty() {
        return Err("Path is empty.".into());
    }
    let sandbox = hub.opts.sandbox_cwd.clone();
    let home = sandbox.clone().or_else(|| hub.opts.home.clone());
    let p = crate::util::expand_home(path, home.as_deref());
    let p = Path::new(cwd).join(p);
    tokio::task::spawn_blocking(move || {
        let dir = crate::ops::validate_cwd(&p.to_string_lossy())?;
        if sandbox.is_some_and(|root| !Path::new(&dir).starts_with(root)) {
            return Err(format!("{dir} is outside the mock workspace."));
        }
        let entries =
            std::fs::read_dir(&dir).map_err(|e| format!("Cannot read directory {dir}: {e}"))?;
        let mut dirs = sub_dir_names(&dir, entries);
        dirs.sort_by_key(|d| (d.to_lowercase(), d.clone()));
        Ok(json!({ "path": dir, "dirs": dirs }))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Names of the directories among `entries` (links to directories included). Unreadable
/// entries and names that are not valid UTF-8 (JSON cannot carry them as they are) are skipped.
fn sub_dir_names(dir: &str, entries: std::fs::ReadDir) -> Vec<String> {
    let mut names = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                tracing::debug!("ListDirs: skipping an unreadable entry of {dir}: {e}");
                continue;
            }
        };
        if !entry.path().is_dir() {
            continue;
        }
        match entry.file_name().into_string() {
            Ok(name) => names.push(name),
            Err(name) => tracing::debug!("ListDirs: skipping {name:?} in {dir}: not UTF-8"),
        }
    }
    names
}

/// Downscales / recompresses prompt images (`images.auto_resize`); undecodable ones are kept.
async fn fit_images(
    images: Vec<ref_proto::ImageData>,
) -> Result<Vec<ref_proto::ImageData>, String> {
    if images.is_empty() {
        return Ok(images);
    }
    tokio::task::spawn_blocking(move || {
        images
            .into_iter()
            .map(|i| ref_tools::imgfit::fit_image_data(&i).unwrap_or(i))
            .collect()
    })
    .await
    .map_err(|_| "Processing the attached images failed.".to_string())
}

async fn reload_config(hub: &Arc<Hub>, s: &Arc<Session>) {
    let cwd = s.lock().info.cwd.clone();
    let Ok(cfg) = crate::ops::load_config(hub, &cwd).await else {
        return;
    };
    let mut st = s.lock();
    if format!("{:?}", *st.cfg) != format!("{cfg:?}") {
        st.cfg = Arc::new(cfg);
        st.system = None;
        hub.mcp.start_servers(&st.cfg);
    }
}

async fn config_info(hub: &Arc<Hub>, cwd: &str) -> Result<ServerConfigInfo, String> {
    let cfg = crate::ops::load_config(hub, cwd).await?;
    let agents = cfg
        .agents
        .values()
        .filter(|a| !a.hidden)
        .map(|a| AgentInfo {
            name: a.name.clone(),
            description: a.description.clone(),
            model: a.model.clone(),
            creatable: a.creatable,
            can_spawn: a.can_spawn,
            color: a.color.clone(),
        })
        .collect();
    let mut models = Vec::new();
    for (pid, p) in &cfg.providers {
        for (mid, m) in &p.models {
            models.push(ModelInfo {
                provider: pid.clone(),
                provider_name: p.name.clone(),
                id: mid.clone(),
                name: m.name.clone(),
                context: m.context,
                output: m.output,
                images: m.images,
                reasoning: m.reasoning,
                variants: m.variants.keys().cloned().collect(),
            });
        }
    }
    let commands = cfg
        .commands
        .values()
        .map(|c| CommandInfo {
            name: c.name.clone(),
            description: c.description.clone(),
            source: "config".into(),
        })
        .collect();
    let mcp = hub.mcp.status_lines(&cfg);
    Ok(ServerConfigInfo {
        agents,
        default_agent: cfg.default_agent.clone(),
        models,
        commands,
        mcp,
        tui: cfg.tui.clone(),
    })
}

async fn run_shell(hub: Arc<Hub>, s: Arc<Session>, command: String) {
    let cwd = s.lock().info.cwd.clone();
    let ctx = ref_tools::ToolCtx {
        cwd: cwd.into(),
        store: s.hashes.clone(),
        cancel: hub.shutdown.child_token(),
        on_output: None,
        resize_images: false,
    };
    let out = ref_tools::execute(
        crate::toolset::BASH,
        json!({"command": command, "timeout": SHELL_TIMEOUT_SECS}),
        &ctx,
    )
    .await;
    let text = format!(
        "I ran this command:\n```\n$ {command}\n{}\n```",
        out.text.trim_end()
    );
    let msg = user_message(MessageSource::Shell, text, vec![]);
    crate::engine::append_only(&hub, &s, msg);
}

/// Highest numbered placeholder (`$9`) of a command template.
const MAX_POSITIONAL: usize = 9;

/// Placeholder for all arguments in a command template.
const ARGUMENTS_PLACEHOLDER: &str = "$ARGUMENTS";

/// `$ARGUMENTS`, `$1`..`$9` substitution (the last numbered placeholder takes the rest).
fn expand_template(template: &str, args: &str) -> String {
    let words: Vec<&str> = args.split_whitespace().collect();
    let max_n = (1..=MAX_POSITIONAL)
        .rev()
        .find(|n| template.contains(&format!("${n}")))
        .unwrap_or(0);
    let mut out = template.replace(ARGUMENTS_PLACEHOLDER, args.trim());
    for n in (1..=MAX_POSITIONAL).rev() {
        let key = format!("${n}");
        if !out.contains(&key) {
            continue;
        }
        let val = if n == max_n && words.len() > n {
            words[n - 1..].join(" ")
        } else {
            words.get(n - 1).copied().unwrap_or("").to_string()
        };
        out = out.replace(&key, &val);
    }
    if !template.contains(ARGUMENTS_PLACEHOLDER) && max_n == 0 && !args.trim().is_empty() {
        out = format!("{}\n\n{}", out.trim_end(), args.trim());
    }
    out
}

/// Replace ``!`cmd` `` with the command's output.
async fn expand_shell(text: &str, cwd: &str) -> String {
    let re = regex::Regex::new(r"!`([^`]+)`").expect("regex");
    let mut out = String::new();
    let mut last = 0;
    for c in re.captures_iter(text) {
        // Group 0 is the whole match; group 1 is not optional in the pattern.
        let m = c.get(0).expect("whole match");
        let cmd = &c[1];
        out.push_str(&text[last..m.start()]);
        let res = tokio::time::timeout(
            EXPAND_SHELL_TIMEOUT,
            tokio::process::Command::new(ref_tools::default_shell())
                .arg("-c")
                .arg(cmd)
                .current_dir(cwd)
                .output(),
        )
        .await;
        match res {
            Ok(Ok(o)) => {
                let mut s = String::from_utf8_lossy(&o.stdout).to_string();
                s.push_str(&String::from_utf8_lossy(&o.stderr));
                out.push_str(s.trim_end());
            }
            Ok(Err(e)) => out.push_str(&format!("(failed to run `{}`: {e})", cmd)),
            Err(_) => out.push_str(&format!("(`{}` timed out)", cmd)),
        }
        last = m.end();
    }
    out.push_str(&text[last..]);
    out
}

async fn list_files(hub: &Arc<Hub>, cwd: &str) -> Result<Arc<Vec<String>>, String> {
    if let Some((t, v)) = lock(&hub.files_cache).get(cwd)
        && t.elapsed() < FILES_CACHE_TTL
    {
        return Ok(v.clone());
    }
    let root = std::path::PathBuf::from(cwd);
    if !root.is_dir() {
        return Err(format!("Not a directory: {cwd}"));
    }
    let files = tokio::task::spawn_blocking(move || walk_files(&root))
        .await
        .map_err(|e| e.to_string())?;
    let files = Arc::new(files);
    let mut cache = lock(&hub.files_cache);
    if cache.len() > FILES_CACHE_MAX_DIRS {
        cache.clear();
    }
    cache.insert(cwd.to_string(), (Instant::now(), files.clone()));
    Ok(files)
}

/// Most entries `list_files` returns for one directory.
const MAX_LISTED_FILES: usize = 50_000;
/// How deep `list_files` looks.
const MAX_LIST_DEPTH: usize = 16;

/// Files and directories below `root` (directories end with `/`), relative to it, separated by
/// `/` on every platform. Hidden and ignored entries are left out.
fn walk_files(root: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    for e in ignore::WalkBuilder::new(root)
        .hidden(true)
        .max_depth(Some(MAX_LIST_DEPTH))
        .build()
        .flatten()
    {
        let Ok(rel) = e.path().strip_prefix(root) else {
            continue;
        };
        let rel = crate::util::relative_slash(rel);
        if rel.is_empty() {
            continue;
        }
        if e.file_type().is_some_and(|t| t.is_dir()) {
            out.push(format!("{rel}/"));
        } else {
            out.push(rel);
        }
        if out.len() >= MAX_LISTED_FILES {
            break;
        }
    }
    out
}

/// Points per matched query character.
const SCORE_MATCH: i64 = 1;

/// Bonus when the match directly follows the previous one.
const SCORE_ADJACENT: i64 = 5;

/// Bonus for a match inside the file name (after the last `/`).
const SCORE_IN_BASENAME: i64 = 2;

/// Bonus for a match at the start of a path or after `/`, `_`, `-` or `.`.
const SCORE_BOUNDARY: i64 = 3;

/// Bonus when the file name starts with the query, or only contains it.
const SCORE_BASENAME_PREFIX: i64 = 20;
const SCORE_BASENAME_CONTAINS: i64 = 10;

/// One point is taken off per this many characters of the path.
const LENGTH_PENALTY_DIVISOR: usize = 10;

/// Fuzzy subsequence score (higher is better); None if `q` is not a subsequence of `path`.
fn fuzzy_score(path: &str, q: &str) -> Option<i64> {
    if q.is_empty() {
        return Some(-(path.len() as i64));
    }
    let p: Vec<char> = path.to_lowercase().chars().collect();
    let qc: Vec<char> = q.to_lowercase().chars().collect();
    let base_start = path
        .trim_end_matches('/')
        .rfind('/')
        .map(|i| path[..=i].chars().count())
        .unwrap_or(0);
    let mut score: i64 = 0;
    let mut pi = 0;
    let mut prev: Option<usize> = None;
    for &c in &qc {
        let mut found = None;
        while pi < p.len() {
            if p[pi] == c {
                found = Some(pi);
                pi += 1;
                break;
            }
            pi += 1;
        }
        let i = found?;
        score += SCORE_MATCH;
        if prev == Some(i.wrapping_sub(1)) {
            score += SCORE_ADJACENT;
        }
        if i >= base_start {
            score += SCORE_IN_BASENAME;
        }
        if i == 0 || matches!(p.get(i.wrapping_sub(1)), Some('/' | '_' | '-' | '.')) {
            score += SCORE_BOUNDARY;
        }
        prev = Some(i);
    }
    let base: String = p[base_start.min(p.len())..].iter().collect();
    let ql: String = qc.iter().collect();
    if base.starts_with(&ql) {
        score += SCORE_BASENAME_PREFIX;
    } else if base.contains(&ql) {
        score += SCORE_BASENAME_CONTAINS;
    }
    score -= (p.len() / LENGTH_PENALTY_DIVISOR) as i64;
    Some(score)
}

fn fuzzy_filter(files: &[String], q: &str, limit: usize) -> Vec<String> {
    let q = q.trim();
    let mut scored: Vec<(i64, &String)> = files
        .iter()
        .filter_map(|f| fuzzy_score(f, q).map(|s| (s, f)))
        .collect();
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then(a.1.len().cmp(&b.1.len()))
            .then(a.1.cmp(b.1))
    });
    scored
        .into_iter()
        .take(limit)
        .map(|(_, f)| f.clone())
        .collect()
}

fn export_markdown(info: &SessionInfo, messages: &[Message]) -> String {
    let mut out = format!(
        "# {}\n\nSession `{}` (\"{}\") — agent {} — model {} — {}\n\n",
        if info.title.is_empty() {
            "Untitled session"
        } else {
            &info.title
        },
        info.id,
        info.name,
        info.agent,
        info.model.key(),
        info.cwd
    );
    for m in messages {
        let who = match (&m.role, &m.source) {
            (Role::Assistant, _) => "Assistant".to_string(),
            (Role::User, MessageSource::Human) => "User".into(),
            (Role::User, MessageSource::Peer { from_name, .. }) => {
                format!("Message from \"{from_name}\"")
            }
            (Role::User, MessageSource::Spawn { from_name, .. }) => {
                format!("Task from \"{from_name}\"")
            }
            (Role::User, MessageSource::Reminder) => "Reminder".into(),
            (Role::User, MessageSource::Compaction) => "Compaction summary".into(),
            (Role::User, MessageSource::Shell) => "User shell".into(),
        };
        out.push_str(&format!("## {who} ({})\n\n", crate::util::fmt_time(m.time)));
        for p in &m.parts {
            match p {
                Part::Text { text, .. } => {
                    out.push_str(text.trim_end());
                    out.push_str("\n\n");
                }
                Part::Reasoning { text, .. } if !text.trim().is_empty() => {
                    out.push_str("<details><summary>Reasoning</summary>\n\n");
                    out.push_str(text.trim_end());
                    out.push_str("\n\n</details>\n\n");
                }
                Part::Image { name, .. } => {
                    out.push_str(&format!(
                        "[image{}]\n\n",
                        name.as_ref().map(|n| format!(": {n}")).unwrap_or_default()
                    ));
                }
                Part::Tool {
                    name,
                    purpose,
                    input,
                    state,
                    output,
                    ..
                } => {
                    let st = serde_json::to_string(state).unwrap_or_default();
                    out.push_str(&format!(
                        "**Tool `{name}`** — {purpose} [{}]\n\n",
                        st.trim_matches('"')
                    ));
                    let inp = serde_json::to_string_pretty(input).unwrap_or_default();
                    out.push_str(&format!("```json\n{inp}\n```\n\n"));
                    if let Some(o) = output {
                        out.push_str(&format!(
                            "```\n{}\n```\n\n",
                            o.trim_end().replace("```", "`\u{200b}``")
                        ));
                    }
                }
                _ => {}
            }
        }
        if let Some(e) = &m.error {
            out.push_str(&format!("> Error: {e}\n\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listed_files_use_slashes_for_nested_files_and_directories() {
        let root = std::env::temp_dir().join(format!("ref-list-{}", crate::util::random_hex(8)));
        let nested = root.join("src").join("sub");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(root.join("src").join("parser.rs"), "").unwrap();
        std::fs::write(nested.join("deep.rs"), "").unwrap();
        let mut listed = walk_files(&root);
        std::fs::remove_dir_all(&root).unwrap();
        listed.sort();
        assert_eq!(
            listed,
            ["src/", "src/parser.rs", "src/sub/", "src/sub/deep.rs"]
        );
    }

    #[test]
    fn templates() {
        assert_eq!(
            expand_template("fix $ARGUMENTS now", "the bug"),
            "fix the bug now"
        );
        assert_eq!(expand_template("a=$1 b=$2", "x y z"), "a=x b=y z");
        assert_eq!(expand_template("plain", "extra"), "plain\n\nextra");
    }

    #[test]
    fn fuzzy() {
        let files: Vec<String> = [
            "src/main.rs",
            "src/engine.rs",
            "docs/main_notes.md",
            "Cargo.toml",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let r = fuzzy_filter(&files, "main", 10);
        assert_eq!(r[0], "src/main.rs");
        assert!(!r.contains(&"Cargo.toml".to_string()));
        assert_eq!(fuzzy_filter(&files, "eng", 10)[0], "src/engine.rs");
        assert_eq!(fuzzy_filter(&files, "", 2).len(), 2);
    }
}
