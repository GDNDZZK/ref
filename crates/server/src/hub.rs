//! Shared server state: connected frontends, open sessions, config cache, MCP clients.
//!
//! Lock order (never acquire in the opposite direction):
//!   session state (`Session::st`) -> `Hub::clients`
//!   `Hub::accepting` -> `Hub::clients`
//! `Hub::sessions` and `Hub::configs` are only held to clone/insert entries.
//!
//! Idle shutdown: `accepting` makes "register a new connection" and "commit to shutting
//! down" mutually exclusive. A connection is registered before the WebSocket upgrade
//! response is sent, and the shutdown is only committed when no frontend and no
//! connection that has not said Hello yet is registered. Once committed, new connections
//! are refused (HTTP 503) so clients look for (or spawn) another backend.

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

/// Outgoing frames, already serialized: a fanned-out event is encoded once, not per client.
pub type Frame = Arc<str>;
pub type ClientTx = mpsc::UnboundedSender<Frame>;

pub fn encode(msg: &ServerMsg) -> Frame {
    match serde_json::to_string(msg) {
        Ok(s) => Arc::from(s),
        Err(e) => {
            tracing::error!("cannot serialize server message: {e}");
            Arc::from(r#"{"type":"toast","level":"error","message":"internal serialization error"}"#)
        }
    }
}

pub struct ClientEntry {
    pub tx: ClientTx,
    pub cwd: String,
    pub watch_bg: bool,
    /// Sent Hello as a real frontend (not a `--status` probe): counts for the idle timer.
    pub frontend: bool,
    /// session id -> foreground
    pub attached: HashMap<String, bool>,
    /// Hello received (frontend or probe).
    pub helloed: bool,
    pub connected_at: Instant,
}

/// A connection that has not sent Hello within this time no longer keeps the server alive.
const HELLO_GRACE: Duration = Duration::from_secs(10);

impl ClientEntry {
    /// Keeps an idle server from committing its shutdown: a frontend, or a fresh connection
    /// that may still turn out to be one.
    fn keeps_alive(&self) -> bool {
        self.frontend || (!self.helloed && self.connected_at.elapsed() < HELLO_GRACE)
    }
}

pub struct Hub {
    pub http: reqwest::Client,
    pub mcp: McpManager,
    pub server_cwd: String,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    clients: Mutex<HashMap<u64, ClientEntry>>,
    next_client: AtomicU64,
    configs: Mutex<HashMap<PathBuf, (Instant, Arc<Config>)>>,
    /// In-flight config loads (single-flight per directory).
    config_loads: Mutex<HashMap<PathBuf, ConfigLoad>>,
    sessions_changed: Arc<Notify>,
    client_count: watch::Sender<usize>,
    /// False once the shutdown is committed: new connections are refused.
    accepting: Mutex<bool>,
    shutting_down: AtomicBool,
    /// Per-session open locks: a session is never loaded twice, and loading one session
    /// does not delay opening another.
    opening: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
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
/// How long a caller waits for a config reload when a previous config is available.
const STALE_WAIT: Duration = Duration::from_secs(1);

type ConfigLoad = watch::Receiver<Option<Arc<Config>>>;

fn load_config_blocking(key: &Path) -> Arc<Config> {
    let start = Instant::now();
    let cwd = key.display();
    let cfg = match std::panic::catch_unwind(|| Config::load(key)) {
        Ok(Ok(c)) => Arc::new(c),
        Ok(Err(e)) => {
            tracing::warn!("config load for {cwd} failed: {e:#}");
            fallback_config()
        }
        Err(_) => {
            tracing::error!("config load for {cwd} panicked");
            fallback_config()
        }
    };
    for w in &cfg.warnings {
        tracing::debug!("config warning ({cwd}): {w}");
    }
    let took = start.elapsed();
    if took >= Duration::from_secs(1) {
        tracing::debug!("config load for {cwd} took {:.1}s", took.as_secs_f64());
    }
    cfg
}

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
            config_loads: Mutex::new(HashMap::new()),
            sessions_changed: Arc::new(Notify::new()),
            client_count: count_tx,
            accepting: Mutex::new(true),
            shutting_down: AtomicBool::new(false),
            opening: Mutex::new(HashMap::new()),
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

    /// Register a new connection. Returns None once the shutdown is committed.
    pub fn add_client(&self, tx: ClientTx) -> Option<u64> {
        let accepting = lock(&self.accepting);
        if !*accepting {
            return None;
        }
        let id = self.next_client.fetch_add(1, Ordering::Relaxed);
        let mut c = lock(&self.clients);
        c.insert(
            id,
            ClientEntry {
                tx,
                cwd: String::new(),
                watch_bg: false,
                frontend: false,
                attached: HashMap::new(),
                helloed: false,
                connected_at: Instant::now(),
            },
        );
        self.publish_counts(&c);
        Some(id)
    }

    fn frontends(c: &HashMap<u64, ClientEntry>) -> usize {
        c.values().filter(|e| e.frontend).count()
    }

    fn publish_counts(&self, c: &HashMap<u64, ClientEntry>) {
        self.client_count.send_replace(Self::frontends(c));
    }

    pub fn remove_client(&self, id: u64) {
        let entry = {
            let mut c = lock(&self.clients);
            let e = c.remove(&id);
            self.publish_counts(&c);
            e
        };
        if let Some(e) = entry {
            for (sid, _) in e.attached {
                if let Some(s) = self.get(&sid) {
                    s.recount_attached(self);
                }
            }
        }
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
        let mut c = lock(&self.clients);
        if let Some(e) = c.get_mut(&id) {
            e.cwd = cwd.to_string();
            e.frontend = frontend;
            e.helloed = true;
        }
        self.publish_counts(&c);
    }

    /// Commit to shutting down: from now on new connections are refused. Unless `force`,
    /// this fails (and the server keeps running) while a frontend, or a connection that has
    /// not said Hello yet, is registered.
    pub fn commit_shutdown(&self, force: bool) -> bool {
        let mut accepting = lock(&self.accepting);
        if !*accepting {
            return true;
        }
        if !force && lock(&self.clients).values().any(|e| e.keeps_alive()) {
            return false;
        }
        *accepting = false;
        true
    }

    /// Write every session's pending changes to disk (non-destructive: turns keep running).
    pub async fn flush_all(&self) {
        let sessions = self.all_sessions();
        let _ = tokio::task::spawn_blocking(move || {
            for s in sessions {
                s.flush_now();
            }
        })
        .await;
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
        // Encode outside the lock: snapshots can be large.
        let frame = encode(&msg);
        if let Some(c) = lock(&self.clients).get(&cid) {
            let _ = c.tx.send(frame);
        }
    }

    pub fn broadcast(&self, msg: ServerMsg) {
        let frame = encode(&msg);
        for c in lock(&self.clients).values() {
            let _ = c.tx.send(frame.clone());
        }
    }

    pub fn toast_all(&self, level: ToastLevel, message: impl Into<String>) {
        self.broadcast(ServerMsg::Toast { level, message: message.into() });
    }

    /// Fan a session event out to attached clients (and background watchers).
    /// Called with the session state locked so events and snapshots stay ordered.
    pub(crate) fn emit(&self, sid: &str, background: bool, event: &Event) {
        let clients = lock(&self.clients);
        let mut frame: Option<Frame> = None;
        for c in clients.values() {
            if c.attached.contains_key(sid) || (background && c.watch_bg) {
                let f = frame.get_or_insert_with(|| encode(&ServerMsg::Event { session: sid.to_string(), event: event.clone() }));
                let _ = c.tx.send(f.clone());
            }
        }
    }

    pub(crate) fn toast_attached(&self, sid: &str, level: ToastLevel, message: &str) {
        let clients = lock(&self.clients);
        let frame = encode(&ServerMsg::Toast { level, message: message.to_string() });
        for c in clients.values() {
            if c.attached.contains_key(sid) {
                let _ = c.tx.send(frame.clone());
            }
        }
    }

    /// Schedule a coalesced `Sessions` push.
    pub fn touch(&self) {
        self.sessions_changed.notify_one();
    }

    // --- config --------------------------------------------------------------

    /// Merged config for a directory (cached for a few seconds, then reloaded).
    ///
    /// Loads run on the blocking pool, one at a time per directory; the load task stores the
    /// result in the cache itself. When a cached config exists, a reload that takes longer
    /// than [`STALE_WAIT`] does not hold the caller up: it gets the previous config.
    pub async fn config(self: &Arc<Self>, cwd: &str) -> Arc<Config> {
        let key = PathBuf::from(cwd);
        let stale = match lock(&self.configs).get(&key) {
            Some((at, c)) if at.elapsed() < CONFIG_TTL => return c.clone(),
            Some((_, c)) => Some(c.clone()),
            None => None,
        };
        let mut rx = {
            let mut loading = lock(&self.config_loads);
            loading.entry(key.clone()).or_insert_with(|| self.spawn_config_load(key.clone())).clone()
        };
        let wait = async {
            let _ = rx.wait_for(|c| c.is_some()).await;
            rx.borrow().clone()
        };
        let loaded = match &stale {
            Some(_) => tokio::time::timeout(STALE_WAIT, wait).await.ok().flatten(),
            None => wait.await,
        };
        match (loaded, stale) {
            (Some(cfg), _) => cfg,
            (None, Some(old)) => {
                tracing::debug!("config reload for {cwd} is slow; using the previous config");
                old
            }
            (None, None) => fallback_config(),
        }
    }

    fn spawn_config_load(self: &Arc<Self>, key: PathBuf) -> ConfigLoad {
        let (tx, rx) = watch::channel(None);
        let hub = Arc::downgrade(self);
        tokio::spawn(async move {
            let k2 = key.clone();
            let mut task = tokio::task::spawn_blocking(move || load_config_blocking(&k2));
            let cfg = tokio::select! {
                r = &mut task => r,
                _ = tokio::time::sleep(Duration::from_secs(5)) => {
                    tracing::warn!("config load for {} is taking more than 5s", key.display());
                    task.await
                }
            }
            .unwrap_or_else(|_| fallback_config());
            if let Some(h) = hub.upgrade() {
                lock(&h.configs).insert(key.clone(), (Instant::now(), cfg.clone()));
                lock(&h.config_loads).remove(&key);
            }
            let _ = tx.send(Some(cfg));
        });
        rx
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
        v.sort_by_key(|i| std::cmp::Reverse(i.updated_at));
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
        anyhow::ensure!(crate::persist::valid_id(id), "unknown session {id:?}");
        let m = lock(&self.opening).entry(id.to_string()).or_default().clone();
        let r = async {
            let _g = m.lock().await;
            if let Some(s) = self.get(id) {
                return Ok(s);
            }
            let id2 = id.to_string();
            let (info, msgs, meta) = tokio::task::spawn_blocking(move || crate::persist::load_full(&id2)).await??;
            let s = Session::new(self, info, msgs, meta);
            s.repair_after_load();
            lock(&self.sessions).insert(id.to_string(), s.clone());
            self.touch();
            self.warm_up(&s.info().cwd);
            Ok(s)
        }
        .await;
        let mut opening = lock(&self.opening);
        if opening.get(id).is_some_and(|x| Arc::ptr_eq(x, &m) && Arc::strong_count(x) <= 2) {
            opening.remove(id);
        }
        r
    }

    /// Start background work a session in `cwd` will need soon (MCP connections). Never waits.
    pub fn warm_up(self: &Arc<Self>, cwd: &str) {
        let hub = self.clone();
        let cwd = cwd.to_string();
        tokio::spawn(async move {
            let config = hub.config(&cwd).await;
            hub.mcp.start(&hub, &config, &cwd, false);
        });
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
        self.mcp.start(self, &config, &s.info().cwd, false);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn commit_shutdown_is_exclusive_with_new_connections() {
        let hub = Hub::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        let a = hub.add_client(tx.clone()).unwrap();
        // A connection that has not said Hello yet blocks the commit.
        assert!(!hub.commit_shutdown(false));
        hub.hello(a, "/", true);
        assert!(!hub.commit_shutdown(false), "a frontend blocks the commit");
        let probe = hub.add_client(tx.clone()).unwrap();
        hub.hello(probe, "/", false);
        hub.remove_client(a);
        // Only a status probe is left: it does not keep the server alive.
        assert!(hub.commit_shutdown(false));
        assert!(hub.add_client(tx.clone()).is_none(), "refused after the commit");
        assert!(hub.commit_shutdown(false), "idempotent");
    }

    #[tokio::test]
    async fn forced_commit_refuses_even_with_frontends() {
        let hub = Hub::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        let a = hub.add_client(tx.clone()).unwrap();
        hub.hello(a, "/", true);
        assert!(hub.commit_shutdown(true));
        assert!(hub.add_client(tx).is_none());
    }
}
