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

use ref_proto::{Message, SessionInfo};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::Hasher;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

const SCHEMA_VERSION: i64 = 1;

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
}

/// Session state besides info/messages, stored as JSON in `sessions.extra`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Extra {
    #[serde(default)]
    reported: bool,
    #[serde(default)]
    context_notice: bool,
}

type Reply<T> = mpsc::Sender<Result<T, String>>;

enum Cmd {
    Save(Box<Persisted>),
    Load(String, Reply<Persisted>),
    Exists(String, Reply<bool>),
    List {
        cwd: Option<String>,
        limit: Option<usize>,
        reply: Reply<Vec<SessionInfo>>,
    },
    Delete(String, Reply<()>),
    Flush(Reply<()>),
}

#[derive(Debug, Clone)]
pub struct Store {
    pub path: PathBuf,
    tx: mpsc::Sender<Cmd>,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

impl Store {
    /// Opens (or creates) `<data_dir>/ref.db` on a background thread.
    pub fn new(data_dir: PathBuf) -> Store {
        let path = data_dir.join("ref.db");
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
            .map_err(|_| "session store is not available".to_string())?;
        rx.recv()
            .map_err(|_| "session store is not available".to_string())?
    }

    /// Queue an incremental save (non-blocking). Sessions without messages that were never
    /// saved are skipped.
    pub fn save(&self, p: Persisted) {
        if !valid_id(&p.info.id) {
            tracing::warn!("not persisting session with invalid id {:?}", p.info.id);
            return;
        }
        let _ = self.tx.send(Cmd::Save(Box::new(p)));
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
        valid_id(id)
            && self
                .call(|r| Cmd::Exists(id.to_string(), r))
                .unwrap_or(false)
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        if !valid_id(id) {
            return Err(format!("Invalid session id: {id}"));
        }
        self.call(|r| Cmd::Delete(id.to_string(), r))
    }

    /// All persisted session infos, newest `updated_at` first.
    pub fn list(&self) -> Vec<SessionInfo> {
        self.list_for(None, None)
    }

    /// Persisted session infos (optionally only for `cwd`), newest first, at most `limit`.
    pub fn list_for(&self, cwd: Option<String>, limit: Option<usize>) -> Vec<SessionInfo> {
        self.call(|reply| Cmd::List { cwd, limit, reply })
            .unwrap_or_else(|e| {
                tracing::warn!("listing sessions failed: {e}");
                Vec::new()
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
    let _ = serde_json::to_writer(&mut w, v);
    w.0.finish()
}

fn open(data_dir: &Path, path: &Path) -> Result<Connection, String> {
    std::fs::create_dir_all(data_dir)
        .map_err(|e| format!("cannot create {}: {e}", data_dir.display()))?;
    let conn =
        Connection::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| e.to_string())?;
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(|e| e.to_string())?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> Result<(), String> {
    let v: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if v > SCHEMA_VERSION {
        return Err(format!(
            "database schema version {v} is newer than supported ({SCHEMA_VERSION})"
        ));
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
        // A save is skipped when a later save of the same session follows before any other
        // command (reads must observe the state queued before them).
        let mut skip = vec![false; batch.len()];
        for i in 0..batch.len() {
            let Cmd::Save(p) = &batch[i] else { continue };
            for later in &batch[i + 1..] {
                match later {
                    Cmd::Save(q) if q.info.id == p.info.id => {
                        skip[i] = true;
                        break;
                    }
                    Cmd::Save(_) => {}
                    _ => break,
                }
            }
        }
        for (c, skip) in batch.into_iter().zip(skip) {
            let Some(db) = conn.as_mut() else {
                fail(c, "session store is not available (see server log)");
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
                    let _ = r.send(load(db, &mut cache, &id));
                }
                Cmd::Exists(id, r) => {
                    let _ = r.send(exists(db, &id));
                }
                Cmd::List { cwd, limit, reply } => {
                    let _ = reply.send(list(db, cwd.as_deref(), limit));
                }
                Cmd::Delete(id, r) => {
                    cache.remove(&id);
                    let _ = r.send(delete(db, &id));
                }
                Cmd::Flush(r) => {
                    let _ = r.send(Ok(()));
                }
            }
        }
    }
}

fn fail(c: Cmd, msg: &str) {
    let m = msg.to_string();
    match c {
        Cmd::Save(_) => {}
        Cmd::Load(_, r) => {
            let _ = r.send(Err(m));
        }
        Cmd::Exists(_, r) => {
            let _ = r.send(Err(m));
        }
        Cmd::List { reply, .. } => {
            let _ = reply.send(Err(m));
        }
        Cmd::Delete(_, r) | Cmd::Flush(r) => {
            let _ = r.send(Err(m));
        }
    }
}

fn exists(db: &Connection, id: &str) -> Result<bool, String> {
    db.query_row("SELECT 1 FROM sessions WHERE id = ?1", [id], |_| Ok(()))
        .optional()
        .map(|r| r.is_some())
        .map_err(|e| e.to_string())
}

fn save(
    db: &mut Connection,
    cache: &mut HashMap<String, Cached>,
    p: &Persisted,
) -> Result<(), String> {
    let id = p.info.id.as_str();
    let extra = Extra {
        reported: p.reported,
        context_notice: p.context_notice,
    };
    let info_hash = hash_json(&(&p.info, &extra));
    let known = cache.contains_key(id);
    if !known && p.messages.is_empty() && !exists(db, id)? {
        // Never persisted and still empty: nothing worth keeping.
        return Ok(());
    }
    let hashes: Vec<u64> = p.messages.iter().map(hash_json).collect();
    let entry = cache.entry(id.to_string()).or_default();
    let tx = db.transaction().map_err(|e| e.to_string())?;
    if !known || entry.info != info_hash {
        let info = serde_json::to_string(&p.info).map_err(|e| e.to_string())?;
        let extra = serde_json::to_string(&extra).map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO sessions (id, info, cwd, title, name, created_at, updated_at, extra)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET info = excluded.info, cwd = excluded.cwd, title = excluded.title,
                 name = excluded.name, created_at = excluded.created_at, updated_at = excluded.updated_at,
                 extra = excluded.extra",
            params![id, info, p.info.cwd, p.info.title, p.info.name, p.info.created_at, p.info.updated_at, extra],
        )
        .map_err(|e| e.to_string())?;
    }
    {
        let mut up = tx
            .prepare_cached(
                "INSERT INTO messages (session_id, seq, id, time, data) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(session_id, seq) DO UPDATE SET id = excluded.id, time = excluded.time, data = excluded.data",
            )
            .map_err(|e| e.to_string())?;
        for (seq, (m, h)) in p.messages.iter().zip(&hashes).enumerate() {
            if known && entry.msgs.get(seq) == Some(h) {
                continue;
            }
            let data = serde_json::to_string(m).map_err(|e| e.to_string())?;
            up.execute(params![id, seq as i64, m.id, m.time, data])
                .map_err(|e| e.to_string())?;
        }
    }
    if !known || entry.msgs.len() > hashes.len() {
        tx.execute(
            "DELETE FROM messages WHERE session_id = ?1 AND seq >= ?2",
            params![id, hashes.len() as i64],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    entry.info = info_hash;
    entry.msgs = hashes;
    Ok(())
}

fn load(
    db: &Connection,
    cache: &mut HashMap<String, Cached>,
    id: &str,
) -> Result<Persisted, String> {
    let row: Option<(String, Option<String>)> = db
        .query_row(
            "SELECT info, extra FROM sessions WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((info, extra)) = row else {
        return Err(format!("Session not found: {id}"));
    };
    let info: SessionInfo =
        serde_json::from_str(&info).map_err(|e| format!("Session {id} is corrupt: {e}"))?;
    let extra: Extra = extra
        .and_then(|x| serde_json::from_str(&x).ok())
        .unwrap_or_default();
    let mut stmt = db
        .prepare_cached("SELECT seq, data FROM messages WHERE session_id = ?1 ORDER BY seq")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?;
    let mut messages = Vec::new();
    let mut contiguous = true;
    for r in rows {
        let (seq, data) = r.map_err(|e| e.to_string())?;
        contiguous &= seq == messages.len() as i64;
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
    };
    if contiguous {
        let e = Extra {
            reported: p.reported,
            context_notice: p.context_notice,
        };
        cache.insert(
            id.to_string(),
            Cached {
                info: hash_json(&(&p.info, &e)),
                msgs: p.messages.iter().map(hash_json).collect(),
            },
        );
    } else {
        // Rows do not map 1:1 to positions: rewrite everything on the next save.
        cache.remove(id);
    }
    Ok(p)
}

fn list(
    db: &Connection,
    cwd: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<SessionInfo>, String> {
    let limit = limit.map(|l| l as i64).unwrap_or(-1);
    let mut stmt = db
        .prepare_cached(
            "SELECT info FROM sessions WHERE (?1 IS NULL OR cwd = ?1) ORDER BY updated_at DESC LIMIT ?2",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![cwd, limit], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        let raw = r.map_err(|e| e.to_string())?;
        match serde_json::from_str::<SessionInfo>(&raw) {
            Ok(i) => out.push(i),
            Err(e) => tracing::warn!("skipping corrupt session row: {e}"),
        }
    }
    Ok(out)
}

fn delete(db: &mut Connection, id: &str) -> Result<(), String> {
    let tx = db.transaction().map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM messages WHERE session_id = ?1", [id])
        .map_err(|e| e.to_string())?;
    let n = tx
        .execute("DELETE FROM sessions WHERE id = ?1", [id])
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    if n == 0 {
        Err(format!("Session not found: {id}"))
    } else {
        Ok(())
    }
}

/// One-time import of `<data_dir>/sessions/*.json`; the directory is renamed afterwards.
fn import_json(db: &mut Connection, data_dir: &Path) -> Result<(), String> {
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
    let tx = db.transaction().map_err(|e| e.to_string())?;
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
        };
        tx.execute(
            "INSERT INTO sessions (id, info, cwd, title, name, created_at, updated_at, extra)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                p.info.id,
                serde_json::to_string(&p.info).map_err(|e| e.to_string())?,
                p.info.cwd,
                p.info.title,
                p.info.name,
                p.info.created_at,
                p.info.updated_at,
                serde_json::to_string(&extra).map_err(|e| e.to_string())?
            ],
        )
        .map_err(|e| e.to_string())?;
        for (seq, m) in p.messages.iter().enumerate() {
            tx.execute(
                "INSERT INTO messages (session_id, seq, id, time, data) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![p.info.id, seq as i64, m.id, m.time, serde_json::to_string(m).map_err(|e| e.to_string())?],
            )
            .map_err(|e| e.to_string())?;
        }
        imported += 1;
    }
    tx.commit().map_err(|e| e.to_string())?;
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
        "ref.db",
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

    #[test]
    fn round_trip_incremental_and_reopen() {
        let dir = tmp();
        let store = Store::new(dir.clone());
        // Empty, never-saved sessions are not stored.
        store.save(Persisted {
            info: info("ses_empty", "/", 1),
            messages: vec![],
            reported: false,
            context_notice: false,
        });
        assert!(!store.exists("ses_empty"));

        let mut p = Persisted {
            info: info("ses_abc", "/a", 2),
            messages: vec![msg("m1", "hi"), msg("m2", "there"), msg("m3", "x")],
            reported: true,
            context_notice: true,
        };
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

        store.save(Persisted {
            info: info("ses_b", "/b", 5),
            messages: vec![msg("x", "y")],
            reported: false,
            context_notice: false,
        });
        let all = store.list();
        assert_eq!(
            all.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            ["ses_b", "ses_abc"]
        );
        assert_eq!(store.list_for(Some("/a".into()), None).len(), 1);
        assert_eq!(store.list_for(None, Some(1)).len(), 1);
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
        let p = Persisted {
            info: info("ses_old", "/old", 7),
            messages: vec![msg("m1", "old one"), msg("m2", "old two")],
            reported: true,
            context_notice: false,
        };
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
}
