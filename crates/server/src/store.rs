//! Session persistence in SQLite (`<data_dir>/ref.db`).
//!
//! All database access happens on one dedicated thread that owns the connection; callers
//! talk to it through an mpsc queue. Saves are fire-and-forget (and coalesced per session),
//! reads block the caller until the reply arrives (use `spawn_blocking` from async code).
//! Because every operation goes through the same FIFO queue, a read always observes the
//! writes queued before it.
//!
//! Saves are incremental: the thread keeps, per session, a hash of every persisted message
//! (by position) and of the info row; only changed rows are written and rows beyond the
//! current length are deleted (undo). One save = one transaction.
//!
//! Legacy JSON files (`<data_dir>/sessions/<id>.json`) are imported once on startup; the
//! directory is then renamed to `sessions.imported`.

use ref_proto::{Message, MessageLayoutMeta, MessageSource, Part, PartLayoutMeta, SessionInfo};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter, types::Value as SqlValue};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::Hasher;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

const SCHEMA_VERSION: i64 = 3;

/// Database file name inside the data directory.
const DB_FILE: &str = "ref.db";

/// Longest accepted session id.
const MAX_ID_LEN: usize = 128;

/// How long SQLite waits for a lock held by another connection.
const DB_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// SQLite's `LIMIT -1`: no limit.
const NO_LIMIT: i64 = -1;

/// Minimum character count required for FTS5 trigram queries.
const FTS_TRIGRAM_MIN_CHARS: usize = 3;

const STORE_UNAVAILABLE: &str = "session store is not available";

/// Full persisted state of one session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Persisted {
    pub info: SessionInfo,
    pub messages: Vec<Message>,
    /// must_report bookkeeping.
    #[serde(default)]
    pub reported: bool,
    /// The recommend-compaction notice was already injected in the current context epoch.
    #[serde(default)]
    pub context_notice: bool,
    /// Whether the session's model was explicitly chosen rather than inherited from defaults.
    #[serde(default)]
    pub model_explicit: bool,
    /// 0-based sequence index of `messages[0]`.
    #[serde(default)]
    pub window_start_seq: u64,
    /// Persist the session row even when `messages` is empty.
    #[serde(default)]
    pub force_persist: bool,
}

/// Raw persisted stash group row + ordered member `(session_id, position, info)`.
#[derive(Debug, Clone)]
pub(crate) struct RawGroup {
    pub id: String,
    pub name: String,
    pub locked: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub members: Vec<(String, u32, SessionInfo)>,
}

/// Session state besides info/messages, stored as JSON in `sessions.extra`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Extra {
    #[serde(default)]
    reported: bool,
    #[serde(default)]
    context_notice: bool,
    #[serde(default)]
    model_explicit: bool,
}

/// Error of a database operation; it reaches callers as its message.
#[derive(Debug)]
struct StoreError(String);

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError(e.to_string())
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(e: serde_json::Error) -> Self {
        StoreError(e.to_string())
    }
}

impl From<String> for StoreError {
    fn from(e: String) -> Self {
        StoreError(e)
    }
}

type DbResult<T> = Result<T, StoreError>;

type Reply<T> = mpsc::Sender<Result<T, String>>;

enum Cmd {
    Save(Box<Persisted>),
    Load(String, Reply<Persisted>),
    LoadActiveWindow(String, Reply<Persisted>),
    LoadMessage(String, String, Reply<Option<Message>>),
    LoadLayoutMetas(String, u64, u64, Reply<Vec<MessageLayoutMeta>>),
    LoadMessagesRange(String, u64, u64, Reply<Vec<Message>>),
    LoadMessagesBefore {
        session_id: String,
        before_seq: u64,
        limit: usize,
        max_bytes: usize,
        reply: Reply<(Vec<Message>, u64, u64)>,
    },
    MessageSeq(String, String, Reply<Option<u64>>),
    LastHumanSeqBefore(String, u64, Reply<Option<u64>>),
    CompactionSeqBefore(String, u64, Reply<u64>),
    Exists(String, Reply<bool>),
    ResolveId(String, Reply<String>),
    IdsByName(String, Reply<Vec<String>>),
    CountExcluding {
        creator: Option<String>,
        creator_ids: Vec<String>,
        exclude_ids: Vec<String>,
        reply: Reply<usize>,
    },
    PageExcluding {
        creator: Option<String>,
        creator_ids: Vec<String>,
        exclude_ids: Vec<String>,
        limit: usize,
        offset: usize,
        reply: Reply<Vec<SessionInfo>>,
    },
    List {
        cwd: Option<String>,
        creator: Option<String>,
        extra_creator_ids: Vec<String>,
        query: Option<String>,
        limit: Option<usize>,
        exclude_ids: Vec<String>,
        reply: Reply<Vec<SessionInfo>>,
    },
    Infos(Vec<String>, Reply<Vec<SessionInfo>>),
    DeleteSession(String, Reply<bool>),
    SetStashedResume(String, Option<String>, Reply<()>),
    TakeStashedResume(String, Reply<Option<String>>),
    ListGroups(Reply<Vec<RawGroup>>),
    CreateGroup {
        id: String,
        name: String,
        session_ids: Vec<String>,
        reply: Reply<Vec<RawGroup>>,
    },
    RenameGroup {
        group_id: String,
        name: String,
        reply: Reply<Vec<RawGroup>>,
    },
    SetGroupLocked {
        group_id: String,
        locked: bool,
        reply: Reply<Vec<RawGroup>>,
    },
    DeleteGroup {
        group_id: String,
        reply: Reply<Vec<RawGroup>>,
    },
    AddToGroup {
        group_id: String,
        session_id: String,
        position: Option<u32>,
        reply: Reply<Vec<RawGroup>>,
    },
    MoveGroupMember {
        from_group_id: Option<String>,
        to_group_id: String,
        session_id: String,
        position: u32,
        reply: Reply<Vec<RawGroup>>,
    },
    RemoveFromGroup {
        group_id: String,
        session_id: String,
        reply: Reply<Vec<RawGroup>>,
    },
    MergeGroups {
        source_group_id: String,
        target_group_id: String,
        reply: Reply<Vec<RawGroup>>,
    },
    FinishRestoreGroup {
        group_id: String,
        reply: Reply<Vec<RawGroup>>,
    },
    FinishRestoreGroupMember {
        group_id: String,
        session_id: String,
        reply: Reply<Vec<RawGroup>>,
    },
    Flush(Reply<()>),
    Close(Reply<()>),
}

#[derive(Debug, Clone)]
pub struct Store {
    pub path: PathBuf,
    tx: mpsc::Sender<Cmd>,
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub(crate) fn normalize_filter(value: Option<String>) -> Option<String> {
    value.and_then(|v| {
        let trimmed = v.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}
impl Store {
    /// Opens (or creates) `<data_dir>/ref.db` on a background thread.
    pub fn new(data_dir: PathBuf) -> Store {
        let path = data_dir.join(DB_FILE);
        let (tx, rx) = mpsc::channel();
        let p2 = path.clone();
        std::thread::Builder::new()
            .name("ref-store".into())
            .spawn(move || db_thread(&data_dir, &p2, rx))
            .expect("spawning the store thread");
        Store { path, tx }
    }

    fn call<T>(&self, f: impl FnOnce(Reply<T>) -> Cmd) -> Result<T, String> {
        let (tx, rx) = mpsc::channel();
        self.tx
            .send(f(tx))
            .map_err(|_| STORE_UNAVAILABLE.to_string())?;
        rx.recv().map_err(|_| STORE_UNAVAILABLE.to_string())?
    }

    /// Queue an incremental save (non-blocking). Sessions without messages that were never
    /// saved are skipped.
    pub fn save(&self, p: Persisted) {
        if !valid_id(&p.info.id) {
            tracing::warn!("not persisting session with invalid id {:?}", p.info.id);
            return;
        }
        if self.tx.send(Cmd::Save(Box::new(p))).is_err() {
            tracing::warn!("session store is gone; save dropped");
        }
    }

    /// Commits what is queued, then closes the database file for good (later saves are
    /// dropped, reads fail). Windows cannot delete a directory with an open SQLite file in it.
    pub fn close(&self) -> Result<(), String> {
        self.call(Cmd::Close)
    }

    /// Wait until every queued write is committed.
    pub fn flush(&self) -> Result<(), String> {
        self.call(Cmd::Flush)
    }

    pub fn load(&self, id: &str) -> Result<Persisted, String> {
        if !valid_id(id) {
            return Err(format!("Invalid session id: {id}"));
        }
        self.call(|r| Cmd::Load(id.to_string(), r))
    }

    pub fn exists(&self, id: &str) -> bool {
        if !valid_id(id) {
            return false;
        }
        self.call(|r| Cmd::Exists(id.to_string(), r))
            .unwrap_or_else(|e| {
                tracing::error!("checking whether session {id} exists failed: {e}");
                false
            })
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        self.delete_session(id).map(|_| ())
    }

    pub(crate) fn delete_session(&self, id: &str) -> Result<bool, String> {
        if !valid_id(id) {
            return Err(format!("Invalid session id: {id}"));
        }
        self.call(|r| Cmd::DeleteSession(id.to_string(), r))
    }

    pub(crate) fn load_active_window(&self, id: &str) -> Result<Persisted, String> {
        if !valid_id(id) {
            return Err(format!("Invalid session id: {id}"));
        }
        self.call(|r| Cmd::LoadActiveWindow(id.to_string(), r))
    }

    pub(crate) fn load_message(
        &self,
        session_id: &str,
        message_id: &str,
    ) -> Result<Option<Message>, String> {
        if !valid_id(session_id) || !valid_id(message_id) {
            return Ok(None);
        }
        self.call(|r| Cmd::LoadMessage(session_id.to_string(), message_id.to_string(), r))
    }

    pub(crate) fn load_layout_metas(
        &self,
        session_id: &str,
        start_seq: u64,
        end_seq: u64,
    ) -> Result<Vec<MessageLayoutMeta>, String> {
        if !valid_id(session_id) || start_seq >= end_seq {
            return Ok(Vec::new());
        }
        self.call(|r| Cmd::LoadLayoutMetas(session_id.to_string(), start_seq, end_seq, r))
    }

    pub(crate) fn load_messages_range(
        &self,
        session_id: &str,
        start_seq: u64,
        end_seq: u64,
    ) -> Result<Vec<Message>, String> {
        if !valid_id(session_id) || start_seq >= end_seq {
            return Ok(Vec::new());
        }
        self.call(|r| Cmd::LoadMessagesRange(session_id.to_string(), start_seq, end_seq, r))
    }

    pub(crate) fn load_messages_before(
        &self,
        session_id: &str,
        before_seq: u64,
        limit: usize,
        max_bytes: usize,
    ) -> Result<(Vec<Message>, u64, u64), String> {
        if !valid_id(session_id) || before_seq == 0 {
            return Ok((Vec::new(), 0, 0));
        }
        self.call(|reply| Cmd::LoadMessagesBefore {
            session_id: session_id.to_string(),
            before_seq,
            limit,
            max_bytes,
            reply,
        })
    }

    pub(crate) fn message_seq(
        &self,
        session_id: &str,
        message_id: &str,
    ) -> Result<Option<u64>, String> {
        if !valid_id(session_id) || !valid_id(message_id) {
            return Ok(None);
        }
        self.call(|r| Cmd::MessageSeq(session_id.to_string(), message_id.to_string(), r))
    }

    pub(crate) fn last_human_seq_before(
        &self,
        session_id: &str,
        before_seq: u64,
    ) -> Result<Option<u64>, String> {
        if !valid_id(session_id) || before_seq == 0 {
            return Ok(None);
        }
        self.call(|r| Cmd::LastHumanSeqBefore(session_id.to_string(), before_seq, r))
    }

    pub(crate) fn compaction_seq_before(
        &self,
        session_id: &str,
        before_seq: u64,
    ) -> Result<u64, String> {
        if !valid_id(session_id) || before_seq == 0 {
            return Ok(0);
        }
        self.call(|r| Cmd::CompactionSeqBefore(session_id.to_string(), before_seq, r))
    }

    pub(crate) fn resolve_id(&self, q: &str) -> Result<String, String> {
        self.call(|r| Cmd::ResolveId(q.trim().to_string(), r))
    }

    pub(crate) fn ids_by_name(&self, name: &str) -> Vec<String> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Vec::new();
        }
        self.call(|r| Cmd::IdsByName(name, r)).unwrap_or_else(|e| {
            tracing::warn!("resolving session ids by name failed: {e}");
            Vec::new()
        })
    }

    pub(crate) fn count_excluding(
        &self,
        creator: Option<String>,
        creator_ids: Vec<String>,
        exclude_ids: Vec<String>,
    ) -> usize {
        let creator = normalize_filter(creator);
        self.call(|reply| Cmd::CountExcluding {
            creator,
            creator_ids,
            exclude_ids,
            reply,
        })
        .unwrap_or_else(|e| {
            tracing::warn!("counting history sessions failed: {e}");
            0
        })
    }

    pub(crate) fn page_excluding(
        &self,
        creator: Option<String>,
        creator_ids: Vec<String>,
        exclude_ids: Vec<String>,
        limit: usize,
        offset: usize,
    ) -> Vec<SessionInfo> {
        if limit == 0 {
            return Vec::new();
        }
        let creator = normalize_filter(creator);
        self.call(|reply| Cmd::PageExcluding {
            creator,
            creator_ids,
            exclude_ids,
            limit,
            offset,
            reply,
        })
        .unwrap_or_else(|e| {
            tracing::warn!("paging history sessions failed: {e}");
            Vec::new()
        })
    }

    /// All persisted session infos, newest `updated_at` first.
    pub fn list(&self) -> Vec<SessionInfo> {
        self.list_for(None, None, None, None)
    }

    /// Persisted session infos matching optional `cwd`, `creator`, and `query`, newest first,
    /// at most `limit`.
    pub(crate) fn list_for(
        &self,
        cwd: Option<String>,
        creator: Option<String>,
        query: Option<String>,
        limit: Option<usize>,
    ) -> Vec<SessionInfo> {
        self.list_for_excluding(cwd, creator, Vec::new(), query, limit, Vec::new())
    }

    pub(crate) fn list_for_excluding(
        &self,
        cwd: Option<String>,
        creator: Option<String>,
        extra_creator_ids: Vec<String>,
        query: Option<String>,
        limit: Option<usize>,
        exclude_ids: Vec<String>,
    ) -> Vec<SessionInfo> {
        let cwd = normalize_filter(cwd);
        let creator = normalize_filter(creator);
        let query = normalize_filter(query);
        self.call(|reply| Cmd::List {
            cwd,
            creator,
            extra_creator_ids,
            query,
            limit,
            exclude_ids,
            reply,
        })
        .unwrap_or_else(|e| {
            tracing::warn!("listing sessions failed: {e}");
            Vec::new()
        })
    }

    /// Persisted `SessionInfo` rows for the given valid session ids (skips missing/corrupt rows).
    pub(crate) fn infos(&self, ids: Vec<String>) -> Vec<SessionInfo> {
        let ids: Vec<String> = ids.into_iter().filter(|id| valid_id(id)).collect();
        if ids.is_empty() {
            return Vec::new();
        }
        self.call(|r| Cmd::Infos(ids, r)).unwrap_or_else(|e| {
            tracing::warn!("loading session infos failed: {e}");
            Vec::new()
        })
    }

    pub(crate) fn set_stashed_resume(
        &self,
        id: &str,
        resume: Option<String>,
    ) -> Result<(), String> {
        if !valid_id(id) {
            return Err(format!("Invalid session id: {id}"));
        }
        self.call(|r| Cmd::SetStashedResume(id.to_string(), resume, r))
    }

    pub(crate) fn take_stashed_resume(&self, id: &str) -> Result<Option<String>, String> {
        if !valid_id(id) {
            return Ok(None);
        }
        self.call(|r| Cmd::TakeStashedResume(id.to_string(), r))
    }

    pub(crate) fn list_groups_raw(&self) -> Result<Vec<RawGroup>, String> {
        self.call(Cmd::ListGroups)
    }

    pub(crate) fn create_group(
        &self,
        id: String,
        name: String,
        session_ids: Vec<String>,
    ) -> Result<Vec<RawGroup>, String> {
        self.call(|reply| Cmd::CreateGroup {
            id,
            name,
            session_ids,
            reply,
        })
    }

    pub(crate) fn rename_group(
        &self,
        group_id: String,
        name: String,
    ) -> Result<Vec<RawGroup>, String> {
        self.call(|reply| Cmd::RenameGroup {
            group_id,
            name,
            reply,
        })
    }

    pub(crate) fn set_group_locked(
        &self,
        group_id: String,
        locked: bool,
    ) -> Result<Vec<RawGroup>, String> {
        self.call(|reply| Cmd::SetGroupLocked {
            group_id,
            locked,
            reply,
        })
    }

    pub(crate) fn delete_group(&self, group_id: String) -> Result<Vec<RawGroup>, String> {
        self.call(|reply| Cmd::DeleteGroup { group_id, reply })
    }

    pub(crate) fn add_to_group(
        &self,
        group_id: String,
        session_id: String,
        position: Option<u32>,
    ) -> Result<Vec<RawGroup>, String> {
        self.call(|reply| Cmd::AddToGroup {
            group_id,
            session_id,
            position,
            reply,
        })
    }

    pub(crate) fn move_group_member(
        &self,
        from_group_id: Option<String>,
        to_group_id: String,
        session_id: String,
        position: u32,
    ) -> Result<Vec<RawGroup>, String> {
        self.call(|reply| Cmd::MoveGroupMember {
            from_group_id,
            to_group_id,
            session_id,
            position,
            reply,
        })
    }

    pub(crate) fn remove_from_group(
        &self,
        group_id: String,
        session_id: String,
    ) -> Result<Vec<RawGroup>, String> {
        self.call(|reply| Cmd::RemoveFromGroup {
            group_id,
            session_id,
            reply,
        })
    }

    pub(crate) fn merge_groups(
        &self,
        source_group_id: String,
        target_group_id: String,
    ) -> Result<Vec<RawGroup>, String> {
        self.call(|reply| Cmd::MergeGroups {
            source_group_id,
            target_group_id,
            reply,
        })
    }

    pub(crate) fn finish_restore_group(&self, group_id: String) -> Result<Vec<RawGroup>, String> {
        self.call(|reply| Cmd::FinishRestoreGroup { group_id, reply })
    }

    pub(crate) fn finish_restore_group_member(
        &self,
        group_id: String,
        session_id: String,
    ) -> Result<Vec<RawGroup>, String> {
        self.call(|reply| Cmd::FinishRestoreGroupMember {
            group_id,
            session_id,
            reply,
        })
    }
}

// ---------------------------------------------------------------------------
// Database thread
// ---------------------------------------------------------------------------

/// Per-session hashes of what is currently in the database.
#[derive(Default)]
struct Cached {
    info: u64,
    window_start_seq: u64,
    msgs: Vec<u64>,
}

/// `io::Write` adapter feeding a hasher (hashing serialized JSON without allocating).
struct HashWriter(std::hash::DefaultHasher);

impl std::io::Write for HashWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn hash_json<T: Serialize>(v: &T) -> u64 {
    let mut w = HashWriter(std::hash::DefaultHasher::new());
    // `HashWriter::write` never fails and the persisted types serialize infallibly.
    serde_json::to_writer(&mut w, v).expect("persisted types serialize");
    w.0.finish()
}

fn source_kind(src: &MessageSource) -> &'static str {
    match src {
        MessageSource::Human => "human",
        MessageSource::Peer { .. } => "peer",
        MessageSource::Spawn { .. } => "spawn",
        MessageSource::Reminder => "reminder",
        MessageSource::Compaction => "compaction",
        MessageSource::Shell => "shell",
    }
}

fn count_lines(s: &str) -> u32 {
    if s.is_empty() {
        0
    } else {
        s.split('\n').count() as u32
    }
}

fn fence_marker(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim_start();
    let ch = trimmed.chars().next()?;
    if ch != '`' && ch != '~' {
        return None;
    }
    let count = trimmed.chars().take_while(|&c| c == ch).count();
    (count >= 3).then_some((ch, count))
}

fn is_table_separator(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.contains('|') || !trimmed.contains('-') {
        return false;
    }
    trimmed
        .chars()
        .all(|c| matches!(c, '|' | '-' | ':' | ' ' | '\t'))
}

fn analyze_markdown(text: &str) -> (u16, u32, u16) {
    let mut code_blocks: u16 = 0;
    let mut code_lines: u32 = 0;
    let mut tables: u16 = 0;
    let mut fence: Option<(char, usize)> = None;
    let mut prev_has_pipe = false;
    let mut in_table = false;

    for line in text.split('\n') {
        if let Some((fch, flen)) = fence {
            let trimmed = line.trim_start();
            let count = trimmed.chars().take_while(|&c| c == fch).count();
            let closes = count >= flen && trimmed[count * fch.len_utf8()..].trim().is_empty();
            if closes {
                fence = None;
            } else {
                code_lines = code_lines.saturating_add(1);
            }
            prev_has_pipe = false;
            in_table = false;
            continue;
        }
        if let Some(marker) = fence_marker(line) {
            fence = Some(marker);
            code_blocks = code_blocks.saturating_add(1);
            prev_has_pipe = false;
            in_table = false;
            continue;
        }
        let has_pipe = line.contains('|');
        if prev_has_pipe && is_table_separator(line) {
            if !in_table {
                tables = tables.saturating_add(1);
                in_table = true;
            }
        } else if !(in_table && has_pipe) {
            in_table = false;
        }
        prev_has_pipe = has_pipe;
    }
    (code_blocks, code_lines, tables)
}

pub(crate) fn compute_layout_meta(seq: u64, msg: &Message) -> MessageLayoutMeta {
    let parts = msg
        .parts
        .iter()
        .map(|p| match p {
            Part::Text { text, .. } => {
                let (code_blocks, code_lines, tables) = analyze_markdown(text);
                PartLayoutMeta {
                    kind: "text".into(),
                    chars: text.chars().count().min(u32::MAX as usize) as u32,
                    lines: count_lines(text),
                    code_blocks,
                    code_lines,
                    tables,
                    tool: None,
                    tool_lines: 0,
                    width: None,
                    height: None,
                }
            }
            Part::Reasoning { text, .. } => PartLayoutMeta {
                kind: "reasoning".into(),
                chars: text.chars().count().min(u32::MAX as usize) as u32,
                lines: count_lines(text),
                code_blocks: 0,
                code_lines: 0,
                tables: 0,
                tool: None,
                tool_lines: 0,
                width: None,
                height: None,
            },
            Part::Image { .. } => PartLayoutMeta {
                kind: "image".into(),
                chars: 0,
                lines: 0,
                code_blocks: 0,
                code_lines: 0,
                tables: 0,
                tool: None,
                tool_lines: 0,
                width: None,
                height: None,
            },
            Part::Tool {
                name,
                purpose,
                input,
                output,
                meta,
                ..
            } => {
                let lines = if name == crate::toolset::TODO {
                    input
                        .get("todos")
                        .and_then(|v| v.as_array())
                        .map_or(0, Vec::len)
                } else if let Some(diff) = meta
                    .as_ref()
                    .and_then(|m| m.get("diff"))
                    .and_then(|v| v.as_str())
                {
                    diff.lines().count()
                } else {
                    output.as_deref().map_or(0, |o| o.lines().count())
                };
                let tool_lines = lines.min(u16::MAX as usize) as u16;
                let chars = (purpose.chars().count()
                    + output.as_deref().map_or(0, |o| o.chars().count()))
                .min(u32::MAX as usize) as u32;
                PartLayoutMeta {
                    kind: "tool".into(),
                    chars,
                    lines: tool_lines as u32,
                    code_blocks: 0,
                    code_lines: 0,
                    tables: 0,
                    tool: Some(name.clone()),
                    tool_lines,
                    width: None,
                    height: None,
                }
            }
        })
        .collect();

    MessageLayoutMeta {
        seq,
        id: msg.id.clone(),
        role: msg.role,
        source_kind: source_kind(&msg.source).into(),
        has_error: msg.error.is_some(),
        parts,
    }
}

fn open(data_dir: &Path, path: &Path) -> DbResult<Connection> {
    std::fs::create_dir_all(data_dir)
        .map_err(|e| format!("cannot create {}: {e}", data_dir.display()))?;
    let conn =
        Connection::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("cannot restrict permissions of {}: {e}", path.display()))?;
    }
    conn.busy_timeout(DB_BUSY_TIMEOUT)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> DbResult<()> {
    let v: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if v > SCHEMA_VERSION {
        return Err(format!(
            "database schema version {v} is newer than supported ({SCHEMA_VERSION})"
        )
        .into());
    }
    if v < 1 {
        conn.execute_batch(
            "BEGIN;
             CREATE TABLE IF NOT EXISTS sessions (
                 id TEXT PRIMARY KEY,
                 info TEXT NOT NULL,
                 cwd TEXT,
                 title TEXT,
                 name TEXT,
                 created_at INTEGER,
                 updated_at INTEGER,
                 extra TEXT
             );
             CREATE INDEX IF NOT EXISTS sessions_updated ON sessions(updated_at);
             CREATE INDEX IF NOT EXISTS sessions_cwd ON sessions(cwd, updated_at);
             CREATE TABLE IF NOT EXISTS messages (
                 session_id TEXT NOT NULL,
                 seq INTEGER NOT NULL,
                 id TEXT NOT NULL,
                 time INTEGER,
                 data TEXT NOT NULL,
                 PRIMARY KEY (session_id, seq)
             );
             CREATE INDEX IF NOT EXISTS messages_id ON messages(session_id, id);
             PRAGMA user_version = 1;
             COMMIT;",
        )
        .map_err(|e| format!("creating the schema failed: {e}"))?;
    }
    if v < 2 {
        conn.execute_batch(
            "BEGIN;
             ALTER TABLE sessions ADD COLUMN parent TEXT;
             UPDATE sessions SET parent = json_extract(info, '$.parent') WHERE json_valid(info);
             CREATE INDEX IF NOT EXISTS sessions_parent ON sessions(parent, updated_at);
             PRAGMA user_version = 2;
             COMMIT;",
        )
        .map_err(|e| format!("migrating the schema to v2 failed: {e}"))?;
    }
    if v < 3 {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(
            "DROP INDEX IF EXISTS sessions_updated;
             CREATE INDEX IF NOT EXISTS sessions_updated ON sessions(updated_at DESC);
             DROP INDEX IF EXISTS sessions_cwd;
             CREATE INDEX IF NOT EXISTS sessions_cwd ON sessions(cwd, updated_at DESC);
             DROP INDEX IF EXISTS sessions_parent;
             CREATE INDEX IF NOT EXISTS sessions_parent ON sessions(parent, updated_at DESC);
             CREATE INDEX IF NOT EXISTS sessions_name_nocase ON sessions(name COLLATE NOCASE);
             ALTER TABLE sessions ADD COLUMN stashed_resume TEXT;
             ALTER TABLE messages ADD COLUMN layout_meta TEXT;
             CREATE VIRTUAL TABLE IF NOT EXISTS sessions_fts USING fts5(
                 id UNINDEXED, title, name, cwd, session_id, parent, tokenize='trigram'
             );
             CREATE TRIGGER IF NOT EXISTS sessions_ai AFTER INSERT ON sessions BEGIN
                 INSERT INTO sessions_fts(rowid, id, title, name, cwd, session_id, parent)
                 VALUES (
                     new.rowid,
                     new.id,
                     COALESCE(new.title, ''),
                     COALESCE(new.name, ''),
                     COALESCE(new.cwd, ''),
                     COALESCE(new.id, ''),
                     COALESCE(new.parent, '')
                 );
             END;
             CREATE TRIGGER IF NOT EXISTS sessions_au AFTER UPDATE OF title, name, cwd, parent ON sessions BEGIN
                 DELETE FROM sessions_fts WHERE rowid = old.rowid;
                 INSERT INTO sessions_fts(rowid, id, title, name, cwd, session_id, parent)
                 VALUES (
                     new.rowid,
                     new.id,
                     COALESCE(new.title, ''),
                     COALESCE(new.name, ''),
                     COALESCE(new.cwd, ''),
                     COALESCE(new.id, ''),
                     COALESCE(new.parent, '')
                 );
             END;
             CREATE TRIGGER IF NOT EXISTS sessions_ad AFTER DELETE ON sessions BEGIN
                 DELETE FROM sessions_fts WHERE rowid = old.rowid;
             END;
             DELETE FROM sessions_fts;
             INSERT INTO sessions_fts(rowid, id, title, name, cwd, session_id, parent)
             SELECT rowid, id, COALESCE(title, ''), COALESCE(name, ''), COALESCE(cwd, ''), COALESCE(id, ''), COALESCE(parent, '')
             FROM sessions;
             CREATE TABLE IF NOT EXISTS session_groups (
                 id TEXT PRIMARY KEY,
                 name TEXT NOT NULL,
                 locked INTEGER NOT NULL DEFAULT 0,
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS session_group_members (
                 group_id TEXT NOT NULL REFERENCES session_groups(id) ON DELETE CASCADE,
                 session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                 position INTEGER NOT NULL,
                 PRIMARY KEY (group_id, session_id)
             );
             CREATE INDEX IF NOT EXISTS session_group_members_session ON session_group_members(session_id);",
        )
        .map_err(|e| format!("migrating the schema to v3 failed: {e}"))?;

        {
            let mut sel = tx.prepare("SELECT session_id, seq, data FROM messages")?;
            let mut upd = tx.prepare(
                "UPDATE messages SET layout_meta = ?1 WHERE session_id = ?2 AND seq = ?3",
            )?;
            let mut rows = sel.query([])?;
            while let Some(row) = rows.next()? {
                let sid: String = row.get(0)?;
                let seq: i64 = row.get(1)?;
                let data: String = row.get(2)?;
                match serde_json::from_str::<Message>(&data) {
                    Ok(m) => {
                        let meta =
                            serde_json::to_string(&compute_layout_meta(seq.max(0) as u64, &m))?;
                        upd.execute(params![meta, sid, seq])?;
                    }
                    Err(e) => {
                        tracing::warn!(session = %sid, seq, "v3 migration skipping corrupt message layout_meta: {e}");
                    }
                }
            }
        }
        tx.execute_batch("PRAGMA user_version = 3;")
            .map_err(|e| format!("setting schema version 3 failed: {e}"))?;
        tx.commit()?;
    }
    Ok(())
}

fn db_thread(data_dir: &Path, path: &Path, rx: mpsc::Receiver<Cmd>) {
    let mut conn = match open(data_dir, path) {
        Ok(c) => Some(c),
        Err(e) => {
            tracing::error!("session store: {e}");
            None
        }
    };
    if let Some(c) = conn.as_mut()
        && let Err(e) = import_json(c, data_dir)
    {
        tracing::warn!("importing legacy JSON sessions failed: {e}");
    }
    let mut cache: HashMap<String, Cached> = HashMap::new();
    while let Ok(first) = rx.recv() {
        // Coalesce: of several queued saves for the same session only the last one matters
        // (each save carries the full state).
        let mut batch = vec![first];
        while let Ok(c) = rx.try_recv() {
            batch.push(c);
        }
        let skip = superseded_saves(&batch);
        for (c, skip) in batch.into_iter().zip(skip) {
            let Some(db) = conn.as_mut() else {
                fail(c, &format!("{STORE_UNAVAILABLE} (see server log)"));
                continue;
            };
            match c {
                Cmd::Save(_) if skip => {}
                Cmd::Save(p) => {
                    if let Err(e) = save(db, &mut cache, &p) {
                        tracing::warn!(session = %p.info.id, "persisting session failed: {e}");
                        cache.remove(&p.info.id);
                    }
                }
                Cmd::Load(id, r) => {
                    answer(r, load_from(db, &mut cache, &id, false));
                }
                Cmd::LoadActiveWindow(id, r) => {
                    answer(r, load_from(db, &mut cache, &id, true));
                }
                Cmd::LoadMessage(sid, mid, r) => {
                    answer(r, load_message(db, &sid, &mid));
                }
                Cmd::LoadLayoutMetas(sid, start, end, r) => {
                    answer(r, load_layout_metas(db, &sid, start, end));
                }
                Cmd::LoadMessagesRange(sid, start, end, r) => {
                    answer(r, load_messages_range(db, &sid, start, end));
                }
                Cmd::LoadMessagesBefore {
                    session_id,
                    before_seq,
                    limit,
                    max_bytes,
                    reply,
                } => {
                    answer(
                        reply,
                        load_messages_before(db, &session_id, before_seq, limit, max_bytes),
                    );
                }
                Cmd::MessageSeq(sid, mid, r) => {
                    answer(r, message_seq(db, &sid, &mid));
                }
                Cmd::LastHumanSeqBefore(sid, before_seq, r) => {
                    answer(r, last_human_seq_before(db, &sid, before_seq));
                }
                Cmd::CompactionSeqBefore(sid, before_seq, r) => {
                    answer(r, compaction_seq_before(db, &sid, before_seq));
                }
                Cmd::Exists(id, r) => {
                    answer(r, exists(db, &id));
                }
                Cmd::ResolveId(q, r) => {
                    answer(r, resolve_id(db, &q));
                }
                Cmd::IdsByName(name, r) => {
                    answer(r, ids_by_name(db, &name));
                }
                Cmd::CountExcluding {
                    creator,
                    creator_ids,
                    exclude_ids,
                    reply,
                } => {
                    answer(
                        reply,
                        count_excluding(db, creator.as_deref(), &creator_ids, &exclude_ids),
                    );
                }
                Cmd::PageExcluding {
                    creator,
                    creator_ids,
                    exclude_ids,
                    limit,
                    offset,
                    reply,
                } => {
                    answer(
                        reply,
                        page_excluding(
                            db,
                            creator.as_deref(),
                            &creator_ids,
                            &exclude_ids,
                            limit,
                            offset,
                        ),
                    );
                }
                Cmd::List {
                    cwd,
                    creator,
                    extra_creator_ids,
                    query,
                    limit,
                    exclude_ids,
                    reply,
                } => {
                    answer(
                        reply,
                        list(
                            db,
                            cwd.as_deref(),
                            creator.as_deref(),
                            &extra_creator_ids,
                            query.as_deref(),
                            limit,
                            &exclude_ids,
                        ),
                    );
                }
                Cmd::Infos(ids, r) => {
                    answer(r, infos(db, &ids));
                }
                Cmd::DeleteSession(id, r) => {
                    cache.remove(&id);
                    answer(r, delete(db, &id));
                }
                Cmd::SetStashedResume(id, resume, r) => {
                    answer(r, set_stashed_resume(db, &id, resume.as_deref()));
                }
                Cmd::TakeStashedResume(id, r) => {
                    answer(r, take_stashed_resume(db, &id));
                }
                Cmd::ListGroups(r) => {
                    answer(r, list_groups_raw(db));
                }
                Cmd::CreateGroup {
                    id,
                    name,
                    session_ids,
                    reply,
                } => {
                    answer(reply, create_group(db, &id, &name, &session_ids));
                }
                Cmd::RenameGroup {
                    group_id,
                    name,
                    reply,
                } => {
                    answer(reply, rename_group(db, &group_id, &name));
                }
                Cmd::SetGroupLocked {
                    group_id,
                    locked,
                    reply,
                } => {
                    answer(reply, set_group_locked(db, &group_id, locked));
                }
                Cmd::DeleteGroup { group_id, reply } => {
                    answer(reply, delete_group(db, &group_id));
                }
                Cmd::AddToGroup {
                    group_id,
                    session_id,
                    position,
                    reply,
                } => {
                    answer(reply, add_to_group(db, &group_id, &session_id, position));
                }
                Cmd::MoveGroupMember {
                    from_group_id,
                    to_group_id,
                    session_id,
                    position,
                    reply,
                } => {
                    answer(
                        reply,
                        move_group_member(
                            db,
                            from_group_id.as_deref(),
                            &to_group_id,
                            &session_id,
                            position,
                        ),
                    );
                }
                Cmd::RemoveFromGroup {
                    group_id,
                    session_id,
                    reply,
                } => {
                    answer(reply, remove_from_group(db, &group_id, &session_id));
                }
                Cmd::MergeGroups {
                    source_group_id,
                    target_group_id,
                    reply,
                } => {
                    answer(reply, merge_groups(db, &source_group_id, &target_group_id));
                }
                Cmd::FinishRestoreGroup { group_id, reply } => {
                    answer(reply, finish_restore_group(db, &group_id));
                }
                Cmd::FinishRestoreGroupMember {
                    group_id,
                    session_id,
                    reply,
                } => {
                    answer(
                        reply,
                        finish_restore_group_member(db, &group_id, &session_id),
                    );
                }
                Cmd::Flush(r) => answer(r, Ok(())),
                Cmd::Close(r) => {
                    conn = None;
                    cache.clear();
                    answer(r, Ok(()));
                }
            }
        }
    }
}

/// A save is skipped when a later save of the same session follows before any other
/// command (reads must observe the state queued before them). The exception is a save with
/// messages (or `force_persist`) followed by one without: an empty save of a session that
/// has no row yet stores nothing, so the earlier save must run to create the row.
fn superseded_saves(batch: &[Cmd]) -> Vec<bool> {
    let mut skip = vec![false; batch.len()];
    for i in 0..batch.len() {
        let Cmd::Save(p) = &batch[i] else { continue };
        for later in &batch[i + 1..] {
            match later {
                Cmd::Save(q) if q.info.id == p.info.id => {
                    skip[i] = (!p.force_persist || q.force_persist)
                        && (p.messages.is_empty() || !q.messages.is_empty());
                    break;
                }
                Cmd::Save(_) => {}
                _ => break,
            }
        }
    }
    skip
}

/// Sends the result to the caller. A dropped receiver means the caller stopped waiting.
fn answer<T>(reply: Reply<T>, result: DbResult<T>) {
    let _ = reply.send(result.map_err(|e| e.0));
}

fn fail(c: Cmd, msg: &str) {
    let m = StoreError(msg.to_string());
    match c {
        Cmd::Save(_) => {}
        Cmd::Load(_, r) | Cmd::LoadActiveWindow(_, r) => answer(r, Err(m)),
        Cmd::LoadMessage(_, _, r) => answer(r, Err(m)),
        Cmd::LoadLayoutMetas(_, _, _, r) => answer(r, Err(m)),
        Cmd::LoadMessagesRange(_, _, _, r) => answer(r, Err(m)),
        Cmd::LoadMessagesBefore { reply, .. } => answer(reply, Err(m)),
        Cmd::MessageSeq(_, _, r) | Cmd::LastHumanSeqBefore(_, _, r) => answer(r, Err(m)),
        Cmd::CompactionSeqBefore(_, _, r) => answer(r, Err(m)),
        Cmd::Exists(_, r) => answer(r, Err(m)),
        Cmd::ResolveId(_, r) => answer(r, Err(m)),
        Cmd::IdsByName(_, r) => answer(r, Err(m)),
        Cmd::CountExcluding { reply, .. } => answer(reply, Err(m)),
        Cmd::PageExcluding { reply, .. } | Cmd::List { reply, .. } => answer(reply, Err(m)),
        Cmd::Infos(_, r) => answer(r, Err(m)),
        Cmd::DeleteSession(_, r) | Cmd::Flush(r) => answer(r, Err(m)),
        Cmd::SetStashedResume(_, _, r) => answer(r, Err(m)),
        Cmd::TakeStashedResume(_, r) => answer(r, Err(m)),
        Cmd::ListGroups(r)
        | Cmd::CreateGroup { reply: r, .. }
        | Cmd::RenameGroup { reply: r, .. }
        | Cmd::SetGroupLocked { reply: r, .. }
        | Cmd::DeleteGroup { reply: r, .. }
        | Cmd::AddToGroup { reply: r, .. }
        | Cmd::MoveGroupMember { reply: r, .. }
        | Cmd::RemoveFromGroup { reply: r, .. }
        | Cmd::MergeGroups { reply: r, .. }
        | Cmd::FinishRestoreGroup { reply: r, .. }
        | Cmd::FinishRestoreGroupMember { reply: r, .. } => answer(r, Err(m)),
        Cmd::Close(r) => answer(r, Ok(())),
    }
}

fn exists(db: &Connection, id: &str) -> DbResult<bool> {
    let row = db
        .query_row("SELECT 1 FROM sessions WHERE id = ?1", [id], |_| Ok(()))
        .optional()?;
    Ok(row.is_some())
}

fn save(db: &mut Connection, cache: &mut HashMap<String, Cached>, p: &Persisted) -> DbResult<()> {
    let id = p.info.id.as_str();
    let extra = Extra {
        reported: p.reported,
        context_notice: p.context_notice,
        model_explicit: p.model_explicit,
    };
    let info_hash = hash_json(&(&p.info, &extra));
    let known = cache.contains_key(id);
    if !known
        && p.messages.is_empty()
        && !p.force_persist
        && p.window_start_seq == 0
        && !exists(db, id)?
    {
        // Never persisted and still empty: nothing worth keeping.
        return Ok(());
    }
    let hashes: Vec<u64> = p.messages.iter().map(hash_json).collect();
    let entry = cache.entry(id.to_string()).or_default();
    let cache_valid = known && entry.window_start_seq == p.window_start_seq;
    let tx = db.transaction()?;
    if !known || entry.info != info_hash {
        let info = serde_json::to_string(&p.info)?;
        let extra = serde_json::to_string(&extra)?;
        tx.execute(
            "INSERT INTO sessions (id, info, cwd, title, name, created_at, updated_at, extra, parent)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET info = excluded.info, cwd = excluded.cwd, title = excluded.title,
                 name = excluded.name, created_at = excluded.created_at, updated_at = excluded.updated_at,
                 extra = excluded.extra, parent = excluded.parent",
            params![
                id,
                info,
                p.info.cwd,
                p.info.title,
                p.info.name,
                p.info.created_at,
                p.info.updated_at,
                extra,
                p.info.parent
            ],
        )?;
    }
    {
        let mut up = tx.prepare_cached(
            "INSERT INTO messages (session_id, seq, id, time, data, layout_meta)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(session_id, seq) DO UPDATE SET id = excluded.id, time = excluded.time,
                 data = excluded.data, layout_meta = excluded.layout_meta",
        )?;
        for (idx, (m, h)) in p.messages.iter().zip(&hashes).enumerate() {
            if cache_valid && entry.msgs.get(idx) == Some(h) {
                continue;
            }
            let seq = p.window_start_seq + idx as u64;
            let data = serde_json::to_string(m)?;
            let meta = serde_json::to_string(&compute_layout_meta(seq, m))?;
            up.execute(params![id, seq as i64, m.id, m.time, data, meta])?;
        }
    }
    let end_seq = p.window_start_seq + hashes.len() as u64;
    let prev_end_seq = entry.window_start_seq + entry.msgs.len() as u64;
    if !cache_valid || prev_end_seq > end_seq {
        tx.execute(
            "DELETE FROM messages WHERE session_id = ?1 AND seq >= ?2",
            params![id, end_seq as i64],
        )?;
    }
    tx.commit()?;
    entry.info = info_hash;
    entry.window_start_seq = p.window_start_seq;
    entry.msgs = hashes;
    Ok(())
}

fn load_from(
    db: &Connection,
    cache: &mut HashMap<String, Cached>,
    id: &str,
    active_only: bool,
) -> DbResult<Persisted> {
    let row: Option<(String, Option<String>)> = db
        .query_row(
            "SELECT info, extra FROM sessions WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((info, extra)) = row else {
        return Err(format!("Session not found: {id}").into());
    };
    let info: SessionInfo =
        serde_json::from_str(&info).map_err(|e| format!("Session {id} is corrupt: {e}"))?;
    let extra: Extra = match extra.map(|x| serde_json::from_str(&x)) {
        None => Extra::default(),
        Some(Ok(e)) => e,
        Some(Err(e)) => {
            tracing::warn!(session = %id, "corrupt extra, using defaults: {e}");
            Extra::default()
        }
    };
    let window_start_seq = if active_only {
        latest_compaction_seq(db, id)?
    } else {
        0
    };
    let mut stmt = db.prepare_cached(
        "SELECT seq, data FROM messages WHERE session_id = ?1 AND seq >= ?2 ORDER BY seq",
    )?;
    let rows = stmt.query_map(params![id, window_start_seq as i64], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut messages = Vec::new();
    let mut contiguous = true;
    for r in rows {
        let (seq, data) = r?;
        contiguous &= seq == (window_start_seq + messages.len() as u64) as i64;
        match serde_json::from_str::<Message>(&data) {
            Ok(m) => messages.push(m),
            Err(e) => {
                tracing::warn!(session = %id, seq, "skipping corrupt message: {e}");
                contiguous = false;
            }
        }
    }
    let p = Persisted {
        info,
        messages,
        reported: extra.reported,
        context_notice: extra.context_notice,
        model_explicit: extra.model_explicit,
        window_start_seq,
        force_persist: false,
    };
    if contiguous {
        let e = Extra {
            reported: p.reported,
            context_notice: p.context_notice,
            model_explicit: p.model_explicit,
        };
        cache.insert(
            id.to_string(),
            Cached {
                info: hash_json(&(&p.info, &e)),
                window_start_seq,
                msgs: p.messages.iter().map(hash_json).collect(),
            },
        );
    } else {
        // Rows do not map 1:1 to positions: rewrite everything on the next save.
        cache.remove(id);
    }
    Ok(p)
}

fn latest_compaction_seq(db: &Connection, id: &str) -> DbResult<u64> {
    let seq: Option<i64> = db
        .query_row(
            "SELECT seq FROM messages
             WHERE session_id = ?1
               AND (json_extract(layout_meta, '$.source_kind') = 'compaction'
                    OR (layout_meta IS NULL AND json_extract(data, '$.source.kind') = 'compaction'))
             ORDER BY seq DESC LIMIT 1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(seq.unwrap_or(0).max(0) as u64)
}

fn load_message(db: &Connection, session_id: &str, message_id: &str) -> DbResult<Option<Message>> {
    let raw: Option<String> = db
        .query_row(
            "SELECT data FROM messages WHERE session_id = ?1 AND id = ?2 LIMIT 1",
            params![session_id, message_id],
            |r| r.get(0),
        )
        .optional()?;
    match raw {
        None => Ok(None),
        Some(data) => match serde_json::from_str::<Message>(&data) {
            Ok(m) => Ok(Some(m)),
            Err(e) => {
                tracing::warn!(session = %session_id, message = %message_id, "skipping corrupt message: {e}");
                Ok(None)
            }
        },
    }
}

fn load_layout_metas(
    db: &Connection,
    session_id: &str,
    start_seq: u64,
    end_seq: u64,
) -> DbResult<Vec<MessageLayoutMeta>> {
    let mut stmt = db.prepare_cached(
        "SELECT seq, layout_meta, data FROM messages
         WHERE session_id = ?1 AND seq >= ?2 AND seq < ?3
         ORDER BY seq",
    )?;
    let rows = stmt.query_map(params![session_id, start_seq as i64, end_seq as i64], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    let mut out = Vec::new();
    for r in rows {
        let (seq, meta_raw, data) = r?;
        let seq_u = seq.max(0) as u64;
        if let Some(raw) = meta_raw
            && let Ok(meta) = serde_json::from_str::<MessageLayoutMeta>(&raw)
        {
            out.push(meta);
            continue;
        }
        match serde_json::from_str::<Message>(&data) {
            Ok(m) => out.push(compute_layout_meta(seq_u, &m)),
            Err(e) => {
                tracing::warn!(session = %session_id, seq, "skipping corrupt message meta: {e}")
            }
        }
    }
    Ok(out)
}

fn load_messages_range(
    db: &Connection,
    session_id: &str,
    start_seq: u64,
    end_seq: u64,
) -> DbResult<Vec<Message>> {
    let mut stmt = db.prepare_cached(
        "SELECT seq, data FROM messages
         WHERE session_id = ?1 AND seq >= ?2 AND seq < ?3
         ORDER BY seq",
    )?;
    let rows = stmt.query_map(params![session_id, start_seq as i64, end_seq as i64], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut out = Vec::new();
    for r in rows {
        let (seq, data) = r?;
        match serde_json::from_str::<Message>(&data) {
            Ok(m) => out.push(m),
            Err(e) => tracing::warn!(session = %session_id, seq, "skipping corrupt message: {e}"),
        }
    }
    Ok(out)
}

fn load_messages_before(
    db: &Connection,
    session_id: &str,
    before_seq: u64,
    limit: usize,
    max_bytes: usize,
) -> DbResult<(Vec<Message>, u64, u64)> {
    let mut stmt = db.prepare_cached(
        "SELECT seq, data FROM messages
         WHERE session_id = ?1 AND seq < ?2
         ORDER BY seq DESC LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![session_id, before_seq as i64, limit as i64], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut picked: Vec<(u64, Message)> = Vec::new();
    let mut bytes = 0usize;
    for r in rows {
        let (seq, data) = r?;
        let mut m = match serde_json::from_str::<Message>(&data) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(session = %session_id, seq, "skipping corrupt message: {e}");
                continue;
            }
        };
        m.strip_image_data();
        let len = serde_json::to_string(&m)?.len();
        if !picked.is_empty() && bytes + len > max_bytes {
            break;
        }
        bytes += len;
        picked.push((seq.max(0) as u64, m));
        if bytes >= max_bytes {
            break;
        }
    }
    picked.reverse();
    let start_seq = picked.first().map_or(before_seq, |(s, _)| *s);
    let end_seq = before_seq;
    let msgs = picked.into_iter().map(|(_, m)| m).collect();
    Ok((msgs, start_seq, end_seq))
}

fn message_seq(db: &Connection, session_id: &str, message_id: &str) -> DbResult<Option<u64>> {
    let seq: Option<i64> = db
        .query_row(
            "SELECT seq FROM messages WHERE session_id = ?1 AND id = ?2 LIMIT 1",
            params![session_id, message_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(seq.map(|s| s.max(0) as u64))
}

fn last_human_seq_before(
    db: &Connection,
    session_id: &str,
    before_seq: u64,
) -> DbResult<Option<u64>> {
    let seq: Option<i64> = db
        .query_row(
            "SELECT seq FROM messages
             WHERE session_id = ?1 AND seq < ?2
               AND json_extract(data, '$.role') = 'user'
               AND json_extract(data, '$.source.kind') = 'human'
             ORDER BY seq DESC LIMIT 1",
            params![session_id, before_seq as i64],
            |r| r.get(0),
        )
        .optional()?;
    Ok(seq.map(|s| s.max(0) as u64))
}

fn compaction_seq_before(db: &Connection, session_id: &str, before_seq: u64) -> DbResult<u64> {
    let seq: Option<i64> = db
        .query_row(
            "SELECT seq FROM messages
             WHERE session_id = ?1 AND seq < ?2
               AND (json_extract(layout_meta, '$.source_kind') = 'compaction'
                    OR (layout_meta IS NULL AND json_extract(data, '$.source.kind') = 'compaction'))
             ORDER BY seq DESC LIMIT 1",
            params![session_id, before_seq as i64],
            |r| r.get(0),
        )
        .optional()?;
    Ok(seq.unwrap_or(0).max(0) as u64)
}

fn resolve_id(db: &Connection, q: &str) -> DbResult<String> {
    let q = q.trim();
    if q.is_empty() {
        return Err("Error: session name or id must not be empty."
            .to_string()
            .into());
    }
    if let Some(id) = db
        .query_row("SELECT id FROM sessions WHERE id = ?1 LIMIT 1", [q], |r| {
            r.get::<_, String>(0)
        })
        .optional()?
    {
        return Ok(id);
    }
    let mut stmt = db.prepare_cached(
        "SELECT id, name FROM sessions WHERE name = ?1 COLLATE NOCASE ORDER BY updated_at DESC",
    )?;
    let candidates: Vec<(String, String)> = stmt
        .query_map([q], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    let exact: Vec<&(String, String)> = candidates.iter().filter(|(_, n)| n == q).collect();
    match exact.len() {
        1 => return Ok(exact[0].0.clone()),
        n if n > 1 => {
            let c: Vec<String> = exact
                .iter()
                .map(|(id, name)| format!("\"{name}\" (id {id})"))
                .collect();
            return Err(format!(
                "Error: \"{q}\" matches multiple history sessions. Candidates: {}. Use the id.",
                c.join(", ")
            )
            .into());
        }
        _ => {}
    }
    match candidates.len() {
        1 => Ok(candidates[0].0.clone()),
        0 => Err(format!("Error: no live or history session matching \"{q}\".").into()),
        _ => {
            let c: Vec<String> = candidates
                .iter()
                .map(|(id, name)| format!("\"{name}\" (id {id})"))
                .collect();
            Err(format!(
                "Error: \"{q}\" is ambiguous in history. Candidates: {}. Use the id.",
                c.join(", ")
            )
            .into())
        }
    }
}

fn ids_by_name(db: &Connection, name: &str) -> DbResult<Vec<String>> {
    let mut stmt = db.prepare_cached("SELECT id FROM sessions WHERE name = ?1 COLLATE NOCASE")?;
    let rows = stmt.query_map([name], |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

fn fts_phrase(q: &str) -> String {
    format!("\"{}\"", q.replace('"', "\"\""))
}

fn build_session_where(
    cwd: Option<&str>,
    creator: Option<&str>,
    extra_creator_ids: &[String],
    query: Option<&str>,
    exclude_ids: &[String],
) -> (String, Vec<SqlValue>) {
    let mut clauses: Vec<String> = Vec::new();
    let mut params: Vec<SqlValue> = Vec::new();

    if let Some(c) = cwd.map(str::trim).filter(|s| !s.is_empty()) {
        clauses.push("s.cwd = ?".into());
        params.push(SqlValue::Text(c.to_string()));
    }

    if let Some(cr) = creator.map(str::trim).filter(|s| !s.is_empty()) {
        if cr == "user" {
            clauses.push("(s.parent IS NULL OR s.parent = '')".into());
        } else if extra_creator_ids.is_empty() {
            clauses.push(
                "(s.parent = ? OR s.parent IN (SELECT id FROM sessions WHERE name = ? COLLATE NOCASE))"
                    .into(),
            );
            params.push(SqlValue::Text(cr.to_string()));
            params.push(SqlValue::Text(cr.to_string()));
        } else {
            let placeholders = vec!["?"; extra_creator_ids.len()].join(",");
            clauses.push(format!(
                "(s.parent = ? OR s.parent IN ({placeholders}) OR s.parent IN (SELECT id FROM sessions WHERE name = ? COLLATE NOCASE))"
            ));
            params.push(SqlValue::Text(cr.to_string()));
            for id in extra_creator_ids {
                params.push(SqlValue::Text(id.clone()));
            }
            params.push(SqlValue::Text(cr.to_string()));
        }
    }

    if let Some(q) = query.map(str::trim).filter(|s| !s.is_empty()) {
        if q.chars().count() >= FTS_TRIGRAM_MIN_CHARS {
            let phrase = fts_phrase(q);
            let parent_phrase = format!("{{title name}} : {phrase}");
            clauses.push(
                "(s.id IN (SELECT id FROM sessions_fts WHERE sessions_fts MATCH ?) \
                 OR s.parent IN (SELECT id FROM sessions_fts WHERE sessions_fts MATCH ?))"
                    .into(),
            );
            params.push(SqlValue::Text(phrase));
            params.push(SqlValue::Text(parent_phrase));
        } else {
            let ql = q.to_lowercase();
            clauses.push(
                "(INSTR(LOWER(COALESCE(s.title, '')), ?) > 0 \
                 OR INSTR(LOWER(COALESCE(s.name, '')), ?) > 0 \
                 OR INSTR(LOWER(COALESCE(s.cwd, '')), ?) > 0 \
                 OR INSTR(LOWER(COALESCE(s.id, '')), ?) > 0 \
                 OR INSTR(LOWER(COALESCE(s.parent, '')), ?) > 0 \
                 OR s.parent IN (SELECT id FROM sessions WHERE INSTR(LOWER(COALESCE(name, '')), ?) > 0 OR INSTR(LOWER(COALESCE(title, '')), ?) > 0))"
                    .into(),
            );
            for _ in 0..7 {
                params.push(SqlValue::Text(ql.clone()));
            }
        }
    }

    if !exclude_ids.is_empty() {
        let placeholders = vec!["?"; exclude_ids.len()].join(",");
        clauses.push(format!("s.id NOT IN ({placeholders})"));
        for id in exclude_ids {
            params.push(SqlValue::Text(id.clone()));
        }
    }

    let where_sql = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };
    (where_sql, params)
}

fn count_excluding(
    db: &Connection,
    creator: Option<&str>,
    creator_ids: &[String],
    exclude_ids: &[String],
) -> DbResult<usize> {
    let (where_sql, params) = build_session_where(None, creator, creator_ids, None, exclude_ids);
    let sql = format!("SELECT COUNT(*) FROM sessions s {where_sql}");
    let n: i64 = db.query_row(&sql, params_from_iter(params), |r| r.get(0))?;
    Ok(n.max(0) as usize)
}

fn page_excluding(
    db: &Connection,
    creator: Option<&str>,
    creator_ids: &[String],
    exclude_ids: &[String],
    limit: usize,
    offset: usize,
) -> DbResult<Vec<SessionInfo>> {
    let (where_sql, mut params) =
        build_session_where(None, creator, creator_ids, None, exclude_ids);
    let sql = format!(
        "SELECT s.info FROM sessions s {where_sql} ORDER BY s.updated_at DESC LIMIT ? OFFSET ?"
    );
    params.push(SqlValue::Integer(limit as i64));
    params.push(SqlValue::Integer(offset as i64));
    let mut stmt = db.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(params), |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for r in rows {
        let raw = r?;
        match serde_json::from_str::<SessionInfo>(&raw) {
            Ok(i) => out.push(i),
            Err(e) => tracing::warn!("skipping corrupt session row: {e}"),
        }
    }
    Ok(out)
}

fn list(
    db: &Connection,
    cwd: Option<&str>,
    creator: Option<&str>,
    extra_creator_ids: &[String],
    query: Option<&str>,
    limit: Option<usize>,
    exclude_ids: &[String],
) -> DbResult<Vec<SessionInfo>> {
    let (where_sql, mut params) =
        build_session_where(cwd, creator, extra_creator_ids, query, exclude_ids);
    let limit = limit.map(|l| l as i64).unwrap_or(NO_LIMIT);
    let sql =
        format!("SELECT s.info FROM sessions s {where_sql} ORDER BY s.updated_at DESC LIMIT ?");
    params.push(SqlValue::Integer(limit));
    let mut stmt = db.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(params), |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for r in rows {
        let raw = r?;
        match serde_json::from_str::<SessionInfo>(&raw) {
            Ok(i) => out.push(i),
            Err(e) => tracing::warn!("skipping corrupt session row: {e}"),
        }
    }
    Ok(out)
}

fn infos(db: &Connection, ids: &[String]) -> DbResult<Vec<SessionInfo>> {
    let mut stmt = db.prepare_cached("SELECT info FROM sessions WHERE id = ?1")?;
    let mut out = Vec::new();
    for id in ids {
        let Some(raw) = stmt.query_row([id], |r| r.get::<_, String>(0)).optional()? else {
            continue;
        };
        match serde_json::from_str::<SessionInfo>(&raw) {
            Ok(i) => out.push(i),
            Err(e) => tracing::warn!(session = %id, "skipping corrupt session row: {e}"),
        }
    }
    Ok(out)
}

fn delete(db: &mut Connection, id: &str) -> DbResult<bool> {
    let tx = db.transaction()?;
    let affected_groups: Vec<String> = {
        let mut stmt =
            tx.prepare("SELECT group_id FROM session_group_members WHERE session_id = ?1")?;
        stmt.query_map([id], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
    };
    tx.execute("DELETE FROM messages WHERE session_id = ?1", [id])?;
    let n = tx.execute("DELETE FROM sessions WHERE id = ?1", [id])?;
    let mut groups_changed = false;
    if !affected_groups.is_empty() {
        groups_changed = true;
        for gid in &affected_groups {
            tx.execute(
                "DELETE FROM session_groups
                 WHERE id = ?1 AND locked = 0
                   AND NOT EXISTS (SELECT 1 FROM session_group_members WHERE group_id = ?1)",
                [gid],
            )?;
        }
    }
    tx.commit()?;
    if n == 0 {
        Err(format!("Session not found: {id}").into())
    } else {
        Ok(groups_changed)
    }
}

// ---------------------------------------------------------------------------
// Stash resume & group persistence
// ---------------------------------------------------------------------------

fn set_stashed_resume(db: &Connection, id: &str, resume: Option<&str>) -> DbResult<()> {
    let n = db.execute(
        "UPDATE sessions SET stashed_resume = ?1 WHERE id = ?2",
        params![resume, id],
    )?;
    if n == 0 {
        return Err(format!("Session not found: {id}").into());
    }
    Ok(())
}

fn take_stashed_resume(db: &mut Connection, id: &str) -> DbResult<Option<String>> {
    let tx = db.transaction()?;
    let val: Option<String> = tx
        .query_row(
            "SELECT stashed_resume FROM sessions WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    if val.is_some() {
        tx.execute(
            "UPDATE sessions SET stashed_resume = NULL WHERE id = ?1",
            [id],
        )?;
    }
    tx.commit()?;
    Ok(val)
}

fn list_groups_raw(db: &Connection) -> DbResult<Vec<RawGroup>> {
    let mut g_stmt = db.prepare_cached(
        "SELECT id, name, locked, created_at, updated_at
         FROM session_groups
         ORDER BY updated_at DESC, created_at DESC, id ASC",
    )?;
    let mut groups: Vec<RawGroup> = g_stmt
        .query_map([], |r| {
            Ok(RawGroup {
                id: r.get(0)?,
                name: r.get(1)?,
                locked: r.get::<_, i64>(2)? != 0,
                created_at: r.get(3)?,
                updated_at: r.get(4)?,
                members: Vec::new(),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut m_stmt = db.prepare_cached(
        "SELECT m.group_id, m.session_id, m.position, s.info
         FROM session_group_members m
         JOIN sessions s ON s.id = m.session_id
         ORDER BY m.group_id, m.position ASC",
    )?;
    let mut by_group: HashMap<String, Vec<(String, u32, SessionInfo)>> = HashMap::new();
    let rows = m_stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    for r in rows {
        let (gid, sid, pos, info_raw) = r?;
        match serde_json::from_str::<SessionInfo>(&info_raw) {
            Ok(info) => {
                by_group
                    .entry(gid)
                    .or_default()
                    .push((sid, pos.max(0) as u32, info));
            }
            Err(e) => tracing::warn!(session = %sid, "skipping corrupt group member info: {e}"),
        }
    }
    for g in &mut groups {
        if let Some(m) = by_group.remove(&g.id) {
            g.members = m;
        }
    }
    Ok(groups)
}

fn group_member_ids(tx: &Connection, group_id: &str) -> DbResult<Vec<String>> {
    let mut stmt = tx.prepare_cached(
        "SELECT session_id FROM session_group_members WHERE group_id = ?1 ORDER BY position ASC",
    )?;
    let rows = stmt.query_map([group_id], |r| r.get::<_, String>(0))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn write_group_members(tx: &Connection, group_id: &str, members: &[String]) -> DbResult<()> {
    tx.execute(
        "DELETE FROM session_group_members WHERE group_id = ?1",
        [group_id],
    )?;
    let mut stmt = tx.prepare_cached(
        "INSERT INTO session_group_members (group_id, session_id, position) VALUES (?1, ?2, ?3)",
    )?;
    for (pos, sid) in members.iter().enumerate() {
        stmt.execute(params![group_id, sid, pos as i64])?;
    }
    Ok(())
}

fn ensure_group_exists(tx: &Connection, group_id: &str) -> DbResult<bool> {
    let locked: Option<i64> = tx
        .query_row(
            "SELECT locked FROM session_groups WHERE id = ?1",
            [group_id],
            |r| r.get(0),
        )
        .optional()?;
    match locked {
        Some(v) => Ok(v != 0),
        None => Err(format!("Stash group not found: {group_id}").into()),
    }
}

fn ensure_session_exists(tx: &Connection, session_id: &str) -> DbResult<()> {
    if !exists(tx, session_id)? {
        return Err(format!("Session not found: {session_id}").into());
    }
    Ok(())
}

fn create_group(
    db: &mut Connection,
    id: &str,
    name: &str,
    session_ids: &[String],
) -> DbResult<Vec<RawGroup>> {
    let now = ref_proto::now_ms();
    let tx = db.transaction()?;
    tx.execute(
        "INSERT INTO session_groups (id, name, locked, created_at, updated_at)
         VALUES (?1, ?2, 0, ?3, ?4)",
        params![id, name, now, now],
    )?;
    let mut dedup = Vec::new();
    for sid in session_ids {
        ensure_session_exists(&tx, sid)?;
        if !dedup.contains(sid) {
            dedup.push(sid.clone());
        }
    }
    write_group_members(&tx, id, &dedup)?;
    tx.commit()?;
    list_groups_raw(db)
}

fn rename_group(db: &mut Connection, group_id: &str, name: &str) -> DbResult<Vec<RawGroup>> {
    let now = ref_proto::now_ms();
    let tx = db.transaction()?;
    let n = tx.execute(
        "UPDATE session_groups SET name = ?1, updated_at = ?2 WHERE id = ?3",
        params![name, now, group_id],
    )?;
    if n == 0 {
        return Err(format!("Stash group not found: {group_id}").into());
    }
    tx.commit()?;
    list_groups_raw(db)
}

fn set_group_locked(db: &mut Connection, group_id: &str, locked: bool) -> DbResult<Vec<RawGroup>> {
    let now = ref_proto::now_ms();
    let tx = db.transaction()?;
    let n = tx.execute(
        "UPDATE session_groups SET locked = ?1, updated_at = ?2 WHERE id = ?3",
        params![i64::from(locked), now, group_id],
    )?;
    if n == 0 {
        return Err(format!("Stash group not found: {group_id}").into());
    }
    tx.commit()?;
    list_groups_raw(db)
}

fn delete_group(db: &mut Connection, group_id: &str) -> DbResult<Vec<RawGroup>> {
    let tx = db.transaction()?;
    let n = tx.execute("DELETE FROM session_groups WHERE id = ?1", [group_id])?;
    if n == 0 {
        return Err(format!("Stash group not found: {group_id}").into());
    }
    tx.commit()?;
    list_groups_raw(db)
}

fn upsert_member_at(
    tx: &Connection,
    group_id: &str,
    session_id: &str,
    position: Option<u32>,
) -> DbResult<()> {
    let mut members = group_member_ids(tx, group_id)?;
    let existing = members.iter().position(|s| s == session_id);
    if existing.is_some() && position.is_none() {
        return Ok(());
    }
    if let Some(idx) = existing {
        members.remove(idx);
    }
    let idx = position.map_or(members.len(), |p| (p as usize).min(members.len()));
    members.insert(idx, session_id.to_string());
    write_group_members(tx, group_id, &members)
}

fn add_to_group(
    db: &mut Connection,
    group_id: &str,
    session_id: &str,
    position: Option<u32>,
) -> DbResult<Vec<RawGroup>> {
    let now = ref_proto::now_ms();
    let tx = db.transaction()?;
    ensure_group_exists(&tx, group_id)?;
    ensure_session_exists(&tx, session_id)?;
    upsert_member_at(&tx, group_id, session_id, position)?;
    tx.execute(
        "UPDATE session_groups SET updated_at = ?1 WHERE id = ?2",
        params![now, group_id],
    )?;
    tx.commit()?;
    list_groups_raw(db)
}

fn move_group_member(
    db: &mut Connection,
    from_group_id: Option<&str>,
    to_group_id: &str,
    session_id: &str,
    position: u32,
) -> DbResult<Vec<RawGroup>> {
    let now = ref_proto::now_ms();
    let tx = db.transaction()?;
    ensure_group_exists(&tx, to_group_id)?;
    ensure_session_exists(&tx, session_id)?;
    if let Some(from_id) = from_group_id
        && from_id != to_group_id
    {
        let locked = ensure_group_exists(&tx, from_id)?;
        let mut from_members = group_member_ids(&tx, from_id)?;
        from_members.retain(|s| s != session_id);
        if from_members.is_empty() && !locked {
            tx.execute("DELETE FROM session_groups WHERE id = ?1", [from_id])?;
        } else {
            write_group_members(&tx, from_id, &from_members)?;
            tx.execute(
                "UPDATE session_groups SET updated_at = ?1 WHERE id = ?2",
                params![now, from_id],
            )?;
        }
    }
    upsert_member_at(&tx, to_group_id, session_id, Some(position))?;
    tx.execute(
        "UPDATE session_groups SET updated_at = ?1 WHERE id = ?2",
        params![now, to_group_id],
    )?;
    tx.commit()?;
    list_groups_raw(db)
}

fn remove_from_group(
    db: &mut Connection,
    group_id: &str,
    session_id: &str,
) -> DbResult<Vec<RawGroup>> {
    let now = ref_proto::now_ms();
    let tx = db.transaction()?;
    let locked = ensure_group_exists(&tx, group_id)?;
    let mut members = group_member_ids(&tx, group_id)?;
    members.retain(|s| s != session_id);
    if members.is_empty() && !locked {
        tx.execute("DELETE FROM session_groups WHERE id = ?1", [group_id])?;
    } else {
        write_group_members(&tx, group_id, &members)?;
        tx.execute(
            "UPDATE session_groups SET updated_at = ?1 WHERE id = ?2",
            params![now, group_id],
        )?;
    }
    tx.commit()?;
    list_groups_raw(db)
}

fn merge_groups(
    db: &mut Connection,
    source_group_id: &str,
    target_group_id: &str,
) -> DbResult<Vec<RawGroup>> {
    if source_group_id == target_group_id {
        return Err("Cannot merge a group into itself.".to_string().into());
    }
    let now = ref_proto::now_ms();
    let tx = db.transaction()?;
    ensure_group_exists(&tx, source_group_id)?;
    ensure_group_exists(&tx, target_group_id)?;
    let src_members = group_member_ids(&tx, source_group_id)?;
    let mut dst_members = group_member_ids(&tx, target_group_id)?;
    for sid in src_members {
        if !dst_members.contains(&sid) {
            dst_members.push(sid);
        }
    }
    write_group_members(&tx, target_group_id, &dst_members)?;
    tx.execute(
        "UPDATE session_groups SET updated_at = ?1 WHERE id = ?2",
        params![now, target_group_id],
    )?;
    tx.execute(
        "DELETE FROM session_groups WHERE id = ?1",
        [source_group_id],
    )?;
    tx.commit()?;
    list_groups_raw(db)
}

fn finish_restore_group(db: &mut Connection, group_id: &str) -> DbResult<Vec<RawGroup>> {
    let tx = db.transaction()?;
    let locked = ensure_group_exists(&tx, group_id)?;
    if !locked {
        tx.execute("DELETE FROM session_groups WHERE id = ?1", [group_id])?;
    }
    tx.commit()?;
    list_groups_raw(db)
}

fn finish_restore_group_member(
    db: &mut Connection,
    group_id: &str,
    session_id: &str,
) -> DbResult<Vec<RawGroup>> {
    let now = ref_proto::now_ms();
    let tx = db.transaction()?;
    let locked = ensure_group_exists(&tx, group_id)?;
    if !locked {
        let mut members = group_member_ids(&tx, group_id)?;
        members.retain(|s| s != session_id);
        if members.is_empty() {
            tx.execute("DELETE FROM session_groups WHERE id = ?1", [group_id])?;
        } else {
            write_group_members(&tx, group_id, &members)?;
            tx.execute(
                "UPDATE session_groups SET updated_at = ?1 WHERE id = ?2",
                params![now, group_id],
            )?;
        }
    }
    tx.commit()?;
    list_groups_raw(db)
}
/// One-time import of `<data_dir>/sessions/*.json`; the directory is renamed afterwards.
fn import_json(db: &mut Connection, data_dir: &Path) -> DbResult<()> {
    let dir = data_dir.join("sessions");
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return Ok(());
    };
    let mut files: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".json") && !n.ends_with(".meta.json"))
        })
        .collect();
    files.sort();
    let mut imported = 0usize;
    let mut skipped = 0usize;
    let tx = db.transaction()?;
    for f in &files {
        let parsed = std::fs::read(f)
            .map_err(|e| e.to_string())
            .and_then(|raw| serde_json::from_slice::<Persisted>(&raw).map_err(|e| e.to_string()));
        let p = match parsed {
            Ok(p) if valid_id(&p.info.id) => p,
            Ok(p) => {
                tracing::warn!(
                    "import: {}: invalid session id {:?}",
                    f.display(),
                    p.info.id
                );
                skipped += 1;
                continue;
            }
            Err(e) => {
                tracing::warn!("import: {}: {e}", f.display());
                skipped += 1;
                continue;
            }
        };
        if exists(&tx, &p.info.id)? {
            continue;
        }
        let extra = Extra {
            reported: p.reported,
            context_notice: false,
            model_explicit: p.model_explicit,
        };
        tx.execute(
            "INSERT INTO sessions (id, info, cwd, title, name, created_at, updated_at, extra, parent)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                p.info.id,
                serde_json::to_string(&p.info)?,
                p.info.cwd,
                p.info.title,
                p.info.name,
                p.info.created_at,
                p.info.updated_at,
                serde_json::to_string(&extra)?,
                p.info.parent
            ],
        )?;
        for (seq, m) in p.messages.iter().enumerate() {
            let data = serde_json::to_string(m)?;
            let meta = serde_json::to_string(&compute_layout_meta(seq as u64, m))?;
            tx.execute(
                "INSERT INTO messages (session_id, seq, id, time, data, layout_meta)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![p.info.id, seq as i64, m.id, m.time, data, meta],
            )?;
        }
    }
    tx.commit()?;
    let mut target = data_dir.join("sessions.imported");
    if target.exists() {
        target = data_dir.join(format!("sessions.imported.{}", ref_proto::now_ms()));
    }
    std::fs::rename(&dir, &target)
        .map_err(|e| format!("renaming {} failed: {e}", dir.display()))?;
    tracing::info!(
        imported,
        skipped,
        "imported legacy JSON sessions into {}; moved files to {}",
        DB_FILE,
        target.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ref_proto::*;

    fn tmp() -> PathBuf {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tmp")
            .join(format!("ref-store-test-{}", crate::util::random_hex(8)));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn info(id: &str, cwd: &str, updated: i64) -> SessionInfo {
        SessionInfo {
            id: id.into(),
            name: "名前".into(),
            title: "t".into(),
            agent: "build".into(),
            model: ModelRef::default(),
            cwd: cwd.into(),
            created_at: 1,
            updated_at: updated,
            status: SessionStatus::Idle,
            attached: 0,
            live: false,
            parent: None,
            must_report: true,
            context: ContextUsage::default(),
            todos: Vec::new(),
            busy_since: None,
            last_error: None,
        }
    }

    fn msg(id: &str, text: &str) -> Message {
        Message {
            id: id.into(),
            role: Role::User,
            time: 1,
            time_end: None,
            source: MessageSource::Human,
            parts: vec![Part::Text {
                text: text.into(),
                time: 1,
            }],
            model: None,
            agent: None,
            usage: None,
            error: None,
        }
    }

    fn persisted(id: &str, cwd: &str, updated: i64, messages: Vec<Message>) -> Persisted {
        Persisted {
            info: info(id, cwd, updated),
            messages,
            reported: false,
            context_notice: false,
            model_explicit: false,
            window_start_seq: 0,
            force_persist: false,
        }
    }

    fn rows(path: &Path, id: &str) -> Vec<(i64, String)> {
        let c = Connection::open(path).unwrap();
        let mut st = c
            .prepare("SELECT seq, id FROM messages WHERE session_id = ?1 ORDER BY seq")
            .unwrap();
        st.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    /// Files this process has open below `dir` (Linux only).
    #[cfg(target_os = "linux")]
    fn open_files_below(dir: &Path) -> Vec<PathBuf> {
        let dir = dir.canonicalize().unwrap();
        std::fs::read_dir("/proc/self/fd")
            .unwrap()
            .filter_map(|fd| std::fs::read_link(fd.ok()?.path()).ok())
            .filter(|target| target.starts_with(&dir))
            .collect()
    }

    #[test]
    fn close_releases_the_database_so_the_directory_can_be_removed() {
        let dir = tmp();
        let store = Store::new(dir.clone());
        store.save(persisted("ses_abc", "/a", 2, vec![msg("m1", "hi")]));
        store.flush().unwrap();
        #[cfg(target_os = "linux")]
        assert!(!open_files_below(&dir).is_empty());

        store.close().unwrap();
        #[cfg(target_os = "linux")]
        assert_eq!(open_files_below(&dir), Vec::<PathBuf>::new());
        assert!(
            store.load("ses_abc").is_err(),
            "a closed store serves nothing"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Queued saves "one message" then "no messages" (a session undone to empty) must not
    /// both be dropped: the empty one alone is not stored when the session was never saved.
    #[test]
    fn a_save_that_gives_the_session_its_first_row_is_not_coalesced_away() {
        let save =
            |id: &str, messages: Vec<Message>| Cmd::Save(Box::new(persisted(id, "/", 1, messages)));
        let batch = [
            save("ses_a", vec![msg("m1", "hi")]),
            save("ses_a", vec![]),
            save("ses_b", vec![msg("m1", "hi")]),
            save("ses_b", vec![msg("m1", "hi"), msg("m2", "there")]),
        ];
        // The first save of ses_a creates the row; the two for ses_b are interchangeable.
        assert_eq!(superseded_saves(&batch), [false, false, true, false]);
    }

    #[test]
    fn round_trip_incremental_and_reopen() {
        let dir = tmp();
        let store = Store::new(dir.clone());
        // Empty, never-saved sessions are not stored.
        store.save(persisted("ses_empty", "/", 1, vec![]));
        assert!(!store.exists("ses_empty"));

        let mut p = persisted(
            "ses_abc",
            "/a",
            2,
            vec![msg("m1", "hi"), msg("m2", "there"), msg("m3", "x")],
        );
        p.reported = true;
        p.context_notice = true;
        store.save(p.clone());
        let l = store.load("ses_abc").unwrap();
        assert_eq!(l.info.name, "名前");
        assert!(l.reported && l.context_notice && l.info.must_report);
        assert_eq!(l.messages.len(), 3);
        // Undo: rows beyond the new length are deleted; changed rows rewritten.
        p.messages.truncate(1);
        p.messages[0] = msg("m1", "edited");
        p.context_notice = false;
        store.save(p.clone());
        store.flush().unwrap();
        assert_eq!(rows(&store.path, "ses_abc"), vec![(0, "m1".to_string())]);
        let l = store.load("ses_abc").unwrap();
        assert_eq!(l.messages[0].raw_text(), "edited");
        assert!(!l.context_notice);

        store.save(persisted("ses_b", "/b", 5, vec![msg("x", "y")]));
        let all = store.list();
        assert_eq!(
            all.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            ["ses_b", "ses_abc"]
        );
        assert_eq!(store.list_for(Some("/a".into()), None, None, None).len(), 1);
        assert_eq!(store.list_for(None, None, None, Some(1)).len(), 1);
        assert!(store.load("../etc").is_err());
        assert!(store.load("ses_nope").is_err());
        store.delete("ses_abc").unwrap();
        assert!(store.delete("ses_abc").is_err());
        assert!(!store.exists("ses_abc"));
        assert!(rows(&store.path, "ses_abc").is_empty());
        drop(store);

        // Reopen: data survives; a fresh process has no cache and rewrites correctly.
        let store = Store::new(dir.clone());
        let mut b = store.load("ses_b").unwrap();
        b.messages.push(msg("z", "new"));
        store.save(b);
        assert_eq!(store.load("ses_b").unwrap().messages.len(), 2);
        let v: i64 = Connection::open(&store.path)
            .unwrap()
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, SCHEMA_VERSION);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn imports_legacy_json_once() {
        let dir = tmp();
        let sessions = dir.join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        let mut p = persisted(
            "ses_old",
            "/old",
            7,
            vec![msg("m1", "old one"), msg("m2", "old two")],
        );
        p.reported = true;
        // Legacy files have no `context_notice` key.
        let mut v = serde_json::to_value(&p).unwrap();
        v.as_object_mut().unwrap().remove("context_notice");
        std::fs::write(sessions.join("ses_old.json"), v.to_string()).unwrap();
        std::fs::write(
            sessions.join("ses_old.meta.json"),
            serde_json::to_string(&p.info).unwrap(),
        )
        .unwrap();
        std::fs::write(sessions.join("ses_bad.json"), "{not json").unwrap();
        let store = Store::new(dir.clone());
        let l = store.load("ses_old").unwrap();
        assert_eq!(l.messages.len(), 2);
        assert!(l.reported);
        assert_eq!(store.list().len(), 1);
        assert!(!sessions.exists());
        assert!(dir.join("sessions.imported/ses_old.json").is_file());
        drop(store);
        let store = Store::new(dir.clone());
        assert_eq!(store.list().len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn migrates_v1_to_v2_and_backfills_parent() {
        let dir = tmp();
        let db_path = dir.join(DB_FILE);
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE sessions (
                     id TEXT PRIMARY KEY,
                     info TEXT NOT NULL,
                     cwd TEXT,
                     title TEXT,
                     name TEXT,
                     created_at INTEGER,
                     updated_at INTEGER,
                     extra TEXT
                 );
                 CREATE INDEX sessions_updated ON sessions(updated_at);
                 CREATE INDEX sessions_cwd ON sessions(cwd, updated_at);
                 CREATE TABLE messages (
                     session_id TEXT NOT NULL,
                     seq INTEGER NOT NULL,
                     id TEXT NOT NULL,
                     time INTEGER,
                     data TEXT NOT NULL,
                     PRIMARY KEY (session_id, seq)
                 );
                 PRAGMA user_version = 1;",
            )
            .unwrap();
            let parent_info = info("ses_parent", "/p", 10);
            let mut child_info = info("ses_child", "/p", 20);
            child_info.parent = Some("ses_parent".into());
            for inf in [&parent_info, &child_info] {
                conn.execute(
                    "INSERT INTO sessions (id, info, cwd, title, name, created_at, updated_at, extra)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        inf.id,
                        serde_json::to_string(inf).unwrap(),
                        inf.cwd,
                        inf.title,
                        inf.name,
                        inf.created_at,
                        inf.updated_at,
                        "{}"
                    ],
                )
                .unwrap();
            }
        }

        let store = Store::new(dir.clone());
        let user = store.list_for(None, Some("user".into()), None, None);
        assert_eq!(
            user.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            ["ses_parent"]
        );
        let children = store.list_for(None, Some("ses_parent".into()), None, None);
        assert_eq!(
            children.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            ["ses_child"]
        );
        store.close().unwrap();

        let conn = Connection::open(&db_path).unwrap();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        let parent_col: Option<String> = conn
            .query_row(
                "SELECT parent FROM sessions WHERE id = 'ses_child'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(parent_col.as_deref(), Some("ses_parent"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn filters_by_creator_and_query() {
        let dir = tmp();
        let store = Store::new(dir.clone());

        let mut p1 = persisted("ses_p1", "/proj/a", 10, vec![msg("m1", "x")]);
        p1.info.name = "BossAgent".into();
        p1.info.title = "Main Orchestration".into();

        let mut p2 = persisted("ses_p2", "/proj/b", 20, vec![msg("m2", "x")]);
        p2.info.name = "OtherUser".into();
        p2.info.title = "Docs Cleanup".into();
        p2.info.parent = Some(String::new());

        let mut c1 = persisted("ses_c1", "/proj/a", 30, vec![msg("m3", "x")]);
        c1.info.name = "worker-alpha".into();
        c1.info.title = "Fix Parser Bug".into();
        c1.info.parent = Some("ses_p1".into());

        let mut c2 = persisted("ses_c2", "/proj/b", 40, vec![msg("m4", "x")]);
        c2.info.name = "worker-beta".into();
        c2.info.title = "Write Tests".into();
        c2.info.parent = Some("ses_p1".into());

        let mut c3 = persisted("ses_c3", "/proj/a", 50, vec![msg("m5", "x")]);
        c3.info.name = "helper".into();
        c3.info.title = "Subtask".into();
        c3.info.parent = Some("ses_p2".into());

        for p in [p1, p2, c1, c2, c3] {
            store.save(p);
        }

        let ids =
            |items: Vec<SessionInfo>| -> Vec<String> { items.into_iter().map(|i| i.id).collect() };

        assert_eq!(
            ids(store.list_for(None, Some("  user ".into()), None, None)),
            ["ses_p2", "ses_p1"]
        );
        assert_eq!(
            ids(store.list_for(None, Some("ses_p1".into()), None, None)),
            ["ses_c2", "ses_c1"]
        );
        assert_eq!(
            ids(store.list_for(None, Some("bossagent".into()), None, None)),
            ["ses_c2", "ses_c1"]
        );
        assert_eq!(
            ids(store.list_for(None, Some("   ".into()), Some("  ".into()), None)).len(),
            5
        );

        // Query across title, name, cwd, id, parent, parent.name, parent.title.
        assert_eq!(
            ids(store.list_for(None, None, Some("parser".into()), None)),
            ["ses_c1"]
        );
        assert_eq!(
            ids(store.list_for(None, None, Some("ALPHA".into()), None)),
            ["ses_c1"]
        );
        assert_eq!(
            ids(store.list_for(None, None, Some("/proj/b".into()), None)),
            ["ses_c2", "ses_p2"]
        );
        assert_eq!(
            ids(store.list_for(None, None, Some("ses_c3".into()), None)),
            ["ses_c3"]
        );
        assert_eq!(
            ids(store.list_for(None, None, Some("ses_p2".into()), None)),
            ["ses_c3", "ses_p2"]
        );
        assert_eq!(
            ids(store.list_for(None, None, Some("bossagent".into()), None)),
            ["ses_c2", "ses_c1", "ses_p1"]
        );
        assert_eq!(
            ids(store.list_for(None, None, Some("ORCHESTRATION".into()), None)),
            ["ses_c2", "ses_c1", "ses_p1"]
        );

        // Combined cwd + creator + query + limit.
        assert_eq!(
            ids(store.list_for(
                Some(" /proj/a ".into()),
                Some("bossagent".into()),
                Some("parser".into()),
                None
            )),
            ["ses_c1"]
        );
        assert_eq!(
            ids(store.list_for(
                None,
                Some("bossagent".into()),
                Some("orchestration".into()),
                Some(1)
            )),
            ["ses_c2"]
        );

        let _ = store.close();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn migrates_v2_to_v3_backfills_layout_meta_and_fts() {
        let dir = tmp();
        let db_path = dir.join(DB_FILE);
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE sessions (
                     id TEXT PRIMARY KEY,
                     info TEXT NOT NULL,
                     cwd TEXT,
                     title TEXT,
                     name TEXT,
                     created_at INTEGER,
                     updated_at INTEGER,
                     extra TEXT,
                     parent TEXT
                 );
                 CREATE INDEX sessions_updated ON sessions(updated_at);
                 CREATE INDEX sessions_cwd ON sessions(cwd, updated_at);
                 CREATE INDEX sessions_parent ON sessions(parent, updated_at);
                 CREATE TABLE messages (
                     session_id TEXT NOT NULL,
                     seq INTEGER NOT NULL,
                     id TEXT NOT NULL,
                     time INTEGER,
                     data TEXT NOT NULL,
                     PRIMARY KEY (session_id, seq)
                 );
                 CREATE INDEX messages_id ON messages(session_id, id);
                 PRAGMA user_version = 2;",
            )
            .unwrap();
            let mut inf = info("ses_v2", "/work/app", 100);
            inf.name = "LegacyV2".into();
            inf.title = "Migrate Trigram Title".into();
            conn.execute(
                "INSERT INTO sessions (id, info, cwd, title, name, created_at, updated_at, extra, parent)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    inf.id,
                    serde_json::to_string(&inf).unwrap(),
                    inf.cwd,
                    inf.title,
                    inf.name,
                    inf.created_at,
                    inf.updated_at,
                    "{}",
                    Option::<String>::None
                ],
            )
            .unwrap();
            let m0 = msg(
                "m0",
                "hello\n```rust\nlet x = 1;\n```\n| a | b |\n| - | - |\n| 1 | 2 |",
            );
            conn.execute(
                "INSERT INTO messages (session_id, seq, id, time, data) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![inf.id, 0i64, m0.id, m0.time, serde_json::to_string(&m0).unwrap()],
            )
            .unwrap();
        }

        let store = Store::new(dir.clone());
        let found = store.list_for(None, None, Some("trigram".into()), None);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "ses_v2");
        let metas = store.load_layout_metas("ses_v2", 0, 1).unwrap();
        assert_eq!(metas.len(), 1);
        assert_eq!(metas[0].parts[0].code_blocks, 1);
        assert_eq!(metas[0].parts[0].code_lines, 1);
        assert_eq!(metas[0].parts[0].tables, 1);
        assert_eq!(store.resolve_id("legacyv2").unwrap(), "ses_v2");
        let _ = store.close();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn heavy_synthetic_data_and_explain_query_plan() {
        let dir = tmp();
        let store = Store::new(dir.clone());
        store.flush().unwrap();
        store.close().unwrap();

        {
            let mut conn = Connection::open(&store.path).unwrap();
            conn.pragma_update(None, "foreign_keys", "ON").unwrap();
            let tx = conn.transaction().unwrap();
            let mut s_stmt = tx
                .prepare(
                    "INSERT INTO sessions (id, info, cwd, title, name, created_at, updated_at, extra, parent)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, '{}', ?8)",
                )
                .unwrap();
            for i in 0..10_000usize {
                let id = format!("ses_{i:05}");
                let cwd = format!("/proj/{}", i % 20);
                let name = if i == 42 {
                    "NeedleAgent".to_string()
                } else {
                    format!("worker_{i:05}")
                };
                let title = if i == 7777 {
                    "UniqueNeedleKeyword Investigation".to_string()
                } else {
                    format!("Session task {i}")
                };
                let parent = (i > 0 && i % 5 == 0).then(|| format!("ses_{:05}", i - 1));
                let mut inf = info(&id, &cwd, i as i64);
                inf.name = name.clone();
                inf.title = title.clone();
                inf.parent = parent.clone();
                s_stmt
                    .execute(params![
                        id,
                        serde_json::to_string(&inf).unwrap(),
                        cwd,
                        title,
                        name,
                        1i64,
                        i as i64,
                        parent
                    ])
                    .unwrap();
            }
            drop(s_stmt);

            let mut m_stmt = tx
                .prepare(
                    "INSERT INTO messages (session_id, seq, id, time, data, layout_meta)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                )
                .unwrap();
            for seq in 0..5_000usize {
                let mut m = msg(&format!("msg_{seq:04}"), &format!("message body {seq}"));
                if seq == 3500 {
                    m.source = MessageSource::Compaction;
                }
                let data = serde_json::to_string(&m).unwrap();
                let meta = serde_json::to_string(&compute_layout_meta(seq as u64, &m)).unwrap();
                m_stmt
                    .execute(params!["ses_09999", seq as i64, m.id, m.time, data, meta])
                    .unwrap();
            }
            drop(m_stmt);
            tx.commit().unwrap();

            let eqp = |sql: &str, p: &[&dyn rusqlite::ToSql]| -> Vec<String> {
                let mut st = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
                st.query_map(p, |r| r.get::<_, String>(3))
                    .unwrap()
                    .map(|r| r.unwrap())
                    .collect()
            };

            let p_cwd = eqp(
                "SELECT s.info FROM sessions s WHERE s.cwd = ?1 ORDER BY s.updated_at DESC LIMIT 20",
                &[&"/proj/3"],
            );
            assert!(
                p_cwd.iter().any(|d| d.contains("sessions_cwd")),
                "{p_cwd:?}"
            );

            let (w_fts, p_vals) =
                build_session_where(None, None, &[], Some("UniqueNeedleKeyword"), &[]);
            let fts_sql = format!(
                "SELECT s.info FROM sessions s {w_fts} ORDER BY s.updated_at DESC LIMIT 20"
            );
            let mut st = conn
                .prepare(&format!("EXPLAIN QUERY PLAN {fts_sql}"))
                .unwrap();
            let p_fts: Vec<String> = st
                .query_map(params_from_iter(p_vals), |r| r.get::<_, String>(3))
                .unwrap()
                .map(|r| r.unwrap())
                .collect();
            assert!(
                p_fts.iter().any(|d| d.contains("sessions_fts")),
                "{p_fts:?}"
            );
            assert!(
                !p_fts.iter().any(|d| d == "SCAN s" || d == "SCAN sessions"),
                "unexpected full table scan: {p_fts:?}"
            );

            let p_msg = eqp(
                "SELECT data FROM messages WHERE session_id = ?1 AND id = ?2 LIMIT 1",
                &[&"ses_09999", &"msg_4200"],
            );
            assert!(p_msg.iter().any(|d| d.contains("messages_id")), "{p_msg:?}");

            let p_name = eqp(
                "SELECT id, name FROM sessions WHERE name = ?1 COLLATE NOCASE",
                &[&"needleagent"],
            );
            assert!(
                p_name.iter().any(|d| d.contains("sessions_name_nocase")),
                "{p_name:?}"
            );
        }

        let store = Store::new(dir.clone());
        assert_eq!(store.resolve_id("needleagent").unwrap(), "ses_00042");
        let hits = store.list_for(None, None, Some("UniqueNeedleKeyword".into()), Some(10));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "ses_07777");
        // Short (<3 chars) query fallback works cleanly too.
        assert!(
            !store
                .list_for(None, None, Some("42".into()), Some(5))
                .is_empty()
        );

        let active = store.load_active_window("ses_09999").unwrap();
        assert_eq!(active.window_start_seq, 3500);
        assert_eq!(active.messages.len(), 1500);
        assert_eq!(active.messages[0].id, "msg_3500");

        let single = store
            .load_message("ses_09999", "msg_0123")
            .unwrap()
            .unwrap();
        assert_eq!(single.id, "msg_0123");

        let (before_msgs, s_seq, e_seq) = store
            .load_messages_before("ses_09999", 100, 50, 1 << 20)
            .unwrap();
        assert_eq!((before_msgs.len(), s_seq, e_seq), (50, 50, 100));

        let _ = store.close();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn stash_groups_cascade_lock_and_merge() {
        let dir = tmp();
        let store = Store::new(dir.clone());
        store.save(persisted("ses_1", "/a", 10, vec![msg("m1", "a")]));
        store.save(persisted("ses_2", "/a", 20, vec![msg("m2", "b")]));
        store.save(persisted("ses_3", "/a", 30, vec![msg("m3", "c")]));
        store.flush().unwrap();

        let gs = store
            .create_group(
                "grp_1".into(),
                "G1".into(),
                vec!["ses_1".into(), "ses_2".into()],
            )
            .unwrap();
        assert_eq!(gs.len(), 1);
        assert_eq!(gs[0].members.len(), 2);

        let gs = store
            .create_group(
                "grp_2".into(),
                "G2".into(),
                vec!["ses_2".into(), "ses_3".into()],
            )
            .unwrap();
        assert_eq!(gs.len(), 2);

        let gs = store.merge_groups("grp_2".into(), "grp_1".into()).unwrap();
        assert_eq!(gs.len(), 1);
        assert_eq!(
            gs[0]
                .members
                .iter()
                .map(|m| m.0.as_str())
                .collect::<Vec<_>>(),
            ["ses_1", "ses_2", "ses_3"]
        );

        // Deleting a session cascades out of session_group_members.
        assert!(store.delete_session("ses_2").unwrap());
        let gs = store.list_groups_raw().unwrap();
        assert_eq!(
            gs[0]
                .members
                .iter()
                .map(|m| m.0.as_str())
                .collect::<Vec<_>>(),
            ["ses_1", "ses_3"]
        );

        // Restoring members of unlocked group deletes group when last member is restored.
        let gs = store
            .finish_restore_group_member("grp_1".into(), "ses_1".into())
            .unwrap();
        assert_eq!(gs.len(), 1);
        let gs = store
            .finish_restore_group_member("grp_1".into(), "ses_3".into())
            .unwrap();
        assert!(gs.is_empty());

        let _ = store.close();
        let _ = std::fs::remove_dir_all(dir);
    }
}
