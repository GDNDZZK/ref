//! Client-side store of sessions (attached or watched), built from SessionSnapshot + incremental updates.

use std::collections::HashMap;

use ref_proto::*;

#[derive(Debug, Clone)]
pub struct MsgEntry {
    pub msg: Message,
    /// Bumped on every change of this message (used as render-cache key).
    pub rev: u64,
}

#[derive(Debug, Clone)]
pub struct SessionView {
    pub info: SessionInfo,
    pub messages: Vec<MsgEntry>,
    pub permissions: Vec<PermissionRequest>,
    /// Live tool output (ToolOutputDelta) per part id, until the final PartUpsert arrives.
    pub live_output: HashMap<PartId, String>,
    /// Global revision of this session view (any change).
    pub rev: u64,
}

impl SessionView {
    pub fn new(info: SessionInfo, messages: Vec<Message>, permissions: Vec<PermissionRequest>) -> Self {
        SessionView {
            info,
            messages: messages.into_iter().map(|msg| MsgEntry { msg, rev: 1 }).collect(),
            permissions,
            live_output: HashMap::new(),
            rev: 1,
        }
    }

    fn idx(&self, id: &str) -> Option<usize> {
        // Updates nearly always target the newest messages: search from the end.
        self.messages.iter().rposition(|m| m.msg.id == id)
    }

    pub fn message(&self, id: &str) -> Option<&Message> {
        self.idx(id).map(|i| &self.messages[i].msg)
    }

    fn touch(&mut self, i: usize) {
        self.rev += 1;
        self.messages[i].rev = self.rev;
    }

    pub fn upsert_message(&mut self, m: Message) {
        match self.idx(&m.id) {
            Some(i) => {
                self.messages[i].msg = m;
                self.touch(i);
            }
            None => {
                // Keep time order (normally appended at the end).
                let pos = self.messages.iter().rposition(|e| e.msg.time <= m.time).map(|p| p + 1).unwrap_or(0);
                self.rev += 1;
                self.messages.insert(pos, MsgEntry { msg: m, rev: self.rev });
            }
        }
    }

    pub fn upsert_part(&mut self, message_id: &str, part: Part) {
        let Some(i) = self.idx(message_id) else { return };
        if let Part::Tool(t) = &part {
            if t.state.is_final() {
                self.live_output.remove(&t.id);
            }
        }
        let parts = &mut self.messages[i].msg.parts;
        match parts.iter().position(|p| p.id() == part.id()) {
            Some(j) => parts[j] = part,
            None => parts.push(part),
        }
        self.touch(i);
    }

    pub fn part_delta(&mut self, message_id: &str, part_id: &str, delta: &str) {
        let Some(i) = self.idx(message_id) else { return };
        let Some(p) = self.messages[i].msg.parts.iter_mut().find(|p| p.id() == part_id) else { return };
        match p {
            Part::Text(t) => t.text.push_str(delta),
            Part::Reasoning(r) => r.text.push_str(delta),
            Part::Tool(t) => t.input_raw.push_str(delta),
            Part::Image(_) => {}
        }
        self.touch(i);
    }

    pub fn tool_output_delta(&mut self, message_id: &str, part_id: &str, delta: &str) {
        let buf = self.live_output.entry(part_id.to_string()).or_default();
        buf.push_str(delta);
        // Bound memory: keep the last 256 KiB.
        if buf.len() > 512 * 1024 {
            let mut cut = buf.len() - 256 * 1024;
            while !buf.is_char_boundary(cut) {
                cut += 1;
            }
            buf.drain(..cut);
        }
        if let Some(i) = self.idx(message_id) {
            self.touch(i);
        }
    }

    pub fn remove_message(&mut self, id: &str) {
        if let Some(i) = self.idx(id) {
            self.messages.remove(i);
            self.rev += 1;
        }
    }

    pub fn busy(&self) -> bool {
        !matches!(self.info.status, SessionStatus::Idle)
    }

    pub fn last_assistant(&self) -> Option<&Message> {
        self.messages.iter().rev().map(|e| &e.msg).find(|m| m.role == Role::Assistant && !m.raw_text().is_empty())
    }

    /// Whole transcript as markdown.
    pub fn transcript_markdown(&self) -> String {
        let mut out = format!("# {}\n\n", self.info.name);
        for e in &self.messages {
            let m = &e.msg;
            let who = match (&m.role, &m.origin) {
                (Role::Assistant, _) => format!("Assistant ({})", m.model.clone().unwrap_or_default()),
                (Role::User, MessageOrigin::User) => "User".to_string(),
                (Role::User, MessageOrigin::Peer { from_name, .. }) => format!("Message from \"{from_name}\""),
                (Role::User, MessageOrigin::Spawn { from_name, .. }) => format!("Spawned by \"{from_name}\""),
                (Role::User, MessageOrigin::Reminder) => "Reminder".to_string(),
                (Role::User, MessageOrigin::Compaction) => "Context compacted (summary)".to_string(),
            };
            out.push_str(&format!("## {} — {}\n\n", who, crate::util::fmt_time(m.time)));
            for p in &m.parts {
                match p {
                    Part::Text(t) => {
                        out.push_str(&t.text);
                        out.push_str("\n\n");
                    }
                    Part::Reasoning(r) if !r.text.trim().is_empty() => {
                        for l in r.text.lines() {
                            out.push_str("> ");
                            out.push_str(l);
                            out.push('\n');
                        }
                        out.push('\n');
                    }
                    Part::Tool(t) => {
                        out.push_str(&format!("**Tool `{}`** — {}\n\n", t.name, t.purpose));
                        let input = serde_json::to_string_pretty(&t.input).unwrap_or_default();
                        out.push_str(&format!("```json\n{input}\n```\n\n"));
                        if !t.output.is_empty() {
                            let fence = if t.output.contains("```") { "~~~~" } else { "```" };
                            out.push_str(&format!("{fence}\n{}\n{fence}\n\n", t.output.trim_end()));
                        }
                    }
                    Part::Image(i) => out.push_str(&format!("[image {}]\n\n", i.name.clone().unwrap_or_else(|| i.mime.clone()))),
                    _ => {}
                }
            }
        }
        out
    }
}

/// What the app should react to after applying a server message.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    None,
    /// A session's data changed (re-render).
    Changed(SessionId),
    SessionListChanged,
    CatalogChanged,
    Closed(SessionId),
    Undone(SessionId, String),
    PermissionAsk(SessionId),
}

#[derive(Default)]
pub struct Store {
    pub sessions: HashMap<SessionId, SessionView>,
    /// From SessionList pushes: all sessions (open + history).
    pub list: Vec<SessionInfo>,
    pub catalog: Option<Catalog>,
}

impl Store {
    pub fn get(&self, id: &str) -> Option<&SessionView> {
        self.sessions.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut SessionView> {
        self.sessions.get_mut(id)
    }

    pub fn info(&self, id: &str) -> Option<&SessionInfo> {
        self.sessions.get(id).map(|s| &s.info).or_else(|| self.list.iter().find(|s| s.id == id))
    }

    pub fn apply(&mut self, msg: ServerMsg) -> Effect {
        match msg {
            ServerMsg::Welcome { catalog, .. } | ServerMsg::Catalog { catalog } => {
                self.catalog = Some(catalog);
                Effect::CatalogChanged
            }
            ServerMsg::SessionList { sessions, .. } => {
                for s in &sessions {
                    if let Some(v) = self.sessions.get_mut(&s.id) {
                        if v.info.updated <= s.updated {
                            v.info = s.clone();
                        }
                    }
                }
                self.list = sessions;
                Effect::SessionListChanged
            }
            ServerMsg::SessionSnapshot { info, messages, permissions, .. } => {
                let id = info.id.clone();
                let prev_rev = self.sessions.get(&id).map(|s| s.rev).unwrap_or(0);
                let mut v = SessionView::new(info, messages, permissions);
                // Keep revisions monotonic so render caches never collide.
                v.rev = prev_rev + 1;
                for m in v.messages.iter_mut() {
                    m.rev = prev_rev + 1;
                }
                self.update_list_entry(&v.info);
                self.sessions.insert(id.clone(), v);
                Effect::Changed(id)
            }
            ServerMsg::SessionUpdated { info } => {
                let id = info.id.clone();
                self.update_list_entry(&info);
                if let Some(v) = self.sessions.get_mut(&id) {
                    v.info = info;
                    v.rev += 1;
                    return Effect::Changed(id);
                }
                Effect::SessionListChanged
            }
            ServerMsg::SessionClosed { session_id } => {
                if let Some(e) = self.list.iter_mut().find(|s| s.id == session_id) {
                    e.open = false;
                    e.attached = 0;
                }
                if let Some(v) = self.sessions.get_mut(&session_id) {
                    v.info.open = false;
                    v.info.status = SessionStatus::Idle;
                    v.permissions.clear();
                }
                Effect::Closed(session_id)
            }
            ServerMsg::MessageUpsert { session_id, message } => self.with(&session_id, |v| v.upsert_message(message)),
            ServerMsg::PartUpsert { session_id, message_id, part } => {
                self.with(&session_id, |v| v.upsert_part(&message_id, part))
            }
            ServerMsg::PartDelta { session_id, message_id, part_id, delta } => {
                self.with(&session_id, |v| v.part_delta(&message_id, &part_id, &delta))
            }
            ServerMsg::ToolOutputDelta { session_id, message_id, part_id, delta } => {
                self.with(&session_id, |v| v.tool_output_delta(&message_id, &part_id, &delta))
            }
            ServerMsg::MessageRemoved { session_id, message_id } => {
                self.with(&session_id, |v| v.remove_message(&message_id))
            }
            ServerMsg::PermissionAsk { request } => {
                let sid = request.session_id.clone();
                let Some(v) = self.sessions.get_mut(&sid) else { return Effect::None };
                if !v.permissions.iter().any(|p| p.call_id == request.call_id) {
                    v.permissions.push(request);
                }
                v.rev += 1;
                Effect::PermissionAsk(sid)
            }
            ServerMsg::PermissionResolved { session_id, call_id } => {
                self.with(&session_id, |v| v.permissions.retain(|p| p.call_id != call_id))
            }
            ServerMsg::Undone { session_id, text } => Effect::Undone(session_id, text),
            ServerMsg::Notice { .. } | ServerMsg::Error { .. } | ServerMsg::Pong { .. } => Effect::None,
        }
    }

    fn update_list_entry(&mut self, info: &SessionInfo) {
        match self.list.iter_mut().find(|s| s.id == info.id) {
            Some(e) => *e = info.clone(),
            None => self.list.push(info.clone()),
        }
    }

    fn with(&mut self, id: &str, f: impl FnOnce(&mut SessionView)) -> Effect {
        match self.sessions.get_mut(id) {
            Some(v) => {
                f(v);
                v.rev += 1;
                Effect::Changed(id.to_string())
            }
            None => Effect::None,
        }
    }

    /// Open background sessions (not attached by anyone), newest first.
    pub fn background(&self) -> Vec<&SessionInfo> {
        let mut v: Vec<&SessionInfo> = self.list.iter().filter(|s| s.open && s.attached == 0).collect();
        v.sort_by_key(|s| s.created);
        v
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use serde_json::json;

    pub fn info(id: &str) -> SessionInfo {
        SessionInfo {
            id: id.into(),
            name: format!("name-{id}"),
            cwd: "/tmp".into(),
            agent: "build".into(),
            model: "anthropic/claude".into(),
            variant: None,
            created: 1,
            updated: 1,
            open: true,
            attached: 1,
            status: SessionStatus::Idle,
            context: ContextInfo { used: 1000, max: 200000, compact_at: 180000 },
            cost: 0.0,
            parent: None,
            must_report: false,
            message_count: 0,
            pending_permissions: 0,
        }
    }

    pub fn msg(id: &str, role: Role, text: &str, time: i64) -> Message {
        let mut m = Message::new(role, MessageOrigin::User);
        m.id = id.into();
        m.time = time;
        if !text.is_empty() {
            m.parts.push(Part::Text(TextPart { id: format!("{id}-t"), text: text.into(), time, meta: None }));
        }
        m
    }

    #[test]
    fn applies_events() {
        let mut st = Store::default();
        st.apply(ServerMsg::SessionSnapshot {
            req: 1,
            info: info("s1"),
            messages: vec![msg("m1", Role::User, "hi", 10)],
            permissions: vec![],
        });
        assert_eq!(st.get("s1").unwrap().messages.len(), 1);
        let e = st.apply(ServerMsg::MessageUpsert { session_id: "s1".into(), message: msg("m2", Role::Assistant, "", 20) });
        assert_eq!(e, Effect::Changed("s1".into()));
        st.apply(ServerMsg::PartUpsert {
            session_id: "s1".into(),
            message_id: "m2".into(),
            part: Part::Text(TextPart { id: "p".into(), text: "Hel".into(), time: 20, meta: None }),
        });
        st.apply(ServerMsg::PartDelta { session_id: "s1".into(), message_id: "m2".into(), part_id: "p".into(), delta: "lo".into() });
        let v = st.get("s1").unwrap();
        assert_eq!(v.messages[1].msg.raw_text(), "Hello");
        let r1 = v.messages[1].rev;

        let tool = ToolPart {
            id: "t".into(),
            call_id: "c1".into(),
            name: "bash".into(),
            purpose: "list".into(),
            input: json!({"command":"ls"}),
            input_raw: String::new(),
            state: ToolState::Running,
            output: String::new(),
            images: vec![],
            time: 21,
            time_start: Some(21),
            time_end: None,
            run_at: None,
            meta: None,
        };
        st.apply(ServerMsg::PartUpsert { session_id: "s1".into(), message_id: "m2".into(), part: Part::Tool(tool.clone()) });
        st.apply(ServerMsg::ToolOutputDelta { session_id: "s1".into(), message_id: "m2".into(), part_id: "t".into(), delta: "a\n".into() });
        let v = st.get("s1").unwrap();
        assert_eq!(v.live_output.get("t").map(|s| s.as_str()), Some("a\n"));
        assert!(v.messages[1].rev > r1);
        let mut done = tool;
        done.state = ToolState::Done;
        done.output = "a\n".into();
        st.apply(ServerMsg::PartUpsert { session_id: "s1".into(), message_id: "m2".into(), part: Part::Tool(done) });
        assert!(st.get("s1").unwrap().live_output.is_empty());

        st.apply(ServerMsg::PermissionAsk {
            request: PermissionRequest {
                session_id: "s1".into(),
                message_id: "m2".into(),
                part_id: "t".into(),
                call_id: "c1".into(),
                tool: "bash".into(),
                purpose: "x".into(),
                input: json!({}),
                time: 0,
            },
        });
        assert_eq!(st.get("s1").unwrap().permissions.len(), 1);
        st.apply(ServerMsg::PermissionResolved { session_id: "s1".into(), call_id: "c1".into() });
        assert!(st.get("s1").unwrap().permissions.is_empty());

        st.apply(ServerMsg::MessageRemoved { session_id: "s1".into(), message_id: "m1".into() });
        assert_eq!(st.get("s1").unwrap().messages.len(), 1);

        let mut i = info("s1");
        i.status = SessionStatus::Busy;
        i.updated = 5;
        st.apply(ServerMsg::SessionUpdated { info: i });
        assert!(st.get("s1").unwrap().busy());
        assert_eq!(st.list.len(), 1);

        assert_eq!(
            st.apply(ServerMsg::Undone { session_id: "s1".into(), text: "t".into() }),
            Effect::Undone("s1".into(), "t".into())
        );
        st.apply(ServerMsg::SessionClosed { session_id: "s1".into() });
        assert!(!st.list[0].open);
        // Events for unknown sessions are ignored.
        assert_eq!(st.apply(ServerMsg::PartDelta { session_id: "zz".into(), message_id: "m".into(), part_id: "p".into(), delta: "x".into() }), Effect::None);
    }

    #[test]
    fn transcript() {
        let mut st = Store::default();
        st.apply(ServerMsg::SessionSnapshot {
            req: 1,
            info: info("s1"),
            messages: vec![msg("m1", Role::User, "question", 10), msg("m2", Role::Assistant, "**answer**", 20)],
            permissions: vec![],
        });
        let md = st.get("s1").unwrap().transcript_markdown();
        assert!(md.contains("question"));
        assert!(md.contains("**answer**"));
    }
}
