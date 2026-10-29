//! Global backend state: connected clients, open sessions, history index, config cache,
//! MCP managers and session-list subscriptions.
//!
//! Locking rules (all locks are `parking_lot` and are never held across `.await`):
//! - `clients`, `sessions`, `history`, `configs`, `mcp` are leaf locks: while holding one of
//!   them no other lock is acquired.
//! - A session's locks may be held while calling into the hub's leaf locks (e.g. broadcasting).
//! - `names` (session-name uniqueness) is taken first, before any session lock.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Mutex, RwLock};
use ref_config::{Config, ConfigPaths};
use ref_mcp::McpManager;
use ref_proto::{Catalog, InstanceInfo, McpStatus, ServerMsg, SessionId, SessionInfo};
use tokio::sync::{mpsc, Notify, OnceCell};
use tokio_util::sync::CancellationToken;

use crate::session::{store, SessionCore};

pub type ClientId = String;

/// Outbound queue capacity per client; a client that falls this far behind is disconnected
/// (it will reconnect and re-attach, receiving fresh snapshots).
pub const CLIENT_QUEUE: usize = 16_384;
const CONFIG_TTL: Duration = Duration::from_secs(5);
const LIST_DEBOUNCE: Duration = Duration::from_millis(100);
/// Max history entries in a SessionList.
const LIST_LIMIT: usize = 1000;

pub struct Client {
    pub id: ClientId,
    tx: mpsc::Sender<Arc<str>>,
    pub cwd: String,
    pub attached: HashSet<SessionId>,
    pub watching: HashSet<SessionId>,
    pub list_sub: bool,
    /// Cancelled to force-disconnect the client.
    pub kill: CancellationToken,
}

pub struct Hub {
    pub instance: InstanceInfo,
    pub token: String,
    pub data_dir: PathBuf,
    pub sessions_dir: PathBuf,
    pub http: reqwest::Client,
    /// Cancelled when the daemon shuts down.
    pub shutdown: CancellationToken,
    config_paths: Option<ConfigPaths>,
    clients: RwLock<HashMap<ClientId, Client>>,
    sessions: Mutex<HashMap<SessionId, Arc<SessionCore>>>,
    /// Non-open sessions known from disk.
    history: Mutex<HashMap<SessionId, SessionInfo>>,
    /// Serializes name allocation + session open/rename.
    pub(crate) names: Mutex<()>,
    configs: Mutex<HashMap<String, (Instant, Arc<Config>)>>,
    mcp: Mutex<HashMap<String, Arc<OnceCell<Arc<McpManager>>>>>,
    list_dirty: Notify,
    pub ever_connected: AtomicBool,
    last_client_change: Mutex<Instant>,
    client_seq: AtomicU64,
}

pub struct HubOpts {
    pub instance: InstanceInfo,
    pub token: String,
    pub data_dir: PathBuf,
    pub config_paths: Option<ConfigPaths>,
}

impl Hub {
    pub fn new(opts: HubOpts) -> Arc<Hub> {
        let sessions_dir = opts.data_dir.join("sessions");
        if let Err(e) = std::fs::create_dir_all(&sessions_dir) {
            tracing::warn!("cannot create {}: {e}", sessions_dir.display());
        }
        let history: HashMap<SessionId, SessionInfo> =
            store::list_metas(&sessions_dir).into_iter().map(|m| (m.id.clone(), m.to_info())).collect();
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .pool_idle_timeout(Duration::from_secs(90))
            .build()
            .unwrap_or_default();
        let hub = Arc::new(Hub {
            instance: opts.instance,
            token: opts.token,
            data_dir: opts.data_dir,
            sessions_dir,
            http,
            shutdown: CancellationToken::new(),
            config_paths: opts.config_paths,
            clients: RwLock::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
            history: Mutex::new(history),
            names: Mutex::new(()),
            configs: Mutex::new(HashMap::new()),
            mcp: Mutex::new(HashMap::new()),
            list_dirty: Notify::new(),
            ever_connected: AtomicBool::new(false),
            last_client_change: Mutex::new(Instant::now()),
            client_seq: AtomicU64::new(0),
        });
        tokio::spawn(list_task(Arc::downgrade(&hub)));
        hub
    }

    // ───────────────────────── clients ─────────────────────────

    pub fn register_client(&self, cwd: String) -> (ClientId, mpsc::Receiver<Arc<str>>, CancellationToken) {
        let n = self.client_seq.fetch_add(1, Ordering::Relaxed) + 1;
        let id = format!("cli_{n}");
        let (tx, rx) = mpsc::channel(CLIENT_QUEUE);
        let kill = CancellationToken::new();
        let client = Client {
            id: id.clone(),
            tx,
            cwd,
            attached: HashSet::new(),
            watching: HashSet::new(),
            list_sub: false,
            kill: kill.clone(),
        };
        self.clients.write().insert(id.clone(), client);
        self.ever_connected.store(true, Ordering::Relaxed);
        *self.last_client_change.lock() = Instant::now();
        (id, rx, kill)
    }

    /// Remove a client and detach it from all sessions.
    pub fn unregister_client(&self, id: &str) {
        let removed = self.clients.write().remove(id);
        *self.last_client_change.lock() = Instant::now();
        if let Some(c) = removed {
            for sid in c.attached {
                self.refresh_attached(&sid);
            }
            self.mark_list_dirty();
        }
    }

    pub fn client_count(&self) -> usize {
        self.clients.read().len()
    }

    pub fn since_last_client_change(&self) -> Duration {
        self.last_client_change.lock().elapsed()
    }

    pub fn client_cwd(&self, id: &str) -> Option<String> {
        self.clients.read().get(id).map(|c| c.cwd.clone())
    }

    pub fn set_client_cwd(&self, id: &str, cwd: String) {
        if let Some(c) = self.clients.write().get_mut(id) {
            c.cwd = cwd;
        }
    }

    pub fn subscribe_list(&self, id: &str) {
        if let Some(c) = self.clients.write().get_mut(id) {
            c.list_sub = true;
        }
    }

    /// Mark `sid` attached (`attach=true`) or detached for a client. Returns whether it changed.
    pub fn set_attached(&self, cid: &str, sid: &str, attach: bool) -> bool {
        let changed = {
            let mut clients = self.clients.write();
            match clients.get_mut(cid) {
                Some(c) if attach => c.attached.insert(sid.to_string()),
                Some(c) => c.attached.remove(sid),
                None => false,
            }
        };
        if changed {
            self.refresh_attached(sid);
            self.mark_list_dirty();
        }
        changed
    }

    pub fn set_watching(&self, cid: &str, sid: &str, watch: bool) {
        if let Some(c) = self.clients.write().get_mut(cid) {
            if watch {
                c.watching.insert(sid.to_string());
            } else {
                c.watching.remove(sid);
            }
        }
    }

    /// Remove a session from every client's attached/watching sets.
    pub fn forget_session_subs(&self, sid: &str) {
        let mut clients = self.clients.write();
        for c in clients.values_mut() {
            c.attached.remove(sid);
            c.watching.remove(sid);
        }
    }

    pub fn attached_count(&self, sid: &str) -> u32 {
        self.clients.read().values().filter(|c| c.attached.contains(sid)).count() as u32
    }

    /// Whether any client attaches or watches the session.
    pub fn has_viewers(&self, sid: &str) -> bool {
        self.clients.read().values().any(|c| c.attached.contains(sid) || c.watching.contains(sid))
    }

    /// Clients attached to or watching the session.
    pub fn viewers(&self, sid: &str) -> Vec<ClientId> {
        self.clients
            .read()
            .values()
            .filter(|c| c.attached.contains(sid) || c.watching.contains(sid))
            .map(|c| c.id.clone())
            .collect()
    }

    fn refresh_attached(&self, sid: &str) {
        if let Some(core) = self.session(sid) {
            let n = self.attached_count(sid);
            core.set_attached(n);
        }
    }

    // ───────────────────────── sending ─────────────────────────

    fn encode(msg: &ServerMsg) -> Option<Arc<str>> {
        match serde_json::to_string(msg) {
            Ok(s) => Some(Arc::from(s)),
            Err(e) => {
                tracing::error!("failed to encode server message: {e}");
                None
            }
        }
    }

    fn deliver(c: &Client, payload: &Arc<str>) {
        if let Err(e) = c.tx.try_send(payload.clone()) {
            if matches!(e, mpsc::error::TrySendError::Full(_)) {
                tracing::warn!("client {} is too slow (queue full); disconnecting", c.id);
                c.kill.cancel();
            }
        }
    }

    pub fn send(&self, cid: &str, msg: &ServerMsg) {
        let Some(p) = Self::encode(msg) else { return };
        if let Some(c) = self.clients.read().get(cid) {
            Self::deliver(c, &p);
        }
    }

    /// Send to every client attached to or watching the session.
    pub fn broadcast_session(&self, sid: &str, msg: &ServerMsg) {
        let clients = self.clients.read();
        let mut payload = None;
        for c in clients.values() {
            if c.attached.contains(sid) || c.watching.contains(sid) {
                if payload.is_none() {
                    payload = Self::encode(msg);
                }
                if let Some(p) = &payload {
                    Self::deliver(c, p);
                }
            }
        }
    }

    pub fn broadcast_all(&self, msg: &ServerMsg) {
        let Some(p) = Self::encode(msg) else { return };
        for c in self.clients.read().values() {
            Self::deliver(c, &p);
        }
    }

    /// Notice to viewers of a session.
    pub fn notice(&self, sid: &str, level: ref_proto::NoticeLevel, text: impl Into<String>) {
        self.broadcast_session(sid, &ServerMsg::Notice { session_id: Some(sid.to_string()), level, text: text.into() });
    }

    // ───────────────────────── sessions ─────────────────────────

    pub fn session(&self, id: &str) -> Option<Arc<SessionCore>> {
        self.sessions.lock().get(id).cloned()
    }

    pub fn open_sessions(&self) -> Vec<Arc<SessionCore>> {
        let mut v: Vec<Arc<SessionCore>> = self.sessions.lock().values().cloned().collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    }

    pub fn open_count(&self) -> usize {
        self.sessions.lock().len()
    }

    pub(crate) fn insert_session(&self, core: Arc<SessionCore>) {
        self.history.lock().remove(&core.id);
        self.sessions.lock().insert(core.id.clone(), core);
        self.mark_list_dirty();
    }

    /// Remove from the open map and record it in history.
    pub(crate) fn remove_session(&self, id: &str, info: Option<SessionInfo>) -> Option<Arc<SessionCore>> {
        let core = self.sessions.lock().remove(id);
        if let Some(mut info) = info {
            info.open = false;
            info.attached = 0;
            info.status = ref_proto::SessionStatus::Idle;
            info.pending_permissions = 0;
            self.history.lock().insert(id.to_string(), info);
        }
        self.mark_list_dirty();
        core
    }

    pub fn history_info(&self, id: &str) -> Option<SessionInfo> {
        self.history.lock().get(id).cloned()
    }

    pub fn history_all(&self) -> Vec<SessionInfo> {
        self.history.lock().values().cloned().collect()
    }

    pub(crate) fn forget_history(&self, id: &str) -> bool {
        let r = self.history.lock().remove(id).is_some();
        self.mark_list_dirty();
        r
    }


    /// Infos of all open sessions.
    pub fn open_infos(&self) -> Vec<SessionInfo> {
        self.open_sessions().iter().map(|s| s.info()).collect()
    }

    /// Open sessions + history, most recently updated first.
    pub fn session_list(&self) -> Vec<SessionInfo> {
        let mut open = self.open_infos();
        let open_ids: HashSet<String> = open.iter().map(|s| s.id.clone()).collect();
        let mut hist: Vec<SessionInfo> =
            self.history_all().into_iter().filter(|h| !open_ids.contains(&h.id)).collect();
        hist.sort_by_key(|s| std::cmp::Reverse(s.updated));
        hist.truncate(LIST_LIMIT);
        open.extend(hist);
        open.sort_by_key(|s| std::cmp::Reverse(s.updated));
        open
    }

    pub fn mark_list_dirty(&self) {
        self.list_dirty.notify_one();
    }

    fn push_list(&self) {
        let subs: Vec<ClientId> =
            self.clients.read().values().filter(|c| c.list_sub).map(|c| c.id.clone()).collect();
        if subs.is_empty() {
            return;
        }
        let msg = ServerMsg::SessionList { req: 0, sessions: self.session_list() };
        let Some(p) = Self::encode(&msg) else { return };
        let clients = self.clients.read();
        for id in subs {
            if let Some(c) = clients.get(&id) {
                Self::deliver(c, &p);
            }
        }
    }

    // ───────────────────────── config / MCP ─────────────────────────

    /// Config for a working directory (cached for 5s).
    pub fn config(&self, cwd: &str) -> anyhow::Result<Arc<Config>> {
        if let Some((t, c)) = self.configs.lock().get(cwd) {
            if t.elapsed() < CONFIG_TTL {
                return Ok(c.clone());
            }
        }
        let path = Path::new(cwd);
        let cfg = match &self.config_paths {
            Some(p) => Config::load_with(p, path)?,
            None => Config::load(path)?,
        };
        let cfg = Arc::new(cfg);
        self.configs.lock().insert(cwd.to_string(), (Instant::now(), cfg.clone()));
        Ok(cfg)
    }

    pub fn clear_config_cache(&self) {
        self.configs.lock().clear();
    }

    fn mcp_key(cfg: &Config) -> String {
        serde_json::to_string(&cfg.mcp).unwrap_or_default()
    }

    /// The MCP manager for a config (started on first use; shared by identical MCP configs).
    pub async fn mcp_for(self: &Arc<Self>, cfg: &Config) -> Option<Arc<McpManager>> {
        if cfg.mcp.is_empty() {
            return None;
        }
        let key = Self::mcp_key(cfg);
        let cell = self.mcp.lock().entry(key).or_default().clone();
        if let Some(m) = cell.get() {
            return Some(m.clone());
        }
        let servers = cfg.mcp.clone();
        let mut fresh = false;
        let m = cell
            .get_or_init(|| async {
                fresh = true;
                Arc::new(McpManager::start(servers).await)
            })
            .await
            .clone();
        if fresh {
            let hub = self.clone();
            tokio::spawn(async move { hub.refresh_catalogs() });
        }
        Some(m)
    }

    /// MCP status without blocking: starts the manager in the background if needed.
    pub fn mcp_status(self: &Arc<Self>, cfg: &Arc<Config>) -> Vec<McpStatus> {
        if cfg.mcp.is_empty() {
            return vec![];
        }
        let key = Self::mcp_key(cfg);
        let cell = self.mcp.lock().entry(key).or_default().clone();
        if let Some(m) = cell.get() {
            return m.status();
        }
        let hub = self.clone();
        let cfg2 = cfg.clone();
        tokio::spawn(async move {
            hub.mcp_for(&cfg2).await;
        });
        cfg.mcp
            .iter()
            .map(|(name, c)| {
                let enabled = match c {
                    ref_proto::spec::McpServerConfig::Local { enabled, .. }
                    | ref_proto::spec::McpServerConfig::Remote { enabled, .. } => enabled.unwrap_or(true),
                };
                McpStatus {
                    name: name.clone(),
                    status: if enabled { "connecting" } else { "disabled" }.into(),
                    tools: 0,
                    error: None,
                }
            })
            .collect()
    }

    pub fn catalog(self: &Arc<Self>, cwd: &str) -> Catalog {
        match self.config(cwd) {
            Ok(cfg) => Catalog {
                models: cfg.models(),
                agents: cfg
                    .agent_entries()
                    .into_iter()
                    .filter(|a| !cfg.agents.get(&a.name).is_some_and(|c| c.hidden))
                    .collect(),
                default_agent: cfg.default_agent.clone(),
                mcp: self.mcp_status(&cfg),
                commands: cfg.command_entries(),
            },
            Err(e) => {
                tracing::warn!("config error for {cwd}: {e:#}");
                Catalog { models: vec![], agents: vec![], default_agent: "build".into(), mcp: vec![], commands: vec![] }
            }
        }
    }

    /// Send a fresh Catalog to every client (computed for its cwd).
    pub fn refresh_catalogs(self: &Arc<Self>) {
        let targets: Vec<(ClientId, String)> =
            self.clients.read().values().map(|c| (c.id.clone(), c.cwd.clone())).collect();
        let mut cache: HashMap<String, Catalog> = HashMap::new();
        for (id, cwd) in targets {
            let cat = cache.entry(cwd.clone()).or_insert_with(|| self.catalog(&cwd)).clone();
            self.send(&id, &ServerMsg::Catalog { catalog: cat });
        }
    }

    /// Reload configuration: clear caches, restart MCP managers, recompute prompts, push catalogs.
    pub async fn reload(self: &Arc<Self>) {
        self.clear_config_cache();
        let sessions = self.open_sessions();
        // Configs now in use (open sessions and clients).
        let mut cwds: HashSet<String> = sessions.iter().map(|s| s.cwd()).collect();
        cwds.extend(self.clients.read().values().map(|c| c.cwd.clone()));
        let mut wanted: HashSet<String> = HashSet::new();
        for cwd in &cwds {
            if let Ok(cfg) = self.config(cwd) {
                if !cfg.mcp.is_empty() {
                    wanted.insert(Self::mcp_key(&cfg));
                }
            }
        }
        let cells: Vec<(String, Arc<OnceCell<Arc<McpManager>>>)> =
            self.mcp.lock().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        for (key, cell) in cells {
            let Some(m) = cell.get().cloned() else { continue };
            if wanted.contains(&key) {
                // Same config: reconnect failed servers.
                if let Ok(servers) = serde_json::from_str(&key) {
                    m.restart(servers).await;
                }
            } else {
                self.mcp.lock().remove(&key);
                m.shutdown().await;
            }
        }
        for s in &sessions {
            s.invalidate_system_prompt();
            s.refresh_context_max();
            s.wake();
        }
        self.refresh_catalogs();
    }

    pub async fn shutdown_mcp(&self) {
        let managers: Vec<Arc<McpManager>> =
            self.mcp.lock().values().filter_map(|c| c.get().cloned()).collect();
        futures::future::join_all(managers.iter().map(|m| m.shutdown())).await;
    }
}

async fn list_task(hub: std::sync::Weak<Hub>) {
    loop {
        let notified = {
            let Some(h) = hub.upgrade() else { return };
            // Wait without holding a strong reference forever.
            let h2 = h.clone();
            drop(h);
            tokio::select! {
                _ = h2.list_dirty.notified() => true,
                _ = h2.shutdown.cancelled() => false,
            }
        };
        if !notified {
            return;
        }
        tokio::time::sleep(LIST_DEBOUNCE).await;
        let Some(h) = hub.upgrade() else { return };
        h.push_list();
    }
}
