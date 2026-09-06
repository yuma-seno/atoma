use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::LazyLock;

/// `--help`'s trailing section, with the providers rendered from the list itself.
///
/// Those lines used to be written out here as well as declared in `infra::llm`: the
/// same facts twice, in one crate, with nothing keeping them in step. Adding a provider
/// left it undocumented; renaming one left this text naming something that no longer
/// existed. Everything else here is prose about behaviour, which is not a second copy
/// of anything.
static ENVIRONMENT_HELP: LazyLock<String> = LazyLock::new(|| {
    format!(
        "ENVIRONMENT VARIABLES:
  One credential per provider, and the credential is what selects the provider when
  ATOMA_PROVIDER is unset. Two of them set at once is an error rather than a
  precedence: which one to use is not something the credentials decide.

{providers}
  ATOMA_PROVIDER         (optional)   Name one of the providers above instead of
                                       letting the credential decide.
  ATOMA_APP_NAME         (optional)   Application name sent to routers that attribute
                                       requests (OpenRouter). Default: atoma
  ATOMA_APP_URL          (optional)   Application URL sent with it.
  ATOMA_LLM_TIMEOUT      (optional)   Per-request LLM timeout in seconds (default: 300)
  ATOMA_HOOK_TIMEOUT     (optional)   Hook script timeout in seconds (default: 30)
  ATOMA_MCP_TIMEOUT      (optional)   MCP tool call timeout in seconds (default: 60)
  ATOMA_MCP_INIT_TIMEOUT (optional)   MCP server init timeout in seconds (default: 120)

  GitHub Copilot also accepts GITHUB_TOKEN or GH_TOKEN, which deliberately take no
  part in auto-detection: a run that talks to GitHub holds one anyway.

  Exactly one provider credential must be set, unless ATOMA_PROVIDER or an agent
  definition's `provider:` names which one to use.

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
  atoma init > atoma.toml",
        providers = crate::infra::llm::describe_providers(),
    )
});

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
    after_long_help = ENVIRONMENT_HELP.as_str()
)]
pub struct Cli {
    /// Allow a debugger to attach to this process, at the cost of its confinement
    ///
    /// By default `atoma` makes itself non-dumpable, so a tool server it spawns
    /// cannot read its environment or memory even though both run as the same
    /// user. That also stops `gdb`, `strace` and `perf` attaching without `sudo`.
    /// Pass this when debugging a run; the credentials this process holds are
    /// then readable by anything running as you.
    #[arg(long, global = true)]
    pub no_process_protection: bool,

    /// Read credentials from a JSON file instead of the environment, and delete it
    ///
    /// The file is a flat `{"NAME": "value"}` object. When given, it is the ONLY
    /// source of credentials — the environment is not consulted for them — and it
    /// is removed as soon as it has been read, before any tool server starts.
    ///
    /// This exists because a value in an environment block cannot be taken back:
    /// `/proc/<pid>/environ` keeps what was there at exec for the process's
    /// lifetime, readable by anything running as the same user. A file's exposure
    /// can be ended; an environment variable's cannot.
    ///
    /// Omit it to read credentials from the environment as usual.
    #[arg(long, global = true, value_name = "FILE")]
    pub credentials_file: Option<std::path::PathBuf>,

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

        /// Stop after N turns. Absent means no turn ceiling.
        #[arg(long, value_name = "N")]
        max_iterations: Option<u32>,

        /// Stop after N seconds. Absent means no time limit.
        ///
        /// Passed per invocation rather than kept in `atoma.toml` because it belongs to
        /// the run's circumstances, not to the agent: the same agent under a CI job with
        /// a 60-minute timeout and the same agent on a workstation want different
        /// answers, and only the caller knows which one it is.
        #[arg(long, value_name = "SECONDS")]
        max_runtime_secs: Option<u64>,
    },

    /// Validate an agent definition and optional tools file
    #[command(after_help = "EXAMPLES:
  atoma validate --agent-def ./agent.md
  atoma validate --agent-def ./agent.md --tools-file ./tools.yml
  atoma validate --agent-def ./agent.md --template ./prompt-template.md")]
    Validate {
        #[arg(long, value_name = "FILE")]
        agent_def: PathBuf,
        #[arg(long, value_name = "FILE")]
        tools_file: Option<PathBuf>,
        /// Prompt template to check for placeholders nothing substitutes
        ///
        /// Optional, like `--tools-file`: absent means the template is not checked
        /// rather than that there is none, since `run` falls back to the built-in
        /// one, which is correct by construction.
        #[arg(long, value_name = "FILE")]
        template: Option<PathBuf>,
    },

    /// Generate a default atoma.toml configuration file
    Init,
}
