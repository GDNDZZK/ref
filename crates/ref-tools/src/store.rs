//! Session-scoped snapshot store: remembers file texts we showed to the model (by tag) and which
//! lines of each snapshot were actually displayed ("seen").

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::hash::file_hash;

pub const MAX_VERSIONS_PER_PATH: usize = 4;
pub const MAX_PATHS: usize = 256;
pub const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;
/// Files larger than this are never snapshotted from disk.
pub const MAX_SNAPSHOT_FILE_BYTES: usize = 4 * 1024 * 1024;

/// Which lines of a snapshot were displayed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seen {
    /// Every line may be edited (e.g. the model wrote the whole file).
    All,
    /// Only these 1-based line numbers were displayed.
    Lines(BTreeSet<usize>),
}

impl Seen {
    pub fn none() -> Seen {
        Seen::Lines(BTreeSet::new())
    }
    pub fn contains(&self, line: usize) -> bool {
        match self {
            Seen::All => true,
            Seen::Lines(s) => s.contains(&line),
        }
    }
    fn union(&mut self, other: &Seen) {
        match (&mut *self, other) {
            (Seen::All, _) => {}
            (_, Seen::All) => *self = Seen::All,
            (Seen::Lines(a), Seen::Lines(b)) => a.extend(b.iter().copied()),
        }
    }
}

impl<I: IntoIterator<Item = usize>> From<I> for Seen {
    fn from(it: I) -> Self {
        Seen::Lines(it.into_iter().collect())
    }
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    /// LF-normalized, BOM-stripped text.
    pub text: Arc<str>,
    pub hash: String,
    pub seen: Seen,
}

#[derive(Default)]
struct Entry {
    /// Newest first.
    versions: VecDeque<Snapshot>,
    tick: u64,
}

/// Per-session snapshot store for hashline tags.
#[derive(Default)]
pub struct SnapshotStore {
    entries: HashMap<PathBuf, Entry>,
    tick: u64,
    total_bytes: usize,
}

impl SnapshotStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record `text` for `path` (a canonical key), merging `seen` into an identical existing
    /// snapshot. Returns the tag.
    pub fn record(&mut self, path: &Path, text: &str, seen: Seen) -> String {
        let hash = file_hash(text);
        self.tick += 1;
        let tick = self.tick;
        let entry = self.entries.entry(path.to_path_buf()).or_default();
        entry.tick = tick;
        if let Some(pos) = entry.versions.iter().position(|s| &*s.text == text) {
            if let Some(mut snap) = entry.versions.remove(pos) {
                snap.seen.union(&seen);
                entry.versions.push_front(snap);
            }
        } else {
            self.total_bytes += text.len();
            entry.versions.push_front(Snapshot {
                text: Arc::from(text),
                hash: hash.clone(),
                seen,
            });
            while entry.versions.len() > MAX_VERSIONS_PER_PATH {
                if let Some(old) = entry.versions.pop_back() {
                    self.total_bytes = self.total_bytes.saturating_sub(old.text.len());
                }
            }
        }
        self.evict(path);
        hash
    }

    /// Mark additional lines of the snapshot with `hash` as seen.
    pub fn mark_seen(&mut self, path: &Path, hash: &str, lines: impl IntoIterator<Item = usize>) {
        if let Some(e) = self.entries.get_mut(path) {
            if let Some(s) = e
                .versions
                .iter_mut()
                .find(|s| s.hash.eq_ignore_ascii_case(hash))
            {
                s.seen.union(&Seen::Lines(lines.into_iter().collect()));
            }
        }
    }

    /// Newest snapshot of `path` with this tag.
    pub fn by_hash(&self, path: &Path, hash: &str) -> Option<&Snapshot> {
        self.entries
            .get(path)?
            .versions
            .iter()
            .find(|s| s.hash.eq_ignore_ascii_case(hash))
    }

    /// Number of retained snapshots of `path`.
    pub fn version_count(&self, path: &Path) -> usize {
        self.entries.get(path).map_or(0, |e| e.versions.len())
    }

    /// Newest snapshot of `path`.
    pub fn latest(&self, path: &Path) -> Option<&Snapshot> {
        self.entries.get(path)?.versions.front()
    }

    /// Another path for which this tag was issued (for error hints).
    pub fn path_with_hash(&self, hash: &str, except: &Path) -> Option<PathBuf> {
        self.entries
            .iter()
            .filter(|(p, _)| p.as_path() != except)
            .find(|(_, e)| e.versions.iter().any(|s| s.hash.eq_ignore_ascii_case(hash)))
            .map(|(p, _)| p.clone())
    }

    /// Forget a path and everything below it (when deleted).
    pub fn remove(&mut self, path: &Path) {
        let keys: Vec<PathBuf> = self
            .entries
            .keys()
            .filter(|k| k.starts_with(path))
            .cloned()
            .collect();
        for k in keys {
            if let Some(e) = self.entries.remove(&k) {
                let bytes: usize = e.versions.iter().map(|s| s.text.len()).sum();
                self.total_bytes = self.total_bytes.saturating_sub(bytes);
            }
        }
    }

    pub fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    pub fn path_count(&self) -> usize {
        self.entries.len()
    }

    fn evict(&mut self, keep: &Path) {
        // Path-count LRU.
        while self.entries.len() > MAX_PATHS {
            if !self.evict_oldest_path(keep) {
                break;
            }
        }
        // Byte budget: drop oldest versions of other paths first, then whole LRU paths.
        while self.total_bytes > MAX_TOTAL_BYTES {
            if !self.evict_oldest_path(keep) {
                // Only `keep` left: trim its older versions.
                let Some(e) = self.entries.get_mut(keep) else {
                    break;
                };
                if e.versions.len() <= 1 {
                    break;
                }
                if let Some(old) = e.versions.pop_back() {
                    self.total_bytes = self.total_bytes.saturating_sub(old.text.len());
                }
            }
        }
    }

    fn evict_oldest_path(&mut self, keep: &Path) -> bool {
        let victim = self
            .entries
            .iter()
            .filter(|(p, _)| p.as_path() != keep)
            .min_by_key(|(_, e)| e.tick)
            .map(|(p, _)| p.clone());
        match victim {
            Some(v) => {
                if let Some(e) = self.entries.remove(&v) {
                    let bytes: usize = e.versions.iter().map(|s| s.text.len()).sum();
                    self.total_bytes = self.total_bytes.saturating_sub(bytes);
                }
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_dedupe_and_bounds() {
        let mut s = SnapshotStore::new();
        let p = Path::new("/x/a");
        let t1 = s.record(p, "a\n", Seen::from([1]));
        let t2 = s.record(p, "b\n", Seen::none());
        assert_ne!(t1, t2);
        s.record(p, "a\n", Seen::from([2]));
        let snap = s.latest(p).unwrap();
        assert_eq!(snap.hash, t1);
        assert!(snap.seen.contains(1) && snap.seen.contains(2));
        for i in 0..10 {
            s.record(p, &format!("{i}\n"), Seen::All);
        }
        assert_eq!(s.entries[p].versions.len(), MAX_VERSIONS_PER_PATH);
        for i in 0..300 {
            s.record(&PathBuf::from(format!("/p/{i}")), "z", Seen::All);
        }
        assert_eq!(s.path_count(), MAX_PATHS);
        s.remove(Path::new("/p"));
        assert_eq!(s.path_count(), 0);
        assert_eq!(s.total_bytes(), 0);
    }
}
