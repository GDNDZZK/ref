//! Session-level operations shared by the request API and the peer tools:
//! create / load / close / persist.

use crate::hub::Hub;
use crate::session::{Session, cancel_unfinished};
use ref_proto::{ModelRef, SessionEvent, SessionInfo, SessionStatus};
use std::sync::Arc;
use std::time::Duration;

pub struct CreateOpts {
    pub cwd: String,
    pub agent: Option<String>,
    pub model: Option<ModelRef>,
    pub name: Option<String>,
    /// On name collision: append `-2`, `-3`, ... instead of failing.
    pub unique_suffix: bool,
    pub parent: Option<String>,
    pub must_report: bool,
}

fn unique_name(hub: &Hub, base: &str, except: Option<&str>) -> String {
    if !hub.name_taken(base, except) {
        return base.to_string();
    }
    for i in 2.. {
        let suffix = format!("-{i}");
        let max = 64 - suffix.chars().count();
        let stem: String = base.chars().take(max).collect();
        let cand = format!("{stem}{suffix}");
        if !hub.name_taken(&cand, except) {
            return cand;
        }
    }
    unreachable!()
}

fn validate_cwd(cwd: &str) -> Result<String, String> {
    let p = std::path::Path::new(cwd);
    if !p.is_absolute() {
        return Err(format!("Working directory must be absolute: {cwd}"));
    }
    let c = std::fs::canonicalize(p).map_err(|_| format!("Directory does not exist: {cwd}"))?;
    if !c.is_dir() {
        return Err(format!("Not a directory: {cwd}"));
    }
    Ok(c.to_string_lossy().to_string())
}

pub async fn create_session(hub: &Arc<Hub>, o: CreateOpts) -> Result<Arc<Session>, String> {
    let cwd = validate_cwd(&o.cwd)?;
    let cfg = {
        let hub = hub.clone();
        let cwd = cwd.clone();
        tokio::task::spawn_blocking(move || hub.load_config(std::path::Path::new(&cwd)))
            .await
            .map_err(|e| format!("loading config failed: {e}"))?
    };
    for w in &cfg.warnings {
        tracing::warn!("config: {w}");
    }
    let agent = match o.agent.filter(|a| !a.trim().is_empty()) {
        Some(a) if a == ref_config::COMPACTION_AGENT => {
            return Err(format!("Agent {a} is internal and cannot run a session."));
        }
        Some(a) if cfg.agents.contains_key(&a) => a,
        Some(a) => return Err(format!("Unknown agent: {a}")),
        None if cfg.default_agent != ref_config::COMPACTION_AGENT && cfg.agents.contains_key(&cfg.default_agent) => {
            cfg.default_agent.clone()
        }
        None => cfg
            .agents
            .values()
            .find(|a| !a.hidden && a.name != ref_config::COMPACTION_AGENT)
            .map(|a| a.name.clone())
            .unwrap_or_else(|| "build".into()),
    };
    let model = match o.model {
        Some(m) => {
            if cfg.model(&m).is_none() {
                return Err(format!("Model {} is not configured.", m.key()));
            }
            m
        }
        None => cfg.default_model_for(&agent).unwrap_or_default(),
    };
    let name = match o.name {
        Some(n) => {
            let n = crate::util::normalize_name(&n)?;
            if hub.name_taken(&n, None) {
                if o.unique_suffix {
                    unique_name(hub, &n, None)
                } else {
                    return Err(format!("A live session named \"{n}\" already exists."));
                }
            } else {
                n
            }
        }
        None => {
            let base = std::path::Path::new(&cwd)
                .file_name()
                .and_then(|x| x.to_str())
                .filter(|x| !x.is_empty())
                .unwrap_or("root");
            let base: String = base.chars().take(50).collect();
            loop {
                let cand = format!("{base}-{}", crate::util::random_hex(4));
                if !hub.name_taken(&cand, None) {
                    break cand;
                }
            }
        }
    };
    let now = ref_proto::now_ms();
    let context = crate::models::context_for(&cfg, &model);
    let info = SessionInfo {
        id: crate::util::new_id("ses"),
        name,
        title: String::new(),
        agent,
        model,
        cwd,
        created_at: now,
        updated_at: now,
        status: SessionStatus::Idle,
        attached: 0,
        live: true,
        parent: o.parent,
        must_report: o.must_report,
        context,
        todos: Vec::new(),
        busy_since: None,
    };
    let cfg = Arc::new(cfg);
    // Start connecting MCP servers in the background.
    let _ = hub.mcp.servers_for(&cfg);
    let s = Session::new(info, Vec::new(), cfg, false, false);
    hub.insert_session(s.clone());
    tracing::info!(session = %s.id, "session created");
    Ok(s)
}

/// Live session by id, loading it from disk if necessary.
pub async fn get_or_load(hub: &Arc<Hub>, id: &str) -> Result<Arc<Session>, String> {
    if let Some(s) = hub.session(id) {
        return Ok(s);
    }
    let store = hub.store.clone();
    let id2 = id.to_string();
    let mut p = tokio::task::spawn_blocking(move || store.load(&id2))
        .await
        .map_err(|e| format!("loading session failed: {e}"))??;
    if let Some(s) = hub.session(id) {
        return Ok(s);
    }
    cancel_unfinished(&mut p.messages, crate::engine::CANCELLED_OUTPUT);
    let cwd = p.info.cwd.clone();
    let cfg = {
        let hub = hub.clone();
        tokio::task::spawn_blocking(move || hub.load_config(std::path::Path::new(&cwd)))
            .await
            .map_err(|e| format!("loading config failed: {e}"))?
    };
    p.info.live = true;
    p.info.status = SessionStatus::Idle;
    p.info.attached = 0;
    p.info.name = unique_name(hub, &p.info.name, Some(&p.info.id));
    let mut ctx = crate::models::context_for(&cfg, &p.info.model);
    ctx.used = p.info.context.used;
    p.info.context = ctx;
    let cfg = Arc::new(cfg);
    let _ = hub.mcp.servers_for(&cfg);
    let s = Session::new(p.info, p.messages, cfg, p.reported, p.context_notice);
    hub.insert_session(s.clone());
    Ok(s)
}

/// Queue an (incremental) save of the session; sessions that never had a message are skipped.
pub fn persist(hub: &Hub, s: &Session) {
    hub.store.save(s.persisted());
}

/// Abort, persist and unload a live session.
pub async fn close_session(hub: &Arc<Hub>, s: &Arc<Session>) {
    crate::engine::abort(hub, s, true);
    crate::engine::wait_idle(s, Duration::from_secs(10)).await;
    s.lock().closed = true;
    persist(hub, s);
    hub.emit(&s.id, SessionEvent::Closed);
    hub.remove_session(&s.id);
    tracing::info!(session = %s.id, "session closed");
}

/// Graceful shutdown: abort every run, persist all sessions, close MCP clients.
pub async fn shutdown_all(hub: &Arc<Hub>) {
    let all = hub.sessions();
    for s in &all {
        crate::engine::abort(hub, s, true);
    }
    for s in &all {
        crate::engine::wait_idle(s, Duration::from_secs(5)).await;
        persist(hub, s);
    }
    let store = hub.store.clone();
    if let Ok(Err(e)) = tokio::task::spawn_blocking(move || store.flush()).await {
        tracing::warn!("flushing the session store failed: {e}");
    }
    hub.mcp.close_all().await;
}
