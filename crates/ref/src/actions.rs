//! User actions: commands, dialogs, submit, permission decisions, copy.

use ref_proto::*;

use crate::app::{App, Mode};
use crate::commands::{self, COMMANDS};
use crate::ui::dialogs::{DialogKind, Item, ListDialog, Popup};
use crate::ui::theme::*;
use crate::util::{fmt_date, fmt_tokens};

impl App {
    // ───────────────────────── submit ─────────────────────────

    pub fn submit(&mut self) {
        let (text, images) = self.editor.submission();
        if text.trim().is_empty() && images.is_empty() {
            return;
        }
        if let Some(rest) = text.strip_prefix('/') {
            let (name, args) = match rest.split_once(char::is_whitespace) {
                Some((n, a)) => (n.to_string(), a.trim().to_string()),
                None => (rest.trim().to_string(), String::new()),
            };
            if commands::find(&name).is_some() || self.custom_command(&name) {
                self.history.push(&text);
                self.editor.clear();
                self.popup = None;
                self.run_command(&name, &args);
                return;
            }
        }
        let Some(sid) = self.current.clone() else {
            self.toast(NoticeLevel::Warning, "No session (not connected yet)");
            return;
        };
        self.history.push(&text);
        self.send(ClientMsg::Prompt { session_id: sid.clone(), text, images });
        self.editor.clear();
        self.popup = None;
        self.view_mut(&sid).scroll_to_bottom();
    }

    pub fn submit_pane(&mut self) {
        let Some(sid) = self.watch.focus.clone() else { return };
        let ed = self.pane_editors.entry(sid.clone()).or_default();
        let (text, images) = ed.submission();
        if text.trim().is_empty() && images.is_empty() {
            return;
        }
        ed.clear();
        self.history.push(&text);
        self.send(ClientMsg::Prompt { session_id: sid.clone(), text, images });
        self.view_mut(&sid).scroll_to_bottom();
    }

    fn custom_command(&self, name: &str) -> bool {
        self.store.catalog.as_ref().map(|c| c.commands.iter().any(|x| x.name == name)).unwrap_or(false)
    }

    // ───────────────────────── commands ─────────────────────────

    pub fn run_command(&mut self, name: &str, args: &str) {
        self.dirty = true;
        let cmd = commands::find(name).map(|c| c.name).unwrap_or(name);
        let sid = self.target_session();
        match cmd {
            "new" => {
                let agent = self.current_view().map(|v| v.info.agent.clone()).or(self.opts.agent.clone());
                self.create_session(agent);
                self.mode = Mode::Normal;
                self.sync_watch();
            }
            "sessions" => self.open_sessions(false),
            "peers" => self.open_sessions(true),
            "models" => self.open_models(),
            "variant" => self.open_variants(),
            "agents" => self.open_agents(),
            "compact" => {
                if let Some(s) = sid {
                    self.send(ClientMsg::Compact { session_id: s });
                    self.toast(NoticeLevel::Info, "Compacting…");
                }
            }
            "undo" => {
                if let Some(s) = sid {
                    self.send(ClientMsg::Undo { session_id: s });
                }
            }
            "rename" => {
                if let Some(s) = sid {
                    if args.is_empty() {
                        let cur = self.store.info(&s).map(|i| i.name.clone()).unwrap_or_default();
                        self.editor.set_text(&format!("/rename {cur}"));
                        self.popup_dismissed = true;
                    } else {
                        self.send(ClientMsg::Rename { session_id: s, name: args.to_string() });
                    }
                }
            }
            "close" => {
                if let Some(s) = sid {
                    self.send(ClientMsg::Close { session_id: s.clone() });
                    if self.current.as_deref() == Some(&s) {
                        self.current = None;
                        self.store.sessions.remove(&s);
                        let agent = self.opts.agent.clone();
                        self.create_session(agent);
                    }
                }
            }
            "export" => self.export(),
            "copy" => {
                if let Some(v) = self.target_session().and_then(|s| self.store.get(&s)) {
                    let md = v.transcript_markdown();
                    self.copy(md, "transcript, ");
                }
            }
            "details" => {
                self.details = !self.details;
                for v in self.views.values_mut() {
                    v.toggled.clear();
                }
                self.toast(NoticeLevel::Info, if self.details { "Tool details: expanded" } else { "Tool details: collapsed" });
            }
            "thinking" => {
                self.thinking = !self.thinking;
                for v in self.views.values_mut() {
                    v.toggled.clear();
                }
                self.toast(NoticeLevel::Info, if self.thinking { "Reasoning: expanded" } else { "Reasoning: collapsed" });
            }
            "watch" => self.toggle_watch(),
            "editor" => self.run_editor = true,
            "sidebar" => {
                let v = self.sidebar_visible();
                self.sidebar = Some(!v);
            }
            "mouse" => {
                self.mouse_capture = !self.mouse_capture;
                crate::term::set_mouse_capture(self.mouse_capture);
                self.toast(
                    NoticeLevel::Info,
                    if self.mouse_capture { "Mouse capture on" } else { "Mouse capture off — native selection works (ctrl+x s to restore)" },
                );
            }
            "reload" => {
                self.send(ClientMsg::Reload);
                self.toast(NoticeLevel::Info, "Reloading configuration…");
            }
            "help" => self.open_help(),
            "exit" => self.quit = true,
            other => {
                if self.custom_command(other) {
                    if let Some(s) = sid {
                        self.send(ClientMsg::Command { session_id: s.clone(), name: other.to_string(), arguments: args.to_string() });
                        self.view_mut(&s).scroll_to_bottom();
                    }
                } else {
                    self.toast(NoticeLevel::Warning, format!("Unknown command /{other}"));
                }
            }
        }
    }

    pub fn toggle_watch(&mut self) {
        if self.mode == Mode::Watch {
            if self.current.is_none() {
                let agent = self.opts.agent.clone();
                self.create_session(agent);
            }
            self.set_mode(Mode::Normal);
        } else {
            self.set_mode(Mode::Watch);
        }
    }

    fn export(&mut self) {
        let Some(v) = self.target_session().and_then(|s| self.store.get(&s)) else { return };
        let md = v.transcript_markdown();
        let safe: String = v.info.name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
        let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let path = std::path::Path::new(&self.opts.cwd).join(format!("ref-{safe}-{ts}.md"));
        match std::fs::write(&path, md) {
            Ok(_) => self.toast(NoticeLevel::Success, format!("Exported to {}", path.display())),
            Err(e) => self.toast(NoticeLevel::Error, format!("Export failed: {e}")),
        }
    }

    // ───────────────────────── dialogs ─────────────────────────

    pub fn open_palette(&mut self) {
        let mut items: Vec<Item> = COMMANDS
            .iter()
            .map(|c| Item { id: format!("cmd:{}", c.name), label: format!("/{}", c.name), detail: c.desc.into(), right: c.key.into(), section: "Commands".into(), ..Default::default() })
            .collect();
        if let Some(cat) = &self.store.catalog {
            for c in &cat.commands {
                items.push(Item { id: format!("custom:{}", c.name), label: format!("/{}", c.name), detail: c.description.clone(), section: "Custom".into(), ..Default::default() });
            }
        }
        self.dialog = Some(ListDialog::new(DialogKind::Commands, "Commands", items, "enter run · esc close"));
    }

    pub fn session_items(&self, open_only: bool) -> Vec<Item> {
        let mut fg: Vec<&SessionInfo> = vec![];
        let mut bg: Vec<&SessionInfo> = vec![];
        let mut hist: Vec<&SessionInfo> = vec![];
        for s in &self.store.list {
            if s.open && (s.attached > 0 || Some(&s.id) == self.current.as_ref()) {
                fg.push(s);
            } else if s.open {
                bg.push(s);
            } else if !open_only {
                hist.push(s);
            }
        }
        for v in [&mut fg, &mut bg, &mut hist] {
            v.sort_by_key(|s| std::cmp::Reverse(s.updated));
        }
        let colors = self.agent_colors();
        let mut items = vec![];
        let mk = |s: &SessionInfo, section: &str| {
            let perm = if s.pending_permissions > 0 { format!(" · ⏸{}", s.pending_permissions) } else { String::new() };
            let model = s.model.split_once('/').map(|x| x.1).unwrap_or(&s.model).to_string();
            let mut badges = vec![];
            if s.open {
                let (b, c) = match s.status {
                    SessionStatus::Idle => ("idle", MUTED),
                    SessionStatus::Busy => ("busy", WARNING),
                    SessionStatus::Retrying { .. } => ("retrying", ERROR),
                };
                badges.push((b.to_string(), c));
            }
            Item {
                id: s.id.clone(),
                label: s.name.clone(),
                detail: format!("{} · {}{} · {}", s.agent, model, perm, short_cwd(&s.cwd)),
                section: section.into(),
                badges,
                right: fmt_date(s.updated),
                current: Some(&s.id) == self.current.as_ref(),
                color: colors.get(&s.agent).copied().filter(|_| false),
            }
        };
        for s in fg {
            items.push(mk(s, "Open — foreground"));
        }
        for s in bg {
            items.push(mk(s, "Open — background"));
        }
        for s in hist {
            items.push(mk(s, "History"));
        }
        items
    }

    pub fn open_sessions(&mut self, open_only: bool) {
        let r = self.req(crate::app::Req::List);
        self.send(ClientMsg::ListSessions { req: r });
        let items = self.session_items(open_only);
        let title = if open_only { "Open sessions" } else { "Sessions" };
        self.dialog = Some(ListDialog::new(DialogKind::Sessions, title, items, "enter attach · ctrl+d delete · ctrl+x close · esc"));
    }

    pub fn refresh_dialog(&mut self) {
        let Some(d) = &self.dialog else { return };
        if d.kind == DialogKind::Sessions {
            let open_only = d.title == "Open sessions";
            let items = self.session_items(open_only);
            if let Some(d) = self.dialog.as_mut() {
                d.set_items(items);
            }
        }
    }

    pub fn open_models(&mut self) {
        let Some(cat) = &self.store.catalog else { return };
        let cur = self.target_session().and_then(|s| self.store.info(&s)).map(|i| i.model.clone()).unwrap_or_default();
        let mut models = cat.models.clone();
        models.sort_by(|a, b| a.provider_name.cmp(&b.provider_name).then(a.name.cmp(&b.name)));
        let items = models
            .iter()
            .map(|m| {
                let mut badges = vec![];
                if m.image_input {
                    badges.push(("image".to_string(), INFO));
                }
                if m.reasoning {
                    badges.push(("reasoning".to_string(), ACCENT));
                }
                Item {
                    id: m.id.clone(),
                    label: m.name.clone(),
                    detail: m.id.clone(),
                    section: m.provider_name.clone(),
                    badges,
                    right: if m.context > 0 { fmt_tokens(m.context) } else { String::new() },
                    current: m.id == cur,
                    color: None,
                }
            })
            .collect();
        self.dialog = Some(ListDialog::new(DialogKind::Models, "Models (this session)", items, "enter select · esc close"));
    }

    pub fn current_variants(&self) -> (Vec<String>, Option<String>) {
        let Some(info) = self.target_session().and_then(|s| self.store.info(&s)).cloned() else { return (vec![], None) };
        let variants = self
            .store
            .catalog
            .as_ref()
            .and_then(|c| c.models.iter().find(|m| m.id == info.model))
            .map(|m| m.variants.clone())
            .unwrap_or_default();
        (variants, info.variant)
    }

    pub fn open_variants(&mut self) {
        let (vs, cur) = self.current_variants();
        let mut items = vec![Item { id: String::new(), label: "default".into(), current: cur.is_none(), ..Default::default() }];
        for v in vs {
            items.push(Item { id: v.clone(), label: v.clone(), current: cur.as_deref() == Some(&v), ..Default::default() });
        }
        self.dialog = Some(ListDialog::new(DialogKind::Variants, "Variant", items, "enter select · ctrl+t cycle"));
    }

    pub fn cycle_variant(&mut self) {
        let Some(sid) = self.target_session() else { return };
        let (vs, cur) = self.current_variants();
        if vs.is_empty() {
            self.toast(NoticeLevel::Info, "This model has no variants");
            return;
        }
        let mut all: Vec<Option<String>> = vec![None];
        all.extend(vs.into_iter().map(Some));
        let i = all.iter().position(|v| *v == cur).unwrap_or(0);
        let next = all[(i + 1) % all.len()].clone();
        self.toast(NoticeLevel::Info, format!("Variant: {}", next.clone().unwrap_or_else(|| "default".into())));
        self.send(ClientMsg::SetVariant { session_id: sid, variant: next });
    }

    pub fn open_agents(&mut self) {
        let Some(cat) = &self.store.catalog else { return };
        let cur = self.target_session().and_then(|s| self.store.info(&s)).map(|i| i.agent.clone()).unwrap_or_default();
        let colors = self.agent_colors();
        let items = cat
            .agents
            .iter()
            .map(|a| Item {
                id: a.name.clone(),
                label: a.name.clone(),
                detail: a.description.clone(),
                right: a.model.split_once('/').map(|x| x.1.to_string()).unwrap_or(a.model.clone()),
                current: a.name == cur,
                color: colors.get(&a.name).copied(),
                ..Default::default()
            })
            .collect();
        self.dialog = Some(ListDialog::new(DialogKind::Agents, "Agents", items, "enter select · tab cycles"));
    }

    pub fn cycle_agent(&mut self, back: bool) {
        let Some(sid) = self.target_session() else { return };
        let Some(cat) = &self.store.catalog else { return };
        if cat.agents.is_empty() {
            return;
        }
        let cur = self.store.info(&sid).map(|i| i.agent.clone()).unwrap_or_default();
        let n = cat.agents.len();
        let i = cat.agents.iter().position(|a| a.name == cur).unwrap_or(0);
        let j = if back { (i + n - 1) % n } else { (i + 1) % n };
        let name = cat.agents[j].name.clone();
        self.send(ClientMsg::SetAgent { session_id: sid.clone(), agent: name.clone() });
        // Optimistic update so rapid tabbing feels instant.
        if let Some(v) = self.store.get_mut(&sid) {
            v.info.agent = name;
            v.rev += 1;
        }
    }

    pub fn open_help(&mut self) {
        use ratatui::text::{Line, Span};
        let mut d = ListDialog::new(DialogKind::Help, "Help", vec![], "↑↓ scroll · esc close");
        let mut lines = vec![];
        let kw = commands::HELP.iter().map(|(k, _)| crate::ui::text::width(k)).max().unwrap_or(0) + 2;
        for (k, v) in commands::HELP {
            if k.is_empty() {
                if !lines.is_empty() {
                    lines.push(Line::default());
                }
                lines.push(Line::from(Span::styled(v.to_string(), bold(ACCENT))));
            } else {
                let pad = " ".repeat(kw - crate::ui::text::width(k));
                lines.push(Line::from(vec![Span::styled(format!("{k}{pad}"), fg(PRIMARY)), Span::styled(v.to_string(), text())]));
            }
        }
        lines.push(Line::default());
        lines.push(Line::from(Span::styled("Tip: hold shift while dragging for native terminal selection,", muted())));
        lines.push(Line::from(Span::styled("or toggle mouse capture with ctrl+x s.", muted())));
        d.help = lines;
        self.dialog = Some(d);
    }

    /// Enter in a dialog.
    pub fn dialog_activate(&mut self) {
        let Some(d) = self.dialog.as_ref() else { return };
        let kind = d.kind.clone();
        let Some(item) = d.selected_item().cloned() else {
            if kind == DialogKind::Help {
                self.dialog = None;
            }
            return;
        };
        self.dialog = None;
        let sid = self.target_session();
        match kind {
            DialogKind::Commands => {
                if let Some(c) = item.id.strip_prefix("cmd:") {
                    self.run_command(c, "");
                } else if let Some(c) = item.id.strip_prefix("custom:") {
                    self.editor.set_text(&format!("/{c} "));
                    self.popup_dismissed = true;
                }
            }
            DialogKind::Sessions => {
                let id = item.id.clone();
                self.attach(&id);
                self.sync_watch();
            }
            DialogKind::Models => {
                if let Some(s) = sid {
                    self.send(ClientMsg::SetModel { session_id: s, model: item.id.clone(), variant: None });
                    self.toast(NoticeLevel::Info, format!("Model: {}", item.id));
                }
            }
            DialogKind::Variants => {
                if let Some(s) = sid {
                    let v = if item.id.is_empty() { None } else { Some(item.id.clone()) };
                    self.send(ClientMsg::SetVariant { session_id: s, variant: v });
                }
            }
            DialogKind::Agents => {
                if let Some(s) = sid {
                    self.send(ClientMsg::SetAgent { session_id: s, agent: item.id.clone() });
                }
            }
            DialogKind::Help => {}
        }
    }

    // ───────────────────────── slash popup ─────────────────────────

    pub fn update_popup(&mut self) {
        let t = self.editor.text();
        let show = self.mode == Mode::Normal && t.starts_with('/') && !t.contains(char::is_whitespace) && !self.popup_dismissed;
        if !t.starts_with('/') {
            self.popup_dismissed = false;
        }
        if !show {
            self.popup = None;
            return;
        }
        let q = t[1..].to_lowercase();
        let mut items: Vec<(u8, Item)> = vec![];
        let mut add = |name: &str, aliases: &[&str], desc: &str, arg: bool| {
            let rank = if name.starts_with(&q) {
                0
            } else if aliases.iter().any(|a| a.starts_with(&q)) {
                1
            } else if name.contains(&q) || desc.to_lowercase().contains(&q) {
                2
            } else {
                return;
            };
            items.push((rank, Item { id: name.to_string(), label: format!("/{name}"), detail: desc.to_string(), right: if arg { "arg".into() } else { String::new() }, ..Default::default() }));
        };
        for c in COMMANDS {
            add(c.name, c.aliases, c.desc, c.arg);
        }
        if let Some(cat) = &self.store.catalog {
            for c in &cat.commands {
                add(&c.name, &[], &c.description, true);
            }
        }
        items.sort_by_key(|(r, _)| *r);
        let items: Vec<Item> = items.into_iter().map(|(_, i)| i).collect();
        if items.is_empty() {
            self.popup = None;
            return;
        }
        let selected = self.popup.as_ref().map(|p| p.selected.min(items.len() - 1)).unwrap_or(0);
        self.popup = Some(Popup { items, selected, rect: Default::default(), max_rows: 10 });
    }

    /// Accept the popup selection. `run` = enter (run if no args needed), else complete.
    pub fn popup_accept(&mut self, run: bool) {
        let Some(p) = self.popup.take() else { return };
        let Some(item) = p.items.get(p.selected) else { return };
        let name = item.id.clone();
        let needs_arg = !item.right.is_empty();
        if run && !needs_arg {
            self.editor.clear();
            self.history.push(&format!("/{name}"));
            self.run_command(&name, "");
        } else {
            self.editor.set_text(&format!("/{name} "));
            self.popup_dismissed = true;
        }
    }

    // ───────────────────────── permissions ─────────────────────────

    pub fn pending_permission(&self) -> Option<(PermissionRequest, usize)> {
        let sid = self.target_session()?;
        let v = self.store.get(&sid)?;
        v.permissions.first().cloned().map(|p| (p, v.permissions.len()))
    }

    /// True while the permission prompt just appeared (guards against keys typed for the input).
    pub fn perm_guard(&mut self) -> bool {
        let Some((p, _)) = self.pending_permission() else { return false };
        match &self.perm_since {
            Some((id, t)) if *id == p.call_id => t.elapsed() < std::time::Duration::from_millis(400),
            _ => {
                self.perm_since = Some((p.call_id.clone(), std::time::Instant::now()));
                true
            }
        }
    }

    pub fn decide(&mut self, d: PermissionDecision) {
        let Some((p, _)) = self.pending_permission() else { return };
        self.send(ClientMsg::Permission { session_id: p.session_id.clone(), call_id: p.call_id.clone(), decision: d });
        if let Some(v) = self.store.get_mut(&p.session_id) {
            v.permissions.retain(|x| x.call_id != p.call_id);
            v.rev += 1;
        }
        self.perm_sel = 0;
        self.perm_scroll = 0;
        self.dirty = true;
    }

    // ───────────────────────── copy ─────────────────────────

    pub fn copy_message(&mut self, sid: &str, mid: &str) {
        let Some(m) = self.store.get(sid).and_then(|v| v.message(mid)) else { return };
        let mut t = m.raw_text();
        if t.is_empty() && m.parts.is_empty() {
            t = m.error.clone().unwrap_or_default();
        }
        if t.is_empty() {
            // Tool-only assistant message: copy tool calls as JSON.
            t = m
                .parts
                .iter()
                .filter_map(|p| match p {
                    Part::Tool(tp) => Some(format!("{} {}\n{}", tp.name, serde_json::to_string_pretty(&tp.input).unwrap_or_default(), tp.output)),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n\n");
        }
        if t.is_empty() {
            t = m
                .parts
                .iter()
                .filter_map(|p| match p {
                    Part::Reasoning(r) if !r.text.trim().is_empty() => Some(r.text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n\n");
        }
        if t.is_empty() {
            self.toast(NoticeLevel::Info, "Nothing to copy");
            return;
        }
        self.copy(t, "");
    }

    pub fn copy_last_assistant(&mut self) {
        let Some(sid) = self.target_session() else { return };
        let Some(t) = self.store.get(&sid).and_then(|v| v.last_assistant()).map(|m| m.raw_text()) else {
            self.toast(NoticeLevel::Info, "No assistant message yet");
            return;
        };
        self.copy(t, "");
    }

    /// Copy the in-app selection of the target view; returns false if none.
    pub fn copy_selection(&mut self) -> bool {
        let Some(sid) = self.selection_session() else { return false };
        let t = self.views.get(&sid).map(|v| v.selected_text()).unwrap_or_default();
        if t.is_empty() {
            return false;
        }
        self.copy(t, "");
        true
    }

    /// Session whose view currently holds a non-empty selection.
    pub fn selection_session(&self) -> Option<String> {
        self.views.iter().find(|(_, v)| v.selection.map(|s| !s.is_empty()).unwrap_or(false)).map(|(k, _)| k.clone())
    }

    pub fn clear_selection(&mut self) {
        for v in self.views.values_mut() {
            v.selection = None;
        }
    }
}

pub fn short_cwd(cwd: &str) -> String {
    if let Some(home) = dirs::home_dir() {
        let h = home.to_string_lossy();
        if let Some(rest) = cwd.strip_prefix(h.as_ref()) {
            return format!("~{rest}");
        }
    }
    cwd.to_string()
}
