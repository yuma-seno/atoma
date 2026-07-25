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
  ATOMA_COPILOT_TOKEN     (required*) GitHub PAT with `copilot` scope for GitHub Copilot mode
                                       (fallback: GITHUB_TOKEN or GH_TOKEN)
  ANTHROPIC_API_KEY      (required*)  API key for Anthropic (Claude) models
  ANTHROPIC_BASE_URL     (optional)   Anthropic API base URL (default: https://api.anthropic.com)
  ATOMA_PROVIDER         (optional)   Force provider: 'openai', 'github-copilot', or 'anthropic'
                                       Auto-detected when unset.
  ATOMA_HOOK_TIMEOUT     (optional)   Hook script timeout in seconds (default: 30)
  ATOMA_MCP_TIMEOUT      (optional)   MCP tool call timeout in seconds (default: 60)
  ATOMA_MCP_INIT_TIMEOUT (optional)   MCP server init timeout in seconds (default: 120)

  * One of OPENAI_API_KEY, ATOMA_COPILOT_TOKEN, or ANTHROPIC_API_KEY must be provided.

CONFIGURATION FILE:
  atoma.toml can be placed in the current directory or any ancestor directory.
  See 'atoma init' for a template.
  Priority: CLI argument > atoma.toml profile > atoma.toml defaults.

PROVIDER SELECTION:
  Priority: agent definition 'provider:' field > ATOMA_PROVIDER env > auto-detect

EXAMPLES:
  atoma run --agent-def ./agent.md --prompt-file ./prompt.txt
  atoma run --profile review --in-session ./sess.json
  atoma run --agent-def ./agent.md --output json --prompt-file ./task.txt
  atoma init > atoma.toml"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Run an agent against a prompt
    #[command(after_help = "EXAMPLES:
  echo \"Hello\" | atoma run --agent-def ./agent.md
  atoma run --agent-def ./agent.md --prompt-file ./prompt.txt
  atoma run --agent-def ./agent.md --in-session ./sess.json --out-session ./sess.json
  atoma run --profile review --in-session ./sess.json")]
    Run {
        /// Path to the agent definition Markdown file
        #[arg(long, value_name = "FILE")]
        agent_def: Option<PathBuf>,

        /// Use a named profile from atoma.toml
        #[arg(long, value_name = "NAME")]
        profile: Option<String>,

        /// Output format: text (default) or json
        #[arg(long, value_name = "FORMAT")]
        output: Option<String>,

        #[arg(long, value_name = "FILE")]
        in_session: Option<PathBuf>,

        #[arg(long, value_name = "FILE")]
        prompt_file: Option<PathBuf>,

        #[arg(long, value_name = "FILE")]
        out_session: Option<PathBuf>,

        #[arg(long, value_name = "FILE")]
        template: Option<PathBuf>,

        #[arg(long, value_name = "FILE")]
        tools_file: Option<PathBuf>,

        /// Directory containing dynamically loadable skill Markdown files
        #[arg(long, value_name = "DIR")]
        skills_dir: Option<PathBuf>,

        #[arg(long, value_name = "N")]
        max_iterations: Option<u32>,
    },

    /// Validate an agent definition and optional tools file
    #[command(after_help = "EXAMPLES:
  atoma validate --agent-def ./agent.md
  atoma validate --agent-def ./agent.md --tools-file ./tools.yml")]
    Validate {
        #[arg(long, value_name = "FILE")]
        agent_def: PathBuf,
        #[arg(long, value_name = "FILE")]
        tools_file: Option<PathBuf>,
    },

    /// Generate a default atoma.toml configuration file
    Init,
}
