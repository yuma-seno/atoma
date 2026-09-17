use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;

use crate::domain::ports::{ToolCallResult, ToolPort};
use crate::domain::skill::SkillCatalog;

pub use crate::domain::skill::LOAD_SKILL_TOOL;
const BUILTIN_PREFIX: &str = "atoma_builtin__";

/// What the model is told about skills, at the moment it is choosing a tool.
///
/// The catalog in the system prompt is read once, at the start of a run. This is read
/// every time a tool is chosen, which is why the wording that has to survive a long run
/// lives here rather than there.
///
/// It opens by saying what a skill is and who wrote it, because the previous wording --
/// "Load the full instructions for an available skill before applying it" -- said neither,
/// and "instructions" without an author reads as reference material. Measured: a run that
/// needed `research/web-search` found the file and read it with `sed -n '1,60p'` instead of
/// loading it, then spent 240 shell searches inside a repository that did not hold the
/// answer. It treated the skill as a document to consult rather than a decision to adopt,
/// which is what the description had described.
///
/// The last sentence is the other half of that failure. Every wording here, and every one
/// in the shipped templates, said "before the work" and none said "again". Skills were
/// loaded twice in that run, both times in its first minute, and nothing invited a second
/// look when the work turned into something else.
const SKILL_TOOL_DESCRIPTION: &str = concat!(
    "A skill is a set of instructions this project has written for a particular kind of ",
    "work -- how a change is delivered here, how a failure is diagnosed, how to reach ",
    "something that is not in this repository. When the work in front of you is of a kind ",
    "a listed skill covers, call this first and follow what it says in place of your own ",
    "approach: it is what this project has decided, not advice to weigh. The Available ",
    "Skills catalog in your instructions carries names and one-line descriptions only. A ",
    "description is not the instructions and nothing in it can be applied without loading ",
    "it, so load it with this tool rather than reading the file yourself. Loading counts ",
    "toward no limit. Check the catalog again whenever the work changes shape: a skill ",
    "that was irrelevant when the run started becomes relevant the moment the work ",
    "reaches it.",
);
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
                "description": SKILL_TOOL_DESCRIPTION,
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

/// The skill a call names, whatever key it used.
///
/// `name` first, because that is the schema and the only key the model is told
/// about. Then the names it reaches for instead, then -- when the object holds one
/// string and nothing else -- that string, because a single value under a single
/// unexpected key is not ambiguous.
///
/// Measured over 169 calls in one repository, 75 failed, all of them here, all of
/// them because the argument was called `skill_name` (56), `skill` (15) or
/// `skill_id`. The schema says `name`, is marked required, and carries an `enum` of
/// every skill; none of that stopped it, and a second measurement in a different
/// repository found the same share again -- 77 of 187.
///
/// The refusal below already read the value out to write the corrected call. Having
/// recovered it, spending a round trip and a model turn to hand it back was a cost
/// with nothing bought: the tool takes one required string, so there was never a
/// second reading to choose between.
fn named_skill(arguments: &Value) -> Option<&str> {
    const ALIASES: [&str; 3] = ["skill", "skill_name", "skill_id"];
    let object = arguments.as_object()?;
    if let Some(name) = object.get("name").and_then(Value::as_str) {
        return Some(name);
    }
    for alias in ALIASES {
        if let Some(name) = object.get(alias).and_then(Value::as_str) {
            return Some(name);
        }
    }
    let mut strings = object.values().filter_map(Value::as_str);
    let only = strings.next()?;
    strings.next().is_none().then_some(only)
}

/// What to say when a skill call carries no skill at all.
///
/// Only reached once [`named_skill`] has failed to find one, which is a call carrying
/// no string, or several with nothing to choose between them. The keys models reach
/// for instead of `name` are read rather than refused; see there for the measurement.
///
/// The message hands back the call that would have worked rather than restating the
/// schema. Measured elsewhere in this project, a refusal naming the next action is
/// taken and one that only states a rule is not -- twice, on two different guards.
fn skill_argument_message(arguments: &Value) -> String {
    let mut keys: Vec<&str> = arguments
        .as_object()
        .map(|o| o.keys().map(String::as_str).collect())
        .unwrap_or_default();
    keys.sort_unstable();

    // A value under the wrong key is almost certainly the skill that was wanted, so the
    // corrected call can be written out in full rather than described.
    let guess = arguments
        .as_object()
        .and_then(|o| o.values().find_map(Value::as_str));

    let received = if keys.is_empty() {
        "no arguments".to_string()
    } else {
        format!("{{{}}}", keys.join(", "))
    };

    match guess {
        Some(value) => format!(
            "This tool takes its argument as 'name'. You passed {}. Call it again with {{\"name\": \"{}\"}}.",
            received, value
        ),
        None => format!(
            "This tool takes its argument as 'name', a string naming one skill from the Available Skills catalog. You passed {}.",
            received
        ),
    }
}

/// What to say when the skill named does not exist.
///
/// Lists what does. The catalog is in the system prompt, but a run that got here read it
/// and still missed, so repeating the names at the moment of the mistake costs a line and
/// removes the guess.
fn unknown_skill_message(asked: &str, available: &[String]) -> String {
    format!(
        "No skill is called '{}'. The ones that exist are: {}.",
        asked,
        available.join(", ")
    )
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
            let skill_name = named_skill(arguments)
                .ok_or_else(|| anyhow::anyhow!(skill_argument_message(arguments)))?;
            let available: Vec<String> =
                self.skills.metadata().into_iter().map(|m| m.name).collect();
            let skill = self
                .skills
                .get(skill_name)
                .ok_or_else(|| anyhow::anyhow!(unknown_skill_message(skill_name, &available)))?;
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

    /// The keys the models actually used, in the order the measurement found them.
    ///
    /// 56 of 75 failures were `skill_name`, 15 were `skill`. Each of these was a run
    /// that had chosen the right skill, named it correctly, and got nothing.
    #[tokio::test]
    async fn a_skill_named_under_another_key_still_loads() {
        for key in ["name", "skill", "skill_name", "skill_id", "unexpected"] {
            let mut tools = RuntimeTools::new(catalog(), None).unwrap();
            let mut arguments = serde_json::Map::new();
            arguments.insert(
                key.to_string(),
                serde_json::Value::String("engineering/tdd".to_string()),
            );
            let result = tools
                .call_tool(
                    "engineer",
                    LOAD_SKILL_TOOL,
                    &serde_json::Value::Object(arguments),
                )
                .await
                .unwrap_or_else(|e| panic!("{key}: {e}"));
            assert!(result.content.contains("# Skill: engineering/tdd"), "{key}");
        }
    }

    /// Forgiveness stops where the reading would have to be guessed at. Two strings
    /// under two unexpected keys is a call this cannot read, and saying so is the
    /// honest answer -- the message that names the call which works is still there.
    #[tokio::test]
    async fn two_unexpected_strings_are_still_refused() {
        let mut tools = RuntimeTools::new(catalog(), None).unwrap();
        let error = tools
            .call_tool(
                "engineer",
                LOAD_SKILL_TOOL,
                &serde_json::json!({"a": "engineering/tdd", "b": "engineering/tdd"}),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("name"), "{error}");
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
        // The property rather than the sentence: it names what was asked for, and it
        // names what exists. The old assertion pinned "Unknown skill: 'missing'" word
        // for word, which said nothing about why that wording had to be there.
        let message = error.to_string();
        assert!(message.contains("'missing'"), "{}", message);
        assert!(message.contains("engineering/tdd"), "{}", message);
    }

    /// The failure this replaced: 75 of 169 calls in one repository, every one of them
    /// passing the skill under a key that was not `name`. A message that restates the
    /// schema is a message the caller has already read past.
    #[tokio::test]
    async fn a_wrongly_named_argument_is_told_what_to_call_instead() {
        let mut tools = RuntimeTools::new(catalog(), None).unwrap();
        let error = tools
            .call_tool(
                "engineer",
                LOAD_SKILL_TOOL,
                &serde_json::json!({"skill_name": "engineering/tdd"}),
            )
            .await
            .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("skill_name"), "{}", message);
        // The corrected call, in full, so following it needs no interpretation.
        assert!(
            message.contains(r#"{"name": "engineering/tdd"}"#),
            "{}",
            message
        );
    }

    /// With nothing to guess from, it says what the argument is and stops.
    #[tokio::test]
    async fn no_arguments_at_all_is_said_plainly() {
        let mut tools = RuntimeTools::new(catalog(), None).unwrap();
        let error = tools
            .call_tool("engineer", LOAD_SKILL_TOOL, &serde_json::json!({}))
            .await
            .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("no arguments"), "{}", message);
        assert!(message.contains("'name'"), "{}", message);
    }
}
