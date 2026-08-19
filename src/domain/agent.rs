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
    /// LLM provider override. Checked before the `ATOMA_PROVIDER` variable and before
    /// auto-detection from the credential.
    ///
    /// The valid names are the providers `infra::llm` declares, and are deliberately not
    /// repeated here: this comment listed four of them for long enough to be wrong,
    /// missing every router provider, while sitting on the field a user fills in.
    /// `atoma --help` prints the real list, and naming one that does not exist fails with
    /// it too.
    #[serde(default)]
    pub provider: Option<String>,
    /// Whether this agent's model can read an image.
    ///
    /// Off unless declared, because the cost of the two mistakes is not
    /// symmetric: sending a picture to a text-only model is an API error that
    /// loses the run, while withholding one from a model that could have read it
    /// costs a tool result that says so. A tool that returns an image to an
    /// agent without this set gets text naming the setting, so the omission
    /// reports itself instead of looking like the image was never produced.
    #[serde(default)]
    pub vision: bool,
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
