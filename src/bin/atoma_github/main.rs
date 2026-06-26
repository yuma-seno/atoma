//! atoma-github — GitHub CLI tools for Atoma workflows.
//!
//! Replaces shell scripts with a single type-safe Rust binary.

use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;

#[derive(Parser)]
#[command(
    name = "atoma-github",
    version,
    about = "GitHub CLI tools for Atoma workflows"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    CreatePr {
        #[arg(long, required = true)]
        title: String,
        #[arg(long, required = true)]
        description: String,
        #[arg(long)]
        linked_issue: Option<u64>,
        #[arg(long)]
        dispatch_agent: Option<String>,
    },
    PushCommits {
        #[arg(long, required = true)]
        pr: u64,
        #[arg(long)]
        dispatch_agent: Option<String>,
    },
    CreateSubIssue {
        #[arg(long, required = true)]
        title: String,
        #[arg(long, required = true)]
        body: String,
        #[arg(long, required = true)]
        parent_issue: u64,
        #[arg(long)]
        notify_agent: Option<String>,
        #[arg(long)]
        trigger_agent: Option<String>,
    },
    AddLabel {
        #[arg(long)]
        issue: u64,
        #[arg(long)]
        label: String,
    },
    CloseIssue {
        #[arg(long)]
        issue: u64,
        #[arg(long)]
        comment: Option<String>,
    },
    FetchEvents {
        #[arg(long, value_parser = ["issue", "pr"])]
        r#type: String,
        #[arg(long)]
        number: u64,
        #[arg(long, default_value = "30000")]
        max_diff_chars: u32,
        #[arg(long)]
        out: Option<String>,
    },
    BuildContext {
        #[arg(long)]
        events: String,
        #[arg(long)]
        agent_name: String,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        orchestration_file: Option<String>,
        #[arg(long)]
        out: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::CreatePr { title, description, linked_issue, dispatch_agent } =>
            commands::create_pr(&title, &description, linked_issue, dispatch_agent.as_deref()).await,
        Command::PushCommits { pr, dispatch_agent } =>
            commands::push_commits(pr, dispatch_agent.as_deref()).await,
        Command::CreateSubIssue { title, body, parent_issue, notify_agent, trigger_agent } =>
            commands::create_sub_issue(&title, &body, parent_issue, notify_agent.as_deref(), trigger_agent.as_deref()).await,
        Command::AddLabel { issue, label } =>
            commands::add_label(issue, &label).await,
        Command::CloseIssue { issue, comment } =>
            commands::close_issue(issue, comment.as_deref()).await,
        Command::FetchEvents { r#type, number, max_diff_chars, out } =>
            commands::fetch_events(&r#type, number, max_diff_chars, out.as_deref()).await,
        Command::BuildContext { events, agent_name, session, orchestration_file, out } =>
            commands::build_context(&events, &agent_name, session.as_deref(), orchestration_file.as_deref(), out.as_deref()).await,
    }
}
