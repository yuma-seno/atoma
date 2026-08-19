/// The built-in template, as a port implementation.
///
/// A unit struct with no state: what it renders is this module's own constant, and the
/// point of the port is that `application` does not need to know that.
pub struct FileTemplateAdapter;

impl crate::domain::ports::TemplatePort for FileTemplateAdapter {
    fn build_system_prompt(&self, context: &crate::domain::ports::PromptContext<'_>) -> String {
        build_system_prompt(
            context.agent,
            context.tool_descriptions,
            context.custom_template,
            context.working_dir,
            context.colleagues,
            context.skills,
        )
    }
}

#[cfg(test)]
use crate::domain::agent::AgentDef;
use crate::domain::agent::ParsedAgentDef;
use crate::domain::skill::SkillMetadata;

static DEFAULT_TEMPLATE: &str = r#"# Identity & Purpose
You are "{{AGENT_NAME}}".

{{AGENT_ROLE_PROMPT}}

You are an autonomous AI agent that uses shared memory to collaborate asynchronously with human users and other AI agents to solve tasks.

# Available Colleagues
If you cannot complete a task on your own, you may delegate or request assistance from the following colleagues.
To make a request, include a `/agent-name` command in your output text (e.g. `/ReviewAgent Please review from a performance perspective`).

{{COLLEAGUES_LIST}}

# Environment & Tools
Working directory: `{{WORKING_DIRECTORY}}`

You interact with the environment through the Model Context Protocol (MCP). Do not guess code or environment state; always execute tools to verify facts.

Each tool runs as its own process and receives only the credentials its own configuration declares. A credential you cannot see from one tool is confined, not missing: a shell that reports nothing for an API token is behaving as intended, and the tool that needs that token has it. Do not hardcode a value, hunt for it in other places, or conclude the setup is broken because a token is absent from where you looked. If a tool genuinely fails to authenticate, report which tool and what it said.

{{AVAILABLE_TOOLS}}

# Available Skills
Skills are reusable instructions loaded on demand. Call `{{LOAD_SKILL_TOOL}}` before work covered by a relevant skill.

{{AVAILABLE_SKILLS}}

# Thought Process & Execution
Before taking action or generating final output, always use the `<thought>` tag to develop a rigorous thought process following the steps below:

<thought>
1. [Analyze]: Analyze the current context, requirements, and environment state.
2. [Plan]: Plan the next steps to execute based on your role and available tools.
3. [Act & Verify]: Execute tools and verify results. If errors or unexpected results occur, analyze the cause and re-execute. Do not proceed based on assumptions.
4. [Communicate]: Determine task completion, blockage status, and what text to output (including which agent to call).
</thought>

# Strict Rules
- [Tone] Eliminate all greetings, unnecessary apologies, and verbose explanations. Communicate in a technical and concise manner.
- [Tool Trustworthiness] Do not fabricate (hallucinate) file contents or execution results.
- [Autonomy & Coordination] Do not repeatedly call yourself or other agents without purpose (no infinite loops). Use `/` commands only when there is a clear request to make.
"#;

/// Build the system prompt by substituting template variables.
///
/// Template variables:
/// - `{{AGENT_NAME}}` — agent name
/// - `{{AGENT_ROLE_PROMPT}}` — custom body or description fallback
/// - `{{COLLEAGUES_LIST}}` — formatted list of known colleagues
/// - `{{AVAILABLE_TOOLS}}` — formatted list of tool descriptions
/// - `{{AVAILABLE_SKILLS}}` — skill names and descriptions (not full instructions)
/// - `{{LOAD_SKILL_TOOL}}` — the name of the tool that loads one, from `domain::skill`
/// - `{{WORKING_DIRECTORY}}` — current working directory
///
/// Pass `custom_template` to override the built-in template entirely.
pub fn build_system_prompt(
    agent: &ParsedAgentDef,
    tool_descriptions: &[String],
    custom_template: Option<&str>,
    working_dir: &str,
    colleagues: &[(String, String)],
    skills: &[SkillMetadata],
) -> String {
    let template = custom_template.unwrap_or(DEFAULT_TEMPLATE);
    let mut prompt = template.to_string();

    prompt = prompt.replace("{{AGENT_NAME}}", &agent.frontmatter.name);
    prompt = prompt.replace("{{WORKING_DIRECTORY}}", working_dir);

    let role_prompt = agent
        .body
        .as_deref()
        .unwrap_or(&agent.frontmatter.description);
    prompt = prompt.replace("{{AGENT_ROLE_PROMPT}}", role_prompt);

    if colleagues.is_empty() {
        prompt = prompt.replace(
            "{{COLLEAGUES_LIST}}",
            "No agents currently available for collaboration.",
        );
    } else {
        let lines: Vec<String> = colleagues
            .iter()
            .map(|(name, desc)| format!("- `{}`: {}", name, desc))
            .collect();
        prompt = prompt.replace("{{COLLEAGUES_LIST}}", &lines.join("\n"));
    }

    if tool_descriptions.is_empty() {
        prompt = prompt.replace("{{AVAILABLE_TOOLS}}", "No tools currently available.");
    } else {
        prompt = prompt.replace("{{AVAILABLE_TOOLS}}", &tool_descriptions.join("\n"));
    }

    // The one place the tool's name enters the prompt. A custom template written before
    // this placeholder existed simply keeps whatever it says, which is the same tolerance
    // every other placeholder here has.
    prompt = prompt.replace("{{LOAD_SKILL_TOOL}}", crate::domain::skill::LOAD_SKILL_TOOL);

    if skills.is_empty() {
        prompt = prompt.replace("{{AVAILABLE_SKILLS}}", "No skills currently available.");
    } else {
        let lines: Vec<String> = skills
            .iter()
            .map(|skill| format!("- `{}`: {}", skill.name, skill.description))
            .collect();
        prompt = prompt.replace("{{AVAILABLE_SKILLS}}", &lines.join("\n"));
    }

    prompt
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_test_agent(body: Option<String>) -> ParsedAgentDef {
        ParsedAgentDef {
            frontmatter: AgentDef {
                name: "TestAgent".to_string(),
                description: "A test agent for unit testing".to_string(),
                model: "openrouter/anthropic/claude-3.5-sonnet".to_string(),
                provider: None,
                vision: false,
                knows_about: vec!["ReviewAgent".to_string()],
                callable_by: vec![],
                mcp_servers: vec![],
                extra_body: HashMap::default(),
            },
            body,
        }
    }

    fn default_colleagues() -> Vec<(String, String)> {
        vec![(
            "ReviewAgent".to_string(),
            "Agent responsible for reviews".to_string(),
        )]
    }

    #[test]
    fn test_custom_body_injected_into_template() {
        let agent = make_test_agent(Some("Custom role description".to_string()));
        let result = build_system_prompt(&agent, &[], None, "/repo", &default_colleagues(), &[]);
        assert!(result.contains("TestAgent"));
        assert!(result.contains("Custom role description"));
        assert!(result.contains("Strict Rules"));
    }

    #[test]
    fn test_description_fallback_when_no_body() {
        let agent = make_test_agent(None);
        let result = build_system_prompt(&agent, &[], None, "/repo", &default_colleagues(), &[]);
        assert!(result.contains("A test agent for unit testing"));
    }

    #[test]
    fn test_template_substitution() {
        let agent = make_test_agent(None);
        let result = build_system_prompt(
            &agent,
            &["- `read_file`".to_string()],
            None,
            "/repo",
            &default_colleagues(),
            &[],
        );
        assert!(result.contains("TestAgent"));
        assert!(result.contains("A test agent for unit testing"));
        assert!(result.contains("ReviewAgent"));
        assert!(result.contains("read_file"));
        assert!(result.contains("/repo"));
    }

    #[test]
    fn test_custom_template() {
        let agent = make_test_agent(None);
        let custom = "Hello {{AGENT_NAME}}! Role: {{AGENT_ROLE_PROMPT}}";
        let result = build_system_prompt(&agent, &[], Some(custom), "/repo", &[], &[]);
        assert_eq!(
            result,
            "Hello TestAgent! Role: A test agent for unit testing"
        );
    }

    #[test]
    fn test_working_dir_substitution() {
        let agent = make_test_agent(None);
        let result = build_system_prompt(
            &agent,
            &[],
            None,
            "/home/runner/work/myrepo",
            &default_colleagues(),
            &[],
        );
        assert!(result.contains("/home/runner/work/myrepo"));
    }

    #[test]
    fn test_colleague_with_description() {
        let agent = make_test_agent(None);
        let colleagues = vec![
            (
                "engineer".to_string(),
                "Agent responsible for implementation".to_string(),
            ),
            (
                "reviewer".to_string(),
                "Agent responsible for reviews".to_string(),
            ),
        ];
        let result = build_system_prompt(&agent, &[], None, "/repo", &colleagues, &[]);
        assert!(result.contains("`engineer`: Agent responsible for implementation"));
        assert!(result.contains("`reviewer`: Agent responsible for reviews"));
    }

    #[test]
    fn test_skill_catalog_exposes_metadata_without_instructions() {
        let agent = make_test_agent(None);
        let skills = vec![SkillMetadata {
            name: "engineering/tdd".to_string(),
            description: "Test first.".to_string(),
        }];
        let result = build_system_prompt(&agent, &[], None, "/repo", &[], &skills);
        assert!(result.contains("`engineering/tdd`: Test first."));
        assert!(!result.contains("red-green-refactor"));
    }
}
