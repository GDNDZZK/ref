//! Client-side mirror of a session: applies `SessionEvent`s, layout estimates, and sliding-window paging.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use ref_proto::{
    ImageData, Message, MessageLayoutMeta, MessageSource, PART_KIND_IMAGE, PART_KIND_REASONING,
    PART_KIND_TEXT, PART_KIND_TOOL, Part, PartLayoutMeta, PermissionRequest, QuestionRequest, Role,
    SOURCE_KIND_COMPACTION, SessionEvent, SessionInfo, SessionSnapshot, compute_layout_meta,
};

use crate::ui::chat::ChatState;
use crate::util::apply_delta;

/// Messages per page for `Attach` and `Messages` range queries.
pub const PAGE_SIZE: usize = ref_proto::PAGE_DEFAULT;
/// Maximum materialized messages kept in a `SessionView` before evicting distant slots.
pub const MAX_RESIDENT_MESSAGES: usize = 300;
/// Newest messages retained when returning to the bottom or detaching.
pub const TAIL_KEEP_MESSAGES: usize = 100;
/// Resident margin kept around the visible sequence range before eviction.
const EVICT_MARGIN: usize = 150;
/// Debounce before sending a viewport-driven `Request::Messages` range query.
pub const RANGE_DEBOUNCE: Duration = Duration::from_millis(80);

/// How many recent messages are compared with queued prompt text.
const QUEUED_MATCH_WINDOW: usize = 20;

const MIN_ESTIMATE_WIDTH: usize = 20;
const USER_BUBBLE_CHROME_COLS: usize = 5;
const USER_BUBBLE_OVERHEAD_ROWS: usize = 2;
const COMPACT_USER_MAX_LINES: usize = 6;
const COMPACT_USER_INDENT_COLS: usize = 2;
const MIN_ASSISTANT_INNER_COLS: usize = 10;
const ASSISTANT_CONTENT_PAD: usize = 3;
const ASSISTANT_FOOTER_ROWS: usize = 2;
const ERROR_BANNER_ROWS: usize = 2;
const CODE_BLOCK_BORDER_ROWS: usize = 2;
const TABLE_OVERHEAD_ROWS: usize = 2;
const TOOL_BOX_OVERHEAD_ROWS: usize = 2;
const COMPACTION_ESTIMATE_ROWS: usize = 2;
const REASONING_INDENT_COLS: usize = 2;
const JUMP_LOOKBACK_DIVISOR: usize = 4;

static VERSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn next_version() -> u64 {
    VERSION.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

#[derive(Debug, Clone)]
pub enum MessageSlot {
    Materialized(Box<Message>),
    Placeholder(MessageLayoutMeta),
}

impl MessageSlot {
    pub fn id(&self) -> &str {
        match self {
            MessageSlot::Materialized(m) => &m.id,
            MessageSlot::Placeholder(meta) => &meta.id,
        }
    }
    pub fn is_compaction(&self) -> bool {
        match self {
            MessageSlot::Materialized(m) => {
                m.role == Role::User && m.source == MessageSource::Compaction
            }
            MessageSlot::Placeholder(meta) => {
                meta.role == Role::User && meta.source_kind == SOURCE_KIND_COMPACTION
            }
        }
    }

    pub fn is_materialized(&self) -> bool {
        matches!(self, MessageSlot::Materialized(_))
    }

    pub fn as_message(&self) -> Option<&Message> {
        match self {
            MessageSlot::Materialized(m) => Some(m),
            MessageSlot::Placeholder(_) => None,
        }
    }

    pub fn as_message_mut(&mut self) -> Option<&mut Message> {
        match self {
            MessageSlot::Materialized(m) => Some(m),
            MessageSlot::Placeholder(_) => None,
        }
    }

    #[cfg(test)]
    pub fn as_meta(&self) -> Option<&MessageLayoutMeta> {
        match self {
            MessageSlot::Placeholder(meta) => Some(meta),
            MessageSlot::Materialized(_) => None,
        }
    }
}

fn estimate_part_rows(
    p: &PartLayoutMeta,
    inner: usize,
    details: bool,
    thinking: bool,
) -> (usize, bool) {
    match p.kind.as_str() {
        PART_KIND_TEXT => {
            let base = (p.chars as usize)
                .div_ceil(inner)
                .max(p.lines as usize)
                .max(1);
            let extra = p.code_blocks as usize * CODE_BLOCK_BORDER_ROWS
                + p.tables as usize * TABLE_OVERHEAD_ROWS;
            (base + extra, false)
        }
        PART_KIND_REASONING => {
            if thinking {
                let r_inner = inner.saturating_sub(REASONING_INDENT_COLS).max(1);
                let body = (p.chars as usize)
                    .div_ceil(r_inner)
                    .max(p.lines as usize)
                    .max(1);
                (1 + body, false)
            } else {
                (1, true)
            }
        }
        PART_KIND_TOOL => {
            if details {
                (
                    TOOL_BOX_OVERHEAD_ROWS + (p.tool_lines as usize).max(1),
                    false,
                )
            } else {
                (1, true)
            }
        }
        PART_KIND_IMAGE => (1, false),
        _ => (1, false),
    }
}

/// Estimates the rendered row count of an unloaded message from its [`MessageLayoutMeta`].
pub fn estimate_lines(
    meta: &MessageLayoutMeta,
    width: usize,
    details: bool,
    thinking: bool,
    compact: bool,
) -> usize {
    let w = width.max(MIN_ESTIMATE_WIDTH);
    if compact {
        return estimate_compact_lines(meta, w);
    }
    match meta.role {
        Role::User => {
            if meta.source_kind == SOURCE_KIND_COMPACTION {
                return COMPACTION_ESTIMATE_ROWS;
            }
            let inner = w.saturating_sub(USER_BUBBLE_CHROME_COLS).max(1);
            let mut body = 0usize;
            let mut has_image = false;
            for p in &meta.parts {
                match p.kind.as_str() {
                    PART_KIND_TEXT => {
                        let wrapped = (p.chars as usize)
                            .div_ceil(inner)
                            .max(p.lines as usize)
                            .max(1);
                        body += wrapped;
                    }
                    PART_KIND_IMAGE => has_image = true,
                    _ => {}
                }
            }
            if body == 0 && !has_image {
                body = 1;
            }
            USER_BUBBLE_OVERHEAD_ROWS + body + usize::from(has_image)
        }
        Role::Assistant => {
            let inner = w
                .saturating_sub(ASSISTANT_CONTENT_PAD)
                .max(MIN_ASSISTANT_INNER_COLS);
            let mut rows = 0usize;
            let mut prev_single = false;
            for (idx, p) in meta.parts.iter().enumerate() {
                let (part_rows, single) = estimate_part_rows(p, inner, details, thinking);
                if idx > 0 && !(prev_single && single) {
                    rows += 1;
                }
                prev_single = single;
                rows += part_rows;
            }
            if meta.has_error {
                rows += ERROR_BANNER_ROWS;
            }
            (rows + ASSISTANT_FOOTER_ROWS).max(1)
        }
    }
}

fn estimate_compact_lines(meta: &MessageLayoutMeta, w: usize) -> usize {
    match meta.role {
        Role::User => {
            if meta.source_kind == SOURCE_KIND_COMPACTION {
                return 1;
            }
            let inner = w.saturating_sub(COMPACT_USER_INDENT_COLS).max(1);
            let lines: usize = meta
                .parts
                .iter()
                .filter(|p| p.kind == PART_KIND_TEXT)
                .map(|p| (p.chars as usize).div_ceil(inner).max(p.lines as usize))
                .sum();
            let capped = lines.clamp(1, COMPACT_USER_MAX_LINES);
            1 + capped + usize::from(lines > COMPACT_USER_MAX_LINES)
        }
        Role::Assistant => {
            let mut rows = 0usize;
            for p in &meta.parts {
                rows += match p.kind.as_str() {
                    PART_KIND_TEXT => (p.chars as usize).div_ceil(w).max(p.lines as usize).max(1),
                    _ => 1,
                };
            }
            rows += usize::from(meta.has_error);
            rows.max(1)
        }
    }
}

fn fallback_meta(seq: u64) -> MessageLayoutMeta {
    MessageLayoutMeta {
        seq,
        id: format!("seq-{seq}"),
        role: Role::Assistant,
        source_kind: String::new(),
        has_error: false,
        parts: Vec::new(),
    }
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
        Queued {
            key: next_version(),
            id: None,
            text,
            images,
            removing: false,
            from: None,
        }
    }

    fn from_message(m: &Message) -> Self {
        let images = m
            .parts
            .iter()
            .filter_map(|p| {
                if let Part::Image { image, .. } = p {
                    Some(image.clone())
                } else {
                    None
                }
            })
            .collect();
        let mut q = Queued::new(m.raw_text(), images);
        q.id = Some(m.id.clone());
        q.from = match &m.source {
            MessageSource::Human => None,
            MessageSource::Peer { from_name, .. } | MessageSource::Spawn { from_name, .. } => {
                Some(from_name.clone())
            }
            MessageSource::Reminder => Some("reminder".into()),
            MessageSource::Shell => Some("shell".into()),
            MessageSource::Compaction => Some("compaction".into()),
        };
        q
    }
}

pub struct SessionView {
    pub info: SessionInfo,
    pub slots: Vec<MessageSlot>,
    /// Parallel to `slots`: bumped on every change (render cache key).
    pub versions: Vec<u64>,
    index: HashMap<String, usize>,
    pub permissions: Vec<PermissionRequest>,
    pub questions: Vec<QuestionRequest>,
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
    /// Total message sequence count reported by the server.
    pub total_messages: u64,
    /// Unloaded messages remain before the resident window.
    pub more_before: bool,
    /// An in-flight `Request::Messages` range query: `(history_gen, start_seq, end_seq)`.
    pub range_pending: Option<(u64, usize, usize)>,
    /// Debounced range query waiting to fire: `(fire_at, start_seq, end_seq)`.
    pub range_due: Option<(Instant, usize, usize)>,
    /// Stopped asking for ranges after an error until the next snapshot or reset.
    pub range_halted: bool,
    /// Set when slots materialized or evicted outside follow mode so scroll anchor re-syncs.
    pub needs_anchor_restore: bool,
    /// Bumped whenever the message list is replaced; stale page responses are dropped.
    pub history_gen: u64,
}

impl SessionView {
    pub fn new(info: SessionInfo, foreground: bool) -> Self {
        let busy = !info.status.is_idle();
        SessionView {
            info,
            slots: Vec::new(),
            versions: Vec::new(),
            index: HashMap::new(),
            permissions: Vec::new(),
            questions: Vec::new(),
            loaded: false,
            busy_since: busy.then(Instant::now),
            queued: Vec::new(),
            error: None,
            closed: false,
            chat: ChatState::default(),
            foreground,
            total_messages: 0,
            more_before: false,
            range_pending: None,
            range_due: None,
            range_halted: false,
            needs_anchor_restore: false,
            history_gen: 0,
        }
    }

    pub fn is_busy(&self) -> bool {
        !self.info.status.is_idle()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn message_at(&self, seq: usize) -> Option<&Message> {
        self.slots.get(seq).and_then(MessageSlot::as_message)
    }

    pub fn message_at_mut(&mut self, seq: usize) -> Option<&mut Message> {
        self.slots
            .get_mut(seq)
            .and_then(MessageSlot::as_message_mut)
    }

    pub fn last_message(&self) -> Option<&Message> {
        self.slots.iter().rev().find_map(MessageSlot::as_message)
    }

    pub fn materialized_iter(&self) -> impl DoubleEndedIterator<Item = &Message> {
        self.slots.iter().filter_map(MessageSlot::as_message)
    }

    pub fn resident_count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_materialized()).count()
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
        for (i, slot) in self.slots.iter().enumerate() {
            self.index.insert(slot.id().to_string(), i);
        }
    }

    pub fn load_snapshot(&mut self, snap: SessionSnapshot) {
        self.set_info(snap.info);
        self.set_queued(snap.queued);
        self.set_window(
            snap.messages,
            snap.more_before,
            snap.total_messages,
            snap.start_seq,
            snap.layout_metas,
        );
        self.permissions = snap.pending_permissions;
        self.questions = snap.pending_questions;
        self.loaded = true;
        self.closed = false;
    }

    fn set_window(
        &mut self,
        msgs: Vec<Message>,
        more_before: bool,
        total_messages: u64,
        start_seq: u64,
        layout_metas: Vec<MessageLayoutMeta>,
    ) {
        let start = start_seq as usize;
        let total = (total_messages as usize).max(start + msgs.len());
        let mut slots: Vec<MessageSlot> = (0..total)
            .map(|seq| MessageSlot::Placeholder(fallback_meta(seq as u64)))
            .collect();
        for meta in layout_metas {
            let seq = meta.seq as usize;
            if seq < start && seq < total {
                slots[seq] = MessageSlot::Placeholder(meta);
            }
        }
        for (offset, m) in msgs.into_iter().enumerate() {
            if start + offset < total {
                slots[start + offset] = MessageSlot::Materialized(Box::new(m));
            }
        }
        self.slots = slots;
        self.versions = self.slots.iter().map(|_| next_version()).collect();
        self.total_messages = total as u64;
        self.more_before = more_before || self.slots.first().is_some_and(|s| !s.is_materialized());
        self.range_pending = None;
        self.range_due = None;
        self.range_halted = false;
        self.needs_anchor_restore = false;
        self.history_gen += 1;
        self.reindex();
        self.drop_matched_queued();
    }

    /// Computes `[start, end)` sequence range to load for the current viewport, if any.
    pub fn desired_range(&self) -> Option<(usize, usize)> {
        if !self.loaded
            || self.range_halted
            || self.range_pending.is_some()
            || self.slots.is_empty()
        {
            return None;
        }
        let h = self.chat.area.height as usize;
        if h == 0 {
            return None;
        }
        let (vis_start, vis_end) = self.chat.visible_seq_range?;
        let len = self.slots.len();
        let vs = vis_start.min(len);
        let ve = vis_end.min(len);
        let at_top = self.chat.top() <= h;
        let probe_start = if at_top { vs.saturating_sub(2) } else { vs };
        let mut first_p = None;
        let mut last_p = None;
        for (seq, slot) in self.slots.iter().enumerate().take(ve).skip(probe_start) {
            if !slot.is_materialized() {
                if first_p.is_none() {
                    first_p = Some(seq);
                }
                last_p = Some(seq);
            }
        }
        let (first_p, last_p) = match (first_p, last_p) {
            (Some(a), Some(b)) => (a, b),
            _ => return None,
        };
        let mut gap_start = first_p;
        while gap_start > 0 && !self.slots[gap_start - 1].is_materialized() {
            gap_start -= 1;
        }
        let mut gap_end = last_p + 1;
        while gap_end < len && !self.slots[gap_end].is_materialized() {
            gap_end += 1;
        }
        if gap_end - gap_start <= PAGE_SIZE {
            return Some((gap_start, gap_end));
        }
        // Scrolling up into a gap immediately above materialized messages: fetch the tail of the gap.
        if last_p + 1 == gap_end && gap_end < len {
            let start = gap_end.saturating_sub(PAGE_SIZE).max(gap_start);
            return Some((start, gap_end));
        }
        if first_p == gap_start {
            return Some((gap_start, (gap_start + PAGE_SIZE).min(gap_end)));
        }
        let start = first_p
            .saturating_sub(PAGE_SIZE / JUMP_LOOKBACK_DIVISOR)
            .max(gap_start);
        let end = (start + PAGE_SIZE).min(gap_end);
        let start = end.saturating_sub(PAGE_SIZE).max(gap_start);
        Some((start, end))
    }

    /// Materializes messages returned by `Request::Messages` at `start_seq`.
    pub fn apply_messages_range(
        &mut self,
        generation: u64,
        start_seq: usize,
        messages: Vec<Message>,
        more_before: bool,
    ) -> bool {
        if generation != self.history_gen {
            return false;
        }
        self.range_pending = None;
        self.range_halted = false;
        if messages.is_empty() {
            if start_seq == 0 {
                self.more_before = more_before;
            }
            return false;
        }
        let mut changed = false;
        for (offset, msg) in messages.into_iter().enumerate() {
            let seq = start_seq + offset;
            if seq >= self.slots.len() {
                break;
            }
            let old_id = self.slots[seq].id();
            if old_id != msg.id {
                let old_id = old_id.to_string();
                self.index.remove(&old_id);
                self.index.insert(msg.id.clone(), seq);
            } else {
                self.index.entry(msg.id.clone()).or_insert(seq);
            }
            self.slots[seq] = MessageSlot::Materialized(Box::new(msg));
            self.versions[seq] = next_version();
            changed = true;
        }
        if changed {
            self.needs_anchor_restore = true;
        }
        self.more_before = self.slots.first().is_some_and(|s| !s.is_materialized())
            || (start_seq == 0 && more_before);
        changed
    }

    /// Evicts materialized messages outside `[visible_start - 150, visible_end + 150]` back to
    /// `Placeholder` when resident count exceeds `MAX_RESIDENT_MESSAGES`.
    pub fn evict_outside(&mut self, visible_start: usize, visible_end: usize) {
        let mut resident = self.resident_count();
        if resident <= MAX_RESIDENT_MESSAGES {
            return;
        }
        let len = self.slots.len();
        let vs = visible_start.min(len);
        let ve = visible_end.min(len);
        let keep_start = vs.saturating_sub(EVICT_MARGIN);
        let keep_end = (ve + EVICT_MARGIN).min(len);
        let mut evicted_any = false;
        for seq in 0..len {
            if resident <= MAX_RESIDENT_MESSAGES {
                break;
            }
            if seq >= keep_start && seq < keep_end {
                continue;
            }
            if self.evict_slot(seq) {
                resident -= 1;
                evicted_any = true;
            }
        }
        if resident > MAX_RESIDENT_MESSAGES {
            let center = (vs + ve) / 2;
            let mut candidates: Vec<usize> = (0..len)
                .filter(|&seq| (seq < vs || seq >= ve) && self.slots[seq].is_materialized())
                .collect();
            candidates.sort_by_key(|&seq| std::cmp::Reverse(seq.abs_diff(center)));
            for seq in candidates {
                if resident <= MAX_RESIDENT_MESSAGES {
                    break;
                }
                if self.evict_slot(seq) {
                    resident -= 1;
                    evicted_any = true;
                }
            }
        }
        if evicted_any {
            self.needs_anchor_restore = true;
            self.more_before = self.slots.first().is_some_and(|s| !s.is_materialized());
        }
    }

    /// Trims resident messages older than the newest `keep_tail` back to placeholders.
    pub fn trim_to_tail(&mut self, keep_tail: usize) {
        let cutoff = self.slots.len().saturating_sub(keep_tail);
        if cutoff == 0 {
            return;
        }
        for seq in 0..cutoff {
            self.evict_slot(seq);
        }
        self.more_before = self.slots.first().is_some_and(|s| !s.is_materialized());
    }

    fn evict_slot(&mut self, seq: usize) -> bool {
        let Some(msg) = self.slots.get(seq).and_then(MessageSlot::as_message) else {
            return false;
        };
        let meta = compute_layout_meta(seq as u64, msg);
        self.slots[seq] = MessageSlot::Placeholder(meta);
        if let Some(v) = self.versions.get_mut(seq) {
            *v = next_version();
        }
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

    /// Slot index of a message by id.
    pub fn find(&self, id: &str) -> Option<usize> {
        self.index
            .get(id)
            .copied()
            .filter(|&i| i < self.slots.len())
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
                    self.slots[i] = MessageSlot::Materialized(Box::new(message));
                    self.touch(i);
                } else {
                    self.index.insert(message.id.clone(), self.slots.len());
                    self.slots
                        .push(MessageSlot::Materialized(Box::new(message)));
                    self.versions.push(next_version());
                    self.total_messages = self.slots.len() as u64;
                }
                self.drop_matched_queued();
            }
            SessionEvent::Part {
                message_id,
                index,
                part,
            } => {
                let Some(i) = self.find(&message_id) else {
                    return false;
                };
                let Some(msg) = self.message_at_mut(i) else {
                    return false;
                };
                let parts = &mut msg.parts;
                if index < parts.len() {
                    parts[index] = part;
                } else {
                    // Gaps should not happen; append to keep order robust.
                    parts.push(part);
                }
                self.touch(i);
            }
            SessionEvent::Delta {
                message_id,
                index,
                delta,
            } => {
                let Some(i) = self.find(&message_id) else {
                    return false;
                };
                let Some(msg) = self.message_at_mut(i) else {
                    return false;
                };
                apply_delta(&mut msg.parts, index, delta);
                self.touch(i);
            }
            SessionEvent::Removed { message_id } => {
                if let Some(i) = self.find(&message_id) {
                    self.slots.remove(i);
                    self.versions.remove(i);
                    self.index.remove(&message_id);
                    for (j, slot) in self.slots.iter_mut().enumerate().skip(i) {
                        if let MessageSlot::Placeholder(meta) = slot {
                            meta.seq = j as u64;
                        }
                        if let Some(idx) = self.index.get_mut(slot.id()) {
                            *idx = j;
                        }
                    }
                    self.total_messages = self.slots.len() as u64;
                }
            }
            SessionEvent::Reset {
                messages,
                more_before,
                total_messages,
                start_seq,
                layout_metas,
            } => {
                self.set_window(
                    messages,
                    more_before,
                    total_messages,
                    start_seq,
                    layout_metas,
                );
            }
            SessionEvent::Permission { request } => {
                if !self
                    .permissions
                    .iter()
                    .any(|p| p.call_id == request.call_id)
                {
                    self.permissions.push(request);
                }
            }
            SessionEvent::PermissionResolved { call_id } => {
                self.permissions.retain(|p| p.call_id != call_id);
            }
            SessionEvent::Question { request } => {
                if !self.questions.iter().any(|q| q.call_id == request.call_id) {
                    self.questions.push(request);
                }
            }
            SessionEvent::QuestionResolved { call_id } => {
                self.questions.retain(|q| q.call_id != call_id);
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
            .slots
            .iter()
            .rev()
            .take(QUEUED_MATCH_WINDOW)
            .filter_map(MessageSlot::as_message)
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
    fn set_queued(&mut self, queue: Vec<Message>) {
        let mut old = std::mem::take(&mut self.queued);
        for m in queue {
            let mine = || {
                old.iter().position(|q| {
                    q.id.is_none()
                        && m.source == MessageSource::Human
                        && q.text.trim() == m.raw_text().trim()
                })
            };
            let q = match old
                .iter()
                .position(|q| q.id.as_deref() == Some(m.id.as_str()))
                .or_else(mine)
            {
                Some(i) => {
                    let mut q = old.remove(i);
                    q.id = Some(m.id.clone());
                    q
                }
                None => Queued::from_message(&m),
            };
            self.queued.push(q);
        }
        self.queued
            .extend(old.into_iter().filter(|q| q.id.is_none()));
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
    m.parts
        .iter()
        .any(|p| matches!(p, Part::Tool { state, .. } if !state.is_final()))
}

#[cfg(test)]
mod tests {
    use ref_proto::Delta;

    use super::*;

    fn info() -> SessionInfo {
        SessionInfo {
            name: "n".into(),
            agent: "build".into(),
            attached: 1,
            ..crate::util::placeholder_info("s", "/")
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
        v.apply(SessionEvent::Message {
            message: msg("a", Role::Assistant),
        });
        v.apply(SessionEvent::Delta {
            message_id: "a".into(),
            index: 0,
            delta: Delta::Reasoning { text: "hm".into() },
        });
        v.apply(SessionEvent::Delta {
            message_id: "a".into(),
            index: 0,
            delta: Delta::Reasoning { text: "m".into() },
        });
        v.apply(SessionEvent::Part {
            message_id: "a".into(),
            index: 1,
            part: Part::Text {
                text: "Hi".into(),
                time: 1,
            },
        });
        v.apply(SessionEvent::Delta {
            message_id: "a".into(),
            index: 1,
            delta: Delta::Text {
                text: " there".into(),
            },
        });
        v.apply(SessionEvent::Delta {
            message_id: "a".into(),
            index: 2,
            delta: Delta::ToolInput {
                text: r#"{"purpose": "Run te"#.into(),
            },
        });
        let m = v.message_at(0).unwrap();
        assert!(matches!(&m.parts[0], Part::Reasoning { text, .. } if text == "hmm"));
        assert_eq!(m.raw_text(), "Hi there");
        assert!(matches!(&m.parts[2], Part::Tool { purpose, .. } if purpose == "Run te"));
        // Unknown message: ignored, no panic.
        assert!(!v.apply(SessionEvent::Delta {
            message_id: "zz".into(),
            index: 5,
            delta: Delta::Text { text: "x".into() }
        }));
        v.apply(SessionEvent::Removed {
            message_id: "a".into(),
        });
        assert!(v.is_empty() && v.find("a").is_none());
    }

    #[test]
    fn queued_matching() {
        let mut v = SessionView::new(info(), true);
        v.queued.push(Queued::new("do it".into(), vec![]));
        let mut m = msg("u", Role::User);
        m.parts.push(Part::Text {
            text: "do it".into(),
            time: 0,
        });
        v.apply(SessionEvent::Message { message: m });
        assert!(v.queued.is_empty());
        // With a known id only that message counts, not one with the same text.
        let mut q = Queued::new("again".into(), vec![]);
        q.id = Some("u3".into());
        v.queued.push(q);
        let mut m = msg("u2", Role::User);
        m.parts.push(Part::Text {
            text: "again".into(),
            time: 0,
        });
        v.apply(SessionEvent::Message { message: m });
        assert_eq!(v.queued.len(), 1);
        let mut m = msg("u3", Role::User);
        m.parts.push(Part::Text {
            text: "again".into(),
            time: 0,
        });
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
        q2.parts.push(Part::Text {
            text: "from snapshot".into(),
            time: 0,
        });
        q2.parts.push(Part::Image {
            image: ImageData {
                mime: "image/png".into(),
                data: "x".into(),
                ..Default::default()
            },
            name: None,
        });
        let mut q1 = msg("q1", Role::User);
        q1.parts.push(Part::Text {
            text: "kept".into(),
            time: 0,
        });
        v.load_snapshot(SessionSnapshot {
            info: info(),
            messages: vec![],
            more_before: false,
            total_messages: 0,
            start_seq: 0,
            layout_metas: vec![],
            pending_permissions: vec![],
            pending_questions: vec![],
            queued: vec![q2, q1],
        });
        let rows: Vec<(Option<&str>, &str, usize)> = v
            .queued
            .iter()
            .map(|q| (q.id.as_deref(), q.text.as_str(), q.images.len()))
            .collect();
        assert_eq!(
            rows,
            [
                (Some("q2"), "from snapshot", 1),
                (Some("q1"), "kept", 0),
                (None, "waiting for id", 0)
            ]
        );
        assert!(
            v.queued[1].removing && v.queued[1].key == kept.key,
            "existing row state kept"
        );
    }

    #[test]
    fn estimate_lines_accounts_for_width_code_tables_and_toggles() {
        let user_meta = MessageLayoutMeta {
            seq: 0,
            id: "u0".into(),
            role: Role::User,
            source_kind: "human".into(),
            has_error: false,
            parts: vec![PartLayoutMeta {
                kind: "text".into(),
                chars: 120,
                lines: 1,
                code_blocks: 0,
                code_lines: 0,
                tables: 0,
                tool: None,
                tool_lines: 0,
                width: None,
                height: None,
            }],
        };
        let wide = estimate_lines(&user_meta, 100, false, false, false);
        let narrow = estimate_lines(&user_meta, 30, false, false, false);
        assert!(narrow > wide, "narrow={narrow} wide={wide}");

        let asst_meta = MessageLayoutMeta {
            seq: 1,
            id: "a1".into(),
            role: Role::Assistant,
            source_kind: "human".into(),
            has_error: false,
            parts: vec![
                PartLayoutMeta {
                    kind: "reasoning".into(),
                    chars: 200,
                    lines: 3,
                    code_blocks: 0,
                    code_lines: 0,
                    tables: 0,
                    tool: None,
                    tool_lines: 0,
                    width: None,
                    height: None,
                },
                PartLayoutMeta {
                    kind: "tool".into(),
                    chars: 80,
                    lines: 12,
                    code_blocks: 0,
                    code_lines: 0,
                    tables: 0,
                    tool: Some("bash".into()),
                    tool_lines: 12,
                    width: None,
                    height: None,
                },
                PartLayoutMeta {
                    kind: "text".into(),
                    chars: 150,
                    lines: 4,
                    code_blocks: 1,
                    code_lines: 2,
                    tables: 1,
                    tool: None,
                    tool_lines: 0,
                    width: None,
                    height: None,
                },
            ],
        };
        let collapsed = estimate_lines(&asst_meta, 80, false, false, false);
        let expanded = estimate_lines(&asst_meta, 80, true, true, false);
        assert!(
            expanded > collapsed,
            "expanded={expanded} collapsed={collapsed}"
        );
    }

    #[test]
    fn eviction_and_trim_bound_resident_messages() {
        let mut v = SessionView::new(info(), true);
        let total = 450usize;
        let msgs: Vec<Message> = (0..total)
            .map(|i| {
                let mut m = msg(&format!("m{i}"), Role::User);
                m.parts.push(Part::Text {
                    text: format!("message {i}"),
                    time: 0,
                });
                m
            })
            .collect();
        v.load_snapshot(SessionSnapshot {
            info: info(),
            messages: msgs,
            more_before: false,
            total_messages: total as u64,
            start_seq: 0,
            layout_metas: vec![],
            pending_permissions: vec![],
            pending_questions: vec![],
            queued: vec![],
        });
        assert_eq!(v.resident_count(), total);

        // Evict keeping a window around [10..20] -> resident count <= MAX_RESIDENT_MESSAGES.
        v.evict_outside(10, 20);
        assert!(v.resident_count() <= MAX_RESIDENT_MESSAGES);
        assert!(v.slots[15].is_materialized());
        assert!(!v.slots[440].is_materialized());

        // Trim to tail keeps only newest TAIL_KEEP_MESSAGES.
        v.slots[440] = MessageSlot::Materialized(Box::new(msg("m440", Role::User)));
        v.trim_to_tail(TAIL_KEEP_MESSAGES);
        assert!(!v.slots[15].is_materialized());
        assert!(v.slots[440].is_materialized());
        assert!(v.more_before);
    }
    #[test]
    fn removed_shifts_trailing_placeholder_seqs_and_index() {
        let mut v = SessionView::new(info(), true);
        let m0 = msg("m0", Role::User);
        let m1 = msg("m1", Role::Assistant);
        let m2 = msg("m2", Role::User);
        let mut p1 = compute_layout_meta(1, &m1);
        p1.id = "p1".into();
        let mut p2 = compute_layout_meta(2, &m2);
        p2.id = "p2".into();
        v.load_snapshot(SessionSnapshot {
            info: info(),
            messages: vec![m0],
            more_before: false,
            total_messages: 1,
            start_seq: 0,
            layout_metas: vec![],
            pending_permissions: vec![],
            pending_questions: vec![],
            queued: vec![],
        });
        v.slots.push(MessageSlot::Placeholder(p1));
        v.versions.push(next_version());
        v.slots.push(MessageSlot::Placeholder(p2));
        v.versions.push(next_version());
        v.total_messages = 3;
        v.reindex();

        assert!(v.apply(SessionEvent::Removed {
            message_id: "m0".into()
        }));
        assert_eq!(v.find("m0"), None);
        assert_eq!(v.find("p1"), Some(0));
        assert_eq!(v.find("p2"), Some(1));
        assert_eq!(v.slots[0].as_meta().unwrap().seq, 0);
        assert_eq!(v.slots[1].as_meta().unwrap().seq, 1);
    }
}
