use serde::Deserialize;
use std::collections::BTreeMap;

/// The name of the built-in tool that loads a skill.
///
/// Here rather than in `application`, because the system prompt names it too and
/// `infra::template` cannot reach an application constant without inverting the
/// dependency. It was a literal in that template, so renaming this would have told every
/// model, in every prompt, to call a tool that no longer exists -- and it would have
/// called it, received "Unknown tool", and loaded no skill, with nothing failing at
/// build time.
pub const LOAD_SKILL_TOOL: &str = "atoma_builtin__load_skill";

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
