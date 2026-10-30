//! Hub: registry of live sessions and frontend connections, event routing, session-list
//! broadcasting (debounced) and the idle-timeout logic.

use crate::mcp::McpManager;
use crate::session::{Session, SessionState};
use crate::store::Store;
use ref_config::Config;
use ref_proto::{
    InstanceInfo, Message, NotifyKind, ServerFrame, SessionEvent, SessionInfo, SessionStatus,
};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

pub type ConfigLoader = Arc<dyn Fn(&Path) -> Config + Send + Sync>;

/// Max queued frames per connection before it is dropped as too slow.
pub const MAX_QUEUE: usize = 10_000;

#[derive(Clone)]
pub struct HubOptions {
    pub data_dir: PathBuf,
    /// Home dir for global instructions / skills discovery (None in tests).
    pub home: Option<PathBuf>,
    pub config_loader: ConfigLoader,
    /// Base delay of the LLM retry backoff (2s in production).
    pub retry_base: Duration,
    /// Max time to wait for MCP servers before the first request.
    pub mcp_wait: Duration,
    /// -1 never, 0 immediately, N seconds after the last frontend leaves.
    pub idle_timeout: i64,
}

impl HubOptions {
    pub fn new(data_dir: PathBuf) -> HubOptions {
        HubOptions {
            data_dir,
            home: dirs::home_dir(),
            config_loader: Arc::new(|cwd: &Path| Config::load(cwd)),
            retry_base: Duration::from_secs(2),
            mcp_wait: Duration::from_secs(30),
            idle_timeout: -1,
        }
    }
}

/// Outgoing frame queue of one connection (coalesces consecutive deltas).
#[derive(Default)]
pub struct OutQueue {
    inner: Mutex<VecDeque<ServerFrame>>,
    pub notify: Notify,
    pub overflow: AtomicBool,
}

impl OutQueue {
    pub fn push(&self, f: ServerFrame) {
        let mut q = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let ServerFrame::Event {
            session_id,
            event:
                SessionEvent::Delta {
                    message_id,
                    index,
                    delta,
                },
        } = &f
            && let Some(ServerFrame::Event {
                session_id: s2,
                event:
                    SessionEvent::Delta {
                        message_id: m2,
                        index: i2,
                        delta: d2,
                    },
            }) = q.back_mut()
            && s2 == session_id
            && m2 == message_id
            && i2 == index
        {
            use ref_proto::Delta as D;
            match (d2, delta) {
                (D::Text { text: a }, D::Text { text: b })
                | (D::Reasoning { text: a }, D::Reasoning { text: b })
                | (D::ToolInput { text: a }, D::ToolInput { text: b }) => {
                    a.push_str(b);
                    return;
                }
                _ => {}
            }
        }
        q.push_back(f);
        if q.len() > MAX_QUEUE {
            self.overflow.store(true, Ordering::SeqCst);
        }
        drop(q);
        self.notify.notify_one();
    }

    pub fn drain(&self) -> Vec<ServerFrame> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
            .collect()
    }

    /// True if every queued frame is a delta (worth waiting a bit to coalesce more).
    pub fn only_deltas(&self) -> bool {
        let q = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        !q.is_empty()
            && q.iter().all(|f| {
                matches!(
                    f,
                    ServerFrame::Event {
                        event: SessionEvent::Delta { .. },
                        ..
                    }
                )
            })
    }
}

#[derive(Default)]
pub struct ConnState {
    pub hello: bool,
    /// Counts for the idle timeout (false for `ref-server --status/--stop` control clients).
    pub frontend: bool,
    pub authed: bool,
    pub cwd: String,
    pub fg: HashSet<String>,
    pub watch: HashSet<String>,
    pub watch_all: bool,
    /// Notification kinds this connection wants (`Hello.notify`, `SetNotify`).
    pub notify: HashSet<NotifyKind>,
    /// The user's window is in the foreground (`SetFocus`).
    pub focused: bool,
    /// Sessions the window shows (`SetFocus`).
    pub shown: HashSet<String>,
    /// When the window last came to the front, as a hub stamp (see `Hub::next_stamp`); the
    /// connection time until the first `SetFocus`.
    pub focus_stamp: u64,
}

pub struct Conn {
    pub id: u64,
    pub out: OutQueue,
    pub st: Mutex<ConnState>,
    pub kill: CancellationToken,
}

impl Conn {
    pub fn lock(&self) -> MutexGuard<'_, ConnState> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn send(&self, f: ServerFrame) {
        self.out.push(f);
    }
}

/// Bookkeeping for `message_status`.
#[derive(Debug, Clone)]
pub struct PeerRecord {
    pub from_id: String,
    pub to_id: String,
    pub to_name: String,
    /// Epoch ms when sent.
    pub time: i64,
    /// First line of the message (for /peers-outbox).
    pub preview: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleAction {
    Nothing,
    ShutdownNow,
    ShutdownAfter(u64),
}

/// Pure idle-timeout decision, evaluated when the number of connected frontends changes.
pub fn idle_action(timeout: i64, ever_connected: bool, connected: usize) -> IdleAction {
    if !ever_connected || connected > 0 || timeout < 0 {
        IdleAction::Nothing
    } else if timeout == 0 {
        IdleAction::ShutdownNow
    } else {
        IdleAction::ShutdownAfter(timeout as u64)
    }
}

/// A cached file list for `files_cache`, with the time it was built.
pub type FileList = (Instant, Arc<Vec<String>>);

#[derive(Default)]
struct IdleState {
    ever: bool,
    generation: u64,
}

pub struct Hub {
    pub opts: HubOptions,
    pub store: Store,
    pub mcp: McpManager,
    sessions: Mutex<BTreeMap<String, Arc<Session>>>,
    conns: Mutex<HashMap<u64, Arc<Conn>>>,
    next_conn: AtomicU64,
    next_run: AtomicU64,
    next_stamp: AtomicU64,
    pub shutdown: CancellationToken,
    dirty: Arc<Notify>,
    idle: Mutex<IdleState>,
    pub peer_msgs: Mutex<HashMap<String, PeerRecord>>,
    pub files_cache: Mutex<HashMap<String, FileList>>,
    pub instance: Mutex<Option<InstanceInfo>>,
}

impl Hub {
    pub fn new(opts: HubOptions) -> Arc<Hub> {
        let hub = Arc::new(Hub {
            store: Store::new(opts.data_dir.clone()),
            opts,
            mcp: McpManager::default(),
            sessions: Mutex::new(BTreeMap::new()),
            conns: Mutex::new(HashMap::new()),
            next_conn: AtomicU64::new(1),
            next_run: AtomicU64::new(1),
            next_stamp: AtomicU64::new(1),
            shutdown: CancellationToken::new(),
            dirty: Arc::new(Notify::new()),
            idle: Mutex::new(IdleState::default()),
            peer_msgs: Mutex::new(HashMap::new()),
            files_cache: Mutex::new(HashMap::new()),
            instance: Mutex::new(None),
        });
        tokio::spawn(broadcaster(Arc::downgrade(&hub)));
        hub
    }

    pub fn load_config(&self, cwd: &Path) -> Config {
        (self.opts.config_loader)(cwd)
    }

    pub fn next_run_id(&self) -> u64 {
        self.next_run.fetch_add(1, Ordering::Relaxed)
    }

    /// A number larger than every earlier one: orders connection and focus times.
    pub fn next_stamp(&self) -> u64 {
        self.next_stamp.fetch_add(1, Ordering::Relaxed)
    }

    // ----- sessions -----

    pub fn session(&self, id: &str) -> Option<Arc<Session>> {
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .cloned()
    }

    pub fn sessions(&self) -> Vec<Arc<Session>> {
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect()
    }

    pub fn insert_session(&self, s: Arc<Session>) {
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(s.id.clone(), s);
        self.mark_dirty();
    }

    pub fn remove_session(&self, id: &str) -> Option<Arc<Session>> {
        let r = self
            .sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
        for c in self.conns() {
            let mut st = c.lock();
            st.fg.remove(id);
            st.watch.remove(id);
        }
        self.mark_dirty();
        r
    }

    pub fn infos(&self) -> Vec<SessionInfo> {
        let mut v: Vec<SessionInfo> = self.sessions().iter().map(|s| s.info()).collect();
        v.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        v
    }

    /// Whether a live session (other than `except`) has this exact name.
    pub fn name_taken(&self, name: &str, except: Option<&str>) -> bool {
        self.sessions()
            .iter()
            .any(|s| Some(s.id.as_str()) != except && s.lock().info.name == name)
    }

    // ----- events -----

    /// Route a session event to subscribed connections. Call while holding the session lock
    /// so that snapshots taken in `Attach` are consistent with the event stream.
    pub fn emit(&self, sid: &str, mut event: SessionEvent) {
        // Frontends get image stubs (see `ImageData`).
        match &mut event {
            SessionEvent::Message { message } => message.strip_image_data(),
            SessionEvent::Part {
                message_id,
                index,
                part,
            } => part.strip_image_data(message_id, *index),
            SessionEvent::Reset { messages, .. } | SessionEvent::Queue { messages } => {
                messages.iter_mut().for_each(Message::strip_image_data)
            }
            _ => {}
        }
        let conns = self.conns();
        let mut frame = Some(ServerFrame::Event {
            session_id: sid.to_string(),
            event,
        });
        let targets: Vec<&Arc<Conn>> = conns
            .iter()
            .filter(|c| {
                let st = c.lock();
                st.hello && (st.watch_all || st.fg.contains(sid) || st.watch.contains(sid))
            })
            .collect();
        let n = targets.len();
        for (i, c) in targets.into_iter().enumerate() {
            let f = if i + 1 == n {
                frame.take()
            } else {
                frame.clone()
            };
            if let Some(f) = f {
                c.send(f);
            }
        }
    }

    /// Emit an `Info` event and schedule a session-list broadcast.
    pub fn emit_info(&self, st: &SessionState) {
        self.emit(
            &st.info.id,
            SessionEvent::Info {
                info: st.info.clone(),
            },
        );
        self.mark_dirty();
    }

    pub fn set_status(&self, st: &mut SessionState, status: SessionStatus) {
        if st.info.status != status {
            st.info.status = status;
            self.emit_info(st);
        }
    }

    pub fn mark_dirty(&self) {
        self.dirty.notify_one();
    }

    pub fn toast(&self, level: ref_proto::ToastLevel, text: impl Into<String>) {
        let text = text.into();
        for c in self.conns() {
            if c.lock().hello {
                c.send(ServerFrame::Toast {
                    level,
                    text: text.clone(),
                });
            }
        }
    }

    // ----- connections -----

    pub fn conns(&self) -> Vec<Arc<Conn>> {
        self.conns
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect()
    }

    pub fn add_conn(&self) -> Arc<Conn> {
        let id = self.next_conn.fetch_add(1, Ordering::Relaxed);
        let c = Arc::new(Conn {
            id,
            out: OutQueue::default(),
            st: Mutex::new(ConnState::default()),
            kill: CancellationToken::new(),
        });
        self.conns
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, c.clone());
        c
    }

    fn frontend_count(&self) -> usize {
        self.conns().iter().filter(|c| c.lock().frontend).count()
    }

    /// Called after a connection completed Hello.
    pub fn frontend_joined(&self) {
        let mut idle = self.idle.lock().unwrap_or_else(|e| e.into_inner());
        idle.ever = true;
        idle.generation += 1;
    }

    pub fn remove_conn(self: &Arc<Self>, id: u64) {
        let Some(c) = self
            .conns
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id)
        else {
            return;
        };
        c.kill.cancel();
        let (fg, was_frontend) = {
            let mut st = c.lock();
            (st.fg.drain().collect::<Vec<String>>(), st.frontend)
        };
        for sid in fg {
            if let Some(s) = self.session(&sid) {
                let mut st = s.lock();
                st.info.attached = st.info.attached.saturating_sub(1);
                self.emit_info(&st);
            }
        }
        if was_frontend {
            self.check_idle();
        }
    }

    fn check_idle(self: &Arc<Self>) {
        let count = self.frontend_count();
        let mut idle = self.idle.lock().unwrap_or_else(|e| e.into_inner());
        match idle_action(self.opts.idle_timeout, idle.ever, count) {
            IdleAction::Nothing => {}
            IdleAction::ShutdownNow => {
                tracing::info!("last frontend left; idle timeout 0 => shutting down");
                self.shutdown.cancel();
            }
            IdleAction::ShutdownAfter(secs) => {
                idle.generation += 1;
                let generation = idle.generation;
                let hub = Arc::downgrade(self);
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(secs)).await;
                    let Some(hub) = hub.upgrade() else { return };
                    let still = hub
                        .idle
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .generation
                        == generation;
                    if still && hub.frontend_count() == 0 {
                        tracing::info!("idle timeout reached; shutting down");
                        hub.shutdown.cancel();
                    }
                });
            }
        }
    }
}

/// Debounced `Sessions` broadcast.
async fn broadcaster(hub: Weak<Hub>) {
    let mut last: Option<String> = None;
    loop {
        let (dirty, shutdown) = {
            let Some(h) = hub.upgrade() else { return };
            (h.dirty.clone(), h.shutdown.clone())
        };
        // The strong ref is dropped while waiting (notify_one stores a permit).
        tokio::select! {
            _ = dirty.notified() => {}
            _ = shutdown.cancelled() => return,
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        let Some(h) = hub.upgrade() else { return };
        let infos = h.infos();
        let key = serde_json::to_string(&infos).unwrap_or_default();
        if last.as_deref() == Some(key.as_str()) {
            continue;
        }
        last = Some(key);
        for c in h.conns() {
            if c.lock().hello {
                c.send(ServerFrame::Sessions {
                    sessions: infos.clone(),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_logic() {
        assert_eq!(idle_action(0, false, 0), IdleAction::Nothing);
        assert_eq!(idle_action(0, true, 0), IdleAction::ShutdownNow);
        assert_eq!(idle_action(0, true, 1), IdleAction::Nothing);
        assert_eq!(idle_action(-1, true, 0), IdleAction::Nothing);
        assert_eq!(idle_action(30, true, 0), IdleAction::ShutdownAfter(30));
        assert_eq!(idle_action(30, false, 0), IdleAction::Nothing);
    }

    #[test]
    fn delta_coalescing() {
        use ref_proto::Delta;
        let q = OutQueue::default();
        let d = |t: &str| ServerFrame::Event {
            session_id: "s".into(),
            event: SessionEvent::Delta {
                message_id: "m".into(),
                index: 0,
                delta: Delta::Text { text: t.into() },
            },
        };
        q.push(d("a"));
        q.push(d("b"));
        assert!(q.only_deltas());
        q.push(ServerFrame::Toast {
            level: ref_proto::ToastLevel::Info,
            text: "x".into(),
        });
        q.push(d("c"));
        let v = q.drain();
        assert_eq!(v.len(), 3);
        match &v[0] {
            ServerFrame::Event {
                event:
                    SessionEvent::Delta {
                        delta: Delta::Text { text },
                        ..
                    },
                ..
            } => {
                assert_eq!(text, "ab")
            }
            _ => panic!(),
        }
    }
}
