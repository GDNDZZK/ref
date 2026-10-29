//! Shared server state: connected frontends, open sessions, config cache, MCP clients.
//!
//! Lock order (never acquire in the opposite direction):
//!   session state (`Session::st`) -> `Hub::clients`
//! `Hub::sessions` and `Hub::configs` are only held to clone/insert entries.

use crate::mcp::McpManager;
use crate::persist::SessionMeta;
use crate::session::Session;
use crate::util;
use hn_config::Config;
use hn_proto::{now_ms, Event, ModelRef, ServerMsg, SessionInfo, ToastLevel};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch, Notify};

pub type ClientTx = mpsc::UnboundedSender<ServerMsg>;

pub struct ClientEntry {
    pub tx: ClientTx,
    pub cwd: String,
    pub watch_bg: bool,
    /// Sent Hello as a real frontend (not a `--status` probe): counts for the idle timer.
    pub frontend: bool,
    /// session id -> foreground
    pub attached: HashMap<String, bool>,
}

pub struct Hub {
    pub http: reqwest::Client,
    pub mcp: McpManager,
    pub server_cwd: String,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    clients: Mutex<HashMap<u64, ClientEntry>>,
    next_client: AtomicU64,
    configs: Mutex<HashMap<PathBuf, (Instant, Arc<Config>)>>,
    sessions_changed: Arc<Notify>,
    client_count: watch::Sender<usize>,
    shutting_down: AtomicBool,
    /// Serializes session opening (load from disk) so a session is never loaded twice.
    open_lock: tokio::sync::Mutex<()>,
}

pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn fallback_config() -> Arc<Config> {
    let mut c = Config::default();
    c.default_agent = "build".into();
    Arc::new(c)
}

const CONFIG_TTL: Duration = Duration::from_secs(3);

impl Hub {
    pub fn new() -> Arc<Hub> {
        let (count_tx, _) = watch::channel(0usize);
        let hub = Arc::new(Hub {
            http: hn_llm::http_client(),
            mcp: McpManager::default(),
            server_cwd: std::env::current_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
            sessions: Mutex::new(HashMap::new()),
            clients: Mutex::new(HashMap::new()),
            next_client: AtomicU64::new(1),
            configs: Mutex::new(HashMap::new()),
            sessions_changed: Arc::new(Notify::new()),
            client_count: count_tx,
            shutting_down: AtomicBool::new(false),
            open_lock: tokio::sync::Mutex::new(()),
        });
        // Coalesced `Sessions` pushes (at most every ~100ms).
        let weak = Arc::downgrade(&hub);
        let changed = hub.sessions_changed.clone();
        tokio::spawn(async move {
            loop {
                changed.notified().await;
                tokio::time::sleep(Duration::from_millis(100)).await;
                let Some(h) = weak.upgrade() else { return };
                let list = h.open_list();
                h.broadcast(ServerMsg::Sessions { sessions: list });
            }
        });
        hub
    }

    pub fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::SeqCst)
    }

    // --- clients -------------------------------------------------------------

    pub fn add_client(&self, tx: ClientTx) -> u64 {
        let id = self.next_client.fetch_add(1, Ordering::Relaxed);
        lock(&self.clients)
            .insert(id, ClientEntry { tx, cwd: String::new(), watch_bg: false, frontend: false, attached: HashMap::new() });
        id
    }

    fn frontends(c: &HashMap<u64, ClientEntry>) -> usize {
        c.values().filter(|e| e.frontend).count()
    }

    pub fn remove_client(&self, id: u64) {
        let (entry, n) = {
            let mut c = lock(&self.clients);
            let e = c.remove(&id);
            (e, Self::frontends(&c))
        };
        if let Some(e) = entry {
            for (sid, _) in e.attached {
                if let Some(s) = self.get(&sid) {
                    s.recount_attached(self);
                }
            }
        }
        self.client_count.send_replace(n);
        self.touch();
    }

    /// Connected frontends (status probes excluded).
    pub fn client_count(&self) -> usize {
        Self::frontends(&lock(&self.clients))
    }

    pub fn subscribe_clients(&self) -> watch::Receiver<usize> {
        self.client_count.subscribe()
    }

    pub fn hello(&self, id: u64, cwd: &str, frontend: bool) {
        let n = {
            let mut c = lock(&self.clients);
            if let Some(e) = c.get_mut(&id) {
                e.cwd = cwd.to_string();
                e.frontend = frontend;
            }
            Self::frontends(&c)
        };
        self.client_count.send_replace(n);
    }

    pub fn client_cwd(&self, id: u64) -> String {
        let c = lock(&self.clients).get(&id).map(|c| c.cwd.clone()).unwrap_or_default();
        if c.is_empty() {
            self.server_cwd.clone()
        } else {
            c
        }
    }

    pub fn set_watch(&self, id: u64, on: bool) {
        if let Some(c) = lock(&self.clients).get_mut(&id) {
            c.watch_bg = on;
        }
    }

    /// Register an attachment. Must be called with the session state locked (see `Session::attach`).
    pub(crate) fn attach_client(&self, cid: u64, sid: &str, foreground: bool) -> bool {
        let mut c = lock(&self.clients);
        match c.get_mut(&cid) {
            Some(e) => {
                let fg = e.attached.get(sid).copied().unwrap_or(false) || foreground;
                e.attached.insert(sid.to_string(), fg);
                true
            }
            None => false,
        }
    }

    pub(crate) fn detach_client(&self, cid: u64, sid: &str) {
        if let Some(e) = lock(&self.clients).get_mut(&cid) {
            e.attached.remove(sid);
        }
    }

    /// Remove every attachment to `sid` and return the senders that were attached.
    pub(crate) fn detach_all(&self, sid: &str) -> Vec<ClientTx> {
        let mut out = Vec::new();
        for e in lock(&self.clients).values_mut() {
            if e.attached.remove(sid).is_some() {
                out.push(e.tx.clone());
            }
        }
        out
    }

    /// Number of foreground attachments of `sid`.
    pub(crate) fn foreground_count(&self, sid: &str) -> u32 {
        lock(&self.clients).values().filter(|e| e.attached.get(sid) == Some(&true)).count() as u32
    }

    pub fn send_to(&self, cid: u64, msg: ServerMsg) {
        if let Some(c) = lock(&self.clients).get(&cid) {
            let _ = c.tx.send(msg);
        }
    }

    pub fn broadcast(&self, msg: ServerMsg) {
        for c in lock(&self.clients).values() {
            let _ = c.tx.send(msg.clone());
        }
    }

    pub fn toast_all(&self, level: ToastLevel, message: impl Into<String>) {
        self.broadcast(ServerMsg::Toast { level, message: message.into() });
    }

    /// Fan a session event out to attached clients (and background watchers).
    /// Called with the session state locked so events and snapshots stay ordered.
    pub(crate) fn emit(&self, sid: &str, background: bool, event: &Event) {
        let clients = lock(&self.clients);
        for c in clients.values() {
            if c.attached.contains_key(sid) || (background && c.watch_bg) {
                let _ = c.tx.send(ServerMsg::Event { session: sid.to_string(), event: event.clone() });
            }
        }
    }

    pub(crate) fn toast_attached(&self, sid: &str, level: ToastLevel, message: &str) {
        let clients = lock(&self.clients);
        for c in clients.values() {
            if c.attached.contains_key(sid) {
                let _ = c.tx.send(ServerMsg::Toast { level, message: message.to_string() });
            }
        }
    }

    /// Schedule a coalesced `Sessions` push.
    pub fn touch(&self) {
        self.sessions_changed.notify_one();
    }

    // --- config --------------------------------------------------------------

    /// Merged config for a directory (cached for a few seconds, then reloaded).
    pub async fn config(&self, cwd: &str) -> Arc<Config> {
        let key = PathBuf::from(cwd);
        if let Some((at, c)) = lock(&self.configs).get(&key) {
            if at.elapsed() < CONFIG_TTL {
                return c.clone();
            }
        }
        let k2 = key.clone();
        let loaded = tokio::task::spawn_blocking(move || Config::load(&k2)).await;
        let cfg = match loaded {
            Ok(Ok(c)) => Arc::new(c),
            Ok(Err(e)) => {
                tracing::warn!("config load for {cwd} failed: {e:#}");
                fallback_config()
            }
            Err(e) => {
                tracing::error!("config load panicked: {e}");
                fallback_config()
            }
        };
        for w in &cfg.warnings {
            tracing::debug!("config warning ({cwd}): {w}");
        }
        lock(&self.configs).insert(key, (Instant::now(), cfg.clone()));
        cfg
    }

    // --- sessions ------------------------------------------------------------

    pub fn get(&self, id: &str) -> Option<Arc<Session>> {
        lock(&self.sessions).get(id).cloned()
    }

    pub fn all_sessions(&self) -> Vec<Arc<Session>> {
        lock(&self.sessions).values().cloned().collect()
    }

    pub fn open_list(&self) -> Vec<SessionInfo> {
        let mut v: Vec<SessionInfo> = self.all_sessions().iter().map(|s| s.info()).collect();
        v.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        v
    }

    /// All persisted sessions (+ open ones), newest first.
    pub fn history_list(&self) -> Vec<SessionInfo> {
        let open: HashMap<String, SessionInfo> = self.open_list().into_iter().map(|i| (i.id.clone(), i)).collect();
        let mut v: Vec<SessionInfo> = crate::persist::list_infos()
            .into_iter()
            .map(|i| open.get(&i.id).cloned().unwrap_or(i))
            .collect();
        for (id, i) in &open {
            if !v.iter().any(|x| &x.id == id) {
                v.push(i.clone());
            }
        }
        v.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        v
    }

    pub(crate) fn remove_session(&self, id: &str) -> Option<Arc<Session>> {
        lock(&self.sessions).remove(id)
    }

    /// Get an open session or load it from disk.
    pub async fn open_session(self: &Arc<Self>, id: &str) -> anyhow::Result<Arc<Session>> {
        if let Some(s) = self.get(id) {
            return Ok(s);
        }
        let _g = self.open_lock.lock().await;
        if let Some(s) = self.get(id) {
            return Ok(s);
        }
        anyhow::ensure!(crate::persist::valid_id(id), "unknown session {id:?}");
        let id2 = id.to_string();
        let (info, msgs, meta) = tokio::task::spawn_blocking(move || crate::persist::load_full(&id2)).await??;
        let s = Session::new(self, info, msgs, meta);
        s.repair_after_load();
        lock(&self.sessions).insert(id.to_string(), s.clone());
        self.touch();
        Ok(s)
    }

    /// Unique display name among open sessions: "x", "x (2)", "x (3)"...
    pub fn unique_name(&self, base: &str) -> String {
        let names: Vec<String> = self.all_sessions().iter().map(|s| s.info().name).collect();
        if !names.iter().any(|n| n == base) {
            return base.to_string();
        }
        (2..).map(|i| format!("{base} ({i})")).find(|n| !names.contains(n)).unwrap_or_else(|| base.to_string())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_session(
        self: &Arc<Self>,
        cwd: &str,
        agent: Option<String>,
        model: Option<ModelRef>,
        name: Option<String>,
        parent: Option<String>,
        must_report: bool,
    ) -> anyhow::Result<Arc<Session>> {
        let cwd_path = Path::new(cwd);
        anyhow::ensure!(cwd_path.is_absolute(), "cwd must be an absolute path: {cwd}");
        let cwd_path = std::fs::canonicalize(cwd_path).map_err(|e| anyhow::anyhow!("cwd {cwd}: {e}"))?;
        anyhow::ensure!(cwd_path.is_dir(), "cwd is not a directory: {}", cwd_path.display());
        let cwd = cwd_path.to_string_lossy().into_owned();
        let config = self.config(&cwd).await;
        let agent = agent.filter(|a| !a.trim().is_empty()).unwrap_or_else(|| config.default_agent.clone());
        let agent = if config.agent(&agent).is_none() && agent == config.default_agent && config.agents.contains_key("build") {
            "build".to_string()
        } else {
            agent
        };
        match config.agent(&agent) {
            Some(a) if !a.disable.unwrap_or(false) => {}
            _ => anyhow::bail!(
                "unknown agent {agent:?} (available: {})",
                config.agent_infos().iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")
            ),
        }
        let model = match model {
            Some(m) => m,
            None => config
                .default_model(&agent)
                .ok_or_else(|| anyhow::anyhow!("no model configured (set `model` in opencode.json)"))?,
        };
        let explicit = name.as_ref().is_some_and(|n| !n.trim().is_empty());
        let base = match &name {
            Some(n) if !n.trim().is_empty() => n.trim().to_string(),
            _ => format!("session-{}", util::rand_suffix(4)),
        };
        let name = self.unique_name(&base);
        let now = now_ms();
        let mut info = SessionInfo {
            id: util::new_id("ses"),
            name,
            agent,
            model: model.clone(),
            cwd,
            created_at: now,
            updated_at: now,
            parent,
            must_report,
            open: true,
            ..Default::default()
        };
        if let Ok(spec) = config.resolve_model(&model) {
            let (max, compact_at) = config.context_limits(&spec, &model);
            info.context.max = max;
            info.context.compact_at = compact_at;
        }
        let meta = SessionMeta { name_explicit: explicit, titled: explicit, reported: false };
        let s = Session::new(self, info, Vec::new(), meta);
        lock(&self.sessions).insert(s.id.clone(), s.clone());
        s.mark_dirty();
        self.touch();
        Ok(s)
    }

    /// Resolve an open session by exact id, else exact (trimmed) name.
    pub fn resolve_open(&self, target: &str) -> Result<Arc<Session>, String> {
        let t = target.trim();
        if let Some(s) = self.get(t) {
            return Ok(s);
        }
        let all = self.all_sessions();
        let matches: Vec<&Arc<Session>> = all.iter().filter(|s| s.info().name.trim() == t).collect();
        match matches.len() {
            1 => Ok(matches[0].clone()),
            0 => {
                let mut names: Vec<String> =
                    all.iter().map(|s| format!("\"{}\" ({})", s.info().name, s.id)).collect();
                names.sort();
                Err(format!(
                    "no open session named or with id \"{t}\". Open sessions: {}",
                    if names.is_empty() { "(none)".to_string() } else { names.join(", ") }
                ))
            }
            _ => Err(format!(
                "\"{t}\" is ambiguous. Candidates: {}. Use the session id.",
                matches.iter().map(|s| format!("\"{}\" (id {})", s.info().name, s.id)).collect::<Vec<_>>().join(", ")
            )),
        }
    }

    /// Flush everything to disk; called once on exit.
    pub async fn shutdown(&self) {
        if self.shutting_down.swap(true, Ordering::SeqCst) {
            return;
        }
        for s in self.all_sessions() {
            s.shutdown_flush();
        }
        let _ = tokio::time::timeout(Duration::from_secs(5), self.mcp.shutdown_all()).await;
    }
}
