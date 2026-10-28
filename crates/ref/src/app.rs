//! Application state and the main event loop.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crossterm::event::{Event, EventStream};
use futures::StreamExt;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ref_proto::*;
use tokio::sync::mpsc;

use crate::conn::{Conn, ConnEvent};
use crate::editor::Editor;
use crate::state::{Effect, Store};
use crate::ui::chat::ChatView;
use crate::ui::dialogs::{ListDialog, Popup};
use crate::ui::theme;
use crate::ui::toast::Toasts;

#[derive(Debug, Clone, Default)]
pub struct StartOpts {
    pub cwd: String,
    pub cont: bool,
    pub session: Option<String>,
    pub watch: bool,
    pub model: Option<String>,
    pub agent: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    Normal,
    Watch,
}

#[derive(Debug, Clone)]
pub enum Req {
    Create,
    Attach,
    StartupList,
    List,
}

/// Events produced by background tasks of the app itself.
pub enum AppEvent {
    ClipboardImage { png: Option<Vec<u8>>, text_fallback: bool },
    ClipboardText(String),
}

#[derive(Default)]
pub struct WatchState {
    pub focus: Option<SessionId>,
    pub zoom: bool,
    pub page: usize,
    /// Pane rects from the last draw (session id, outer rect, chat rect).
    pub panes: Vec<(SessionId, Rect, Rect)>,
    pub more: usize,
    pub pages: usize,
}

#[derive(Debug, Clone)]
pub struct Drag {
    pub session: SessionId,
    pub last: (u16, u16),
    pub moved: bool,
}

/// Screen regions from the last draw, for mouse hit-testing.
#[derive(Default, Clone)]
pub struct Regions {
    pub chat: Rect,
    pub input: Rect,
    pub input_text_x: u16,
    pub input_text_w: u16,
    pub input_scroll: usize,
    pub perm: Rect,
    pub perm_buttons: Vec<(Rect, PermissionDecision)>,
    pub sidebar: Rect,
    pub cursor: Option<ratatui::layout::Position>,
}

pub struct App {
    pub conn: Conn,
    pub store: Store,
    pub opts: StartOpts,
    pub app_tx: mpsc::UnboundedSender<AppEvent>,
    pub connected: bool,
    pub conn_error: Option<String>,
    pub startup_done: bool,
    pub current: Option<SessionId>,
    pub watched: HashSet<SessionId>,
    pub views: HashMap<SessionId, ChatView>,
    pub editor: Editor,
    pub pane_editors: HashMap<SessionId, Editor>,
    pub history: crate::history::History,
    pub mode: Mode,
    pub watch: WatchState,
    pub dialog: Option<ListDialog>,
    pub popup: Option<Popup>,
    pub popup_dismissed: bool,
    pub perm_sel: usize,
    pub perm_scroll: usize,
    /// When the currently shown permission prompt appeared (call id, time) — keys are ignored briefly.
    pub perm_since: Option<(String, Instant)>,
    pub toasts: Toasts,
    pub leader: Option<Instant>,
    pub esc_at: Option<Instant>,
    pub details: bool,
    pub thinking: bool,
    pub sidebar: Option<bool>,
    pub mouse_capture: bool,
    pub tick: u64,
    pub dirty: bool,
    pub quit: bool,
    pub next_req: u64,
    pub reqs: HashMap<u64, Req>,
    pub model_override_used: bool,
    pub drag: Option<Drag>,
    pub regions: Regions,
    pub run_editor: bool,
    pub size: (u16, u16),
    pub highlight: bool,
}

impl App {
    pub fn new(conn: Conn, opts: StartOpts, app_tx: mpsc::UnboundedSender<AppEvent>) -> Self {
        let mode = if opts.watch { Mode::Watch } else { Mode::Normal };
        App {
            conn,
            store: Store::default(),
            opts,
            app_tx,
            connected: false,
            conn_error: None,
            startup_done: false,
            current: None,
            watched: HashSet::new(),
            views: HashMap::new(),
            editor: Editor::new(),
            pane_editors: HashMap::new(),
            history: crate::history::History::load(),
            mode,
            watch: WatchState::default(),
            dialog: None,
            popup: None,
            popup_dismissed: false,
            perm_sel: 0,
            perm_scroll: 0,
            perm_since: None,
            toasts: Toasts::default(),
            leader: None,
            esc_at: None,
            details: false,
            thinking: false,
            sidebar: None,
            mouse_capture: true,
            tick: 0,
            dirty: true,
            quit: false,
            next_req: 1,
            reqs: HashMap::new(),
            model_override_used: false,
            drag: None,
            regions: Regions::default(),
            run_editor: false,
            size: (80, 24),
            highlight: true,
        }
    }

    pub fn req(&mut self, kind: Req) -> u64 {
        let r = self.next_req;
        self.next_req += 1;
        self.reqs.insert(r, kind);
        r
    }

    pub fn send(&self, m: ClientMsg) {
        self.conn.send(m);
    }

    pub fn toast(&mut self, level: NoticeLevel, text: impl Into<String>) {
        self.toasts.push(level, text);
        self.dirty = true;
    }

    pub fn agent_colors(&self) -> HashMap<String, Color> {
        let mut m = HashMap::new();
        if let Some(c) = &self.store.catalog {
            for (i, a) in c.agents.iter().enumerate() {
                let col = a.color.as_deref().and_then(theme::parse_color).unwrap_or(theme::AGENT_PALETTE[i % theme::AGENT_PALETTE.len()]);
                m.insert(a.name.clone(), col);
            }
        }
        m
    }

    pub fn agent_color(&self, name: &str) -> Color {
        self.agent_colors().get(name).copied().unwrap_or(theme::SECONDARY)
    }

    pub fn current_view(&self) -> Option<&crate::state::SessionView> {
        self.current.as_ref().and_then(|id| self.store.get(id))
    }

    pub fn view_mut(&mut self, id: &str) -> &mut ChatView {
        self.views.entry(id.to_string()).or_default()
    }

    /// Session that keyboard input targets.
    pub fn target_session(&self) -> Option<SessionId> {
        match self.mode {
            Mode::Normal => self.current.clone(),
            Mode::Watch => self.watch.focus.clone(),
        }
    }

    pub fn any_busy(&self) -> bool {
        self.store.sessions.values().any(|s| s.busy())
    }

    pub fn sidebar_visible(&self) -> bool {
        self.mode == Mode::Normal && self.sidebar.unwrap_or(self.size.0 >= 140) && self.size.0 >= 80
    }

    // ───────────────────────── connection events ─────────────────────────

    pub fn on_conn(&mut self, ev: ConnEvent) {
        self.dirty = true;
        match ev {
            ConnEvent::Connected => {
                self.connected = true;
                self.conn_error = None;
                let r = self.req(if self.startup_done { Req::List } else { Req::StartupList });
                self.send(ClientMsg::ListSessions { req: r });
                if self.startup_done {
                    if let Some(id) = self.current.clone() {
                        let r = self.req(Req::Attach);
                        self.send(ClientMsg::Attach { req: r, session_id: id });
                    }
                    for id in self.watched.clone() {
                        self.send(ClientMsg::Watch { session_id: id });
                    }
                } else {
                    self.startup();
                }
            }
            ConnEvent::Disconnected(reason) => {
                self.connected = false;
                self.conn_error = Some(reason);
            }
            ConnEvent::Error(e) => {
                self.connected = false;
                self.conn_error = Some(e);
            }
            ConnEvent::Msg(m) => self.on_server(*m),
        }
    }

    fn startup(&mut self) {
        if self.opts.watch {
            self.startup_done = true;
        } else if let Some(id) = self.opts.session.clone() {
            let r = self.req(Req::Attach);
            self.send(ClientMsg::Attach { req: r, session_id: id });
            self.startup_done = true;
        } else if self.opts.cont {
            // Decided when the startup SessionList arrives.
        } else {
            self.create_session(self.opts.agent.clone());
            self.startup_done = true;
        }
    }

    pub fn create_session(&mut self, agent: Option<String>) {
        let model = if self.model_override_used { None } else { self.opts.model.clone() };
        self.model_override_used = true;
        let r = self.req(Req::Create);
        self.send(ClientMsg::CreateSession { req: r, cwd: self.opts.cwd.clone(), agent, model, variant: None, name: None });
    }

    /// Make `id` the foreground session of the normal view.
    pub fn attach(&mut self, id: &str) {
        if self.current.as_deref() == Some(id) {
            self.mode = Mode::Normal;
            return;
        }
        if self.watched.remove(id) {
            self.send(ClientMsg::Unwatch { session_id: id.to_string() });
        }
        // The old session is detached once the new snapshot arrives (no blank screen meanwhile).
        let r = self.req(Req::Attach);
        self.send(ClientMsg::Attach { req: r, session_id: id.to_string() });
        self.mode = Mode::Normal;
    }

    pub fn on_server(&mut self, m: ServerMsg) {
        let mut req_kind = None;
        match &m {
            ServerMsg::SessionSnapshot { req, .. } | ServerMsg::SessionList { req, .. } if *req != 0 => {
                req_kind = self.reqs.remove(req);
            }
            ServerMsg::Notice { session_id, level, text } => {
                let prefix = match session_id {
                    Some(s) if Some(s) != self.current.as_ref() => {
                        self.store.info(s).map(|i| format!("[{}] ", i.name)).unwrap_or_default()
                    }
                    _ => String::new(),
                };
                self.toast(*level, format!("{prefix}{text}"));
            }
            ServerMsg::Error { req, text } => {
                let kind = self.reqs.remove(req);
                self.toast(NoticeLevel::Error, text.clone());
                if matches!(kind, Some(Req::Attach)) && self.current.is_none() && !self.opts.watch && self.mode == Mode::Normal {
                    // e.g. `-s` with an unknown id: fall back to a new session.
                    if self.opts.session.is_some() && !self.model_override_used {
                        self.create_session(self.opts.agent.clone());
                    }
                }
            }
            _ => {}
        }
        let snapshot_id = match &m {
            ServerMsg::SessionSnapshot { info, .. } => Some(info.id.clone()),
            _ => None,
        };
        if let Some(id) = &snapshot_id {
            let wanted = req_kind.is_some() || self.current.as_ref() == Some(id) || self.watched.contains(id);
            if !wanted {
                return;
            }
        }
        let info_update = matches!(m, ServerMsg::SessionUpdated { .. });
        let eff = self.store.apply(m);
        if let (Some(id), Some(Req::Create | Req::Attach)) = (&snapshot_id, &req_kind) {
            if self.current.as_deref() != Some(id) {
                if let Some(old) = self.current.take() {
                    self.send(ClientMsg::Detach { session_id: old.clone() });
                    if !self.watched.contains(&old) {
                        self.store.sessions.remove(&old);
                        self.views.remove(&old);
                    }
                }
            }
            self.current = Some(id.clone());
            self.view_mut(id).scroll_to_bottom();
            self.view_mut(id).follow = true;
            self.perm_sel = 0;
            self.perm_scroll = 0;
        }
        match eff {
            Effect::SessionListChanged => {
                if matches!(req_kind, Some(Req::StartupList)) && !self.startup_done {
                    self.startup_done = true;
                    let cwd = self.opts.cwd.clone();
                    let best = self.store.list.iter().filter(|s| s.cwd == cwd).max_by_key(|s| s.updated).map(|s| s.id.clone());
                    match best {
                        Some(id) => {
                            let r = self.req(Req::Attach);
                            self.send(ClientMsg::Attach { req: r, session_id: id });
                        }
                        None => self.create_session(self.opts.agent.clone()),
                    }
                }
                self.sync_watch();
                self.refresh_dialog();
            }
            Effect::Closed(id) => {
                if self.current.as_deref() == Some(&id) {
                    self.toast(NoticeLevel::Info, "Session closed");
                }
                self.sync_watch();
                self.refresh_dialog();
            }
            Effect::Undone(id, text) => {
                if self.current.as_deref() == Some(&id) {
                    self.editor.set_text(&text);
                }
            }
            Effect::Changed(_) if info_update && self.mode == Mode::Watch => self.sync_watch(),
            Effect::PermissionAsk(_) | Effect::Changed(_) | Effect::CatalogChanged | Effect::None => {}
        }
    }

    /// Watch exactly the open background sessions (only in watch mode).
    pub fn sync_watch(&mut self) {
        let want: HashSet<SessionId> = if self.mode == Mode::Watch {
            self.store.background().into_iter().map(|s| s.id.clone()).filter(|id| Some(id) != self.current.as_ref()).collect()
        } else {
            HashSet::new()
        };
        for id in want.difference(&self.watched.clone()) {
            self.send(ClientMsg::Watch { session_id: id.clone() });
            self.watched.insert(id.clone());
        }
        for id in self.watched.clone().difference(&want) {
            // Keep watching while it is still open, just foreground elsewhere? Spec: panes follow background set.
            self.send(ClientMsg::Unwatch { session_id: id.clone() });
            self.watched.remove(id);
            if self.current.as_ref() != Some(id) {
                self.store.sessions.remove(id);
            }
        }
        let panes = self.watch_ids();
        if self.watch.focus.as_ref().map(|f| !panes.contains(f)).unwrap_or(true) {
            self.watch.focus = panes.first().cloned();
        }
    }

    /// Ordered list of pane sessions in watch mode.
    pub fn watch_ids(&self) -> Vec<SessionId> {
        let mut v: Vec<&SessionInfo> = self.store.list.iter().filter(|s| self.watched.contains(&s.id)).collect();
        v.sort_by(|a, b| a.created.cmp(&b.created).then(a.id.cmp(&b.id)));
        v.into_iter().map(|s| s.id.clone()).collect()
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        self.drag = None;
        self.sync_watch();
    }

    pub fn on_app_event(&mut self, ev: AppEvent) {
        self.dirty = true;
        match ev {
            AppEvent::ClipboardImage { png: Some(png), .. } => {
                let image = ImageInput { mime: "image/png".into(), data: crate::clipboard::b64(&png), name: None };
                self.target_editor().add_image(image);
            }
            AppEvent::ClipboardImage { png: None, text_fallback } => {
                if text_fallback {
                    let tx = self.app_tx.clone();
                    tokio::spawn(async move {
                        if let Some(t) = crate::clipboard::read_text().await {
                            let _ = tx.send(AppEvent::ClipboardText(t));
                        }
                    });
                } else {
                    self.toast(NoticeLevel::Warning, "No image in clipboard");
                }
            }
            AppEvent::ClipboardText(t) => self.paste_text(&t),
        }
    }

    pub fn target_editor(&mut self) -> &mut Editor {
        match (&self.mode, self.watch.focus.clone()) {
            (Mode::Watch, Some(f)) => self.pane_editors.entry(f).or_default(),
            _ => &mut self.editor,
        }
    }

    pub fn paste_text(&mut self, t: &str) {
        if let Some((mime, data, name)) = crate::clipboard::image_from_path(t) {
            let image = ImageInput { mime, data: crate::clipboard::b64(&data), name: Some(name) };
            self.target_editor().add_image(image);
            return;
        }
        if self.mode == Mode::Watch {
            let t = t.replace('\n', " ");
            self.target_editor().insert_str(&t);
        } else {
            self.editor.paste(t);
        }
    }

    pub fn request_clipboard_image(&self, text_fallback: bool) {
        let tx = self.app_tx.clone();
        tokio::spawn(async move {
            let png = crate::clipboard::read_image().await;
            let _ = tx.send(AppEvent::ClipboardImage { png, text_fallback });
        });
    }

    pub fn copy(&mut self, text: String, what: &str) {
        let n = text.chars().count();
        crate::clipboard::copy(text);
        self.toast(NoticeLevel::Success, format!("Copied {what}{n} chars"));
    }

    /// Periodic tick (~10 Hz). Returns true if a redraw is needed.
    pub fn on_tick(&mut self) -> bool {
        let mut dirty = false;
        if self.toasts.gc() {
            dirty = true;
        }
        if let Some(t) = self.leader {
            if t.elapsed() > Duration::from_secs(2) {
                self.leader = None;
                dirty = true;
            }
        }
        if let Some(t) = self.esc_at {
            if t.elapsed() > Duration::from_secs(1) {
                self.esc_at = None;
                dirty = true;
            }
        }
        if self.drag_autoscroll() {
            dirty = true;
        }
        if self.any_busy() || !self.connected {
            self.tick += 1;
            dirty = true;
        }
        dirty
    }
}

/// Run the UI until quit.
pub async fn run(
    terminal: &mut crate::term::Term,
    mut app: App,
    mut conn_rx: mpsc::UnboundedReceiver<ConnEvent>,
    mut app_rx: mpsc::UnboundedReceiver<AppEvent>,
) -> anyhow::Result<()> {
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let frame = Duration::from_millis(16);
    let mut last_draw = Instant::now() - frame;
    if let Ok(s) = terminal.size() {
        app.size = (s.width, s.height);
    }
    loop {
        if app.dirty {
            let since = last_draw.elapsed();
            if since >= frame {
                crate::term::draw(terminal, &mut app)?;
                last_draw = Instant::now();
                app.dirty = false;
            }
        }
        if app.quit {
            break;
        }
        if app.run_editor {
            app.run_editor = false;
            drop(events);
            let text = app.target_editor().text().to_string();
            let edited = crate::term::with_suspended(terminal, || crate::term::edit_in_editor(&text))?;
            events = EventStream::new();
            if let Some(t) = edited {
                let t = t.trim_end_matches('\n').to_string();
                app.target_editor().set_text(&t);
            }
            app.dirty = true;
            continue;
        }
        let wait = if app.dirty { frame.saturating_sub(last_draw.elapsed()) } else { Duration::from_secs(3600) };
        tokio::select! {
            ev = events.next() => match ev {
                Some(Ok(ev)) => {
                    if let Event::Resize(w, h) = ev { app.size = (w, h); }
                    let moved = matches!(&ev, Event::Mouse(m) if m.kind == crossterm::event::MouseEventKind::Moved);
                    if !moved {
                        app.on_event(ev);
                        app.dirty = true;
                    }
                }
                Some(Err(_)) | None => break,
            },
            ev = conn_rx.recv() => {
                let Some(ev) = ev else { break };
                app.on_conn(ev);
                // Drain bursts (streaming deltas) before rendering.
                let mut n = 0;
                while n < 2000 {
                    match conn_rx.try_recv() { Ok(ev) => app.on_conn(ev), Err(_) => break }
                    n += 1;
                }
            }
            ev = app_rx.recv() => { if let Some(ev) = ev { app.on_app_event(ev); } }
            _ = tick.tick() => { if app.on_tick() { app.dirty = true; } }
            _ = tokio::time::sleep(wait) => {}
        }
    }
    Ok(())
}
