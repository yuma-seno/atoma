use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::domain::ports::{FinishReason, LlmChoice, LlmPort, LlmResponse, LlmUsage};
use crate::domain::session::Message;
use crate::infra::llm::shared::openai_compat_call;

/// Client for any endpoint speaking OpenAI's chat-completions dialect.
///
/// Which endpoint, which credential and which extra headers all come from the
/// caller. They used to be read here — an environment variable with a default,
/// inline in the constructor — which is how one provider's attribution headers came
/// to be sent to every provider. See `Provider` in `mod.rs`.
pub struct OpenAIClient {
    pub(crate) client: reqwest::Client,
    pub(crate) base_url: String,
    pub(crate) api_key: String,
    pub(crate) extra_headers: Vec<(String, String)>,
}

impl OpenAIClient {
    pub fn new(
        client: reqwest::Client,
        base_url: String,
        api_key: String,
        extra_headers: Vec<(String, String)>,
    ) -> Self {
        OpenAIClient {
            client,
            base_url,
            api_key,
            extra_headers,
        }
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
                    // The canonical spelling is this dialect's own, so a value that
                    // does not read is a provider inventing one — `None`, and the runner
                    // says so, rather than being quietly taken for `stop`.
                    finish_reason: c
                        .finish_reason
                        .as_deref()
                        .and_then(FinishReason::from_openai),
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
