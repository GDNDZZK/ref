//! Status line under the prompt and the prompt metadata row.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use super::message::compose;
use super::text;
use super::theme;
use crate::state::SessionView;

pub struct StatusCtx<'a> {
    pub view: Option<&'a SessionView>,
    pub cwd: &'a str,
    pub tick: u64,
    /// Transient hint replacing the middle section (e.g. "esc again to interrupt").
    pub hint: Option<(String, Color)>,
    pub leader: bool,
    pub shell_mode: bool,
    pub connected: bool,
    pub watch_mode: bool,
    pub watch_count: usize,
    /// Split view: a tile is focused.
    pub watch_focused: bool,
    pub page: Option<(usize, usize)>,
}

pub fn status_label(v: &SessionView, tick: u64, now: i64) -> (String, Color) {
    use ref_proto::SessionStatus::*;
    let el = v.busy_ms(now).map(text::fmt_elapsed).unwrap_or_default();
    match &v.info.status {
        Idle => (String::new(), theme::MUTED),
        Busy => (
            format!("{} working {el}", theme::spinner(tick)),
            theme::PRIMARY,
        ),
        Running => (
            format!("{} running tools {el}", theme::spinner(tick)),
            theme::PRIMARY,
        ),
        WaitingPermission => ("△ waiting for permission".into(), theme::WARNING),
        WaitingQuestion => ("? waiting for your answer".into(), theme::WARNING),
        Retrying {
            next_at, attempt, ..
        } => {
            let s = ((next_at - now).max(0) + 999) / 1000;
            (
                format!("{} retrying in {s}s (#{attempt})", theme::spinner(tick)),
                theme::WARNING,
            )
        }
        Compacting => (
            format!("{} compacting", theme::spinner(tick)),
            theme::ACCENT,
        ),
    }
}

pub fn context_spans(v: &SessionView) -> Vec<Span<'static>> {
    let c = v.info.context;
    if c.max == 0 && c.used == 0 {
        return Vec::new();
    }
    let pct = (c.used * 100).checked_div(c.max).unwrap_or(0);
    let color = if c.auto > 0 && c.used >= c.auto {
        theme::ERROR
    } else if c.recommend > 0 && c.used >= c.recommend {
        theme::WARNING
    } else if pct >= 60 {
        theme::TEXT
    } else {
        theme::MUTED
    };
    let max = if c.max > 0 {
        format!("/{}", text::fmt_tokens(c.max))
    } else {
        String::new()
    };
    vec![Span::styled(
        format!("{}{max} ({pct}%)", text::fmt_tokens(c.used)),
        theme::fg(color),
    )]
}

pub fn draw_status(buf: &mut Buffer, area: Rect, cx: &StatusCtx<'_>) {
    if area.height == 0 {
        return;
    }
    let now = ref_proto::now_ms();
    let mut left: Vec<Span<'static>> = vec![Span::raw(" ")];
    if !cx.connected {
        left.push(Span::styled(
            "● disconnected — reconnecting…",
            theme::fg(theme::ERROR),
        ));
    } else if cx.watch_mode {
        left.push(Span::styled(
            "◫ background sessions",
            theme::bold(theme::PRIMARY),
        ));
        left.push(Span::styled(
            format!(" · {}", cx.watch_count),
            theme::muted(),
        ));
        if let Some((p, n)) = cx.page
            && n > 1
        {
            left.push(Span::styled(
                format!(" · page {}/{n}", p + 1),
                theme::muted(),
            ));
        }
    } else {
        left.push(Span::styled(text::tilde(cx.cwd), theme::muted()));
        if let Some(v) = cx.view {
            left.push(Span::styled("  ", theme::muted()));
            left.push(Span::styled(v.info.name.clone(), theme::text()));
            let (s, c) = status_label(v, cx.tick, now);
            if !s.is_empty() {
                left.push(Span::styled(format!("  {s}"), theme::fg(c)));
            }
        }
    }
    // Hint section.
    if cx.leader {
        left.push(Span::styled("   ctrl+x ", theme::bold(theme::PRIMARY)));
        left.push(Span::styled(
            "n new · l sessions · k close · m models · a agents · w watch · h help",
            theme::muted(),
        ));
    } else if let Some((h, c)) = &cx.hint {
        left.push(Span::styled(
            format!("   {h}"),
            theme::fg(*c).add_modifier(Modifier::BOLD),
        ));
    } else if cx.shell_mode {
        left.push(Span::styled("   esc exit shell mode", theme::muted()));
    } else if cx.watch_mode && cx.watch_count > 0 && !cx.watch_focused {
        left.push(Span::styled(
            "   click or tab to focus a session · wheel scrolls a tile",
            theme::fg(theme::DIM),
        ));
    } else if cx.watch_mode && cx.watch_count > 0 {
        left.push(Span::styled(
            "   enter send · pgup/pgdn scroll · esc unfocus · esc esc abort · ctrl+o zoom",
            theme::fg(theme::DIM),
        ));
    } else if cx.view.is_some_and(|v| v.is_busy()) {
        left.push(Span::styled("   esc ", theme::text()));
        left.push(Span::styled("interrupt", theme::muted()));
    }
    let mut right: Vec<Span<'static>> = Vec::new();
    if let Some(v) = cx.view.filter(|_| !cx.watch_mode) {
        right.extend(context_spans(v));
        if !right.is_empty() {
            right.push(Span::raw("  "));
        }
    }
    right.push(Span::styled("ctrl+p ", theme::text()));
    right.push(Span::styled("commands ", theme::muted()));
    let (line, _) = compose(left, right, area.width as usize);
    buf.set_line(area.x, area.y, &line, area.width);
}

/// Prompt metadata row: `Agent · Model provider · variant`.
pub fn meta_line(
    agent: &str,
    agent_color: Color,
    model: &str,
    provider: &str,
    variant: Option<&str>,
    shell: bool,
    w: usize,
) -> Line<'static> {
    let bg = theme::PANEL;
    if shell {
        return Line::from(vec![Span::styled(
            "Shell",
            Style::new()
                .fg(theme::WARNING)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        )]);
    }
    let mut spans = vec![Span::styled(
        titlecase(agent),
        Style::new()
            .fg(agent_color)
            .bg(bg)
            .add_modifier(Modifier::BOLD),
    )];
    if !model.is_empty() {
        spans.push(Span::styled(" · ", Style::new().fg(theme::MUTED).bg(bg)));
        spans.push(Span::styled(
            model.to_string(),
            Style::new().fg(theme::TEXT).bg(bg),
        ));
        if !provider.is_empty() {
            spans.push(Span::styled(
                format!(" {provider}"),
                Style::new().fg(theme::MUTED).bg(bg),
            ));
        }
    }
    if let Some(v) = variant {
        spans.push(Span::styled(" · ", Style::new().fg(theme::MUTED).bg(bg)));
        spans.push(Span::styled(
            v.to_string(),
            Style::new()
                .fg(theme::WARNING)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        ));
    }
    Line::from(super::message::truncate_spans(spans, w))
}

fn titlecase(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}
