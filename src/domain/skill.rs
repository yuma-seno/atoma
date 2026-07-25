use serde::Deserialize;

/// Metadata exposed in the system prompt before a skill is loaded.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
}

/// A validated skill available to the current run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub metadata: SkillMetadata,
    pub instructions: String,
}
