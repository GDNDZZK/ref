//! Session persistence: `<data_dir>/sessions/<id>.meta.json` + `<id>.jsonl`.
//!
//! The jsonl file holds one record per line: a full `Message` (last record per message id
//! wins) or `{"removed": "<id>"}`. It is rewritten (compacted) on close or when it contains
//! more than twice as many lines as live messages.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use ref_proto::{ContextInfo, Message, SessionInfo, SessionStatus};
use serde::{Deserialize, Serialize};

use crate::util;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: String,
    pub name: String,
    pub cwd: String,
    pub agent: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    pub created: i64,
    pub updated: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default)]
    pub must_report: bool,
    #[serde(default)]
    pub cost: f64,
    /// Name was generated (default / title) and may be replaced by a generated title.
    #[serde(default)]
    pub auto_name: bool,
    #[serde(default)]
    pub message_count: u32,
    #[serde(default)]
    pub context: ContextInfo,
}

impl SessionMeta {
    pub fn from_info(info: &SessionInfo, auto_name: bool) -> Self {
        SessionMeta {
            id: info.id.clone(),
            name: info.name.clone(),
            cwd: info.cwd.clone(),
            agent: info.agent.clone(),
            model: info.model.clone(),
            variant: info.variant.clone(),
            created: info.created,
            updated: info.updated,
            parent: info.parent.clone(),
            must_report: info.must_report,
            cost: info.cost,
            auto_name,
            message_count: info.message_count,
            context: info.context.clone(),
        }
    }

    pub fn to_info(&self) -> SessionInfo {
        SessionInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            cwd: self.cwd.clone(),
            agent: self.agent.clone(),
            model: self.model.clone(),
            variant: self.variant.clone(),
            created: self.created,
            updated: self.updated,
            open: false,
            attached: 0,
            status: SessionStatus::Idle,
            context: self.context.clone(),
            cost: self.cost,
            parent: self.parent.clone(),
            must_report: self.must_report,
            message_count: self.message_count,
            pending_permissions: 0,
        }
    }
}

/// Session ids come from clients: only allow safe file-name characters.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

pub fn meta_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.meta.json"))
}

pub fn log_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.jsonl"))
}

pub fn write_meta(dir: &Path, meta: &SessionMeta) -> std::io::Result<()> {
    let data = serde_json::to_vec_pretty(meta).map_err(std::io::Error::other)?;
    util::write_atomic(&meta_path(dir, &meta.id), &data)
}

pub fn read_meta(dir: &Path, id: &str) -> Option<SessionMeta> {
    let b = std::fs::read(meta_path(dir, id)).ok()?;
    serde_json::from_slice(&b).ok()
}

/// All session metas in the directory (unreadable ones are skipped with a warning).
pub fn list_metas(dir: &Path) -> Vec<SessionMeta> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(id) = name.strip_suffix(".meta.json") else { continue };
        if !valid_id(id) {
            continue;
        }
        match read_meta(dir, id) {
            Some(m) if m.id == id => out.push(m),
            _ => tracing::warn!("skipping unreadable session meta {name}"),
        }
    }
    out
}

pub fn delete(dir: &Path, id: &str) {
    for p in [meta_path(dir, id), log_path(dir, id)] {
        if let Err(e) = std::fs::remove_file(&p) {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!("failed to delete {}: {e}", p.display());
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Removed {
    removed: String,
}

/// Parse a jsonl message log. Returns the messages (first-appearance order, last record wins)
/// and the number of records read.
pub fn parse_log(reader: impl BufRead) -> (Vec<Message>, usize) {
    let mut order: Vec<Option<Message>> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut lines = 0;
    for line in reader.lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        lines += 1;
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            tracing::warn!("skipping corrupt session log line");
            continue;
        };
        if let Some(id) = v.get("removed").and_then(|x| x.as_str()) {
            if v.get("role").is_none() {
                if let Some(i) = index.remove(id) {
                    order[i] = None;
                }
                continue;
            }
        }
        match serde_json::from_value::<Message>(v) {
            Ok(m) => match index.get(&m.id) {
                Some(&i) => order[i] = Some(m),
                None => {
                    index.insert(m.id.clone(), order.len());
                    order.push(Some(m));
                }
            },
            Err(e) => tracing::warn!("skipping invalid message record: {e}"),
        }
    }
    (order.into_iter().flatten().collect(), lines)
}

pub fn load_messages(dir: &Path, id: &str) -> (Vec<Message>, usize) {
    match std::fs::File::open(log_path(dir, id)) {
        Ok(f) => parse_log(std::io::BufReader::new(f)),
        Err(_) => (Vec::new(), 0),
    }
}

/// Append-only writer for one session's log.
pub struct SessionLog {
    path: PathBuf,
    /// Records currently in the file.
    pub lines: usize,
}

pub enum Record<'a> {
    Message(&'a Message),
    Removed(&'a str),
}

impl SessionLog {
    pub fn new(dir: &Path, id: &str, lines: usize) -> Self {
        SessionLog { path: log_path(dir, id), lines }
    }

    pub fn append(&mut self, records: &[Record]) -> std::io::Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        let mut buf = Vec::new();
        for r in records {
            match r {
                Record::Message(m) => serde_json::to_writer(&mut buf, m).map_err(std::io::Error::other)?,
                Record::Removed(id) => serde_json::to_writer(&mut buf, &Removed { removed: id.to_string() })
                    .map_err(std::io::Error::other)?,
            }
            buf.push(b'\n');
        }
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut f = util::open_private(&self.path, false)?;
        f.write_all(&buf)?;
        self.lines += records.len();
        Ok(())
    }

    /// Rewrite the file with exactly `messages`.
    pub fn rewrite(&mut self, messages: &[Message]) -> std::io::Result<()> {
        let mut buf = Vec::new();
        for m in messages {
            serde_json::to_writer(&mut buf, m).map_err(std::io::Error::other)?;
            buf.push(b'\n');
        }
        util::write_atomic(&self.path, &buf)?;
        self.lines = messages.len();
        Ok(())
    }

    pub fn needs_compaction(&self, live: usize) -> bool {
        self.lines > 2 * live + 16
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ref_proto::{MessageOrigin, Part, Role, TextPart};

    fn msg(text: &str) -> Message {
        let mut m = Message::new(Role::User, MessageOrigin::User);
        m.parts.push(Part::Text(TextPart { id: "p".into(), text: text.into(), time: 0, meta: None }));
        m
    }

    #[test]
    fn log_roundtrip_last_wins_and_removal() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = SessionLog::new(dir.path(), "ses_1", 0);
        let a = msg("a");
        let mut b = msg("b");
        let c = msg("c");
        log.append(&[Record::Message(&a), Record::Message(&b), Record::Message(&c)]).unwrap();
        b.parts.clear();
        b.parts.push(Part::Text(TextPart { id: "p".into(), text: "b2".into(), time: 0, meta: None }));
        log.append(&[Record::Message(&b), Record::Removed(&a.id)]).unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(log_path(dir.path(), "ses_1"))
            .unwrap()
            .write_all(b"{garbage\n")
            .unwrap();
        let (msgs, lines) = load_messages(dir.path(), "ses_1");
        assert_eq!(lines, 6);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].raw_text(), "b2");
        assert_eq!(msgs[1].raw_text(), "c");
        assert!(!log.needs_compaction(2));
        log.rewrite(&msgs).unwrap();
        let (again, lines) = load_messages(dir.path(), "ses_1");
        assert_eq!(lines, 2);
        assert_eq!(again.len(), 2);
    }

    #[test]
    fn meta_roundtrip_and_ids() {
        let dir = tempfile::tempdir().unwrap();
        let meta = SessionMeta {
            id: "ses_x".into(),
            name: "名字".into(),
            cwd: "/tmp".into(),
            agent: "build".into(),
            model: "p/m".into(),
            variant: None,
            created: 1,
            updated: 2,
            parent: None,
            must_report: true,
            cost: 0.5,
            auto_name: false,
            message_count: 3,
            context: ContextInfo::default(),
        };
        write_meta(dir.path(), &meta).unwrap();
        let all = list_metas(dir.path());
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "名字");
        assert!(all[0].to_info().must_report);
        assert!(valid_id("ses_01abc-Z"));
        assert!(!valid_id("../etc"));
        assert!(!valid_id(""));
        delete(dir.path(), "ses_x");
        assert!(list_metas(dir.path()).is_empty());
    }
}
