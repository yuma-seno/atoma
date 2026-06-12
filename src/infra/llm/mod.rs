pub mod anthropic;
pub mod copilot;
pub mod openai;
pub(crate) mod shared;

use anyhow::{Context, Result};

use crate::domain::ports::LlmPort;

pub use anthropic::AnthropicClient;
pub use copilot::CopilotClient;
pub use openai::OpenAIClient;

/// Build an `LlmPort` implementation from environment variables.
///
/// Provider resolution priority:
/// 1. `provider_hint` argument (from agent definition `provider:` field)
/// 2. `ATOMA_PROVIDER` environment variable
/// 3. Auto-detection based on available credentials
pub async fn build_llm_client(
    provider_hint: Option<&str>,
) -> Result<Box<dyn LlmPort + Send + Sync>> {
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .user_agent(format!(
            "{}/{}",
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .context("Failed to create HTTP client")?;

    let provider = provider_hint
        .map(|s| s.to_string())
        .or_else(|| std::env::var("ATOMA_PROVIDER").ok())
        .unwrap_or_else(auto_detect_provider);

    tracing::info!("LLM provider: {}", provider);

    match provider.as_str() {
        "openai" => Ok(Box::new(OpenAIClient::from_env(http)?)),
        "github-copilot" => Ok(Box::new(CopilotClient::from_env(http).await?)),
        "anthropic" => Ok(Box::new(AnthropicClient::from_env(http)?)),
        other => anyhow::bail!(
            "Unknown provider '{other}'. Valid values: openai, github-copilot, anthropic"
        ),
    }
}

fn auto_detect_provider() -> String {
    let has_github = std::env::var("GITHUB_TOKEN").is_ok() || std::env::var("GH_TOKEN").is_ok();
    let has_openai = std::env::var("OPENAI_API_KEY").is_ok();
    let has_anthropic = std::env::var("ANTHROPIC_API_KEY").is_ok();

    if has_anthropic && !has_openai && !has_github {
        "anthropic".to_string()
    } else if has_github && !has_openai {
        "github-copilot".to_string()
    } else {
        "openai".to_string()
    }
}
