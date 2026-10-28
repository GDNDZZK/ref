//! Client-side session state: applies protocol [`Event`]s to messages.
//! Every part carries a revision number so renderers can cache by `(rev, width)`.

use hn_proto::{DeltaField, Event, Message, Part, SessionInfo, ToolState};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct MsgEntry {
    pub msg: Message,
    /// Revision of each part (bumped on every change).
    pub revs: Vec<u64>,
    /// Revision of the message envelope (meta: completed/finish/error...).
    pub rev: u64,
}

#[derive(Debug, Default)]
pub struct SessionState {
    pub info: SessionInfo,
    pub msgs: Vec<MsgEntry>,
    index: HashMap<String, usize>,
    /// Received a snapshot (full state).
    pub loaded: bool,
    /// Backend reported `Event::Closed`.
    pub closed: bool,
    /// Bumped on any change.
    pub rev: u64,
}

/// Global monotonically increasing revision source.
fn next_rev() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static REV: AtomicU64 = AtomicU64::new(1);
    REV.fetch_add(1, Ordering::Relaxed)
}

impl SessionState {
    pub fn new(info: SessionInfo) -> Self {
        SessionState { info, rev: next_rev(), ..Default::default() }
    }

    fn reindex(&mut self) {
        self.index = self.msgs.iter().enumerate().map(|(i, m)| (m.msg.id.clone(), i)).collect();
    }

    pub fn get(&self, id: &str) -> Option<&MsgEntry> {
        self.index.get(id).and_then(|&i| self.msgs.get(i))
    }

    fn entry(msg: Message) -> MsgEntry {
        let revs = msg.parts.iter().map(|_| next_rev()).collect();
        MsgEntry { msg, revs, rev: next_rev() }
    }

    fn upsert(&mut self, msg: Message) {
        if let Some(&i) = self.index.get(&msg.id) {
            self.msgs[i] = Self::entry(msg);
            return;
        }
        // Keep chronological order (normally just an append).
        let pos = self.msgs.iter().rposition(|m| m.msg.time <= msg.time).map(|p| p + 1).unwrap_or(0);
        if pos == self.msgs.len() {
            self.index.insert(msg.id.clone(), pos);
            self.msgs.push(Self::entry(msg));
        } else {
            self.msgs.insert(pos, Self::entry(msg));
            self.reindex();
        }
    }

    /// Apply one event. Returns false if the event referred to unknown state (ignored).
    pub fn apply(&mut self, ev: Event) -> bool {
        self.rev = next_rev();
        match ev {
            Event::Snapshot { info, messages } => {
                self.info = info;
                self.msgs = messages.into_iter().map(Self::entry).collect();
                self.reindex();
                self.loaded = true;
                self.closed = false;
                true
            }
            Event::Info { info } => {
                self.info = info;
                true
            }
            Event::Message { message } => {
                self.upsert(message);
                true
            }
            Event::Part { message, index, part } => {
                let Some(&i) = self.index.get(&message) else { return false };
                let e = &mut self.msgs[i];
                if index < e.msg.parts.len() {
                    e.msg.parts[index] = part;
                    e.revs[index] = next_rev();
                } else {
                    e.msg.parts.push(part);
                    e.revs.push(next_rev());
                }
                e.rev = next_rev();
                true
            }
            Event::Delta { message, index, field, text } => {
                let Some(&i) = self.index.get(&message) else { return false };
                let e = &mut self.msgs[i];
                let Some(part) = e.msg.parts.get_mut(index) else { return false };
                let ok = match (field, part) {
                    (DeltaField::Text, Part::Text { text: t, .. }) => {
                        t.push_str(&text);
                        true
                    }
                    (DeltaField::Reasoning, Part::Reasoning { text: t, .. }) => {
                        t.push_str(&text);
                        true
                    }
                    (DeltaField::ToolInput, Part::Tool(tp)) => {
                        tp.raw_input.push_str(&text);
                        true
                    }
                    (DeltaField::ToolOutput, Part::Tool(tp)) => {
                        tp.output.push_str(&text);
                        true
                    }
                    _ => false,
                };
                if ok {
                    e.revs[index] = next_rev();
                }
                ok
            }
            Event::Removed { messages } => {
                let before = self.msgs.len();
                self.msgs.retain(|m| !messages.contains(&m.msg.id));
                if self.msgs.len() != before {
                    self.reindex();
                }
                true
            }
            Event::Closed => {
                self.closed = true;
                self.info.open = false;
                true
            }
        }
    }

    /// First tool part waiting for permission: (message id, part index).
    pub fn pending_permission(&self) -> Option<(&str, usize, &hn_proto::ToolPart)> {
        for m in self.msgs.iter().rev() {
            for (i, p) in m.msg.parts.iter().enumerate() {
                if let Part::Tool(tp) = p {
                    if tp.state == ToolState::Permission {
                        return Some((&m.msg.id, i, tp));
                    }
                }
            }
        }
        None
    }

    /// A tool currently in its pre-execution delay.
    pub fn delayed_tool(&self) -> Option<&hn_proto::ToolPart> {
        self.msgs.iter().rev().take(4).flat_map(|m| m.msg.parts.iter()).find_map(|p| match p {
            Part::Tool(tp) if tp.state == ToolState::Delayed => Some(tp),
            _ => None,
        })
    }

    /// Raw text of the last assistant message with any text.
    pub fn last_assistant_text(&self) -> Option<String> {
        self.msgs
            .iter()
            .rev()
            .filter(|m| m.msg.role == hn_proto::Role::Assistant)
            .map(|m| m.msg.raw_text())
            .find(|t| !t.trim().is_empty())
    }

    /// Anything streaming / running right now (drives animation).
    pub fn is_animating(&self) -> bool {
        if self.info.status.is_active() {
            return true;
        }
        self.msgs.last().is_some_and(|m| {
            m.msg.completed.is_none() && m.msg.role == hn_proto::Role::Assistant && m.msg.error.is_none()
                && m.msg.finish.is_none()
                && m.msg.parts.iter().any(|p| match p {
                    Part::Tool(t) => !t.state.is_final(),
                    Part::Reasoning { time_end, .. } => time_end.is_none(),
                    _ => false,
                })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hn_proto::{Origin, Role, ToolPart};

    fn msg(id: &str, role: Role, time: i64, parts: Vec<Part>) -> Message {
        Message {
            id: id.into(),
            role,
            origin: Origin::Human,
            time,
            completed: None,
            parts,
            model: None,
            agent: None,
            usage: None,
            finish: None,
            error: None,
            replay_key: None,
        }
    }

    fn text(t: &str) -> Part {
        Part::Text { text: t.into(), time: 0 }
    }

    #[test]
    fn snapshot_then_deltas() {
        let mut s = SessionState::default();
        s.apply(Event::Snapshot {
            info: SessionInfo { id: "s".into(), ..Default::default() },
            messages: vec![msg("u1", Role::User, 1, vec![text("hi")])],
        });
        assert!(s.loaded);
        s.apply(Event::Message { message: msg("a1", Role::Assistant, 2, vec![]) });
        s.apply(Event::Part { message: "a1".into(), index: 0, part: text("") });
        let r0 = s.get("a1").unwrap().revs[0];
        s.apply(Event::Delta { message: "a1".into(), index: 0, field: DeltaField::Text, text: "Hel".into() });
        s.apply(Event::Delta { message: "a1".into(), index: 0, field: DeltaField::Text, text: "lo".into() });
        let e = s.get("a1").unwrap();
        assert_eq!(e.msg.raw_text(), "Hello");
        assert!(e.revs[0] > r0);
    }

    #[test]
    fn tool_deltas_and_replace() {
        let mut s = SessionState::default();
        let tp = ToolPart { call_id: "c".into(), name: "bash".into(), ..Default::default() };
        s.apply(Event::Message { message: msg("a", Role::Assistant, 1, vec![Part::Tool(tp)]) });
        s.apply(Event::Delta { message: "a".into(), index: 0, field: DeltaField::ToolInput, text: "{\"x\":".into() });
        s.apply(Event::Delta { message: "a".into(), index: 0, field: DeltaField::ToolOutput, text: "out".into() });
        match &s.get("a").unwrap().msg.parts[0] {
            Part::Tool(t) => {
                assert_eq!(t.raw_input, "{\"x\":");
                assert_eq!(t.output, "out");
            }
            _ => panic!(),
        }
        // mismatched field is ignored
        assert!(!s.apply(Event::Delta { message: "a".into(), index: 0, field: DeltaField::Text, text: "x".into() }));
        // unknown message ignored
        assert!(!s.apply(Event::Delta { message: "zz".into(), index: 0, field: DeltaField::Text, text: "x".into() }));
        let done = ToolPart { call_id: "c".into(), name: "bash".into(), state: ToolState::Permission, ..Default::default() };
        s.apply(Event::Part { message: "a".into(), index: 0, part: Part::Tool(done) });
        assert!(s.pending_permission().is_some());
    }

    #[test]
    fn upsert_keeps_order_and_removal_reindexes() {
        let mut s = SessionState::default();
        s.apply(Event::Message { message: msg("b", Role::User, 20, vec![]) });
        s.apply(Event::Message { message: msg("a", Role::User, 10, vec![]) });
        s.apply(Event::Message { message: msg("c", Role::User, 30, vec![]) });
        let ids: Vec<_> = s.msgs.iter().map(|m| m.msg.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b", "c"]);
        s.apply(Event::Removed { messages: vec!["b".into()] });
        assert!(s.get("b").is_none());
        assert_eq!(s.get("c").unwrap().msg.id, "c");
        // replacing keeps position
        s.apply(Event::Message { message: msg("a", Role::User, 10, vec![text("new")]) });
        assert_eq!(s.msgs[0].msg.raw_text(), "new");
        s.apply(Event::Closed);
        assert!(s.closed);
    }
}
