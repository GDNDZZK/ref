//! Live session state and persistence glue.

use crate::store::Persisted;
use ref_config::Config;
use ref_proto::{Message, MessageSource, Part, PermissionRequest, Role, SessionEvent, SessionInfo, SessionStatus, ToolState};
use ref_tools::HashStore;
use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

pub struct RunHandle {
    pub cancel: CancellationToken,
    pub id: u64,
}

pub struct PendingPerm {
    pub request: PermissionRequest,
    /// (allow, always)
    pub tx: Option<oneshot::Sender<(bool, bool)>>,
}

pub struct SessionState {
    pub info: SessionInfo,
    pub messages: Vec<Message>,
    /// User / peer messages that arrived while a run was active.
    pub queue: VecDeque<Message>,
    pub run: Option<RunHandle>,
    pub perms: Vec<PendingPerm>,
    /// Tools allowed for the rest of the session ("always").
    pub always: HashSet<String>,
    pub cfg: Arc<Config>,
    /// Cached system prompt parts (kept byte-identical between requests).
    pub system: Option<Vec<String>>,
    /// Date (YYYY-MM-DD) fixed when the session was created / loaded.
    pub date: String,
    /// must_report: whether the session has reported since the last relevant user input.
    pub reported: bool,
    /// Consecutive must_report reminders.
    pub reminders: u32,
    /// The recommend-compaction notice was issued in the current context epoch (persisted;
    /// reset by compaction).
    pub context_notice: bool,
    /// The notice is due and will be appended at the next safe point.
    pub notice_pending: bool,
    /// Summary requested by the `compact_context` tool, applied after the current tool batch.
    pub pending_compact: Option<String>,
    /// Abort requested by a user (do not auto-start a new run for queued messages).
    pub user_aborted: bool,
    pub title_started: bool,
    pub closed: bool,
}

pub struct Session {
    pub id: String,
    pub state: Mutex<SessionState>,
    pub hashes: Arc<HashStore>,
}

impl Session {
    pub fn new(info: SessionInfo, messages: Vec<Message>, cfg: Arc<Config>, reported: bool, context_notice: bool) -> Arc<Session> {
        Arc::new(Session {
            id: info.id.clone(),
            hashes: Arc::new(HashStore::new()),
            state: Mutex::new(SessionState {
                date: crate::util::fmt_date(ref_proto::now_ms()),
                title_started: !info.title.is_empty(),
                info,
                messages,
                queue: VecDeque::new(),
                run: None,
                perms: Vec::new(),
                always: HashSet::new(),
                cfg,
                system: None,
                reported,
                reminders: 0,
                context_notice,
                notice_pending: false,
                pending_compact: None,
                user_aborted: false,
                closed: false,
            }),
        })
    }

    pub fn lock(&self) -> MutexGuard<'_, SessionState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn info(&self) -> SessionInfo {
        self.lock().info.clone()
    }

    pub fn persisted(&self) -> Persisted {
        let st = self.lock();
        let mut info = st.info.clone();
        info.live = false;
        info.attached = 0;
        info.status = SessionStatus::Idle;
        info.busy_since = None;
        let mut messages = st.messages.clone();
        // Queued messages were accepted from the user; keep them.
        messages.extend(st.queue.iter().cloned());
        Persisted { info, messages, reported: st.reported, context_notice: st.context_notice }
    }
}

impl SessionState {
    /// Append a message to the history, updating must_report bookkeeping.
    pub fn push_message(&mut self, msg: Message) {
        if msg.role == Role::User {
            match &msg.source {
                MessageSource::Reminder => {}
                MessageSource::Human | MessageSource::Spawn { .. } => {
                    self.reported = false;
                    self.reminders = 0;
                }
                MessageSource::Peer { from_id, .. } => {
                    // A request from the parent needs a new report; other peers don't.
                    if self.info.parent.as_deref() == Some(from_id.as_str()) {
                        self.reported = false;
                    }
                    self.reminders = 0;
                }
                MessageSource::Compaction | MessageSource::Shell => {}
            }
        }
        self.info.updated_at = ref_proto::now_ms();
        self.messages.push(msg);
    }

    pub fn find_message(&self, id: &str) -> Option<usize> {
        self.messages.iter().rposition(|m| m.id == id)
    }

    /// `Reset` event with the newest page of messages.
    pub fn reset_event(&self) -> SessionEvent {
        let (messages, more_before) = page(&self.messages, self.messages.len(), None);
        SessionEvent::Reset { messages, more_before }
    }
}

/// Messages per page when the client does not say (`Attach`, `Messages`, `Reset`).
pub const PAGE_DEFAULT: usize = 100;
pub const PAGE_MAX: usize = 1000;
/// A page stops growing once its serialized size (images as stubs) reaches this.
pub const PAGE_BYTES: usize = 1 << 20;

/// The newest messages of `messages[..end]` that fit one page, with images as stubs, and
/// whether older messages remain. At least one message unless `end` is 0.
pub fn page(messages: &[Message], end: usize, limit: Option<u32>) -> (Vec<Message>, bool) {
    let limit = limit.map_or(PAGE_DEFAULT, |l| (l as usize).clamp(1, PAGE_MAX));
    let end = end.min(messages.len());
    let mut out = Vec::new();
    let mut bytes = 0;
    for m in messages[..end].iter().rev() {
        if out.len() >= limit || bytes >= PAGE_BYTES {
            break;
        }
        let mut m = m.clone();
        m.strip_image_data();
        bytes += serde_json::to_string(&m).map_or(0, |s| s.len());
        out.push(m);
    }
    out.reverse();
    let more = end > out.len();
    (out, more)
}

/// Marks unfinished tool parts as cancelled (used after loading from disk / abort).
pub fn cancel_unfinished(messages: &mut [Message], output: &str) -> bool {
    let mut changed = false;
    let now = ref_proto::now_ms();
    for m in messages {
        for p in &mut m.parts {
            if let Part::Tool { state, output: out, time_end, run_at, .. } = p
                && !state.is_final()
            {
                *state = ToolState::Cancelled;
                if out.is_none() {
                    *out = Some(output.to_string());
                }
                *time_end = Some(now);
                *run_at = None;
                changed = true;
            }
        }
    }
    changed
}

/// Plain user message.
pub fn user_message(source: MessageSource, text: String, images: Vec<ref_proto::ImageData>) -> Message {
    let now = ref_proto::now_ms();
    let mut parts = vec![Part::Text { text, time: now }];
    for img in images {
        parts.push(Part::Image { image: img, name: None });
    }
    Message {
        id: crate::util::new_id("msg"),
        role: Role::User,
        time: now,
        time_end: None,
        source,
        parts,
        model: None,
        agent: None,
        usage: None,
        error: None,
    }
}
