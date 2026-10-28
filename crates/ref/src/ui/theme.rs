//! "opencode" dark theme colors.

use ratatui::style::{Color, Modifier, Style};

pub const fn rgb(h: u32) -> Color {
    Color::Rgb((h >> 16) as u8, (h >> 8) as u8, h as u8)
}

pub const PANEL: Color = rgb(0x141414);
pub const ELEMENT: Color = rgb(0x1e1e1e);
pub const PRIMARY: Color = rgb(0xfab283);
pub const SECONDARY: Color = rgb(0x5c9cf5);
pub const ACCENT: Color = rgb(0x9d7cd8);
pub const TEXT: Color = rgb(0xeeeeee);
pub const MUTED: Color = rgb(0x808080);
pub const BORDER: Color = rgb(0x484848);
pub const BORDER_ACTIVE: Color = rgb(0x606060);
pub const BORDER_SUBTLE: Color = rgb(0x3c3c3c);
pub const SUCCESS: Color = rgb(0x7fd88f);
pub const WARNING: Color = rgb(0xf5a742);
pub const ERROR: Color = rgb(0xe06c75);
pub const INFO: Color = rgb(0x56b6c2);

pub const MD_HEADING: Color = ACCENT;
pub const MD_LINK_TEXT: Color = INFO;
pub const MD_CODE: Color = SUCCESS;
pub const MD_QUOTE: Color = rgb(0xe5c07b);
pub const MD_EMPH: Color = rgb(0xe5c07b);
pub const MD_STRONG: Color = WARNING;
pub const MD_HR: Color = MUTED;
pub const MD_LIST: Color = PRIMARY;
pub const MD_ENUM: Color = INFO;
pub const MD_CODE_BLOCK: Color = TEXT;

pub const SYN_COMMENT: Color = MUTED;
pub const SYN_KEYWORD: Color = ACCENT;
pub const SYN_FUNCTION: Color = PRIMARY;
pub const SYN_VARIABLE: Color = ERROR;
pub const SYN_STRING: Color = SUCCESS;
pub const SYN_NUMBER: Color = WARNING;
pub const SYN_TYPE: Color = rgb(0xe5c07b);
pub const SYN_OPERATOR: Color = INFO;

/// Background of in-app text selection.
pub const SELECTION: Color = rgb(0x31456b);

/// Categorical agent palette (opencode dark order).
pub const AGENT_PALETTE: [Color; 5] = [SECONDARY, ACCENT, SUCCESS, PRIMARY, ERROR];

pub fn fg(c: Color) -> Style {
    Style::new().fg(c)
}

pub fn muted() -> Style {
    Style::new().fg(MUTED)
}

pub fn text() -> Style {
    Style::new().fg(TEXT)
}

pub fn bold(c: Color) -> Style {
    Style::new().fg(c).add_modifier(Modifier::BOLD)
}

/// Parse an agent color from config: `#rrggbb` or a theme token name.
pub fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 6 {
            return u32::from_str_radix(hex, 16).ok().map(rgb);
        }
        if hex.len() == 3 {
            let v = u32::from_str_radix(hex, 16).ok()?;
            let (r, g, b) = ((v >> 8) & 0xf, (v >> 4) & 0xf, v & 0xf);
            return Some(Color::Rgb((r * 17) as u8, (g * 17) as u8, (b * 17) as u8));
        }
        return None;
    }
    Some(match s.to_ascii_lowercase().as_str() {
        "primary" => PRIMARY,
        "secondary" => SECONDARY,
        "accent" => ACCENT,
        "success" | "green" => SUCCESS,
        "warning" | "yellow" | "orange" => WARNING,
        "error" | "red" => ERROR,
        "info" | "cyan" => INFO,
        "blue" => SECONDARY,
        "purple" | "magenta" => ACCENT,
        _ => return None,
    })
}
