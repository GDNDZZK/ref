//! Dark theme modeled after opencode's default theme (true colors).

use ratatui::style::{Color, Modifier, Style};

pub const fn rgb(hex: u32) -> Color {
    Color::Rgb(((hex >> 16) & 0xff) as u8, ((hex >> 8) & 0xff) as u8, (hex & 0xff) as u8)
}

pub const BG: Color = rgb(0x0a0a0a);
pub const PANEL: Color = rgb(0x141414);
pub const ELEMENT: Color = rgb(0x1e1e1e);
pub const PRIMARY: Color = rgb(0xfab283);
pub const SECONDARY: Color = rgb(0x5c9cf5);
pub const ACCENT: Color = rgb(0x9d7cd8);
pub const TEXT: Color = rgb(0xeeeeee);
pub const MUTED: Color = rgb(0x808080);
pub const SUCCESS: Color = rgb(0x7fd88f);
pub const WARNING: Color = rgb(0xf5a742);
pub const ERROR: Color = rgb(0xe06c75);
pub const INFO: Color = rgb(0x56b6c2);
pub const BORDER: Color = rgb(0x484848);
pub const BORDER_ACTIVE: Color = rgb(0x606060);
pub const BORDER_SUBTLE: Color = rgb(0x3c3c3c);
/// Selection highlight background.
pub const SELECTION: Color = rgb(0x3a3d5c);
/// Dim text (slightly darker than muted), used for faint hints.
pub const FAINT: Color = rgb(0x5a5a5a);

pub const MD_HEADING: Color = ACCENT;
pub const MD_LINK_TEXT: Color = INFO;
pub const MD_CODE: Color = SUCCESS;
pub const MD_QUOTE: Color = rgb(0xe5c07b);
pub const MD_EMPH: Color = rgb(0xe5c07b);
pub const MD_STRONG: Color = WARNING;
pub const MD_LIST: Color = PRIMARY;
pub const MD_ENUM: Color = INFO;

pub const SYN_COMMENT: Color = MUTED;
pub const SYN_KEYWORD: Color = ACCENT;
pub const SYN_FUNCTION: Color = PRIMARY;
pub const SYN_STRING: Color = SUCCESS;
pub const SYN_NUMBER: Color = WARNING;
pub const SYN_TYPE: Color = rgb(0xe5c07b);
pub const SYN_PUNCT: Color = TEXT;

/// Categorical agent colors (opencode: blue, purple, green, orange, red).
pub const AGENT_COLORS: [Color; 5] = [SECONDARY, ACCENT, SUCCESS, WARNING, ERROR];

pub fn fg(c: Color) -> Style {
    Style::new().fg(c)
}

pub fn text() -> Style {
    fg(TEXT)
}

pub fn muted() -> Style {
    fg(MUTED)
}

pub fn faint() -> Style {
    fg(FAINT)
}

pub fn bold(c: Color) -> Style {
    fg(c).add_modifier(Modifier::BOLD)
}

/// Parse an agent color: `#rrggbb` or a theme color name.
pub fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if let Some(h) = s.strip_prefix('#') {
        if h.len() == 6 {
            return u32::from_str_radix(h, 16).ok().map(rgb);
        }
        if h.len() == 3 {
            let v = u32::from_str_radix(h, 16).ok()?;
            let (r, g, b) = ((v >> 8) & 0xf, (v >> 4) & 0xf, v & 0xf);
            return Some(Color::Rgb((r * 17) as u8, (g * 17) as u8, (b * 17) as u8));
        }
        return None;
    }
    Some(match s.to_ascii_lowercase().as_str() {
        "primary" => PRIMARY,
        "secondary" | "blue" => SECONDARY,
        "accent" | "purple" => ACCENT,
        "success" | "green" => SUCCESS,
        "warning" | "orange" | "yellow" => WARNING,
        "error" | "red" => ERROR,
        "info" | "cyan" => INFO,
        "text" | "white" => TEXT,
        "muted" | "gray" | "grey" => MUTED,
        _ => return None,
    })
}

/// Linear blend between two RGB colors (t in 0..=1).
pub fn blend(a: Color, b: Color, t: f32) -> Color {
    match (a, b) {
        (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) => {
            let t = t.clamp(0.0, 1.0);
            let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
            Color::Rgb(m(r1, r2), m(g1, g2), m(b1, b2))
        }
        _ => b,
    }
}

pub const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn spinner(now_ms: i64) -> &'static str {
    SPINNER[((now_ms / 80).rem_euclid(SPINNER.len() as i64)) as usize]
}
