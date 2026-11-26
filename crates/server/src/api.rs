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

type Reply = Result<Value, ApiError>;

/// Error returned by [`handle`] and its helpers. `code` is set only for errors clients branch on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    pub message: String,
    pub code: Option<&'static str>,
}

impl ApiError {
    pub fn coded(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: Some(code),
        }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<String> for ApiError {
    fn from(message: String) -> Self {
        Self {
            message,
            code: None,
        }
    }
}

impl From<&str> for ApiError {
    fn from(message: &str) -> Self {
        Self {
            message: message.to_string(),
            code: None,
        }
    }
}

/// Sessions `ListHistory` returns when the client sets no limit.
const HISTORY_DEFAULT_LIMIT: usize = 100;
/// `ListHistory.creator` value meaning "sessions without a creator (user-created)".
const CREATOR_USER: &str = "user";
/// Files `ListFiles` returns by default, and at most.
const FILES_DEFAULT_LIMIT: usize = 50;
const FILES_MAX_LIMIT: usize = 500;

/// Binaries `ListBinaries` returns by default, and at most.
const BINARIES_DEFAULT_LIMIT: usize = 50;
const BINARIES_MAX_LIMIT: usize = 200;
const BINARIES_CACHE_TTL: Duration = Duration::from_secs(30);

const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

const SHELL_BUILTINS: &[&str] = &[
    "alias", "bg", "cd", "echo", "exit", "export", "false", "fg", "jobs", "kill", "pwd", "source",
    "test", "true", "type", "unset", "wait", "which",
];

const MOCK_DEV_BINARIES: &[&str] = &[
    "awk", "cargo", "cat", "curl", "find", "git", "grep", "ls", "node", "pnpm", "printf",
    "python3", "rg", "seq", "sleep",
];
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

fn resolve_req_cwd(hub: &Hub, conn: &Conn, cwd: Option<String>) -> String {
    cwd.filter(|c| !c.trim().is_empty())
        .map(|c| hub.effective_cwd(c.trim()))
        .unwrap_or_else(|| conn.lock().cwd.clone())
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
        Request::ListDirs {
            path,
            cwd,
            include_files,
        } => {
            let base_cwd = resolve_req_cwd(hub, conn, cwd);
            list_dirs(hub, &base_cwd, &path, include_files).await
        }
        Request::Shutdown => {
            tracing::info!("shutdown requested by a client");
            hub.shutdown.cancel();
            Ok(json!({}))
        }
        Request::GetConfig { cwd } => {
            let cwd = resolve_req_cwd(hub, conn, cwd);
            Ok(json!({ "config": config_info(hub, &cwd).await? }))
        }
        Request::ReloadConfig { cwd } => {
            let cwd = resolve_req_cwd(hub, conn, cwd);
            let warnings = crate::ops::reload_all(hub, Some(&cwd)).await?;
            let config = config_info(hub, &cwd).await?;
            Ok(json!({ "config": config, "warnings": warnings }))
        }
        Request::GetConfigFiles { file, cwd } => {
            let cwd = resolve_req_cwd(hub, conn, cwd);
            crate::config_files::get_config_files(hub, file, cwd).await
        }
        Request::SaveConfigFile { file, content, cwd } => {
            let cwd = resolve_req_cwd(hub, conn, cwd);
            crate::config_files::save_config_file(hub, file, content, cwd).await
        }
        Request::CreateConfigFile {
            label,
            clone_current,
            content,
            cwd,
        } => {
            let cwd = resolve_req_cwd(hub, conn, cwd);
            crate::config_files::create_config_file(hub, label, clone_current, content, cwd).await
        }
        Request::SelectConfigFile { file, cwd } => {
            let cwd = resolve_req_cwd(hub, conn, cwd);
            crate::config_files::select_config_file(hub, file, cwd).await
        }
        Request::DeleteConfigFile { file, cwd } => {
            let cwd = resolve_req_cwd(hub, conn, cwd);
            crate::config_files::delete_config_file(hub, file, cwd).await
        }
        Request::ListBinaries { query, limit } => list_binaries(hub, &query, limit).await,
        Request::CompleteShell {
            cwd,
            line,
            cursor,
            limit,
        } => complete_shell(hub, conn, &cwd, &line, cursor, limit).await,
        Request::ListSessions => Ok(json!({ "sessions": hub.infos() })),
        Request::McpStatus { cwd } => {
            let cwd = resolve_req_cwd(hub, conn, cwd);
            let cfg = crate::ops::load_config(hub, &cwd).await?;
            Ok(json!({ "servers": hub.mcp.status(&cfg) }))
        }
        Request::ListHistory {
            cwd,
            limit,
            creator,
            query,
        } => list_history(hub, cwd, creator, query, limit).await,
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
            start_seq,
            end_seq,
            limit,
        } => messages(hub, session_id, before, start_seq, end_seq, limit).await,
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
                None => Err(ApiError::coded(
                    ref_proto::ERR_NO_PENDING_PERMISSION,
                    ref_proto::NO_PENDING_PERMISSION,
                )),
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
            let info = {
                let mut st = s.lock();
                if st.cfg.model(&model).is_none() {
                    return Err(format!("Model {} is not configured.", model.key()).into());
                }
                st.info.context =
                    crate::models::context_keeping_used(&st.cfg, &model, st.info.context.used);
                st.info.model = model;
                st.model_explicit = true;
                hub.emit_info(&st);
                st.info.clone()
            };
            crate::ops::persist(hub, &s);
            Ok(json!({ "session": info }))
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
                return Err(format!("A live session named \"{name}\" already exists.").into());
            }
            {
                let mut st = s.lock();
                st.info.name = name;
                hub.emit_info(&st);
            }
            crate::ops::persist(hub, &s);
            Ok(json!({ "session": s.info() }))
        }
        Request::SetPinned { session_id, pinned } => {
            let pinned_at = pinned.then(ref_proto::now_ms);
            if let Some(s) = hub.session(&session_id) {
                let info = {
                    let mut st = s.lock();
                    st.info.pinned_at = pinned_at;
                    hub.emit_info(&st);
                    st.info.clone()
                };
                crate::ops::persist(hub, &s);
                Ok(json!({ "session": info }))
            } else {
                let store = hub.store.clone();
                let sid = session_id.clone();
                let info = tokio::task::spawn_blocking(move || store.set_pinned(&sid, pinned_at))
                    .await
                    .map_err(|e| e.to_string())??;
                let Some(info) = info else {
                    return Err(format!("Session {session_id} not found").into());
                };
                hub.broadcast_history_changed();
                Ok(json!({ "session": info }))
            }
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
            let _guard = hub.stash_lock.lock().await;
            let store = hub.store.clone();
            let groups_changed =
                tokio::task::spawn_blocking(move || store.delete_session(&session_id))
                    .await
                    .map_err(|e| e.to_string())??;
            if groups_changed {
                let groups = crate::stash::list_groups_hydrated(hub).await?;
                hub.broadcast_groups_changed(groups);
            }
            Ok(json!({}))
        }
        Request::ExpandCommand { name, args, cwd } => {
            let cwd = hub.effective_cwd(&cwd);
            let cfg = crate::ops::load_config(hub, &cwd).await?;
            let Some(cmd) = cfg.commands.get(&name) else {
                return Err(format!("Unknown command: /{name}").into());
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
        Request::RetryNow { session_id } => {
            let s = live(hub, &session_id)?;
            Ok(json!({ "retried": crate::engine::retry_now(&s) }))
        }
        Request::ListGroups => crate::stash::list_groups(hub).await,
        Request::StashAll { name } => crate::stash::stash_all(hub, name).await,
        Request::CreateGroup {
            name,
            session_ids,
            stash_live,
        } => crate::stash::create_group(hub, name, session_ids, stash_live).await,
        Request::RenameGroup { group_id, name } => {
            crate::stash::rename_group(hub, group_id, name).await
        }
        Request::SetGroupLocked { group_id, locked } => {
            crate::stash::set_group_locked(hub, group_id, locked).await
        }
        Request::DeleteGroup { group_id } => crate::stash::delete_group(hub, group_id).await,
        Request::AddToGroup {
            group_id,
            session_id,
            position,
            stash_live,
        } => crate::stash::add_to_group(hub, group_id, session_id, position, stash_live).await,
        Request::MoveGroupMember {
            from_group_id,
            to_group_id,
            session_id,
            position,
            stash_live,
        } => {
            crate::stash::move_group_member(
                hub,
                from_group_id,
                to_group_id,
                session_id,
                position,
                stash_live,
            )
            .await
        }
        Request::RemoveFromGroup {
            group_id,
            session_id,
        } => crate::stash::remove_from_group(hub, group_id, session_id).await,
        Request::MergeGroups {
            source_group_id,
            target_group_id,
        } => crate::stash::merge_groups(hub, source_group_id, target_group_id).await,
        Request::RestoreGroup { group_id } => crate::stash::restore_group(hub, group_id).await,
        Request::RestoreGroupMember {
            group_id,
            session_id,
        } => crate::stash::restore_group_member(hub, group_id, session_id).await,
    }
}

/// Persisted and live sessions matching optional `cwd`, `creator`, and `query`, newest first.
async fn list_history(
    hub: &Arc<Hub>,
    cwd: Option<String>,
    creator: Option<String>,
    query: Option<String>,
    limit: Option<usize>,
) -> Reply {
    let cwd = crate::store::normalize_filter(cwd);
    let creator = crate::store::normalize_filter(creator);
    let query = crate::store::normalize_filter(query);
    let limit = limit.unwrap_or(HISTORY_DEFAULT_LIMIT);
    let live: Vec<SessionInfo> = hub.infos();
    let exclude_ids: Vec<String> = live.iter().map(|i| i.id.clone()).collect();

    let extra_creator_ids: Vec<String> = match creator.as_deref() {
        Some(cr) if cr != CREATOR_USER => {
            let cr_lower = cr.to_lowercase();
            live.iter()
                .filter(|s| s.name.to_lowercase() == cr_lower)
                .map(|s| s.id.clone())
                .collect()
        }
        _ => Vec::new(),
    };

    let matched_live = filter_live_sessions(
        hub,
        &live,
        cwd.as_deref(),
        creator.as_deref(),
        query.as_deref(),
    )
    .await?;

    let store = hub.store.clone();
    let mut list = tokio::task::spawn_blocking(move || {
        store.list_for_excluding(
            cwd,
            creator,
            extra_creator_ids,
            query,
            Some(limit),
            exclude_ids,
        )
    })
    .await
    .map_err(|e| e.to_string())?;

    list.extend(matched_live);
    list.sort_by_key(|s| {
        (
            std::cmp::Reverse(s.pinned_at.is_some()),
            std::cmp::Reverse(s.pinned_at.unwrap_or(0)),
            std::cmp::Reverse(s.updated_at),
        )
    });
    list.truncate(limit);
    Ok(json!({ "sessions": list }))
}

async fn filter_live_sessions(
    hub: &Arc<Hub>,
    live: &[SessionInfo],
    cwd: Option<&str>,
    creator: Option<&str>,
    query: Option<&str>,
) -> Result<Vec<SessionInfo>, ApiError> {
    if cwd.is_none() && creator.is_none() && query.is_none() {
        return Ok(live.to_vec());
    }
    let mut by_id: std::collections::HashMap<String, SessionInfo> =
        live.iter().map(|s| (s.id.clone(), s.clone())).collect();
    if creator.is_some() || query.is_some() {
        let missing_parents: Vec<String> = live
            .iter()
            .filter_map(|s| s.parent.as_deref())
            .filter(|pid| !pid.is_empty() && !by_id.contains_key(*pid))
            .map(str::to_string)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        if !missing_parents.is_empty() {
            let store = hub.store.clone();
            let parents = tokio::task::spawn_blocking(move || store.infos(missing_parents))
                .await
                .map_err(|e| e.to_string())?;
            for p in parents {
                by_id.entry(p.id.clone()).or_insert(p);
            }
        }
    }
    let creator_lower = creator.map(str::to_lowercase);
    let query_lower = query.map(str::to_lowercase);
    Ok(live
        .iter()
        .filter(|s| {
            if let Some(c) = cwd
                && s.cwd != c
            {
                return false;
            }
            let parent_id = s.parent.as_deref().filter(|p| !p.is_empty());
            let parent = parent_id.and_then(|pid| by_id.get(pid));
            if let Some(cr) = creator {
                if cr == CREATOR_USER {
                    if parent_id.is_some() {
                        return false;
                    }
                } else {
                    let cr_lower = creator_lower.as_deref().unwrap_or(cr);
                    let matched = parent_id.is_some_and(|pid| {
                        pid == cr || parent.is_some_and(|p| p.name.to_lowercase() == cr_lower)
                    });
                    if !matched {
                        return false;
                    }
                }
            }
            if let Some(q) = query_lower.as_deref() {
                let contains = |hay: &str| hay.to_lowercase().contains(q);
                let hit = contains(&s.title)
                    || contains(&s.name)
                    || contains(&s.cwd)
                    || contains(&s.id)
                    || parent_id.is_some_and(contains)
                    || parent.is_some_and(|p| contains(&p.name) || contains(&p.title));
                if !hit {
                    return false;
                }
            }
            true
        })
        .cloned()
        .collect())
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
        let total_messages = st.window_start_seq + st.messages.len() as u64;
        let (messages, _) = crate::session::page(&st.messages, st.messages.len(), limit);
        let start_seq = total_messages.saturating_sub(messages.len() as u64);
        let more_before = start_seq > 0;
        let layout_metas = st.layout_metas_before(start_seq);
        let mut queued: Vec<Message> = st.queue.iter().cloned().collect();
        queued.iter_mut().for_each(Message::strip_image_data);
        SessionSnapshot {
            info: st.info.clone(),
            messages,
            more_before,
            total_messages,
            start_seq,
            layout_metas,
            pending_permissions: st.perms.iter().map(|p| p.request.clone()).collect(),
            pending_questions: st.questions.iter().map(|p| p.request.clone()).collect(),
            queued,
        }
    };
    Ok(json!({ "snapshot": snapshot }))
}

async fn messages(
    hub: &Arc<Hub>,
    session_id: String,
    before: Option<String>,
    start_seq: Option<u64>,
    end_seq: Option<u64>,
    limit: Option<u32>,
) -> Reply {
    let s = live(hub, &session_id)?;
    let effective_limit = limit.map_or(crate::session::PAGE_DEFAULT, |l| {
        (l as usize).clamp(1, crate::session::PAGE_MAX)
    });

    if let Some(before_id) = before {
        enum BeforePlan {
            Memory(Vec<Message>, u64, u64),
            DiskBefore(u64),
            LookupDisk,
        }
        let plan = {
            let st = s.lock();
            match st.find_message(&before_id) {
                Some(idx) if idx > 0 => {
                    let before_seq = st.window_start_seq + idx as u64;
                    let (msgs, _) = crate::session::page(&st.messages, idx, limit);
                    let start = before_seq.saturating_sub(msgs.len() as u64);
                    BeforePlan::Memory(msgs, start, before_seq)
                }
                Some(_) => {
                    if st.window_start_seq == 0 {
                        BeforePlan::Memory(Vec::new(), 0, 0)
                    } else {
                        BeforePlan::DiskBefore(st.window_start_seq)
                    }
                }
                None => {
                    if st.window_start_seq == 0 {
                        return Err("That message is no longer in the conversation.".into());
                    }
                    BeforePlan::LookupDisk
                }
            }
        };
        let (msgs, start, end) = match plan {
            BeforePlan::Memory(msgs, start, end) => (msgs, start, end),
            BeforePlan::DiskBefore(before_seq) => {
                let store = hub.store.clone();
                let sid = session_id.clone();
                tokio::task::spawn_blocking(move || {
                    store.load_messages_before(
                        &sid,
                        before_seq,
                        effective_limit,
                        crate::session::PAGE_BYTES,
                    )
                })
                .await
                .map_err(|e| e.to_string())??
            }
            BeforePlan::LookupDisk => {
                let store = hub.store.clone();
                let sid = session_id.clone();
                let bid = before_id.clone();
                let before_seq = tokio::task::spawn_blocking(move || store.message_seq(&sid, &bid))
                    .await
                    .map_err(|e| e.to_string())??
                    .ok_or_else(|| "That message is no longer in the conversation.".to_string())?;
                let store = hub.store.clone();
                let sid = session_id.clone();
                tokio::task::spawn_blocking(move || {
                    store.load_messages_before(
                        &sid,
                        before_seq,
                        effective_limit,
                        crate::session::PAGE_BYTES,
                    )
                })
                .await
                .map_err(|e| e.to_string())??
            }
        };
        return Ok(json!({
            "messages": msgs,
            "more_before": start > 0,
            "start_seq": start,
            "end_seq": end,
        }));
    }

    if start_seq.is_none() && end_seq.is_none() {
        return Err("Missing `before` or `start_seq`/`end_seq`.".into());
    }
    enum RangePlan {
        Ready(Vec<Message>, u64, u64),
        DiskAndMem {
            req_start: u64,
            disk_end: u64,
            mem_tail: Vec<Message>,
        },
    }
    let plan = {
        let st = s.lock();
        let total = st.window_start_seq + st.messages.len() as u64;
        let req_start = start_seq.unwrap_or(0).min(total);
        let req_end = end_seq.unwrap_or(total).min(total);
        if req_start >= req_end {
            RangePlan::Ready(Vec::new(), req_start, req_start)
        } else {
            let capped_end = req_end.min(req_start.saturating_add(effective_limit as u64));
            if req_start >= st.window_start_seq {
                let start_idx = (req_start - st.window_start_seq) as usize;
                let end_idx = (capped_end - st.window_start_seq) as usize;
                let msgs = crate::session::page_forward(&st.messages, start_idx, end_idx, limit);
                let actual_end = req_start + msgs.len() as u64;
                RangePlan::Ready(msgs, req_start, actual_end)
            } else {
                let disk_end = capped_end.min(st.window_start_seq);
                let mem_take = capped_end.saturating_sub(st.window_start_seq) as usize;
                let mem_tail = st.messages.iter().take(mem_take).cloned().collect();
                RangePlan::DiskAndMem {
                    req_start,
                    disk_end,
                    mem_tail,
                }
            }
        }
    };
    let (msgs, actual_start, actual_end) = match plan {
        RangePlan::Ready(msgs, start, end) => (msgs, start, end),
        RangePlan::DiskAndMem {
            req_start,
            disk_end,
            mem_tail,
        } => {
            let store = hub.store.clone();
            let sid = session_id.clone();
            let mut combined = tokio::task::spawn_blocking(move || {
                store.load_messages_range(&sid, req_start, disk_end)
            })
            .await
            .map_err(|e| e.to_string())??;
            combined.extend(mem_tail);
            let msgs = crate::session::page_forward(&combined, 0, combined.len(), limit);
            let actual_end = req_start + msgs.len() as u64;
            (msgs, req_start, actual_end)
        }
    };
    Ok(json!({
        "messages": msgs,
        "more_before": actual_start > 0,
        "start_seq": actual_start,
        "end_seq": actual_end,
    }))
}

/// The full data of an image that history pages carry as a stub.
async fn get_image(hub: &Arc<Hub>, session_id: String, image_id: String) -> Reply {
    let mid = ref_proto::image_message_id(&image_id)
        .ok_or_else(|| format!("Invalid image id: {image_id}"))?
        .to_string();
    let mem_image = hub.session(&session_id).and_then(|s| {
        let st = s.lock();
        st.messages
            .iter()
            .rev()
            .chain(st.queue.iter())
            .find(|m| m.id == mid)
            .and_then(|m| m.image(&image_id))
            .filter(|i| !i.is_stub())
            .cloned()
    });
    let image = match mem_image {
        Some(img) => Some(img),
        None => {
            let store = hub.store.clone();
            let sid = session_id.clone();
            let mid2 = mid.clone();
            let msg = tokio::task::spawn_blocking(move || store.load_message(&sid, &mid2))
                .await
                .map_err(|e| e.to_string())??;
            msg.and_then(|m| m.image(&image_id).filter(|i| !i.is_stub()).cloned())
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
        return Err(format!("Agent {agent} is internal and cannot run a session.").into());
    }
    let Some(a) = st.cfg.agents.get(&agent) else {
        return Err(format!("Unknown agent: {agent}").into());
    };
    let mut model_changed = false;
    let mut explicit_changed = false;
    if a.model.is_some() || !st.model_explicit {
        let m = st.cfg.default_model_for(&agent).unwrap_or_default();
        st.info.context = crate::models::context_keeping_used(&st.cfg, &m, st.info.context.used);
        model_changed = st.info.model != m;
        explicit_changed = st.model_explicit;
        st.info.model = m;
        st.model_explicit = false;
    }
    let old = st.info.agent.clone();
    let changed = old != agent;
    if changed {
        st.info.agent = agent;
        st.system = None;
        crate::agent_switch::switched(hub, &mut st, &old);
    }
    hub.emit_info(&st);
    let info = st.info.clone();
    drop(st);
    if changed || model_changed || explicit_changed {
        crate::ops::persist(hub, &s);
    }
    Ok(json!({ "session": info }))
}

/// Sets or cycles the model variant of a live session.
fn set_variant(hub: &Arc<Hub>, session_id: String, variant: Option<String>) -> Reply {
    let s = live(hub, &session_id)?;
    let (info, changed) = {
        let mut st = s.lock();
        let list = crate::models::variants(&st.cfg, &st.info.model);
        let next = match variant {
            Some(v) if v.is_empty() => None,
            Some(v) if list.contains(&v) => Some(v),
            Some(v) => {
                return Err(format!("Model {} has no variant {v}.", st.info.model.key()).into());
            }
            None => crate::models::next_variant(&list, st.info.model.variant.as_deref()),
        };
        let changed = st.info.model.variant != next;
        st.info.model.variant = next;
        hub.emit_info(&st);
        (st.info.clone(), changed)
    };
    if changed {
        crate::ops::persist(hub, &s);
    }
    Ok(json!({ "session": info }))
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

    // If the target message sits before `window_start_seq`, reload from the previous compaction
    // boundary so truncating across compaction boundaries stays consistent.
    let need_reload_upto = {
        let st = s.lock();
        match &message_id {
            Some(id) => match st.find_message(id) {
                Some(0) if st.window_start_seq > 0 => Some((st.window_start_seq, true)),
                Some(_) => None,
                None if st.window_start_seq > 0 => Some((st.window_start_seq, false)),
                None => return Err("That message is no longer in the conversation.".into()),
            },
            None => {
                let in_mem = st
                    .messages
                    .iter()
                    .rposition(|m| m.role == Role::User && m.source == MessageSource::Human);
                match in_mem {
                    Some(_) => None,
                    None if st.window_start_seq > 0 => Some((st.window_start_seq, false)),
                    None => return Err("Nothing to undo.".into()),
                }
            }
        }
    };

    if let Some((window_start, only_previous_epoch)) = need_reload_upto {
        let store = hub.store.clone();
        let sid = session_id.clone();
        let mid_opt = message_id.clone();
        let (new_start, prefix) = tokio::task::spawn_blocking(move || -> Result<_, String> {
            let target_seq = if only_previous_epoch {
                window_start
            } else if let Some(id) = mid_opt {
                store
                    .message_seq(&sid, &id)?
                    .ok_or_else(|| "That message is no longer in the conversation.".to_string())?
            } else {
                store
                    .last_human_seq_before(&sid, window_start)?
                    .ok_or_else(|| "Nothing to undo.".to_string())?
            };
            let new_start = store.compaction_seq_before(&sid, target_seq)?;
            let prefix = store.load_messages_range(&sid, new_start, window_start)?;
            Ok((new_start, prefix))
        })
        .await
        .map_err(|e| e.to_string())??;

        let mut st = s.lock();
        if st.window_start_seq == window_start && !prefix.is_empty() {
            let mut combined = prefix;
            combined.append(&mut st.messages);
            st.messages = combined;
            st.window_start_seq = new_start;
            st.layout_metas_prefix.retain(|m| m.seq < new_start);
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

/// Writes the session as Markdown to `path` (relative to its working directory, expanding `~`).
async fn export(hub: &Arc<Hub>, session_id: String, path: String) -> Reply {
    let s = crate::ops::get_or_load(hub, &session_id).await?;
    let (info, window_start, mem_messages) = {
        let st = s.lock();
        (st.info.clone(), st.window_start_seq, st.messages.clone())
    };
    let messages = if window_start > 0 {
        let store = hub.store.clone();
        let sid = session_id.clone();
        let mut prefix =
            tokio::task::spawn_blocking(move || store.load_messages_range(&sid, 0, window_start))
                .await
                .map_err(|e| e.to_string())??;
        prefix.extend(mem_messages);
        prefix
    } else {
        mem_messages
    };
    let home = hub
        .opts
        .sandbox_cwd
        .clone()
        .or_else(|| hub.opts.home.clone());
    let p = crate::util::expand_home(path.trim(), home.as_deref());
    let p = Path::new(&info.cwd).join(p);
    let md = export_markdown(&info, &messages);
    std::fs::write(&p, md).map_err(|e| format!("Cannot write {}: {e}", p.display()))?;
    Ok(json!({ "path": p.to_string_lossy() }))
}

const BUSY_CWD: &str = "The session is busy; wait for it or interrupt it (esc esc) before changing the working directory.";

/// `/cd`: switch the session to another directory and tell the model with a reminder.
async fn set_cwd(hub: &Arc<Hub>, s: &Arc<Session>, path: &str) -> Result<(), ApiError> {
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
        let (info_changed, _) = crate::ops::apply_reloaded_config(hub, &mut st, cfg);
        hub.mcp.start_servers(&st.cfg);
        if !info_changed {
            hub.emit_info(&st);
        }
    }
    let text = format!(
        "{REMINDER_PREFIX} The user changed the working directory from {old} to {new}. Relative paths and commands now resolve against {new}."
    );
    crate::engine::append_only(hub, s, user_message(MessageSource::Reminder, text, vec![]));
    Ok(())
}

/// `ListDirs`: the normalized absolute path, its sub-directories (links to directories included),
/// and optionally regular files, case-insensitively sorted. In mock mode `~` is the sandbox and
/// nothing outside it is listed.
async fn list_dirs(hub: &Arc<Hub>, cwd: &str, path: &str, include_files: bool) -> Reply {
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
            return Err(format!("{dir} is outside the mock workspace.").into());
        }
        let entries = std::fs::read_dir(&dir)
            .map_err(|e| ApiError::from(format!("Cannot read directory {dir}: {e}")))?;
        let (mut dirs, mut files) = dir_entries(&dir, entries, include_files);
        dirs.sort_by_key(|d| (d.to_lowercase(), d.clone()));
        files.sort_by_key(|f| (f.to_lowercase(), f.clone()));
        Ok(json!({ "path": dir, "dirs": dirs, "files": files }))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Names of directories (and optionally files) among `entries`. Unreadable entries and names that
/// are not valid UTF-8 are skipped.
fn dir_entries(
    dir: &str,
    entries: std::fs::ReadDir,
    include_files: bool,
) -> (Vec<String>, Vec<String>) {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                tracing::debug!("ListDirs: skipping an unreadable entry of {dir}: {e}");
                continue;
            }
        };
        let path = entry.path();
        let is_dir = path.is_dir();
        let is_file = !is_dir && include_files && path.is_file();
        if !is_dir && !is_file {
            continue;
        }
        let name = match entry.file_name().into_string() {
            Ok(name) => name,
            Err(name) => {
                tracing::debug!("ListDirs: skipping {name:?} in {dir}: not UTF-8");
                continue;
            }
        };
        if is_dir {
            dirs.push(name);
        } else {
            files.push(name);
        }
    }
    (dirs, files)
}

fn mock_binaries() -> Vec<String> {
    let set: std::collections::BTreeSet<String> = SHELL_BUILTINS
        .iter()
        .chain(MOCK_DEV_BINARIES.iter())
        .map(|s| (*s).to_string())
        .collect();
    let mut list: Vec<String> = set.into_iter().collect();
    list.sort_by_key(|b| (b.to_lowercase(), b.clone()));
    list
}

async fn list_binaries(hub: &Arc<Hub>, query: &str, limit: Option<usize>) -> Reply {
    let limit = limit
        .unwrap_or(BINARIES_DEFAULT_LIMIT)
        .clamp(1, BINARIES_MAX_LIMIT);
    if hub.opts.sandbox_cwd.is_some() {
        let matched = filter_binaries(&mock_binaries(), query, limit);
        return Ok(json!({ "binaries": matched }));
    }
    let path_os = std::env::var_os("PATH").unwrap_or_default();
    let pathext_os = if cfg!(windows) {
        std::env::var_os("PATHEXT")
    } else {
        None
    };
    let cache_key = (path_os, pathext_os);
    let cached = {
        let guard = lock(&hub.binaries_cache);
        guard.as_ref().and_then(|(cached_key, at, list)| {
            (cached_key == &cache_key && at.elapsed() < BINARIES_CACHE_TTL).then(|| list.clone())
        })
    };
    let binaries = match cached {
        Some(list) => list,
        None => {
            let (path_for_scan, pathext_for_scan) = cache_key.clone();
            let scanned = tokio::task::spawn_blocking(move || {
                collect_binaries(&path_for_scan, pathext_for_scan.as_deref())
            })
            .await
            .map_err(|e| e.to_string())?;
            let list = Arc::new(scanned);
            *lock(&hub.binaries_cache) = Some((cache_key, Instant::now(), list.clone()));
            list
        }
    };
    let query = query.to_string();
    let matched = tokio::task::spawn_blocking(move || filter_binaries(&binaries, &query, limit))
        .await
        .map_err(|e| e.to_string())?;
    Ok(json!({ "binaries": matched }))
}

/// Max command line length accepted by `complete_shell` (bytes).
const COMPLETE_SHELL_MAX_LINE_BYTES: usize = 8192;

async fn complete_shell(
    hub: &Arc<Hub>,
    conn: &Arc<Conn>,
    cwd: &str,
    line: &str,
    cursor: Option<usize>,
    limit: Option<usize>,
) -> Reply {
    if line.len() > COMPLETE_SHELL_MAX_LINE_BYTES {
        return Ok(json!({ "items": [] }));
    }
    let conn_cwd = conn.lock().cwd.clone();
    let dir = if hub.opts.sandbox_cwd.is_some() {
        hub.effective_cwd(cwd)
    } else if cwd.trim().is_empty() {
        conn_cwd
    } else {
        let home = hub.opts.home.clone().or_else(dirs::home_dir);
        let p = crate::util::expand_home(cwd.trim(), home.as_deref());
        Path::new(&conn_cwd).join(p).to_string_lossy().into_owned()
    };
    let dir = tokio::task::spawn_blocking(move || crate::ops::validate_cwd(&dir))
        .await
        .map_err(|e| e.to_string())??;
    let items = crate::shell_complete::complete_shell(hub, &dir, line, cursor, limit).await;
    Ok(json!({ "items": items }))
}

fn parse_pathext(pathext: Option<&std::ffi::OsStr>) -> Vec<String> {
    let raw = pathext
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_else(|| DEFAULT_PATHEXT.to_string());
    let parsed: Vec<String> = raw
        .split(';')
        .map(str::trim)
        .map(|ext| ext.strip_prefix('.').unwrap_or(ext))
        .filter(|ext| !ext.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    if parsed.is_empty() {
        parse_pathext(None)
    } else {
        parsed
    }
}

fn windows_binary_stem(file_name: &str, pathext: &[String]) -> Option<String> {
    let path = Path::new(file_name);
    let ext = path.extension()?.to_str()?;
    if !pathext
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(ext))
    {
        return None;
    }
    let stem = path.file_stem()?.to_str()?;
    (!stem.is_empty()).then(|| stem.to_string())
}

fn is_unix_executable(path: &Path) -> bool {
    let Ok(meta) = path.metadata() else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn collect_dir_binaries(dir: &Path, pathext: &[String], windows_mode: bool) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_name) = entry.file_name().into_string() else {
            continue;
        };
        if file_name.is_empty() {
            continue;
        }
        if windows_mode {
            if path.metadata().is_ok_and(|m| m.is_file())
                && let Some(stem) = windows_binary_stem(&file_name, pathext)
            {
                names.push(stem);
            }
        } else if is_unix_executable(&path) {
            names.push(file_name);
        }
    }
    names.sort_by_key(|b| (b.to_lowercase(), b.clone()));
    names
}

fn collect_binaries_for_platform(
    path_env: &std::ffi::OsStr,
    pathext: Option<&std::ffi::OsStr>,
    windows_mode: bool,
) -> Vec<String> {
    let exts = if windows_mode {
        parse_pathext(pathext)
    } else {
        Vec::new()
    };
    let mut list: Vec<String> = if windows_mode {
        let mut seen: std::collections::BTreeMap<String, String> = SHELL_BUILTINS
            .iter()
            .map(|s| (s.to_lowercase(), (*s).to_string()))
            .collect();
        for dir in std::env::split_paths(path_env) {
            for stem in collect_dir_binaries(&dir, &exts, true) {
                seen.entry(stem.to_lowercase()).or_insert(stem);
            }
        }
        seen.into_values().collect()
    } else {
        let mut set: std::collections::BTreeSet<String> =
            SHELL_BUILTINS.iter().map(|s| (*s).to_string()).collect();
        for dir in std::env::split_paths(path_env) {
            set.extend(collect_dir_binaries(&dir, &exts, false));
        }
        set.into_iter().collect()
    };
    list.sort_by_key(|b| (b.to_lowercase(), b.clone()));
    list
}

fn collect_binaries(path_env: &std::ffi::OsStr, pathext: Option<&std::ffi::OsStr>) -> Vec<String> {
    collect_binaries_for_platform(path_env, pathext, cfg!(windows))
}

fn filter_binaries(all: &[String], query: &str, limit: usize) -> Vec<String> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return all.iter().take(limit).cloned().collect();
    }
    let mut prefix = Vec::new();
    let mut substring = Vec::new();
    for name in all {
        let lower = name.to_lowercase();
        if lower.starts_with(&q) {
            prefix.push(name.clone());
        } else if lower.contains(&q) {
            substring.push(name.clone());
        }
    }
    prefix.into_iter().chain(substring).take(limit).collect()
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

/// Reload the session config before a human prompt; invalidate the cached system prompt
/// only when the config actually changed (keeps prompt cache hits).
async fn reload_config(hub: &Arc<Hub>, s: &Arc<Session>) {
    let cwd = s.lock().info.cwd.clone();
    let cfg = match crate::ops::load_config(hub, &cwd).await {
        Ok(cfg) => cfg,
        Err(e) => {
            tracing::warn!(session = %s.id, "reloading the config failed: {e}");
            return;
        }
    };
    let mut st = s.lock();
    if *st.cfg != cfg {
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
        warnings: cfg.warnings,
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
                    let st = serde_json::to_string(state).expect("ToolState serializes");
                    out.push_str(&format!(
                        "**Tool `{name}`** — {purpose} [{}]\n\n",
                        st.trim_matches('"')
                    ));
                    out.push_str(&format!("```json\n{input:#}\n```\n\n"));
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

    #[test]
    fn binary_completions_include_builtins_and_rank_prefix_before_substring() {
        let dir = std::env::temp_dir().join(format!("ref-bin-{}", crate::util::random_hex(8)));
        std::fs::create_dir_all(&dir).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let exe_1 = dir.join("git-custom");
            let exe_2 = dir.join("my-git-helper");
            let non_exe = dir.join("git-not-executable");
            std::fs::write(&exe_1, "#!/bin/sh\n").unwrap();
            std::fs::write(&exe_2, "#!/bin/sh\n").unwrap();
            std::fs::write(&non_exe, "plain text").unwrap();
            std::fs::set_permissions(&exe_1, std::fs::Permissions::from_mode(0o755)).unwrap();
            std::fs::set_permissions(&exe_2, std::fs::Permissions::from_mode(0o755)).unwrap();
            std::fs::set_permissions(&non_exe, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        #[cfg(windows)]
        {
            std::fs::write(dir.join("git-custom.exe"), "").unwrap();
            std::fs::write(dir.join("my-git-helper.cmd"), "").unwrap();
            std::fs::write(dir.join("git-not-executable.txt"), "plain text").unwrap();
        }
        let all = collect_binaries(dir.as_os_str(), None);
        std::fs::remove_dir_all(&dir).unwrap();
        for builtin in SHELL_BUILTINS {
            assert!(
                all.iter().any(|b| b == builtin),
                "missing builtin {builtin}"
            );
        }
        assert!(all.contains(&"git-custom".to_string()), "{all:?}");
        assert!(all.contains(&"my-git-helper".to_string()), "{all:?}");
        assert!(!all.contains(&"git-custom.exe".to_string()), "{all:?}");
        assert!(!all.contains(&"git-not-executable".to_string()), "{all:?}");
        assert!(
            !all.contains(&"git-not-executable.txt".to_string()),
            "{all:?}"
        );
        let git_matches = filter_binaries(&all, "git", 50);
        assert_eq!(git_matches, vec!["git-custom", "my-git-helper"]);
        assert_eq!(filter_binaries(&all, "git", 1), vec!["git-custom"]);
    }

    #[test]
    fn windows_pathext_stripping_custom_extensions_and_case_dedup() {
        use std::ffi::OsStr;
        let root = std::env::temp_dir().join(format!("ref-winbin-{}", crate::util::random_hex(8)));
        let dir_a = root.join("a");
        let dir_b = root.join("b");
        std::fs::create_dir_all(&dir_a).unwrap();
        std::fs::create_dir_all(&dir_b).unwrap();

        std::fs::write(dir_a.join("cargo.exe"), "").unwrap();
        std::fs::write(dir_a.join("NPM.CMD"), "").unwrap();
        std::fs::write(dir_a.join("my.tool.bat"), "").unwrap();
        std::fs::write(dir_a.join("Git.EXE"), "").unwrap();
        std::fs::write(dir_a.join("ECHO.EXE"), "").unwrap();
        std::fs::write(dir_a.join("runner.py"), "").unwrap();
        std::fs::write(dir_a.join("README.md"), "").unwrap();
        std::fs::write(dir_a.join("bare-no-ext"), "").unwrap();
        std::fs::write(dir_b.join("git.cmd"), "").unwrap();
        std::fs::write(dir_b.join("npm.exe"), "").unwrap();

        let joined_path = std::env::join_paths([&dir_a, &dir_b]).unwrap();
        let default_bins = collect_binaries_for_platform(&joined_path, None, true);
        assert!(
            default_bins.contains(&"cargo".to_string()),
            "{default_bins:?}"
        );
        assert!(!default_bins.contains(&"cargo.exe".to_string()));
        assert!(
            default_bins.contains(&"NPM".to_string()),
            "{default_bins:?}"
        );
        assert!(
            !default_bins.contains(&"npm".to_string()),
            "{default_bins:?}"
        );
        assert!(default_bins.contains(&"my.tool".to_string()));
        assert!(default_bins.contains(&"Git".to_string()));
        assert!(!default_bins.contains(&"git".to_string()));
        assert!(default_bins.contains(&"echo".to_string()));
        assert!(!default_bins.contains(&"ECHO".to_string()));
        assert!(!default_bins.contains(&"runner".to_string()));
        assert!(!default_bins.contains(&"README".to_string()));
        assert!(!default_bins.contains(&"bare-no-ext".to_string()));

        let custom_bins =
            collect_binaries_for_platform(&joined_path, Some(OsStr::new(".EXE;.PY")), true);
        assert!(custom_bins.contains(&"runner".to_string()));
        assert!(custom_bins.contains(&"cargo".to_string()));
        assert!(!custom_bins.contains(&"my.tool".to_string()));
        // npm.exe from dir_b matches .EXE while NPM.CMD from dir_a is excluded!
        assert!(custom_bins.contains(&"npm".to_string()));
        assert!(!custom_bins.contains(&"NPM".to_string()));

        assert_eq!(
            parse_pathext(Some(OsStr::new(" ; . "))),
            vec!["com", "exe", "bat", "cmd"]
        );
        assert_eq!(windows_binary_stem(".exe", &["exe".into()]), None);
        std::fs::remove_dir_all(&root).unwrap();
    }
    #[tokio::test]
    async fn attach_and_undo_succeed_with_corrupt_history_row() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tmp")
            .join(format!("ref-api-corrupt-{}", crate::util::random_hex(8)));
        std::fs::create_dir_all(&dir).unwrap();
        let mut opts = crate::HubOptions::new(dir.clone());
        opts.home = None;
        let hub = Hub::new(opts);

        let mk_msg = |id: &str, src: MessageSource, text: &str| Message {
            id: id.into(),
            role: Role::User,
            time: 1,
            time_end: None,
            source: src,
            parts: vec![Part::Text {
                text: text.into(),
                time: 1,
            }],
            model: None,
            agent: None,
            usage: None,
            error: None,
        };
        let info = SessionInfo {
            id: "ses_corrupt_api".into(),
            name: "corrupt-api".into(),
            title: String::new(),
            agent: "build".into(),
            model: Default::default(),
            cwd: "/tmp".into(),
            created_at: 1,
            updated_at: 2,
            status: ref_proto::SessionStatus::Idle,
            attached: 0,
            live: false,
            parent: None,
            must_report: false,
            context: Default::default(),
            todos: Vec::new(),
            busy_since: None,
            last_error: None,
            pinned_at: None,
        };
        hub.store.save(crate::store::Persisted {
            info,
            messages: vec![
                mk_msg("m0", MessageSource::Human, "broken before compaction"),
                mk_msg("m1", MessageSource::Human, "kept human"),
                mk_msg("m2", MessageSource::Compaction, "summary"),
            ],
            reported: false,
            context_notice: false,
            model_explicit: false,
            window_start_seq: 0,
            force_persist: false,
        });
        hub.store.flush().unwrap();
        {
            let conn = rusqlite::Connection::open(&hub.store.path).unwrap();
            conn.execute(
                "UPDATE messages SET data = '{corrupt' WHERE session_id = 'ses_corrupt_api' AND seq = 0",
                [],
            )
            .unwrap();
        }

        let conn = hub.add_conn();
        conn.lock().hello = true;
        let snap = handle(
            &hub,
            &conn,
            Request::Attach {
                session_id: "ses_corrupt_api".into(),
                foreground: true,
                limit: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(snap["snapshot"]["start_seq"], 2);
        assert_eq!(
            snap["snapshot"]["layout_metas"].as_array().unwrap().len(),
            2
        );

        // Undoing back across the compaction boundary finds m1 (seq 1) without failing on seq 0.
        let undone = handle(
            &hub,
            &conn,
            Request::Undo {
                session_id: "ses_corrupt_api".into(),
                message_id: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(undone["text"], "kept human");

        let _ = hub.store.close();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
