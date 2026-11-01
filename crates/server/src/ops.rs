//! Session-level operations shared by the request API and the peer tools:
//! create / load / close / persist.

use crate::hub::Hub;
use crate::session::{Session, cancel_unfinished};
use ref_config::Config;
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
        let max = crate::util::MAX_NAME_CHARS - suffix.chars().count();
        let stem: String = base.chars().take(max).collect();
        let cand = format!("{stem}{suffix}");
        if !hub.name_taken(&cand, except) {
            return cand;
        }
    }
    unreachable!()
}

pub(crate) fn validate_cwd(cwd: &str) -> Result<String, String> {
    let p = std::path::Path::new(cwd);
    if !p.is_absolute() {
        return Err(format!("Working directory must be absolute: {cwd}"));
    }
    let c = dunce::canonicalize(p).map_err(|_| format!("Directory does not exist: {cwd}"))?;
    if !c.is_dir() {
        return Err(format!("Not a directory: {cwd}"));
    }
    Ok(c.to_string_lossy().to_string())
}

/// Agent used when the config has no usable default.
const FALLBACK_AGENT: &str = ref_config::DEFAULT_AGENT;

/// Directory name used for the default session name when the cwd has none (`/`).
const FALLBACK_DIR_NAME: &str = "root";

/// Characters of the directory name kept in a generated session name.
const DEFAULT_NAME_STEM_CHARS: usize = 50;

/// Random hex digits appended to a generated session name.
const NAME_SUFFIX_HEX_DIGITS: usize = 4;

/// How long closing a session or undoing waits for the running turn to stop.
pub(crate) const SESSION_STOP_WAIT: Duration = Duration::from_secs(10);

/// How long shutdown waits for each session to stop.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(5);

/// Loads the merged config for `cwd` off the async runtime.
pub(crate) async fn load_config(hub: &Arc<Hub>, cwd: &str) -> Result<Config, String> {
    let hub = hub.clone();
    let cwd = cwd.to_string();
    tokio::task::spawn_blocking(move || hub.load_config(std::path::Path::new(&cwd)))
        .await
        .map_err(|e| format!("loading config failed: {e}"))
}

/// The requested agent if usable, else the config default, else the first visible agent.
fn resolve_agent(cfg: &Config, requested: Option<String>) -> Result<String, String> {
    match requested.filter(|a| !a.trim().is_empty()) {
        Some(a) if a == ref_config::COMPACTION_AGENT => {
            Err(format!("Agent {a} is internal and cannot run a session."))
        }
        Some(a) if cfg.agents.contains_key(&a) => Ok(a),
        Some(a) => Err(format!("Unknown agent: {a}")),
        None if cfg.default_agent != ref_config::COMPACTION_AGENT
            && cfg.agents.contains_key(&cfg.default_agent) =>
        {
            Ok(cfg.default_agent.clone())
        }
        None => Ok(cfg
            .agents
            .values()
            .find(|a| !a.hidden && a.name != ref_config::COMPACTION_AGENT)
            .map(|a| a.name.clone())
            .unwrap_or_else(|| FALLBACK_AGENT.into())),
    }
}

/// The requested model if configured, else the agent's default.
fn resolve_model(
    cfg: &Config,
    agent: &str,
    requested: Option<ModelRef>,
) -> Result<ModelRef, String> {
    let Some(m) = requested else {
        return Ok(cfg.default_model_for(agent).unwrap_or_default());
    };
    if cfg.model(&m).is_none() {
        return Err(format!("Model {} is not configured.", m.key()));
    }
    Ok(m)
}

/// The given name (made unique when allowed), or `<dir>-<hex>` derived from the cwd.
fn pick_name(
    hub: &Hub,
    cwd: &str,
    name: Option<String>,
    unique_suffix: bool,
) -> Result<String, String> {
    let Some(n) = name else {
        let base = std::path::Path::new(cwd)
            .file_name()
            .and_then(|x| x.to_str())
            .filter(|x| !x.is_empty())
            .unwrap_or(FALLBACK_DIR_NAME);
        let base: String = base.chars().take(DEFAULT_NAME_STEM_CHARS).collect();
        loop {
            let cand = format!("{base}-{}", crate::util::random_hex(NAME_SUFFIX_HEX_DIGITS));
            if !hub.name_taken(&cand, None) {
                return Ok(cand);
            }
        }
    };
    let n = crate::util::normalize_name(&n)?;
    if !hub.name_taken(&n, None) {
        return Ok(n);
    }
    if unique_suffix {
        return Ok(unique_name(hub, &n, None));
    }
    Err(format!("A live session named \"{n}\" already exists."))
}

pub async fn create_session(hub: &Arc<Hub>, o: CreateOpts) -> Result<Arc<Session>, String> {
    let cwd = validate_cwd(&hub.effective_cwd(&o.cwd))?;
    let cfg = load_config(hub, &cwd).await?;
    for w in &cfg.warnings {
        tracing::warn!("config: {w}");
    }
    let agent = resolve_agent(&cfg, o.agent)?;
    let model = resolve_model(&cfg, &agent, o.model)?;
    let name = pick_name(hub, &cwd, o.name, o.unique_suffix)?;
    let now = ref_proto::now_ms();
    let context = crate::models::context_for(&cfg, &model);
    let info = SessionInfo {
        id: crate::util::new_session_id(),
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
        last_error: None,
    };
    let cfg = Arc::new(cfg);
    // Start connecting MCP servers in the background.
    hub.mcp.start_servers(&cfg);
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
    let cfg = load_config(hub, &p.info.cwd).await?;
    p.info.live = true;
    p.info.status = SessionStatus::Idle;
    p.info.attached = 0;
    p.info.name = unique_name(hub, &p.info.name, Some(&p.info.id));
    p.info.context = crate::models::context_keeping_used(&cfg, &p.info.model, p.info.context.used);
    let cfg = Arc::new(cfg);
    hub.mcp.start_servers(&cfg);
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
    crate::engine::wait_idle(s, SESSION_STOP_WAIT).await;
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
        crate::engine::wait_idle(s, SHUTDOWN_WAIT).await;
        persist(hub, s);
    }
    let store = hub.store.clone();
    match tokio::task::spawn_blocking(move || store.flush()).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::warn!("flushing the session store failed: {e}"),
        Err(e) => tracing::warn!("flush task failed: {e}"),
    }
    hub.mcp.close_all().await;
}
