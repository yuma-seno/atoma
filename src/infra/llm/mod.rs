pub mod anthropic;
pub mod copilot;
pub mod openai;
pub mod openai_responses;
pub(crate) mod shared;

use anyhow::{Context, Result};
use std::time::Duration;

use crate::domain::ports::LlmPort;
use crate::infra::credentials::Credentials;

pub use anthropic::AnthropicClient;
pub use copilot::CopilotClient;
pub use openai::OpenAIClient;
pub use openai_responses::OpenAIResponsesClient;

/// Ceiling on a single completion request, end to end.
///
/// This is a stall detector, not a generation budget: a provider that has not
/// finished in this long has almost always stopped responding altogether rather
/// than fallen behind. Raising it delays detection of exactly that case, so
/// prefer tuning `ATOMA_LLM_TIMEOUT` per model over changing this default.
const DEFAULT_LLM_TIMEOUT_SECS: u64 = 300;

/// Resolve the request timeout from a raw `ATOMA_LLM_TIMEOUT` value.
///
/// Absent, unparseable, and zero values all fall back to the default; a timeout
/// of zero would mean "no timeout", which is never what an operator wants here.
fn resolve_timeout_secs(raw: Option<&str>) -> u64 {
    raw.and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .unwrap_or(DEFAULT_LLM_TIMEOUT_SECS)
}

/// Build an `LlmPort` implementation from environment variables.
///
/// Provider resolution priority:
/// 1. `provider_hint` argument (from agent definition `provider:` field)
/// 2. `ATOMA_PROVIDER` environment variable
/// 3. Auto-detection based on available credentials
pub async fn build_llm_client(
    provider_hint: Option<&str>,
    credentials: &Credentials,
) -> Result<Box<dyn LlmPort + Send + Sync>> {
    let raw_timeout = std::env::var("ATOMA_LLM_TIMEOUT").ok();
    let timeout_secs = resolve_timeout_secs(raw_timeout.as_deref());
    tracing::debug!("LLM request timeout: {}s", timeout_secs);

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
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
        .unwrap_or_else(|| auto_detect_provider(credentials));

    tracing::info!("LLM provider: {}", provider);

    match provider.as_str() {
        "openai" => Ok(Box::new(OpenAIClient::from_credentials(http, credentials)?)),
        "openai-responses" => Ok(Box::new(OpenAIResponsesClient::from_credentials(
            http,
            credentials,
        )?)),
        "github-copilot" => Ok(Box::new(
            CopilotClient::from_credentials(http, credentials).await?,
        )),
        "anthropic" => Ok(Box::new(AnthropicClient::from_credentials(
            http,
            credentials,
        )?)),
        other => anyhow::bail!(
            "Unknown provider '{other}'. Valid values: openai, openai-responses, github-copilot, anthropic"
        ),
    }
}

/// Guess the provider from which credential the run was given.
///
/// Reads `credentials` rather than the environment directly, and that is the
/// whole point of the parameter. When credentials arrive in a file the
/// environment holds none of them, so an environment-based guess would answer
/// "openai" for every run — and a project configured only for Anthropic would
/// then fail with `OPENAI_API_KEY is not set`, a message about a provider it
/// never asked for.
fn auto_detect_provider(credentials: &Credentials) -> String {
    let has_github = credentials.has("ATOMA_COPILOT_TOKEN");
    let has_openai = credentials.has("OPENAI_API_KEY");
    let has_anthropic = credentials.has("ANTHROPIC_API_KEY");

    if has_anthropic && !has_openai && !has_github {
        "anthropic".to_string()
    } else if has_github && !has_openai {
        "github-copilot".to_string()
    } else {
        "openai".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_defaults_when_unset() {
        assert_eq!(resolve_timeout_secs(None), DEFAULT_LLM_TIMEOUT_SECS);
    }

    #[test]
    fn timeout_reads_an_explicit_value() {
        assert_eq!(resolve_timeout_secs(Some("900")), 900);
        assert_eq!(resolve_timeout_secs(Some("  120 ")), 120);
    }

    #[test]
    fn timeout_rejects_zero_and_garbage() {
        for raw in ["0", "", "abc", "-5", "12.5"] {
            assert_eq!(
                resolve_timeout_secs(Some(raw)),
                DEFAULT_LLM_TIMEOUT_SECS,
                "{raw:?} should fall back to the default"
            );
        }
    }
}
