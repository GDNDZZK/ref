//! Question dialog for the `question` tool: one or several questions with options, multiple
//! choice, an inline custom answer and a page that confirms the answers.
//!
//! A single-choice question alone is a plain list; otherwise a tab bar leads from question to
//! question and ends on a "Confirm" tab.

use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ref_proto::{Question, QuestionRequest};

use super::dialog::frame;
use super::editor::Editor;
use super::text::{self, width};
use super::theme;

pub const CUSTOM_LABEL: &str = "Type your own answer";
const CONFIRM_LABEL: &str = "Confirm";
const NOT_ANSWERED: &str = "(not answered)";
const DEFAULT_TAG: &str = " (default)";
const TICK: &str = " ✓";
const POINTER: &str = "› ";
const NO_POINTER: &str = "  ";
const OPEN_SECS: f32 = 0.12;
const MIN_WIDTH: u16 = 30;
const MAX_WIDTH: u16 = 90;
/// Borders, the blank row and the hint row around the body.
const CHROME_ROWS: usize = 4;
const MAX_TAB_LABEL: usize = 18;

/// What a key did to the dialog.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// Nothing to send; the dialog is redrawn.
    Open,
    /// The answers, one list per question (empty: not answered).
    Submit(Vec<Vec<String>>),
    Dismiss,
}

#[derive(Default)]
struct Answer {
    /// Cursor row: the options, then the custom row when the question has one.
    cursor: usize,
    /// Picked options (single choice: at most one).
    picked: Vec<bool>,
    /// The typed answer.
    custom: Option<String>,
}

pub struct QuestionView {
    pub call_id: String,
    pub opened: Instant,
    /// The reply is on its way; keys are ignored until the backend answers.
    pub submitting: bool,
    /// Why the backend refused the answers.
    pub error: Option<String>,
    pub rect: Rect,
    pub(super) questions: Vec<Question>,
    answers: Vec<Answer>,
    /// Current tab; `questions.len()` is the confirm tab.
    pub(super) tab: usize,
    /// The custom answer being typed.
    pub edit: Option<Editor>,
    /// Row hit zones of the last draw: (y, row).
    pub(super) row_hits: Vec<(u16, usize)>,
    /// Tab hit zones of the last draw: (x0, x1, tab) on `tab_y`.
    pub(super) tab_hits: Vec<(u16, u16, usize)>,
    pub(super) tab_y: u16,
    /// Where the terminal cursor goes while typing.
    pub cursor_at: Option<(u16, u16)>,
}

impl QuestionView {
    pub fn new(request: &QuestionRequest) -> Self {
        let answers = request
            .questions
            .iter()
            .map(|q| Answer {
                cursor: if q.multiple {
                    0
                } else {
                    q.options.iter().position(|o| o.default).unwrap_or(0)
                },
                picked: q.options.iter().map(|o| q.multiple && o.default).collect(),
                custom: None,
            })
            .collect();
        QuestionView {
            call_id: request.call_id.clone(),
            opened: Instant::now(),
            submitting: false,
            error: None,
            rect: Rect::default(),
            questions: request.questions.clone(),
            answers,
            tab: 0,
            edit: None,
            row_hits: Vec::new(),
            tab_hits: Vec::new(),
            tab_y: 0,
            cursor_at: None,
        }
    }

    /// One single-choice question: no tabs, choosing submits.
    pub fn is_simple(&self) -> bool {
        self.questions.len() == 1 && !self.questions[0].multiple
    }

    pub(super) fn on_confirm_tab(&self) -> bool {
        !self.is_simple() && self.tab >= self.questions.len()
    }

    /// Option rows plus the custom row.
    pub(super) fn row_count(&self, q: usize) -> usize {
        let q = &self.questions[q];
        q.options.len() + usize::from(q.custom)
    }

    pub(super) fn cursor(&self, q: usize) -> usize {
        self.answers[q].cursor
    }

    pub(super) fn is_picked(&self, q: usize, option: usize) -> bool {
        self.answers[q].picked[option]
    }

    pub(super) fn custom_text(&self, q: usize) -> Option<&str> {
        self.answers[q].custom.as_deref()
    }

    pub(super) fn is_answered(&self, q: usize) -> bool {
        let a = &self.answers[q];
        a.custom.is_some() || a.picked.iter().any(|p| *p)
    }

    /// The labels and typed text of question `q`.
    pub(super) fn answer_of(&self, q: usize) -> Vec<String> {
        let (question, a) = (&self.questions[q], &self.answers[q]);
        let mut out: Vec<String> = question
            .options
            .iter()
            .zip(&a.picked)
            .filter(|(_, picked)| **picked)
            .map(|(o, _)| o.label.clone())
            .collect();
        out.extend(a.custom.clone());
        out
    }

    fn all_answers(&self) -> Vec<Vec<String>> {
        (0..self.questions.len())
            .map(|q| self.answer_of(q))
            .collect()
    }

    /// Number of tabs including the confirm tab.
    fn tab_count(&self) -> usize {
        self.questions.len() + 1
    }

    fn move_tab(&mut self, delta: isize) {
        let n = self.tab_count() as isize;
        self.tab = (self.tab as isize + delta).rem_euclid(n) as usize;
    }

    fn move_cursor(&mut self, delta: isize) {
        let q = self.tab;
        let n = self.row_count(q) as isize;
        let c = self.answers[q].cursor as isize + delta;
        self.answers[q].cursor = c.clamp(0, n - 1) as usize;
    }

    pub fn key(&mut self, k: &KeyEvent) -> Outcome {
        if self.submitting {
            return Outcome::Open;
        }
        self.error = None;
        if self.edit.is_some() {
            return self.edit_key(k);
        }
        let plain = k.modifiers.difference(KeyModifiers::SHIFT).is_empty();
        match k.code {
            KeyCode::Esc => return Outcome::Dismiss,
            KeyCode::Tab if !self.is_simple() => self.move_tab(1),
            KeyCode::BackTab if !self.is_simple() => self.move_tab(-1),
            KeyCode::Right if !self.is_simple() => self.move_tab(1),
            KeyCode::Left if !self.is_simple() => self.move_tab(-1),
            _ if self.on_confirm_tab() => {
                if k.code == KeyCode::Enter {
                    return Outcome::Submit(self.all_answers());
                }
            }
            KeyCode::Up | KeyCode::Char('k') if plain => self.move_cursor(-1),
            KeyCode::Down | KeyCode::Char('j') if plain => self.move_cursor(1),
            KeyCode::Enter | KeyCode::Char(' ') => {
                let row = self.answers[self.tab].cursor;
                return self.choose(row);
            }
            KeyCode::Char(c @ '1'..='9') if plain => {
                let row = c as usize - '1' as usize;
                if row < self.row_count(self.tab) {
                    self.answers[self.tab].cursor = row;
                    return self.choose(row);
                }
            }
            _ => {}
        }
        Outcome::Open
    }

    /// Takes row `row` of the current question: picks or toggles an option, or starts typing.
    pub fn choose(&mut self, row: usize) -> Outcome {
        let q = self.tab;
        let question = &self.questions[q];
        if row >= question.options.len() {
            // On a multiple choice the custom row toggles: a typed answer is taken back.
            if question.multiple && self.answers[q].custom.is_some() {
                self.answers[q].custom = None;
                return Outcome::Open;
            }
            let mut editor = Editor::default();
            editor.set_text(self.answers[q].custom.as_deref().unwrap_or_default());
            self.edit = Some(editor);
            return Outcome::Open;
        }
        if question.multiple {
            let p = &mut self.answers[q].picked[row];
            *p = !*p;
            return Outcome::Open;
        }
        let a = &mut self.answers[q];
        a.picked.iter_mut().for_each(|p| *p = false);
        a.picked[row] = true;
        a.custom = None;
        self.after_single_choice()
    }

    /// A single choice is made: a lone question is done, otherwise go to the next tab.
    fn after_single_choice(&mut self) -> Outcome {
        if self.is_simple() {
            return Outcome::Submit(self.all_answers());
        }
        self.tab += 1;
        Outcome::Open
    }

    fn edit_key(&mut self, k: &KeyEvent) -> Outcome {
        let Some(editor) = self.edit.as_mut() else {
            return Outcome::Open;
        };
        match k.code {
            KeyCode::Esc => self.edit = None,
            KeyCode::Enter => {
                let text = editor.text.trim().to_string();
                self.edit = None;
                if text.is_empty() {
                    return Outcome::Open;
                }
                let q = self.tab;
                let a = &mut self.answers[q];
                a.custom = Some(text);
                if !self.questions[q].multiple {
                    a.picked.iter_mut().for_each(|p| *p = false);
                    return self.after_single_choice();
                }
            }
            _ => {
                crate::input::edit_key(editor, k, false);
            }
        }
        Outcome::Open
    }

    /// A click on a row or a tab.
    pub fn click(&mut self, x: u16, y: u16) -> Outcome {
        if self.submitting || self.edit.is_some() {
            return Outcome::Open;
        }
        self.error = None;
        if y == self.tab_y
            && let Some((_, _, tab)) = self.tab_hits.iter().find(|(x0, x1, _)| x >= *x0 && x < *x1)
        {
            self.tab = *tab;
            return Outcome::Open;
        }
        let Some((_, row)) = self.row_hits.iter().find(|(ry, _)| *ry == y).copied() else {
            return Outcome::Open;
        };
        if self.on_confirm_tab() {
            return Outcome::Open;
        }
        self.answers[self.tab].cursor = row;
        self.choose(row)
    }
}

/// The body of a page: its lines and which of them belong to which row.
struct Body {
    lines: Vec<Line<'static>>,
    /// (line index, row) for every line of an option row.
    row_lines: Vec<(usize, usize)>,
    /// First line of the cursor row.
    cursor_line: usize,
    /// Line and column of the typing field.
    edit_at: Option<(usize, usize)>,
}

impl QuestionView {
    pub fn draw(&mut self, buf: &mut Buffer, screen: Rect, session_name: Option<&str>) {
        let t = (self.opened.elapsed().as_secs_f32() / OPEN_SECS).min(1.0);
        let w = screen.width.saturating_sub(4).clamp(MIN_WIDTH, MAX_WIDTH);
        let iw = w.min(screen.width).saturating_sub(4) as usize;
        let title = self.title(session_name);
        let body = self.body(iw);
        let tabs = !self.is_simple();
        let extra = usize::from(self.error.is_some());
        let head = if tabs { 2 } else { 0 };
        let max_h = screen.height.saturating_sub(2).clamp(8, 36) as usize;
        let h = (body.lines.len() + head + extra + CHROME_ROWS).min(max_h);
        let r = frame(buf, screen, w, h as u16, &title, t);
        self.rect = r;
        let inner = Rect::new(
            r.x + 2,
            r.y + 1,
            r.width.saturating_sub(4),
            r.height.saturating_sub(2),
        );
        self.tab_hits.clear();
        self.row_hits.clear();
        self.cursor_at = None;
        if tabs {
            self.draw_tabs(buf, inner);
        }
        let body_y = inner.y + head as u16;
        let body_h = (inner.height as usize).saturating_sub(head + extra + 2);
        let scroll = body
            .cursor_line
            .saturating_sub(body_h.saturating_sub(1))
            .min(body.lines.len().saturating_sub(body_h));
        for (i, line) in body.lines.iter().skip(scroll).take(body_h).enumerate() {
            buf.set_line(inner.x, body_y + i as u16, line, inner.width);
        }
        for (line, row) in &body.row_lines {
            if *line >= scroll && *line < scroll + body_h {
                self.row_hits.push((body_y + (*line - scroll) as u16, *row));
            }
        }
        if let Some((line, col)) = body.edit_at
            && line >= scroll
            && line < scroll + body_h
            && let Some(editor) = self.edit.as_mut()
        {
            let field = Rect::new(
                inner.x + col as u16,
                body_y + (line - scroll) as u16,
                inner.width.saturating_sub(col as u16),
                1,
            );
            self.cursor_at = editor.draw(buf, field, "your answer", true);
        }
        let mut y = inner.y + inner.height.saturating_sub(1);
        buf.set_line(inner.x, y, &self.hint(), inner.width);
        if let Some(e) = &self.error {
            y = y.saturating_sub(2);
            let msg = text::truncate(e, inner.width as usize);
            buf.set_string(inner.x, y, msg, theme::fg(theme::ERROR));
        }
    }

    fn title(&self, session_name: Option<&str>) -> String {
        let base = if self.is_simple() {
            format!("? {}", self.questions[0].header)
        } else {
            "? Questions".to_string()
        };
        match session_name {
            Some(n) => format!("{base} · {n}"),
            None => base,
        }
    }

    fn draw_tabs(&mut self, buf: &mut Buffer, inner: Rect) {
        let n = self.tab_count();
        let per_tab = (inner.width as usize).saturating_sub(n) / n;
        let max_label = per_tab.saturating_sub(2).clamp(3, MAX_TAB_LABEL);
        self.tab_y = inner.y;
        let mut x = inner.x;
        for i in 0..n {
            let label = match self.questions.get(i) {
                Some(q) => text::truncate(&q.header, max_label),
                None => CONFIRM_LABEL.to_string(),
            };
            let label = format!(" {label} ");
            let style = if i == self.tab {
                Style::new()
                    .fg(theme::PANEL)
                    .bg(theme::PRIMARY)
                    .add_modifier(Modifier::BOLD)
            } else if i == n - 1 || self.is_answered(i) {
                theme::text()
            } else {
                theme::fg(theme::DIM)
            };
            let w = width(&label) as u16;
            buf.set_string(x, inner.y, &label, style);
            self.tab_hits.push((x, x + w, i));
            x += w + 1;
        }
    }

    fn hint(&self) -> Line<'static> {
        let pairs: Vec<(&str, &str)> = if self.submitting {
            vec![("", "sending…")]
        } else if self.edit.is_some() {
            let done = if self.is_simple() { "submit" } else { "done" };
            vec![("enter", done), ("esc", "back")]
        } else if self.is_simple() {
            vec![("↑↓", "select"), ("enter", "submit"), ("esc", "dismiss")]
        } else if self.on_confirm_tab() {
            vec![("⇆", "tab"), ("enter", "submit"), ("esc", "dismiss")]
        } else {
            let enter = if self.questions[self.tab].multiple {
                "toggle"
            } else {
                "select"
            };
            vec![
                ("⇆", "tab"),
                ("↑↓", "select"),
                ("enter", enter),
                ("esc", "dismiss"),
            ]
        };
        let mut spans = Vec::new();
        for (key, label) in pairs {
            if !spans.is_empty() {
                spans.push(Span::raw("  "));
            }
            if !key.is_empty() {
                spans.push(Span::styled(key.to_string(), theme::text()));
                spans.push(Span::raw(" "));
            }
            spans.push(Span::styled(label.to_string(), theme::muted()));
        }
        Line::from(spans)
    }

    fn body(&self, iw: usize) -> Body {
        if self.on_confirm_tab() {
            return self.confirm_body(iw);
        }
        let q = self.tab;
        let question = &self.questions[q];
        let mut body = Body {
            lines: text::wrap_spans(
                &[Span::styled(
                    question.question.clone(),
                    theme::bold(theme::TEXT),
                )],
                iw,
            ),
            row_lines: Vec::new(),
            cursor_line: 0,
            edit_at: None,
        };
        body.lines.push(Line::default());
        let cursor = self.cursor(q);
        for row in 0..self.row_count(q) {
            let on_cursor = row == cursor;
            let first = body.lines.len();
            if on_cursor {
                body.cursor_line = first;
            }
            let is_custom = row >= question.options.len();
            let (marker, checked) = self.marker(q, row);
            let prefix = format!("{}{marker}", if on_cursor { POINTER } else { NO_POINTER });
            let marker_style = if checked {
                theme::fg(theme::SUCCESS)
            } else {
                theme::muted()
            };
            let label_style = match (on_cursor, checked) {
                (_, true) => theme::bold(theme::SUCCESS),
                (true, false) => theme::bold(theme::PRIMARY),
                (false, false) => theme::text(),
            };
            let tick = checked && !question.multiple;
            let mut spans = vec![
                Span::styled(
                    if on_cursor { POINTER } else { NO_POINTER },
                    theme::fg(theme::PRIMARY),
                ),
                Span::styled(marker.clone(), marker_style),
            ];
            if is_custom {
                match (&self.edit, self.custom_text(q)) {
                    (Some(_), _) => body.edit_at = Some((first, width(&prefix))),
                    (None, Some(text)) => {
                        spans.push(Span::styled(format!("{CUSTOM_LABEL}: "), theme::muted()));
                        spans.push(Span::styled(text::one_line(text), label_style));
                    }
                    (None, None) => spans.push(Span::styled(CUSTOM_LABEL, label_style)),
                }
                if tick {
                    spans.push(Span::styled(TICK, theme::fg(theme::SUCCESS)));
                }
                body.lines.push(Line::from(spans));
            } else {
                let option = &question.options[row];
                spans.push(Span::styled(option.label.clone(), label_style));
                if option.default {
                    spans.push(Span::styled(DEFAULT_TAG, theme::muted()));
                }
                if tick {
                    spans.push(Span::styled(TICK, theme::fg(theme::SUCCESS)));
                }
                body.lines.push(Line::from(spans));
                let indent = " ".repeat(width(&prefix));
                let room = iw.saturating_sub(width(&prefix));
                for l in text::wrap_spans(
                    &[Span::styled(option.description.clone(), theme::muted())],
                    room,
                ) {
                    let mut spans = vec![Span::raw(indent.clone())];
                    spans.extend(l.spans);
                    body.lines.push(Line::from(spans));
                }
            }
            for line in first..body.lines.len() {
                body.row_lines.push((line, row));
            }
        }
        body
    }

    /// The number or checkbox in front of a row, and whether the row is chosen.
    fn marker(&self, q: usize, row: usize) -> (String, bool) {
        let question = &self.questions[q];
        let is_custom = row >= question.options.len();
        let checked = if is_custom {
            self.custom_text(q).is_some()
        } else {
            self.is_picked(q, row)
        };
        if question.multiple {
            let mark = if checked { "✓" } else { " " };
            (format!("[{mark}] "), checked)
        } else {
            (format!("{}. ", row + 1), checked && !self.is_simple())
        }
    }

    fn confirm_body(&self, iw: usize) -> Body {
        let mut lines = vec![
            Line::from(Span::styled(
                "Review your answers",
                theme::bold(theme::TEXT),
            )),
            Line::default(),
        ];
        for (q, question) in self.questions.iter().enumerate() {
            lines.extend(text::wrap_spans(
                &[Span::styled(question.question.clone(), theme::muted())],
                iw,
            ));
            let answer = self.answer_of(q);
            if answer.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!("  {NOT_ANSWERED}"),
                    theme::fg(theme::ERROR),
                )));
            }
            for a in answer {
                lines.push(Line::from(vec![
                    Span::styled("  ✓ ", theme::fg(theme::SUCCESS)),
                    Span::styled(text::one_line(&a), theme::text()),
                ]));
            }
            lines.push(Line::default());
        }
        lines.pop();
        Body {
            lines,
            row_lines: Vec::new(),
            cursor_line: 0,
            edit_at: None,
        }
    }
}
