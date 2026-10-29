//! Built-in slash commands, dialog construction and dialog outcomes.

use crate::app::{Action, App, ExportDest, Mode, Tag, BUILTINS};
use crate::dialog::{DKind, DValue, Dialog, ListItem, Outcome};
use crate::text::{fmt_time, tilde};
use crate::theme;
use hn_proto::{ClientMsg, ListScope, ModelRef, SessionInfo, ToastLevel};

impl App {
    pub fn run_builtin(&mut self, name: &str, args: &str) {
        let id = self.active();
        match name {
            "new" => {
                if self.mode == Mode::Monitor {
                    self.toast(ToastLevel::Info, "Monitor mode shows background sessions; run `hernees` for a new one");
                } else {
                    self.create_session();
                }
            }
            "sessions" => self.open_sessions_dialog(),
            "open" => self.open_open_dialog(false),
            "close" => {
                if args.is_empty() {
                    self.open_open_dialog(true);
                } else {
                    self.close_session(args);
                }
            }
            "models" => self.open_models_dialog(),
            "agents" => {
                if !args.is_empty() {
                    if let Some(id) = id {
                        self.req_set_agent(&id, args);
                    }
                } else {
                    self.open_agents_dialog();
                }
            }
            "compact" => {
                if let Some(id) = id {
                    self.client.request(ClientMsg::Compact { session: id }, Tag::Ack("Compacting context…".into()));
                }
            }
            "undo" => {
                if let Some(id) = id {
                    self.client.request(ClientMsg::Undo { session: id.clone() }, Tag::Undo(id));
                }
            }
            "rename" => {
                if let Some(id) = id {
                    if args.is_empty() {
                        let cur = self.session_info(&id).map(|i| i.name.clone()).unwrap_or_default();
                        self.open_dialog(Dialog::input(DKind::Rename, "Rename session", &cur));
                    } else {
                        self.rename(&id, args);
                    }
                }
            }
            "export" => {
                if let Some(id) = id {
                    let dest = match args {
                        "copy" | "clipboard" => ExportDest::Clipboard,
                        "" => ExportDest::File(None),
                        p => ExportDest::File(Some(p.to_string())),
                    };
                    self.client.request(ClientMsg::Export { session: id }, Tag::Export(dest));
                }
            }
            "copy" => {
                let text = id.and_then(|id| self.sessions.get(&id)).and_then(|s| s.last_assistant_text());
                match text {
                    Some(t) => self.copy_text(&t, "last response"),
                    None => self.toast(ToastLevel::Warning, "No assistant message to copy"),
                }
            }
            "editor" => self.actions.push(Action::RunEditor),
            "thinking" => {
                self.show_thinking = !self.show_thinking;
                let s = if self.show_thinking { "shown" } else { "hidden" };
                self.toast(ToastLevel::Info, format!("Reasoning {s}"));
            }
            "details" => {
                self.details = !self.details;
                for v in self.views.values_mut() {
                    v.reset_toggles();
                }
                let s = if self.details { "expanded" } else { "collapsed" };
                self.toast(ToastLevel::Info, format!("Tool details {s}"));
            }
            "sidebar" => {
                let cur = self.sidebar.unwrap_or(true);
                self.sidebar = Some(!cur);
            }
            "select" => self.toggle_mouse(),
            "help" => self.open_help(),
            "exit" => self.quit = true,
            _ => {}
        }
    }

    pub fn toggle_mouse(&mut self) {
        self.mouse_capture = !self.mouse_capture;
        self.actions.push(Action::SetMouse(self.mouse_capture));
        if self.mouse_capture {
            self.toast(ToastLevel::Info, "Mouse capture on");
        } else {
            self.toast(ToastLevel::Info, "Native selection mode · F2 to return");
        }
    }

    fn req_set_agent(&mut self, id: &str, agent: &str) {
        if let Some(s) = self.sessions.get_mut(id) {
            s.info.agent = agent.to_string();
        }
        self.client.request(ClientMsg::SetAgent { session: id.into(), agent: agent.into() }, Tag::Quiet);
    }

    /// Switch the session's model (optimistic update + toast on success).
    pub fn set_model(&mut self, id: &str, m: ModelRef) {
        let mut label = self.catalog.model_name(&m);
        if let Some(v) = &m.variant {
            label.push_str(&format!(" · {v}"));
        }
        if let Some(s) = self.sessions.get_mut(id) {
            s.info.model = m.clone();
        }
        self.client.request(ClientMsg::SetModel { session: id.into(), model: m }, Tag::Ack(format!("Model: {label}")));
    }

    /// ctrl+t: cycle the current model's variants (default → v1 → v2 → … → default).
    pub fn cycle_variant(&mut self) {
        let Some(id) = self.active() else { return };
        let Some(cur) = self.session_info(&id).map(|i| i.model.clone()) else { return };
        let Some(info) = self.catalog.model(&cur) else {
            self.toast(ToastLevel::Info, "Model list not loaded yet");
            return;
        };
        if info.variants.is_empty() {
            self.toast(ToastLevel::Info, format!("{} has no variants", info.name));
            return;
        }
        let pos = cur.variant.as_ref().and_then(|v| info.variants.iter().position(|x| x == v));
        let next = match pos {
            None => Some(info.variants[0].clone()),
            Some(i) if i + 1 < info.variants.len() => Some(info.variants[i + 1].clone()),
            Some(_) => None,
        };
        let m = ModelRef { variant: next, ..cur };
        self.set_model(&id, m);
    }

    fn rename(&mut self, id: &str, name: &str) {
        if let Some(s) = self.sessions.get_mut(id) {
            s.info.name = name.to_string();
        }
        self.client.request(ClientMsg::Rename { session: id.into(), name: name.into() }, Tag::Ack("Renamed".into()));
    }

    fn close_session(&mut self, id: &str) {
        self.client.request(ClientMsg::Close { session: id.into() }, Tag::Ack("Session closed".into()));
    }

    // --- dialog builders ------------------------------------------------------

    pub fn open_palette(&mut self) {
        let mut items = Vec::new();
        for (name, _, desc, key) in BUILTINS {
            items.push(
                ListItem::new("Commands", format!("/{name}"), DValue::Command(name.to_string())).desc(*desc).right(*key),
            );
        }
        for c in &self.catalog.commands {
            let g = match c.source.as_str() {
                "skill" => "Skills",
                "mcp" => "MCP prompts",
                _ => "Custom commands",
            };
            items.push(ListItem::new(g, format!("/{}", c.name), DValue::Command(format!("@{}", c.name))).desc(&c.description));
        }
        let mut d = Dialog::list(DKind::Commands, "Commands", items);
        d.placeholder = "Search commands".into();
        self.open_dialog(d);
    }

    pub fn open_models_dialog(&mut self) {
        let cur = self.active().and_then(|id| self.session_info(&id).map(|i| i.model.clone()));
        let mut items = Vec::new();
        for m in &self.catalog.models {
            let mut caps = Vec::new();
            if m.reasoning {
                caps.push("reasoning");
            }
            if m.image_input {
                caps.push("image");
            }
            let ctx = if m.context > 0 { format!("{} ctx", crate::text::fmt_tokens(m.context)) } else { String::new() };
            let right = [ctx, caps.join(" ")].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
            let is_cur = cur.as_ref().is_some_and(|c| c.provider == m.provider && c.model == m.model && c.variant.is_none());
            let mr = ModelRef { provider: m.provider.clone(), model: m.model.clone(), variant: None };
            let key = format!("{}/{}", m.provider, m.model);
            // Show the id only when it adds information beyond the display name.
            let desc = if m.name.eq_ignore_ascii_case(&m.model) { String::new() } else { m.model.clone() };
            let right = if m.variants.is_empty() {
                right
            } else {
                let n = m.variants.len();
                format!("{n} variant{} · {right}", if n == 1 { "" } else { "s" })
            };
            items.push(
                ListItem::new(&m.provider_name, &m.name, DValue::Model(mr))
                    .desc(desc)
                    .right(right)
                    .keys(format!("{key} {}", m.provider))
                    .current(is_cur),
            );
            for v in &m.variants {
                let is_cur = cur.as_ref().is_some_and(|c| {
                    c.provider == m.provider && c.model == m.model && c.variant.as_deref() == Some(v.as_str())
                });
                let mr = ModelRef { provider: m.provider.clone(), model: m.model.clone(), variant: Some(v.clone()) };
                items.push(
                    ListItem::new(&m.provider_name, v, DValue::Model(mr))
                        .parent(&m.name)
                        .keys(format!("{key}#{v} {}", m.provider))
                        .current(is_cur),
                );
            }
        }
        let mut d = Dialog::list(DKind::Models, "Select model", items);
        d.placeholder = "Search models (e.g. \"dgn sonnet high\")".into();
        d.hints = vec![("↑↓", "select"), ("enter", "use for this session")];
        if self.catalog.models.is_empty() {
            d.loading = true;
            self.client.request(ClientMsg::ListModels, Tag::Models);
        }
        self.open_dialog(d);
    }

    pub fn open_agents_dialog(&mut self) {
        let cur = self.active().and_then(|id| self.session_info(&id).map(|i| i.agent.clone())).unwrap_or_default();
        let items = self
            .catalog
            .agents
            .iter()
            .map(|a| {
                let model = a.model.as_ref().map(|m| self.catalog.model_name(m)).unwrap_or_default();
                ListItem::new("", crate::catalog::title_case(&a.name), DValue::Agent(a.name.clone()))
                    .desc(&a.description)
                    .right(model)
                    .dot("■", self.catalog.agent_color(&a.name))
                    .current(a.name == cur)
            })
            .collect();
        let mut d = Dialog::list(DKind::Agents, "Select agent", items);
        d.placeholder = "Search agents".into();
        self.open_dialog(d);
    }

    pub fn open_sessions_dialog(&mut self) {
        let mut d = Dialog::list(DKind::Sessions, "Sessions", Vec::new());
        d.loading = true;
        d.placeholder = "Search sessions".into();
        d.hints = vec![("↑↓", "select"), ("enter", "resume"), ("ctrl+d", "delete")];
        self.open_dialog(d);
        self.client.request(ClientMsg::ListSessions { scope: ListScope::History }, Tag::SessionsDialog);
    }

    pub fn open_open_dialog(&mut self, close: bool) {
        let title = if close { "Close a session" } else { "Open sessions" };
        let mut d = Dialog::list(DKind::Open { close }, title, Vec::new());
        d.loading = true;
        d.placeholder = "Search open sessions".into();
        d.hints = if close {
            vec![("↑↓", "select"), ("enter", "close session")]
        } else {
            vec![("↑↓", "select"), ("enter", "attach"), ("ctrl+d", "close")]
        };
        self.open_dialog(d);
        self.client.request(ClientMsg::ListSessions { scope: ListScope::Open }, Tag::OpenDialog);
    }

    fn session_item(&self, s: &SessionInfo, group: String) -> ListItem {
        let name = if s.name.is_empty() { format!("New session - {}", fmt_time(s.created_at)) } else { s.name.clone() };
        let me = self.current.as_deref() == Some(s.id.as_str());
        let (dot, color) = if !s.open {
            (" ", theme::MUTED)
        } else if s.status.is_active() {
            (theme::spinner(hn_proto::now_ms()), crate::app::status_color(&s.status))
        } else if s.is_foreground() {
            ("●", theme::SUCCESS)
        } else {
            ("◌", theme::INFO)
        };
        let state = if !s.open {
            String::new()
        } else if me {
            "this terminal".into()
        } else if s.is_foreground() {
            format!("foreground ×{}", s.attached)
        } else {
            "background".into()
        };
        let right = [state, fmt_time(s.updated_at)].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" · ");
        let desc = if s.cwd == self.cwd { s.agent.clone() } else { format!("{} · {}", s.agent, tilde(&s.cwd)) };
        ListItem::new(group, name, DValue::Session(s.id.clone())).desc(desc).right(right).dot(dot, color).current(me)
    }

    pub fn history_items(&self, mut v: Vec<SessionInfo>) -> Vec<ListItem> {
        v.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
        let today = chrono::Local::now().date_naive();
        v.iter()
            .map(|s| {
                use chrono::TimeZone;
                let g = chrono::Local
                    .timestamp_millis_opt(s.updated_at)
                    .single()
                    .map(|t| {
                        let d = t.date_naive();
                        if d == today {
                            "Today".to_string()
                        } else if Some(d) == today.pred_opt() {
                            "Yesterday".to_string()
                        } else {
                            t.format("%a %b %d %Y").to_string()
                        }
                    })
                    .unwrap_or_default();
                self.session_item(s, g)
            })
            .collect()
    }

    pub fn refresh_open_dialog(&mut self) {
        let Some(DKind::Open { .. }) = self.dialog.as_ref().map(|d| d.kind) else { return };
        let mut v: Vec<SessionInfo> = self.open.iter().filter(|s| s.open).cloned().collect();
        v.sort_by_key(|s| (s.is_foreground(), std::cmp::Reverse(s.updated_at)));
        let items: Vec<ListItem> = v
            .iter()
            .map(|s| {
                let g = if s.is_foreground() { "Foreground" } else { "Background" };
                self.session_item(s, g.into())
            })
            .collect();
        if let Some(d) = self.dialog.as_mut() {
            d.set_items(items);
        }
    }

    pub fn open_help(&mut self) {
        let keys: &[(&str, &str)] = &[
            ("enter", "send prompt"),
            ("shift+enter / alt+enter / ctrl+j", "newline"),
            ("esc esc", "interrupt the running session"),
            ("ctrl+c", "clear input · twice to exit"),
            ("ctrl+p", "command palette"),
            ("tab / shift+tab", "cycle agents"),
            ("ctrl+t", "cycle model variant (e.g. high / medium / default)"),
            ("↑ / ↓", "prompt history (single-line input)"),
            ("pgup / pgdn / wheel", "scroll messages"),
            ("ctrl+g / ctrl+alt+g", "jump to top / bottom"),
            ("alt+↑ / alt+↓", "focus previous / next tool or thinking block"),
            ("enter / space (focused)", "expand / collapse block"),
            ("click ▸ line", "expand / collapse"),
            ("click ⧉ copy", "copy raw message / tool input / output"),
            ("drag", "select text · ctrl+y / ctrl+shift+c / y copies"),
            ("F2 / ctrl+x s", "native terminal selection (mouse off)"),
            ("ctrl+v", "paste image from clipboard"),
            ("ctrl+x n", "new session"),
            ("ctrl+x l", "sessions"),
            ("ctrl+x o", "open sessions (attach to background)"),
            ("ctrl+x m / a", "models / agents"),
            ("ctrl+x c / u / r", "compact / undo / rename"),
            ("ctrl+x x / y / e", "export / copy last / $EDITOR"),
            ("ctrl+x t / d / b", "thinking / details / sidebar"),
            ("ctrl+x q", "exit"),
            ("monitor: alt+arrows, ctrl+x 1-9", "focus pane"),
            ("monitor: alt+z / ctrl+x z", "zoom pane"),
            ("monitor: ctrl+x [ / ]", "previous / next page"),
        ];
        // Align descriptions into a column.
        let kw = keys.iter().map(|(k, _)| crate::text::width(k)).max().unwrap_or(0);
        let mut items: Vec<ListItem> = keys
            .iter()
            .map(|(k, d)| {
                let pad = " ".repeat(kw.saturating_sub(crate::text::width(k)));
                ListItem::new("Keybinds", format!("{k}{pad}"), DValue::None).desc(*d)
            })
            .collect();
        for (name, aliases, desc, _) in BUILTINS {
            let al = if aliases.is_empty() { String::new() } else { format!("(/{})", aliases.join(", /")) };
            items.push(ListItem::new("Commands", format!("/{name}"), DValue::Command(name.to_string())).desc(*desc).right(al));
        }
        for c in &self.catalog.commands {
            items.push(
                ListItem::new("Custom", format!("/{}", c.name), DValue::Command(format!("@{}", c.name))).desc(&c.description),
            );
        }
        let mut d = Dialog::list(DKind::Help, "Help", items);
        d.placeholder = "Search keybinds and commands".into();
        self.open_dialog(d);
    }

    // --- dialog outcomes ------------------------------------------------------

    pub fn dialog_outcome(&mut self, out: Outcome) {
        let Some(kind) = self.dialog.as_ref().map(|d| d.kind) else { return };
        match out {
            Outcome::None => {}
            Outcome::Close => self.dialog = None,
            Outcome::Submit(text) => {
                self.dialog = None;
                if kind == DKind::Rename && !text.is_empty() {
                    if let Some(id) = self.active() {
                        self.rename(&id, &text);
                    }
                }
            }
            Outcome::Delete(DValue::Session(sid)) => {
                match kind {
                    DKind::Sessions => {
                        if self.current.as_deref() == Some(sid.as_str()) {
                            self.toast(ToastLevel::Warning, "Can't delete the current session");
                            return;
                        }
                        self.client.request(ClientMsg::Delete { session: sid.clone() }, Tag::Ack("Session deleted".into()));
                    }
                    _ => self.close_session(&sid),
                }
                if let Some(d) = self.dialog.as_mut() {
                    d.items.retain(|i| i.value != DValue::Session(sid.clone()));
                    let items = std::mem::take(&mut d.items);
                    d.set_items(items);
                }
            }
            Outcome::Delete(_) => {}
            Outcome::Select(v) => {
                self.dialog = None;
                match v {
                    DValue::Command(c) => {
                        if let Some(name) = c.strip_prefix('@') {
                            // Custom command: put it in the prompt so the user can add args.
                            let ed = self.active_editor();
                            ed.set_text(&format!("/{name} "));
                        } else if kind == DKind::Help || kind == DKind::Commands {
                            self.run_builtin(&c, "");
                        }
                    }
                    DValue::Model(m) => {
                        if let Some(id) = self.active() {
                            self.set_model(&id, m);
                        }
                    }
                    DValue::Agent(a) => {
                        if let Some(id) = self.active() {
                            self.req_set_agent(&id, &a);
                        }
                    }
                    DValue::Session(sid) => match kind {
                        DKind::Open { close: true } => self.close_session(&sid),
                        _ => {
                            if self.mode == Mode::Monitor {
                                // In monitor mode: focus that pane if it's one of ours.
                                if self.panes().contains(&sid) {
                                    self.focus_pane = Some(sid);
                                } else {
                                    self.toast(ToastLevel::Info, "That session is in the foreground elsewhere");
                                }
                            } else {
                                self.switch_to(&sid);
                            }
                        }
                    },
                    DValue::None => {}
                }
            }
        }
    }
}
