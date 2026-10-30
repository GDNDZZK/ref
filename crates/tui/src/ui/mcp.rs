//! MCP server status: data model (from `McpStatus` or the `GetConfig` fallback), sidebar rows
//! and the read-only `/mcp` dialog.

use std::collections::BTreeSet;
use std::time::Instant;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ref_proto::McpServerStatus;

use super::text::{self, width};
use super::{dialog, message, theme};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpState {
    Connected,
    Connecting,
    Failed,
    Disabled,
    /// Unknown status text from a newer backend.
    Other(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct McpEntry {
    pub name: String,
    pub state: McpState,
    pub error: Option<String>,
    /// Tool names with the `<server>_` prefix stripped (empty in fallback mode).
    pub tools: Vec<String>,
    /// Number of tools (known even when the names are not).
    pub n_tools: usize,
}

fn parse_state(s: &str) -> McpState {
    match s.trim().to_ascii_lowercase().as_str() {
        "connected" => McpState::Connected,
        "connecting" | "pending" => McpState::Connecting,
        "failed" | "error" => McpState::Failed,
        "disabled" => McpState::Disabled,
        o => McpState::Other(o.to_string()),
    }
}

impl McpEntry {
    pub fn from_status(s: &McpServerStatus) -> Self {
        let prefix = format!("{}_", s.name);
        let tools: Vec<String> = s
            .tools
            .iter()
            .map(|t| {
                t.strip_prefix(&prefix)
                    .filter(|x| !x.is_empty())
                    .unwrap_or(t)
                    .to_string()
            })
            .collect();
        McpEntry {
            name: s.name.clone(),
            state: parse_state(&s.status),
            error: s.error.clone().filter(|e| !e.trim().is_empty()),
            n_tools: tools.len(),
            tools,
        }
    }

    /// Fallback: a `ServerConfigInfo.mcp` status line, e.g. `connected (3 tools)` / `failed: boom`.
    pub fn from_line(name: &str, line: &str) -> Self {
        let l = line.trim();
        let (head, rest) = match l.split_once(':') {
            Some((h, r)) if !h.contains('(') => (h.trim(), Some(r.trim())),
            _ => (l, None),
        };
        let word = head
            .split(|c: char| c.is_whitespace() || c == '(')
            .next()
            .unwrap_or("");
        let state = parse_state(word);
        let n_tools = head
            .split_once('(')
            .and_then(|(_, r)| r.split_whitespace().next())
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        let error = match state {
            McpState::Failed => rest.filter(|r| !r.is_empty()).map(str::to_string),
            _ => None,
        };
        McpEntry {
            name: name.to_string(),
            state,
            error,
            tools: Vec::new(),
            n_tools,
        }
    }

    pub fn connecting(&self) -> bool {
        self.state == McpState::Connecting
    }
}

pub fn sorted(mut v: Vec<McpEntry>) -> Vec<McpEntry> {
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

/// Status dot; the connecting one pulses with `tick`.
pub fn dot(state: &McpState, tick: u64) -> Span<'static> {
    match state {
        McpState::Connected => Span::styled("●", theme::fg(theme::SUCCESS)),
        McpState::Connecting => {
            let phase = (tick % 12) as f32 / 6.0;
            let t = (phase - 1.0).abs();
            Span::styled(
                "◌",
                theme::fg(theme::blend(theme::DIM, theme::WARNING, 0.35 + 0.65 * t)),
            )
        }
        McpState::Failed => Span::styled("●", theme::fg(theme::ERROR)),
        McpState::Disabled => Span::styled("○", theme::fg(theme::DIM)),
        McpState::Other(_) => Span::styled("●", theme::fg(theme::MUTED)),
    }
}

fn tools_label(n: usize) -> String {
    format!("{n} tool{}", if n == 1 { "" } else { "s" })
}

/// Short right-hand status text and its colour.
pub fn status_text(e: &McpEntry) -> (String, Color) {
    match &e.state {
        McpState::Connected => (tools_label(e.n_tools), theme::MUTED),
        McpState::Connecting => ("connecting…".into(), theme::WARNING),
        McpState::Failed => (
            e.error
                .as_deref()
                .map(text::one_line)
                .unwrap_or_else(|| "failed".into()),
            theme::ERROR,
        ),
        McpState::Disabled => ("disabled".into(), theme::DIM),
        McpState::Other(s) => (s.clone(), theme::MUTED),
    }
}

/// `MCP` header summary, e.g. `2/3` (connected / enabled).
pub fn summary(entries: &[McpEntry]) -> String {
    let enabled = entries
        .iter()
        .filter(|e| e.state != McpState::Disabled)
        .count();
    let on = entries
        .iter()
        .filter(|e| e.state == McpState::Connected)
        .count();
    format!("{on}/{enabled}")
}

/// One sidebar row: `● name        12 tools` within `w` columns. The name keeps at least about
/// half the row; the status text is truncated first.
pub fn sidebar_line(e: &McpEntry, w: usize, tick: u64) -> Line<'static> {
    let (st, color) = status_text(e);
    let name_w = width(&e.name);
    let avail = w.saturating_sub(2);
    let min_name = name_w.min(avail.div_ceil(2).max(4));
    let right_room = avail.saturating_sub(min_name + 1);
    let right = if right_room >= 3 {
        text::truncate(&st, right_room)
    } else {
        String::new()
    };
    let left = vec![
        dot(&e.state, tick),
        Span::raw(" "),
        Span::styled(e.name.clone(), theme::text()),
    ];
    let r = if right.is_empty() {
        vec![]
    } else {
        vec![Span::styled(right, theme::fg(color))]
    };
    message::compose(left, r, w).0
}

// ---------------------------------------------------------------------------
// /mcp dialog
// ---------------------------------------------------------------------------

/// Dialog content: lines (with the server index on header rows) and each server's row range.
type Content = (Vec<(Line<'static>, Option<usize>)>, Vec<(usize, usize)>);

pub struct McpDialog {
    pub sel: usize,
    /// Expanded server names (tool list shown).
    pub expanded: BTreeSet<String>,
    pub scroll: usize,
    pub opened: Instant,
    pub rect: Rect,
    /// Header rows at last draw: (y, server index).
    rows: Vec<(u16, usize)>,
    max_scroll: usize,
    /// Keep the selected block visible on the next draw.
    follow: bool,
    /// Server to select once the list arrives.
    pub want: Option<String>,
}

impl McpDialog {
    pub fn new(select: Option<&str>) -> Self {
        let mut expanded = BTreeSet::new();
        if let Some(s) = select {
            expanded.insert(s.to_string());
        }
        McpDialog {
            sel: 0,
            expanded,
            scroll: 0,
            opened: Instant::now(),
            rect: Rect::default(),
            rows: Vec::new(),
            max_scroll: 0,
            follow: true,
            want: select.map(str::to_string),
        }
    }

    pub fn move_sel(&mut self, d: i32, n: usize) {
        if n == 0 {
            return;
        }
        self.sel = (self.sel as i32 + d).rem_euclid(n as i32) as usize;
        self.follow = true;
    }

    /// Expand (`Some(true)`), collapse (`Some(false)`) or toggle (`None`) the selected server.
    pub fn set_expanded(&mut self, entries: &[McpEntry], v: Option<bool>) {
        let Some(e) = entries.get(self.sel) else {
            return;
        };
        let on = v.unwrap_or(!self.expanded.contains(&e.name));
        if on {
            self.expanded.insert(e.name.clone());
        } else {
            self.expanded.remove(&e.name);
        }
        self.follow = true;
    }

    pub fn scroll_by(&mut self, d: i32) {
        self.scroll = (self.scroll as i32 + d).clamp(0, self.max_scroll as i32) as usize;
        self.follow = false;
    }

    pub fn server_at(&self, x: u16, y: u16) -> Option<usize> {
        if x < self.rect.x || x >= self.rect.x + self.rect.width {
            return None;
        }
        self.rows.iter().find(|(ry, _)| *ry == y).map(|(_, i)| *i)
    }

    pub fn contains(&self, x: u16, y: u16) -> bool {
        let r = self.rect;
        x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
    }

    /// Content lines for width `iw`: (line, server index for header rows).
    fn content(&self, entries: &[McpEntry], iw: usize, tick: u64, loaded: bool) -> Content {
        let mut out: Vec<(Line<'static>, Option<usize>)> = Vec::new();
        let mut blocks = Vec::new();
        if entries.is_empty() {
            let msg = if loaded {
                "No MCP servers configured"
            } else {
                "Loading MCP servers…"
            };
            out.push((Line::from(Span::styled(msg, theme::muted())), None));
            if loaded {
                out.push((
                    Line::from(Span::styled(
                        "add servers under \"mcp\" in your config",
                        theme::fg(theme::DIM),
                    )),
                    None,
                ));
            }
            return (out, blocks);
        }
        for (i, e) in entries.iter().enumerate() {
            let start = out.len();
            let exp = self.expanded.contains(&e.name);
            let has_tools = e.n_tools > 0;
            let arrow = if has_tools {
                if exp { "▾ " } else { "▸ " }
            } else {
                "  "
            };
            let left = vec![
                Span::styled(arrow, theme::fg(theme::DIM)),
                dot(&e.state, tick),
                Span::raw(" "),
                Span::styled(e.name.clone(), theme::bold(theme::TEXT)),
            ];
            let (st, color) = match &e.state {
                McpState::Connected => (
                    format!("connected · {}", tools_label(e.n_tools)),
                    theme::SUCCESS,
                ),
                McpState::Failed => ("failed".to_string(), theme::ERROR),
                _ => status_text(e),
            };
            let (line, _) = message::compose(left, vec![Span::styled(st, theme::fg(color))], iw);
            out.push((line, Some(i)));
            let ind = "    ";
            let body_w = iw.saturating_sub(ind.len()).max(8);
            if let Some(err) = &e.error {
                let flat = text::sanitize(err);
                let mut lines = Vec::new();
                for (li, logical) in flat.trim().split('\n').enumerate() {
                    let content = if li == 0 {
                        format!("error: {logical}")
                    } else {
                        logical.to_string()
                    };
                    lines.extend(text::wrap_spans(
                        &[Span::styled(content, theme::fg(theme::ERROR))],
                        body_w,
                    ));
                }
                // Long errors are capped unless the server is expanded.
                let cap = if exp { usize::MAX } else { 4 };
                let n = lines.len();
                for l in lines.into_iter().take(cap) {
                    let mut spans = vec![Span::raw(ind)];
                    spans.extend(l.spans);
                    out.push((Line::from(spans), None));
                }
                if n > cap {
                    out.push((
                        Line::from(vec![
                            Span::raw(ind),
                            Span::styled(
                                format!("… {} more lines (enter to show)", n - cap),
                                theme::fg(theme::DIM),
                            ),
                        ]),
                        None,
                    ));
                }
            }
            if exp && has_tools {
                if e.tools.is_empty() {
                    out.push((
                        Line::from(vec![
                            Span::raw(ind),
                            Span::styled(
                                format!(
                                    "{} (names not reported by this backend)",
                                    tools_label(e.n_tools)
                                ),
                                theme::muted(),
                            ),
                        ]),
                        None,
                    ));
                } else {
                    for l in tool_grid(&e.tools, body_w) {
                        let mut spans = vec![Span::raw(ind)];
                        spans.extend(l.spans);
                        out.push((Line::from(spans), None));
                    }
                }
            }
            blocks.push((start, out.len()));
            if i + 1 < entries.len() && (exp || e.error.is_some()) {
                out.push((Line::default(), None));
            }
        }
        (out, blocks)
    }

    pub fn draw(
        &mut self,
        buf: &mut Buffer,
        screen: Rect,
        entries: &[McpEntry],
        tick: u64,
        loaded: bool,
    ) {
        if let Some(w) = self.want.clone()
            && let Some(p) = entries.iter().position(|e| e.name == w)
        {
            self.sel = p;
            self.want = None;
            self.follow = true;
        }
        self.sel = self.sel.min(entries.len().saturating_sub(1));
        let t = (self.opened.elapsed().as_secs_f32() / 0.12).min(1.0);
        let w = screen.width.saturating_sub(4).clamp(30, 90);
        let iw = w.saturating_sub(4) as usize;
        let (lines, blocks) = self.content(entries, iw, tick, loaded);
        // Rows: border, gap, content, gap, footer, border.
        let max_h = screen.height.saturating_sub(4).clamp(6, 40);
        let h = (lines.len() as u16 + 5).clamp(6, max_h);
        let r = dialog::frame(buf, screen, w, h, "MCP servers", t);
        self.rect = r;
        let inner = Rect::new(
            r.x + 2,
            r.y + 2,
            r.width.saturating_sub(4),
            r.height.saturating_sub(5),
        );
        let vh = inner.height as usize;
        self.max_scroll = lines.len().saturating_sub(vh);
        if self.follow
            && let Some(&(s, e)) = blocks.get(self.sel)
        {
            // Show the whole block if it fits, else at least its header.
            if e > self.scroll + vh {
                self.scroll = e.saturating_sub(vh);
            }
            if s < self.scroll {
                self.scroll = s;
            }
            self.follow = false;
        }
        self.scroll = self.scroll.min(self.max_scroll);
        let bg = theme::PANEL;
        self.rows.clear();
        for (k, (l, idx)) in lines.iter().enumerate().skip(self.scroll).take(vh) {
            let y = inner.y + (k - self.scroll) as u16;
            let selected = idx.is_some_and(|i| i == self.sel);
            let mut l = l.clone();
            for s in &mut l.spans {
                if selected {
                    s.style = s
                        .style
                        .fg(theme::PANEL)
                        .bg(theme::PRIMARY)
                        .add_modifier(Modifier::BOLD);
                } else {
                    s.style = s.style.bg(bg);
                    if let Some(c) = s.style.fg {
                        s.style.fg = Some(theme::blend(bg, c, t));
                    }
                }
            }
            if selected {
                buf.set_style(
                    Rect::new(inner.x.saturating_sub(1), y, inner.width + 2, 1),
                    Style::new().bg(theme::PRIMARY),
                );
            }
            buf.set_line(inner.x, y, &l, inner.width);
            if let Some(i) = idx {
                self.rows.push((y, *i));
            }
        }
        // Scroll hint + footer.
        let fy = r.y + r.height.saturating_sub(2);
        let more = if self.scroll < self.max_scroll {
            " · pgdn more"
        } else {
            ""
        };
        let hint = format!("↑↓ select · enter/→ tools · ← hide{more} · esc close");
        let total: usize = entries.iter().map(|e| e.n_tools).sum();
        let count = format!("{} connected · {}", summary(entries), tools_label(total));
        let iwf = inner.width as usize;
        let cw = width(&count);
        buf.set_string(
            inner.x,
            fy,
            text::truncate(&hint, iwf.saturating_sub(cw + 2)),
            Style::new().fg(theme::blend(bg, theme::DIM, t)).bg(bg),
        );
        if cw + 2 < iwf {
            buf.set_string(
                inner.x + inner.width - cw as u16,
                fy,
                &count,
                Style::new().fg(theme::blend(bg, theme::DIM, t)).bg(bg),
            );
        }
    }
}

/// Tool names laid out in columns within `w`.
fn tool_grid(tools: &[String], w: usize) -> Vec<Line<'static>> {
    let maxw = tools.iter().map(|t| width(t)).max().unwrap_or(1).min(w);
    let colw = maxw + 3;
    let cols = (w / colw).max(1);
    let rows = tools.len().div_ceil(cols);
    let mut out = Vec::new();
    for r in 0..rows {
        let mut spans = Vec::new();
        for c in 0..cols {
            // Column-major so the list reads top to bottom.
            let Some(t) = tools.get(c * rows + r) else {
                break;
            };
            let name = text::truncate(t, maxw);
            let pad = if c + 1 < cols {
                colw.saturating_sub(width(&name) + 2)
            } else {
                0
            };
            spans.push(Span::styled("· ", theme::fg(theme::DIM)));
            spans.push(Span::styled(name, theme::fg(theme::INFO)));
            spans.push(Span::raw(" ".repeat(pad)));
        }
        out.push(Line::from(spans));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_fallback_lines() {
        let e = McpEntry::from_line("ctx", "connected (12 tools)");
        assert_eq!(
            (e.state.clone(), e.n_tools, e.error.clone()),
            (McpState::Connected, 12, None)
        );
        let e = McpEntry::from_line("x", "failed: HTTP 401: Unauthorized");
        assert_eq!(e.state, McpState::Failed);
        assert_eq!(e.error.as_deref(), Some("HTTP 401: Unauthorized"));
        assert_eq!(
            McpEntry::from_line("x", "connecting").state,
            McpState::Connecting
        );
        assert_eq!(
            McpEntry::from_line("x", "disabled").state,
            McpState::Disabled
        );
        assert!(matches!(
            McpEntry::from_line("x", "needs_auth").state,
            McpState::Other(_)
        ));
    }

    #[test]
    fn status_strips_prefix() {
        let s = McpServerStatus {
            name: "gh".into(),
            status: "connected".into(),
            error: None,
            tools: vec!["gh_search".into(), "gh_".into(), "other".into()],
        };
        let e = McpEntry::from_status(&s);
        assert_eq!(e.tools, vec!["search", "gh_", "other"]);
        assert_eq!(e.n_tools, 3);
    }

    #[test]
    fn sidebar_line_fits() {
        let e = McpEntry {
            name: "very-long-server-name-here".into(),
            state: McpState::Failed,
            error: Some("connection refused (os error 111) while connecting".into()),
            tools: vec![],
            n_tools: 0,
        };
        for w in [12usize, 20, 30] {
            let l = sidebar_line(&e, w, 0);
            assert!(text::line_width(&l) <= w, "w={w}: {}", text::line_text(&l));
        }
        let l = text::line_text(&sidebar_line(&e, 30, 0));
        assert!(l.starts_with("● very-long-ser"), "{l}");
        assert!(l.contains("conn"), "{l}");
    }
}
