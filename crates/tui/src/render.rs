//! Frame layout and drawing.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::app::{AcKind, App, Mode};
use crate::state::SessionView;
use crate::ui::chat::{self, Env};
use crate::ui::dialog::Dialog;
use crate::ui::footer::{self, StatusCtx};
use crate::ui::message;
use crate::ui::permission::PermissionView;
use crate::ui::text::{self, width};
use crate::ui::{split, theme, todo};

pub const SIDEBAR_W: u16 = 32;
/// Minimum terminal width at which the sidebar is shown.
pub const SIDEBAR_MIN_SCREEN: u16 = 140;
const EDITOR_MAX: usize = 10;

pub struct Layout {
    pub chat: Rect,
    pub prompt: Rect,
    pub editor: Rect,
    pub status: Rect,
    pub sidebar: Option<Rect>,
}

pub fn chat_layout(app: &App, area: Rect) -> Layout {
    let sidebar =
        (app.sidebar && area.width >= SIDEBAR_MIN_SCREEN && app.current.is_some()).then(|| {
            Rect::new(
                area.x + area.width - SIDEBAR_W,
                area.y,
                SIDEBAR_W,
                area.height,
            )
        });
    let main_w = area.width - sidebar.map(|s| s.width + 1).unwrap_or(0);
    let main = Rect::new(area.x, area.y, main_w, area.height);
    // Prompt box: 1 col margin each side; inner: bar(1) + pad(2) + text + pad(2).
    let px = main.x + 1;
    let pw = main.width.saturating_sub(2);
    let ew = pw.saturating_sub(5).max(1) as usize;
    let max_h = EDITOR_MAX.min((area.height as usize / 3).max(1));
    let eh = app.editor.height(ew).clamp(1, max_h) as u16;
    let prompt_h = eh + 4; // pad, editor, pad, meta, bottom edge
    let status_h = 1u16;
    let avail = main.height.saturating_sub(status_h);
    let prompt_h = prompt_h.min(avail.saturating_sub(3).max(3));
    let chat_h = avail.saturating_sub(prompt_h);
    let chat = Rect::new(main.x, main.y, main.width, chat_h);
    let prompt = Rect::new(px, main.y + chat_h, pw, prompt_h);
    let editor = Rect::new(
        px + 3,
        prompt.y + 1,
        ew as u16,
        eh.min(prompt_h.saturating_sub(3)).max(1),
    );
    let status = Rect::new(
        main.x,
        main.y + main.height.saturating_sub(1),
        main.width,
        1,
    );
    Layout {
        chat,
        prompt,
        editor,
        status,
        sidebar,
    }
}

pub fn draw(f: &mut Frame<'_>, app: &mut App) {
    let area = f.area();
    app.screen = area;
    let now = ref_proto::now_ms();
    let tick = app.tick();
    let mut cursor: Option<(u16, u16)> = None;
    let buf = f.buffer_mut();
    match app.mode {
        Mode::Chat => cursor = draw_chat_mode(buf, area, app, now, tick),
        Mode::Watch => cursor = draw_watch_mode(buf, area, app, now, tick).or(cursor),
    }
    // Overlays: permission modal > dialog.
    if let Some((sid, req, more)) = app.active_permission() {
        if app.perm.as_ref().is_none_or(|p| p.call_id != req.call_id) {
            app.perm = Some(PermissionView::new(&req.call_id));
        }
        let name =
            (app.mode == Mode::Watch || app.current.as_deref() != Some(sid.as_str())).then(|| {
                app.views
                    .get(&sid)
                    .map(|v| v.info.name.clone())
                    .unwrap_or_default()
            });
        if let Some(p) = app.perm.as_mut() {
            p.draw(buf, area, &req, name.as_deref(), more);
        }
        cursor = None;
    } else {
        app.perm = None;
        match app.dialog.as_mut() {
            Some(Dialog::List(d)) => {
                d.draw(buf, area, tick);
                cursor = Some(d.cursor());
            }
            Some(Dialog::Help(h)) => {
                h.draw(buf, area);
                cursor = None;
            }
            Some(Dialog::Mcp(d)) => {
                d.draw(buf, area, &app.mcp, tick, app.mcp_loaded);
                cursor = None;
            }
            None => {}
        }
    }
    // Disconnected banner (top, full width) after we had a connection.
    if !app.connected && app.ever_connected && area.height > 4 {
        let msg = format!(" {} Disconnected — reconnecting… ", theme::spinner(tick));
        let r = Rect::new(area.x, area.y, area.width, 1);
        fill(buf, r, Style::new().bg(theme::ERROR));
        let w = width(&msg) as u16;
        buf.set_string(
            area.x + area.width.saturating_sub(w) / 2,
            area.y,
            msg,
            Style::new()
                .fg(theme::PANEL)
                .bg(theme::ERROR)
                .add_modifier(Modifier::BOLD),
        );
    }
    app.toasts.draw(buf, area);
    if let Some((x, y)) = cursor {
        f.set_cursor_position((x, y));
    }
}

fn env<'a>(app: &'a App, now: i64, tick: u64) -> Env<'a> {
    Env {
        agents: app
            .config
            .as_ref()
            .map(|c| c.agents.as_slice())
            .unwrap_or(&[]),
        models: app
            .config
            .as_ref()
            .map(|c| c.models.as_slice())
            .unwrap_or(&[]),
        details: app.details,
        thinking: app.thinking,
        tick,
        now,
    }
}

fn draw_chat_mode(
    buf: &mut Buffer,
    area: Rect,
    app: &mut App,
    now: i64,
    tick: u64,
) -> Option<(u16, u16)> {
    let lay = chat_layout(app, area);
    // Transcript.
    let cur = app.current.clone();
    match cur
        .as_ref()
        .and_then(|c| app.views.contains_key(c).then_some(c.clone()))
    {
        Some(c) => {
            let mut v = app.views.remove(&c).expect("present");
            {
                let e = env(app, now, tick);
                chat::draw(buf, lay.chat, &mut v, &e);
            }
            app.views.insert(c, v);
        }
        None => draw_center(buf, lay.chat, &connect_text(app)),
    }
    // Sidebar.
    if let Some(sb) = lay.sidebar {
        draw_sidebar(buf, sb, app, now, tick);
    } else {
        app.sidebar_rows.clear();
        app.sidebar_mcp_rows.clear();
        app.sidebar_rect = Rect::default();
    }
    // Prompt box.
    let focused = app.dialog.is_none() && app.perm.is_none();
    let cursor = draw_prompt(buf, &lay, app, focused);
    // Autocomplete popup above the prompt.
    draw_autocomplete(buf, &lay, app);
    // Status line.
    let view = app.current.as_ref().and_then(|c| app.views.get(c));
    let hint = app.hint.as_ref().map(|h| (h.0.clone(), h.1));
    let sc = StatusCtx {
        view,
        cwd: view
            .map(|v| v.info.cwd.as_str())
            .filter(|c| !c.is_empty())
            .unwrap_or(&app.cwd),
        tick,
        hint,
        leader: app.leader.is_some(),
        shell_mode: app.shell_mode,
        connected: app.connected || !app.ever_connected,
        watch_mode: false,
        watch_count: 0,
        watch_focused: false,
        page: None,
    };
    footer::draw_status(buf, lay.status, &sc);
    cursor
}

fn connect_text(app: &App) -> Vec<Line<'static>> {
    let mut l = vec![Line::from(Span::styled(
        format!("{} connecting…", theme::spinner(app.tick())),
        theme::muted(),
    ))];
    if let Some(s) = &app.net_status {
        l.push(Line::from(Span::styled(s.clone(), theme::fg(theme::DIM))));
    }
    l
}

fn draw_center(buf: &mut Buffer, area: Rect, lines: &[Line<'static>]) {
    let h = lines.len() as u16;
    let y0 = area.y + area.height.saturating_sub(h) / 2;
    for (i, l) in lines.iter().enumerate() {
        let y = y0 + i as u16;
        if y >= area.y + area.height {
            break;
        }
        let w = text::line_width(l) as u16;
        buf.set_line(area.x + area.width.saturating_sub(w) / 2, y, l, area.width);
    }
}

fn fill(buf: &mut Buffer, r: Rect, style: Style) {
    for y in r.y..r.y + r.height {
        for x in r.x..r.x + r.width {
            if let Some(c) = buf.cell_mut((x, y)) {
                c.set_symbol(" ");
                c.set_style(style);
            }
        }
    }
}

fn draw_prompt(buf: &mut Buffer, lay: &Layout, app: &mut App, focused: bool) -> Option<(u16, u16)> {
    let p = lay.prompt;
    if p.height < 3 || p.width < 6 {
        return None;
    }
    let bg = theme::PANEL;
    let view = app.cur();
    let agent = view.map(|v| v.info.agent.clone()).unwrap_or_default();
    let busy = view.is_some_and(|v| v.is_busy());
    let view_model = view
        .map(|v| v.info.model.clone())
        .filter(|m| !m.model.is_empty());
    let queued = view.map(|v| v.queued.len()).unwrap_or(0);
    let todos = view.map(|v| v.info.todos.clone()).unwrap_or_default();
    let bar_color = if app.shell_mode {
        theme::WARNING
    } else {
        app.agent_color(&agent)
    };
    // Body (all rows but the bottom edge).
    let body = Rect::new(p.x, p.y, p.width, p.height - 1);
    fill(buf, body, Style::new().bg(bg));
    for y in body.y..body.y + body.height {
        buf.set_string(p.x, y, "┃", Style::new().fg(bar_color).bg(bg));
    }
    // Bottom edge: ╹ + ▀ in panel colour (gives a soft rounded bottom like opencode).
    let by = p.y + p.height - 1;
    buf.set_string(p.x, by, "╹", Style::new().fg(bar_color));
    for x in p.x + 1..p.x + p.width {
        buf.set_string(x, by, "▀", Style::new().fg(bg));
    }
    // Editor.
    let placeholder = if app.shell_mode {
        "Run a shell command…"
    } else if !app.connected && app.ever_connected {
        "Disconnected — reconnecting…"
    } else if busy {
        "Type to queue a message…  (esc twice to interrupt)"
    } else {
        "Ask anything…  (/ commands · @ files · ! shell · ctrl+v image)"
    };
    let cursor = app.editor.draw(buf, lay.editor, placeholder, focused);
    // Metadata row.
    let my = p.y + p.height - 2;
    let (model, provider, variant) = match view_model {
        Some(m) => (
            app.model_display(&m),
            app.provider_display(&m),
            m.variant.clone(),
        ),
        None => (String::new(), String::new(), None),
    };
    let mw = p.width.saturating_sub(5) as usize;
    let mut meta = footer::meta_line(
        if agent.is_empty() { "…" } else { &agent },
        app.agent_color(&agent),
        &model,
        &provider,
        variant.as_deref(),
        app.shell_mode,
        mw,
    );
    // Right side: attachments count / queued count.
    let mut right: Vec<Span<'static>> = Vec::new();
    let n_img = app.editor.images.len();
    if n_img > 0 {
        right.push(Span::styled(
            format!("{n_img} image{}", if n_img > 1 { "s" } else { "" }),
            Style::new().fg(theme::ACCENT).bg(bg),
        ));
    }
    if queued > 0 {
        let q = queued;
        if !right.is_empty() {
            right.push(Span::styled(" · ", Style::new().fg(theme::MUTED).bg(bg)));
        }
        right.push(Span::styled(
            format!("{q} queued"),
            Style::new().fg(theme::MUTED).bg(bg),
        ));
    }
    // Todo progress (current item · done/total), at most ~half the row so the model stays visible.
    let used: usize = right.iter().map(|s| width(&s.content)).sum();
    let room = (mw / 2).saturating_sub(used + if used > 0 { 3 } else { 0 });
    if let Some(spans) = todo::progress_spans(&todos, room) {
        if !right.is_empty() {
            right.insert(0, Span::styled(" · ", Style::new().fg(theme::MUTED).bg(bg)));
        }
        for s in spans.into_iter().rev() {
            right.insert(0, s);
        }
    }
    if !right.is_empty() {
        let (l, _) = message::compose(meta.spans.clone(), right, mw);
        meta = l;
        for s in &mut meta.spans {
            if s.style.bg.is_none() {
                s.style = s.style.bg(bg);
            }
        }
    }
    buf.set_line(p.x + 3, my, &meta, mw as u16);
    cursor
}

fn draw_autocomplete(buf: &mut Buffer, lay: &Layout, app: &mut App) {
    let Some(ac) = app.ac.as_mut() else { return };
    if ac.items.is_empty() {
        return;
    }
    let max_h = 10usize.min(lay.chat.height.saturating_sub(1) as usize);
    let n = ac.items.len().min(max_h);
    if n == 0 {
        return;
    }
    let h = n as u16 + 2;
    let x = lay.prompt.x;
    let w = lay.prompt.width;
    let y = lay.prompt.y.saturating_sub(h);
    let r = Rect::new(x, y, w, h);
    ac.rect = r;
    crate::ui::dialog::clear_wide_edges(buf, r);
    let bg = theme::PANEL;
    fill(buf, r, Style::new().bg(bg));
    let bst = Style::new().fg(theme::BORDER).bg(bg);
    for xx in r.x..r.x + r.width {
        buf.set_string(xx, r.y, "─", bst);
    }
    let title = match ac.kind {
        AcKind::Slash => " commands ",
        AcKind::File => " files ",
    };
    buf.set_string(r.x + 2, r.y, title, Style::new().fg(theme::MUTED).bg(bg));
    let start = ac.sel.saturating_sub(n - 1);
    for (i, (_, label, desc)) in ac.items.iter().enumerate().skip(start).take(n) {
        let yy = r.y + 1 + (i - start) as u16;
        let sel = i == ac.sel;
        let rbg = if sel { theme::PRIMARY } else { bg };
        let fgc = if sel { theme::PANEL } else { theme::TEXT };
        fill(buf, Rect::new(r.x, yy, r.width, 1), Style::new().bg(rbg));
        let lw = (r.width as usize).saturating_sub(4);
        let label_w = width(label).min(lw);
        buf.set_stringn(
            r.x + 2,
            yy,
            label,
            lw,
            Style::new().fg(fgc).bg(rbg).add_modifier(if sel {
                Modifier::BOLD
            } else {
                Modifier::empty()
            }),
        );
        let dx = (label_w + 2).max(24.min(lw / 2));
        if !desc.is_empty() && dx + 4 < lw {
            buf.set_stringn(
                r.x + 2 + dx as u16,
                yy,
                text::truncate(desc, lw - dx),
                lw - dx,
                Style::new()
                    .fg(if sel { theme::PANEL } else { theme::MUTED })
                    .bg(rbg),
            );
        }
    }
}

/// Click target of a sidebar row.
#[derive(Clone)]
enum SbClick {
    Session(String),
    /// MCP server (empty = header: open the dialog without a selection).
    Mcp(String),
}

struct SbRow {
    spans: Vec<Span<'static>>,
    click: Option<SbClick>,
}

fn sb(spans: Vec<Span<'static>>) -> SbRow {
    SbRow { spans, click: None }
}

fn sb_blank() -> SbRow {
    sb(Vec::new())
}

/// A sidebar section below the session info: gap + header + items.
struct SbSection {
    header: Vec<Span<'static>>,
    header_click: Option<SbClick>,
    items: Vec<SbRow>,
}

impl SbSection {
    fn len(&self) -> usize {
        2 + self.items.len()
    }
    fn render(self) -> Vec<SbRow> {
        let mut out = vec![
            sb_blank(),
            SbRow {
                spans: self.header,
                click: self.header_click,
            },
        ];
        out.extend(self.items);
        out
    }
}

fn context_bar(c: ref_proto::ContextUsage, bw: usize) -> Vec<Span<'static>> {
    let fillw = ((c.used as f64 / c.max as f64).min(1.0) * bw as f64).round() as usize;
    let mark = |t: u64| -> Option<usize> {
        (t > 0).then(|| ((t as f64 / c.max as f64) * bw as f64) as usize)
    };
    let (rm, am) = (mark(c.recommend), mark(c.auto));
    let col = if c.auto > 0 && c.used >= c.auto {
        theme::ERROR
    } else if c.recommend > 0 && c.used >= c.recommend {
        theme::WARNING
    } else {
        theme::PRIMARY
    };
    (0..bw)
        .map(|i| {
            let ch = if i < fillw {
                "━"
            } else if Some(i) == rm || Some(i) == am {
                "┊"
            } else {
                "─"
            };
            Span::styled(
                ch,
                if i < fillw {
                    theme::fg(col)
                } else {
                    theme::fg(theme::BORDER)
                },
            )
        })
        .collect()
}

fn draw_sidebar(buf: &mut Buffer, r: Rect, app: &mut App, now: i64, tick: u64) {
    app.sidebar_rows.clear();
    app.sidebar_mcp_rows.clear();
    app.sidebar_rect = r;
    let Some(v) = app.cur() else { return };
    // Separator.
    for y in r.y..r.y + r.height {
        buf.set_string(r.x - 1, y, "│", theme::fg(theme::BORDER_SUBTLE));
    }
    let x = r.x + 1;
    let w = r.width.saturating_sub(2) as usize;
    if w < 4 || r.height < 2 {
        return;
    }
    let h = (r.height - 1) as usize; // top margin

    // --- Session info (full and compact forms).
    let title = if v.info.title.trim().is_empty() {
        "New session".to_string()
    } else {
        text::one_line(&v.info.title)
    };
    let title_rows: Vec<SbRow> =
        text::wrap_spans(&[Span::styled(title.clone(), theme::bold(theme::TEXT))], w)
            .into_iter()
            .take(3)
            .map(|l| sb(l.spans))
            .collect();
    let ac = theme::agent_color(app.agents(), &v.info.agent);
    let model = app.model_display(&v.info.model);
    let c = v.info.context;
    let mut ctx_spans = footer::context_spans(v);
    if ctx_spans.is_empty() {
        ctx_spans.push(Span::styled("—", theme::muted()));
    }
    let bar = (c.max > 0).then(|| context_bar(c, w));
    let mut full = title_rows;
    full.push(sb_blank());
    full.push(sb(vec![
        Span::styled("name  ", theme::muted()),
        Span::styled(v.info.name.clone(), theme::text()),
    ]));
    full.push(sb(vec![
        Span::styled("id    ", theme::muted()),
        Span::styled(v.info.id.clone(), theme::fg(theme::DIM)),
    ]));
    full.push(sb(vec![
        Span::styled("agent ", theme::muted()),
        Span::styled(v.info.agent.clone(), theme::fg(ac)),
    ]));
    let mut mspans = vec![
        Span::styled("model ", theme::muted()),
        Span::styled(model.clone(), theme::text()),
    ];
    if let Some(var) = &v.info.model.variant {
        mspans.push(Span::styled(format!(" {var}"), theme::fg(theme::WARNING)));
    }
    full.push(sb(mspans));
    full.push(sb_blank());
    full.push(sb(vec![Span::styled(
        "Context",
        theme::bold(theme::ACCENT),
    )]));
    full.push(sb(ctx_spans.clone()));
    if let Some(b) = &bar {
        full.push(sb(b.clone()));
    }
    let mut compact = vec![
        sb(vec![Span::styled(title, theme::bold(theme::TEXT))]),
        sb(vec![
            Span::styled(v.info.agent.clone(), theme::fg(ac)),
            Span::styled(" · ", theme::muted()),
            Span::styled(model, theme::text()),
        ]),
        sb(ctx_spans),
    ];
    if let Some(b) = bar {
        compact.push(sb(b));
    }

    // --- Sections (display order: MCP, Todo, Live; priority: Todo, MCP, Live).
    let todos = v.info.todos.clone();
    let todo_sec = (!todos.is_empty()).then(|| {
        let (done, total) = todo::counts(&todos);
        let header = vec![
            Span::styled("Todo", theme::bold(theme::ACCENT)),
            Span::styled(format!("  {done}/{total}"), theme::muted()),
        ];
        let items = todo::todo_rows(&todos, w, usize::MAX)
            .into_iter()
            .map(|r| sb(r.line.spans))
            .collect();
        SbSection {
            header,
            header_click: None,
            items,
        }
    });
    let mcp_sec = (!app.mcp.is_empty()).then(|| {
        let sum = crate::ui::mcp::summary(&app.mcp);
        let header = vec![
            Span::styled("MCP", theme::bold(theme::ACCENT)),
            Span::styled(format!("  {sum}"), theme::muted()),
        ];
        let items = app
            .mcp
            .iter()
            .map(|e| SbRow {
                spans: crate::ui::mcp::sidebar_line(e, w, tick).spans,
                click: Some(SbClick::Mcp(e.name.clone())),
            })
            .collect();
        SbSection {
            header,
            header_click: Some(SbClick::Mcp(String::new())),
            items,
        }
    });
    let mut live: Vec<_> = app.live.iter().filter(|s| s.live).cloned().collect();
    live.sort_by_key(|s| s.created_at);
    let live_sec = (!live.is_empty()).then(|| {
        let header = vec![
            Span::styled("Live sessions", theme::bold(theme::ACCENT)),
            Span::styled(format!("  {}", live.len()), theme::muted()),
        ];
        let cur_id = v.info.id.clone();
        let items = live
            .iter()
            .map(|s| {
                let me = s.id == cur_id;
                let dot = if !s.status.is_idle() {
                    theme::spinner(tick).to_string()
                } else {
                    "○".to_string()
                };
                let dc = if !s.status.is_idle() {
                    theme::PRIMARY
                } else {
                    theme::MUTED
                };
                let fgbg = if s.attached > 0 { "fg" } else { "bg" };
                let name_st = if me {
                    theme::bold(theme::PRIMARY)
                } else {
                    theme::text()
                };
                let left = vec![
                    Span::styled(format!("{dot} "), theme::fg(dc)),
                    Span::styled(s.name.clone(), name_st),
                    Span::styled(format!(" {fgbg}"), theme::fg(theme::DIM)),
                ];
                // Right-aligned: todo count, then the working time of the current run (same source
                // as the status line). `compose` truncates the name first, these stay whole.
                let todos = if me { &v.info.todos } else { &s.todos };
                let mut right: Vec<Span<'static>> =
                    crate::ui::todo::badge(todos).into_iter().collect();
                if let Some(ms) = crate::state::busy_ms(s, now) {
                    if !right.is_empty() {
                        right.push(Span::raw(" "));
                    }
                    right.push(Span::styled(text::fmt_elapsed(ms), theme::fg(theme::MUTED)));
                }
                SbRow {
                    spans: message::compose(left, right, w).0.spans,
                    click: Some(SbClick::Session(s.id.clone())),
                }
            })
            .collect();
        SbSection {
            header,
            header_click: None,
            items,
        }
    });

    // --- Layout: full session info when everything fits; otherwise the compact form, and the
    // whole list scrolls (wheel over the sidebar) with a thumb on the separator.
    let secs: Vec<SbSection> = [mcp_sec, todo_sec, live_sec]
        .into_iter()
        .flatten()
        .collect(); // display order
    let sec_len: usize = secs.iter().map(SbSection::len).sum();
    let mut rows = if full.len() + sec_len <= h {
        full
    } else {
        compact
    };
    for s in secs {
        rows.extend(s.render());
    }
    let max_scroll = rows.len().saturating_sub(h);
    app.sidebar_max_scroll = max_scroll;
    app.sidebar_scroll = app.sidebar_scroll.min(max_scroll);
    let scroll = app.sidebar_scroll;
    for (y, row) in (r.y + 1..r.y + r.height).zip(rows.into_iter().skip(scroll)) {
        let line = Line::from(message::truncate_spans(row.spans, w));
        buf.set_line(x, y, &line, w as u16);
        match row.click {
            Some(SbClick::Session(id)) => app.sidebar_rows.push((y, id)),
            Some(SbClick::Mcp(name)) => app.sidebar_mcp_rows.push((y, name)),
            None => {}
        }
    }
    // Scrollbar in the (otherwise empty) rightmost column.
    if max_scroll > 0 {
        let bar = Rect::new(r.x + r.width - 1, r.y + 1, 1, h as u16);
        chat::draw_scrollbar(
            buf,
            bar,
            h + max_scroll,
            scroll as f64,
            max_scroll as f64,
            ("┃", theme::BORDER_ACTIVE),
            Some(("│", theme::BORDER_SUBTLE)),
        );
    }
}

// ---------------------------------------------------------------------------
// Watch (split) mode
// ---------------------------------------------------------------------------

fn draw_watch_mode(
    buf: &mut Buffer,
    area: Rect,
    app: &mut App,
    now: i64,
    tick: u64,
) -> Option<(u16, u16)> {
    let status = Rect::new(
        area.x,
        area.y + area.height.saturating_sub(1),
        area.width,
        1,
    );
    let body = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(1));
    let ids = app.tile_ids();
    let per = split::per_page(body.width, body.height).max(1);
    let pages = ids.len().div_ceil(per).max(1);
    app.watch.pages = pages;
    if app.watch.page >= pages {
        app.watch.page = pages - 1;
    }
    // Keep the focused tile's page visible.
    if let Some(f) = &app.watch.focus
        && let Some(pos) = ids.iter().position(|x| x == f)
    {
        app.watch.page = pos / per;
    }
    let page_ids: Vec<String> = ids
        .iter()
        .skip(app.watch.page * per)
        .take(per)
        .cloned()
        .collect();
    let rects = split::layout(page_ids.len(), body);
    app.watch.tiles = page_ids
        .iter()
        .cloned()
        .zip(rects.iter().copied())
        .collect();
    let mut cursor = None;
    if page_ids.is_empty() {
        let lines = vec![
            Line::from(Span::styled("No background sessions", theme::muted())),
            Line::from(Span::styled(
                "sessions created by agents or detached from a frontend appear here live",
                theme::fg(theme::DIM),
            )),
            Line::default(),
            Line::from(Span::styled(
                if app.current.is_some() {
                    "ctrl+x w back to chat"
                } else {
                    "ctrl+x n new session · ctrl+x l sessions"
                },
                theme::fg(theme::DIM),
            )),
        ];
        draw_center(buf, body, &lines);
    }
    for (sid, r) in app.watch.tiles.clone() {
        let focused = app.watch.focus.as_deref() == Some(sid.as_str()) && app.dialog.is_none();
        let Some(mut v) = app.views.remove(&sid) else {
            // Not yet attached: draw an empty frame.
            if let Some(info) = app.live.iter().find(|s| s.id == sid).cloned() {
                let tmp = SessionView::new(info, false);
                let inner = split::draw_frame(buf, r, &tmp, focused, tick, app.agents());
                draw_center(
                    buf,
                    inner,
                    &[Line::from(Span::styled("loading…", theme::muted()))],
                );
            }
            continue;
        };
        let inner = split::draw_frame(buf, r, &v, focused, tick, app.agents());
        if inner.width > 2 && inner.height > 0 {
            let content = Rect::new(inner.x + 1, inner.y, inner.width - 2, inner.height);
            let e = env(app, now, tick);
            chat::draw_tile(buf, content, r.x + r.width - 1, &mut v, &e);
            if v.messages.is_empty() {
                draw_center(
                    buf,
                    content,
                    &[Line::from(Span::styled(
                        if v.loaded { "empty" } else { "loading…" },
                        theme::muted(),
                    ))],
                );
            }
        } else {
            v.chat.area = Rect::default();
        }
        if focused && r.height >= 5 {
            let ir = split::draw_input_sep(buf, r);
            let ed = app.watch.inputs.entry(sid.clone()).or_default();
            let ph = if v.is_busy() {
                "message (queued) · esc esc abort"
            } else {
                "message · enter on empty = zoom"
            };
            cursor = ed.draw(buf, ir, ph, app.perm.is_none());
        }
        app.views.insert(sid, v);
    }
    let hint = app.hint.as_ref().map(|h| (h.0.clone(), h.1));
    let sc = StatusCtx {
        view: None,
        cwd: &app.cwd,
        tick,
        hint,
        leader: app.leader.is_some(),
        shell_mode: false,
        connected: app.connected || !app.ever_connected,
        watch_mode: true,
        watch_count: ids.len(),
        watch_focused: app.watch.focus.is_some(),
        page: Some((app.watch.page, pages)),
    };
    footer::draw_status(buf, status, &sc);
    cursor
}
