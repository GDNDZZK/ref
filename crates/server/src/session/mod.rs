//! A live session: in-memory state, attachments, input queue, persistence, runtime task.
//!
//! All state lives behind one short-held std mutex (`st`). Events are emitted while the lock
//! is held so every client sees changes in the same order as they were applied, and a
//! snapshot taken on attach is consistent with the events that follow it.

pub mod history;
pub mod tools;
pub mod turn;

use crate::hub::{lock, Hub};
use crate::persist::{self, SessionMeta};
use crate::util;
use hn_proto::*;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;
use tokio::sync::{oneshot, watch, Notify};
use tokio_util::sync::CancellationToken;

/// Something that should become a user message.
#[derive(Debug, Clone)]
pub enum Input {
    Human { text: String, images: Vec<ImageData> },
    Peer { from_id: String, from_name: String, from_cwd: String, text: String },
    Spawn { from_id: String, from_name: String, text: String, must_report: bool },
    System(String),
}

impl Input {
    pub fn into_message(self, agent: &str) -> Message {
        let now = now_ms();
        let (origin, text, images) = match self {
            Input::Human { text, images } => (Origin::Human, text, images),
            Input::Peer { from_id, from_name, from_cwd, text } => {
                let body = format!(
                    "[message from session \"{from_name}\" (id {from_id}) @ {from_cwd}]\n{text}\n\n(Reply with send_message to \"{from_id}\" if a response is needed.)"
                );
                (Origin::Peer { from_id, from_name }, body, Vec::new())
            }
            Input::Spawn { from_id, from_name, text, must_report } => {
                let mut body = format!("[task from session \"{from_name}\" (id {from_id})]\n{text}");
                if must_report {
                    body.push_str(&format!(
                        "\n\n(When you are done, report your result to session \"{from_id}\" with send_message.)"
                    ));
                }
                (Origin::Spawn { from_id, from_name }, body, Vec::new())
            }
            Input::System(t) => (Origin::System, t, Vec::new()),
        };
        let mut parts = Vec::new();
        if !text.is_empty() {
            parts.push(Part::Text { text, time: now });
        }
        parts.extend(images.into_iter().map(Part::Image));
        Message {
            id: util::new_id("msg"),
            role: Role::User,
            origin,
            time: now,
            completed: Some(now),
            parts,
            model: None,
            agent: Some(agent.to_string()),
            usage: None,
            finish: None,
            error: None,
            replay_key: None,
        }
    }
}

pub struct State {
    pub info: SessionInfo,
    pub msgs: Vec<Message>,
    pub meta: SessionMeta,
    pub queue: VecDeque<Input>,
    /// Set by abort: queued inputs wait for the next new input.
    pub paused: bool,
    pub busy: bool,
    pub retry: Option<SessionStatus>,
    pub cancel: Option<CancellationToken>,
    pub perms: HashMap<String, oneshot::Sender<PermissionDecision>>,
    /// Tools allowed for the rest of the session ("Always").
    pub always: HashSet<String>,
    /// Manual compaction requested (runs at the next boundary).
    pub pending_compact: bool,
    /// Summary from the `compact_context` tool, applied after the current step.
    pub checkpoint: Option<String>,
    pub info_dirty: bool,
    pub msgs_dirty: bool,
    pub closed: bool,
    /// MCP connections were (re)tried for this session already.
    pub mcp_tried: bool,
    /// Persistence snapshot counter.
    snap_seq: u64,
}

pub struct Session {
    pub id: String,
    hub: Weak<Hub>,
    st: Mutex<State>,
    kick: Notify,
    persist_kick: Arc<Notify>,
    busy_tx: watch::Sender<bool>,
    pub store: Arc<hn_tools::EditStore>,
    /// Serializes disk writes; see [`Session::write_snapshot`].
    disk: Mutex<DiskSeq>,
}

impl Session {
    pub fn new(hub: &Arc<Hub>, mut info: SessionInfo, msgs: Vec<Message>, meta: SessionMeta) -> Arc<Session> {
        info.open = true;
        info.attached = 0;
        info.status = SessionStatus::Idle;
        info.message_count = msgs.len() as u32;
        let (busy_tx, _) = watch::channel(false);
        let s = Arc::new(Session {
            id: info.id.clone(),
            hub: Arc::downgrade(hub),
            st: Mutex::new(State {
                info,
                msgs,
                meta,
                queue: VecDeque::new(),
                paused: false,
                busy: false,
                retry: None,
                cancel: None,
                perms: HashMap::new(),
                always: HashSet::new(),
                pending_compact: false,
                checkpoint: None,
                info_dirty: false,
                msgs_dirty: false,
                closed: false,
                mcp_tried: false,
                snap_seq: 0,
            }),
            kick: Notify::new(),
            persist_kick: Arc::new(Notify::new()),
            busy_tx,
            store: Arc::new(hn_tools::EditStore::new()),
            disk: Mutex::new(DiskSeq::default()),
        });
        tokio::spawn(runtime(s.clone()));
        tokio::spawn(persister(Arc::downgrade(&s), s.persist_kick.clone()));
        s
    }

    pub fn hub(&self) -> Option<Arc<Hub>> {
        self.hub.upgrade()
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, State> {
        lock(&self.st)
    }

    pub fn info(&self) -> SessionInfo {
        self.lock().info.clone()
    }

    pub fn is_closed(&self) -> bool {
        self.lock().closed
    }

    /// Emit an event for this session (call with the state locked).
    pub(crate) fn emit(&self, st: &State, ev: Event) {
        if let Some(h) = self.hub() {
            h.emit(&self.id, st.info.attached == 0, &ev);
        }
    }

    /// Info changed: push Info + Sessions, persist.
    pub(crate) fn info_changed(&self, st: &mut State) {
        st.info_dirty = true;
        self.emit(st, Event::Info { info: st.info.clone() });
        if let Some(h) = self.hub() {
            h.touch();
        }
        self.persist_kick.notify_one();
    }

    pub(crate) fn msgs_changed(&self, st: &mut State) {
        st.msgs_dirty = true;
        self.persist_kick.notify_one();
    }

    pub fn mark_dirty(&self) {
        let mut st = self.lock();
        st.info_dirty = true;
        st.msgs_dirty = true;
        drop(st);
        self.persist_kick.notify_one();
    }

    /// Recompute the status from the state; push if changed.
    pub(crate) fn refresh_status(&self, st: &mut State) {
        let status = if !st.busy {
            SessionStatus::Idle
        } else if let Some(r) = &st.retry {
            r.clone()
        } else if !st.perms.is_empty() {
            SessionStatus::Permission
        } else {
            SessionStatus::Busy
        };
        if st.info.status != status {
            st.info.status = status;
            self.emit(st, Event::Info { info: st.info.clone() });
            if let Some(h) = self.hub() {
                h.touch();
            }
        }
    }

    pub(crate) fn push_msg(&self, st: &mut State, msg: Message) {
        st.info.updated_at = now_ms();
        st.msgs.push(msg.clone());
        st.info.message_count = st.msgs.len() as u32;
        self.emit(st, Event::Message { message: msg });
        self.msgs_changed(st);
        self.info_changed(st);
    }

    pub(crate) fn remove_msgs(&self, st: &mut State, ids: Vec<String>) {
        if ids.is_empty() {
            return;
        }
        st.msgs.retain(|m| !ids.contains(&m.id));
        st.info.message_count = st.msgs.len() as u32;
        self.emit(st, Event::Removed { messages: ids });
        self.msgs_changed(st);
        self.info_changed(st);
    }

    // --- attachments ----------------------------------------------------------

    /// Attach a client; sends the Response (SessionInfo) and then the Snapshot.
    pub fn attach(&self, hub: &Hub, cid: u64, foreground: bool, req_id: u64) -> anyhow::Result<()> {
        let mut st = self.lock();
        anyhow::ensure!(!st.closed, "session {} is closed", self.id);
        anyhow::ensure!(hub.attach_client(cid, &self.id, foreground), "client gone");
        let n = hub.foreground_count(&self.id);
        let changed = n != st.info.attached;
        st.info.attached = n;
        let info = st.info.clone();
        hub.send_to(cid, ServerMsg::ok(req_id, &info));
        hub.send_to(
            cid,
            ServerMsg::Event { session: self.id.clone(), event: Event::Snapshot { info: info.clone(), messages: st.msgs.clone() } },
        );
        if changed {
            self.emit(&st, Event::Info { info });
            hub.touch();
        }
        Ok(())
    }

    pub fn detach(&self, hub: &Hub, cid: u64) {
        hub.detach_client(cid, &self.id);
        self.recount_attached(hub);
    }

    pub fn recount_attached(&self, hub: &Hub) {
        let mut st = self.lock();
        let n = hub.foreground_count(&self.id);
        if n != st.info.attached {
            st.info.attached = n;
            self.emit(&st, Event::Info { info: st.info.clone() });
            hub.touch();
        }
    }

    // --- inputs ---------------------------------------------------------------

    /// Queue an input. Returns Ok(true) if the session was idle (a turn starts now).
    pub fn enqueue(&self, input: Input) -> anyhow::Result<bool> {
        let mut st = self.lock();
        anyhow::ensure!(!st.closed, "session {} is closed", self.id);
        if let Input::Spawn { .. } = &input {
            st.meta.reported = false;
        }
        st.queue.push_back(input);
        st.paused = false;
        let idle = !st.busy;
        drop(st);
        self.kick.notify_one();
        Ok(idle)
    }

    pub fn request_compact(&self) -> anyhow::Result<bool> {
        let mut st = self.lock();
        anyhow::ensure!(!st.closed, "session {} is closed", self.id);
        st.pending_compact = true;
        st.paused = false;
        let idle = !st.busy;
        drop(st);
        self.kick.notify_one();
        Ok(idle)
    }

    pub fn permission(&self, call_id: &str, decision: PermissionDecision) -> anyhow::Result<()> {
        let tx = self.lock().perms.remove(call_id);
        match tx {
            Some(tx) => {
                let _ = tx.send(decision);
                Ok(())
            }
            None => anyhow::bail!("no pending permission request {call_id:?}"),
        }
    }

    pub fn is_busy(&self) -> bool {
        self.lock().busy
    }

    /// Cancel the running turn (if any). Returns true if something was running.
    pub fn abort(&self) -> bool {
        let mut st = self.lock();
        st.paused = true;
        st.pending_compact = false;
        match &st.cancel {
            Some(c) => {
                c.cancel();
                true
            }
            None => false,
        }
    }

    /// Abort and wait (bounded) until the turn has finished.
    pub async fn abort_and_wait(&self) {
        if !self.abort() && !self.is_busy() {
            return;
        }
        let mut rx = self.busy_tx.subscribe();
        let _ = tokio::time::timeout(Duration::from_secs(15), rx.wait_for(|b| !*b)).await;
    }

    // --- mutations from the frontend -------------------------------------------

    pub fn rename(&self, name: &str) {
        let mut st = self.lock();
        st.info.name = name.to_string();
        st.meta.name_explicit = true;
        st.info.updated_at = now_ms();
        self.info_changed(&mut st);
    }

    /// Set an automatic title unless the user named the session meanwhile.
    pub fn set_auto_title(&self, title: &str) {
        let mut st = self.lock();
        if st.meta.name_explicit || st.closed {
            return;
        }
        st.meta.titled = true;
        st.info.name = title.to_string();
        self.info_changed(&mut st);
    }

    pub fn set_model(&self, model: ModelRef, limits: Option<(u64, u64)>) {
        let mut st = self.lock();
        st.info.model = model;
        if let Some((max, at)) = limits {
            st.info.context.max = max;
            st.info.context.compact_at = at;
        }
        self.info_changed(&mut st);
    }

    pub fn set_agent(&self, agent: &str, model: Option<ModelRef>, limits: Option<(u64, u64)>) {
        let mut st = self.lock();
        st.info.agent = agent.to_string();
        if let Some(m) = model {
            st.info.model = m;
        }
        if let Some((max, at)) = limits {
            st.info.context.max = max;
            st.info.context.compact_at = at;
        }
        self.info_changed(&mut st);
    }

    /// Remove the last human message and everything after it. Returns its text.
    pub async fn undo(&self) -> anyhow::Result<String> {
        self.abort_and_wait().await;
        let mut st = self.lock();
        let Some(pos) = st.msgs.iter().rposition(|m| m.role == Role::User && m.origin == Origin::Human) else {
            anyhow::bail!("nothing to undo");
        };
        let text = util::parts_text(&st.msgs[pos].parts);
        let ids: Vec<String> = st.msgs[pos..].iter().map(|m| m.id.clone()).collect();
        self.remove_msgs(&mut st, ids);
        Ok(text)
    }

    /// Unload: abort, flush, notify attached clients. Keeps files.
    pub async fn close(&self, hub: &Hub) {
        self.abort_and_wait().await;
        {
            let mut st = self.lock();
            st.closed = true;
            st.queue.clear();
            for (_, tx) in st.perms.drain() {
                drop(tx);
            }
        }
        self.kick.notify_one();
        self.persist_kick.notify_one();
        self.flush_now();
        hub.remove_session(&self.id);
        for tx in hub.detach_all(&self.id) {
            let _ = tx.send(crate::hub::encode(&ServerMsg::Event { session: self.id.clone(), event: Event::Closed }));
        }
        hub.touch();
    }

    /// Take the dirty state for writing (clears the dirty flags).
    fn take_dirty(&self) -> Snapshot {
        let mut st = self.lock();
        st.snap_seq += 1;
        let snap = Snapshot {
            seq: st.snap_seq,
            info: st.info_dirty.then(|| st.info.clone()),
            msgs: st.msgs_dirty.then(|| st.msgs.clone()),
            meta: st.meta.clone(),
        };
        st.info_dirty = false;
        st.msgs_dirty = false;
        snap
    }

    /// Write a snapshot unless a newer one was already written (writes from the debounced
    /// persister and from close/shutdown may race; the newest state must win on disk).
    fn write_snapshot(&self, snap: Snapshot) {
        let mut disk = lock(&self.disk);
        let msgs = snap.msgs.filter(|_| snap.seq > disk.msgs);
        let info = snap.info.filter(|_| snap.seq > disk.info);
        if msgs.is_some() {
            disk.msgs = snap.seq;
        }
        if info.is_some() {
            disk.info = snap.seq;
        }
        write_out(&self.id, info.as_ref(), msgs.as_deref(), &snap.meta);
    }

    /// Synchronous flush (used on close / shutdown).
    pub fn flush_now(&self) {
        let snap = self.take_dirty();
        self.write_snapshot(snap);
    }

    /// Called on server exit: in-flight work is marked aborted and everything is written.
    pub fn shutdown_flush(&self) {
        {
            let mut st = self.lock();
            if let Some(c) = &st.cancel {
                c.cancel();
            }
            if st.busy {
                let now = now_ms();
                if let Some(m) = st.msgs.iter_mut().rev().find(|m| m.role == Role::Assistant) {
                    if m.completed.is_none() {
                        m.completed = Some(now);
                        m.finish = Some("aborted".into());
                    }
                    if cancel_open_tools(m, now) {
                        // Stopped while its tool calls were pending / running.
                        m.finish = Some("aborted".into());
                    }
                }
            }
            st.info_dirty = true;
            st.msgs_dirty = true;
        }
        self.flush_now();
    }

    /// Fix state persisted mid-turn (e.g. after a crash).
    pub fn repair_after_load(&self) {
        let mut st = self.lock();
        let now = now_ms();
        let mut changed = false;
        for m in st.msgs.iter_mut().filter(|m| m.role == Role::Assistant) {
            if m.completed.is_none() {
                m.completed = Some(now);
                m.finish.get_or_insert_with(|| "aborted".into());
                changed = true;
            }
            if cancel_open_tools(m, now) {
                m.finish = Some("aborted".into());
                changed = true;
            }
        }
        if changed {
            st.msgs_dirty = true;
        }
    }
}

/// Mark non-final tool parts cancelled. Returns true if anything changed.
pub(crate) fn cancel_open_tools(m: &mut Message, now: Millis) -> bool {
    let mut changed = false;
    for p in &mut m.parts {
        if let Part::Tool(tp) = p {
            if !tp.state.is_final() {
                tp.state = ToolState::Cancelled;
                if tp.output.is_empty() {
                    tp.output = "Tool call was cancelled by the user.".into();
                } else {
                    tp.output.push_str("\n\n[cancelled by the user]");
                }
                tp.time_end = Some(now);
                tp.run_at = None;
                changed = true;
            }
        }
    }
    changed
}

fn write_out(id: &str, info: Option<&SessionInfo>, msgs: Option<&[Message]>, meta: &SessionMeta) {
    if let Some(m) = msgs {
        if let Err(e) = persist::save_messages(id, m) {
            tracing::error!("session {id}: saving messages failed: {e:#}");
        }
    }
    if let Some(i) = info {
        if let Err(e) = persist::save_info(i) {
            tracing::error!("session {id}: saving info failed: {e:#}");
        }
        if let Err(e) = persist::save_meta(id, meta) {
            tracing::error!("session {id}: saving meta failed: {e:#}");
        }
    }
}

struct Snapshot {
    seq: u64,
    info: Option<SessionInfo>,
    msgs: Option<Vec<Message>>,
    meta: SessionMeta,
}

/// Sequence numbers of the snapshots last written to disk.
#[derive(Default)]
struct DiskSeq {
    info: u64,
    msgs: u64,
}

/// Debounced persistence (~300ms after the last change).
async fn persister(s: Weak<Session>, kick: Arc<Notify>) {
    loop {
        kick.notified().await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        let Some(sess) = s.upgrade() else { return };
        let snap = sess.take_dirty();
        let closed = sess.lock().closed;
        if snap.info.is_some() || snap.msgs.is_some() {
            let _ = tokio::task::spawn_blocking(move || sess.write_snapshot(snap)).await;
        }
        if closed {
            return;
        }
    }
}

/// Runtime task: runs turns while there is work.
async fn runtime(s: Arc<Session>) {
    loop {
        s.kick.notified().await;
        loop {
            let work = {
                let st = s.lock();
                if st.closed {
                    return;
                }
                !st.paused && (!st.queue.is_empty() || st.pending_compact)
            };
            if !work {
                break;
            }
            let Some(hub) = s.hub() else { return };
            if hub.is_shutting_down() {
                return;
            }
            turn::run_turn(&s, &hub).await;
        }
    }
}
