mod application;
mod cli;
mod domain;
mod infra;

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::EnvFilter;

use crate::application::runner::{RunDeps, RunSettings};
use crate::cli::{Cli, Command};
use crate::domain::config::OutputFormat;
use crate::domain::ports::AgentDefPort;
use crate::infra::config::{self as config_module, CliOverrides};

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
            let config = match config_module::discover_and_load() {
                Ok((_path, Some(cfg))) => Some(cfg),
                _ => None,
            };

            let output_format = match output.as_deref() {
                Some("json") => OutputFormat::Json,
                _ => OutputFormat::Text,
            };

            let resolved = config_module::resolve_run_config(
                CliOverrides {
                    agent_def,
                    tools_file,
                    max_iterations,
                    output: Some(output_format),
                },
                profile.as_deref(),
                config.as_ref(),
            )?;

            if let Some(ref cfg) = config {
                for (key, value) in &cfg.env {
                    if std::env::var(key).is_err() {
                        std::env::set_var(key, value);
                    }
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
                    template_path: template,
                    tools_file: resolved.tools_file,
                    max_iterations: resolved.max_iterations,
                    output_format: resolved.output,
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

            if let Err(err) = &result {
                if err
                    .downcast_ref::<application::runner::MaxIterationsReached>()
                    .is_some()
                {
                    std::process::exit(2);
                }
            }

            result
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
