use crate::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action { Allow, Ask, Deny }

/// Flattened ordered rules: (tool glob, subject glob, action). Last match wins.
#[derive(Debug, Clone, Default)]
pub struct PermissionRules { pub rules: Vec<(String, String, Action)> }

pub(crate) fn evaluate(_c: &Config, _agent: &str, _tool: &str, _subject: &str) -> Action { todo!() }
pub(crate) fn delay(_c: &Config, _agent: &str, _tool: &str) -> Duration { todo!() }
