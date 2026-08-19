use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;

use crate::domain::ports::{ToolCallResult, ToolPort};
use crate::domain::skill::SkillCatalog;

pub use crate::domain::skill::LOAD_SKILL_TOOL;
const BUILTIN_PREFIX: &str = "atoma_builtin__";

/// Combines Atoma's unconfigurable built-in tools with configured MCP tools.
pub struct RuntimeTools {
    skills: SkillCatalog,
    external: Option<Box<dyn ToolPort + Send>>,
}

impl RuntimeTools {
    pub fn new(skills: SkillCatalog, external: Option<Box<dyn ToolPort + Send>>) -> Result<Self> {
        if let Some(ref tools) = external {
            if let Some(name) = tools
                .tool_definitions()
                .iter()
                .find_map(tool_name)
                .filter(|name| name.starts_with(BUILTIN_PREFIX))
            {
                anyhow::bail!("External tool uses reserved Atoma namespace: '{}'", name);
            }
        }
        Ok(Self { skills, external })
    }

    fn load_skill_definition(&self) -> Value {
        let names: Vec<String> = self
            .skills
            .metadata()
            .into_iter()
            .map(|metadata| metadata.name)
            .collect();
        serde_json::json!({
            "type": "function",
            "function": {
                "name": LOAD_SKILL_TOOL,
                "description": "Load the full instructions for an available skill before applying it.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Exact skill name from the Available Skills catalog.",
                            "enum": names,
                        }
                    },
                    "required": ["name"],
                    "additionalProperties": false,
                }
            }
        })
    }
}

fn tool_name(definition: &Value) -> Option<&str> {
    definition.pointer("/function/name").and_then(Value::as_str)
}

#[async_trait]
impl ToolPort for RuntimeTools {
    fn tool_definitions(&self) -> Vec<Value> {
        let mut definitions = vec![self.load_skill_definition()];
        if let Some(ref external) = self.external {
            definitions.extend(external.tool_definitions());
        }
        definitions
    }

    async fn call_tool(
        &mut self,
        agent_name: &str,
        name: &str,
        arguments: &Value,
    ) -> Result<ToolCallResult> {
        if name == LOAD_SKILL_TOOL {
            let skill_name = arguments
                .get("name")
                .and_then(Value::as_str)
                .context("Skill tool requires a string 'name' argument")?;
            let skill = self
                .skills
                .get(skill_name)
                .with_context(|| format!("Unknown skill: '{}'", skill_name))?;
            return Ok(ToolCallResult {
                content: format!("# Skill: {}\n\n{}", skill.metadata.name, skill.instructions),
                ..Default::default()
            });
        }

        self.external
            .as_mut()
            .context("Unknown tool and no external MCP servers are configured")?
            .call_tool(agent_name, name, arguments)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn catalog() -> SkillCatalog {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("tdd.md"),
            "---\nname: engineering/tdd\ndescription: Test first.\n---\n\nUse red-green-refactor.\n",
        )
        .unwrap();
        crate::infra::persistence::skill::load(dir.path()).unwrap()
    }

    #[tokio::test]
    async fn built_in_skill_tool_is_always_visible_and_loads_instructions() {
        let mut tools = RuntimeTools::new(catalog(), None).unwrap();
        let definitions = tools.tool_definitions();
        assert_eq!(tool_name(&definitions[0]), Some(LOAD_SKILL_TOOL));
        assert_eq!(
            definitions[0].pointer("/function/parameters/properties/name/enum/0"),
            Some(&Value::String("engineering/tdd".to_string()))
        );

        let result = tools
            .call_tool(
                "engineer",
                LOAD_SKILL_TOOL,
                &serde_json::json!({"name": "engineering/tdd"}),
            )
            .await
            .unwrap();
        assert!(result.content.contains("# Skill: engineering/tdd"));
        assert!(result.content.contains("Use red-green-refactor."));
        assert!(!result.session_ends);
    }

    #[tokio::test]
    async fn unknown_skill_is_rejected() {
        let mut tools = RuntimeTools::new(catalog(), None).unwrap();
        let error = tools
            .call_tool(
                "engineer",
                LOAD_SKILL_TOOL,
                &serde_json::json!({"name": "missing"}),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("Unknown skill: 'missing'"));
    }
}
