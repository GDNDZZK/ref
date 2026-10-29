//! Top-level screen layouts: normal session view (+ sidebar) and monitor grid.

use crate::app::{status_color, App, Mode, UiHit, ESC_WINDOW};
use crate::catalog::title_case;
use crate::render::grid;
use crate::render::util::{fit_spans, left_right, spans_width, with_bg};
use crate::text::{self, fmt_tokens, tilde};
use crate::theme;
use crate::ui::{self, fill, put};
use hn_proto::SessionInfo;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::Frame;

pub const SIDEBAR_W: u16 = 42;

pub fn draw(app: &mut App, f: &mut Frame) {
    let area = f.area();
    app.hits.clear();
    let buf = f.buffer_mut();
    fill(buf, area, theme::BG);
    let cursor = match app.mode {
        Mode::Normal => draw_normal(app, area, buf),
        Mode::Monitor => draw_monitor(app, area, buf),
    };
    if let Some(d) = app.dialog.as_mut() {
        d.draw(area, buf);
    }
    ui::draw_toasts(app, area, buf);
    if app.dialog.is_none() {
        if let Some((x, y)) = cursor {
            if x < area.right() && y < area.bottom() {
                f.set_cursor_position((x, y));
            }
        }
    }
}

fn sidebar_visible(app: &App, w: u16) -> bool {
    match app.sidebar {
        Some(v) => v && w >= 80,
        None => w > 120,
    }
}

/// The session is running a turn (per the backend's status; a tool delay or permission
/// prompt only exists while it is Busy / Permission). Drives the "interrupt" hints.
fn busy(app: &App, sid: &str) -> bool {
    app.session_info(sid).is_some_and(|i| i.status.is_active())
}

// ---------------------------------------------------------------------------
// Normal mode
// ---------------------------------------------------------------------------

fn draw_normal(app: &mut App, area: Rect, buf: &mut ratatui::buffer::Buffer) -> Option<(u16, u16)> {
    let side = sidebar_visible(app, area.width);
    app.sidebar_shown = side;
    let main = if side { Rect { width: area.width - SIDEBAR_W, ..area } } else { area };
    if side {
        let sr = Rect { x: main.right(), width: SIDEBAR_W, ..area };
        draw_sidebar(app, sr, buf);
    }
    let sid = app.current.clone();
    // Column with horizontal padding.
    let col = Rect { x: main.x + 2, width: main.width.saturating_sub(3), ..main };
    let footer_y = col.bottom().saturating_sub(1);
    let mut bottom = footer_y;
    // Permission panel replaces the prompt box while pending (opencode style).
    let perm = sid
        .as_ref()
        .and_then(|id| app.sessions.get(id))
        .and_then(|s| s.pending_permission().map(|(mid, _, tp)| (mid.to_string(), tp.clone())));
    let mut prompt_r = None;
    let perm_r = match &perm {
        Some((_, tp)) => {
            let max = (col.height / 2).max(9);
            let h = ui::perm_height(tp, col.width, max, false);
            let r = Rect { x: col.x, y: bottom.saturating_sub(h + 1), width: col.width, height: h };
            bottom = r.y;
            Some(r)
        }
        None => {
            let prompt_h = ui::prompt_height(&app.editor, col.width).min(col.height.saturating_sub(3).max(3));
            let r = Rect { x: col.x, y: bottom.saturating_sub(prompt_h), width: col.width, height: prompt_h };
            bottom = r.y;
            prompt_r = Some(r);
            None
        }
    };
    // Status lines (retry / delayed / connection)
    let mut status = sid.as_deref().map(|id| ui::status_lines(app, id, col.width as usize - 2)).unwrap_or_default();
    if let Some(l) = ui::conn_line(app) {
        status.insert(0, l);
    }
    let status_h = status.len() as u16;
    let status_y = bottom.saturating_sub(status_h + if status_h > 0 { 1 } else { 0 });
    if status_h > 0 {
        for (i, l) in status.iter().enumerate() {
            put(buf, col.x + 1, status_y + i as u16, l, col.width - 1);
        }
        bottom = status_y;
    }
    // The last column of the message area is the scrollbar; keep one blank column before the sidebar.
    let msgs_w = if side { col.width } else { col.width + 1 };
    let msgs_r = Rect { x: col.x, y: col.y, width: msgs_w, height: bottom.saturating_sub(col.y + 1) };
    match &sid {
        Some(id) if app.sessions.contains_key(id) => ui::draw_messages(app, id, msgs_r, buf, false),
        _ => draw_welcome(app, msgs_r, buf),
    }
    if let (Some(r), Some((mid, tp))) = (perm_r, perm.as_ref()) {
        ui::draw_permission(app, sid.as_deref().unwrap_or(""), mid, tp, r, buf, false);
    }
    let mut cursor = None;
    if let Some(prompt_r) = prompt_r {
        cursor = ui::draw_prompt(app, sid.as_deref(), prompt_r, buf);
        // Autocomplete above the prompt
        let items = ui::autocomplete(app, &app.editor);
        if !items.is_empty() {
            if app.ac_sel >= items.len() {
                app.ac_sel = items.len() - 1;
            }
            ui::draw_autocomplete(app, &items, prompt_r, buf);
        }
    }
    draw_footer(app, sid.as_deref(), Rect { x: col.x, y: footer_y, width: col.width, height: 1 }, buf);
    cursor
}

fn draw_welcome(app: &App, r: Rect, buf: &mut ratatui::buffer::Buffer) {
    let logo = [
        "█ █ █▀▀ █▀█ █▄ █ █▀▀ █▀▀ █▀▀",
        "█▀█ █▀▀ █▀▄ █ ▀█ █▀▀ █▀▀ ▀▀█",
        "▀ ▀ ▀▀▀ ▀ ▀ ▀  ▀ ▀▀▀ ▀▀▀ ▀▀▀",
    ];
    let y0 = r.y + r.height.saturating_sub(6) / 2;
    for (i, l) in logo.iter().enumerate() {
        let w = text::width(l) as u16;
        let x = r.x + r.width.saturating_sub(w) / 2;
        let color = if i == 0 { theme::TEXT } else { theme::MUTED };
        put(buf, x, y0 + i as u16, &Line::from(Span::styled(*l, theme::fg(color))), w);
    }
    let wait = app.creating_since.filter(|_| app.creating).map(|t| t.elapsed().as_secs()).unwrap_or(0);
    let msg = match &app.conn {
        crate::client::ConnState::Connected if wait >= 3 => format!("Starting session… {wait}s (backend is still loading)"),
        crate::client::ConnState::Connected => "Starting session…".to_string(),
        _ => "Connecting to backend…".to_string(),
    };
    let msg = msg.as_str();
    let w = text::width(msg) as u16;
    put(buf, r.x + r.width.saturating_sub(w) / 2, y0 + 4, &Line::from(Span::styled(msg, theme::muted())), w);
}

fn draw_footer(app: &mut App, sid: Option<&str>, r: Rect, buf: &mut ratatui::buffer::Buffer) {
    let now = hn_proto::now_ms();
    let mut left: Vec<Span<'static>> = Vec::new();
    let is_busy = sid.is_some_and(|id| busy(app, id));
    let armed = app.esc_armed.as_ref().is_some_and(|(s, at)| Some(s.as_str()) == sid && at.elapsed() < ESC_WINDOW);
    if let Some(at) = app.leader {
        let _ = at;
        left.push(Span::styled("ctrl+x", theme::bold(theme::PRIMARY)));
        left.push(Span::styled("  n new · l sessions · o open · m models · a agents · c compact · u undo · e editor · h help", theme::muted()));
    } else if app.ctrlc_armed.is_some() {
        left.push(Span::styled("ctrl+c again to exit", theme::fg(theme::WARNING)));
    } else if is_busy {
        let ac = sid.and_then(|id| app.session_info(id)).map(|i| app.catalog.agent_color(&i.agent)).unwrap_or(theme::PRIMARY);
        left.extend(ui::scanner(now, 8, ac));
        left.push(Span::raw("  "));
        if armed {
            left.push(Span::styled("esc again to interrupt", theme::fg(theme::WARNING)));
        } else {
            left.push(Span::styled("esc", theme::text()));
            left.push(Span::styled(" interrupt", theme::muted()));
        }
    } else {
        left.push(Span::styled(tilde(&app.cwd), theme::muted()));
    }
    let mut right: Vec<Span<'static>> = Vec::new();
    if !app.mouse_capture {
        right.push(Span::styled(" SELECT ", Style::new().fg(theme::BG).bg(theme::WARNING).add_modifier(Modifier::BOLD)));
        right.push(Span::styled(" F2", theme::text()));
        right.push(Span::styled(" mouse   ", theme::muted()));
    }
    let has_sel = sid.and_then(|id| app.views.get(id)).and_then(|v| v.selection).is_some_and(|s| !s.is_empty());
    if has_sel {
        right.push(Span::styled("ctrl+y", theme::text()));
        right.push(Span::styled(" copy selection   ", theme::muted()));
    }
    if let Some(info) = sid.and_then(|id| app.session_info(id)) {
        if info.context.max > 0 && !sidebar_visible(app, r.width + 3 + SIDEBAR_W) {
            let pct = info.context.used * 100 / info.context.max.max(1);
            let c = if info.context.compact_at > 0 && info.context.used >= info.context.compact_at {
                theme::WARNING
            } else {
                theme::MUTED
            };
            right.push(Span::styled(format!("{pct}% ctx   "), theme::fg(c)));
        }
    }
    right.push(Span::styled("tab", theme::text()));
    right.push(Span::styled(" agents  ", theme::muted()));
    right.push(Span::styled("ctrl+p", theme::text()));
    right.push(Span::styled(" commands", theme::muted()));
    let w = r.width.saturating_sub(1);
    let (l, _) = left_right(left, right, w as usize, Style::new());
    put(buf, r.x + 1, r.y, &l, w);
}

fn draw_sidebar(app: &mut App, r: Rect, buf: &mut ratatui::buffer::Buffer) {
    fill(buf, r, theme::PANEL);
    let x = r.x + 2;
    let w = r.width.saturating_sub(4) as usize;
    let mut y = r.y + 1;
    let info = app.current.as_deref().and_then(|id| app.session_info(id)).cloned();
    let line = |buf: &mut ratatui::buffer::Buffer, y: &mut u16, spans: Vec<Span<'static>>| {
        if *y < r.bottom().saturating_sub(1) {
            put(buf, x, *y, &Line::from(fit_spans(spans, w)), w as u16);
            *y += 1;
        }
    };
    if let Some(i) = &info {
        let name = if i.name.is_empty() { "New session".to_string() } else { i.name.clone() };
        for wl in text::wrap_segments(&[(name, theme::bold(theme::TEXT))], w).into_iter().take(3) {
            line(buf, &mut y, wl.spans);
        }
        y += 1;
        let ac = app.catalog.agent_color(&i.agent);
        line(buf, &mut y, vec![Span::styled("Agent", theme::bold(theme::TEXT))]);
        line(buf, &mut y, vec![Span::styled("■ ", theme::fg(ac)), Span::styled(title_case(&i.agent), theme::fg(ac))]);
        y += 1;
        line(buf, &mut y, vec![Span::styled("Model", theme::bold(theme::TEXT))]);
        let mut ms = vec![Span::styled(app.catalog.model_name(&i.model), theme::text())];
        if let Some(v) = &i.model.variant {
            ms.push(Span::styled(format!(" · {v}"), theme::bold(theme::WARNING)));
        }
        line(buf, &mut y, ms);
        line(buf, &mut y, vec![Span::styled(app.catalog.provider_name(&i.model), theme::muted())]);
        y += 1;
        line(buf, &mut y, vec![Span::styled("Context", theme::bold(theme::TEXT))]);
        let c = &i.context;
        let pct = (c.used * 100).checked_div(c.max).unwrap_or(0) as usize;
        line(
            buf,
            &mut y,
            vec![
                Span::styled(format!("{} tokens", fmt_tokens(c.used)), theme::text()),
                Span::styled(if c.max > 0 { format!(" / {}", fmt_tokens(c.max)) } else { String::new() }, theme::muted()),
            ],
        );
        if c.max > 0 {
            let bw = w.min(30);
            let filled = (pct.min(100) * bw).div_ceil(100);
            let compact_mark = if c.compact_at > 0 { (c.compact_at * bw as u64 / c.max) as usize } else { usize::MAX };
            let color = if c.compact_at > 0 && c.used >= c.compact_at {
                theme::WARNING
            } else if pct >= 90 {
                theme::ERROR
            } else {
                theme::SUCCESS
            };
            let mut bar: Vec<Span<'static>> = (0..bw)
                .map(|i| {
                    if i < filled {
                        Span::styled("━", theme::fg(color))
                    } else if i == compact_mark {
                        Span::styled("┊", theme::fg(theme::WARNING))
                    } else {
                        Span::styled("─", theme::fg(theme::BORDER_SUBTLE))
                    }
                })
                .collect();
            bar.push(Span::styled(format!(" {pct}%"), theme::muted()));
            line(buf, &mut y, bar);
        }
        line(buf, &mut y, vec![Span::styled(format!("${:.2} spent", i.cost), theme::muted())]);
        y += 1;
        line(buf, &mut y, vec![Span::styled("Directory", theme::bold(theme::TEXT))]);
        for wl in text::wrap_segments(&[(tilde(&i.cwd), theme::muted())], w).into_iter().take(2) {
            line(buf, &mut y, wl.spans);
        }
        y += 1;
    }
    // Open sessions
    let mut others: Vec<SessionInfo> = app.open.iter().filter(|s| s.open).cloned().collect();
    let cur = app.current.clone();
    others.sort_by_key(|s| {
        (cur.as_deref() != Some(s.id.as_str()), !s.is_foreground(), s.parent.is_some(), std::cmp::Reverse(s.created_at))
    });
    if !others.is_empty() && y + 2 < r.bottom() {
        line(
            buf,
            &mut y,
            vec![
                Span::styled("Open sessions", theme::bold(theme::TEXT)),
                Span::styled(format!("  {}", others.len()), theme::muted()),
            ],
        );
        let now = hn_proto::now_ms();
        for s in &others {
            if y + 2 >= r.bottom() {
                break;
            }
            let me = app.current.as_deref() == Some(s.id.as_str());
            let (dot, c) = if s.status.is_active() {
                (theme::spinner(now).to_string(), status_color(&s.status))
            } else if s.is_foreground() {
                ("●".into(), theme::SUCCESS)
            } else {
                ("◌".into(), theme::INFO)
            };
            let name = if s.name.is_empty() { "New session".into() } else { s.name.clone() };
            let tag = if me { " (this)" } else if s.is_foreground() { " fg" } else { " bg" };
            let st = if me { theme::bold(theme::PRIMARY) } else { theme::text() };
            let spans = vec![
                Span::styled(format!("{dot} "), theme::fg(c)),
                Span::styled(name, st),
                Span::styled(tag, theme::faint()),
            ];
            if !me {
                app.hits.push((Rect::new(x, y, w as u16, 1), UiHit::SidebarSession(s.id.clone())));
            }
            line(buf, &mut y, spans);
        }
    }
    // Bottom: backend + version
    let by = r.bottom().saturating_sub(2);
    let v = vec![
        Span::styled("hernees ", theme::bold(theme::TEXT)),
        Span::styled(env!("CARGO_PKG_VERSION"), theme::muted()),
    ];
    put(buf, x, by, &Line::from(v), w as u16);
    let url = app.url.trim_start_matches("ws://").trim_end_matches('/').to_string();
    put(buf, x, by + 1, &Line::from(Span::styled(text::truncate(&url, w), theme::faint())), w as u16);
}

// ---------------------------------------------------------------------------
// Monitor mode
// ---------------------------------------------------------------------------

fn draw_monitor(app: &mut App, area: Rect, buf: &mut ratatui::buffer::Buffer) -> Option<(u16, u16)> {
    let panes = app.panes();
    let body = Rect { y: area.y + 1, height: area.height.saturating_sub(2), ..area };
    let cap = grid::capacity(body).max(1);
    let pages = panes.len().div_ceil(cap).max(1);
    // Keep the focused pane on the current page.
    if let Some(f) = &app.focus_pane {
        if let Some(i) = panes.iter().position(|p| p == f) {
            app.page = i / cap;
        }
    }
    app.page = app.page.min(pages - 1);
    // Header
    let now = hn_proto::now_ms();
    let busy_n = panes.iter().filter(|p| busy(app, p)).count();
    let mut left = vec![
        Span::styled(" hernees", theme::bold(theme::PRIMARY)),
        Span::styled(" · background monitor", theme::muted()),
        Span::styled(format!(" · {} session{}", panes.len(), if panes.len() == 1 { "" } else { "s" }), theme::text()),
    ];
    if busy_n > 0 {
        left.push(Span::styled(format!(" · {} {busy_n} busy", theme::spinner(now)), theme::fg(theme::PRIMARY)));
    }
    if let Some(l) = ui::conn_line(app) {
        left.push(Span::raw("  "));
        left.extend(l.spans);
    }
    let mut right = Vec::new();
    if pages > 1 {
        right.push(Span::styled(format!("page {}/{} ", app.page + 1, pages), theme::bold(theme::TEXT)));
        right.push(Span::styled("ctrl+x [ ]  ", theme::muted()));
    }
    let (hl, _) = left_right(left, right, area.width as usize, Style::new());
    put(buf, area.x, area.y, &with_bg(hl, area.width as usize, theme::PANEL), area.width);

    let mut cursor = None;
    if panes.is_empty() {
        let msgs = ["No background sessions are open.", "Panes appear here automatically as sessions start in the background."];
        for (i, m) in msgs.iter().enumerate() {
            let w = text::width(m) as u16;
            let st = if i == 0 { theme::text() } else { theme::muted() };
            put(buf, body.x + body.width.saturating_sub(w) / 2, body.y + body.height / 2 + i as u16, &Line::from(Span::styled(*m, st)), w);
        }
    } else if app.zoom && app.focus_pane.is_some() {
        let id = app.focus_pane.clone().unwrap();
        cursor = draw_pane(app, &id, 0, body, buf, true);
    } else {
        let page: Vec<String> = panes.iter().skip(app.page * cap).take(cap).cloned().collect();
        let rects = grid::layout(page.len(), body);
        for (i, (id, r)) in page.iter().zip(rects).enumerate() {
            let focused = app.focus_pane.as_deref() == Some(id.as_str());
            let c = draw_pane(app, id, app.page * cap + i + 1, r, buf, focused);
            if focused {
                cursor = c;
            }
        }
    }
    // Footer
    let fy = area.bottom().saturating_sub(1);
    let sid = app.focus_pane.clone();
    let mut left = Vec::new();
    if app.leader.is_some() {
        left.push(Span::styled("ctrl+x", theme::bold(theme::PRIMARY)));
        left.push(Span::styled("  1-9 focus · arrows move · z zoom · [ ] page · o open · m models · q quit", theme::muted()));
    } else if app.ctrlc_armed.is_some() {
        left.push(Span::styled("ctrl+c again to exit", theme::fg(theme::WARNING)));
    } else if let Some(id) = &sid {
        let is_busy = busy(app, id);
        let armed = app.esc_armed.as_ref().is_some_and(|(s, at)| s == id && at.elapsed() < ESC_WINDOW);
        if armed && is_busy {
            left.push(Span::styled("esc again to interrupt", theme::fg(theme::WARNING)));
        } else if is_busy {
            left.push(Span::styled("esc esc", theme::text()));
            left.push(Span::styled(" interrupt focused", theme::muted()));
        } else {
            left.push(Span::styled("enter", theme::text()));
            left.push(Span::styled(" send to focused pane", theme::muted()));
        }
    }
    let mut right = Vec::new();
    if !app.mouse_capture {
        right.push(Span::styled(" SELECT ", Style::new().fg(theme::BG).bg(theme::WARNING).add_modifier(Modifier::BOLD)));
        right.push(Span::raw("  "));
    }
    for (k, a) in [("alt+←→↑↓", " focus  "), ("ctrl+x z", " zoom  "), ("ctrl+p", " commands")] {
        right.push(Span::styled(k, theme::text()));
        right.push(Span::styled(a, theme::muted()));
    }
    let (l, _) = left_right(left, right, area.width.saturating_sub(2) as usize, Style::new());
    put(buf, area.x + 1, fy, &l, area.width.saturating_sub(2));
    cursor
}

fn draw_pane(
    app: &mut App,
    id: &str,
    number: usize,
    r: Rect,
    buf: &mut ratatui::buffer::Buffer,
    focused: bool,
) -> Option<(u16, u16)> {
    if r.width < 10 || r.height < 4 {
        return None;
    }
    let now = hn_proto::now_ms();
    let info = app.session_info(id).cloned().unwrap_or_default();
    let border = if focused { theme::PRIMARY } else { theme::BORDER_SUBTLE };
    // Box border
    let bst = theme::fg(border);
    for x in r.x..r.right() {
        for (y, ch) in [(r.y, "─"), (r.bottom() - 1, "─")] {
            if let Some(c) = buf.cell_mut((x, y)) {
                c.set_symbol(ch);
                c.set_style(bst);
            }
        }
    }
    for y in r.y..r.bottom() {
        for (x, ch) in [(r.x, "│"), (r.right() - 1, "│")] {
            if let Some(c) = buf.cell_mut((x, y)) {
                c.set_symbol(ch);
                c.set_style(bst);
            }
        }
    }
    for (x, y, ch) in [(r.x, r.y, "╭"), (r.right() - 1, r.y, "╮"), (r.x, r.bottom() - 1, "╰"), (r.right() - 1, r.bottom() - 1, "╯")] {
        if let Some(c) = buf.cell_mut((x, y)) {
            c.set_symbol(ch);
            c.set_style(bst);
        }
    }
    app.hits.push((r, UiHit::Pane(id.to_string())));
    // Title on the top border
    let ac = app.catalog.agent_color(&info.agent);
    let (dot, dc) = if info.status.is_active() {
        (theme::spinner(now).to_string(), status_color(&info.status))
    } else {
        ("●".to_string(), theme::MUTED)
    };
    let name = if info.name.is_empty() { "New session".to_string() } else { info.name.clone() };
    let mut title = vec![Span::raw(" ")];
    if number > 0 && number <= 9 {
        title.push(Span::styled(format!("{number} "), theme::faint()));
    }
    title.push(Span::styled(format!("{dot} "), theme::fg(dc)));
    title.push(Span::styled(name, if focused { theme::bold(theme::TEXT) } else { theme::text() }));
    title.push(Span::raw(" "));
    let mut meta = vec![
        Span::raw(" "),
        Span::styled(title_case(&info.agent), theme::fg(ac)),
        Span::styled(" · ", theme::faint()),
        Span::styled(app.catalog.model_name(&info.model), theme::muted()),
    ];
    if info.context.max > 0 {
        let pct = info.context.used * 100 / info.context.max.max(1);
        let c = if info.context.compact_at > 0 && info.context.used >= info.context.compact_at { theme::WARNING } else { theme::MUTED };
        meta.push(Span::styled(format!(" · {pct}%"), theme::fg(c)));
    }
    meta.push(Span::raw(" "));
    let inner_w = r.width.saturating_sub(4) as usize;
    let mw = spans_width(&meta);
    let meta = if mw + 12 > inner_w { Vec::new() } else { meta };
    let mw = spans_width(&meta);
    let title = fit_spans(title, inner_w.saturating_sub(mw + 1));
    put(buf, r.x + 2, r.y, &Line::from(title), inner_w as u16);
    if mw > 0 {
        put(buf, r.right() - 2 - mw as u16, r.y, &Line::from(meta), mw as u16);
    }
    // Inner layout
    let inner = Rect { x: r.x + 1, y: r.y + 1, width: r.width - 2, height: r.height - 2 };
    let input_y = inner.bottom() - 1;
    let mut bottom = input_y;
    // separator above input
    let sep_y = bottom.saturating_sub(1);
    for x in inner.x..inner.right() {
        if let Some(c) = buf.cell_mut((x, sep_y)) {
            c.set_symbol("─");
            c.set_fg(theme::BORDER_SUBTLE);
        }
    }
    bottom = sep_y;
    let perm = app.sessions.get(id).and_then(|s| s.pending_permission().map(|(m, _, tp)| (m.to_string(), tp.clone())));
    let mut perm_r = None;
    if let Some((_, tp)) = &perm {
        let max = (inner.height / 2).max(5);
        let h = ui::perm_height(tp, inner.width, max, true);
        let pr = Rect { x: inner.x, y: bottom.saturating_sub(h), width: inner.width, height: h };
        bottom = pr.y;
        perm_r = Some(pr);
    }
    let status = ui::status_lines(app, id, inner.width as usize - 2);
    let sh = status.len() as u16;
    if sh > 0 && bottom > inner.y + sh {
        let sy = bottom - sh;
        for (i, l) in status.iter().enumerate() {
            put(buf, inner.x + 1, sy + i as u16, l, inner.width - 1);
        }
        bottom = sy;
    }
    let msgs_r = Rect { x: inner.x, y: inner.y, width: inner.width, height: bottom.saturating_sub(inner.y) };
    if app.sessions.contains_key(id) {
        ui::draw_messages(app, id, msgs_r, buf, true);
    }
    if let (Some(pr), Some((mid, tp))) = (perm_r, perm.as_ref()) {
        ui::draw_permission(app, id, mid, tp, pr, buf, true);
    }
    // One-line input
    let prompt_st = if focused { theme::fg(theme::PRIMARY) } else { theme::faint() };
    put(buf, inner.x + 1, input_y, &Line::from(Span::styled("›", prompt_st)), 1);
    let iw = inner.width.saturating_sub(4) as usize;
    let ed = app.pane_editors.entry(id.to_string()).or_default();
    let mut cursor = None;
    if ed.is_empty() {
        let ph = if focused { "Send a message to this session…" } else { "" };
        put(buf, inner.x + 3, input_y, &Line::from(Span::styled(ph, theme::faint())), iw as u16);
        cursor = Some((inner.x + 3, input_y));
    } else {
        let t = text::one_line(ed.text());
        let before = text::one_line(&ed.text()[..ed.cursor()]);
        let cw = text::width(&before);
        let shown = if cw >= iw { text::tail_fit(&before, iw.saturating_sub(1)) } else { text::truncate(&t, iw) };
        let cx = if cw >= iw { text::width(&shown) } else { cw };
        put(buf, inner.x + 3, input_y, &Line::from(Span::styled(shown, theme::text())), iw as u16);
        cursor = cursor.or(Some((inner.x + 3 + cx as u16, input_y)));
    }
    if perm.is_some() {
        None
    } else {
        cursor
    }
}

/// Pane order for keyboard navigation: returns rects of the current page.
pub fn pane_rects(app: &App, area: Rect) -> Vec<(String, Rect)> {
    let panes = app.panes();
    let body = Rect { y: area.y + 1, height: area.height.saturating_sub(2), ..area };
    let cap = grid::capacity(body).max(1);
    let page: Vec<String> = panes.iter().skip(app.page * cap).take(cap).cloned().collect();
    let rects = grid::layout(page.len(), body);
    page.into_iter().zip(rects).collect()
}

pub fn pane_capacity(area: Rect) -> usize {
    let body = Rect { y: area.y + 1, height: area.height.saturating_sub(2), ..area };
    grid::capacity(body).max(1)
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::StartOpts;
    use crate::store::SessionState;
    use hn_proto::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn monitor_app(status: SessionStatus, stale_delayed_tool: bool) -> App {
        let client = crate::client::detached();
        let mut app = App::new(client, "/tmp".into(), "ws://x".into(), Mode::Monitor, StartOpts::default());
        let info = SessionInfo { id: "ses_a".into(), name: "a".into(), open: true, status, ..Default::default() };
        app.open = vec![info.clone()];
        let mut st = SessionState::new(info.clone());
        let mut msgs = Vec::new();
        if stale_delayed_tool {
            msgs.push(Message {
                id: "m1".into(),
                role: Role::Assistant,
                origin: Origin::Human,
                time: 1,
                completed: Some(2),
                parts: vec![Part::Tool(ToolPart { call_id: "c".into(), name: "bash".into(), state: ToolState::Delayed, ..Default::default() })],
                model: None,
                agent: None,
                usage: None,
                finish: Some("tool_use".into()),
                error: None,
                replay_key: None,
            });
        }
        st.apply(Event::Snapshot { info, messages: msgs });
        app.sessions.insert("ses_a".into(), st);
        app.focus_pane = Some("ses_a".into());
        app
    }

    fn footer(app: &mut App) -> String {
        let mut t = Terminal::new(TestBackend::new(120, 20)).unwrap();
        t.draw(|f| draw(app, f)).unwrap();
        let buf = t.backend().buffer();
        (0..buf.area.width).map(|x| buf[(x, buf.area.height - 1)].symbol().to_string()).collect()
    }

    #[test]
    fn monitor_footer_interrupt_hint_only_when_busy() {
        let mut idle = monitor_app(SessionStatus::Idle, true);
        let f = footer(&mut idle);
        assert!(!f.contains("interrupt"), "{f}");
        assert!(f.contains("send to focused pane"), "{f}");
        // A stale armed esc on an idle session shows nothing either.
        idle.esc_armed = Some(("ses_a".into(), std::time::Instant::now()));
        assert!(!footer(&mut idle).contains("interrupt"));

        let mut busy = monitor_app(SessionStatus::Busy, false);
        assert!(footer(&mut busy).contains("esc esc interrupt focused"));
        busy.esc_armed = Some(("ses_a".into(), std::time::Instant::now()));
        assert!(footer(&mut busy).contains("esc again to interrupt"));
        let mut perm = monitor_app(SessionStatus::Permission, false);
        assert!(footer(&mut perm).contains("esc esc interrupt focused"));
    }
}
