//! Todo list rendering (the `todo` tool input and `SessionInfo.todos`).

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ref_proto::{Todo, TodoStatus};
use serde_json::Value;

use super::{Row, text, theme};

/// Parses the `todos` array of a `todo` tool input (lenient: bad entries are skipped).
pub fn parse_todos(input: &Value) -> Vec<Todo> {
    let Some(items) = input.get("todos").and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|it| {
            let content = it
                .get("content")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|c| !c.is_empty())?;
            let status = match it
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("pending")
            {
                "in_progress" => TodoStatus::InProgress,
                "completed" => TodoStatus::Completed,
                "cancelled" => TodoStatus::Cancelled,
                _ => TodoStatus::Pending,
            };
            Some(Todo {
                content: content.to_string(),
                status,
            })
        })
        .collect()
}

pub fn icon(s: TodoStatus) -> &'static str {
    match s {
        TodoStatus::Pending => "☐",
        TodoStatus::InProgress => "◐",
        TodoStatus::Completed => "☑",
        TodoStatus::Cancelled => "☒",
    }
}

pub fn style(s: TodoStatus) -> (Style, Style) {
    match s {
        TodoStatus::Pending => (theme::fg(theme::MUTED), theme::text()),
        TodoStatus::InProgress => (theme::bold(theme::PRIMARY), theme::bold(theme::PRIMARY)),
        TodoStatus::Completed => (
            theme::muted(),
            theme::muted().add_modifier(Modifier::CROSSED_OUT),
        ),
        TodoStatus::Cancelled => (
            theme::fg(theme::DIM),
            theme::fg(theme::DIM).add_modifier(Modifier::CROSSED_OUT),
        ),
    }
}

/// One row per item (wrapped content is indented under the text), at most `max` items.
pub fn todo_rows(todos: &[Todo], w: usize, max: usize) -> Vec<Row> {
    let mut rows = Vec::new();
    let tw = w.saturating_sub(2).max(4);
    for t in todos.iter().take(max) {
        let (ist, cst) = style(t.status);
        let content = text::one_line(&t.content);
        for (i, (l, sp)) in text::wrap_spans_flags(&[Span::styled(content, cst)], tw)
            .into_iter()
            .enumerate()
        {
            let lead = if i == 0 {
                Span::styled(format!("{} ", icon(t.status)), ist)
            } else {
                Span::raw("  ")
            };
            rows.push(Row::new(l).soft_sp(i > 0, sp).indent(vec![lead]));
        }
    }
    if todos.len() > max {
        rows.push(
            Row::new(Line::from(Span::styled(
                format!("… {} more", todos.len() - max),
                theme::muted(),
            )))
            .decor(),
        );
    }
    rows
}

/// `(done, total)` where done counts completed items (cancelled are excluded from both).
pub fn counts(todos: &[Todo]) -> (usize, usize) {
    let total = todos
        .iter()
        .filter(|t| t.status != TodoStatus::Cancelled)
        .count();
    let done = todos
        .iter()
        .filter(|t| t.status == TodoStatus::Completed)
        .count();
    (done, total)
}

/// Whether there is still work left (any pending / in-progress item).
pub fn active(todos: &[Todo]) -> bool {
    todos
        .iter()
        .any(|t| matches!(t.status, TodoStatus::Pending | TodoStatus::InProgress))
}

/// The item to feature in compact indicators: in-progress first, else the next pending one.
pub fn current(todos: &[Todo]) -> Option<&Todo> {
    todos
        .iter()
        .find(|t| t.status == TodoStatus::InProgress)
        .or_else(|| todos.iter().find(|t| t.status == TodoStatus::Pending))
}

/// Compact one-line progress: `◐ Fix bug · 2/5` truncated to `w` columns. None when nothing is left.
pub fn progress_spans(todos: &[Todo], w: usize) -> Option<Vec<Span<'static>>> {
    if !active(todos) || w < 8 {
        return None;
    }
    let cur = current(todos)?;
    let (done, total) = counts(todos);
    let tail = format!(" · {done}/{total}");
    let ic = format!("{} ", icon(cur.status));
    let room = w.saturating_sub(text::width(&ic) + text::width(&tail));
    let (ist, _) = style(cur.status);
    let cst = if cur.status == TodoStatus::InProgress {
        theme::text()
    } else {
        theme::muted()
    };
    Some(vec![
        Span::styled(ic, ist),
        Span::styled(text::truncate(&text::one_line(&cur.content), room), cst),
        Span::styled(tail, theme::muted()),
    ])
}

/// Count for a Live sessions row: `2/5` (primary while an item is in progress, muted when only
/// pending ones are left), `✓3/3` in green once everything is done. None without countable items.
pub fn badge(todos: &[Todo]) -> Option<Span<'static>> {
    let (done, total) = counts(todos);
    if total == 0 {
        return None;
    }
    Some(if !active(todos) {
        Span::styled(format!("✓{done}/{total}"), theme::fg(theme::SUCCESS))
    } else if todos.iter().any(|t| t.status == TodoStatus::InProgress) {
        Span::styled(format!("{done}/{total}"), theme::fg(theme::PRIMARY))
    } else {
        Span::styled(format!("{done}/{total}"), theme::fg(theme::MUTED))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn todos() -> Vec<Todo> {
        parse_todos(&json!({"todos": [
            {"content": "Read the parser", "status": "completed"},
            {"content": "Fix the bug", "status": "in_progress"},
            {"content": "Run tests", "status": "pending"},
            {"content": "Old idea", "status": "cancelled"},
            {"content": "", "status": "pending"},
            {"content": "Weird", "status": "bogus"}
        ]}))
    }

    #[test]
    fn parse_and_counts() {
        let t = todos();
        assert_eq!(t.len(), 5, "empty content skipped");
        assert_eq!(t[4].status, TodoStatus::Pending, "unknown status → pending");
        assert_eq!(counts(&t), (1, 4));
        assert_eq!(current(&t).unwrap().content, "Fix the bug");
        assert!(parse_todos(&json!({})).is_empty());
    }

    #[test]
    fn rows_and_progress() {
        let t = todos();
        let rows = todo_rows(&t, 40, usize::MAX);
        let texts: Vec<String> = rows.iter().map(|r| text::line_text(&r.line)).collect();
        assert_eq!(texts[0], "☑ Read the parser");
        assert_eq!(texts[1], "◐ Fix the bug");
        assert_eq!(texts[2], "☐ Run tests");
        assert_eq!(texts[3], "☒ Old idea");
        let done = rows[0].line.spans.last().unwrap().style;
        assert!(done.add_modifier.contains(Modifier::CROSSED_OUT));
        let ip = rows[1].line.spans.last().unwrap().style;
        assert!(ip.add_modifier.contains(Modifier::BOLD) && ip.fg == Some(theme::PRIMARY));
        let capped = todo_rows(&t, 40, 2);
        assert_eq!(text::line_text(&capped.last().unwrap().line), "… 3 more");
        let p: String = progress_spans(&t, 80)
            .unwrap()
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert_eq!(p, "◐ Fix the bug · 1/4");
        let p: String = progress_spans(&t, 14)
            .unwrap()
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert!(text::width(&p) <= 14, "{p}");
        let all_done = parse_todos(&json!({"todos": [{"content": "x", "status": "completed"}]}));
        assert!(progress_spans(&all_done, 80).is_none());
        // CJK content wraps within the width.
        let cjk = parse_todos(
            &json!({"todos": [{"content": "修复解析器中的边界错误并运行全部测试用例", "status": "pending"}]}),
        );
        for r in todo_rows(&cjk, 16, 10) {
            assert!(text::line_width(&r.line) <= 16);
        }
    }

    #[test]
    fn badge_states() {
        let t = |items: &[(&str, &str)]| {
            parse_todos(
                &json!({"todos": items.iter().map(|(c, s)| json!({"content": c, "status": s})).collect::<Vec<_>>()}),
            )
        };
        let b = badge(&t(&[
            ("a", "completed"),
            ("b", "in_progress"),
            ("c", "pending"),
            ("d", "cancelled"),
        ]))
        .unwrap();
        assert_eq!(
            (b.content.as_ref(), b.style.fg),
            ("1/3", Some(theme::PRIMARY))
        );
        let b = badge(&t(&[("a", "completed"), ("b", "pending")])).unwrap();
        assert_eq!(
            (b.content.as_ref(), b.style.fg),
            ("1/2", Some(theme::MUTED))
        );
        let b = badge(&t(&[
            ("a", "completed"),
            ("b", "completed"),
            ("c", "cancelled"),
        ]))
        .unwrap();
        assert_eq!(
            (b.content.as_ref(), b.style.fg),
            ("✓2/2", Some(theme::SUCCESS))
        );
        assert!(badge(&[]).is_none());
        assert!(badge(&t(&[("a", "cancelled")])).is_none());
    }
}
