use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    /// Path to SKILL.md
    pub path: PathBuf,
}
