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
    /// The context window this agent's model serves, in tokens.
    ///
    /// `None` — the default — means no compaction: every agent written before this
    /// field existed runs unchanged, and an agent whose runs are short does not
    /// need one. The number is the model's real window, not a target: atoma
    /// compacts when the PROMPT has grown past it (see
    /// `domain::context_budget`), and the provider's 400 is what happens when
    /// this number is a lie.
    ///
    /// Declared here rather than guessed from the model name, for the same reason
    /// every other definition fact lives in the definition: a table of model names
    /// in this crate is wrong the week a new model ships.
    #[serde(default)]
    pub context_limit: Option<u64>,
    #[serde(default)]
    pub knows_about: Vec<String>,
    // `callable_by` was here. It listed who may invoke an agent -- `"user"`,
    // `"agent"` -- and nothing enforced it. Its own documentation said so: "the
    // atoma binary itself does not enforce it".
    //
    // What it did was let `atoma validate` check that a `knows_about` target had
    // declared `callable_by: ["agent"]`. That is one declaration checking another
    // declaration of the same fact: listing an agent in `knows_about` IS saying an
    // agent may call it. The check passed whenever both were written and failed
    // whenever one was forgotten, which made it a spelling test rather than a rule.
    //
    // Removed rather than enforced, because there was nothing to enforce: invocation
    // happens in the calling automation, which never read this field.
    /// Names of MCP tool servers used by this agent.
    /// Each name must correspond to an entry in the tools file (--tools-file).
    #[serde(default)]
    pub mcp_servers: Vec<String>,
    /// Arbitrary key-value pairs merged into the LLM API request body.
    /// The reserved fields `model` and `messages` cannot be overridden.
    #[serde(default)]
    pub extra_body: HashMap<String, Value>,
    /// Headers sent with every request this agent makes.
    ///
    /// For a setting a provider reads from a header rather than from the body --
    /// OrcaRouter's `X-OrcaRouter-Session-Id`, which keeps one conversation on one
    /// deployment so its prompt cache stays warm, is the case this was added for.
    ///
    /// A header Atoma sets itself is refused by `validate` rather than merged: those
    /// carry authentication, the shape of the body, the API version, and which model
    /// catalogue the account may reach. Overriding one does not read as a mistake
    /// afterwards -- it reads as the provider behaving strangely.
    #[serde(default)]
    pub extra_headers: HashMap<String, String>,
}

/// A fully parsed agent definition: frontmatter + optional body.
pub struct ParsedAgentDef {
    pub frontmatter: AgentDef,
    /// Markdown body after the frontmatter block (used as role prompt template).
    /// `None` means the built-in template fallback should be used.
    pub body: Option<String>,
}
