//! Per-session snapshot store: canonical path → recent versions with the lines the model saw.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use super::file_tag;

const MAX_PATHS: usize = 256;
const MAX_VERSIONS: usize = 4;
const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;
/// Files larger than this are never snapshotted (no tag is minted).
pub const MAX_SNAPSHOT_BYTES: u64 = 4 * 1024 * 1024;

/// Which lines of a version the model has seen in full.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seen {
    /// Model knows the whole content (it wrote it).
    All,
    /// Only these line numbers were displayed (line 0 = "provenance, nothing displayed").
    Lines(BTreeSet<u32>),
}

impl Seen {
    pub fn lines(lines: impl IntoIterator<Item = u32>) -> Seen {
        Seen::Lines(lines.into_iter().collect())
    }
    pub fn contains(&self, line: u32) -> bool {
        match self {
            Seen::All => true,
            Seen::Lines(set) => set.contains(&line),
        }
    }
    fn merge(&mut self, other: &Seen) {
        match (self, other) {
            (Seen::All, _) => {}
            (me @ Seen::Lines(_), Seen::All) => *me = Seen::All,
            (Seen::Lines(a), Seen::Lines(b)) => a.extend(b.iter().copied()),
        }
    }
}

/// How a version entered the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Displayed by read/grep/mismatch reply.
    Display,
    /// Produced by this session's edit/write tool.
    Written,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub path: PathBuf,
    pub text: Arc<str>,
    pub tag: String,
    pub seen: Seen,
    pub origin: Origin,
}

#[derive(Default)]
struct History {
    versions: Vec<Snapshot>,
    touched: u64,
}

#[derive(Default)]
struct State {
    histories: HashMap<PathBuf, History>,
    /// Named clipboard registers (persist across calls within a session).
    registers: HashMap<String, Vec<String>>,
    /// path → (hash of last no-op patch, consecutive count)
    noop: HashMap<PathBuf, (u64, u32)>,
    clock: u64,
}

impl State {
    fn total_bytes(&self) -> usize {
        self.histories
            .values()
            .flat_map(|h| h.versions.iter())
            .map(|v| v.text.len())
            .sum()
    }
    fn evict(&mut self) {
        while self.histories.len() > MAX_PATHS
            || (self.histories.len() > 1 && self.total_bytes() > MAX_TOTAL_BYTES)
        {
            let Some(oldest) = self
                .histories
                .iter()
                .min_by_key(|(_, h)| h.touched)
                .map(|(p, _)| p.clone())
            else {
                break;
            };
            self.histories.remove(&oldest);
        }
    }
}

/// Per-session snapshot store (path -> recent versions, seen lines, registers).
#[derive(Default)]
pub struct EditStore {
    inner: Mutex<State>,
}

impl EditStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Record a version of `key` (canonical path) with LF-normalized `text`; returns its tag.
    /// Seen lines are merged into an existing identical version.
    pub fn record(&self, key: &Path, text: &str, seen: Seen, origin: Origin) -> String {
        let tag = file_tag(text);
        if text.len() as u64 > MAX_SNAPSHOT_BYTES {
            return tag;
        }
        let mut st = self.lock();
        st.clock += 1;
        let clock = st.clock;
        let h = st.histories.entry(key.to_path_buf()).or_default();
        h.touched = clock;
        if let Some(idx) = h.versions.iter().position(|v| v.tag == tag && &*v.text == text) {
            let mut v = h.versions.remove(idx);
            v.seen.merge(&seen);
            if origin == Origin::Written {
                v.origin = Origin::Written;
            }
            h.versions.insert(0, v);
        } else {
            h.versions.insert(
                0,
                Snapshot { path: key.to_path_buf(), text: Arc::from(text), tag: tag.clone(), seen, origin },
            );
            h.versions.truncate(MAX_VERSIONS);
        }
        st.evict();
        tag
    }

    /// Mark extra lines of version `tag` as seen.
    pub fn add_seen(&self, key: &Path, tag: &str, lines: &[u32]) {
        let mut st = self.lock();
        if let Some(v) = st
            .histories
            .get_mut(key)
            .and_then(|h| h.versions.iter_mut().find(|v| v.tag == tag))
        {
            v.seen.merge(&Seen::lines(lines.iter().copied()));
        }
    }

    pub fn by_tag(&self, key: &Path, tag: &str) -> Option<Snapshot> {
        let st = self.lock();
        st.histories.get(key)?.versions.iter().find(|v| v.tag == tag).cloned()
    }

    pub fn by_content(&self, key: &Path, text: &str) -> Option<Snapshot> {
        let st = self.lock();
        st.histories.get(key)?.versions.iter().find(|v| &*v.text == text).cloned()
    }

    pub fn head(&self, key: &Path) -> Option<Snapshot> {
        let st = self.lock();
        st.histories.get(key)?.versions.first().cloned()
    }

    /// Paths (other than none) holding a version with this tag.
    pub fn paths_with_tag(&self, tag: &str) -> Vec<PathBuf> {
        let st = self.lock();
        let mut out: Vec<PathBuf> = st
            .histories
            .iter()
            .filter(|(_, h)| h.versions.iter().any(|v| v.tag == tag))
            .map(|(p, _)| p.clone())
            .collect();
        out.sort();
        out
    }

    pub fn invalidate(&self, key: &Path) {
        self.lock().histories.remove(key);
    }

    /// Move history from one path to another (after MV).
    pub fn relocate(&self, from: &Path, to: &Path) {
        let mut st = self.lock();
        if let Some(mut h) = st.histories.remove(from) {
            for v in &mut h.versions {
                v.path = to.to_path_buf();
            }
            st.histories.insert(to.to_path_buf(), h);
        }
    }

    pub fn registers(&self) -> HashMap<String, Vec<String>> {
        self.lock().registers.clone()
    }

    pub fn commit_registers(&self, regs: HashMap<String, Vec<String>>) {
        self.lock().registers.extend(regs);
    }

    /// Count consecutive identical no-op patches for a path; returns the new count.
    pub fn note_noop(&self, key: &Path, patch_hash: u64) -> u32 {
        let mut st = self.lock();
        let e = st.noop.entry(key.to_path_buf()).or_insert((patch_hash, 0));
        if e.0 != patch_hash {
            *e = (patch_hash, 0);
        }
        e.1 += 1;
        e.1
    }

    pub fn clear_noop(&self, key: &Path) {
        self.lock().noop.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_merges_seen_and_caps_versions() {
        let s = EditStore::new();
        let p = Path::new("/x/a.rs");
        let t = s.record(p, "a\nb\n", Seen::lines([1]), Origin::Display);
        s.record(p, "a\nb\n", Seen::lines([2]), Origin::Display);
        let snap = s.by_tag(p, &t).unwrap();
        assert!(snap.seen.contains(1) && snap.seen.contains(2) && !snap.seen.contains(3));
        for i in 0..10 {
            s.record(p, &format!("v{i}\n"), Seen::All, Origin::Written);
        }
        assert!(s.by_tag(p, &t).is_none());
        assert_eq!(&*s.head(p).unwrap().text, "v9\n");
    }

    #[test]
    fn seen_all_absorbs() {
        let mut a = Seen::lines([1]);
        a.merge(&Seen::All);
        assert_eq!(a, Seen::All);
        a.merge(&Seen::lines([3]));
        assert_eq!(a, Seen::All);
    }
}
