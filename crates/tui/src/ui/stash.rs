//! Two-pane Stash Groups dialog (groups on the left, members on the right).

use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ref_proto::{SessionInfo, StashGroup};

use super::dialog::{clear_wide_edges, frame, open_progress};
use super::editor::Editor;
use super::theme;
use crate::input::edit_key;
use crate::text::{self, width};

const MAX_DIALOG_W: u16 = 104;
const MAX_DIALOG_H: u16 = 28;
const MIN_DIALOG_W: u16 = 24;
const MIN_DIALOG_H: u16 = 10;
/// Share of inner width given to the left groups pane.
const LEFT_PANE_PCT: usize = 38;
const MIN_LEFT_PANE_W: u16 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StashPane {
    Groups,
    Members,
}

impl StashPane {
    fn delete_btn(self) -> StashButton {
        match self {
            StashPane::Groups => StashButton::DeleteGroup,
            StashPane::Members => StashButton::RemoveMember,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StashInputAction {
    CreateGroup,
    RenameGroup { group_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StashPickAction {
    MergeGroup {
        source_id: String,
    },
    MoveMember {
        from_group_id: String,
        session_id: String,
    },
    AddLive {
        group_id: String,
    },
}

pub enum StashMode {
    Browse,
    Input {
        action: StashInputAction,
        editor: Editor,
    },
    PickTarget {
        action: StashPickAction,
        targets: Vec<(String, String)>,
        sel: usize,
        rows: Vec<(u16, usize)>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StashButton {
    NewGroup,
    StashAll,
    RestoreGroup,
    RenameGroup,
    ToggleLock,
    MergeGroup,
    DeleteGroup,
    RestoreMember,
    RemoveMember,
    MoveMember,
    AddLive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StashCommand {
    Close,
    StashAll {
        name: Option<String>,
    },
    CreateGroup {
        name: Option<String>,
    },
    RenameGroup {
        group_id: String,
        name: String,
    },
    SetLocked {
        group_id: String,
        locked: bool,
    },
    DeleteGroup {
        group_id: String,
    },
    RestoreGroup {
        group_id: String,
    },
    RestoreMember {
        group_id: String,
        session_id: String,
    },
    RemoveMember {
        group_id: String,
        session_id: String,
    },
    MoveMember {
        from_group_id: String,
        to_group_id: String,
        session_id: String,
        position: u32,
    },
    MergeGroups {
        source_group_id: String,
        target_group_id: String,
    },
    AddToGroup {
        group_id: String,
        session_id: String,
    },
    Refresh,
}

pub struct StashDialog {
    pub pane: StashPane,
    pub group_sel: usize,
    pub member_sel: usize,
    pub group_scroll: usize,
    pub member_scroll: usize,
    pub opened: Instant,
    pub rect: Rect,
    pub groups_rect: Rect,
    pub members_rect: Rect,
    pub group_rows: Vec<(u16, usize)>,
    pub member_rows: Vec<(u16, usize)>,
    pub action_rects: Vec<(Rect, StashButton)>,
    pub confirm_delete: Option<String>,
    pub mode: StashMode,
    pub cursor_at: Option<(u16, u16)>,
    selected_group_id: Option<String>,
    selected_member_id: Option<String>,
}

impl Default for StashDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl StashDialog {
    pub fn new() -> Self {
        Self {
            pane: StashPane::Groups,
            group_sel: 0,
            member_sel: 0,
            group_scroll: 0,
            member_scroll: 0,
            opened: Instant::now(),
            rect: Rect::default(),
            groups_rect: Rect::default(),
            members_rect: Rect::default(),
            group_rows: Vec::new(),
            member_rows: Vec::new(),
            action_rects: Vec::new(),
            confirm_delete: None,
            mode: StashMode::Browse,
            cursor_at: None,
            selected_group_id: None,
            selected_member_id: None,
        }
    }

    /// Reconciles selection indices after `groups` changes (e.g. `GroupsChanged`).
    pub fn sync(&mut self, groups: &[StashGroup]) {
        if groups.is_empty() {
            self.group_sel = 0;
            self.member_sel = 0;
            self.selected_group_id = None;
            self.selected_member_id = None;
            self.confirm_delete = None;
            return;
        }
        if let Some(gid) = &self.selected_group_id
            && let Some(pos) = groups.iter().position(|g| &g.id == gid)
        {
            self.group_sel = pos;
        } else {
            self.group_sel = self.group_sel.min(groups.len() - 1);
            self.selected_group_id = Some(groups[self.group_sel].id.clone());
        }
        let members = &groups[self.group_sel].members;
        if members.is_empty() {
            self.member_sel = 0;
            self.selected_member_id = None;
        } else if let Some(sid) = &self.selected_member_id
            && let Some(pos) = members.iter().position(|m| &m.session_id == sid)
        {
            self.member_sel = pos;
        } else {
            self.member_sel = self.member_sel.min(members.len() - 1);
            self.selected_member_id = Some(members[self.member_sel].session_id.clone());
        }
        if let Some(cid) = &self.confirm_delete
            && !groups.iter().any(|g| &g.id == cid)
        {
            self.confirm_delete = None;
        }
    }

    fn current_group<'a>(&self, groups: &'a [StashGroup]) -> Option<&'a StashGroup> {
        groups.get(self.group_sel)
    }

    fn record_selection(&mut self, groups: &[StashGroup]) {
        self.selected_group_id = groups.get(self.group_sel).map(|g| g.id.clone());
        self.selected_member_id = groups
            .get(self.group_sel)
            .and_then(|g| g.members.get(self.member_sel))
            .map(|m| m.session_id.clone());
    }

    fn move_group(&mut self, delta: i32, groups: &[StashGroup]) {
        if groups.is_empty() {
            return;
        }
        self.group_sel = (self.group_sel as i32 + delta).rem_euclid(groups.len() as i32) as usize;
        self.member_sel = 0;
        self.member_scroll = 0;
        self.confirm_delete = None;
        self.record_selection(groups);
    }

    fn move_member(&mut self, delta: i32, groups: &[StashGroup]) {
        let len = self.current_group(groups).map_or(0, |g| g.members.len());
        if len == 0 {
            return;
        }
        self.member_sel = (self.member_sel as i32 + delta).rem_euclid(len as i32) as usize;
        self.confirm_delete = None;
        self.record_selection(groups);
    }

    pub fn handle_key(
        &mut self,
        k: &KeyEvent,
        groups: &[StashGroup],
        live: &[SessionInfo],
    ) -> Option<StashCommand> {
        self.sync(groups);
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match &self.mode {
            StashMode::Input { .. } => self.handle_input_key(k, ctrl),
            StashMode::PickTarget { .. } => self.handle_pick_key(k, ctrl),
            StashMode::Browse => self.handle_browse_key(k, groups, live),
        }
    }

    fn confirm_input(action: StashInputAction, text: String) -> Option<StashCommand> {
        match action {
            StashInputAction::CreateGroup => Some(StashCommand::CreateGroup {
                name: (!text.is_empty()).then_some(text),
            }),
            StashInputAction::RenameGroup { group_id } => {
                (!text.is_empty()).then_some(StashCommand::RenameGroup {
                    group_id,
                    name: text,
                })
            }
        }
    }

    fn handle_input_key(&mut self, k: &KeyEvent, ctrl: bool) -> Option<StashCommand> {
        let StashMode::Input { action, editor } = &mut self.mode else {
            return None;
        };
        match k.code {
            KeyCode::Esc => self.mode = StashMode::Browse,
            KeyCode::Char('c') if ctrl => self.mode = StashMode::Browse,
            KeyCode::Enter => {
                let text = editor.text.trim().to_string();
                let act = action.clone();
                self.mode = StashMode::Browse;
                return Self::confirm_input(act, text);
            }
            _ => {
                edit_key(editor, k, false);
            }
        }
        None
    }

    fn pick_command(action: StashPickAction, target_id: String) -> StashCommand {
        match action {
            StashPickAction::MergeGroup { source_id } => StashCommand::MergeGroups {
                source_group_id: source_id,
                target_group_id: target_id,
            },
            StashPickAction::MoveMember {
                from_group_id,
                session_id,
            } => StashCommand::MoveMember {
                from_group_id,
                to_group_id: target_id,
                session_id,
                position: 0,
            },
            StashPickAction::AddLive { group_id } => StashCommand::AddToGroup {
                group_id,
                session_id: target_id,
            },
        }
    }

    fn handle_pick_key(&mut self, k: &KeyEvent, ctrl: bool) -> Option<StashCommand> {
        let StashMode::PickTarget {
            action,
            targets,
            sel,
            ..
        } = &mut self.mode
        else {
            return None;
        };
        match k.code {
            KeyCode::Esc => self.mode = StashMode::Browse,
            KeyCode::Char('c') if ctrl => self.mode = StashMode::Browse,
            KeyCode::Up | KeyCode::Char('k') => {
                if !targets.is_empty() {
                    *sel = (*sel as i32 - 1).rem_euclid(targets.len() as i32) as usize;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !targets.is_empty() {
                    *sel = (*sel as i32 + 1).rem_euclid(targets.len() as i32) as usize;
                }
            }
            KeyCode::Enter => {
                let picked = targets.get(*sel).map(|(id, _)| id.clone());
                let act = action.clone();
                self.mode = StashMode::Browse;
                return picked.map(|target_id| Self::pick_command(act, target_id));
            }
            _ => {}
        }
        None
    }

    fn handle_browse_key(
        &mut self,
        k: &KeyEvent,
        groups: &[StashGroup],
        live: &[SessionInfo],
    ) -> Option<StashCommand> {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        match k.code {
            KeyCode::Esc | KeyCode::Char('q') if !ctrl => {
                if self.confirm_delete.take().is_some() {
                    return None;
                }
                return Some(StashCommand::Close);
            }
            KeyCode::Char('c') if ctrl => {
                return Some(StashCommand::Close);
            }
            KeyCode::Tab => {
                self.pane = match self.pane {
                    StashPane::Groups => StashPane::Members,
                    StashPane::Members => StashPane::Groups,
                };
            }
            KeyCode::Left => self.pane = StashPane::Groups,
            KeyCode::Right => self.pane = StashPane::Members,
            KeyCode::Up if shift && self.pane == StashPane::Members => {
                return self.reorder_member(-1, groups);
            }
            KeyCode::Down if shift && self.pane == StashPane::Members => {
                return self.reorder_member(1, groups);
            }
            KeyCode::Char('K') if self.pane == StashPane::Members => {
                return self.reorder_member(-1, groups);
            }
            KeyCode::Char('J') if self.pane == StashPane::Members => {
                return self.reorder_member(1, groups);
            }
            KeyCode::Up | KeyCode::Char('k') => match self.pane {
                StashPane::Groups => self.move_group(-1, groups),
                StashPane::Members => self.move_member(-1, groups),
            },
            KeyCode::Down | KeyCode::Char('j') => match self.pane {
                StashPane::Groups => self.move_group(1, groups),
                StashPane::Members => self.move_member(1, groups),
            },
            KeyCode::Char('p') if ctrl => match self.pane {
                StashPane::Groups => self.move_group(-1, groups),
                StashPane::Members => self.move_member(-1, groups),
            },
            KeyCode::Char('n') if ctrl => match self.pane {
                StashPane::Groups => self.move_group(1, groups),
                StashPane::Members => self.move_member(1, groups),
            },
            KeyCode::Char('c') => return self.trigger_button(StashButton::NewGroup, groups, live),
            KeyCode::Char('s') => return self.trigger_button(StashButton::StashAll, groups, live),
            KeyCode::Char('r') => {
                return self.trigger_button(StashButton::RenameGroup, groups, live);
            }
            KeyCode::Char('l') => {
                return self.trigger_button(StashButton::ToggleLock, groups, live);
            }
            KeyCode::Char('R') => {
                return self.trigger_button(StashButton::RestoreGroup, groups, live);
            }
            KeyCode::Char('m') => {
                let btn = match self.pane {
                    StashPane::Groups => StashButton::MergeGroup,
                    StashPane::Members => StashButton::MoveMember,
                };
                return self.trigger_button(btn, groups, live);
            }
            KeyCode::Char('a') => return self.trigger_button(StashButton::AddLive, groups, live),
            KeyCode::Char('g') => return Some(StashCommand::Refresh),
            KeyCode::Char('d') if ctrl => {
                let btn = match self.pane {
                    StashPane::Groups => StashPane::Groups.delete_btn(),
                    StashPane::Members => StashPane::Members.delete_btn(),
                };
                return self.trigger_button(btn, groups, live);
            }
            KeyCode::Char('x') | KeyCode::Delete => {
                let btn = match self.pane {
                    StashPane::Groups => StashPane::Groups.delete_btn(),
                    StashPane::Members => StashPane::Members.delete_btn(),
                };
                return self.trigger_button(btn, groups, live);
            }
            // In the Groups pane, Enter drills into a non-empty group's members (`R` or
            // double-click restores the whole group); empty groups trigger RestoreGroup directly.
            KeyCode::Enter => match self.pane {
                StashPane::Groups => {
                    if let Some(g) = self.current_group(groups) {
                        if !g.members.is_empty() {
                            self.pane = StashPane::Members;
                        } else {
                            return Some(StashCommand::RestoreGroup {
                                group_id: g.id.clone(),
                            });
                        }
                    }
                }
                StashPane::Members => {
                    return self.trigger_button(StashButton::RestoreMember, groups, live);
                }
            },
            _ => {}
        }
        None
    }

    fn reorder_member(&mut self, delta: i32, groups: &[StashGroup]) -> Option<StashCommand> {
        let g = self.current_group(groups)?;
        if g.members.len() < 2 {
            return None;
        }
        let m = g.members.get(self.member_sel)?;
        let next_pos =
            (self.member_sel as i32 + delta).clamp(0, (g.members.len() - 1) as i32) as usize;
        if next_pos == self.member_sel {
            return None;
        }
        let sid = m.session_id.clone();
        self.member_sel = next_pos;
        self.selected_member_id = Some(sid.clone());
        Some(StashCommand::MoveMember {
            from_group_id: g.id.clone(),
            to_group_id: g.id.clone(),
            session_id: sid,
            position: next_pos as u32,
        })
    }

    pub fn trigger_button(
        &mut self,
        btn: StashButton,
        groups: &[StashGroup],
        live: &[SessionInfo],
    ) -> Option<StashCommand> {
        match btn {
            StashButton::NewGroup => {
                self.mode = StashMode::Input {
                    action: StashInputAction::CreateGroup,
                    editor: Editor::default(),
                };
                None
            }
            StashButton::StashAll => Some(StashCommand::StashAll { name: None }),
            StashButton::RestoreGroup => {
                let g = self.current_group(groups)?;
                Some(StashCommand::RestoreGroup {
                    group_id: g.id.clone(),
                })
            }
            StashButton::RenameGroup => {
                let g = self.current_group(groups)?;
                let mut editor = Editor::default();
                editor.set_text(&g.name);
                self.mode = StashMode::Input {
                    action: StashInputAction::RenameGroup {
                        group_id: g.id.clone(),
                    },
                    editor,
                };
                None
            }
            StashButton::ToggleLock => {
                let g = self.current_group(groups)?;
                Some(StashCommand::SetLocked {
                    group_id: g.id.clone(),
                    locked: !g.locked,
                })
            }
            StashButton::DeleteGroup => {
                let g = self.current_group(groups)?;
                if self.confirm_delete.as_deref() == Some(g.id.as_str()) {
                    self.confirm_delete = None;
                    Some(StashCommand::DeleteGroup {
                        group_id: g.id.clone(),
                    })
                } else {
                    self.confirm_delete = Some(g.id.clone());
                    None
                }
            }
            StashButton::MergeGroup => {
                let g = self.current_group(groups)?;
                let targets: Vec<(String, String)> = groups
                    .iter()
                    .filter(|other| other.id != g.id)
                    .map(|other| {
                        (
                            other.id.clone(),
                            format!("{} ({})", other.name, other.members.len()),
                        )
                    })
                    .collect();
                if targets.is_empty() {
                    return None;
                }
                self.mode = StashMode::PickTarget {
                    action: StashPickAction::MergeGroup {
                        source_id: g.id.clone(),
                    },
                    targets,
                    sel: 0,
                    rows: Vec::new(),
                };
                None
            }
            StashButton::RestoreMember => {
                let g = self.current_group(groups)?;
                let m = g.members.get(self.member_sel)?;
                Some(StashCommand::RestoreMember {
                    group_id: g.id.clone(),
                    session_id: m.session_id.clone(),
                })
            }
            StashButton::RemoveMember => {
                let g = self.current_group(groups)?;
                let m = g.members.get(self.member_sel)?;
                Some(StashCommand::RemoveMember {
                    group_id: g.id.clone(),
                    session_id: m.session_id.clone(),
                })
            }
            StashButton::MoveMember => {
                let g = self.current_group(groups)?;
                let m = g.members.get(self.member_sel)?;
                let targets: Vec<(String, String)> = groups
                    .iter()
                    .filter(|other| other.id != g.id)
                    .map(|other| {
                        (
                            other.id.clone(),
                            format!("{} ({})", other.name, other.members.len()),
                        )
                    })
                    .collect();
                if targets.is_empty() {
                    return None;
                }
                self.mode = StashMode::PickTarget {
                    action: StashPickAction::MoveMember {
                        from_group_id: g.id.clone(),
                        session_id: m.session_id.clone(),
                    },
                    targets,
                    sel: 0,
                    rows: Vec::new(),
                };
                None
            }
            StashButton::AddLive => {
                let g = self.current_group(groups)?;
                let targets: Vec<(String, String)> = live
                    .iter()
                    .filter(|s| s.live && !g.members.iter().any(|m| m.session_id == s.id))
                    .map(|s| {
                        let label = if s.title.trim().is_empty() {
                            s.name.clone()
                        } else {
                            format!("{} · {}", s.name, text::one_line(&s.title))
                        };
                        (s.id.clone(), label)
                    })
                    .collect();
                if targets.is_empty() {
                    return None;
                }
                self.mode = StashMode::PickTarget {
                    action: StashPickAction::AddLive {
                        group_id: g.id.clone(),
                    },
                    targets,
                    sel: 0,
                    rows: Vec::new(),
                };
                None
            }
        }
    }

    pub fn handle_mouse(
        &mut self,
        me: MouseEvent,
        groups: &[StashGroup],
        live: &[SessionInfo],
        double_click: bool,
    ) -> (bool, Option<StashCommand>) {
        self.sync(groups);
        let pos = Position::new(me.column, me.row);
        match me.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.handle_left_click(pos, me.row, groups, live, double_click)
            }
            MouseEventKind::ScrollUp => self.handle_scroll(pos, -1, groups),
            MouseEventKind::ScrollDown => self.handle_scroll(pos, 1, groups),
            _ => (false, None),
        }
    }

    fn handle_left_click(
        &mut self,
        pos: Position,
        y: u16,
        groups: &[StashGroup],
        live: &[SessionInfo],
        double_click: bool,
    ) -> (bool, Option<StashCommand>) {
        if !self.rect.contains(pos) {
            return (true, Some(StashCommand::Close));
        }
        if let StashMode::PickTarget {
            action,
            targets,
            sel,
            rows,
        } = &mut self.mode
        {
            if let Some((_, idx)) = rows.iter().find(|(ry, _)| *ry == y) {
                *sel = *idx;
                let picked = targets.get(*sel).map(|(id, _)| id.clone());
                let act = action.clone();
                self.mode = StashMode::Browse;
                if let Some(target_id) = picked {
                    return (true, Some(Self::pick_command(act, target_id)));
                }
            }
            return (true, None);
        }
        if let Some((_, btn)) = self
            .action_rects
            .iter()
            .find(|(r, _)| r.contains(pos))
            .copied()
        {
            let cmd = self.trigger_button(btn, groups, live);
            return (true, cmd);
        }
        if let Some((_, idx)) = self.group_rows.iter().find(|(ry, _)| *ry == y).copied() {
            self.pane = StashPane::Groups;
            self.group_sel = idx;
            self.member_sel = 0;
            self.confirm_delete = None;
            self.record_selection(groups);
            // Double-click restores the whole group immediately (single Enter drills into members).
            if double_click && let Some(g) = groups.get(idx) {
                return (
                    true,
                    Some(StashCommand::RestoreGroup {
                        group_id: g.id.clone(),
                    }),
                );
            }
            return (true, None);
        }
        if let Some((_, idx)) = self.member_rows.iter().find(|(ry, _)| *ry == y).copied() {
            self.pane = StashPane::Members;
            self.member_sel = idx;
            self.confirm_delete = None;
            self.record_selection(groups);
            if double_click
                && let Some(g) = self.current_group(groups)
                && let Some(m) = g.members.get(idx)
            {
                return (
                    true,
                    Some(StashCommand::RestoreMember {
                        group_id: g.id.clone(),
                        session_id: m.session_id.clone(),
                    }),
                );
            }
            return (true, None);
        }
        if self.groups_rect.contains(pos) {
            self.pane = StashPane::Groups;
        } else if self.members_rect.contains(pos) {
            self.pane = StashPane::Members;
        }
        (true, None)
    }

    fn handle_scroll(
        &mut self,
        pos: Position,
        delta: i32,
        groups: &[StashGroup],
    ) -> (bool, Option<StashCommand>) {
        if !self.rect.contains(pos) {
            return (false, None);
        }
        if !matches!(self.mode, StashMode::Browse) {
            return (true, None);
        }
        if self.members_rect.contains(pos) {
            self.move_member(delta, groups);
        } else if self.groups_rect.contains(pos) {
            self.move_group(delta, groups);
        }
        (true, None)
    }

    pub fn draw(&mut self, buf: &mut Buffer, screen: Rect, groups: &[StashGroup]) {
        self.sync(groups);
        self.group_rows.clear();
        self.member_rows.clear();
        self.action_rects.clear();
        self.cursor_at = None;
        self.groups_rect = Rect::default();
        self.members_rect = Rect::default();

        if screen.width < MIN_DIALOG_W || screen.height < MIN_DIALOG_H {
            self.rect = Rect::default();
            return;
        }
        let t = open_progress(self.opened);
        let w = screen
            .width
            .saturating_sub(4)
            .clamp(MIN_DIALOG_W, MAX_DIALOG_W)
            .min(screen.width);
        let h = screen
            .height
            .saturating_sub(4)
            .clamp(MIN_DIALOG_H, MAX_DIALOG_H)
            .min(screen.height);
        let r = frame(buf, screen, w, h, "Stashed groups", t);
        self.rect = r;
        clear_wide_edges(buf, r);
        if r.width < 12 || r.height < 6 {
            return;
        }
        let inner = Rect::new(
            r.x + 2,
            r.y + 1,
            r.width.saturating_sub(4),
            r.height.saturating_sub(2),
        );
        let bg = theme::PANEL;
        let fade = |c: Color| theme::blend(bg, c, t);

        if let StashMode::Input { action, editor } = &mut self.mode {
            let prompt_label = match action {
                StashInputAction::CreateGroup => "New stash group name (optional):",
                StashInputAction::RenameGroup { .. } => "Rename stash group:",
            };
            buf.set_string(
                inner.x,
                inner.y + 1,
                text::truncate(prompt_label, inner.width as usize),
                Style::new()
                    .fg(fade(theme::ACCENT))
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            );
            let farea = Rect::new(inner.x, inner.y + 3, inner.width, 1);
            self.cursor_at = editor.draw_single_line(buf, farea, Some(bg), "Group name…", true);
            let hint_y = inner.y + inner.height.saturating_sub(1);
            buf.set_string(
                inner.x,
                hint_y,
                text::truncate("enter confirm · esc cancel", inner.width as usize),
                Style::new().fg(fade(theme::DIM)).bg(bg),
            );
            return;
        }

        if let StashMode::PickTarget {
            action,
            targets,
            sel,
            rows,
        } = &mut self.mode
        {
            rows.clear();
            let title = match action {
                StashPickAction::MergeGroup { .. } => "Merge selected group into:",
                StashPickAction::MoveMember { .. } => "Move member to group:",
                StashPickAction::AddLive { .. } => "Add live session to group:",
            };
            buf.set_string(
                inner.x,
                inner.y,
                text::truncate(title, inner.width as usize),
                Style::new()
                    .fg(fade(theme::ACCENT))
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            );
            let list_top = inner.y + 2;
            let list_h = inner.height.saturating_sub(3) as usize;
            for (i, (_, label)) in targets.iter().enumerate().take(list_h) {
                let y = list_top + i as u16;
                let selected = i == *sel;
                let rbg = if selected { theme::PRIMARY } else { bg };
                let fg = if selected {
                    theme::PANEL
                } else {
                    fade(theme::TEXT)
                };
                buf.set_style(Rect::new(inner.x, y, inner.width, 1), Style::new().bg(rbg));
                buf.set_string(
                    inner.x + 1,
                    y,
                    text::truncate(label, inner.width.saturating_sub(2) as usize),
                    Style::new().fg(fg).bg(rbg),
                );
                rows.push((y, i));
            }
            let hint_y = inner.y + inner.height.saturating_sub(1);
            buf.set_string(
                inner.x,
                hint_y,
                text::truncate("↑↓ select · enter confirm · esc back", inner.width as usize),
                Style::new().fg(fade(theme::DIM)).bg(bg),
            );
            return;
        }

        // Action toolbar row at top of inner area.
        let mut bx = inner.x;
        let toolbar_y = inner.y;
        let buttons: &[(&str, StashButton)] = &[
            ("+ New (c)", StashButton::NewGroup),
            ("Stash all (s)", StashButton::StashAll),
            ("Restore (R)", StashButton::RestoreGroup),
            ("Lock (l)", StashButton::ToggleLock),
            ("Rename (r)", StashButton::RenameGroup),
            ("Merge (m)", StashButton::MergeGroup),
            ("Del (x)", StashButton::DeleteGroup),
        ];
        for (label, btn) in buttons {
            let bw = width(label) as u16 + 2;
            if bx + bw > inner.x + inner.width {
                break;
            }
            let rect = Rect::new(bx, toolbar_y, bw, 1);
            buf.set_string(
                bx,
                toolbar_y,
                format!(" {label} "),
                Style::new().fg(fade(theme::TEXT)).bg(theme::ELEMENT),
            );
            self.action_rects.push((rect, *btn));
            bx += bw + 1;
        }

        let panes_top = inner.y + 2;
        let panes_h = inner.height.saturating_sub(3);
        let left_w = ((inner.width as usize * LEFT_PANE_PCT) / 100)
            .max(MIN_LEFT_PANE_W as usize)
            .min(inner.width.saturating_sub(MIN_LEFT_PANE_W) as usize) as u16;
        let right_x = inner.x + left_w + 1;
        let right_w = inner.width.saturating_sub(left_w + 1);
        self.groups_rect = Rect::new(inner.x, panes_top, left_w, panes_h);
        self.members_rect = Rect::new(right_x, panes_top, right_w, panes_h);

        // Vertical divider between panes.
        for yy in panes_top..panes_top + panes_h {
            buf.set_string(
                inner.x + left_w,
                yy,
                "│",
                Style::new().fg(fade(theme::BORDER_SUBTLE)).bg(bg),
            );
        }

        self.draw_groups_pane(buf, groups, fade);
        self.draw_members_pane(buf, groups, fade);

        let hint_y = inner.y + inner.height.saturating_sub(1);
        let hint = match self.pane {
            StashPane::Groups => {
                "tab pane · enter members · R restore · c new · s stash all · r rename · l lock · m merge · x del"
            }
            StashPane::Members => {
                "tab pane · enter restore · Shift+↑↓ reorder · m move · a add live · x remove · esc close"
            }
        };
        buf.set_string(
            inner.x,
            hint_y,
            text::truncate(hint, inner.width as usize),
            Style::new().fg(fade(theme::DIM)).bg(bg),
        );
    }

    fn draw_groups_pane(
        &mut self,
        buf: &mut Buffer,
        groups: &[StashGroup],
        fade: impl Fn(Color) -> Color,
    ) {
        let r = self.groups_rect;
        if r.height == 0 || r.width < 4 {
            return;
        }
        let active = self.pane == StashPane::Groups;
        let hdr_color = if active {
            fade(theme::PRIMARY)
        } else {
            fade(theme::ACCENT)
        };
        buf.set_string(
            r.x,
            r.y,
            text::truncate(&format!("Groups ({})", groups.len()), r.width as usize),
            Style::new()
                .fg(hdr_color)
                .bg(theme::PANEL)
                .add_modifier(Modifier::BOLD),
        );
        let list_h = r.height.saturating_sub(1) as usize;
        if groups.is_empty() {
            if list_h > 0 {
                buf.set_string(
                    r.x,
                    r.y + 1,
                    text::truncate("No stashed groups", r.width as usize),
                    Style::new().fg(fade(theme::MUTED)).bg(theme::PANEL),
                );
            }
            return;
        }
        if self.group_sel < self.group_scroll {
            self.group_scroll = self.group_sel;
        }
        if list_h > 0 && self.group_sel >= self.group_scroll + list_h {
            self.group_scroll = self.group_sel + 1 - list_h;
        }
        for (row_i, (gi, g)) in groups
            .iter()
            .enumerate()
            .skip(self.group_scroll)
            .take(list_h)
            .enumerate()
        {
            let y = r.y + 1 + row_i as u16;
            let selected = gi == self.group_sel;
            let rbg = if selected && active {
                theme::PRIMARY
            } else if selected {
                theme::PANEL_HOVER
            } else {
                theme::PANEL
            };
            let fg = if selected && active {
                theme::PANEL
            } else {
                fade(theme::TEXT)
            };
            let muted = if selected && active {
                theme::PANEL
            } else {
                fade(theme::MUTED)
            };
            buf.set_style(Rect::new(r.x, y, r.width, 1), Style::new().bg(rbg));
            let lock_tag = if g.locked { "🔒 " } else { "" };
            let confirm = self.confirm_delete.as_deref() == Some(g.id.as_str());
            let right_text = if confirm {
                "x again!".to_string()
            } else {
                format!("({})", g.members.len())
            };
            let left = vec![Span::styled(
                format!(" {lock_tag}{}", g.name),
                Style::new().fg(fg).bg(rbg).add_modifier(if selected {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
            )];
            let right = vec![Span::styled(
                format!("{right_text} "),
                Style::new()
                    .fg(if confirm { fade(theme::ERROR) } else { muted })
                    .bg(rbg),
            )];
            let (line, _) = super::message::compose(left, right, r.width as usize);
            buf.set_line(r.x, y, &line, r.width);
            self.group_rows.push((y, gi));
        }
    }

    fn draw_members_pane(
        &mut self,
        buf: &mut Buffer,
        groups: &[StashGroup],
        fade: impl Fn(Color) -> Color,
    ) {
        let r = self.members_rect;
        if r.height == 0 || r.width < 4 {
            return;
        }
        let active = self.pane == StashPane::Members;
        let hdr_color = if active {
            fade(theme::PRIMARY)
        } else {
            fade(theme::ACCENT)
        };
        let Some(g) = groups.get(self.group_sel) else {
            buf.set_string(
                r.x,
                r.y,
                "Members",
                Style::new()
                    .fg(hdr_color)
                    .bg(theme::PANEL)
                    .add_modifier(Modifier::BOLD),
            );
            return;
        };
        let hdr = format!("{} · {} member(s)", g.name, g.members.len());
        buf.set_string(
            r.x,
            r.y,
            text::truncate(&hdr, r.width as usize),
            Style::new()
                .fg(hdr_color)
                .bg(theme::PANEL)
                .add_modifier(Modifier::BOLD),
        );
        let list_h = r.height.saturating_sub(1) as usize;
        if g.members.is_empty() {
            if list_h > 0 {
                buf.set_string(
                    r.x,
                    r.y + 1,
                    text::truncate(
                        "Empty group (press a to add live session)",
                        r.width as usize,
                    ),
                    Style::new().fg(fade(theme::MUTED)).bg(theme::PANEL),
                );
            }
            return;
        }
        if self.member_sel < self.member_scroll {
            self.member_scroll = self.member_sel;
        }
        if list_h > 0 && self.member_sel >= self.member_scroll + list_h {
            self.member_scroll = self.member_sel + 1 - list_h;
        }
        let now = ref_proto::now_ms();
        let creator_colors = theme::assign_root_colors(groups.iter().flat_map(|grp| {
            grp.members.iter().map(|m| {
                m.info
                    .parent
                    .as_deref()
                    .filter(|p| !p.is_empty())
                    .unwrap_or(m.info.id.as_str())
            })
        }));
        for (row_i, (mi, m)) in g
            .members
            .iter()
            .enumerate()
            .skip(self.member_scroll)
            .take(list_h)
            .enumerate()
        {
            let y = r.y + 1 + row_i as u16;
            let selected = mi == self.member_sel;
            let rbg = if selected && active {
                theme::PRIMARY
            } else if selected {
                theme::PANEL_HOVER
            } else {
                theme::PANEL
            };
            let fg = if selected && active {
                theme::PANEL
            } else {
                fade(theme::TEXT)
            };
            let muted = if selected && active {
                theme::PANEL
            } else {
                fade(theme::MUTED)
            };
            buf.set_style(Rect::new(r.x, y, r.width, 1), Style::new().bg(rbg));
            let creator_key = m
                .info
                .parent
                .as_deref()
                .filter(|p| !p.is_empty())
                .unwrap_or(m.info.id.as_str());
            let creator_tint = creator_colors
                .get(creator_key)
                .copied()
                .unwrap_or_else(|| theme::root_task_color(creator_key));
            let bar_fg = if selected && active {
                theme::PANEL
            } else {
                fade(creator_tint)
            };
            let title = if m.info.title.trim().is_empty() {
                m.info.name.clone()
            } else {
                format!("{} · {}", m.info.name, text::one_line(&m.info.title))
            };
            let left = vec![
                Span::styled("▍", Style::new().fg(bar_fg).bg(rbg)),
                Span::styled(format!(" {title}"), Style::new().fg(fg).bg(rbg)),
            ];
            let right = vec![Span::styled(
                format!("{} ", text::fmt_ago(m.info.updated_at, now)),
                Style::new().fg(muted).bg(rbg),
            )];
            let (line, _) = super::message::compose(left, right, r.width as usize);
            buf.set_line(r.x, y, &line, r.width);
            self.member_rows.push((y, mi));
        }
    }
}
