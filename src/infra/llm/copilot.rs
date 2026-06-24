use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;

use crate::domain::ports::{LlmChoice, LlmPort, LlmResponse, LlmUsage};
use crate::domain::session::Message;
use crate::infra::llm::shared::{exchange_copilot_token, openai_compat_call};

const COPILOT_BASE_URL: &str = "https://api.githubcopilot.com";

/// Client for GitHub Copilot (OpenAI-compatible wire protocol with Copilot auth).
pub struct CopilotClient {
    pub(crate) client: reqwest::Client,
    pub(crate) copilot_token: String,
}

impl CopilotClient {
    pub async fn from_env(client: reqwest::Client) -> Result<Self> {
        let github_token = std::env::var("ATOMA_COPILOT_TOKEN")
            .or_else(|_| std::env::var("GITHUB_TOKEN"))
            .or_else(|_| std::env::var("GH_TOKEN"))
            .context("ATOMA_COPILOT_TOKEN, GITHUB_TOKEN, or GH_TOKEN is required for the github-copilot provider")?;
        let copilot_token = exchange_copilot_token(&client, &github_token).await?;
        Ok(CopilotClient {
            client,
            copilot_token,
        })
    }
}

#[async_trait]
impl LlmPort for CopilotClient {
    async fn chat_completion(
        &self,
        model: &str,
        messages: &[Message],
        tools: Option<&[Value]>,
        extra_body: &std::collections::HashMap<String, Value>,
    ) -> Result<LlmResponse> {
        let pkg_id = format!("{}/{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        let headers = vec![
            ("Editor-Version".to_string(), pkg_id.clone()),
            ("Editor-Plugin-Version".to_string(), pkg_id),
            (
                "Copilot-Integration-Id".to_string(),
                "atoma-cli".to_string(),
            ),
            (
                "Openai-Intent".to_string(),
                "conversation-panel".to_string(),
            ),
        ];
        let resp = openai_compat_call(
            &self.client,
            COPILOT_BASE_URL,
            &self.copilot_token,
            &headers,
            model,
            messages,
            tools,
            extra_body,
        )
        .await?;

        Ok(LlmResponse {
            choices: resp
                .choices
                .into_iter()
                .map(|c| LlmChoice {
                    message: c.message,
                    finish_reason: c.finish_reason,
                })
                .collect(),
            usage: resp.usage.map(|u| LlmUsage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                total_tokens: u.total_tokens,
            }),
        })
    }
}
