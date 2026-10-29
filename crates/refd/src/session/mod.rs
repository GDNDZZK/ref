//! Open sessions: shared state ([`SessionCore`]), lifecycle (create/open/close/delete) and
//! client-facing commands. The agent loop lives in [`engine`], tool execution in [`exec`],
//! session-level tools in [`tools`].
//!
//! Concurrency model: each open session has one driver task (see [`engine::driver`]) that
//! runs turns and manual compactions one at a time. Client commands mutate the state
//! directly under the session mutex (never held across `.await`) and wake the driver.
//! Every state change that is broadcast to clients is broadcast while holding the session
//! mutex, so a snapshot and the event stream that follows it are always consistent.
//!
//! Queued input: a prompt (user / peer / reminder) that arrives while the session is busy is
//! broadcast immediately (`MessageUpsert`) plus an info `Notice`, but is only inserted into
//! the history at the next step boundary (right before the next LLM request), so the model
//! sees it on its next step. On abort, queued inputs are moved into the history without
//! starting a turn (they are already visible in the transcript and will be seen by the
//! model with the next prompt).

pub mod engine;
pub mod exec;
pub mod manage;
pub mod prompt;
pub mod store;
pub mod tools;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use ref_config::Skill;
use ref_proto::{
    ImageInput, ImagePart, Message, MessageOrigin, NoticeLevel, Part, PermissionDecision, PermissionRequest, Role,
    ServerMsg, SessionId, SessionInfo, SessionStatus, TextPart, ToolState,
};
use tokio::sync::{oneshot, Notify};
use tokio_util::sync::CancellationToken;

use crate::hub::Hub;
use store::{Record, SessionLog, SessionMeta};

pub struct PendingPerm {
    pub req: PermissionRequest,
    pub tx: Option<oneshot::Sender<PermissionDecision>>,
}

/// Mutable session state (behind `SessionCore::st`).
pub struct State {
    pub info: SessionInfo,
    /// Name is generated and may be replaced by a generated title.
    pub auto_name: bool,
    pub title_started: bool,
    pub messages: Vec<Message>,
    /// Inputs received while busy; inserted at the next step boundary.
    pub queue: Vec<Message>,
    /// Token of the running work item (Some = busy).
    pub turn: Option<CancellationToken>,
    pub run_requested: bool,
    /// Client asked for a compaction.
    pub compact_requested: bool,
    /// Summary given to the `compact` tool, applied at the next step boundary.
    pub compact_summary: Option<String>,
    pub permissions: Vec<PendingPerm>,
    /// Tools allowed for the rest of the session ("Always").
    pub allowed: HashSet<String>,
    /// Cached system prompt blocks (stable for the session; reset on agent change / reload).
    pub system: Option<Arc<Vec<String>>>,
    pub skills: Arc<Vec<Skill>>,
    /// must_report: has sent a message to its parent since the last prompt.
    pub reported: bool,
    pub reminders: u32,
    dirty: Vec<String>,
    removed: Vec<String>,
    pub closing: bool,
}

impl State {
    pub fn info_snapshot(&self) -> SessionInfo {
        let mut i = self.info.clone();
        i.message_count = self.messages.len() as u32;
        i.pending_permissions = self.permissions.len() as u32;
        i
    }

    pub fn bcast(&self, hub: &Hub, msg: &ServerMsg) {
        hub.broadcast_session(&self.info.id, msg);
    }

    /// Broadcast SessionUpdated and schedule a SessionList push.
    pub fn info_changed(&self, hub: &Hub) {
        self.bcast(hub, &ServerMsg::SessionUpdated { info: self.info_snapshot() });
        hub.mark_list_dirty();
    }

    pub fn set_status(&mut self, hub: &Hub, status: SessionStatus) {
        if self.info.status != status {
            self.info.status = status;
            self.info_changed(hub);
        }
    }

    pub fn touch(&mut self) {
        self.info.updated = ref_proto::now_ms();
    }

    pub fn mark_dirty(&mut self, id: &str) {
        if !self.dirty.iter().any(|d| d == id) {
            self.dirty.push(id.to_string());
        }
    }

    pub fn msg_index(&self, id: &str) -> Option<usize> {
        self.messages.iter().rposition(|m| m.id == id)
    }

    pub fn msg_mut(&mut self, id: &str) -> Option<&mut Message> {
        let i = self.msg_index(id)?;
        self.messages.get_mut(i)
    }

    pub fn part_mut(&mut self, msg_id: &str, part_id: &str) -> Option<&mut Part> {
        self.msg_mut(msg_id)?.parts.iter_mut().find(|p| p.id() == part_id)
    }

    /// Insert or replace a message and broadcast it.
    pub fn upsert_message(&mut self, hub: &Hub, m: Message) {
        let id = m.id.clone();
        let msg = ServerMsg::MessageUpsert { session_id: self.info.id.clone(), message: m.clone() };
        match self.msg_index(&id) {
            Some(i) => self.messages[i] = m,
            None => self.messages.push(m),
        }
        self.mark_dirty(&id);
        self.touch();
        self.bcast(hub, &msg);
    }

    /// Re-broadcast an existing message (after in-place edits).
    pub fn emit_message(&mut self, hub: &Hub, id: &str) {
        if let Some(i) = self.msg_index(id) {
            let msg = ServerMsg::MessageUpsert { session_id: self.info.id.clone(), message: self.messages[i].clone() };
            self.mark_dirty(id);
            self.touch();
            self.bcast(hub, &msg);
        }
    }

    pub fn upsert_part(&mut self, hub: &Hub, msg_id: &str, part: Part) {
        let Some(m) = self.msg_mut(msg_id) else { return };
        match m.parts.iter().position(|p| p.id() == part.id()) {
            Some(i) => m.parts[i] = part.clone(),
            None => m.parts.push(part.clone()),
        }
        self.mark_dirty(msg_id);
        self.touch();
        self.bcast(
            hub,
            &ServerMsg::PartUpsert { session_id: self.info.id.clone(), message_id: msg_id.to_string(), part },
        );
    }

    /// Mutate a part in place and broadcast the result.
    pub fn update_part(&mut self, hub: &Hub, msg_id: &str, part_id: &str, f: impl FnOnce(&mut Part)) {
        let Some(p) = self.part_mut(msg_id, part_id) else { return };
        f(p);
        let part = p.clone();
        self.mark_dirty(msg_id);
        self.touch();
        self.bcast(
            hub,
            &ServerMsg::PartUpsert { session_id: self.info.id.clone(), message_id: msg_id.to_string(), part },
        );
    }

    pub fn remove_message(&mut self, hub: &Hub, id: &str) {
        if let Some(i) = self.msg_index(id) {
            self.messages.remove(i);
            self.dirty.retain(|d| d != id);
            self.removed.push(id.to_string());
            self.touch();
            self.bcast(hub, &ServerMsg::MessageRemoved { session_id: self.info.id.clone(), message_id: id.to_string() });
        }
    }

    /// Move queued inputs into the history (they were already broadcast).
    pub fn drain_queue(&mut self) -> usize {
        let q = std::mem::take(&mut self.queue);
        let n = q.len();
        for m in q {
            let id = m.id.clone();
            self.messages.push(m);
            self.mark_dirty(&id);
        }
        if n > 0 {
            self.touch();
        }
        n
    }

    /// Resolve all pending permission requests as cancelled (drops the waiters).
    pub fn cancel_permissions(&mut self, hub: &Hub) {
        let perms = std::mem::take(&mut self.permissions);
        for p in perms {
            self.bcast(hub, &ServerMsg::PermissionResolved { session_id: self.info.id.clone(), call_id: p.req.call_id });
        }
    }

    /// Projected context size of the next request: the last reported usage plus an estimate of
    /// what was added after it (tool results of that response, later messages). Falls back to
    /// a full estimate when no usage is known.
    pub fn projected_context(&self) -> u64 {
        let start = prompt::context_start(&self.messages);
        let msgs = &self.messages[start..];
        let Some((i, used)) =
            msgs.iter().enumerate().rev().find_map(|(i, m)| m.usage.as_ref().map(|u| (i, u.context_tokens())))
        else {
            return self.estimate_context();
        };
        if used == 0 {
            return self.estimate_context();
        }
        let mut chars = 0usize;
        // Tool results of the response that reported the usage are not part of it.
        for p in &msgs[i].parts {
            if let Part::Tool(t) = p {
                chars += t.output.len();
                if !t.images.is_empty() {
                    chars += 6000 * t.images.len();
                }
            }
        }
        for m in &msgs[i + 1..] {
            chars += message_chars(m);
        }
        used + crate::util::estimate_tokens(chars)
    }

    /// Context token estimate for the current LLM context (chars / 4).
    pub fn estimate_context(&self) -> u64 {
        let start = prompt::context_start(&self.messages);
        let sys: usize = self.system.as_ref().map(|s| s.iter().map(|b| b.len()).sum()).unwrap_or(0);
        let mut chars = sys;
        for m in &self.messages[start..] {
            chars += message_chars(m);
        }
        crate::util::estimate_tokens(chars)
    }
}

/// Approximate size (chars) of a message as sent to the model.
fn message_chars(m: &Message) -> usize {
    m.parts
        .iter()
        .map(|p| match p {
            Part::Text(t) => t.text.len(),
            Part::Reasoning(r) => r.text.len(),
            Part::Tool(t) => {
                let input = if t.input_raw.is_empty() { t.input.to_string().len() } else { t.input_raw.len() };
                input + t.output.len() + 6000 * t.images.len()
            }
            Part::Image(_) => 6000,
        })
        .sum()
}

pub enum Submitted {
    /// Session was idle: the input starts a turn.
    Started,
    /// Session is busy: delivered at the next step boundary.
    Queued,
    /// Session is closing: the input was dropped.
    Closed,
}

pub struct SessionCore {
    pub id: SessionId,
    pub hub: Arc<Hub>,
    pub st: Mutex<State>,
    log: Mutex<SessionLog>,
    pub snapshots: Arc<Mutex<ref_tools::SnapshotStore>>,
    /// Wakes the driver (new work).
    pub(crate) wake: Notify,
    /// Wakes a retry backoff early (model change / reload).
    pub(crate) retry_wake: Notify,
    /// Notified (all waiters) when the session becomes idle.
    pub(crate) idle: Notify,
    pub(crate) closed: CancellationToken,
    driver: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl SessionCore {
    fn new(hub: Arc<Hub>, info: SessionInfo, messages: Vec<Message>, auto_name: bool, log_lines: usize) -> Arc<Self> {
        let log = SessionLog::new(&hub.sessions_dir, &info.id, log_lines);
        Arc::new(SessionCore {
            id: info.id.clone(),
            st: Mutex::new(State {
                info,
                auto_name,
                title_started: false,
                messages,
                queue: Vec::new(),
                turn: None,
                run_requested: false,
                compact_requested: false,
                compact_summary: None,
                permissions: Vec::new(),
                allowed: HashSet::new(),
                system: None,
                skills: Arc::new(Vec::new()),
                reported: false,
                reminders: 0,
                dirty: Vec::new(),
                removed: Vec::new(),
                closing: false,
            }),
            hub,
            log: Mutex::new(log),
            snapshots: Arc::new(Mutex::new(ref_tools::SnapshotStore::default())),
            wake: Notify::new(),
            retry_wake: Notify::new(),
            idle: Notify::new(),
            closed: CancellationToken::new(),
            driver: Mutex::new(None),
        })
    }

    pub fn info(&self) -> SessionInfo {
        self.st.lock().info_snapshot()
    }

    pub fn cwd(&self) -> String {
        self.st.lock().info.cwd.clone()
    }

    pub fn name(&self) -> String {
        self.st.lock().info.name.clone()
    }

    pub fn is_busy(&self) -> bool {
        self.st.lock().turn.is_some()
    }

    pub fn wake(&self) {
        self.wake.notify_one();
        self.retry_wake.notify_waiters();
    }

    pub fn set_attached(&self, n: u32) {
        let mut st = self.st.lock();
        if st.info.attached != n {
            st.info.attached = n;
            st.info_changed(&self.hub);
        }
    }

    pub fn invalidate_system_prompt(&self) {
        self.st.lock().system = None;
    }

    /// Send a full snapshot to one client (under the state lock: consistent with events).
    pub fn send_snapshot(&self, client: &str, req: u64) {
        let st = self.st.lock();
        let mut messages = st.messages.clone();
        messages.extend(st.queue.iter().cloned());
        let msg = ServerMsg::SessionSnapshot {
            req,
            info: st.info_snapshot(),
            messages,
            permissions: st.permissions.iter().map(|p| p.req.clone()).collect(),
        };
        self.hub.send(client, &msg);
    }

    /// Persist dirty messages / removals and the meta file.
    ///
    /// No-op once the session is closed: the final state was written by `shutdown`, and a
    /// late writer (e.g. a title arriving after close/delete) must not resurrect the files.
    pub fn persist(&self) {
        // Checked under the log lock: `shutdown` cancels `closed` before taking the lock for
        // the final rewrite, so no write can slip in after it.
        let mut log = self.log.lock();
        if self.closed.is_cancelled() {
            return;
        }
        let (records, full, meta) = {
            let mut st = self.st.lock();
            let live = st.messages.len();
            let pending = st.dirty.len() + st.removed.len();
            let compact = log.needs_compaction(live) || log.lines + pending > 2 * live + 64;
            let dirty = std::mem::take(&mut st.dirty);
            let removed = std::mem::take(&mut st.removed);
            let full = compact.then(|| st.messages.clone());
            // Removals first: a message that was removed and re-inserted (compaction moves the
            // trailing prompt after the summary) must end up present, at its new position.
            let records: Vec<Result<Message, String>> = if compact {
                vec![]
            } else {
                let mut r: Vec<Result<Message, String>> = removed.into_iter().map(Err).collect();
                // In history order, so new messages are appended in the right order.
                let mut idx: Vec<usize> = dirty.iter().filter_map(|id| st.msg_index(id)).collect();
                idx.sort_unstable();
                r.extend(idx.into_iter().map(|i| Ok(st.messages[i].clone())));
                r
            };
            (records, full, SessionMeta::from_info(&st.info_snapshot(), st.auto_name))
        };
        let res = match full {
            Some(all) => log.rewrite(&all),
            None => {
                let recs: Vec<Record> = records
                    .iter()
                    .map(|r| match r {
                        Ok(m) => Record::Message(m),
                        Err(id) => Record::Removed(id),
                    })
                    .collect();
                log.append(&recs)
            }
        };
        if let Err(e) = res {
            tracing::error!("session {}: failed to write message log: {e}", self.id);
        }
        if let Err(e) = store::write_meta(&self.hub.sessions_dir, &meta) {
            tracing::error!("session {}: failed to write meta: {e}", self.id);
        }
    }

    /// Rewrite the log with exactly the current messages (on close).
    fn persist_compacted(&self) {
        let mut log = self.log.lock();
        let (all, meta) = {
            let mut st = self.st.lock();
            st.dirty.clear();
            st.removed.clear();
            (st.messages.clone(), SessionMeta::from_info(&st.info_snapshot(), st.auto_name))
        };
        if let Err(e) = log.rewrite(&all) {
            tracing::error!("session {}: failed to rewrite message log: {e}", self.id);
        }
        if let Err(e) = store::write_meta(&self.hub.sessions_dir, &meta) {
            tracing::error!("session {}: failed to write meta: {e}", self.id);
        }
    }

    /// Submit an input message (user prompt, peer message, spawn prompt, reminder).
    pub fn submit(self: &Arc<Self>, msg: Message) -> Submitted {
        let hub = &self.hub;
        let started = {
            let mut st = self.st.lock();
            if st.closing {
                return Submitted::Closed;
            }
            // must_report counts from the last real prompt (user prompt or spawn task). Peer
            // messages do not re-arm it: a parent answering "thanks" must not make the child
            // report again, and two sessions must not ping-pong reminders forever.
            if matches!(msg.origin, MessageOrigin::User | MessageOrigin::Spawn { .. }) {
                st.reported = false;
                st.reminders = 0;
            }
            if st.turn.is_none() {
                st.upsert_message(hub, msg);
                st.run_requested = true;
                st.begin_work(hub);
                st.info_changed(hub);
                true
            } else {
                st.queue.push(msg.clone());
                st.touch();
                st.bcast(hub, &ServerMsg::MessageUpsert { session_id: self.id.clone(), message: msg });
                st.bcast(
                    hub,
                    &ServerMsg::Notice {
                        session_id: Some(self.id.clone()),
                        level: NoticeLevel::Info,
                        text: "Queued: the agent will see this message after its current step".into(),
                    },
                );
                false
            }
        };
        // Also wake when queued: the driver may hold an aborted (not yet started) turn.
        self.wake.notify_one();
        if started {
            self.persist();
            Submitted::Started
        } else {
            Submitted::Queued
        }
    }

    /// Prompt typed by a user.
    pub fn prompt(self: &Arc<Self>, text: String, images: Vec<ImageInput>) {
        let mut msg = Message::new(Role::User, MessageOrigin::User);
        if !text.is_empty() || images.is_empty() {
            msg.parts.push(text_part(text));
        }
        for img in images {
            msg.parts.push(Part::Image(ImagePart {
                id: ref_proto::new_id("prt"),
                mime: img.mime,
                data: img.data,
                name: img.name,
            }));
        }
        let first_text = msg.raw_text();
        self.submit(msg);
        let start_title = {
            let mut st = self.st.lock();
            let go = st.auto_name && !st.title_started && !first_text.trim().is_empty();
            if go {
                st.title_started = true;
            }
            go
        };
        if start_title {
            let core = self.clone();
            tokio::spawn(async move { engine::generate_title(core, first_text).await });
        }
    }

    /// Abort the running work: cancels streaming, tools, permission requests and delays.
    pub fn abort(&self) {
        let hub = &self.hub;
        let mut st = self.st.lock();
        let Some(tok) = st.turn.clone() else { return };
        tok.cancel();
        st.run_requested = false;
        st.compact_requested = false;
        st.compact_summary = None;
        st.drain_queue();
        st.cancel_permissions(hub);
        st.info_changed(hub);
    }

    /// Wait until the session is idle (or the timeout elapses).
    pub async fn wait_idle(&self, timeout: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let notified = self.idle.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if !self.is_busy() {
                return true;
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return !self.is_busy();
            }
        }
    }

    pub fn permission(&self, call_id: &str, decision: PermissionDecision) -> bool {
        let hub = &self.hub;
        let mut st = self.st.lock();
        let Some(i) = st.permissions.iter().position(|p| p.req.call_id == call_id) else { return false };
        let mut p = st.permissions.remove(i);
        let tool = p.req.tool.clone();
        if let Some(tx) = p.tx.take() {
            let _ = tx.send(decision);
        }
        st.bcast(hub, &ServerMsg::PermissionResolved { session_id: self.id.clone(), call_id: call_id.to_string() });
        if decision == PermissionDecision::Always {
            st.allowed.insert(tool.clone());
            // Other pending requests for the same tool are now allowed too.
            let same: Vec<usize> =
                (0..st.permissions.len()).rev().filter(|&j| st.permissions[j].req.tool == tool).collect();
            for j in same {
                let mut q = st.permissions.remove(j);
                if let Some(tx) = q.tx.take() {
                    let _ = tx.send(PermissionDecision::Once);
                }
                st.bcast(hub, &ServerMsg::PermissionResolved { session_id: self.id.clone(), call_id: q.req.call_id });
            }
        }
        st.info_changed(hub);
        true
    }

    /// Remove the last user prompt and everything after it. Returns the removed text.
    pub async fn undo(self: &Arc<Self>) -> Result<String, String> {
        if self.is_busy() {
            self.abort();
            if !self.wait_idle(Duration::from_secs(15)).await {
                return Err("Session is still busy; try again".into());
            }
        }
        let hub = &self.hub;
        let text = {
            let mut st = self.st.lock();
            if st.turn.is_some() {
                return Err("Session is busy; try again".into());
            }
            let Some(i) = st.messages.iter().rposition(|m| m.role == Role::User && m.origin == MessageOrigin::User)
            else {
                return Err("Nothing to undo".into());
            };
            let text = st.messages[i].raw_text();
            let ids: Vec<String> = st.messages[i..].iter().map(|m| m.id.clone()).collect();
            for id in ids.iter().rev() {
                st.remove_message(hub, id);
            }
            st.info.context.used = last_context_used(&st.messages).unwrap_or_else(|| st.estimate_context());
            st.bcast(hub, &ServerMsg::Undone { session_id: self.id.clone(), text: text.clone() });
            st.info_changed(hub);
            text
        };
        for cid in hub.viewers(&self.id) {
            self.send_snapshot(&cid, 0);
        }
        self.persist();
        Ok(text)
    }

    pub fn request_compaction(&self) {
        let mut st = self.st.lock();
        st.compact_requested = true;
        drop(st);
        self.wake.notify_one();
    }
}

pub fn text_part(text: String) -> Part {
    Part::Text(TextPart { id: ref_proto::new_id("prt"), text, time: ref_proto::now_ms(), meta: None })
}

pub fn user_message(origin: MessageOrigin, text: String) -> Message {
    let mut m = Message::new(Role::User, origin);
    m.parts.push(text_part(text));
    m
}

/// Context tokens reported by the last assistant message in the current context.
pub fn last_context_used(messages: &[Message]) -> Option<u64> {
    let start = prompt::context_start(messages);
    messages[start..].iter().rev().find_map(|m| m.usage.as_ref().map(|u| u.context_tokens()))
}

/// Fix up messages loaded from disk that were interrupted (backend crash / restart).
fn repair_loaded(messages: &mut Vec<Message>) {
    // An assistant message that was persisted before it got any content is useless noise.
    messages.retain(|m| !(m.role == Role::Assistant && m.parts.is_empty() && m.finish.is_none()));
    for m in messages.iter_mut() {
        if m.role != Role::Assistant {
            continue;
        }
        let mut changed = false;
        for p in &mut m.parts {
            if let Part::Tool(t) = p {
                if !t.state.is_final() {
                    t.state = ToolState::Cancelled;
                    if t.output.is_empty() {
                        t.output = "Interrupted (backend restarted)".into();
                    }
                    changed = true;
                }
            }
        }
        if m.finish.is_none() {
            m.finish = Some("aborted".into());
            changed = true;
        }
        if changed && m.time_end.is_none() {
            m.time_end = Some(m.time);
        }
    }
}
