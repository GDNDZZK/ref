//! Application state, server message handling and command execution.

use crate::catalog::Catalog;
use crate::client::{Client, ConnState, Incoming};
use crate::dialog::{DKind, Dialog};
use crate::input::Editor;
use crate::render::message::{Doc, ViewState};
use crate::store::SessionState;
use crate::theme;
use hn_proto::{
    AgentInfo, ClientMsg, CommandInfo, Event, ListScope, ModelInfo, ModelRef, PermissionDecision, ServerMsg,
    SessionInfo, ToastLevel,
};
use ratatui::layout::Rect;
use serde_json::Value;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Reply routing tags for requests.
pub enum Tag {
    Created,
    Attached(String),
    Models,
    Agents,
    Commands,
    SessionsDialog,
    OpenDialog,
    /// ListSessions{Open} at connect: same handling as a Sessions push.
    OpenList,
    Continue,
    Export(ExportDest),
    Undo(String),
    /// Toast the error on failure; on success toast the message if non-empty.
    Ack(String),
    /// Fire-and-forget with error toast only.
    Quiet,
}

pub enum ExportDest {
    File(Option<String>),
    Clipboard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Monitor,
}

pub struct Toast {
    pub level: ToastLevel,
    pub msg: String,
    pub until: Instant,
}

/// Clickable UI regions (rebuilt every frame).
#[derive(Debug, Clone, PartialEq)]
pub enum UiHit {
    Messages(String),
    Prompt(Option<String>),
    PermButton(String, String, PermissionDecision),
    Autocomplete(usize),
    NewMessages(String),
    SidebarSession(String),
    Pane(String),
}

/// Requests for the main loop (things that need the terminal).
pub enum Action {
    RunEditor,
    SetMouse(bool),
}

pub struct Drag {
    pub session: String,
    pub rect: Rect,
    pub start: (u16, u16),
    pub moved: bool,
}

#[derive(Default, Clone)]
pub struct StartOpts {
    pub prompt: Option<String>,
    pub session: Option<String>,
    pub cont: bool,
    pub agent: Option<String>,
    pub model: Option<ModelRef>,
}

pub struct App {
    pub client: Client<Tag>,
    pub cwd: String,
    pub url: String,
    pub mode: Mode,
    pub conn: ConnState,
    pub ever_connected: bool,
    pub sessions: HashMap<String, SessionState>,
    pub views: HashMap<String, ViewState>,
    pub docs: HashMap<String, Doc>,
    pub open: Vec<SessionInfo>,
    pub catalog: Catalog,
    pub current: Option<String>,
    pub editor: Editor,
    pub pane_editors: HashMap<String, Editor>,
    pub focus_pane: Option<String>,
    pub zoom: bool,
    pub page: usize,
    pub dialog: Option<Dialog>,
    pub toasts: Vec<Toast>,
    pub leader: Option<Instant>,
    pub esc_armed: Option<(String, Instant)>,
    pub ctrlc_armed: Option<Instant>,
    pub history: Vec<String>,
    pub hist_pos: Option<usize>,
    pub hist_draft: String,
    pub show_thinking: bool,
    pub details: bool,
    pub sidebar: Option<bool>,
    /// Whether the sidebar was visible in the last frame.
    pub sidebar_shown: bool,
    pub mouse_capture: bool,
    pub ac_sel: usize,
    pub ac_hidden: bool,
    pub perm_sel: HashMap<String, usize>,
    pub perm_scroll: HashMap<String, usize>,
    pub hits: Vec<(Rect, UiHit)>,
    pub drag: Option<Drag>,
    pub prompt_width: usize,
    pub quit: bool,
    pub actions: Vec<Action>,
    pub start: StartOpts,
    pub started: bool,
    /// Prompt to send once the session exists.
    pub queued_prompt: Option<String>,
    /// A CreateSession is in flight: accept events for the (still unknown) new session.
    pub creating: bool,
    /// When the in-flight CreateSession was sent (welcome screen shows a wait timer).
    pub creating_since: Option<Instant>,
}

pub const ESC_WINDOW: Duration = Duration::from_millis(1500);
pub const LEADER_WINDOW: Duration = Duration::from_millis(2000);

/// Built-in slash commands: (name, aliases, description, key hint).
pub const BUILTINS: &[(&str, &[&str], &str, &str)] = &[
    ("new", &["clear"], "Start a new session", "ctrl+x n"),
    ("sessions", &["resume", "history"], "Browse and resume past sessions", "ctrl+x l"),
    ("open", &["active"], "Open sessions in the backend (attach to background)", "ctrl+x o"),
    ("models", &["model"], "Switch model for this session", "ctrl+x m"),
    ("agents", &["agent"], "Switch agent", "ctrl+x a"),
    ("compact", &["summarize"], "Summarize and compact the context", "ctrl+x c"),
    ("undo", &[], "Undo the last user message", "ctrl+x u"),
    ("rename", &[], "Rename this session", "ctrl+x r"),
    ("export", &[], "Export transcript (file; `/export copy` = clipboard)", "ctrl+x x"),
    ("copy", &[], "Copy the last assistant message", "ctrl+x y"),
    ("editor", &[], "Compose the prompt in $EDITOR", "ctrl+x e"),
    ("thinking", &[], "Show / hide reasoning", "ctrl+x t"),
    ("details", &[], "Expand / collapse all tool calls", "ctrl+x d"),
    ("close", &[], "Close a background session", "ctrl+x k"),
    ("sidebar", &[], "Toggle the sidebar", "ctrl+x b"),
    ("select", &["mouse"], "Toggle native terminal selection (mouse capture)", "F2"),
    ("help", &[], "Keybinds and commands", "ctrl+x h"),
    ("exit", &["quit", "q"], "Exit", "ctrl+x q"),
];

pub fn builtin(name: &str) -> Option<&'static str> {
    BUILTINS.iter().find(|(n, a, _, _)| *n == name || a.contains(&name)).map(|b| b.0)
}

fn history_path() -> std::path::PathBuf {
    hn_proto::state_dir().join("tui-history.jsonl")
}

fn load_history() -> Vec<String> {
    std::fs::read_to_string(history_path())
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<String>(l).ok())
        .collect()
}

impl App {
    pub fn new(client: Client<Tag>, cwd: String, url: String, mode: Mode, start: StartOpts) -> Self {
        App {
            client,
            cwd,
            url,
            mode,
            conn: ConnState::Connecting,
            ever_connected: false,
            sessions: HashMap::new(),
            views: HashMap::new(),
            docs: HashMap::new(),
            open: Vec::new(),
            catalog: Catalog::default(),
            current: None,
            editor: Editor::new(),
            pane_editors: HashMap::new(),
            focus_pane: None,
            zoom: false,
            page: 0,
            dialog: None,
            toasts: Vec::new(),
            leader: None,
            esc_armed: None,
            ctrlc_armed: None,
            history: load_history(),
            hist_pos: None,
            hist_draft: String::new(),
            show_thinking: true,
            details: false,
            sidebar: None,
            sidebar_shown: false,
            mouse_capture: true,
            ac_sel: 0,
            ac_hidden: false,
            perm_sel: HashMap::new(),
            perm_scroll: HashMap::new(),
            hits: Vec::new(),
            drag: None,
            prompt_width: 60,
            quit: false,
            actions: Vec::new(),
            start,
            started: false,
            queued_prompt: None,
            creating: false,
            creating_since: None,
        }
    }

    // --- helpers ------------------------------------------------------------

    pub fn toast(&mut self, level: ToastLevel, msg: impl Into<String>) {
        let ms = match level {
            ToastLevel::Error => 6000,
            ToastLevel::Warning => 4500,
            _ => 2500,
        };
        let msg = msg.into();
        // Same message, or a newer status of the same kind ("Model: …"), replaces the old toast.
        let kind = msg.split_once(": ").map(|(k, _)| format!("{k}: ")).filter(|k| k.len() <= 12);
        self.toasts.retain(|t| t.msg != msg && !kind.as_ref().is_some_and(|k| t.msg.starts_with(k.as_str())));
        self.toasts.push(Toast { level, msg, until: Instant::now() + Duration::from_millis(ms) });
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
    }

    /// The session keyboard input applies to.
    pub fn active(&self) -> Option<String> {
        match self.mode {
            Mode::Normal => self.current.clone(),
            Mode::Monitor => self.focus_pane.clone(),
        }
    }

    pub fn active_editor(&mut self) -> &mut Editor {
        match (self.mode, self.focus_pane.clone()) {
            (Mode::Monitor, Some(id)) => self.pane_editors.entry(id).or_default(),
            _ => &mut self.editor,
        }
    }

    pub fn session_info(&self, id: &str) -> Option<&SessionInfo> {
        self.sessions.get(id).map(|s| &s.info).or_else(|| self.open.iter().find(|s| s.id == id))
    }

    /// Background sessions shown as panes in monitor mode (stable order: creation time).
    pub fn panes(&self) -> Vec<String> {
        let mut v: Vec<&SessionInfo> = self.open.iter().filter(|s| s.open && !s.is_foreground()).collect();
        v.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        v.into_iter().map(|s| s.id.clone()).collect()
    }

    /// Redraw interval while something animates: ~30fps for live sessions,
    /// spinner rate (80ms) when only small indicators move. None = idle.
    pub fn anim_interval(&self) -> Option<Duration> {
        let visible: Vec<String> = match self.mode {
            Mode::Normal => self.current.iter().cloned().collect(),
            Mode::Monitor => self.panes(),
        };
        if visible.iter().any(|id| self.sessions.get(id).is_some_and(|s| s.is_animating())) {
            return Some(Duration::from_millis(33));
        }
        let spinners = matches!(self.conn, ConnState::Reconnecting { .. } | ConnState::Connecting)
            || (self.creating && self.current.is_none())
            || (self.mode == Mode::Normal && self.sidebar_shown && self.open.iter().any(|s| s.status.is_active()))
            || self.dialog.as_ref().is_some_and(|d| matches!(d.kind, DKind::Sessions | DKind::Open { .. }));
        spinners.then(|| Duration::from_millis(80))
    }

    /// Next wake-up deadline for timers when not animating.
    pub fn next_timer(&self) -> Option<Instant> {
        let mut t: Vec<Instant> = self.toasts.iter().map(|t| t.until).collect();
        if let Some((_, at)) = &self.esc_armed {
            t.push(*at + ESC_WINDOW);
        }
        if let Some(at) = self.ctrlc_armed {
            t.push(at + ESC_WINDOW);
        }
        if let Some(at) = self.leader {
            t.push(at + LEADER_WINDOW);
        }
        t.into_iter().min()
    }

    /// Expire timers. Returns true if something changed.
    pub fn tick(&mut self) -> bool {
        let now = Instant::now();
        let mut changed = false;
        let n = self.toasts.len();
        self.toasts.retain(|t| t.until > now);
        changed |= n != self.toasts.len();
        if self.esc_armed.as_ref().is_some_and(|(_, at)| now >= *at + ESC_WINDOW) {
            self.esc_armed = None;
            changed = true;
        }
        if self.ctrlc_armed.is_some_and(|at| now >= at + ESC_WINDOW) {
            self.ctrlc_armed = None;
            changed = true;
        }
        if self.leader.is_some_and(|at| now >= at + LEADER_WINDOW) {
            self.leader = None;
            changed = true;
        }
        changed
    }

    fn req(&self, msg: ClientMsg, tag: Tag) {
        self.client.request(msg, tag);
    }

    // --- startup & connection -------------------------------------------------

    fn on_connected(&mut self) {
        let first = !self.ever_connected;
        self.ever_connected = true;
        self.req(ClientMsg::ListModels, Tag::Models);
        self.req(ClientMsg::ListAgents, Tag::Agents);
        self.req(ClientMsg::ListSessions { scope: ListScope::Open }, Tag::OpenList);
        match self.mode {
            Mode::Monitor => {
                self.req(ClientMsg::WatchBackground { enabled: true }, Tag::Quiet);
                // On (re)connect, panes are rebuilt from the server's Sessions push:
                // drop stale state so sync_panes re-attaches every pane (views are kept).
                self.sessions.clear();
                self.docs.clear();
            }
            Mode::Normal => {
                if first && !self.started {
                    self.started = true;
                    self.queued_prompt = self.start.prompt.take();
                    if let Some(id) = self.start.session.clone() {
                        self.switch_to(&id);
                    } else if self.start.cont {
                        self.req(ClientMsg::ListSessions { scope: ListScope::History }, Tag::Continue);
                    } else {
                        self.create_session();
                    }
                } else if let Some(id) = self.current.clone() {
                    self.attach(&id, true);
                } else if !self.creating {
                    // The connection died before the initial session was opened (e.g. the
                    // backend was shutting down): redo the startup step on the new connection.
                    if self.start.cont {
                        self.req(ClientMsg::ListSessions { scope: ListScope::History }, Tag::Continue);
                    } else {
                        self.create_session();
                    }
                }
            }
        }
    }

    pub fn create_session(&mut self) {
        let agent = self.start.agent.clone();
        let model = self.start.model.clone();
        self.creating = true;
        self.creating_since = Some(Instant::now());
        self.req(ClientMsg::CreateSession { cwd: self.cwd.clone(), agent, model, name: None }, Tag::Created);
    }

    fn attach(&self, id: &str, foreground: bool) {
        self.req(ClientMsg::Attach { session: id.into(), foreground }, Tag::Attached(id.into()));
    }

    /// Make `id` the foreground session (normal mode).
    pub fn switch_to(&mut self, id: &str) {
        if self.current.as_deref() == Some(id) {
            return;
        }
        if let Some(old) = self.current.take() {
            self.client.send(ClientMsg::Detach { session: old.clone() });
            self.sessions.remove(&old);
            self.docs.remove(&old);
        }
        self.current = Some(id.to_string());
        let info = self.open.iter().find(|s| s.id == id).cloned().unwrap_or_else(|| SessionInfo {
            id: id.into(),
            ..Default::default()
        });
        self.sessions.insert(id.into(), SessionState::new(info));
        self.views.entry(id.into()).or_default().scroll_to_bottom();
        self.attach(id, true);
        self.req(ClientMsg::ListCommands { session: id.into() }, Tag::Commands);
    }

    // --- incoming -----------------------------------------------------------

    pub fn on_incoming(&mut self, m: Incoming<Tag>) {
        match m {
            Incoming::Conn(c) => {
                let was = std::mem::replace(&mut self.conn, c.clone());
                match c {
                    ConnState::Connected => {
                        if self.ever_connected && was != ConnState::Connected {
                            self.toast(ToastLevel::Success, "Reconnected");
                        }
                        self.on_connected();
                    }
                    ConnState::Failed(e) => self.toast(ToastLevel::Error, e),
                    _ => {}
                }
            }
            Incoming::Backend { url, spawned } => {
                self.url = url;
                if spawned {
                    self.toast(ToastLevel::Warning, "Backend was gone · started a new one and resumed");
                }
            }
            Incoming::Server(msg) => self.on_server(msg),
            Incoming::Reply(tag, r) => self.on_reply(tag, r),
        }
    }

    fn on_server(&mut self, msg: ServerMsg) {
        match msg {
            ServerMsg::Event { session, event } => self.on_event(session, event),
            ServerMsg::Sessions { sessions } => {
                self.open = sessions;
                for info in &self.open {
                    if let Some(s) = self.sessions.get_mut(&info.id) {
                        s.info = info.clone();
                    }
                }
                if self.mode == Mode::Monitor {
                    self.sync_panes();
                }
            }
            ServerMsg::Toast { level, message } => self.toast(level, message),
            ServerMsg::Response { .. } => {}
        }
    }

    fn on_event(&mut self, session: String, event: Event) {
        let is_snapshot = matches!(event, Event::Snapshot { .. });
        if !self.sessions.contains_key(&session) {
            let wanted = match self.mode {
                Mode::Normal => self.creating || self.current.as_deref() == Some(session.as_str()),
                Mode::Monitor => self.panes().contains(&session),
            };
            if !(wanted && (is_snapshot || self.creating)) {
                return;
            }
            self.sessions.insert(session.clone(), SessionState::default());
        }
        let closed = matches!(event, Event::Closed);
        let s = self.sessions.get_mut(&session).unwrap();
        s.apply(event);
        if closed {
            match self.mode {
                Mode::Monitor => {
                    self.sessions.remove(&session);
                    self.docs.remove(&session);
                    self.pane_editors.remove(&session);
                    if self.focus_pane.as_deref() == Some(session.as_str()) {
                        self.focus_pane = None;
                    }
                }
                Mode::Normal => {
                    if self.current.as_deref() == Some(session.as_str()) {
                        self.toast(ToastLevel::Warning, "This session was closed on the backend · /new to start another");
                    }
                }
            }
        }
    }

    /// Monitor mode: attach to new background sessions, detach from gone ones.
    pub fn sync_panes(&mut self) {
        let panes = self.panes();
        for id in &panes {
            if !self.sessions.contains_key(id) {
                let info = self.open.iter().find(|s| &s.id == id).cloned().unwrap_or_default();
                self.sessions.insert(id.clone(), SessionState::new(info));
                self.attach(id, false);
            }
        }
        let gone: Vec<String> = self.sessions.keys().filter(|k| !panes.contains(k)).cloned().collect();
        for id in gone {
            self.sessions.remove(&id);
            self.docs.remove(&id);
            self.client.send(ClientMsg::Detach { session: id.clone() });
        }
        if self.focus_pane.as_ref().is_none_or(|f| !panes.contains(f)) {
            self.focus_pane = panes.first().cloned();
            self.zoom = false;
        }
    }

    fn on_reply(&mut self, tag: Tag, r: Result<Value, String>) {
        let r = match r {
            Ok(v) => v,
            Err(e) => {
                if matches!(tag, Tag::Created) {
                    self.creating = false;
                }
                // The session we tried to open doesn't exist (bad --session id, deleted):
                // don't leave the user on an endless "Loading session…".
                if let Tag::Attached(id) = &tag {
                    let unloaded = self.sessions.get(id).is_some_and(|s| !s.loaded);
                    if self.mode == Mode::Normal && self.current.as_deref() == Some(id.as_str()) && unloaded
                        && e != "disconnected" && e != "not connected"
                    {
                        self.toast(ToastLevel::Error, format!("Cannot open session: {e}"));
                        self.sessions.remove(id);
                        self.current = None;
                        self.create_session();
                        return;
                    }
                }
                let what = match &tag {
                    Tag::Created => "Create session failed",
                    Tag::Attached(_) => "Attach failed",
                    Tag::Models => "Listing models failed",
                    Tag::Agents => "Listing agents failed",
                    Tag::Commands => return,
                    Tag::Export(_) => "Export failed",
                    Tag::Undo(_) => "Undo failed",
                    _ => "Request failed",
                };
                if let Some(d) = &mut self.dialog {
                    d.loading = false;
                }
                if e != "disconnected" && e != "not connected" {
                    self.toast(ToastLevel::Error, format!("{what}: {e}"));
                }
                return;
            }
        };
        match tag {
            Tag::Created => {
                self.creating = false;
                let Ok(info) = serde_json::from_value::<SessionInfo>(r) else { return };
                let id = info.id.clone();
                if let Some(old) = self.current.take() {
                    if old != id {
                        self.client.send(ClientMsg::Detach { session: old.clone() });
                        self.sessions.remove(&old);
                        self.docs.remove(&old);
                    }
                }
                // Events (or a snapshot) for the new session may have arrived before this reply.
                match self.sessions.get_mut(&id) {
                    Some(s) => {
                        if !s.loaded {
                            s.info = info;
                            s.loaded = true;
                        }
                    }
                    None => {
                        let mut s = SessionState::new(info);
                        s.loaded = true;
                        self.sessions.insert(id.clone(), s);
                    }
                }
                // Drop anything accepted speculatively while creating.
                self.sessions.retain(|k, _| *k == id);
                self.current = Some(id.clone());
                self.views.insert(id.clone(), ViewState::default());
                self.req(ClientMsg::ListCommands { session: id.clone() }, Tag::Commands);
                if let Some(p) = self.queued_prompt.take() {
                    self.send_prompt(&id, p, Vec::new());
                }
            }
            Tag::Attached(id) => {
                if let Ok(info) = serde_json::from_value::<SessionInfo>(r) {
                    if let Some(s) = self.sessions.get_mut(&id) {
                        if !s.loaded {
                            s.info = info;
                        }
                    }
                }
                if self.mode == Mode::Normal && self.current.as_deref() == Some(id.as_str()) {
                    if let Some(p) = self.queued_prompt.take() {
                        self.send_prompt(&id, p, Vec::new());
                    }
                }
            }
            Tag::Models => {
                if let Ok(v) = serde_json::from_value::<Vec<ModelInfo>>(r) {
                    self.catalog.models = v;
                }
            }
            Tag::Agents => {
                if let Ok(v) = serde_json::from_value::<Vec<AgentInfo>>(r) {
                    self.catalog.agents = v;
                }
            }
            Tag::Commands => {
                if let Ok(v) = serde_json::from_value::<Vec<CommandInfo>>(r) {
                    self.catalog.commands = v;
                }
            }
            Tag::SessionsDialog => {
                if let Ok(v) = serde_json::from_value::<Vec<SessionInfo>>(r) {
                    let items = self.history_items(v);
                    if let Some(d) = self.dialog.as_mut().filter(|d| d.kind == DKind::Sessions) {
                        d.set_items(items);
                    }
                }
            }
            Tag::OpenDialog => {
                if let Ok(v) = serde_json::from_value::<Vec<SessionInfo>>(r) {
                    self.open = v;
                    self.refresh_open_dialog();
                }
            }
            Tag::OpenList => {
                if let Ok(v) = serde_json::from_value::<Vec<SessionInfo>>(r) {
                    self.on_server(ServerMsg::Sessions { sessions: v });
                }
            }
            Tag::Continue => {
                let v = serde_json::from_value::<Vec<SessionInfo>>(r).unwrap_or_default();
                let best = v.into_iter().filter(|s| s.cwd == self.cwd).max_by_key(|s| s.updated_at);
                match best {
                    Some(s) => self.switch_to(&s.id),
                    None => {
                        self.toast(ToastLevel::Info, "No previous session here; started a new one");
                        self.create_session();
                    }
                }
            }
            Tag::Export(dest) => {
                let md = r.as_str().map(str::to_string).unwrap_or_else(|| r.to_string());
                self.finish_export(dest, md);
            }
            Tag::Undo(id) => {
                let text = r.as_str().unwrap_or("").to_string();
                if !text.is_empty() {
                    let ed = match self.mode {
                        Mode::Monitor => self.pane_editors.entry(id).or_default(),
                        Mode::Normal => &mut self.editor,
                    };
                    if ed.is_empty() {
                        ed.set_text(&text);
                    }
                }
                self.toast(ToastLevel::Info, "Undid last message");
            }
            Tag::Ack(msg) => {
                if !msg.is_empty() {
                    self.toast(ToastLevel::Success, msg);
                }
            }
            Tag::Quiet => {}
        }
    }

    fn finish_export(&mut self, dest: ExportDest, md: String) {
        match dest {
            ExportDest::Clipboard => {
                crate::clipboard::copy(&md);
                self.toast(ToastLevel::Success, "Transcript copied to clipboard");
            }
            ExportDest::File(path) => {
                let name = self
                    .active()
                    .and_then(|id| self.session_info(&id).map(|i| i.name.clone()))
                    .unwrap_or_default();
                let slug: String = name
                    .chars()
                    .map(|c| if c.is_alphanumeric() { c } else { '-' })
                    .collect::<String>()
                    .trim_matches('-')
                    .chars()
                    .take(40)
                    .collect();
                let slug = if slug.is_empty() { "session".to_string() } else { slug };
                let p = path.map(std::path::PathBuf::from).unwrap_or_else(|| {
                    std::path::PathBuf::from(&self.cwd)
                        .join(format!("{slug}-{}.md", chrono::Local::now().format("%Y%m%d-%H%M%S")))
                });
                match std::fs::write(&p, md) {
                    Ok(()) => self.toast(ToastLevel::Success, format!("Exported to {}", crate::text::tilde(&p.to_string_lossy()))),
                    Err(e) => self.toast(ToastLevel::Error, format!("Export failed: {e}")),
                }
            }
        }
    }

    // --- actions --------------------------------------------------------------

    pub fn send_prompt(&mut self, id: &str, text: String, images: Vec<hn_proto::ImageData>) {
        if text.trim().is_empty() && images.is_empty() {
            return;
        }
        if !text.trim().is_empty() {
            self.push_history(&text);
        }
        self.views.entry(id.into()).or_default().scroll_to_bottom();
        self.req(ClientMsg::Prompt { session: id.into(), text, images }, Tag::Quiet);
    }

    fn push_history(&mut self, text: &str) {
        if self.history.last().map(|s| s.as_str()) != Some(text) {
            self.history.push(text.to_string());
            if self.history.len() > 500 {
                self.history.drain(..self.history.len() - 500);
            }
            let path = history_path();
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let body: String =
                self.history.iter().filter_map(|h| serde_json::to_string(h).ok()).map(|l| l + "\n").collect();
            let _ = std::fs::write(path, body);
        }
        self.hist_pos = None;
    }

    pub fn submit(&mut self) {
        let Some(id) = self.active() else {
            self.toast(ToastLevel::Warning, "No session yet");
            return;
        };
        let raw = self.active_editor().text().to_string();
        let trimmed = raw.trim_start();
        if let Some(rest) = trimmed.strip_prefix('/') {
            let (name, args) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            if let Some(b) = builtin(name) {
                self.active_editor().clear();
                self.run_builtin(b, args.trim());
                return;
            }
            if self.catalog.commands.iter().any(|c| c.name == name) {
                self.push_history(&raw);
                self.active_editor().clear();
                self.views.entry(id.clone()).or_default().scroll_to_bottom();
                self.req(
                    ClientMsg::Command { session: id, name: name.into(), args: args.trim().into() },
                    Tag::Quiet,
                );
                return;
            }
        }
        let (text, images) = self.active_editor().take_submission();
        self.send_prompt(&id, text, images);
    }

    pub fn abort(&mut self, id: &str) {
        self.req(ClientMsg::Abort { session: id.into() }, Tag::Quiet);
        self.toast(ToastLevel::Info, "Interrupted");
    }

    pub fn permission(&mut self, id: &str, call_id: &str, d: PermissionDecision) {
        self.req(
            ClientMsg::Permission { session: id.into(), call_id: call_id.into(), decision: d },
            Tag::Quiet,
        );
        self.perm_sel.remove(call_id);
        self.perm_scroll.remove(call_id);
    }

    pub fn cycle_agent(&mut self, dir: i64) {
        let Some(id) = self.active() else { return };
        let agents: Vec<&AgentInfo> = self.catalog.agents.iter().collect();
        if agents.is_empty() {
            return;
        }
        let cur = self.session_info(&id).map(|i| i.agent.clone()).unwrap_or_default();
        let pos = agents.iter().position(|a| a.name == cur).unwrap_or(0) as i64;
        let next = agents[((pos + dir).rem_euclid(agents.len() as i64)) as usize].name.clone();
        self.set_agent(&id, &next);
    }

    fn set_agent(&mut self, id: &str, agent: &str) {
        // Optimistic update so the prompt meta reflects the change immediately.
        if let Some(s) = self.sessions.get_mut(id) {
            s.info.agent = agent.to_string();
        }
        self.req(ClientMsg::SetAgent { session: id.into(), agent: agent.into() }, Tag::Quiet);
    }

    pub fn copy_text(&mut self, text: &str, what: &str) {
        if text.is_empty() {
            self.toast(ToastLevel::Warning, format!("Nothing to copy ({what})"));
            return;
        }
        let how = crate::clipboard::copy(text);
        let n = text.chars().count();
        let extra = if how == "osc52" { " (via terminal)" } else { "" };
        self.toast(ToastLevel::Success, format!("Copied {what} · {n} chars{extra}"));
    }

    pub fn open_dialog(&mut self, d: Dialog) {
        self.dialog = Some(d);
    }
}


pub fn status_color(s: &hn_proto::SessionStatus) -> ratatui::style::Color {
    match s {
        hn_proto::SessionStatus::Idle => theme::MUTED,
        hn_proto::SessionStatus::Busy => theme::PRIMARY,
        hn_proto::SessionStatus::Retry { .. } => theme::WARNING,
        hn_proto::SessionStatus::Permission => theme::WARNING,
    }
}

