//! App state, server-frame handling and actions.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use ratatui::layout::Rect;
use ratatui::style::Color;
use ref_proto::{
    ImageData, ModelRef, Request, ServerConfigInfo, ServerFrame, SessionEvent, SessionInfo, SessionSnapshot, SessionStatus,
};
use serde_json::Value;

use crate::net::{Client, NetEvent};
use crate::state::SessionView;
use crate::ui::dialog::{Dialog, HelpDialog, Item, Kind, ListDialog};
use crate::ui::editor::{Editor, History};
use crate::ui::permission::PermissionView;
use crate::ui::text;
use crate::ui::theme;
use crate::ui::toast::Toasts;

/// What to do at first connect.
#[derive(Debug, Clone, Default)]
pub struct InitPlan {
    pub session: Option<String>,
    pub continue_last: bool,
    pub watch: bool,
    pub agent: Option<String>,
    pub model: Option<ModelRef>,
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Chat,
    Watch,
}

#[derive(Debug)]
pub enum Pending {
    Config,
    LiveList,
    Created { prompt: Option<(String, Vec<ImageData>)> },
    Attach { sid: String, fg: bool },
    HistoryDialog,
    Continue,
    Undo,
    Expand { sid: Option<String> },
    Files { query: String },
    /// Generic: toast `ok` on success (if non-empty); errors always toast.
    Simple { ok: String },
    Export,
    /// Toast the resulting variant.
    Variant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcKind {
    Slash,
    File,
}

#[derive(Debug, Clone)]
pub struct Autocomplete {
    pub kind: AcKind,
    /// (value inserted, label, description)
    pub items: Vec<(String, String, String)>,
    pub sel: usize,
    /// Byte offset where the replaced token starts.
    pub start: usize,
    pub rect: Rect,
}

#[derive(Default)]
pub struct Watch {
    pub focus: Option<String>,
    pub page: usize,
    pub pages: usize,
    pub inputs: HashMap<String, Editor>,
    /// Tiles at last draw.
    pub tiles: Vec<(String, Rect)>,
    /// WatchAll state last sent to the server.
    pub subscribed: bool,
}

#[derive(Clone)]
pub struct Cmd {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub desc: &'static str,
    pub key: &'static str,
    pub args: bool,
}

pub const COMMANDS: &[Cmd] = &[
    Cmd { name: "new", aliases: &["clear"], desc: "New session", key: "ctrl+x n", args: false },
    Cmd { name: "sessions", aliases: &["resume", "continue"], desc: "Switch session (live + history)", key: "ctrl+x l", args: false },
    Cmd { name: "models", aliases: &[], desc: "Switch model (this session only)", key: "ctrl+x m", args: false },
    Cmd { name: "agents", aliases: &[], desc: "Switch agent", key: "ctrl+x a", args: false },
    Cmd { name: "variant", aliases: &["variants", "effort"], desc: "Set or cycle model variant", key: "ctrl+t", args: true },
    Cmd { name: "compact", aliases: &["summarize"], desc: "Compact the conversation", key: "ctrl+x c", args: false },
    Cmd { name: "undo", aliases: &[], desc: "Undo last prompt", key: "ctrl+x u", args: false },
    Cmd { name: "rename", aliases: &[], desc: "Rename the session", key: "", args: true },
    Cmd { name: "close", aliases: &[], desc: "Close this session and open a new one", key: "", args: false },
    Cmd { name: "export", aliases: &[], desc: "Export transcript as markdown", key: "ctrl+x x", args: true },
    Cmd { name: "copy", aliases: &[], desc: "Copy last assistant message (raw)", key: "ctrl+x y", args: false },
    Cmd { name: "thinking", aliases: &[], desc: "Toggle expanded thinking", key: "ctrl+x t", args: false },
    Cmd { name: "details", aliases: &[], desc: "Toggle expanded tool details", key: "ctrl+x d", args: false },
    Cmd { name: "watch", aliases: &["split"], desc: "Split view of background sessions", key: "ctrl+x w", args: false },
    Cmd { name: "sidebar", aliases: &[], desc: "Toggle sidebar", key: "ctrl+x b", args: false },
    Cmd { name: "editor", aliases: &[], desc: "Compose prompt in $EDITOR", key: "ctrl+x e", args: false },
    Cmd { name: "help", aliases: &[], desc: "Keybindings and commands", key: "ctrl+x h", args: false },
    Cmd { name: "exit", aliases: &["quit", "q"], desc: "Exit", key: "ctrl+x q", args: false },
];

pub fn find_cmd(name: &str) -> Option<&'static Cmd> {
    COMMANDS.iter().find(|c| c.name == name || c.aliases.contains(&name))
}

pub struct App {
    pub client: Client,
    pub cwd: String,
    next_id: u64,
    pub pending: HashMap<u64, Pending>,
    pub config: Option<ServerConfigInfo>,
    pub views: HashMap<String, SessionView>,
    pub current: Option<String>,
    pub live: Vec<SessionInfo>,
    pub history_list: Vec<SessionInfo>,
    pub mode: Mode,
    pub editor: Editor,
    pub history: History,
    pub dialog: Option<Dialog>,
    pub perm: Option<PermissionView>,
    pub toasts: Toasts,
    pub details: bool,
    pub thinking: bool,
    pub start: Instant,
    pub leader: Option<Instant>,
    pub esc_armed: Option<Instant>,
    pub ctrlc_armed: Option<Instant>,
    pub hint: Option<(String, Color, Instant)>,
    pub connected: bool,
    pub ever_connected: bool,
    pub net_status: Option<String>,
    pub quit: bool,
    pub ac: Option<Autocomplete>,
    pub watch: Watch,
    pub sidebar: bool,
    pub init: Option<InitPlan>,
    pub shell_mode: bool,
    pub run_editor: bool,
    pub dirty: bool,
    pub last_click: Option<(Instant, u16, u16, u8)>,
    /// Session id whose transcript holds the active selection.
    pub sel_owner: Option<String>,
    pub files_inflight: bool,
    pub files_want: Option<String>,
    /// Debounced `@` file query: (fire at, query).
    pub files_due: Option<(Instant, String)>,
    /// Sessions we've asked to attach (watch) and are waiting on.
    pub attaching: HashSet<String>,
    pub screen: Rect,
    /// Sidebar live-session rows at last draw: (y, session id).
    pub sidebar_rows: Vec<(u16, String)>,
    pub sidebar_rect: Rect,
    /// Full terminal repaint requested (ctrl+l).
    pub force_clear: bool,
    /// Command waiting for config / session snapshot: (name, args, requested at).
    pub deferred: Option<(String, String, Instant)>,
    /// Autocomplete dismissed with esc until the next edit.
    pub suppress_ac: bool,
}

impl App {
    pub fn new(client: Client, cwd: String, init: InitPlan) -> Self {
        let history = History::load(ref_proto::state_dir().join("prompt-history.jsonl"));
        let mode = if init.watch { Mode::Watch } else { Mode::Chat };
        App {
            client,
            cwd,
            next_id: 1,
            pending: HashMap::new(),
            config: None,
            views: HashMap::new(),
            current: None,
            live: Vec::new(),
            history_list: Vec::new(),
            mode,
            editor: Editor::default(),
            history,
            dialog: None,
            perm: None,
            toasts: Toasts::default(),
            details: false,
            thinking: false,
            start: Instant::now(),
            leader: None,
            esc_armed: None,
            ctrlc_armed: None,
            hint: None,
            connected: false,
            ever_connected: false,
            net_status: None,
            quit: false,
            ac: None,
            watch: Watch::default(),
            sidebar: true,
            init: Some(init),
            shell_mode: false,
            run_editor: false,
            dirty: true,
            last_click: None,
            sel_owner: None,
            files_inflight: false,
            files_want: None,
            files_due: None,
            attaching: HashSet::new(),
            screen: Rect::default(),
            sidebar_rows: Vec::new(),
            sidebar_rect: Rect::default(),
            force_clear: false,
            suppress_ac: false,
            deferred: None,
        }
    }

    pub fn tick(&self) -> u64 {
        (self.start.elapsed().as_millis() / 80) as u64
    }

    pub fn req(&mut self, req: Request, p: Pending) {
        if !self.connected {
            // Requests can't be answered while disconnected; user actions get feedback.
            match p {
                Pending::Files { .. } => self.files_inflight = false,
                _ => self.toasts.warn("Not connected to the backend"),
            }
            return;
        }
        let id = self.next_id;
        self.next_id += 1;
        if self.client.send(Some(id), req) {
            self.pending.insert(id, p);
        }
    }

    /// Fire-and-forget request; errors are toasted.
    pub fn send(&mut self, req: Request) {
        self.req(req, Pending::Simple { ok: String::new() });
    }

    pub fn set_hint(&mut self, s: impl Into<String>, c: Color) {
        self.hint = Some((s.into(), c, Instant::now()));
    }

    pub fn cur(&self) -> Option<&SessionView> {
        self.current.as_ref().and_then(|id| self.views.get(id))
    }

    pub fn cur_mut(&mut self) -> Option<&mut SessionView> {
        let id = self.current.clone()?;
        self.views.get_mut(&id)
    }

    pub fn agents(&self) -> &[ref_proto::AgentInfo] {
        self.config.as_ref().map(|c| c.agents.as_slice()).unwrap_or(&[])
    }

    pub fn agent_color(&self, name: &str) -> Color {
        theme::agent_color(self.agents(), name)
    }

    // -----------------------------------------------------------------------
    // Network events
    // -----------------------------------------------------------------------

    pub fn on_net(&mut self, ev: NetEvent) {
        self.dirty = true;
        match ev {
            NetEvent::Status(s) => self.net_status = Some(s),
            NetEvent::Connected => {
                let reconnect = self.ever_connected;
                self.connected = true;
                self.ever_connected = true;
                self.net_status = None;
                self.watch.subscribed = false;
                self.req(Request::GetConfig, Pending::Config);
                self.req(Request::ListSessions, Pending::LiveList);
                self.update_autocomplete();
                if reconnect {
                    self.toasts.info("Reconnected");
                    // Re-attach everything we had.
                    let ids: Vec<(String, bool)> = self.views.iter().map(|(k, v)| (k.clone(), v.foreground)).collect();
                    for (sid, fg) in ids {
                        self.attaching.insert(sid.clone());
                        self.req(Request::Attach { session_id: sid.clone(), foreground: fg }, Pending::Attach { sid, fg });
                    }
                } else if let Some(plan) = self.init.take() {
                    self.run_init(plan);
                }
            }
            NetEvent::Disconnected(reason) => {
                if self.connected {
                    self.toasts.warn(format!("Disconnected: {reason}"));
                }
                self.connected = false;
                if !self.ever_connected {
                    self.net_status = Some(reason);
                }
                self.pending.clear();
                self.attaching.clear();
                self.files_inflight = false;
            }
            NetEvent::Frame(f) => self.on_frame(f),
        }
    }

    fn run_init(&mut self, plan: InitPlan) {
        if let Some(sid) = plan.session.clone() {
            self.attach_fg(&sid);
            if let Some(p) = plan.prompt {
                self.send_prompt(&sid, p, Vec::new());
            }
        } else if plan.continue_last {
            self.init = Some(InitPlan { continue_last: false, ..plan });
            self.req(Request::ListHistory { cwd: Some(self.cwd.clone()), limit: Some(1) }, Pending::Continue);
        } else if plan.watch {
            self.mode = Mode::Watch;
        } else {
            let prompt = plan.prompt.map(|p| (p, Vec::new()));
            self.req(
                Request::CreateSession { cwd: self.cwd.clone(), agent: plan.agent, model: plan.model, name: None },
                Pending::Created { prompt },
            );
        }
    }

    fn on_frame(&mut self, f: ServerFrame) {
        match f {
            ServerFrame::Response { id, ok, result, error } => {
                let Some(p) = self.pending.remove(&id) else { return };
                if ok {
                    self.on_response(p, result);
                } else {
                    self.on_error(p, error.unwrap_or_else(|| "request failed".into()));
                }
            }
            ServerFrame::Event { session_id, event } => self.on_event(session_id, event),
            ServerFrame::Sessions { sessions } => self.on_sessions(sessions),
            ServerFrame::Toast { level, text } => self.toasts.push(level, text),
        }
    }

    fn on_error(&mut self, p: Pending, err: String) {
        match p {
            Pending::Attach { sid, fg } => {
                self.attaching.remove(&sid);
                if fg {
                    self.toasts.error(format!("Cannot open session: {err}"));
                } else {
                    self.views.remove(&sid);
                }
            }
            Pending::Files { .. } => {
                self.files_inflight = false;
            }
            Pending::Continue => {
                self.toasts.warn(format!("No session to continue: {err}"));
                self.create_session(None);
            }
            Pending::Created { .. } => self.toasts.error(format!("Cannot create session: {err}")),
            _ => self.toasts.error(err),
        }
    }

    fn on_response(&mut self, p: Pending, result: Value) {
        match p {
            Pending::Config => {
                let parsed = result.get("config").map(|c| serde_json::from_value::<ServerConfigInfo>(c.clone()));
                if let Some(Err(e)) = &parsed {
                    self.toasts.error(format!("Malformed config from backend: {e}"));
                }
                if let Some(Ok(c)) = parsed {
                    // TUI passthrough settings.
                    if let Some(t) = c.tui.get("thinking").and_then(Value::as_bool) {
                        self.thinking = t;
                    }
                    if let Some(t) = c.tui.get("details").and_then(Value::as_bool) {
                        self.details = t;
                    }
                    if let Some(t) = c.tui.get("sidebar").and_then(Value::as_bool) {
                        self.sidebar = t;
                    }
                    self.config = Some(c);
                    for v in self.views.values_mut() {
                        v.chat.invalidate();
                    }
                }
            }
            Pending::LiveList => {
                if let Some(s) = sessions_of(&result) {
                    self.on_sessions(s);
                }
            }
            Pending::Created { prompt } => {
                let Some(info) = result.get("session").and_then(|s| serde_json::from_value::<SessionInfo>(s.clone()).ok()) else {
                    self.toasts.error("Malformed create_session response");
                    return;
                };
                let sid = info.id.clone();
                self.views.entry(sid.clone()).or_insert_with(|| SessionView::new(info, true));
                self.attach_fg(&sid);
                if let Some((t, imgs)) = prompt {
                    self.send_prompt(&sid, t, imgs);
                }
            }
            Pending::Attach { sid, fg } => {
                self.attaching.remove(&sid);
                let snap = result.get("snapshot").and_then(|s| serde_json::from_value::<SessionSnapshot>(s.clone()).ok());
                let Some(snap) = snap else {
                    self.toasts.error("Malformed attach response");
                    return;
                };
                let v = self.views.entry(sid.clone()).or_insert_with(|| SessionView::new(snap.info.clone(), fg));
                v.foreground = v.foreground || fg;
                v.load_snapshot(snap);
            }
            Pending::HistoryDialog => {
                if let Some(s) = sessions_of(&result) {
                    self.history_list = s;
                    self.refresh_session_dialog();
                }
            }
            Pending::Continue => {
                let s = sessions_of(&result).unwrap_or_default();
                match s.first() {
                    Some(info) => {
                        let sid = info.id.clone();
                        self.views.entry(sid.clone()).or_insert_with(|| SessionView::new(info.clone(), true));
                        self.attach_fg(&sid);
                        if let Some(p) = self.init.take().and_then(|i| i.prompt) {
                            self.send_prompt(&sid, p, Vec::new());
                        }
                    }
                    None => {
                        let plan = self.init.take().unwrap_or_default();
                        self.toasts.info("No previous session here — started a new one");
                        self.req(
                            Request::CreateSession { cwd: self.cwd.clone(), agent: plan.agent, model: plan.model, name: None },
                            Pending::Created { prompt: plan.prompt.map(|p| (p, Vec::new())) },
                        );
                    }
                }
            }
            Pending::Undo => {
                if let Some(t) = result.get("text").and_then(Value::as_str) {
                    self.editor.set_text(t);
                    self.toasts.info("Undid last prompt");
                }
            }
            Pending::Expand { sid } => {
                let t = result.get("text").and_then(Value::as_str).unwrap_or_default().to_string();
                if t.trim().is_empty() {
                    self.toasts.warn("Command expanded to nothing");
                    return;
                }
                match sid {
                    Some(s) => self.send_prompt(&s, t, Vec::new()),
                    None => self.create_session(Some((t, Vec::new()))),
                }
            }
            Pending::Files { query } => {
                self.files_inflight = false;
                let files: Vec<String> = result
                    .get("files")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                if let Some((start, q)) = self.editor.mention()
                    && q == query
                {
                    let items: Vec<(String, String, String)> = files.into_iter().map(|f| (format!("@{f} "), f, String::new())).collect();
                    if items.is_empty() {
                        self.ac = None;
                    } else {
                        let sel = self.ac.as_ref().filter(|a| a.kind == AcKind::File).map(|a| a.sel).unwrap_or(0);
                        self.ac = Some(Autocomplete { kind: AcKind::File, sel: sel.min(items.len() - 1), items, start, rect: Rect::default() });
                    }
                }
                if let Some(w) = self.files_want.take()
                    && w != query
                {
                    self.request_files(w);
                }
            }
            Pending::Simple { ok } => {
                if let Some(info) = result.get("session").and_then(|s| serde_json::from_value::<SessionInfo>(s.clone()).ok())
                    && let Some(v) = self.views.get_mut(&info.id)
                {
                    v.set_info(info);
                }
                if !ok.is_empty() {
                    self.toasts.info(ok);
                }
            }
            Pending::Variant => {
                if let Some(info) = result.get("session").and_then(|s| serde_json::from_value::<SessionInfo>(s.clone()).ok()) {
                    let v = info.model.variant.clone().unwrap_or_else(|| "default".into());
                    if let Some(view) = self.views.get_mut(&info.id) {
                        view.set_info(info);
                    }
                    self.toasts.info(format!("Variant: {v}"));
                }
            }
            Pending::Export => {
                let p = result.get("path").and_then(Value::as_str).unwrap_or("file");
                self.toasts.info(format!("Exported to {p}"));
            }
        }
    }

    fn on_event(&mut self, sid: String, ev: SessionEvent) {
        let closed = matches!(ev, SessionEvent::Closed);
        let Some(v) = self.views.get_mut(&sid) else { return };
        if !v.loaded && !closed {
            // Events before the snapshot are already reflected in it.
            return;
        }
        if let SessionEvent::Error { text } = &ev
            && (self.mode == Mode::Watch || self.current.as_deref() != Some(sid.as_str()))
        {
            let name = v.info.name.clone();
            self.toasts.error(text::truncate(&format!("{name}: {}", text::one_line(text)), 200));
        }
        v.apply(ev);
        if closed {
            if self.current.as_deref() == Some(sid.as_str()) {
                self.toasts.warn("Session closed");
            } else if !self.views.get(&sid).is_some_and(|v| v.foreground) {
                self.views.remove(&sid);
            }
        }
    }

    fn on_sessions(&mut self, sessions: Vec<SessionInfo>) {
        for s in &sessions {
            if let Some(v) = self.views.get_mut(&s.id) {
                // Keep our view's info fresh (status, name, title, model, context).
                v.set_info(s.clone());
            }
        }
        self.live = sessions;
        self.sync_watch();
        self.refresh_session_dialog();
    }

    /// Background sessions shown in split view.
    pub fn tile_ids(&self) -> Vec<String> {
        let mut v: Vec<&SessionInfo> = self.live.iter().filter(|s| s.live && s.attached == 0 && self.current.as_deref() != Some(s.id.as_str())).collect();
        v.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        v.into_iter().map(|s| s.id.clone()).collect()
    }

    /// In watch mode: watch-attach new background sessions, drop ones that left.
    pub fn sync_watch(&mut self) {
        if !self.connected {
            return;
        }
        let want = self.mode == Mode::Watch;
        if want != self.watch.subscribed {
            self.watch.subscribed = want;
            self.send(Request::WatchAll { enabled: want });
        }
        let tiles: HashSet<String> = if self.mode == Mode::Watch { self.tile_ids().into_iter().collect() } else { HashSet::new() };
        for sid in &tiles {
            if !self.views.contains_key(sid) && !self.attaching.contains(sid) {
                if let Some(info) = self.live.iter().find(|s| &s.id == sid).cloned() {
                    self.views.insert(sid.clone(), SessionView::new(info, false));
                }
                self.attaching.insert(sid.clone());
                self.req(Request::Attach { session_id: sid.clone(), foreground: false }, Pending::Attach { sid: sid.clone(), fg: false });
            }
        }
        let drop: Vec<String> = self.views.iter().filter(|(k, v)| !v.foreground && !tiles.contains(*k)).map(|(k, _)| k.clone()).collect();
        for sid in drop {
            self.views.remove(&sid);
            self.watch.inputs.remove(&sid);
            if self.connected {
                self.client.send(None, Request::Detach { session_id: sid });
            }
        }
        if let Some(f) = &self.watch.focus
            && !tiles.contains(f)
        {
            self.watch.focus = None;
        }
        if self.watch.focus.is_none() {
            self.watch.focus = self.tile_ids().into_iter().find(|t| tiles.contains(t));
        }
    }

    // -----------------------------------------------------------------------
    // Session actions
    // -----------------------------------------------------------------------

    /// Attach `sid` as foreground and make it current (detaching the previous one).
    pub fn attach_fg(&mut self, sid: &str) {
        if let Some(old) = self.current.clone()
            && old != sid
        {
            if let Some(v) = self.views.get_mut(&old) {
                v.foreground = false;
            }
            self.views.remove(&old);
            self.client.send(None, Request::Detach { session_id: old });
        }
        self.current = Some(sid.to_string());
        self.mode = Mode::Chat;
        let info = self
            .views
            .get(sid)
            .map(|v| v.info.clone())
            .or_else(|| self.live.iter().chain(self.history_list.iter()).find(|s| s.id == sid).cloned())
            .unwrap_or_else(|| placeholder_info(sid, &self.cwd));
        let v = self.views.entry(sid.to_string()).or_insert_with(|| SessionView::new(info, true));
        v.foreground = true;
        v.chat.scroll_to_bottom(false);
        self.attaching.insert(sid.to_string());
        self.req(Request::Attach { session_id: sid.to_string(), foreground: true }, Pending::Attach { sid: sid.to_string(), fg: true });
        self.perm = None;
        self.shell_mode = false;
    }

    pub fn create_session(&mut self, prompt: Option<(String, Vec<ImageData>)>) {
        self.req(Request::CreateSession { cwd: self.cwd.clone(), agent: None, model: None, name: None }, Pending::Created { prompt });
    }

    pub fn send_prompt(&mut self, sid: &str, text: String, images: Vec<ImageData>) {
        if let Some(v) = self.views.get_mut(sid) {
            if v.is_busy() {
                v.queued.push(text.clone());
            }
            v.error = None;
            v.chat.scroll_to_bottom(true);
        }
        self.send(Request::Prompt { session_id: sid.to_string(), text, images });
    }

    /// Sends to the current session, creating one if needed.
    pub fn submit_prompt_text(&mut self, text: String, images: Vec<ImageData>) {
        // Typed before the first connection: becomes the startup prompt.
        if !self.ever_connected
            && let Some(plan) = self.init.as_mut()
            && images.is_empty()
        {
            match &mut plan.prompt {
                Some(p) => {
                    p.push_str("\n\n");
                    p.push_str(&text);
                }
                None => plan.prompt = Some(text),
            }
            self.toasts.info("Will send once connected");
            return;
        }
        match self.current.clone() {
            Some(sid) if self.views.get(&sid).is_some_and(|v| !v.closed) => self.send_prompt(&sid, text, images),
            _ => self.create_session(Some((text, images))),
        }
    }

    pub fn abort(&mut self, sid: &str) {
        self.send(Request::Abort { session_id: sid.to_string() });
    }

    pub fn reply_permission(&mut self, sid: &str, call_id: &str, allow: bool, always: bool) {
        self.send(Request::PermissionReply { session_id: sid.to_string(), call_id: call_id.to_string(), allow, always });
        if let Some(v) = self.views.get_mut(sid) {
            v.permissions.retain(|p| p.call_id != call_id);
        }
        self.perm = None;
    }

    /// The permission request to show (current session first, then watched tiles).
    pub fn active_permission(&self) -> Option<(String, ref_proto::PermissionRequest, usize)> {
        let mut order: Vec<&String> = Vec::new();
        if let Some(c) = &self.current
            && self.mode == Mode::Chat
        {
            order.push(c);
        }
        if self.mode == Mode::Watch {
            if let Some(f) = &self.watch.focus {
                order.push(f);
            }
            for (k, _) in &self.watch.tiles {
                order.push(k);
            }
            // The foreground session keeps running while the split view is shown.
            if let Some(c) = &self.current {
                order.push(c);
            }
        }
        let total: usize = order.iter().filter_map(|k| self.views.get(*k)).map(|v| v.permissions.len()).sum();
        for k in order {
            if let Some(v) = self.views.get(k)
                && let Some(p) = v.permissions.first()
            {
                return Some((k.clone(), p.clone(), total.saturating_sub(1)));
            }
        }
        None
    }

    pub fn copy_text(&mut self, t: &str, what: &str) {
        let n = t.chars().count();
        if n == 0 {
            self.toasts.warn("Nothing to copy");
            return;
        }
        crate::clipboard::copy(t);
        self.toasts.info(format!("Copied {what} ({n} chars)"));
    }

    pub fn copy_message(&mut self, sid: &str, msg: &str) {
        let raw = self.views.get(sid).and_then(|v| v.find(msg).map(|i| v.messages[i].raw_text()));
        match raw {
            Some(r) if !r.is_empty() => self.copy_text(&r, "raw message"),
            Some(_) => self.toasts.warn("Message has no text"),
            None => {}
        }
    }

    /// Copies the active selection, if any. Returns true when something was copied.
    pub fn copy_selection(&mut self) -> bool {
        let owner = self.sel_owner.clone();
        let t = owner.as_ref().and_then(|o| self.views.get(o)).and_then(|v| v.chat.selected_text());
        if let Some(t) = t {
            self.copy_text(&t, "selection");
            if let Some(v) = owner.and_then(|o| self.views.get_mut(&o)) {
                v.chat.sel = None;
            }
            self.sel_owner = None;
            return true;
        }
        false
    }

    pub fn has_selection(&self) -> bool {
        self.sel_owner.as_ref().and_then(|o| self.views.get(o)).is_some_and(|v| v.chat.has_selection())
    }

    pub fn clear_selection(&mut self) {
        if let Some(v) = self.sel_owner.take().and_then(|o| self.views.get_mut(&o)) {
            v.chat.sel = None;
        }
    }

    pub fn toggle_details(&mut self) {
        self.details = !self.details;
        self.reset_toggles();
        self.toasts.info(if self.details { "Tool details expanded" } else { "Tool details collapsed" });
    }

    pub fn toggle_thinking(&mut self) {
        self.thinking = !self.thinking;
        self.reset_toggles();
        self.toasts.info(if self.thinking { "Thinking expanded" } else { "Thinking collapsed" });
    }

    fn reset_toggles(&mut self) {
        for v in self.views.values_mut() {
            v.chat.toggled.clear();
        }
    }

    pub fn cycle_agent(&mut self, dir: i32) {
        let Some(sid) = self.current.clone() else { return };
        let agents: Vec<String> = self.agents().iter().map(|a| a.name.clone()).collect();
        if agents.is_empty() {
            return;
        }
        let cur = self.views.get(&sid).map(|v| v.info.agent.clone()).unwrap_or_default();
        let i = agents.iter().position(|a| *a == cur).map(|i| i as i32).unwrap_or(-1);
        let n = agents.len() as i32;
        let next = agents[((i + dir).rem_euclid(n)) as usize].clone();
        if let Some(v) = self.views.get_mut(&sid) {
            v.info.agent = next.clone();
            if let Some(m) = self.config.as_ref().and_then(|c| c.agents.iter().find(|a| a.name == next)).and_then(|a| a.model.clone()) {
                v.info.model = m;
            }
        }
        self.send(Request::SetAgent { session_id: sid, agent: next });
    }

    pub fn cycle_variant(&mut self) {
        if let Some(sid) = self.current.clone() {
            self.req(Request::SetVariant { session_id: sid, variant: None }, Pending::Variant);
        }
    }

    // -----------------------------------------------------------------------
    // Dialogs
    // -----------------------------------------------------------------------

    pub fn open_palette(&mut self) {
        let mut items: Vec<Item> = COMMANDS
            .iter()
            .map(|c| Item { key: c.name.into(), title: c.desc.into(), desc: format!("/{}", c.name), right: c.key.into(), keywords: c.aliases.join(" "), ..Default::default() })
            .collect();
        if let Some(cfg) = &self.config {
            for c in &cfg.commands {
                items.push(Item {
                    key: format!("srv:{}", c.name),
                    title: if c.description.is_empty() { format!("/{}", c.name) } else { c.description.clone() },
                    desc: format!("/{}", c.name),
                    right: c.source.clone(),
                    section: "Custom commands".into(),
                    ..Default::default()
                });
            }
        }
        for it in items.iter_mut().filter(|i| i.section.is_empty()) {
            it.section = "Commands".into();
        }
        self.dialog = Some(Dialog::List(ListDialog::new(Kind::Palette, "Commands", items)));
    }

    pub fn open_sessions(&mut self) {
        let d = ListDialog::new(Kind::Sessions, "Sessions", Vec::new()).with_hint("enter open · ctrl+d close/delete · esc close");
        self.dialog = Some(Dialog::List(d));
        self.refresh_session_dialog();
        if let Some(c) = &self.current
            && let Some(Dialog::List(d)) = self.dialog.as_mut()
        {
            d.select_key(&format!("live:{c}"));
        }
        self.req(Request::ListHistory { cwd: None, limit: Some(200) }, Pending::HistoryDialog);
        self.req(Request::ListSessions, Pending::LiveList);
    }

    fn refresh_session_dialog(&mut self) {
        let Some(Dialog::List(d)) = self.dialog.as_mut() else { return };
        if d.kind != Kind::Sessions {
            return;
        }
        let now = ref_proto::now_ms();
        let models = self.config.as_ref().map(|c| c.models.as_slice()).unwrap_or(&[]);
        let agents = self.config.as_ref().map(|c| c.agents.as_slice()).unwrap_or(&[]);
        let mut items = Vec::new();
        let mut live: Vec<&SessionInfo> = self.live.iter().filter(|s| s.live).collect();
        live.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
        for s in &live {
            let current = self.current.as_deref() == Some(s.id.as_str());
            let model = models.iter().find(|m| m.provider == s.model.provider && m.id == s.model.model).map(|m| m.name.clone()).unwrap_or_else(|| s.model.model.clone());
            let (badge, bc) = if s.attached > 0 { ("fg", theme::SUCCESS) } else { ("bg", theme::SECONDARY) };
            items.push(Item {
                key: format!("live:{}", s.id),
                title: s.name.clone(),
                desc: text::truncate(&text::one_line(&s.title), 40),
                right: format!("{} · {} · {}", s.agent, model, text::tilde(&s.cwd)),
                section: "Live".into(),
                badge: Some((badge.into(), bc)),
                title_color: Some(theme::agent_color(agents, &s.agent)),
                marker: Some((if current { "●".into() } else if s.status.is_idle() { "○".into() } else { "◉".into() }, if current { theme::PRIMARY } else { theme::MUTED })),
                spinner: !s.status.is_idle() && !current,
                keywords: format!("{} {}", s.id, s.cwd),
            });
        }
        let live_ids: HashSet<&str> = live.iter().map(|s| s.id.as_str()).collect();
        for s in self.history_list.iter().filter(|s| !live_ids.contains(s.id.as_str())) {
            let title = if s.title.trim().is_empty() { s.name.clone() } else { text::one_line(&s.title) };
            items.push(Item {
                key: format!("hist:{}", s.id),
                title,
                desc: s.name.clone(),
                right: format!("{} · {}", text::tilde(&s.cwd), text::fmt_ago(s.updated_at, now)),
                section: "History".into(),
                keywords: format!("{} {}", s.id, s.cwd),
                ..Default::default()
            });
        }
        d.set_items(items);
    }

    pub fn open_models(&mut self) {
        let Some(cfg) = &self.config else { return };
        let cur = self.cur().map(|v| v.info.model.clone());
        let items: Vec<Item> = cfg
            .models
            .iter()
            .map(|m| {
                let is_cur = cur.as_ref().is_some_and(|c| c.provider == m.provider && c.model == m.id);
                let mut caps = vec![format!("{} ctx", text::fmt_tokens(m.context))];
                if m.images {
                    caps.push("images".into());
                }
                if m.reasoning {
                    caps.push("reasoning".into());
                }
                Item {
                    key: format!("{}/{}", m.provider, m.id),
                    title: m.name.clone(),
                    desc: caps.join(" · "),
                    right: if m.variants.is_empty() { String::new() } else { m.variants.join(" ") },
                    section: if m.provider_name.is_empty() { m.provider.clone() } else { m.provider_name.clone() },
                    marker: Some((if is_cur { "●".into() } else { " ".into() }, theme::PRIMARY)),
                    keywords: format!("{} {}", m.provider, m.id),
                    ..Default::default()
                }
            })
            .collect();
        let mut d = ListDialog::new(Kind::Models, "Switch model · this session only", items);
        if let Some(c) = cur {
            d.select_key(&c.key());
        }
        self.dialog = Some(Dialog::List(d));
    }

    pub fn open_agents(&mut self) {
        let cur = self.cur().map(|v| v.info.agent.clone()).unwrap_or_default();
        let agents = self.agents().to_vec();
        let items: Vec<Item> = agents
            .iter()
            .map(|a| Item {
                key: a.name.clone(),
                title: a.name.clone(),
                desc: text::one_line(&a.description),
                right: a.model.as_ref().map(|m| m.key()).unwrap_or_default(),
                title_color: Some(theme::agent_color(&agents, &a.name)),
                marker: Some((if a.name == cur { "●".into() } else { " ".into() }, theme::PRIMARY)),
                ..Default::default()
            })
            .collect();
        let mut d = ListDialog::new(Kind::Agents, "Switch agent", items);
        d.select_key(&cur);
        self.dialog = Some(Dialog::List(d));
    }

    pub fn open_variants(&mut self) {
        let Some(v) = self.cur() else { return };
        let m = v.info.model.clone();
        let variants = self
            .config
            .as_ref()
            .and_then(|c| c.models.iter().find(|x| x.provider == m.provider && x.id == m.model))
            .map(|x| x.variants.clone())
            .unwrap_or_default();
        if variants.is_empty() {
            self.toasts.info("This model has no variants");
            return;
        }
        let mut items = vec![Item { key: String::new(), title: "default".into(), marker: Some((if m.variant.is_none() { "●".into() } else { " ".into() }, theme::PRIMARY)), ..Default::default() }];
        for x in variants {
            let is = m.variant.as_deref() == Some(x.as_str());
            items.push(Item { key: x.clone(), title: x, marker: Some((if is { "●".into() } else { " ".into() }, theme::PRIMARY)), ..Default::default() });
        }
        let mut d = ListDialog::new(Kind::Variants, "Model variant", items);
        d.select_key(m.variant.as_deref().unwrap_or(""));
        self.dialog = Some(Dialog::List(d));
    }

    pub fn open_help(&mut self) {
        let mut cmds: Vec<(String, String)> = COMMANDS.iter().map(|c| (c.name.to_string(), c.desc.to_string())).collect();
        if let Some(cfg) = &self.config {
            cmds.extend(cfg.commands.iter().map(|c| (c.name.clone(), c.description.clone())));
        }
        self.dialog = Some(Dialog::Help(HelpDialog::new(&cmds)));
    }

    /// Enter on a list dialog.
    pub fn dialog_submit(&mut self) {
        let Some(Dialog::List(d)) = self.dialog.as_ref() else { return };
        let kind = d.kind.clone();
        let Some(key) = d.selected().map(|i| i.key.clone()) else { return };
        self.dialog = None;
        match kind {
            Kind::Palette => {
                if let Some(name) = key.strip_prefix("srv:") {
                    self.editor.set_text(&format!("/{name} "));
                } else if let Some(c) = find_cmd(&key) {
                    if c.args && c.name == "rename" {
                        self.editor.set_text("/rename ");
                    } else {
                        self.run_command(c.name, "");
                    }
                }
            }
            Kind::Sessions => {
                let sid = key.split_once(':').map(|x| x.1).unwrap_or(&key).to_string();
                if self.current.as_deref() != Some(sid.as_str()) {
                    self.attach_fg(&sid);
                } else {
                    self.mode = Mode::Chat;
                }
            }
            Kind::Models => {
                if let (Some(sid), Some(mut m)) = (self.current.clone(), ModelRef::parse(&key)) {
                    // Keep the current variant when the new model supports it.
                    let cur_var = self.cur().and_then(|v| v.info.model.variant.clone());
                    let supports = self.config.as_ref().and_then(|c| c.models.iter().find(|x| x.provider == m.provider && x.id == m.model)).is_some_and(|x| cur_var.as_ref().is_some_and(|v| x.variants.contains(v)));
                    if supports {
                        m.variant = cur_var;
                    }
                    if let Some(v) = self.views.get_mut(&sid) {
                        v.info.model = m.clone();
                    }
                    let name = self.model_display(&m);
                    self.req(Request::SetModel { session_id: sid, model: m }, Pending::Simple { ok: format!("Model: {name}") });
                }
            }
            Kind::Agents => {
                if let Some(sid) = self.current.clone() {
                    if let Some(v) = self.views.get_mut(&sid) {
                        v.info.agent = key.clone();
                    }
                    self.send(Request::SetAgent { session_id: sid, agent: key });
                }
            }
            Kind::Variants => {
                if let Some(sid) = self.current.clone() {
                    let variant = (!key.is_empty()).then_some(key);
                    self.req(Request::SetVariant { session_id: sid, variant }, Pending::Simple { ok: String::new() });
                }
            }
        }
    }

    /// ctrl+d in the sessions dialog: confirm, then close (live) / delete (history).
    pub fn dialog_delete(&mut self) {
        let Some(Dialog::List(d)) = self.dialog.as_mut() else { return };
        if d.kind != Kind::Sessions {
            return;
        }
        let Some(key) = d.selected().map(|i| i.key.clone()) else { return };
        if d.confirm.as_deref() != Some(key.as_str()) {
            d.confirm = Some(key);
            return;
        }
        d.confirm = None;
        if let Some(sid) = key.strip_prefix("live:") {
            let sid = sid.to_string();
            self.req(Request::Close { session_id: sid.clone() }, Pending::Simple { ok: "Session closed".into() });
            if self.current.as_deref() == Some(sid.as_str()) {
                self.views.remove(&sid);
                self.current = None;
                self.create_session(None);
            }
        } else if let Some(sid) = key.strip_prefix("hist:") {
            let sid = sid.to_string();
            self.history_list.retain(|s| s.id != sid);
            self.req(Request::Delete { session_id: sid }, Pending::Simple { ok: "Session deleted".into() });
            self.refresh_session_dialog();
        }
    }

    pub fn model_display(&self, m: &ModelRef) -> String {
        self.config
            .as_ref()
            .and_then(|c| c.models.iter().find(|x| x.provider == m.provider && x.id == m.model))
            .map(|x| x.name.clone())
            .unwrap_or_else(|| m.model.clone())
    }

    pub fn provider_display(&self, m: &ModelRef) -> String {
        self.config
            .as_ref()
            .and_then(|c| c.models.iter().find(|x| x.provider == m.provider))
            .map(|x| if x.provider_name.is_empty() { x.provider.clone() } else { x.provider_name.clone() })
            .unwrap_or_else(|| m.provider.clone())
    }

    // -----------------------------------------------------------------------
    // Commands
    // -----------------------------------------------------------------------

    pub fn run_command(&mut self, name: &str, args: &str) {
        let args = args.trim();
        // Commands needing backend data run once it has arrived (startup race).
        let needs_config = matches!(name, "models" | "agents" | "variant");
        let needs_session = matches!(name, "variant" | "compact" | "undo" | "rename" | "export" | "copy");
        if (needs_config && self.config.is_none()) || (needs_session && self.cur().is_none_or(|v| !v.loaded)) {
            if self.connected || !self.ever_connected {
                self.deferred = Some((name.to_string(), args.to_string(), Instant::now()));
                return;
            }
            self.toasts.warn("Not connected to the backend");
            return;
        }
        let cur = self.current.clone();
        match name {
            "new" => {
                self.create_session(None);
            }
            "sessions" => self.open_sessions(),
            "models" => self.open_models(),
            "agents" => self.open_agents(),
            "variant" => {
                if args.is_empty() {
                    self.open_variants();
                } else if let Some(sid) = cur {
                    self.req(Request::SetVariant { session_id: sid, variant: Some(args.to_string()) }, Pending::Simple { ok: format!("Variant: {args}") });
                }
            }
            "compact" => {
                if let Some(sid) = cur {
                    self.req(Request::Compact { session_id: sid }, Pending::Simple { ok: String::new() });
                }
            }
            "undo" => {
                if let Some(sid) = cur {
                    self.req(Request::Undo { session_id: sid }, Pending::Undo);
                }
            }
            "rename" => {
                if args.is_empty() {
                    self.editor.set_text("/rename ");
                } else if let Some(sid) = cur {
                    self.req(Request::Rename { session_id: sid, name: args.to_string() }, Pending::Simple { ok: format!("Renamed to \"{args}\"") });
                }
            }
            "close" => {
                if let Some(sid) = cur {
                    self.req(Request::Close { session_id: sid.clone() }, Pending::Simple { ok: "Session closed".into() });
                    self.views.remove(&sid);
                    self.current = None;
                }
                self.create_session(None);
            }
            "export" => {
                if let Some(sid) = cur {
                    let p = if args.is_empty() { format!("ref-session-{sid}.md") } else { args.to_string() };
                    let p = if p.starts_with('/') { p } else { format!("{}/{p}", self.cwd.trim_end_matches('/')) };
                    self.req(Request::Export { session_id: sid, path: p }, Pending::Export);
                }
            }
            "copy" => {
                let raw = self.cur().and_then(|v| v.messages.iter().rev().find(|m| m.role == ref_proto::Role::Assistant && !m.raw_text().is_empty()).map(|m| m.raw_text()));
                match raw {
                    Some(r) => self.copy_text(&r, "last assistant message"),
                    None => self.toasts.warn("No assistant message yet"),
                }
            }
            "thinking" => self.toggle_thinking(),
            "details" => self.toggle_details(),
            "watch" => self.toggle_watch(),
            "sidebar" => self.sidebar = !self.sidebar,
            "editor" => self.run_editor = true,
            "help" => self.open_help(),
            "exit" => self.quit = true,
            _ => {}
        }
    }

    pub fn toggle_watch(&mut self) {
        match self.mode {
            Mode::Chat => {
                self.mode = Mode::Watch;
                self.clear_selection();
                self.sync_watch();
            }
            Mode::Watch => {
                if self.current.is_some() {
                    self.mode = Mode::Chat;
                    self.sync_watch();
                } else {
                    self.toasts.info("No foreground session — use ctrl+o to zoom a tile or /new");
                }
            }
        }
    }

    /// Zoom a tile: attach it as foreground.
    pub fn zoom(&mut self, sid: &str) {
        let sid = sid.to_string();
        if let Some(v) = self.views.get_mut(&sid) {
            v.foreground = true;
        }
        self.attach_fg(&sid);
        self.sync_watch();
    }

    /// Enter in the main editor.
    pub fn submit(&mut self) {
        let raw = self.editor.text.clone();
        let trimmed = raw.trim();
        if trimmed.is_empty() && self.editor.images.is_empty() {
            return;
        }
        if self.shell_mode {
            let cmd = trimmed.to_string();
            self.history.push(&format!("!{cmd}"));
            self.editor.clear();
            self.shell_mode = false;
            if let Some(sid) = self.current.clone() {
                self.req(Request::Shell { session_id: sid, command: cmd }, Pending::Simple { ok: String::new() });
            }
            return;
        }
        if matches!(trimmed, "exit" | "quit" | ":q") {
            self.quit = true;
            return;
        }
        if let Some(rest) = trimmed.strip_prefix('/') {
            let (name, args) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            if let Some(c) = find_cmd(name) {
                self.history.push(trimmed);
                self.editor.clear();
                self.run_command(c.name, args);
                return;
            }
            if self.config.as_ref().is_some_and(|c| c.commands.iter().any(|x| x.name == name)) {
                self.history.push(trimmed);
                let sub = self.editor.submit();
                let args = sub.text.trim().strip_prefix('/').and_then(|r| r.split_once(char::is_whitespace)).map(|x| x.1.to_string()).unwrap_or_default();
                let sid = self.current.clone();
                self.req(Request::ExpandCommand { name: name.to_string(), args, cwd: self.cwd.clone() }, Pending::Expand { sid });
                return;
            }
        }
        self.history.push(&raw);
        let sub = self.editor.submit();
        let images = sub.images.into_iter().map(|(i, _)| i).collect();
        self.submit_prompt_text(sub.text, images);
    }

    // -----------------------------------------------------------------------
    // Autocomplete
    // -----------------------------------------------------------------------

    pub fn update_autocomplete(&mut self) {
        let t = &self.editor.text;
        if self.shell_mode || self.suppress_ac {
            self.ac = None;
            return;
        }
        if let Some(q) = t.strip_prefix('/')
            && !q.contains(char::is_whitespace)
            && self.editor.cursor == t.len()
        {
            let mut items: Vec<(i64, (String, String, String))> = Vec::new();
            for c in COMMANDS {
                // Canonical name wins ties (aliases only shown when they match better).
                let mut best: Option<(i64, &str)> = None;
                for n in std::iter::once(c.name).chain(c.aliases.iter().copied()) {
                    if let Some(s) = text::fuzzy_score(n, q)
                        && best.is_none_or(|b| s > b.0)
                    {
                        best = Some((s, n));
                    }
                }
                if let Some((s, n)) = best {
                    let label = if n == c.name { format!("/{}", c.name) } else { format!("/{n} → /{}", c.name) };
                    items.push((s + 5, (format!("/{} ", c.name), label, c.desc.to_string())));
                }
            }
            if let Some(cfg) = &self.config {
                for c in &cfg.commands {
                    if let Some(s) = text::fuzzy_score(&c.name, q) {
                        items.push((s, (format!("/{} ", c.name), format!("/{}", c.name), c.description.clone())));
                    }
                }
            }
            items.sort_by_key(|x| std::cmp::Reverse(x.0));
            let items: Vec<_> = items.into_iter().map(|x| x.1).take(50).collect();
            if items.is_empty() {
                self.ac = None;
            } else {
                let sel = self.ac.as_ref().filter(|a| a.kind == AcKind::Slash).map(|a| a.sel.min(items.len() - 1)).unwrap_or(0);
                let sel = if q.is_empty() { sel } else { 0 };
                self.ac = Some(Autocomplete { kind: AcKind::Slash, items, sel, start: 0, rect: Rect::default() });
            }
            return;
        }
        if let Some((_, q)) = self.editor.mention() {
            if self.ac.as_ref().is_some_and(|a| a.kind == AcKind::Slash) {
                self.ac = None;
            }
            self.files_due = Some((Instant::now() + Duration::from_millis(80), q));
            return;
        }
        self.files_due = None;
        self.ac = None;
    }

    fn request_files(&mut self, q: String) {
        if self.files_inflight {
            self.files_want = Some(q);
            return;
        }
        self.files_inflight = true;
        self.req(Request::ListFiles { cwd: self.cwd.clone(), query: q.clone(), limit: Some(20) }, Pending::Files { query: q });
    }

    /// Accepts the selected completion. Returns true if a slash command should run now.
    pub fn accept_completion(&mut self, run: bool) -> bool {
        let Some(ac) = self.ac.take() else { return false };
        let Some((value, _, _)) = ac.items.get(ac.sel).cloned() else { return false };
        match ac.kind {
            AcKind::Slash => {
                let name = value.trim().trim_start_matches('/').to_string();
                let takes_args = find_cmd(&name).map(|c| c.args).unwrap_or(true);
                if run && !takes_args {
                    self.editor.clear();
                    self.history.push(&format!("/{name}"));
                    self.run_command(&name, "");
                    return true;
                }
                self.editor.set_text(&value);
            }
            AcKind::File => {
                self.editor.replace_before_cursor(ac.start, &value);
            }
        }
        false
    }

    // -----------------------------------------------------------------------
    // Housekeeping
    // -----------------------------------------------------------------------

    /// Whether anything needs periodic redraws, and how often.
    pub fn frame_interval(&self) -> Option<Duration> {
        let fast = self.views.values().any(|v| v.chat.scrolling() || v.chat.drag_edge != 0)
            || self.toasts.animating()
            || self.dialog.as_ref().is_some_and(|d| d.animating())
            || self.perm.as_ref().is_some_and(|p| p.opened.elapsed() < Duration::from_millis(150));
        if fast {
            return Some(Duration::from_millis(16));
        }
        if self.deferred.is_some() {
            return Some(Duration::from_millis(50));
        }
        if let Some((at, _)) = &self.files_due {
            return Some(at.saturating_duration_since(Instant::now()).max(Duration::from_millis(5)));
        }
        let anim = self.views.values().any(|v| v.animating() || matches!(v.info.status, SessionStatus::Retrying { .. }))
            || self.dialog.as_ref().is_some_and(|_| self.live.iter().any(|s| !s.status.is_idle()))
            || !self.connected;
        if anim {
            return Some(Duration::from_millis(80));
        }
        if let Some(d) = self.toasts.next_deadline() {
            return Some(d.max(Duration::from_millis(16)));
        }
        if self.hint.is_some() || self.leader.is_some() || self.esc_armed.is_some() || self.ctrlc_armed.is_some() {
            return Some(Duration::from_millis(250));
        }
        None
    }

    /// Expires timers (hints, leader, armed keys).
    pub fn housekeeping(&mut self) {
        if let Some((name, args, at)) = self.deferred.take() {
            let ready = self.config.is_some() && self.cur().is_some_and(|v| v.loaded);
            if ready {
                self.run_command(&name, &args);
                self.dirty = true;
            } else if at.elapsed() < Duration::from_secs(10) {
                self.deferred = Some((name, args, at));
            }
        }
        if self.hint.as_ref().is_some_and(|h| h.2.elapsed() > Duration::from_millis(1500)) {
            self.hint = None;
            self.dirty = true;
        }
        if self.leader.is_some_and(|l| l.elapsed() > Duration::from_secs(2)) {
            self.leader = None;
            self.dirty = true;
        }
        if self.esc_armed.is_some_and(|l| l.elapsed() > Duration::from_millis(1500)) {
            self.esc_armed = None;
        }
        if self.ctrlc_armed.is_some_and(|l| l.elapsed() > Duration::from_millis(1500)) {
            self.ctrlc_armed = None;
        }
        if let Some((at, _)) = &self.files_due
            && Instant::now() >= *at
            && let Some((_, q)) = self.files_due.take()
        {
            self.request_files(q);
        }
        self.toasts.gc();
        for v in self.views.values_mut() {
            if v.chat.drag_tick() {
                self.dirty = true;
            }
        }
    }
}

fn sessions_of(v: &Value) -> Option<Vec<SessionInfo>> {
    v.get("sessions").and_then(|s| serde_json::from_value(s.clone()).ok())
}

pub fn placeholder_info(sid: &str, cwd: &str) -> SessionInfo {
    SessionInfo {
        id: sid.to_string(),
        name: String::new(),
        title: String::new(),
        agent: String::new(),
        model: ModelRef::default(),
        cwd: cwd.to_string(),
        created_at: 0,
        updated_at: 0,
        status: SessionStatus::Idle,
        attached: 0,
        live: true,
        parent: None,
        must_report: false,
        context: Default::default(),
    }
}
