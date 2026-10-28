//! Hashline snapshots: whole-file content tags and the per-session snapshot store.
//!
//! A file is identified to the model by a 4-hex TAG (`[path#TAG]` header) computed with
//! [`file_hash`] over its BOM-stripped, LF-normalized text. Every tag handed out is
//! recorded in the [`HashStore`] together with the full text and the set of line numbers
//! the model has actually been shown, so `edit` can validate anchors and remap stale ones.

use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use xxhash_rust::xxh32::Xxh32;

/// Full-file versions retained per path.
pub const MAX_VERSIONS_PER_PATH: usize = 4;
/// Retained path count before LRU eviction.
pub const MAX_PATHS: usize = 256;
/// Global ceiling on retained snapshot bytes (estimated).
pub const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;
/// Files larger than this are never snapshotted.
pub const MAX_SNAPSHOT_FILE_BYTES: usize = 4 * 1024 * 1024;

/// Four-hex uppercase content tag of `text`.
///
/// XXH32 (seed 0) over the text with trailing spaces, tabs and CRs stripped from every
/// line; low 16 bits formatted as `{:04X}`. Callers pass BOM-stripped, LF-normalized text.
pub fn file_hash(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut hasher = Xxh32::new(0);
    let mut run_start = 0;
    let mut line_start = 0;
    for segment in text.split_inclusive('\n') {
        let line = segment.strip_suffix('\n').unwrap_or(segment);
        let kept = line.trim_end_matches([' ', '\t', '\r']).len();
        if kept < line.len() {
            hasher.update(&bytes[run_start..line_start + kept]);
            run_start = line_start + line.len();
        }
        line_start += segment.len();
    }
    hasher.update(&bytes[run_start..]);
    format!("{:04X}", hasher.digest() & 0xffff)
}

/// `[path#TAG]`
pub fn format_header(path: &str, tag: &str) -> String {
    format!("[{path}#{tag}]")
}

/// Normalizes a model-supplied tag (`#ab12`, ` AB12 `) to uppercase hex; `None` if malformed.
pub fn normalize_tag(raw: &str) -> Option<String> {
    let t = raw.trim().trim_start_matches('#').trim();
    let t = t.strip_prefix('[').and_then(|s| s.strip_suffix(']')).unwrap_or(t);
    let t = t.rsplit_once('#').map_or(t, |(_, tag)| tag);
    (t.len() == 4 && t.chars().all(|c| c.is_ascii_hexdigit())).then(|| t.to_ascii_uppercase())
}

static SEEN_ROW_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[ *]?(\d+):").expect("valid regex"));

/// Line numbers of `N:` / `*N:` / ` N:` rows in a hashline-formatted body.
pub fn seen_lines_from_body(body: &str) -> Vec<u32> {
    body.split('\n')
        .filter_map(|row| SEEN_ROW_RE.captures(row).and_then(|c| c[1].parse().ok()))
        .collect()
}

/// Compact set of 1-based line numbers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineSet {
    bits: Vec<u64>,
}

impl LineSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, line: u32) {
        let (w, b) = (line as usize / 64, line as usize % 64);
        if self.bits.len() <= w {
            self.bits.resize(w + 1, 0);
        }
        self.bits[w] |= 1 << b;
    }

    pub fn contains(&self, line: u32) -> bool {
        let (w, b) = (line as usize / 64, line as usize % 64);
        self.bits.get(w).is_some_and(|x| x & (1 << b) != 0)
    }

    pub fn union_with(&mut self, other: &LineSet) {
        if self.bits.len() < other.bits.len() {
            self.bits.resize(other.bits.len(), 0);
        }
        for (a, b) in self.bits.iter_mut().zip(&other.bits) {
            *a |= *b;
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        self.bits.iter().enumerate().flat_map(|(w, &word)| {
            (0..64).filter(move |b| word & (1u64 << b) != 0).map(move |b| (w * 64 + b) as u32)
        })
    }

    pub fn is_empty(&self) -> bool {
        self.bits.iter().all(|w| *w == 0)
    }

    fn bytes(&self) -> usize {
        self.bits.len() * 8
    }
}

impl FromIterator<u32> for LineSet {
    fn from_iter<I: IntoIterator<Item = u32>>(iter: I) -> Self {
        let mut s = LineSet::new();
        for l in iter {
            s.insert(l);
        }
        s
    }
}

/// Which lines of a snapshot the model has seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seen {
    /// Unrestricted provenance (the model wrote the content, e.g. `write`).
    All,
    Lines(LineSet),
}

impl Seen {
    pub fn lines<I: IntoIterator<Item = u32>>(iter: I) -> Seen {
        Seen::Lines(iter.into_iter().collect())
    }

    pub fn contains(&self, line: u32) -> bool {
        match self {
            Seen::All => true,
            Seen::Lines(s) => s.contains(line),
        }
    }

    fn merge(&mut self, other: &Seen) {
        match (&mut *self, other) {
            (Seen::All, _) => {}
            (_, Seen::All) => *self = Seen::All,
            (Seen::Lines(a), Seen::Lines(b)) => a.union_with(b),
        }
    }

    fn bytes(&self) -> usize {
        match self {
            Seen::All => 0,
            Seen::Lines(s) => s.bytes(),
        }
    }
}

/// One recorded version of a file.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub tag: String,
    /// Full BOM-stripped, LF-normalized text.
    pub text: Arc<str>,
    pub seen: Seen,
}

impl Snapshot {
    fn bytes(&self) -> usize {
        self.text.len() + self.seen.bytes() + 64
    }
}

struct PathEntry {
    /// Newest first.
    versions: Vec<Snapshot>,
    used: u64,
}

#[derive(Default)]
struct Inner {
    paths: HashMap<PathBuf, PathEntry>,
    total: usize,
    tick: u64,
}

impl Inner {
    fn evict(&mut self, keep: &Path) {
        while self.total > MAX_TOTAL_BYTES || self.paths.len() > MAX_PATHS {
            // Least recently used path, preferring others over the one just touched.
            let victim = self
                .paths
                .iter()
                .filter(|(p, _)| p.as_path() != keep)
                .min_by_key(|(_, e)| e.used)
                .map(|(p, _)| p.clone());
            let Some(victim) = victim else {
                // Only `keep` remains: trim its older versions.
                let Some(e) = self.paths.get_mut(keep) else { return };
                if e.versions.len() <= 1 {
                    return;
                }
                if let Some(s) = e.versions.pop() {
                    self.total -= s.bytes();
                }
                continue;
            };
            if let Some(e) = self.paths.remove(&victim) {
                self.total -= e.versions.iter().map(Snapshot::bytes).sum::<usize>();
            }
        }
    }
}

/// Thread-safe, per-session snapshot store keyed by canonical path.
#[derive(Default)]
pub struct HashStore {
    inner: Mutex<Inner>,
}

impl std::fmt::Debug for HashStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HashStore").field("bytes", &self.total_bytes()).finish()
    }
}

fn key(path: &Path) -> PathBuf {
    crate::util::store_key(path)
}

impl HashStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Records `text` (LF-normalized) for `path`, merging `seen` into an existing snapshot
    /// with the same tag. Returns the tag. Texts over [`MAX_SNAPSHOT_FILE_BYTES`] are not
    /// stored (the tag is still returned).
    pub fn record(&self, path: &Path, text: &str, seen: Seen) -> String {
        let tag = file_hash(text);
        if text.len() > MAX_SNAPSHOT_FILE_BYTES {
            return tag;
        }
        let k = key(path);
        let mut g = self.lock();
        g.tick += 1;
        let tick = g.tick;
        let inner = &mut *g;
        let entry = inner.paths.entry(k.clone()).or_insert_with(|| PathEntry { versions: Vec::new(), used: tick });
        entry.used = tick;
        let snap = match entry.versions.iter().position(|s| s.tag == tag) {
            Some(i) => {
                let old = entry.versions.remove(i);
                inner.total -= old.bytes();
                if &*old.text == text {
                    let mut s = old;
                    s.seen.merge(&seen);
                    s
                } else {
                    Snapshot { tag: tag.clone(), text: Arc::from(text), seen }
                }
            }
            None => Snapshot { tag: tag.clone(), text: Arc::from(text), seen },
        };
        inner.total += snap.bytes();
        entry.versions.insert(0, snap);
        while entry.versions.len() > MAX_VERSIONS_PER_PATH {
            if let Some(s) = entry.versions.pop() {
                inner.total -= s.bytes();
            }
        }
        g.evict(&k);
        tag
    }

    /// Marks additional lines of snapshot `tag` as seen.
    pub fn mark_seen<I: IntoIterator<Item = u32>>(&self, path: &Path, tag: &str, lines: I) {
        let k = key(path);
        let mut g = self.lock();
        let inner = &mut *g;
        if let Some(e) = inner.paths.get_mut(&k)
            && let Some(s) = e.versions.iter_mut().find(|s| s.tag.eq_ignore_ascii_case(tag)) {
                let before = s.bytes();
                s.seen.merge(&Seen::lines(lines));
                inner.total = inner.total - before + s.bytes();
            }
    }

    /// Snapshot of `path` with `tag` (case-insensitive).
    pub fn get(&self, path: &Path, tag: &str) -> Option<Snapshot> {
        let k = key(path);
        let mut g = self.lock();
        g.tick += 1;
        let tick = g.tick;
        let e = g.paths.get_mut(&k)?;
        e.used = tick;
        e.versions.iter().find(|s| s.tag.eq_ignore_ascii_case(tag)).cloned()
    }

    /// Most recently recorded snapshot of `path`.
    pub fn latest(&self, path: &Path) -> Option<Snapshot> {
        let k = key(path);
        self.lock().paths.get(&k).and_then(|e| e.versions.first().cloned())
    }

    /// Paths (canonical) that have a snapshot with `tag`.
    pub fn paths_for_tag(&self, tag: &str) -> Vec<PathBuf> {
        let g = self.lock();
        let mut v: Vec<PathBuf> = g
            .paths
            .iter()
            .filter(|(_, e)| e.versions.iter().any(|s| s.tag.eq_ignore_ascii_case(tag)))
            .map(|(p, _)| p.clone())
            .collect();
        v.sort();
        v
    }

    /// Drops all snapshots for `path` and anything below it (directory deletes).
    pub fn forget(&self, path: &Path) {
        let k = key(path);
        let mut g = self.lock();
        let victims: Vec<PathBuf> = g.paths.keys().filter(|p| p.starts_with(&k) || p.starts_with(path)).cloned().collect();
        for v in victims {
            if let Some(e) = g.paths.remove(&v) {
                g.total -= e.versions.iter().map(Snapshot::bytes).sum::<usize>();
            }
        }
    }

    /// Estimated retained bytes.
    pub fn total_bytes(&self) -> usize {
        self.lock().total
    }

    /// Number of paths with snapshots.
    pub fn path_count(&self) -> usize {
        self.lock().paths.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(text: &str) -> String {
        let mut normalized = String::with_capacity(text.len());
        for segment in text.split_inclusive('\n') {
            let (line, nl) = segment.strip_suffix('\n').map_or((segment, ""), |l| (l, "\n"));
            normalized.push_str(line.trim_end_matches([' ', '\t', '\r']));
            normalized.push_str(nl);
        }
        format!("{:04X}", xxhash_rust::xxh32::xxh32(normalized.as_bytes(), 0) & 0xffff)
    }

    #[test]
    fn hash_vectors() {
        assert_eq!(file_hash(""), "5D05");
        assert_eq!(file_hash("hello\n"), "5BF9");
        assert_eq!(file_hash("a \n b\t\r\nc"), "80BA");
        for t in ["", "x", "a  \n\n  b \t", "trailing \n", "\r\n\r\n", "über  \nzz\t"] {
            assert_eq!(file_hash(t), reference(t), "{t:?}");
        }
    }

    #[test]
    fn tag_normalization() {
        assert_eq!(normalize_tag("#ab12").as_deref(), Some("AB12"));
        assert_eq!(normalize_tag("[a.rs#00fF]").as_deref(), Some("00FF"));
        assert_eq!(normalize_tag("XYZ1"), None);
        assert_eq!(normalize_tag("12345"), None);
    }

    #[test]
    fn seen_rows() {
        assert_eq!(seen_lines_from_body("[a#1234]\n1:x\n*12:y\n 13:z\n...\nfoo"), vec![1, 12, 13]);
    }

    #[test]
    fn lineset() {
        let mut s: LineSet = [1, 64, 200].into_iter().collect();
        assert!(s.contains(64) && !s.contains(2));
        s.union_with(&[2].into_iter().collect());
        assert_eq!(s.iter().collect::<Vec<_>>(), vec![1, 2, 64, 200]);
    }

    #[test]
    fn store_versions_and_merge() {
        let dir = crate::util::test_support::temp_dir("store");
        let p = dir.join("f.txt");
        let store = HashStore::new();
        let t1 = store.record(&p, "a\nb\n", Seen::lines([1]));
        store.record(&p, "a\nb\n", Seen::lines([2]));
        let s = store.get(&p, &t1.to_lowercase()).unwrap();
        assert!(s.seen.contains(1) && s.seen.contains(2) && !s.seen.contains(3));
        for i in 0..6 {
            store.record(&p, &format!("v{i}\n"), Seen::All);
        }
        assert!(store.get(&p, &t1).is_none(), "old versions evicted");
        assert_eq!(&*store.latest(&p).unwrap().text, "v5\n");
        assert_eq!(store.paths_for_tag(&file_hash("v5\n")).len(), 1);
        store.forget(&dir);
        assert_eq!(store.path_count(), 0);
        assert_eq!(store.total_bytes(), 0);
    }

    #[test]
    fn store_skips_huge() {
        let store = HashStore::new();
        let big = "x".repeat(MAX_SNAPSHOT_FILE_BYTES + 1);
        let tag = store.record(Path::new("/nonexistent/big"), &big, Seen::All);
        assert_eq!(tag.len(), 4);
        assert_eq!(store.path_count(), 0);
    }

    #[test]
    fn store_memory_cap() {
        let store = HashStore::new();
        let chunk = "y".repeat(3 * 1024 * 1024);
        for i in 0..40 {
            store.record(Path::new(&format!("/nonexistent/f{i}")), &format!("{chunk}{i}"), Seen::All);
        }
        assert!(store.total_bytes() <= MAX_TOTAL_BYTES);
        assert!(store.latest(Path::new("/nonexistent/f39")).is_some());
    }
}
