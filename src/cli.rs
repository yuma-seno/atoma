use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "atoma",
    version,
    about = "Stateless MCP orchestrator CLI",
    long_about = "Atoma is a lightweight, stateless CLI that orchestrates AI agents \
via Model Context Protocol (MCP). It connects LLMs (OpenAI-compatible) with MCP \
server tools in an autonomous inference loop.

It does NOT depend on any specific platform (GitHub, etc.). It is a pure protocol \
orchestrator: parse agent definition → call LLM → execute tools → repeat until final response.",
    after_long_help = "ENVIRONMENT VARIABLES:
  OPENAI_API_KEY         (required*)  API key for the OpenAI-compatible endpoint
  OPENAI_BASE_URL        (optional)   API base URL (default: https://openrouter.ai/api/v1)
  GITHUB_TOKEN / GH_TOKEN (required*) GitHub token for GitHub Copilot mode
  ANTHROPIC_API_KEY      (required*)  API key for Anthropic (Claude) models
  ANTHROPIC_BASE_URL     (optional)   Anthropic API base URL (default: https://api.anthropic.com)
  ATOMA_PROVIDER         (optional)   Force provider: 'openai', 'github-copilot', or 'anthropic'
                                       Auto-detected when unset (anthropic if only ANTHROPIC_API_KEY
                                       is set; github-copilot if only GITHUB_TOKEN is set; otherwise openai).
  ATOMA_HOOK_TIMEOUT     (optional)   Hook script timeout in seconds (default: 30)
  ATOMA_MCP_TIMEOUT      (optional)   MCP tool call timeout in seconds (default: 60)
  ATOMA_MCP_INIT_TIMEOUT (optional)   MCP server init timeout in seconds (default: 120)

  * One of OPENAI_API_KEY, GITHUB_TOKEN / GH_TOKEN, or ANTHROPIC_API_KEY must be provided.

PROVIDER SELECTION:
  Priority: agent definition 'provider:' field > ATOMA_PROVIDER env > auto-detect

EXAMPLES:
  # One-shot (no session saved)
  atoma run --agent-def ./agent.md --prompt-file ./prompt.txt

  # Inject per-run context without polluting the durable session
  atoma run --agent-def ./agent.md \
    --context-session ./issue-summary.json \
    --context-session ./latest-diff.json

  # Stdin as prompt — composable with shell pipelines
  echo \"Hello\" | atoma run --agent-def ./agent.md
  git diff | atoma run --agent-def ./ReviewAgent.md --in-session ./sess.json --out-session ./sess.json

  # First run: creates a new session
  atoma run --agent-def ./OrchestratorAgent.md \\
    --prompt-file ./task.txt --out-session ./session.json

  # Resume a conversation
  atoma run --agent-def ./agent.md \\
    --in-session ./session.json --prompt-file ./followup.txt --out-session ./session.json

  # Read previous session, write to a new file
  atoma run --agent-def ./agent.md \\
    --in-session ./prev.json --prompt-file ./task.txt --out-session ./next.json

AGENT DEFINITION:
  Agent definitions are Markdown files with YAML frontmatter:

  ---
  name: MyAgent
  description: What this agent does
  model: openrouter/anthropic/claude-3.5-sonnet
  callable_by: [user, agent]
  mcp_servers:
    - filesystem
    - shell
  ---

  Optional body after frontmatter: used as custom system prompt instead of the built-in template.
  MCP server names in `mcp_servers` must match keys in the --tools-file YAML.

MCP TOOLS:
  Tools are auto-discovered from MCP servers at startup. Each tool is prefixed with
  the server name (e.g., \"filesystem__read_file\") for routing.

INFERENCE LOOP:
  1. Load input session (or start fresh)
  2. Build system prompt from agent definition
  3. Connect MCP servers, discover tools
  4. Append user prompt (from --prompt-file or stdin)
  5. Loop:
     a. Call LLM
     b. If tool_calls → execute via MCP, add results, continue
     c. If text response → print to stdout, save session, exit

SESSION FILES:
  JSON format with messages array (OpenAI-compatible chat format).
  Input (--in-session) and output (--out-session) are always separate arguments,
  making data flow explicit. Specify the same path for both to update in place.

  --context-session accepts additional session JSON files whose messages are
  injected only for the current run. These messages are inserted after the
  system prompt and before the durable session history, then removed before
  --out-session is written."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Run the agent inference loop
    #[command(
        long_about = "Run the agent inference loop.

Parses the agent definition, loads input session state, connects MCP servers,
executes the LLM tool-calling loop, and outputs the final response.",
        after_help = "EXAMPLES:
  # One-shot with stdin prompt
  echo \"Hello\" | atoma run --agent-def ./agent.md

  # Inject per-run context without persisting it
  atoma run --agent-def ./agent.md \
    --context-session ./issue-summary.json \
    --context-session ./latest-diff.json

  # File prompt, no session
  atoma run --agent-def ./agent.md --prompt-file ./prompt.txt

  # Resume session, write back to same file
  atoma run --agent-def ./agent.md \\
    --in-session ./sess.json --prompt-file ./msg.txt --out-session ./sess.json"
    )]
    Run {
        /// Path to the agent definition Markdown file
        ///
        /// Must contain YAML frontmatter with at least: name, description,
        /// model. MCP server configurations are optional.
        #[arg(long, value_name = "FILE", verbatim_doc_comment)]
        agent_def: PathBuf,

        /// Path to the input session JSON file
        ///
        /// Loads prior conversation history (messages array). If omitted,
        /// the agent starts with an empty session.
        #[arg(long, value_name = "FILE")]
        in_session: Option<PathBuf>,

        /// Additional session JSON files to inject only for this run
        ///
        /// Messages from these files are inserted after the system prompt and
        /// before the persistent session history, in the same order the flags
        /// are provided. System messages inside these files are ignored.
        ///
        /// Use this to keep durable session state small while letting an
        /// external workflow assemble arbitrary per-run context bundles.
        /// Injected messages are never written back to --out-session.
        #[arg(long, value_name = "FILE")]
        context_session: Vec<PathBuf>,

        /// Path to the user prompt text file
        ///
        /// The content is appended as a "user" role message.
        /// If omitted, the prompt is read from stdin (when stdin is not a TTY).
        /// If stdin is also a TTY, the agent runs with no new user message
        /// (useful for resuming a session without adding new input).
        #[arg(long, value_name = "FILE")]
        prompt_file: Option<PathBuf>,

        /// Path to write the updated session JSON
        ///
        /// If not specified, the session is discarded after the run.
        /// Atomic write: written to a .tmp file first, then renamed.
        /// Specify the same path as --in-session to update in place.
        #[arg(long, value_name = "FILE")]
        out_session: Option<PathBuf>,

        /// Path to a custom system prompt template file
        ///
        /// Overrides the built-in template. The file may contain the following
        /// placeholder variables, which are substituted at runtime:
        ///
        ///   {{AGENT_NAME}}        — agent name from frontmatter
        ///   {{AGENT_ROLE_PROMPT}} — body from Markdown or description field
        ///   {{COLLEAGUES_LIST}}   — formatted list of knows_about entries
        ///   {{AVAILABLE_TOOLS}}   — formatted list of MCP tools
        ///
        /// All variables are optional; unused ones are left as-is.
        #[arg(long, value_name = "FILE", verbatim_doc_comment)]
        template: Option<PathBuf>,

        /// Path to the tools YAML file defining MCP server configurations
        ///
        /// Required when the agent definition references any mcp_servers entries.
        /// The file maps server names to their command, args, env, and optional hooks.
        ///
        /// Example format:
        ///   filesystem:
        ///     command: npx
        ///     args: ["-y", "@modelcontextprotocol/server-filesystem", "."]
        ///     hooks:
        ///       tool_allowlist: ["filesystem__*"]
        ///       before_tool: ./scripts/fs_guard.py
        #[arg(long, value_name = "FILE", verbatim_doc_comment)]
        tools_file: Option<PathBuf>,

        /// Maximum number of LLM inference iterations before aborting
        ///
        /// Each iteration calls the LLM once. If the LLM keeps requesting tool calls
        /// without reaching a final response, the loop aborts after this many iterations.
        /// Defaults to 50.
        #[arg(long, value_name = "N", default_value_t = 50)]
        max_iterations: u32,

        /// Path to an executable invoked after each inference iteration
        ///
        /// If the script writes non-empty content to stdout, that content is
        /// appended as a `user` message before the next iteration. Empty output
        /// (or no output) is silently ignored.
        ///
        /// Environment variables available to the script:
        ///   ATOMA_AGENT      — agent name
        ///   ATOMA_ITERATION  — current iteration number (1-based)
        ///
        /// This hook is platform-agnostic. Use it to poll for new input from
        /// external systems (e.g. new GitHub issue comments) without coupling
        /// atoma to any specific platform.
        #[arg(long, value_name = "FILE", verbatim_doc_comment)]
        after_iteration_hook: Option<PathBuf>,
    },

    /// Validate an agent definition and optional tools file
    ///
    /// Parses the agent definition and checks for common configuration errors:
    ///   - Required fields (name, description, model) must be present
    ///   - knows_about entries must have corresponding .md files in the same directory
    ///   - mcp_servers entries must be present in the tools file (if provided)
    ///   - extra_body must not override reserved keys (model, messages)
    ///
    /// Exits with code 0 if valid, 1 if any errors are found.
    #[command(after_help = "EXAMPLES:
  # Validate agent definition only
  atoma validate --agent-def ./agent.md

  # Validate agent definition together with tools file
  atoma validate --agent-def ./agent.md --tools-file ./tools.yml")]
    Validate {
        /// Path to the agent definition Markdown file to validate
        #[arg(long, value_name = "FILE")]
        agent_def: PathBuf,

        /// Path to the tools YAML file (optional; required to validate mcp_servers)
        #[arg(long, value_name = "FILE")]
        tools_file: Option<PathBuf>,
    },
}
