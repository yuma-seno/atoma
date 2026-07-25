use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

/// Frontmatter of an agent definition file.
///
/// Parsing from YAML and loading from disk is handled by
/// `crate::infra::persistence::agent_def`.
#[derive(Debug, Clone, Deserialize)]
pub struct AgentDef {
    pub name: String,
    pub description: String,
    pub model: String,
    /// LLM provider override. Checked before `ATOMA_PROVIDER` env var and auto-detection.
    /// Valid values: `openai`, `github-copilot`, `anthropic`.
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub knows_about: Vec<String>,
    /// Who may invoke this agent: `"user"` (human entry point, e.g. a slash-command
    /// or new-issue trigger) and/or `"agent"` (delegated to by another agent via
    /// `knows_about` / orchestration tooling). Purely advisory metadata checked by
    /// `atoma validate`; the atoma binary itself does not enforce it (any real
    /// invocation-time access control lives in the calling automation).
    #[serde(default)]
    pub callable_by: Vec<String>,
    /// Names of MCP tool servers used by this agent.
    /// Each name must correspond to an entry in the tools file (--tools-file).
    #[serde(default)]
    pub mcp_servers: Vec<String>,
    /// Arbitrary key-value pairs merged into the LLM API request body.
    /// The reserved fields `model` and `messages` cannot be overridden.
    #[serde(default)]
    pub extra_body: HashMap<String, Value>,
}

/// A fully parsed agent definition: frontmatter + optional body.
pub struct ParsedAgentDef {
    pub frontmatter: AgentDef,
    /// Markdown body after the frontmatter block (used as role prompt template).
    /// `None` means the built-in template fallback should be used.
    pub body: Option<String>,
}
