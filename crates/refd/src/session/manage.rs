//! Session lifecycle: create, open from history, close, delete, rename, model/agent changes.

use std::sync::Arc;
use std::time::Duration;

use ref_proto::{ContextInfo, MessageOrigin, NoticeLevel, ServerMsg, SessionInfo, SessionStatus};

use super::{engine, repair_loaded, store, SessionCore};
use crate::hub::Hub;
use crate::peers;

#[derive(Debug, Clone, Default)]
pub struct CreateOpts {
    pub cwd: String,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub variant: Option<String>,
    pub name: Option<String>,
    pub parent: Option<String>,
    pub must_report: bool,
    /// Model to use when the agent has no model of its own (children inherit the parent's).
    pub fallback_model: Option<(String, Option<String>)>,
}

fn normalize_cwd(cwd: &str) -> Result<String, String> {
    let p = std::path::Path::new(cwd);
    if cwd.trim().is_empty() || !p.is_absolute() {
        return Err(format!("Working directory must be an absolute path: {cwd:?}"));
    }
    let canon = std::fs::canonicalize(p).map_err(|e| format!("Working directory {cwd}: {e}"))?;
    if !canon.is_dir() {
        return Err(format!("Not a directory: {cwd}"));
    }
    Ok(canon.to_string_lossy().into_owned())
}

/// Names of open sessions except `except`.
fn taken_names(hub: &Hub, except: &str) -> Vec<String> {
    hub.open_sessions().iter().filter(|s| s.id != except).map(|s| s.name()).collect()
}

impl Hub {
    pub fn create_session(self: &Arc<Self>, opts: CreateOpts) -> Result<Arc<SessionCore>, String> {
        let cwd = normalize_cwd(&opts.cwd)?;
        let cfg = self.config(&cwd).map_err(|e| format!("Configuration error: {e:#}"))?;
        let agent = opts.agent.filter(|a| !a.is_empty()).unwrap_or_else(|| cfg.default_agent.clone());
        if !cfg.agents.contains_key(&agent) {
            let names: Vec<&str> = cfg.agents.keys().map(String::as_str).collect();
            return Err(format!("Unknown agent \"{agent}\". Available: {}", names.join(", ")));
        }
        let (model, variant) = match opts.model.filter(|m| !m.is_empty()) {
            Some(m) => {
                let spec = cfg.resolve_for_agent(&agent, &m, opts.variant.as_deref()).map_err(|e| format!("{e:#}"))?;
                (spec.full_id(), opts.variant.or_else(|| m.split_once('#').map(|x| x.1.to_string())))
            }
            None => match (cfg.agents.get(&agent).and_then(|a| a.model.clone()), opts.fallback_model) {
                (None, Some(fb)) if cfg.resolve_model(&fb.0, None).is_ok() => fb,
                _ => cfg
                    .default_model_for(&agent)
                    .ok_or_else(|| "No model configured: add a provider to your opencode config".to_string())?,
            },
        };
        let variant = opts_variant(variant);
        let id = ref_proto::new_id("ses");
        let now = ref_proto::now_ms();
        let auto_name = opts.name.as_deref().is_none_or(|n| peers::clean_name(n).is_empty());
        let base = match opts.name.as_deref().map(peers::clean_name).filter(|n| !n.is_empty()) {
            Some(n) => n,
            None => format!("{} {}", crate::util::basename(&cwd), short_id(&id)),
        };
        let core = {
            let _names = self.names.lock();
            let name = peers::unique_name(&base, &taken_names(self, &id));
            let info = SessionInfo {
                id: id.clone(),
                name,
                cwd,
                agent,
                model,
                variant,
                created: now,
                updated: now,
                open: true,
                attached: 0,
                status: SessionStatus::Idle,
                context: ContextInfo::default(),
                cost: 0.0,
                parent: opts.parent,
                must_report: opts.must_report,
                message_count: 0,
                pending_permissions: 0,
            };
            let core = SessionCore::new(self.clone(), info, Vec::new(), auto_name, 0);
            self.insert_session(core.clone());
            core
        };
        core.refresh_context_max();
        core.persist();
        engine::spawn_driver(&core);
        tracing::info!("session {} created", core.id);
        Ok(core)
    }

    /// Open a session (load it from history if needed).
    pub fn open_session(self: &Arc<Self>, id: &str) -> Result<Arc<SessionCore>, String> {
        if let Some(s) = self.session(id) {
            return Ok(s);
        }
        if !store::valid_id(id) {
            return Err(format!("Unknown session {id}"));
        }
        let _names = self.names.lock();
        if let Some(s) = self.session(id) {
            return Ok(s);
        }
        let meta = store::read_meta(&self.sessions_dir, id).ok_or_else(|| format!("Unknown session {id}"))?;
        let (mut messages, lines) = store::load_messages(&self.sessions_dir, id);
        repair_loaded(&mut messages);
        let mut info = meta.to_info();
        info.open = true;
        info.name = peers::unique_name(&info.name, &taken_names(self, id));
        info.message_count = messages.len() as u32;
        let core = SessionCore::new(self.clone(), info, messages, meta.auto_name, lines);
        {
            // Never regenerate titles of sessions that already had prompts.
            let mut st = core.st.lock();
            st.title_started = st.messages.iter().any(|m| m.origin == MessageOrigin::User);
            if let Some(u) = super::last_context_used(&st.messages) {
                st.info.context.used = u;
            }
        }
        self.insert_session(core.clone());
        drop(_names);
        core.refresh_context_max();
        engine::spawn_driver(&core);
        tracing::info!("session {id} opened from history");
        Ok(core)
    }

    /// Abort and unload a session (it stays in history).
    pub async fn close_session(self: &Arc<Self>, id: &str) -> Result<(), String> {
        let core = self.session(id).ok_or_else(|| format!("Session {id} is not open"))?;
        core.shutdown().await;
        let info = core.info();
        self.broadcast_all(&ServerMsg::SessionClosed { session_id: id.to_string() });
        self.forget_session_subs(id);
        self.remove_session(id, Some(info));
        tracing::info!("session {id} closed");
        Ok(())
    }

    pub async fn delete_session(self: &Arc<Self>, id: &str) -> Result<(), String> {
        if self.session(id).is_some() {
            self.close_session(id).await?;
        }
        if !store::valid_id(id) || (store::read_meta(&self.sessions_dir, id).is_none() && self.history_info(id).is_none()) {
            return Err(format!("Unknown session {id}"));
        }
        store::delete(&self.sessions_dir, id);
        self.forget_history(id);
        Ok(())
    }

    /// Close every open session (daemon shutdown).
    pub async fn close_all(self: &Arc<Self>) {
        let sessions = self.open_sessions();
        futures::future::join_all(sessions.iter().map(|s| s.shutdown())).await;
    }
}

fn opts_variant(v: Option<String>) -> Option<String> {
    v.filter(|v| !v.is_empty())
}

pub fn short_id(id: &str) -> String {
    let s: Vec<char> = id.chars().collect();
    s[s.len().saturating_sub(6)..].iter().collect()
}

impl SessionCore {
    /// Abort work, stop the driver and write the compacted log.
    pub(crate) async fn shutdown(&self) {
        {
            let mut st = self.st.lock();
            st.closing = true;
        }
        self.abort();
        self.closed.cancel();
        let handle = self.take_driver();
        if let Some(h) = handle {
            if tokio::time::timeout(Duration::from_secs(10), h).await.is_err() {
                tracing::warn!("session {}: driver did not stop in time", self.id);
            }
        }
        {
            let mut st = self.st.lock();
            st.cancel_permissions(&self.hub);
            st.drain_queue();
            st.info.status = SessionStatus::Idle;
        }
        self.persist_compacted();
    }

    /// Update `context.max` / `compact_at` from the current model.
    pub fn refresh_context_max(&self) {
        let (cwd, agent, model, variant) = {
            let st = self.st.lock();
            (st.info.cwd.clone(), st.info.agent.clone(), st.info.model.clone(), st.info.variant.clone())
        };
        let Ok(cfg) = self.hub.config(&cwd) else { return };
        let Ok(spec) = cfg.resolve_for_agent(&agent, &model, variant.as_deref()) else { return };
        let mut st = self.st.lock();
        st.info.context.max = spec.context;
        st.info.context.compact_at = cfg.compact_at(&spec);
        if st.info.context.used == 0 && !st.messages.is_empty() {
            st.info.context.used = st.estimate_context();
        }
        st.info_changed(&self.hub);
    }

    /// Rename. With `exact`, a name taken by another open session is an error; otherwise a
    /// unique suffix is added. `refresh_prompt`: rebuild the system prompt (which mentions the
    /// name); automatic titles skip this to keep the prompt cache warm (the id stays valid).
    pub fn rename(&self, name: &str, exact: bool, refresh_prompt: bool) -> Result<String, String> {
        let name = peers::clean_name(name);
        if name.is_empty() {
            return Err("Name must not be empty".into());
        }
        if self.closed.is_cancelled() {
            // E.g. a late title for a session that was closed (and maybe reopened as a new core).
            return Err("Session is closed".into());
        }
        let _names = self.hub.names.lock();
        let taken = taken_names(&self.hub, &self.id);
        let final_name = if exact {
            if taken.contains(&name) {
                return Err(format!("Another open session is already named \"{name}\""));
            }
            name
        } else {
            peers::unique_name(&name, &taken)
        };
        {
            let mut st = self.st.lock();
            st.info.name = final_name.clone();
            st.auto_name = false;
            if refresh_prompt {
                st.system = None;
            }
            st.touch();
            st.info_changed(&self.hub);
        }
        drop(_names);
        self.persist();
        Ok(final_name)
    }

    pub fn set_model(&self, model: &str, variant: Option<String>) -> Result<(), String> {
        let (cwd, agent) = {
            let st = self.st.lock();
            (st.info.cwd.clone(), st.info.agent.clone())
        };
        let cfg = self.hub.config(&cwd).map_err(|e| format!("{e:#}"))?;
        let variant = opts_variant(variant);
        let spec = cfg.resolve_for_agent(&agent, model, variant.as_deref()).map_err(|e| format!("{e:#}"))?;
        {
            let mut st = self.st.lock();
            st.info.model = spec.full_id();
            st.info.variant = variant.or_else(|| model.split_once('#').map(|x| x.1.to_string()));
            st.info.context.max = spec.context;
            st.info.context.compact_at = cfg.compact_at(&spec);
            st.touch();
            st.info_changed(&self.hub);
        }
        self.persist();
        self.retry_wake.notify_waiters();
        Ok(())
    }

    pub fn set_variant(&self, variant: Option<String>) -> Result<(), String> {
        let model = self.st.lock().info.model.clone();
        self.set_model(&model, variant)
    }

    pub fn set_agent(&self, agent: &str) -> Result<(), String> {
        let cwd = self.cwd();
        let cfg = self.hub.config(&cwd).map_err(|e| format!("{e:#}"))?;
        if !cfg.agents.contains_key(agent) {
            let names: Vec<&str> = cfg.agents.keys().map(String::as_str).collect();
            return Err(format!("Unknown agent \"{agent}\". Available: {}", names.join(", ")));
        }
        let (model, variant) =
            cfg.default_model_for(agent).ok_or_else(|| "No model configured for this agent".to_string())?;
        {
            let mut st = self.st.lock();
            st.info.agent = agent.to_string();
            st.info.model = model;
            st.info.variant = variant;
            st.system = None;
            st.allowed.clear();
            st.touch();
            // Broadcast even if the model can't be resolved (refresh_context_max bails out).
            st.info_changed(&self.hub);
        }
        self.refresh_context_max();
        self.persist();
        self.retry_wake.notify_waiters();
        Ok(())
    }

    pub(crate) fn set_driver(&self, h: tokio::task::JoinHandle<()>) {
        *self.driver.lock() = Some(h);
    }

    fn take_driver(&self) -> Option<tokio::task::JoinHandle<()>> {
        self.driver.lock().take()
    }

    pub fn notice(&self, level: NoticeLevel, text: impl Into<String>) {
        self.hub.notice(&self.id, level, text);
    }
}
