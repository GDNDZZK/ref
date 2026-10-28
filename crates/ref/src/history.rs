//! Prompt history persisted in `<data dir>/prompt-history.jsonl`.

use std::io::Write;
use std::path::PathBuf;

const MAX: usize = 1000;

#[derive(Default)]
pub struct History {
    pub items: Vec<String>,
    /// Index while browsing (None = editing a fresh prompt).
    pub pos: Option<usize>,
    /// The draft saved when browsing started.
    pub draft: String,
    path: Option<PathBuf>,
}

impl History {
    pub fn load() -> Self {
        let path = crate::util::data_dir().join("prompt-history.jsonl");
        let mut items = Vec::new();
        if let Ok(s) = std::fs::read_to_string(&path) {
            for l in s.lines() {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(l) {
                    if let Some(t) = v.get("text").and_then(|t| t.as_str()) {
                        items.push(t.to_string());
                    }
                }
            }
        }
        if items.len() > MAX {
            items.drain(..items.len() - MAX);
        }
        History { items, pos: None, draft: String::new(), path: Some(path) }
    }

    pub fn push(&mut self, text: &str) {
        self.pos = None;
        if text.trim().is_empty() || self.items.last().map(|l| l == text).unwrap_or(false) {
            return;
        }
        self.items.push(text.to_string());
        if let Some(p) = &self.path {
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
                let _ = writeln!(f, "{}", serde_json::json!({ "text": text }));
            }
            if self.items.len() > MAX * 2 {
                self.items.drain(..self.items.len() - MAX);
                let body: String = self.items.iter().map(|t| format!("{}\n", serde_json::json!({ "text": t }))).collect();
                let _ = std::fs::write(p, body);
            }
        }
    }

    /// Previous entry (older). `current` is the editor text (saved as draft when starting).
    pub fn prev(&mut self, current: &str) -> Option<String> {
        if self.items.is_empty() {
            return None;
        }
        let p = match self.pos {
            None => {
                self.draft = current.to_string();
                self.items.len() - 1
            }
            Some(0) => return None,
            Some(p) => p - 1,
        };
        self.pos = Some(p);
        Some(self.items[p].clone())
    }

    pub fn next(&mut self) -> Option<String> {
        let p = self.pos?;
        if p + 1 >= self.items.len() {
            self.pos = None;
            return Some(std::mem::take(&mut self.draft));
        }
        self.pos = Some(p + 1);
        Some(self.items[p + 1].clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browse() {
        let mut h = History { items: vec!["a".into(), "b".into()], ..Default::default() };
        assert_eq!(h.prev("draft").as_deref(), Some("b"));
        assert_eq!(h.prev("").as_deref(), Some("a"));
        assert_eq!(h.prev(""), None);
        assert_eq!(h.next().as_deref(), Some("b"));
        assert_eq!(h.next().as_deref(), Some("draft"));
        assert_eq!(h.next(), None);
    }
}
