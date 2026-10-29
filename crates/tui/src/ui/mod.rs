pub mod chat;
pub mod dialog;
pub mod editor;
pub mod footer;
pub mod highlight;
pub mod markdown;
pub mod message;
pub mod permission;
pub mod selection;
pub mod split;
pub mod text;
pub mod theme;
pub mod toast;

use ratatui::text::{Line, Span};

/// One rendered screen row of transcript content.
#[derive(Debug, Clone, Default)]
pub struct Row {
    pub line: Line<'static>,
    /// Continuation of the previous row (soft wrap) — joined without a newline when copying.
    pub soft: bool,
    /// For soft rows: the wrap swallowed whitespace here (copy re-inserts one space).
    pub soft_space: bool,
    /// Leading decorative columns (padding, bars) excluded from copied text.
    pub skip: u16,
    /// Columns at/after this are decorative (labels, buttons) and excluded from copied text.
    pub end: Option<u16>,
    /// Purely decorative row (excluded from copied text entirely).
    pub decor: bool,
    /// Clickable zones on this row. Earlier zones win.
    pub hits: Vec<Zone>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Zone {
    pub x0: u16,
    pub x1: u16,
    pub hit: Hit,
}

/// Click targets inside the transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hit {
    /// Toggle expansion of a part (`message id`, part index).
    Toggle { msg: String, part: usize },
    /// Show full output of a tool part.
    ShowAll { msg: String, part: usize },
    /// Copy raw text of a message.
    Copy { msg: String },
}

impl Hit {
    pub fn is_button(&self) -> bool {
        matches!(self, Hit::Copy { .. } | Hit::ShowAll { .. })
    }
}

impl Row {
    pub fn new(line: Line<'static>) -> Self {
        Row { line, ..Default::default() }
    }
    pub fn blank() -> Self {
        Row::default()
    }
    pub fn soft(mut self, s: bool) -> Self {
        self.soft = s;
        self
    }
    /// Soft continuation that replaced a space (word wrap).
    pub fn soft_sp(mut self, soft: bool, space: bool) -> Self {
        self.soft = soft;
        self.soft_space = soft && space;
        self
    }
    pub fn skip(mut self, n: u16) -> Self {
        self.skip = n;
        self
    }
    pub fn end(mut self, e: u16) -> Self {
        self.end = Some(e);
        self
    }
    pub fn decor(mut self) -> Self {
        self.decor = true;
        self
    }
    /// Whole-row click target (added after any button zones).
    pub fn hit(mut self, h: Hit) -> Self {
        self.hits.push(Zone { x0: 0, x1: u16::MAX, hit: h });
        self
    }
    /// Click target on columns `x0..x1`, taking priority over whole-row zones.
    pub fn zone(mut self, x0: u16, x1: u16, h: Hit) -> Self {
        self.hits.insert(0, Zone { x0, x1, hit: h });
        self
    }
    /// Prepends decorative spans (shifting skip/end/zones).
    pub fn indent(mut self, spans: Vec<Span<'static>>) -> Self {
        let w: usize = spans.iter().map(|s| text::width(&s.content)).sum();
        let w = w as u16;
        let mut new = spans;
        new.append(&mut self.line.spans);
        self.line.spans = new;
        self.skip = self.skip.saturating_add(w);
        if let Some(e) = self.end.as_mut() {
            *e = e.saturating_add(w);
        }
        for z in &mut self.hits {
            if z.x1 != u16::MAX {
                z.x0 = z.x0.saturating_add(w);
                z.x1 = z.x1.saturating_add(w);
            }
        }
        self
    }
    pub fn hit_at(&self, x: u16) -> Option<&Hit> {
        self.hits.iter().find(|z| x >= z.x0 && x < z.x1).map(|z| &z.hit)
    }
}
