//! Request dispatch for one frontend connection.

use crate::hub::{Conn, Hub};
use crate::session::{Session, user_message};
use ref_proto::{
    AgentInfo, CommandInfo, Message, MessageSource, ModelInfo, Part, Request, Role,
    ServerConfigInfo, SessionInfo, SessionSnapshot,
};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

type R = Result<Value, String>;

async fn load_cfg(hub: &Arc<Hub>, cwd: &str) -> Result<ref_config::Config, String> {
    let hub = hub.clone();
    let cwd = cwd.to_string();
    tokio::task::spawn_blocking(move || hub.load_config(Path::new(&cwd)))
        .await
        .map_err(|e| e.to_string())
}

fn live(hub: &Arc<Hub>, id: &str) -> Result<Arc<Session>, String> {
    hub.session(id)
        .ok_or_else(|| format!("Session {id} is not live (attach to it first)."))
}

pub async fn handle(hub: &Arc<Hub>, conn: &Arc<Conn>, req: Request) -> R {
    match req {
        Request::Hello { .. } => Err("Hello was already received.".into()),
        Request::Ping => Ok(json!({})),
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
                .unwrap_or_else(|| conn.lock().cwd.clone());
            let cfg = load_cfg(hub, &cwd).await?;
            Ok(json!({ "servers": hub.mcp.status(&cfg) }))
        }
        Request::ListHistory { cwd, limit } => {
            let store = hub.store.clone();
            let cwd = cwd.filter(|c| !c.is_empty());
            let (c2, limit) = (cwd.clone(), limit.unwrap_or(100));
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
        } => {
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
                let (messages, more_before) =
                    crate::session::page(&st.messages, st.messages.len(), limit);
                let mut queued: Vec<Message> = st.queue.iter().cloned().collect();
                queued.iter_mut().for_each(Message::strip_image_data);
                SessionSnapshot {
                    info: st.info.clone(),
                    messages,
                    more_before,
                    pending_permissions: st.perms.iter().map(|p| p.request.clone()).collect(),
                    queued,
                }
            };
            Ok(json!({ "snapshot": snapshot }))
        }
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
        } => {
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
            let image = image.ok_or_else(|| {
                "Image not found (the message may have been removed).".to_string()
            })?;
            Ok(json!({ "image": image }))
        }
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
                    let _ = tx.send((allow, always));
                    Ok(json!({}))
                }
                None => Err("No pending permission request with that id.".into()),
            }
        }
        Request::SetModel { session_id, model } => {
            let s = live(hub, &session_id)?;
            let mut st = s.lock();
            if st.cfg.model(&model).is_none() {
                return Err(format!("Model {} is not configured.", model.key()));
            }
            let used = st.info.context.used;
            st.info.context = crate::models::context_for(&st.cfg, &model);
            st.info.context.used = used;
            st.info.model = model;
            hub.emit_info(&st);
            Ok(json!({ "session": st.info.clone() }))
        }
        Request::SetAgent { session_id, agent } => {
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
                let used = st.info.context.used;
                st.info.context = crate::models::context_for(&st.cfg, &m);
                st.info.context.used = used;
                st.info.model = m;
            }
            if st.info.agent != agent {
                st.info.agent = agent;
                st.system = None;
            }
            hub.emit_info(&st);
            Ok(json!({ "session": st.info.clone() }))
        }
        Request::SetVariant {
            session_id,
            variant,
        } => {
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
        } => {
            let s = live(hub, &session_id)?;
            if s.lock().run.is_some() {
                crate::engine::abort(hub, &s, true);
                if !crate::engine::wait_idle(&s, Duration::from_secs(10)).await {
                    return Err("The session did not stop in time.".into());
                }
            }
            let (text, images) = {
                let mut st = s.lock();
                let i = match &message_id {
                    Some(id) => st.find_message(id).ok_or_else(|| {
                        "That message is no longer in the conversation.".to_string()
                    })?,
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
            let cfg = load_cfg(hub, &cwd).await?;
            let Some(cmd) = cfg.commands.get(&name) else {
                return Err(format!("Unknown command: /{name}"));
            };
            let text = expand_template(&cmd.template, &args);
            let text = expand_shell(&text, &cwd).await;
            Ok(json!({ "text": text }))
        }
        Request::ListFiles { cwd, query, limit } => {
            let files = list_files(hub, &cwd).await?;
            let limit = limit.unwrap_or(50).clamp(1, 500);
            let res = tokio::task::spawn_blocking(move || fuzzy_filter(&files, &query, limit))
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!({ "files": res }))
        }
        Request::Export { session_id, path } => {
            let s = crate::ops::get_or_load(hub, &session_id).await?;
            let (info, messages) = {
                let st = s.lock();
                (st.info.clone(), st.messages.clone())
            };
            let p = std::path::PathBuf::from(&path);
            let p = if p.is_absolute() {
                p
            } else {
                Path::new(&info.cwd).join(p)
            };
            let md = export_markdown(&info, &messages);
            std::fs::write(&p, md).map_err(|e| format!("Cannot write {}: {e}", p.display()))?;
            Ok(json!({ "path": p.to_string_lossy() }))
        }
    }
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
    let p = if p.is_absolute() {
        p
    } else {
        Path::new(&old).join(p)
    };
    let new = crate::ops::validate_cwd(&p.to_string_lossy())?;
    if new == old {
        return Ok(());
    }
    // Instructions (AGENTS.md), skills and project config depend on the directory.
    let cfg = load_cfg(hub, &new).await?;
    {
        let mut st = s.lock();
        if st.run.is_some() {
            return Err(BUSY_CWD.into());
        }
        st.info.cwd = new.clone();
        st.cfg = Arc::new(cfg);
        st.system = None;
        let _ = hub.mcp.servers_for(&st.cfg);
        hub.emit_info(&st);
    }
    let text = format!(
        "[Reminder] The user changed the working directory from {old} to {new}. Relative paths and commands now resolve against {new}."
    );
    crate::engine::append_only(hub, s, user_message(MessageSource::Reminder, text, vec![]));
    Ok(())
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
    let Ok(cfg) = load_cfg(hub, &cwd).await else {
        return;
    };
    let mut st = s.lock();
    if format!("{:?}", *st.cfg) != format!("{cfg:?}") {
        st.cfg = Arc::new(cfg);
        st.system = None;
        let _ = hub.mcp.servers_for(&st.cfg);
    }
}

async fn config_info(hub: &Arc<Hub>, cwd: &str) -> Result<ServerConfigInfo, String> {
    let cfg = load_cfg(hub, cwd).await?;
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
    let out = ref_tools::execute("bash", json!({"command": command, "timeout": 600}), &ctx).await;
    let text = format!(
        "I ran this command:\n```\n$ {command}\n{}\n```",
        out.text.trim_end()
    );
    let msg = user_message(MessageSource::Shell, text, vec![]);
    crate::engine::append_only(&hub, &s, msg);
}

/// `$ARGUMENTS`, `$1`..`$9` substitution (the last numbered placeholder takes the rest).
pub fn expand_template(template: &str, args: &str) -> String {
    let words: Vec<&str> = args.split_whitespace().collect();
    let max_n = (1..=9)
        .rev()
        .find(|n| template.contains(&format!("${n}")))
        .unwrap_or(0);
    let mut out = template.replace("$ARGUMENTS", args.trim());
    for n in (1..=9).rev() {
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
    if !template.contains("$ARGUMENTS") && max_n == 0 && !args.trim().is_empty() {
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
        let (Some(m), Some(cmd)) = (c.get(0), c.get(1)) else {
            continue;
        };
        out.push_str(&text[last..m.start()]);
        let res = tokio::time::timeout(
            Duration::from_secs(30),
            tokio::process::Command::new(ref_tools::default_shell())
                .arg("-c")
                .arg(cmd.as_str())
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
            Ok(Err(e)) => out.push_str(&format!("(failed to run `{}`: {e})", cmd.as_str())),
            Err(_) => out.push_str(&format!("(`{}` timed out)", cmd.as_str())),
        }
        last = m.end();
    }
    out.push_str(&text[last..]);
    out
}

async fn list_files(hub: &Arc<Hub>, cwd: &str) -> Result<Arc<Vec<String>>, String> {
    if let Some((t, v)) = hub
        .files_cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(cwd)
        && t.elapsed() < Duration::from_secs(10)
    {
        return Ok(v.clone());
    }
    let root = std::path::PathBuf::from(cwd);
    if !root.is_dir() {
        return Err(format!("Not a directory: {cwd}"));
    }
    let files = tokio::task::spawn_blocking(move || {
        let mut out = Vec::new();
        for e in ignore::WalkBuilder::new(&root)
            .hidden(true)
            .max_depth(Some(16))
            .build()
            .flatten()
        {
            let Ok(rel) = e.path().strip_prefix(&root) else {
                continue;
            };
            let rel = rel.to_string_lossy();
            if rel.is_empty() {
                continue;
            }
            if e.file_type().is_some_and(|t| t.is_dir()) {
                out.push(format!("{rel}/"));
            } else {
                out.push(rel.to_string());
            }
            if out.len() >= 50_000 {
                break;
            }
        }
        out
    })
    .await
    .map_err(|e| e.to_string())?;
    let files = Arc::new(files);
    let mut cache = hub.files_cache.lock().unwrap_or_else(|e| e.into_inner());
    if cache.len() > 32 {
        cache.clear();
    }
    cache.insert(cwd.to_string(), (Instant::now(), files.clone()));
    Ok(files)
}

/// Fuzzy subsequence score (higher is better); None if `q` is not a subsequence of `path`.
pub fn fuzzy_score(path: &str, q: &str) -> Option<i64> {
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
        score += 1;
        if prev == Some(i.wrapping_sub(1)) {
            score += 5;
        }
        if i >= base_start {
            score += 2;
        }
        if i == 0 || matches!(p.get(i.wrapping_sub(1)), Some('/' | '_' | '-' | '.')) {
            score += 3;
        }
        prev = Some(i);
    }
    let base: String = p[base_start.min(p.len())..].iter().collect();
    let ql: String = qc.iter().collect();
    if base.starts_with(&ql) {
        score += 20;
    } else if base.contains(&ql) {
        score += 10;
    }
    score -= (p.len() / 10) as i64;
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

pub fn export_markdown(info: &SessionInfo, messages: &[Message]) -> String {
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
