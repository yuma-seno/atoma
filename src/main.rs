mod application;
mod cli;
mod domain;
mod infra;

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::EnvFilter;

use crate::application::runner::{CompletionReason, RunDeps, RunOutcome, RunSettings};
use crate::cli::{Cli, Command};
use crate::domain::ports::AgentDefPort;
use crate::infra::config::{self as config_module, CliOverrides, OutputFormat};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Run {
            agent_def,
            profile,
            output,
            in_session,
            prompt_file,
            out_session,
            template,
            tools_file,
            max_iterations,
        } => {
            let config = config_module::discover_and_load()?.1;

            let output_override = match output.as_deref() {
                Some("json") => Some(OutputFormat::Json),
                Some("text") => Some(OutputFormat::Text),
                Some(other) => anyhow::bail!("Unsupported output format: {}", other),
                None => None,
            };

            let resolved = config_module::resolve_run_config(
                CliOverrides {
                    agent_def,
                    tools_file,
                    template,
                    max_iterations,
                    output: output_override,
                },
                profile.as_deref(),
                config.as_ref(),
            )?;
            let output_format = resolved.output.clone();

            for (key, value) in &resolved.env {
                if std::env::var(key).is_err() {
                    std::env::set_var(key, value);
                }
            }

            let agent_def_port = infra::persistence::agent_def::FileAgentDefAdapter;
            let parsed_agent = AgentDefPort::parse(&agent_def_port, &resolved.agent_def)?;
            let provider_hint = parsed_agent.frontmatter.provider.as_deref();
            let llm = infra::llm::build_llm_client(provider_hint).await?;
            let session_port = infra::persistence::session::FileSessionAdapter;
            let tool_def_port = infra::persistence::tool_def::FileToolDefAdapter;
            let mcp_factory = infra::mcp::McpRegistryFactory;

            let result = application::runner::run(
                RunSettings {
                    agent_def_path: resolved.agent_def,
                    in_session,
                    prompt_file,
                    out_session,
                    template_path: resolved.template,
                    tools_file: resolved.tools_file,
                    max_iterations: resolved.max_iterations,
                },
                RunDeps {
                    llm: llm.as_ref(),
                    agent_def: &agent_def_port,
                    session: &session_port,
                    tool_def: &tool_def_port,
                    mcp_factory: &mcp_factory,
                },
            )
            .await;

            let outcome = match result {
                Ok(outcome) => outcome,
                Err(err)
                    if err
                        .downcast_ref::<application::runner::MaxIterationsReached>()
                        .is_some() =>
                {
                    std::process::exit(2);
                }
                Err(err) => return Err(err),
            };

            if let RunOutcome::Completed {
                text,
                usage,
                reason,
                session_path,
            } = outcome
            {
                match output_format {
                    OutputFormat::Text => println!("{}", text),
                    OutputFormat::Json => {
                        let output = serde_json::json!({
                            "response": text,
                            "usage": {
                                "prompt_tokens": usage.prompt_tokens,
                                "completion_tokens": usage.completion_tokens,
                                "total_tokens": usage.total_tokens,
                            },
                            "finish_reason": match reason {
                                CompletionReason::Stop => "stop",
                                CompletionReason::Length => "length",
                            },
                            "session_path": session_path.map(|p| p.to_string_lossy().to_string()),
                        });
                        println!("{}", serde_json::to_string(&output)?);
                    }
                }
            }

            Ok(())
        }
        Command::Validate {
            agent_def,
            tools_file,
        } => {
            let agent_def_port = infra::persistence::agent_def::FileAgentDefAdapter;
            let tool_def_port = infra::persistence::tool_def::FileToolDefAdapter;
            application::validator::validate(agent_def, tools_file, &agent_def_port, &tool_def_port)
        }
        Command::Init => {
            let template = config_module::generate_default_config();
            println!("{}", template);
            Ok(())
        }
    }
}
