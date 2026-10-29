//! Disk persistence of sessions: `<data_dir>/sessions/<id>.json` (full) and `<id>.meta.json` (info only).

use crate::util::write_atomic;
use ref_proto::{Message, SessionInfo};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Persisted {
    pub info: SessionInfo,
    pub messages: Vec<Message>,
    /// must_report bookkeeping.
    #[serde(default)]
    pub reported: bool,
}

#[derive(Debug, Clone)]
pub struct Store {
    pub dir: PathBuf,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

impl Store {
    pub fn new(data_dir: PathBuf) -> Store {
        Store { dir: data_dir.join("sessions") }
    }

    fn full_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    fn meta_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.meta.json"))
    }

    pub fn save(&self, p: &Persisted) -> std::io::Result<()> {
        if !valid_id(&p.info.id) {
            return Err(std::io::Error::other("invalid session id"));
        }
        let full = serde_json::to_vec(p).map_err(std::io::Error::other)?;
        write_atomic(&self.full_path(&p.info.id), &full, true)?;
        let meta = serde_json::to_vec(&p.info).map_err(std::io::Error::other)?;
        write_atomic(&self.meta_path(&p.info.id), &meta, true)
    }

    pub fn load(&self, id: &str) -> Result<Persisted, String> {
        if !valid_id(id) {
            return Err(format!("Invalid session id: {id}"));
        }
        let raw = std::fs::read(self.full_path(id)).map_err(|_| format!("Session not found: {id}"))?;
        serde_json::from_slice(&raw).map_err(|e| format!("Session file {id} is corrupt: {e}"))
    }

    pub fn exists(&self, id: &str) -> bool {
        valid_id(id) && self.full_path(id).is_file()
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        if !valid_id(id) {
            return Err(format!("Invalid session id: {id}"));
        }
        let a = std::fs::remove_file(self.full_path(id));
        let _ = std::fs::remove_file(self.meta_path(id));
        a.map_err(|_| format!("Session not found: {id}"))
    }

    /// All persisted session infos, newest `updated_at` first.
    pub fn list(&self) -> Vec<SessionInfo> {
        let mut out = Vec::new();
        let Ok(rd) = std::fs::read_dir(&self.dir) else { return out };
        for e in rd.flatten() {
            let name = e.file_name();
            let Some(name) = name.to_str() else { continue };
            if !name.ends_with(".meta.json") {
                continue;
            }
            let Ok(raw) = std::fs::read(e.path()) else { continue };
            if let Ok(info) = serde_json::from_slice::<SessionInfo>(&raw) {
                out.push(info);
            }
        }
        out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ref_proto::*;

    #[test]
    fn round_trip() {
        let dir = std::env::temp_dir().join(format!("ref-store-test-{}", crate::util::random_hex(8)));
        let store = Store::new(dir.clone());
        let info = SessionInfo {
            id: "ses_abc".into(),
            name: "名前".into(),
            title: "t".into(),
            agent: "build".into(),
            model: ModelRef::default(),
            cwd: "/".into(),
            created_at: 1,
            updated_at: 2,
            status: SessionStatus::Idle,
            attached: 0,
            live: false,
            parent: None,
            must_report: true,
            context: ContextUsage::default(),
            todos: Vec::new(),
        };
        let msg = Message {
            id: "msg_1".into(),
            role: Role::User,
            time: 1,
            time_end: None,
            source: MessageSource::Human,
            parts: vec![Part::Text { text: "hi".into(), time: 1 }],
            model: None,
            agent: None,
            usage: None,
            error: None,
        };
        store.save(&Persisted { info, messages: vec![msg], reported: true }).unwrap();
        let p = store.load("ses_abc").unwrap();
        assert_eq!(p.info.name, "名前");
        assert!(p.reported && p.info.must_report);
        assert_eq!(p.messages.len(), 1);
        assert_eq!(store.list().len(), 1);
        assert!(store.load("../etc").is_err());
        store.delete("ses_abc").unwrap();
        assert!(store.list().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}
