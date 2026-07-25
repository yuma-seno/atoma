use serde::Deserialize;
use std::collections::BTreeMap;

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

/// Immutable, name-indexed set of skills validated at run startup.
#[derive(Debug, Clone, Default)]
pub struct SkillCatalog {
    skills: BTreeMap<String, Skill>,
}

impl SkillCatalog {
    pub fn new(skills: Vec<Skill>) -> anyhow::Result<Self> {
        let mut indexed = BTreeMap::new();
        for skill in skills {
            let name = skill.metadata.name.clone();
            if indexed.insert(name.clone(), skill).is_some() {
                anyhow::bail!("Duplicate skill name '{}'", name);
            }
        }
        Ok(Self { skills: indexed })
    }

    pub fn metadata(&self) -> Vec<SkillMetadata> {
        self.skills
            .values()
            .map(|skill| skill.metadata.clone())
            .collect()
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.get(name)
    }
}
