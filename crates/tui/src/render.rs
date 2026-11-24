//! Frame layout and drawing.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::app::{App, Mode};
use crate::state::SessionView;
use crate::text::{self, width};
use crate::ui::chat::{self, Env};
use crate::ui::dialog::Dialog;
use crate::ui::editor::MIN_EDITOR_COLS;
use crate::ui::footer::{self, StatusCtx};
use crate::ui::message;
use crate::ui::permission::PermissionView;
use crate::ui::question::QuestionView;
use crate::ui::{split, theme, todo};

pub(crate) const SIDEBAR_W: u16 = 32;
/// Minimum terminal width at which the sidebar is shown.
pub const SIDEBAR_MIN_SCREEN: u16 = 140;
const EDITOR_MAX: usize = 10;
/// The prompt box leaves this many columns free on each side.
const PROMPT_MARGIN_X: u16 = 1;
/// Columns left of the editor text inside the prompt box: bar(1) + pad(2).
const PROMPT_TEXT_INSET: u16 = 3;
/// Columns taken by the bar and the paddings around the editor text.
const PROMPT_CHROME_COLS: u16 = 5;
/// The prompt box is drawn only when it is wider than its chrome.
const MIN_PROMPT_WIDTH: u16 = PROMPT_CHROME_COLS + 1;
/// Rows around the editor text: pad, pad, meta row, bottom edge.
const PROMPT_CHROME_ROWS: u16 = 4;
/// The prompt box never gets fewer rows than this (pad, editor, meta).
const MIN_PROMPT_ROWS: u16 = 3;
/// The disconnected banner needs a screen taller than this.
const BANNER_MIN_SCREEN_H: u16 = 4;
pub struct Layout {
    pub chat: Rect,
    pub prompt: Rect,
    pub editor: Rect,
    pub status: Rect,
    pub sidebar: Option<Rect>,
}

pub fn chat_layout(app: &App, area: Rect) -> Layout {
    let show_sidebar = app.sidebar
        && area.width >= SIDEBAR_MIN_SCREEN
        && (app.current.is_some()
            || (app.connected && (!app.live.is_empty() || !app.mcp.is_empty())));
    let sidebar = show_sidebar.then(|| {
        Rect::new(
            area.x + area.width - SIDEBAR_W,
            area.y,
            SIDEBAR_W,
            area.height,
        )
    });
    let main_w = area
        .width
        .saturating_sub(sidebar.map(|s| s.width + 1).unwrap_or(0));
    let main = Rect::new(area.x, area.y, main_w, area.height);
    // Prompt box: bar(1) + pad(2) + text + pad(2) inside the side margins.
    let px = main.x + PROMPT_MARGIN_X;
    let pw = main.width.saturating_sub(2 * PROMPT_MARGIN_X);
    let ew = (pw.saturating_sub(PROMPT_CHROME_COLS) as usize).max(MIN_EDITOR_COLS);
    let max_h = EDITOR_MAX.min((area.height as usize / 3).max(1));
    let eh = app.editor.height(ew).clamp(1, max_h) as u16;
    let prompt_h = eh + PROMPT_CHROME_ROWS;
    let status_h = 1u16;
    let avail = main.height.saturating_sub(status_h);
    let prompt_h = prompt_h.min(avail.saturating_sub(MIN_PROMPT_ROWS).max(MIN_PROMPT_ROWS));
    let chat_h = avail.saturating_sub(prompt_h);
    let chat = Rect::new(main.x, main.y, main.width, chat_h);
    let prompt = Rect::new(px, main.y + chat_h, pw, prompt_h);
    let editor = Rect::new(
        px + PROMPT_TEXT_INSET,
        prompt.y + 1,
        ew as u16,
        eh.min(prompt_h.saturating_sub(MIN_PROMPT_ROWS)).max(1),
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
    app.switch_backend_rects.clear();
    app.watch.filter_pill_rect = None;
    if area.width == 0 || area.height == 0 {
        return;
    }
    let tick = app.tick();
    let buf = f.buffer_mut();
    let banner_h = draw_top_notice(buf, area, app);
    let content_area = Rect::new(
        area.x,
        area.y + banner_h,
        area.width,
        area.height.saturating_sub(banner_h),
    );
    let cursor = match app.mode {
        Mode::Chat => draw_chat_mode(buf, content_area, app, now, tick),
        Mode::Watch => draw_watch_mode(buf, content_area, app, now, tick),
    };
    let cursor = draw_overlays(buf, area, app, tick, cursor);
    draw_disconnected_banner(buf, area, app, tick);
    app.toasts.draw(buf, area);
    if let Some((x, y)) = cursor
        && x >= area.x
        && x < area.x.saturating_add(area.width)
        && y >= area.y
        && y < area.y.saturating_add(area.height)
    {
        f.set_cursor_position((x, y));
    }
}

/// Draws the topmost overlay: permission modal > question modal > dialog.
/// Returns the cursor position, which an overlay replaces or hides.
fn draw_overlays(
    buf: &mut Buffer,
    area: Rect,
    app: &mut App,
    tick: u64,
    cursor: Option<(u16, u16)>,
) -> Option<(u16, u16)> {
    if let Some((sid, req, more)) = app.active_permission() {
        if app.perm.as_ref().is_none_or(|p| p.call_id != req.call_id) {
            app.perm = Some(PermissionView::new(&req.call_id));
        }
        let name = app.modal_origin_name(&sid);
        if let Some(p) = app.perm.as_mut() {
            p.draw(buf, area, &req, name.as_deref(), more);
        }
        return None;
    }
    if let Some((sid, req)) = app.active_question() {
        app.perm = None;
        if app
            .question
            .as_ref()
            .is_none_or(|q| q.call_id != req.call_id)
        {
            app.question = Some(QuestionView::new(&req));
        }
        let name = app.modal_origin_name(&sid);
        return app.question.as_mut().and_then(|q| {
            q.draw(buf, area, name.as_deref());
            q.cursor_at
        });
    }
    app.perm = None;
    app.question = None;
    match app.dialog.as_mut() {
        Some(Dialog::List(d)) => {
            d.draw(buf, area, tick);
            d.cursor()
        }
        Some(Dialog::Help(h)) => {
            h.draw(buf, area);
            None
        }
        Some(Dialog::Mcp(d)) => {
            d.draw(buf, area, &app.mcp, tick, app.mcp_loaded);
            None
        }
        Some(Dialog::Backend(d)) => {
            d.draw(buf, area);
            d.cursor()
        }
        Some(Dialog::Stash(d)) => {
            d.draw(buf, area, &app.groups);
            d.cursor_at
        }
        None => cursor,
    }
}

/// Persistent top banner for protocol version warnings (when connected to an older backend)
/// or a refused reconnect (after we were previously connected). Returns the rows consumed.
fn draw_top_notice(buf: &mut Buffer, area: Rect, app: &App) -> u16 {
    if area.height <= BANNER_MIN_SCREEN_H || area.width < 4 {
        return 0;
    }
    let (msg, bg) = if app.connected {
        let Some(p) = app.backend_too_old else {
            return 0;
        };
        (
            format!(
                "The backend speaks protocol {p} but this ref needs {}. Run ref-server --stop and start ref again.",
                ref_proto::PROTOCOL_VERSION
            ),
            theme::WARNING,
        )
    } else if app.ever_connected {
        let Some(reason) = &app.refused else {
            return 0;
        };
        (reason.clone(), theme::ERROR)
    } else {
        return 0;
    };
    let lines = text::wrap_spans(
        &[Span::raw(text::one_line(&msg))],
        area.width.saturating_sub(2) as usize,
    );
    let rows = (lines.len() as u16).min(area.height.saturating_sub(BANNER_MIN_SCREEN_H));
    for (i, line) in lines.into_iter().take(rows as usize).enumerate() {
        let y = area.y + i as u16;
        fill(
            buf,
            Rect::new(area.x, y, area.width, 1),
            Style::new().bg(bg),
        );
        let w = text::line_width(&line) as u16;
        let x = area.x + area.width.saturating_sub(w) / 2;
        let styled = Line::from(
            line.spans
                .into_iter()
                .map(|s| {
                    Span::styled(
                        s.content,
                        Style::new()
                            .fg(theme::PANEL)
                            .bg(bg)
                            .add_modifier(Modifier::BOLD),
                    )
                })
                .collect::<Vec<_>>(),
        );
        buf.set_line(x, y, &styled, area.width);
    }
    rows
}

/// Top full-width banner shown when the connection dropped after we had one.
fn draw_disconnected_banner(buf: &mut Buffer, area: Rect, app: &mut App, tick: u64) {
    if app.connected
        || !app.ever_connected
        || app.refused.is_some()
        || area.height <= BANNER_MIN_SCREEN_H
        || area.width < 4
    {
        return;
    }
    let msg = format!(" {} Disconnected — reconnecting… ", theme::spinner(tick));
    let btn = " Switch Backend… ";
    let btn_w = width(btn) as u16;
    let r = Rect::new(area.x, area.y, area.width, 1);
    fill(buf, r, Style::new().bg(theme::ERROR));
    let msg_w = width(&msg) as u16;
    let gap = if area.width >= msg_w + btn_w + 2 {
        2
    } else {
        0
    };
    let total_w = msg_w + gap + if gap > 0 { btn_w } else { 0 };
    let x = area.x + area.width.saturating_sub(total_w) / 2;
    let max_w = (area.x + area.width - x) as usize;
    buf.set_stringn(
        x,
        area.y,
        &msg,
        max_w,
        Style::new()
            .fg(theme::PANEL)
            .bg(theme::ERROR)
            .add_modifier(Modifier::BOLD),
    );
    if gap > 0 {
        let bx = x + msg_w + gap;
        buf.set_stringn(
            bx,
            area.y,
            btn,
            btn_w as usize,
            Style::new()
                .fg(theme::TEXT)
                .bg(theme::ELEMENT)
                .add_modifier(Modifier::BOLD),
        );
        app.switch_backend_rects
            .push(Rect::new(bx, area.y, btn_w, 1));
    }
}
fn env<'a>(app: &'a App, now: i64, tick: u64) -> Env<'a> {
    Env {
        agents: app.agents(),
        models: app.models(),
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
        None => draw_empty_chat(buf, lay.chat, app),
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
    if let Some(ac) = app.ac.as_mut() {
        crate::ui::autocomplete::draw(buf, lay.prompt, lay.chat.height, ac);
    }
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
        connected: app.connected || (!app.ever_connected && app.refused.is_none()),
        refused: app.refused.is_some(),
        watch_mode: false,
        watch_count: 0,
        watch_focused: false,
        page: None,
        watch_filter_pill: None,
    };
    let _ = footer::draw_status(buf, lay.status, &sc);
    cursor
}

fn draw_empty_chat(buf: &mut Buffer, area: Rect, app: &mut App) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let mut lines = if app.connected && app.refused.is_none() {
        let mut l = Vec::new();
        for row in crate::app::LOGO {
            let split = row
                .char_indices()
                .nth(crate::app::LOGO_PRIMARY_CHARS)
                .map(|x| x.0)
                .unwrap_or(row.len());
            let (a, b) = row.split_at(split);
            l.push(Line::from(vec![
                Span::styled(a.to_string(), theme::bold(theme::PRIMARY)),
                Span::styled(b.trim_end().to_string(), theme::muted()),
            ]));
        }
        l.push(Line::default());
        l.push(Line::from(vec![
            Span::styled(text::tilde(&app.cwd), theme::text()),
            Span::styled(text::SEPARATOR, theme::muted()),
            Span::styled(app.backend_summary(), theme::muted()),
        ]));
        l.push(Line::default());
        l.push(Line::from(Span::styled(
            "Type a message to start a session · /sessions · /agent · /model · /backend",
            theme::fg(theme::DIM),
        )));
        l
    } else {
        connect_text(app, area.width as usize)
    };
    let btn_label = " Switch Backend… ";
    let btn_w = width(btn_label) as u16;
    let show_btn = area.width >= btn_w + 2;
    if show_btn {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            btn_label,
            Style::new()
                .fg(theme::TEXT)
                .bg(theme::ELEMENT)
                .add_modifier(Modifier::BOLD),
        )));
    }
    let h = lines.len() as u16;
    let y0 = area.y + area.height.saturating_sub(h) / 2;
    draw_center(buf, area, &lines);
    if show_btn {
        let btn_y = y0 + h.saturating_sub(1);
        if btn_y >= area.y && btn_y < area.y + area.height {
            let bx = area.x + area.width.saturating_sub(btn_w) / 2;
            app.switch_backend_rects
                .push(Rect::new(bx, btn_y, btn_w, 1));
        }
    }
}

fn connect_text(app: &App, max_w: usize) -> Vec<Line<'static>> {
    if let Some(reason) = &app.refused {
        let mut l = vec![Line::from(Span::styled(
            "Disconnected",
            theme::bold(theme::ERROR),
        ))];
        for line in text::wrap_spans(
            &[Span::raw(text::one_line(reason))],
            max_w.saturating_sub(4).max(10),
        ) {
            l.push(Line::from(
                line.spans
                    .into_iter()
                    .map(|s| Span::styled(s.content, theme::fg(theme::TEXT)))
                    .collect::<Vec<_>>(),
            ));
        }
        return l;
    }
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
    if area.width == 0 || area.height == 0 {
        return;
    }
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
    if p.height < MIN_PROMPT_ROWS || p.width < MIN_PROMPT_WIDTH {
        return None;
    }
    let bg = theme::PANEL;
    let view = app.cur();
    let agent = view
        .map(|v| v.info.agent.clone())
        .unwrap_or_else(|| app.effective_draft_agent());
    let busy = view.is_some_and(|v| v.is_busy());
    let view_model = view
        .map(|v| v.info.model.clone())
        .filter(|m| !m.model.is_empty())
        .or_else(|| app.effective_draft_model());
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
    } else if !app.connected && app.refused.is_some() {
        "Disconnected"
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
    let mw = p.width.saturating_sub(PROMPT_CHROME_COLS) as usize;
    let mut meta = footer::meta_line(
        if agent.is_empty() {
            text::ELLIPSIS
        } else {
            &agent
        },
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
        if !right.is_empty() {
            right.push(Span::styled(
                text::SEPARATOR,
                Style::new().fg(theme::MUTED).bg(bg),
            ));
        }
        right.push(Span::styled(
            format!("{queued} queued"),
            Style::new().fg(theme::MUTED).bg(bg),
        ));
    }
    // Todo progress (current item · done/total), at most ~half the row so the model stays visible.
    let used: usize = text::spans_width(&right);
    let separator_w = if used > 0 { width(text::SEPARATOR) } else { 0 };
    let room = (mw / 2).saturating_sub(used + separator_w);
    if let Some(spans) = todo::progress_spans(&todos, room) {
        if !right.is_empty() {
            right.insert(
                0,
                Span::styled(text::SEPARATOR, Style::new().fg(theme::MUTED).bg(bg)),
            );
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
    buf.set_line(p.x + PROMPT_TEXT_INSET, my, &meta, mw as u16);
    cursor
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
    let v = app.cur();
    // Separator.
    if r.x > 0 {
        for y in r.y..r.y + r.height {
            buf.set_string(r.x - 1, y, "│", theme::fg(theme::BORDER_SUBTLE));
        }
    }
    let w = r.width.saturating_sub(2) as usize;
    if w < 4 || r.height < 2 {
        return;
    }
    let h = (r.height - 1) as usize; // top margin

    // Display order: MCP, Todo, Live (rows are dropped by priority: Todo, MCP, Live).
    let secs: Vec<SbSection> = [
        mcp_section(app, w, tick),
        v.and_then(|cur| todo_section(&cur.info.todos, w)),
        live_section(app, v, w, now, tick),
    ]
    .into_iter()
    .flatten()
    .collect();
    let mut rows = if let Some(cur) = v {
        let (full, compact) = session_info_rows(app, cur, w);
        // Full session info when everything fits; otherwise the compact form.
        let sec_len: usize = secs.iter().map(SbSection::len).sum();
        if full.len() + sec_len <= h {
            full
        } else {
            compact
        }
    } else {
        vec![sb(vec![Span::styled("No active session", theme::muted())])]
    };
    for s in secs {
        rows.extend(s.render());
    }
    paint_sidebar_rows(buf, r, app, rows);
}

/// The session info block in its full and compact forms.
fn session_info_rows(app: &App, v: &SessionView, w: usize) -> (Vec<SbRow>, Vec<SbRow>) {
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
            Span::styled(text::SEPARATOR, theme::muted()),
            Span::styled(model, theme::text()),
        ]),
        sb(ctx_spans),
    ];
    if let Some(b) = bar {
        compact.push(sb(b));
    }
    (full, compact)
}

fn todo_section(todos: &[ref_proto::Todo], w: usize) -> Option<SbSection> {
    if todos.is_empty() {
        return None;
    }
    let (done, total) = todo::counts(todos);
    let header = vec![
        Span::styled("Todo", theme::bold(theme::ACCENT)),
        Span::styled(format!("  {done}/{total}"), theme::muted()),
    ];
    let items = todo::todo_rows(todos, w, usize::MAX)
        .into_iter()
        .map(|r| sb(r.line.spans))
        .collect();
    Some(SbSection {
        header,
        header_click: None,
        items,
    })
}

fn mcp_section(app: &App, w: usize, tick: u64) -> Option<SbSection> {
    if app.mcp.is_empty() {
        return None;
    }
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
    Some(SbSection {
        header,
        header_click: Some(SbClick::Mcp(String::new())),
        items,
    })
}

fn live_section(
    app: &App,
    v: Option<&SessionView>,
    w: usize,
    now: i64,
    tick: u64,
) -> Option<SbSection> {
    let mut live: Vec<_> = app.live.iter().filter(|s| s.live).collect();
    if live.is_empty() {
        return None;
    }
    live.sort_by_key(|s| s.created_at);
    let header = vec![
        Span::styled("Live sessions", theme::bold(theme::ACCENT)),
        Span::styled(format!("  {}", live.len()), theme::muted()),
    ];
    let items = live
        .iter()
        .map(|s| {
            let me = v.is_some_and(|cur| s.id == cur.info.id);
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
            let todos = if let Some(cur) = v.filter(|_| me) {
                &cur.info.todos
            } else {
                &s.todos
            };
            let mut right: Vec<Span<'static>> = crate::ui::todo::badge(todos).into_iter().collect();
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
    Some(SbSection {
        header,
        header_click: None,
        items,
    })
}

/// Paints the rows below the top margin, records their click targets and draws the scrollbar.
/// The whole list scrolls (wheel over the sidebar) with a thumb in the rightmost column.
fn paint_sidebar_rows(buf: &mut Buffer, r: Rect, app: &mut App, rows: Vec<SbRow>) {
    let x = r.x + 1;
    let w = r.width.saturating_sub(2) as usize;
    let h = (r.height - 1) as usize;
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
    let mut ac_anchor: Option<Rect> = None;
    let mut focused_shell = false;
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
                let root = split::root_task(&tmp.info, &app.live, &app.history_list);
                let inner = split::draw_frame(
                    buf,
                    r,
                    &tmp,
                    focused,
                    tick,
                    app.agents(),
                    root.as_ref().map(|(id, name)| (id.as_str(), name.as_str())),
                );
                draw_center(
                    buf,
                    inner,
                    &[Line::from(Span::styled("loading…", theme::muted()))],
                );
            }
            continue;
        };
        let root = split::root_task(&v.info, &app.live, &app.history_list);
        let inner = split::draw_frame(
            buf,
            r,
            &v,
            focused,
            tick,
            app.agents(),
            root.as_ref().map(|(id, name)| (id.as_str(), name.as_str())),
        );
        if inner.width > 2 && inner.height > 0 {
            let content = Rect::new(inner.x + 1, inner.y, inner.width - 2, inner.height);
            let e = env(app, now, tick);
            chat::draw_tile(buf, content, r.x + r.width - 1, &mut v, &e);
            if v.is_empty() {
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
            let shell_mode = app.watch.shell_modes.contains(&sid);
            focused_shell = shell_mode;
            let ir = split::draw_input_sep(buf, r, shell_mode);
            let ed = app.watch.inputs.entry(sid.clone()).or_default();
            let ph = if shell_mode {
                "Run a shell command…"
            } else if v.is_busy() {
                "message (queued) · esc esc abort"
            } else {
                "message · enter on empty = zoom"
            };
            let tile_focused = app.perm.is_none() && app.question.is_none() && app.dialog.is_none();
            cursor = ed.draw_single_line(buf, ir, None, ph, tile_focused);
            if ir.width > 0 {
                ac_anchor = Some(Rect::new(
                    r.x + 1,
                    ir.y.saturating_sub(1),
                    r.width.saturating_sub(2),
                    1,
                ));
            }
        }
        app.views.insert(sid, v);
    }
    if let Some(ac) = app.ac.as_mut() {
        if let Some(anchor) = ac_anchor {
            let avail_h = anchor.y.saturating_sub(body.y);
            crate::ui::autocomplete::draw(buf, anchor, avail_h, ac);
        } else {
            ac.rect = Rect::default();
        }
    }
    let hint = app.hint.as_ref().map(|h| (h.0.clone(), h.1));
    let sc = StatusCtx {
        view: None,
        cwd: &app.cwd,
        tick,
        hint,
        leader: app.leader.is_some(),
        shell_mode: focused_shell,
        connected: app.connected || (!app.ever_connected && app.refused.is_none()),
        refused: app.refused.is_some(),
        watch_mode: true,
        watch_count: ids.len(),
        watch_focused: app.watch.focus.is_some(),
        page: Some((app.watch.page, pages)),
        watch_filter_pill: Some(app.watch_filter_pill()),
    };
    app.watch.filter_pill_rect = footer::draw_status(buf, status, &sc);
    cursor
}
