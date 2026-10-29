//! Client-side mirror of a session: applies `SessionEvent`s (incl. `Delta`/`Part`/`Reset`).

use std::collections::HashMap;
use std::time::Instant;

use ref_proto::{Delta, ImageData, Message, MessageSource, Part, PermissionRequest, Role, SessionEvent, SessionInfo, SessionSnapshot, ToolState};

use crate::ui::chat::ChatState;

static VERSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub fn next_version() -> u64 {
    VERSION.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// A prompt sent while the session was busy, not yet appended by the server.
#[derive(Debug, Clone)]
pub struct Queued {
    /// Client-local key (click target identity; `id` is unknown until the server answers).
    pub key: u64,
    /// Server message id (from the `Prompt` response).
    pub id: Option<String>,
    pub text: String,
    pub images: Vec<ImageData>,
    /// Delete clicked: waiting for the server (or for `id`).
    pub removing: bool,
    /// Sender when it is not a prompt of the user (another session, a reminder). Such rows
    /// are shown only; they cannot be removed.
    pub from: Option<String>,
}

impl Queued {
    pub fn new(text: String, images: Vec<ImageData>) -> Self {
        Queued { key: next_version(), id: None, text, images, removing: false, from: None }
    }

    fn from_message(m: &Message) -> Self {
        let images = m.parts.iter().filter_map(|p| if let Part::Image { image, .. } = p { Some(image.clone()) } else { None }).collect();
        let mut q = Queued::new(m.raw_text(), images);
        q.id = Some(m.id.clone());
        q.from = match &m.source {
            MessageSource::Human => None,
            MessageSource::Peer { from_name, .. } | MessageSource::Spawn { from_name, .. } => Some(from_name.clone()),
            MessageSource::Reminder => Some("reminder".into()),
            MessageSource::Shell => Some("shell".into()),
            MessageSource::Compaction => Some("compaction".into()),
        };
        q
    }
}

pub struct SessionView {
    pub info: SessionInfo,
    pub messages: Vec<Message>,
    /// Parallel to `messages`: bumped on every change (render cache key).
    pub versions: Vec<u64>,
    index: HashMap<String, usize>,
    pub permissions: Vec<PermissionRequest>,
    /// Snapshot received.
    pub loaded: bool,
    /// When the session last became non-idle (client clock). Fallback for backends that don't
    /// send `info.busy_since`; resets when the view is recreated.
    pub busy_since: Option<Instant>,
    /// Prompts sent while busy, not yet appended by the server.
    pub queued: Vec<Queued>,
    /// Last `Error` event text (cleared on next prompt).
    pub error: Option<String>,
    pub closed: bool,
    pub chat: ChatState,
    /// Attached as foreground (true) or watch-only.
    pub foreground: bool,
    /// The server has messages older than `messages[0]` (history is loaded page by page).
    pub more_before: bool,
    /// A request for the page before `messages[0]` is in flight.
    pub older_pending: bool,
    /// Bumped whenever the message list is replaced; stale page responses are dropped.
    pub history_gen: u64,
}

impl SessionView {
    pub fn new(info: SessionInfo, foreground: bool) -> Self {
        let busy = !info.status.is_idle();
        SessionView {
            info,
            messages: Vec::new(),
            versions: Vec::new(),
            index: HashMap::new(),
            permissions: Vec::new(),
            loaded: false,
            busy_since: busy.then(Instant::now),
            queued: Vec::new(),
            error: None,
            closed: false,
            chat: ChatState::default(),
            foreground,
            more_before: false,
            older_pending: false,
            history_gen: 0,
        }
    }

    pub fn is_busy(&self) -> bool {
        !self.info.status.is_idle()
    }

    /// How long the current run has been going (ms), if busy.
    pub fn busy_ms(&self, now: i64) -> Option<i64> {
        if !self.is_busy() {
            return None;
        }
        busy_ms(&self.info, now).or_else(|| self.busy_since.map(|s| s.elapsed().as_millis() as i64))
    }

    fn reindex(&mut self) {
        self.index.clear();
        for (i, m) in self.messages.iter().enumerate() {
            self.index.insert(m.id.clone(), i);
        }
    }

    pub fn load_snapshot(&mut self, snap: SessionSnapshot) {
        self.set_info(snap.info);
        self.set_queued(snap.queued);
        self.set_messages(snap.messages, snap.more_before);
        self.permissions = snap.pending_permissions;
        self.loaded = true;
        self.closed = false;
    }

    fn set_messages(&mut self, msgs: Vec<Message>, more_before: bool) {
        self.messages = msgs;
        self.versions = self.messages.iter().map(|_| next_version()).collect();
        self.more_before = more_before;
        self.older_pending = false;
        self.history_gen += 1;
        self.reindex();
        self.drop_matched_queued();
    }

    /// Wants the page before `messages[0]`: `(generation, first message id)` for the request.
    pub fn older_request(&self) -> Option<(u64, String)> {
        (self.loaded && self.more_before && !self.older_pending).then(|| self.messages.first().map(|m| (self.history_gen, m.id.clone()))).flatten()
    }

    /// Inserts an older page in front, if it still fits (`generation` and `before` unchanged).
    pub fn prepend_older(&mut self, generation: u64, before: &str, older: Vec<Message>, more_before: bool) -> bool {
        if generation != self.history_gen || self.messages.first().map(|m| m.id.as_str()) != Some(before) {
            return false;
        }
        self.older_pending = false;
        self.more_before = more_before;
        let older: Vec<Message> = older.into_iter().filter(|m| !self.index.contains_key(&m.id)).collect();
        if older.is_empty() {
            return false;
        }
        let mut versions: Vec<u64> = older.iter().map(|_| next_version()).collect();
        versions.append(&mut self.versions);
        self.versions = versions;
        let mut msgs = older;
        msgs.append(&mut self.messages);
        self.messages = msgs;
        self.reindex();
        true
    }

    pub fn set_info(&mut self, info: SessionInfo) {
        let was_idle = self.info.status.is_idle();
        let idle = info.status.is_idle();
        if was_idle && !idle {
            self.busy_since = Some(Instant::now());
        } else if idle {
            self.busy_since = None;
        } else if self.busy_since.is_none() {
            self.busy_since = Some(Instant::now());
        }
        self.info = info;
    }

    /// Index of a message by id.
    pub fn find(&self, id: &str) -> Option<usize> {
        self.index.get(id).copied().filter(|&i| i < self.messages.len())
    }

    fn touch(&mut self, i: usize) {
        if let Some(v) = self.versions.get_mut(i) {
            *v = next_version();
        }
    }

    /// Applies an event. Returns true if anything visible changed.
    pub fn apply(&mut self, ev: SessionEvent) -> bool {
        match ev {
            SessionEvent::Info { info } => {
                self.set_info(info);
            }
            SessionEvent::Message { message } => {
                if let Some(i) = self.find(&message.id) {
                    self.messages[i] = message;
                    self.touch(i);
                } else {
                    self.index.insert(message.id.clone(), self.messages.len());
                    self.messages.push(message);
                    self.versions.push(next_version());
                }
                self.drop_matched_queued();
            }
            SessionEvent::Part { message_id, index, part } => {
                let Some(i) = self.find(&message_id) else { return false };
                let parts = &mut self.messages[i].parts;
                if index < parts.len() {
                    parts[index] = part;
                } else {
                    // Gaps should not happen; append to keep order robust.
                    parts.push(part);
                }
                self.touch(i);
            }
            SessionEvent::Delta { message_id, index, delta } => {
                let Some(i) = self.find(&message_id) else { return false };
                apply_delta(&mut self.messages[i].parts, index, delta);
                self.touch(i);
            }
            SessionEvent::Removed { message_id } => {
                if let Some(i) = self.find(&message_id) {
                    self.messages.remove(i);
                    self.versions.remove(i);
                    self.reindex();
                }
            }
            SessionEvent::Reset { messages, more_before } => {
                self.set_messages(messages, more_before);
            }
            SessionEvent::Permission { request } => {
                if !self.permissions.iter().any(|p| p.call_id == request.call_id) {
                    self.permissions.push(request);
                }
            }
            SessionEvent::PermissionResolved { call_id } => {
                self.permissions.retain(|p| p.call_id != call_id);
            }
            SessionEvent::Queue { messages } => {
                self.set_queued(messages);
            }
            SessionEvent::Error { text } => {
                self.error = Some(text);
            }
            SessionEvent::Closed => {
                self.closed = true;
                self.info.live = false;
            }
        }
        true
    }

    /// Removes queued prompts that the server has now appended as user messages: by id, or by
    /// text while the id is still unknown (the `Prompt` response hasn't arrived yet).
    fn drop_matched_queued(&mut self) {
        if self.queued.is_empty() {
            return;
        }
        let tail: Vec<String> = self
            .messages
            .iter()
            .rev()
            .take(20)
            .filter(|m| m.role == Role::User && m.source == MessageSource::Human)
            .map(|m| m.raw_text().trim().to_string())
            .collect();
        let index = &self.index;
        self.queued.retain(|q| match &q.id {
            Some(id) => !index.contains_key(id),
            None => !tail.iter().any(|t| t == q.text.trim()),
        });
    }

    /// Queued rows from the server's queue (snapshot or `Queue` event), in queue order.
    /// Existing rows keep their key / pending delete. A row of ours still waiting for its id
    /// takes the id of the queued human prompt with the same text (no duplicate row); the rest
    /// of those stay. Other id-bound rows are gone from the server queue (delivered, withdrawn
    /// elsewhere or undone).
    fn set_queued(&mut self, queue: Vec<Message>) {
        let mut old = std::mem::take(&mut self.queued);
        for m in queue {
            let mine = || old.iter().position(|q| q.id.is_none() && m.source == MessageSource::Human && q.text.trim() == m.raw_text().trim());
            let q = match old.iter().position(|q| q.id.as_deref() == Some(m.id.as_str())).or_else(mine) {
                Some(i) => {
                    let mut q = old.remove(i);
                    q.id = Some(m.id.clone());
                    q
                }
                None => Queued::from_message(&m),
            };
            self.queued.push(q);
        }
        self.queued.extend(old.into_iter().filter(|q| q.id.is_none()));
    }

    pub fn queued_mut(&mut self, key: u64) -> Option<&mut Queued> {
        self.queued.iter_mut().find(|q| q.key == key)
    }

    /// Context usage percentage (0..=100+), if known.
    pub fn context_pct(&self) -> Option<u64> {
        let c = self.info.context;
        (c.max > 0).then(|| c.used * 100 / c.max)
    }

    /// Whether anything in the transcript is animating (streaming, running tools).
    pub fn animating(&self) -> bool {
        self.is_busy()
    }
}

/// Working time of a session from the server's run start (`info.busy_since`), if busy.
pub fn busy_ms(info: &SessionInfo, now: i64) -> Option<i64> {
    if info.status.is_idle() {
        return None;
    }
    info.busy_since.map(|t| (now - t).max(0))
}

/// Assistant message still in progress (no end time) or with non-final tools.
pub fn message_live(m: &Message) -> bool {
    if m.role != Role::Assistant {
        return false;
    }
    if m.time_end.is_none() && m.error.is_none() {
        return true;
    }
    m.parts.iter().any(|p| matches!(p, Part::Tool { state, .. } if !state.is_final()))
}

pub fn apply_delta(parts: &mut Vec<Part>, index: usize, delta: Delta) {
    if index >= parts.len() {
        // Delta for a part we haven't seen: create it.
        let now = ref_proto::now_ms();
        let p = match &delta {
            Delta::Text { .. } => Part::Text { text: String::new(), time: now },
            Delta::Reasoning { .. } => Part::Reasoning { text: String::new(), time: now, time_end: None, provider_data: None },
            Delta::ToolInput { .. } => Part::Tool {
                call_id: format!("pending-{index}"),
                name: String::new(),
                purpose: String::new(),
                input: serde_json::Value::Null,
                raw_input: String::new(),
                state: ToolState::Streaming,
                output: None,
                meta: None,
                images: Vec::new(),
                time: now,
                time_end: None,
                run_at: None,
                provider_data: None,
            },
        };
        while parts.len() < index {
            parts.push(Part::Text { text: String::new(), time: now });
        }
        parts.push(p);
    }
    match (&mut parts[index], delta) {
        (Part::Text { text, .. }, Delta::Text { text: d }) => text.push_str(&d),
        (Part::Reasoning { text, .. }, Delta::Reasoning { text: d }) => text.push_str(&d),
        (Part::Tool { raw_input, purpose, .. }, Delta::ToolInput { text: d }) => {
            raw_input.push_str(&d);
            if purpose.is_empty()
                && let Some(p) = partial_purpose(raw_input)
            {
                *purpose = p;
            }
        }
        // Mismatched delta type: append text where it makes sense, otherwise ignore.
        (Part::Text { text, .. }, Delta::Reasoning { text: d }) => text.push_str(&d),
        (Part::Reasoning { text, .. }, Delta::Text { text: d }) => text.push_str(&d),
        _ => {}
    }
}

/// Extracts a (possibly partial) `"purpose": "..."` value from streaming tool JSON.
pub fn partial_purpose(raw: &str) -> Option<String> {
    let k = raw.find("\"purpose\"")?;
    let rest = raw[k + 9..].trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    let mut out = String::new();
    let mut esc = false;
    for c in rest.chars() {
        if esc {
            out.push(match c {
                'n' => ' ',
                't' => ' ',
                c => c,
            });
            esc = false;
        } else if c == '\\' {
            esc = true;
        } else if c == '"' {
            return Some(out);
        } else {
            out.push(c);
        }
    }
    (!out.is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> SessionInfo {
        SessionInfo {
            id: "s".into(),
            name: "n".into(),
            title: String::new(),
            agent: "build".into(),
            model: Default::default(),
            cwd: "/".into(),
            created_at: 0,
            updated_at: 0,
            status: Default::default(),
            attached: 1,
            live: true,
            parent: None,
            must_report: false,
            context: Default::default(),
            todos: Vec::new(),
            busy_since: None,
        }
    }

    fn msg(id: &str, role: Role) -> Message {
        Message {
            id: id.into(),
            role,
            time: 0,
            time_end: None,
            source: MessageSource::Human,
            parts: vec![],
            model: None,
            agent: None,
            usage: None,
            error: None,
        }
    }

    #[test]
    fn deltas_and_parts() {
        let mut v = SessionView::new(info(), true);
        v.apply(SessionEvent::Message { message: msg("a", Role::Assistant) });
        v.apply(SessionEvent::Delta { message_id: "a".into(), index: 0, delta: Delta::Reasoning { text: "hm".into() } });
        v.apply(SessionEvent::Delta { message_id: "a".into(), index: 0, delta: Delta::Reasoning { text: "m".into() } });
        v.apply(SessionEvent::Part { message_id: "a".into(), index: 1, part: Part::Text { text: "Hi".into(), time: 1 } });
        v.apply(SessionEvent::Delta { message_id: "a".into(), index: 1, delta: Delta::Text { text: " there".into() } });
        v.apply(SessionEvent::Delta { message_id: "a".into(), index: 2, delta: Delta::ToolInput { text: r#"{"purpose": "Run te"#.into() } });
        let m = &v.messages[0];
        assert!(matches!(&m.parts[0], Part::Reasoning { text, .. } if text == "hmm"));
        assert_eq!(m.raw_text(), "Hi there");
        assert!(matches!(&m.parts[2], Part::Tool { purpose, .. } if purpose == "Run te"));
        // Unknown message: ignored, no panic.
        assert!(!v.apply(SessionEvent::Delta { message_id: "zz".into(), index: 5, delta: Delta::Text { text: "x".into() } }));
        v.apply(SessionEvent::Removed { message_id: "a".into() });
        assert!(v.messages.is_empty() && v.find("a").is_none());
    }

    #[test]
    fn queued_matching() {
        let mut v = SessionView::new(info(), true);
        v.queued.push(Queued::new("do it".into(), vec![]));
        let mut m = msg("u", Role::User);
        m.parts.push(Part::Text { text: "do it".into(), time: 0 });
        v.apply(SessionEvent::Message { message: m });
        assert!(v.queued.is_empty());
        // With a known id only that message counts, not one with the same text.
        let mut q = Queued::new("again".into(), vec![]);
        q.id = Some("u3".into());
        v.queued.push(q);
        let mut m = msg("u2", Role::User);
        m.parts.push(Part::Text { text: "again".into(), time: 0 });
        v.apply(SessionEvent::Message { message: m });
        assert_eq!(v.queued.len(), 1);
        let mut m = msg("u3", Role::User);
        m.parts.push(Part::Text { text: "again".into(), time: 0 });
        v.apply(SessionEvent::Message { message: m });
        assert!(v.queued.is_empty());
    }

    #[test]
    fn snapshot_restores_queued_rows() {
        let mut v = SessionView::new(info(), true);
        let mut pending = Queued::new("waiting for id".into(), vec![]);
        pending.key = 1;
        let mut kept = Queued::new("kept".into(), vec![]);
        kept.id = Some("q1".into());
        kept.removing = true;
        let mut gone = Queued::new("gone".into(), vec![]);
        gone.id = Some("q0".into());
        v.queued = vec![gone, pending, kept.clone()];
        let mut q2 = msg("q2", Role::User);
        q2.parts.push(Part::Text { text: "from snapshot".into(), time: 0 });
        q2.parts.push(Part::Image { image: ImageData { mime: "image/png".into(), data: "x".into(), ..Default::default() }, name: None });
        let mut q1 = msg("q1", Role::User);
        q1.parts.push(Part::Text { text: "kept".into(), time: 0 });
        v.load_snapshot(SessionSnapshot { info: info(), messages: vec![], more_before: false, pending_permissions: vec![], queued: vec![q2, q1] });
        let rows: Vec<(Option<&str>, &str, usize)> = v.queued.iter().map(|q| (q.id.as_deref(), q.text.as_str(), q.images.len())).collect();
        assert_eq!(rows, [(Some("q2"), "from snapshot", 1), (Some("q1"), "kept", 0), (None, "waiting for id", 0)]);
        assert!(v.queued[1].removing && v.queued[1].key == kept.key, "existing row state kept");
    }

    #[test]
    fn purpose_extraction() {
        assert_eq!(partial_purpose(r#"{"purpose":"Read \"x\" file","path":"a"}"#).as_deref(), Some("Read \"x\" file"));
        assert_eq!(partial_purpose(r#"{"path":"a"}"#), None);
        assert_eq!(partial_purpose(r#"{"purpose": "#), None);
    }
}
