//! Shared lookup data for rendering: agents, models, commands.

use hn_proto::{AgentInfo, CommandInfo, ModelInfo, ModelRef};
use ratatui::style::Color;

use crate::theme;

#[derive(Debug, Default, Clone)]
pub struct Catalog {
    pub agents: Vec<AgentInfo>,
    pub models: Vec<ModelInfo>,
    pub commands: Vec<CommandInfo>,
}

impl Catalog {
    pub fn agent_color(&self, name: &str) -> Color {
        if let Some((i, a)) = self.agents.iter().enumerate().find(|(_, a)| a.name == name) {
            if let Some(c) = a.color.as_deref().and_then(theme::parse_color) {
                return c;
            }
            return theme::AGENT_COLORS[i % theme::AGENT_COLORS.len()];
        }
        // Unknown agent: stable color from the name.
        let h = name.bytes().fold(0usize, |a, b| a.wrapping_mul(31).wrapping_add(b as usize));
        theme::AGENT_COLORS[h % theme::AGENT_COLORS.len()]
    }

    pub fn model(&self, m: &ModelRef) -> Option<&ModelInfo> {
        self.models.iter().find(|x| x.provider == m.provider && x.model == m.model)
    }

    /// Human model name ("Claude Opus 5.5"), falling back to the model id.
    pub fn model_name(&self, m: &ModelRef) -> String {
        self.model(m).map(|x| x.name.clone()).unwrap_or_else(|| m.model.clone())
    }

    pub fn provider_name(&self, m: &ModelRef) -> String {
        self.model(m).map(|x| x.provider_name.clone()).unwrap_or_else(|| m.provider.clone())
    }
}

/// Capitalize the first letter (agent display names).
pub fn title_case(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}
