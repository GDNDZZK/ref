//! Stash groups and session suspend/resume service.

use crate::api::ApiError;
use crate::hub::Hub;
use crate::session::{Session, user_message};
use crate::store::RawGroup;
use ref_proto::{Message, MessageSource, SessionStatus, StashGroup, StashMember};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum StashResumeKind {
    None,
    Continue,
    Reprompt { wait_kind: String, detail: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StashedResume {
    kind: StashResumeKind,
    #[serde(default)]
    pending_queue: Vec<Message>,
}

fn default_group_name() -> String {
    format!("Stash {}", chrono::Local::now().format("%Y-%m-%d %H:%M"))
}

fn normalize_group_name(name: Option<String>) -> String {
    name.and_then(|s| {
        let t = s.trim();
        (!t.is_empty()).then(|| t.to_string())
    })
    .unwrap_or_else(default_group_name)
}

fn hydrate_groups(hub: &Hub, raw: Vec<RawGroup>) -> Vec<StashGroup> {
    raw.into_iter()
        .map(|g| StashGroup {
            id: g.id,
            name: g.name,
            locked: g.locked,
            created_at: g.created_at,
            updated_at: g.updated_at,
            members: g
                .members
                .into_iter()
                .map(|(session_id, position, mut info)| {
                    if let Some(s) = hub.session(&session_id) {
                        info = s.info();
                    } else {
                        info.live = false;
                        info.attached = 0;
                        info.status = SessionStatus::Idle;
                    }
                    StashMember {
                        session_id,
                        position,
                        info,
                    }
                })
                .collect(),
        })
        .collect()
}

pub(crate) async fn list_groups_hydrated(hub: &Arc<Hub>) -> Result<Vec<StashGroup>, ApiError> {
    let store = hub.store.clone();
    let raw = tokio::task::spawn_blocking(move || store.list_groups_raw())
        .await
        .map_err(|e| ApiError::from(e.to_string()))?
        .map_err(ApiError::from)?;
    Ok(hydrate_groups(hub, raw))
}

async fn stash_live_session(hub: &Arc<Hub>, s: &Arc<Session>) -> Result<(), ApiError> {
    let (kind, pending_queue) = {
        let mut st = s.lock();
        let kind = match &st.info.status {
            SessionStatus::Idle => StashResumeKind::None,
            SessionStatus::Busy
            | SessionStatus::Running
            | SessionStatus::Retrying { .. }
            | SessionStatus::Compacting => StashResumeKind::Continue,
            SessionStatus::WaitingPermission => {
                let tools: Vec<&str> = st.perms.iter().map(|p| p.request.tool.as_str()).collect();
                let detail = if tools.is_empty() {
                    "permission".to_string()
                } else {
                    tools.join(", ")
                };
                StashResumeKind::Reprompt {
                    wait_kind: "permission".into(),
                    detail,
                }
            }
            SessionStatus::WaitingQuestion => {
                let headers: Vec<&str> = st
                    .questions
                    .iter()
                    .flat_map(|q| q.request.questions.iter().map(|qq| qq.header.as_str()))
                    .collect();
                let detail = if headers.is_empty() {
                    "question".to_string()
                } else {
                    headers.join(", ")
                };
                StashResumeKind::Reprompt {
                    wait_kind: "question".into(),
                    detail,
                }
            }
        };
        let pending_queue: Vec<Message> = st.queue.drain(..).collect();
        if !pending_queue.is_empty() {
            crate::engine::emit_queue(hub, s, &st);
        }
        st.force_persist = true;
        (kind, pending_queue)
    };

    crate::ops::close_session(hub, s).await;

    let resume_json = if matches!(kind, StashResumeKind::None) && pending_queue.is_empty() {
        None
    } else {
        let payload = StashedResume {
            kind,
            pending_queue,
        };
        Some(serde_json::to_string(&payload).map_err(|e| ApiError::from(e.to_string()))?)
    };
    let store = hub.store.clone();
    let sid = s.id.clone();
    tokio::task::spawn_blocking(move || store.set_stashed_resume(&sid, resume_json))
        .await
        .map_err(|e| ApiError::from(e.to_string()))?
        .map_err(ApiError::from)?;
    Ok(())
}

async fn prepare_session_for_group(
    hub: &Arc<Hub>,
    session_id: &str,
    stash_live: bool,
) -> Result<(), ApiError> {
    if let Some(s) = hub.session(session_id) {
        if stash_live {
            stash_live_session(hub, &s).await?;
        } else {
            s.lock().force_persist = true;
            crate::ops::persist(hub, &s);
        }
        return Ok(());
    }
    let store = hub.store.clone();
    let sid = session_id.to_string();
    let exists = tokio::task::spawn_blocking(move || store.exists(&sid))
        .await
        .map_err(|e| ApiError::from(e.to_string()))?;
    if !exists {
        return Err(ApiError::from(format!("Session not found: {session_id}")));
    }
    Ok(())
}

async fn restore_session(hub: &Arc<Hub>, session_id: &str) -> Result<bool, ApiError> {
    if hub.session(session_id).is_some() {
        return Ok(false);
    }
    let s = crate::ops::get_or_load(hub, session_id)
        .await
        .map_err(ApiError::from)?;

    let store = hub.store.clone();
    let sid = session_id.to_string();
    let raw_resume = tokio::task::spawn_blocking(move || store.take_stashed_resume(&sid))
        .await
        .map_err(|e| ApiError::from(e.to_string()))?
        .map_err(ApiError::from)?;

    let Some(raw) = raw_resume else {
        return Ok(true);
    };
    let resume = match serde_json::from_str::<StashedResume>(&raw) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(session = %session_id, "ignoring corrupt stashed_resume: {e}");
            return Ok(true);
        }
    };
    if !resume.pending_queue.is_empty() {
        let mut st = s.lock();
        st.queue.extend(resume.pending_queue);
        crate::engine::emit_queue(hub, &s, &st);
    }
    let reminder = match resume.kind {
        StashResumeKind::None => None,
        StashResumeKind::Continue => Some(
            "[Reminder] This session was stashed while working and has been restored. Continue your work from where you left off."
                .to_string(),
        ),
        StashResumeKind::Reprompt { wait_kind, detail } => Some(format!(
            "[Reminder] This session was stashed while waiting for user input ({wait_kind}: {detail}). Re-issue the pending {wait_kind} request for confirmation without skipping it."
        )),
    };
    if let Some(text) = reminder {
        let msg = user_message(MessageSource::Reminder, text, vec![]);
        crate::engine::submit(hub, &s, msg);
    }
    Ok(true)
}

pub(crate) async fn list_groups(hub: &Arc<Hub>) -> Result<Value, ApiError> {
    let _guard = hub.stash_lock.lock().await;
    let groups = list_groups_hydrated(hub).await?;
    Ok(json!({ "groups": groups }))
}

pub(crate) async fn stash_all(hub: &Arc<Hub>, name: Option<String>) -> Result<Value, ApiError> {
    let _guard = hub.stash_lock.lock().await;
    let mut live = hub.sessions();
    if live.is_empty() {
        let groups = list_groups_hydrated(hub).await?;
        return Ok(json!({ "group": Value::Null, "groups": groups }));
    }
    live.sort_by(|a, b| {
        let ai = a.info();
        let bi = b.info();
        ai.created_at.cmp(&bi.created_at).then(ai.id.cmp(&bi.id))
    });
    let mut session_ids = Vec::with_capacity(live.len());
    for s in live {
        session_ids.push(s.id.clone());
        stash_live_session(hub, &s).await?;
    }
    let group_id = format!("grp_{}", crate::util::random_hex(8));
    let group_name = normalize_group_name(name);
    let store = hub.store.clone();
    let gid = group_id.clone();
    let raw = tokio::task::spawn_blocking(move || store.create_group(gid, group_name, session_ids))
        .await
        .map_err(|e| ApiError::from(e.to_string()))?
        .map_err(ApiError::from)?;
    let groups = hydrate_groups(hub, raw);
    let group = groups.iter().find(|g| g.id == group_id).cloned();
    hub.broadcast_groups_changed(groups.clone());
    Ok(json!({ "group": group, "groups": groups }))
}

pub(crate) async fn create_group(
    hub: &Arc<Hub>,
    name: Option<String>,
    session_ids: Vec<String>,
    stash_live: bool,
) -> Result<Value, ApiError> {
    let _guard = hub.stash_lock.lock().await;
    let mut dedup = Vec::new();
    for sid in session_ids {
        if !dedup.contains(&sid) {
            dedup.push(sid);
        }
    }
    for sid in &dedup {
        prepare_session_for_group(hub, sid, stash_live).await?;
    }
    let group_id = format!("grp_{}", crate::util::random_hex(8));
    let group_name = normalize_group_name(name);
    let store = hub.store.clone();
    let gid = group_id.clone();
    let raw = tokio::task::spawn_blocking(move || store.create_group(gid, group_name, dedup))
        .await
        .map_err(|e| ApiError::from(e.to_string()))?
        .map_err(ApiError::from)?;
    let groups = hydrate_groups(hub, raw);
    let group = groups
        .iter()
        .find(|g| g.id == group_id)
        .cloned()
        .ok_or_else(|| ApiError::from("Created stash group missing."))?;
    hub.broadcast_groups_changed(groups.clone());
    Ok(json!({ "group": group, "groups": groups }))
}

pub(crate) async fn rename_group(
    hub: &Arc<Hub>,
    group_id: String,
    name: String,
) -> Result<Value, ApiError> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(ApiError::from("Group name cannot be empty."));
    }
    let _guard = hub.stash_lock.lock().await;
    let store = hub.store.clone();
    let raw = tokio::task::spawn_blocking(move || store.rename_group(group_id, name))
        .await
        .map_err(|e| ApiError::from(e.to_string()))?
        .map_err(ApiError::from)?;
    let groups = hydrate_groups(hub, raw);
    hub.broadcast_groups_changed(groups.clone());
    Ok(json!({ "groups": groups }))
}

pub(crate) async fn set_group_locked(
    hub: &Arc<Hub>,
    group_id: String,
    locked: bool,
) -> Result<Value, ApiError> {
    let _guard = hub.stash_lock.lock().await;
    let store = hub.store.clone();
    let raw = tokio::task::spawn_blocking(move || store.set_group_locked(group_id, locked))
        .await
        .map_err(|e| ApiError::from(e.to_string()))?
        .map_err(ApiError::from)?;
    let groups = hydrate_groups(hub, raw);
    hub.broadcast_groups_changed(groups.clone());
    Ok(json!({ "groups": groups }))
}

pub(crate) async fn delete_group(hub: &Arc<Hub>, group_id: String) -> Result<Value, ApiError> {
    let _guard = hub.stash_lock.lock().await;
    let store = hub.store.clone();
    let raw = tokio::task::spawn_blocking(move || store.delete_group(group_id))
        .await
        .map_err(|e| ApiError::from(e.to_string()))?
        .map_err(ApiError::from)?;
    let groups = hydrate_groups(hub, raw);
    hub.broadcast_groups_changed(groups.clone());
    Ok(json!({ "groups": groups }))
}

pub(crate) async fn add_to_group(
    hub: &Arc<Hub>,
    group_id: String,
    session_id: String,
    position: Option<u32>,
    stash_live: bool,
) -> Result<Value, ApiError> {
    let _guard = hub.stash_lock.lock().await;
    prepare_session_for_group(hub, &session_id, stash_live).await?;
    let store = hub.store.clone();
    let raw =
        tokio::task::spawn_blocking(move || store.add_to_group(group_id, session_id, position))
            .await
            .map_err(|e| ApiError::from(e.to_string()))?
            .map_err(ApiError::from)?;
    let groups = hydrate_groups(hub, raw);
    hub.broadcast_groups_changed(groups.clone());
    Ok(json!({ "groups": groups }))
}

pub(crate) async fn move_group_member(
    hub: &Arc<Hub>,
    from_group_id: Option<String>,
    to_group_id: String,
    session_id: String,
    position: u32,
    stash_live: bool,
) -> Result<Value, ApiError> {
    let _guard = hub.stash_lock.lock().await;
    prepare_session_for_group(hub, &session_id, stash_live).await?;
    let store = hub.store.clone();
    let raw = tokio::task::spawn_blocking(move || {
        store.move_group_member(from_group_id, to_group_id, session_id, position)
    })
    .await
    .map_err(|e| ApiError::from(e.to_string()))?
    .map_err(ApiError::from)?;
    let groups = hydrate_groups(hub, raw);
    hub.broadcast_groups_changed(groups.clone());
    Ok(json!({ "groups": groups }))
}

pub(crate) async fn remove_from_group(
    hub: &Arc<Hub>,
    group_id: String,
    session_id: String,
) -> Result<Value, ApiError> {
    let _guard = hub.stash_lock.lock().await;
    let store = hub.store.clone();
    let raw = tokio::task::spawn_blocking(move || store.remove_from_group(group_id, session_id))
        .await
        .map_err(|e| ApiError::from(e.to_string()))?
        .map_err(ApiError::from)?;
    let groups = hydrate_groups(hub, raw);
    hub.broadcast_groups_changed(groups.clone());
    Ok(json!({ "groups": groups }))
}

pub(crate) async fn merge_groups(
    hub: &Arc<Hub>,
    source_group_id: String,
    target_group_id: String,
) -> Result<Value, ApiError> {
    let _guard = hub.stash_lock.lock().await;
    let store = hub.store.clone();
    let raw =
        tokio::task::spawn_blocking(move || store.merge_groups(source_group_id, target_group_id))
            .await
            .map_err(|e| ApiError::from(e.to_string()))?
            .map_err(ApiError::from)?;
    let groups = hydrate_groups(hub, raw);
    hub.broadcast_groups_changed(groups.clone());
    Ok(json!({ "groups": groups }))
}

pub(crate) async fn restore_group(hub: &Arc<Hub>, group_id: String) -> Result<Value, ApiError> {
    let _guard = hub.stash_lock.lock().await;
    let groups_before = list_groups_hydrated(hub).await?;
    let group = groups_before
        .into_iter()
        .find(|g| g.id == group_id)
        .ok_or_else(|| ApiError::from(format!("Stash group not found: {group_id}")))?;

    let mut restored_ids = Vec::new();
    for m in &group.members {
        match restore_session(hub, &m.session_id).await {
            Ok(true) => restored_ids.push(m.session_id.clone()),
            Ok(false) => {}
            Err(e) => {
                tracing::warn!(session = %m.session_id, "restoring stashed group member failed: {e}");
            }
        }
    }

    let store = hub.store.clone();
    let raw = tokio::task::spawn_blocking(move || store.finish_restore_group(group_id))
        .await
        .map_err(|e| ApiError::from(e.to_string()))?
        .map_err(ApiError::from)?;
    let groups = hydrate_groups(hub, raw);
    hub.broadcast_groups_changed(groups.clone());
    Ok(json!({ "restored_ids": restored_ids, "groups": groups }))
}

pub(crate) async fn restore_group_member(
    hub: &Arc<Hub>,
    group_id: String,
    session_id: String,
) -> Result<Value, ApiError> {
    let _guard = hub.stash_lock.lock().await;
    let groups_before = list_groups_hydrated(hub).await?;
    let group = groups_before
        .into_iter()
        .find(|g| g.id == group_id)
        .ok_or_else(|| ApiError::from(format!("Stash group not found: {group_id}")))?;
    if !group.members.iter().any(|m| m.session_id == session_id) {
        return Err(ApiError::from(format!(
            "Session {session_id} is not in group {group_id}."
        )));
    }

    let restored = restore_session(hub, &session_id).await?;
    let store = hub.store.clone();
    let raw = tokio::task::spawn_blocking(move || {
        store.finish_restore_group_member(group_id, session_id)
    })
    .await
    .map_err(|e| ApiError::from(e.to_string()))?
    .map_err(ApiError::from)?;
    let groups = hydrate_groups(hub, raw);
    hub.broadcast_groups_changed(groups.clone());
    Ok(json!({ "restored": restored, "groups": groups }))
}
