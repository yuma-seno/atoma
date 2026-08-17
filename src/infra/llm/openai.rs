use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;

use crate::domain::ports::{LlmChoice, LlmPort, LlmResponse, LlmUsage};
use crate::domain::session::Message;
use crate::infra::credentials::Credentials;
use crate::infra::llm::shared::openai_compat_call;

pub(crate) const DEFAULT_OPENAI_BASE_URL: &str = "https://openrouter.ai/api/v1";

/// Client for OpenAI and OpenAI-compatible endpoints (e.g. OpenRouter).
pub struct OpenAIClient {
    pub(crate) client: reqwest::Client,
    pub(crate) base_url: String,
    pub(crate) api_key: String,
    pub(crate) extra_headers: Vec<(String, String)>,
}

impl OpenAIClient {
    pub fn from_credentials(client: reqwest::Client, credentials: &Credentials) -> Result<Self> {
        let api_key = credentials.get("OPENAI_API_KEY").context(
            "OPENAI_API_KEY is not set. Set OPENAI_API_KEY for OpenAI-compatible providers,\n\
             ANTHROPIC_API_KEY for Anthropic,\n\
             or ATOMA_COPILOT_TOKEN for GitHub Copilot.\n\
             Use ATOMA_PROVIDER to select explicitly.",
        )?;
        let base_url = std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| DEFAULT_OPENAI_BASE_URL.to_string());
        let app_name =
            std::env::var("OPENAI_APP_NAME").unwrap_or_else(|_| env!("CARGO_PKG_NAME").to_string());
        let app_url = std::env::var("OPENAI_APP_URL")
            .unwrap_or_else(|_| env!("CARGO_PKG_REPOSITORY").to_string());

        Ok(OpenAIClient {
            client,
            base_url,
            api_key,
            extra_headers: vec![
                ("X-Title".to_string(), app_name.clone()),
                ("X-OpenRouter-Title".to_string(), app_name),
                ("HTTP-Referer".to_string(), app_url),
            ],
        })
    }
}

#[async_trait]
impl LlmPort for OpenAIClient {
    async fn chat_completion(
        &self,
        model: &str,
        messages: &[Message],
        tools: Option<&[Value]>,
        extra_body: &std::collections::HashMap<String, Value>,
    ) -> Result<LlmResponse> {
        let resp = openai_compat_call(
            &self.client,
            &self.base_url,
            &self.api_key,
            &self.extra_headers,
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
