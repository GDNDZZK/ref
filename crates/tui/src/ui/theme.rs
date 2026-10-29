//! opencode dark palette. Background is never painted except for panels.

use ratatui::style::{Color, Modifier, Style};

pub const fn rgb(hex: u32) -> Color {
    Color::Rgb(((hex >> 16) & 0xff) as u8, ((hex >> 8) & 0xff) as u8, (hex & 0xff) as u8)
}

pub const PRIMARY: Color = rgb(0xfab283);
pub const SECONDARY: Color = rgb(0x5c9cf5);
pub const ACCENT: Color = rgb(0x9d7cd8);
pub const ERROR: Color = rgb(0xe06c75);
pub const WARNING: Color = rgb(0xf5a742);
pub const SUCCESS: Color = rgb(0x7fd88f);
pub const INFO: Color = rgb(0x56b6c2);
pub const TEXT: Color = rgb(0xeeeeee);
pub const MUTED: Color = rgb(0x808080);
pub const DIM: Color = rgb(0x5a5a5a);
pub const BORDER: Color = rgb(0x484848);
pub const BORDER_ACTIVE: Color = rgb(0x606060);
pub const BORDER_SUBTLE: Color = rgb(0x3c3c3c);
/// User message block / dialogs.
pub const PANEL: Color = rgb(0x1e1e1e);
/// Hovered panel / tool blocks.
pub const PANEL_HOVER: Color = rgb(0x282828);
/// Code blocks / expanded tool blocks.
pub const ELEMENT: Color = rgb(0x141414);
pub const SELECTION: Color = rgb(0x3a3a5a);

pub const MD_HEADING: Color = ACCENT;
pub const MD_LINK: Color = PRIMARY;
pub const MD_LINK_TEXT: Color = INFO;
pub const MD_CODE: Color = SUCCESS;
pub const MD_QUOTE: Color = rgb(0xe5c07b);
pub const MD_EMPH: Color = rgb(0xe5c07b);
pub const MD_STRONG: Color = WARNING;
pub const MD_LIST: Color = PRIMARY;
pub const MD_ENUM: Color = INFO;

pub const DIFF_ADDED: Color = rgb(0x4fd6be);
pub const DIFF_REMOVED: Color = rgb(0xc53b53);
pub const DIFF_ADDED_BG: Color = rgb(0x20303b);
pub const DIFF_REMOVED_BG: Color = rgb(0x37222c);
pub const DIFF_CONTEXT: Color = rgb(0x828bb8);

/// Default agent colours (opencode categorical palette).
const AGENT_PALETTE: [Color; 5] = [SECONDARY, ACCENT, SUCCESS, PRIMARY, ERROR];

pub fn fg(c: Color) -> Style {
    Style::new().fg(c)
}
pub fn bold(c: Color) -> Style {
    Style::new().fg(c).add_modifier(Modifier::BOLD)
}
pub fn muted() -> Style {
    fg(MUTED)
}
pub fn text() -> Style {
    fg(TEXT)
}

/// Parses `#rrggbb` or a few named colours.
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
        "primary" | "orange" => PRIMARY,
        "secondary" | "blue" => SECONDARY,
        "accent" | "purple" => ACCENT,
        "error" | "red" => ERROR,
        "warning" | "yellow" => WARNING,
        "success" | "green" => SUCCESS,
        "info" | "cyan" => INFO,
        _ => return None,
    })
}

/// Colour of an agent: configured colour, else palette entry by position.
pub fn agent_color(agents: &[ref_proto::AgentInfo], name: &str) -> Color {
    let idx = agents.iter().position(|a| a.name == name);
    if let Some(c) = idx.and_then(|i| agents[i].color.as_deref()).and_then(parse_color) {
        return c;
    }
    let i = match idx {
        Some(i) => i,
        None => name.bytes().fold(0usize, |a, b| a.wrapping_mul(31).wrapping_add(b as usize)),
    };
    AGENT_PALETTE[i % AGENT_PALETTE.len()]
}

/// Linear blend of two RGB colours (t = 0 → a, 1 → b).
pub fn blend(a: Color, b: Color, t: f32) -> Color {
    match (a, b) {
        (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) => {
            let t = t.clamp(0.0, 1.0);
            let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
            Color::Rgb(m(r1, r2), m(g1, g2), m(b1, b2))
        }
        _ => {
            if t < 0.5 {
                a
            } else {
                b
            }
        }
    }
}

pub const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn spinner(tick: u64) -> &'static str {
    SPINNER[(tick as usize) % SPINNER.len()]
}
