mod application;
mod cli;
mod domain;
mod infra;

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::EnvFilter;

use crate::cli::{Cli, Command};
use crate::domain::ports::AgentDefPort;

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
            in_session,
            context_session,
            prompt_file,
            out_session,
            template,
            tools_file,
            max_iterations,
            after_iteration_hook,
        } => {
            let agent_def_port = infra::persistence::agent_def::FileAgentDefAdapter;
            let parsed_agent = agent_def_port.parse(&agent_def)?;
            let provider_hint = parsed_agent.frontmatter.provider.as_deref();
            let llm = infra::llm::build_llm_client(provider_hint).await?;
            let session_port = infra::persistence::session::FileSessionAdapter;
            let tool_def_port = infra::persistence::tool_def::FileToolDefAdapter;
            let mcp_factory = infra::mcp::McpRegistryFactory;

            let result = application::runner::run(
                agent_def,
                in_session,
                context_session,
                prompt_file,
                out_session,
                template,
                tools_file,
                max_iterations,
                after_iteration_hook,
                llm.as_ref(),
                &agent_def_port,
                &session_port,
                &tool_def_port,
                &mcp_factory,
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
    }
}
