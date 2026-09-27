use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::domain::ports::{DeltaHandler, LlmPort, LlmResponse};
use crate::domain::session::Message;
use crate::infra::llm::shared::{
    chat_response_to_llm, openai_compat_call, openai_compat_call_streaming,
};

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
        let reply = openai_compat_call(
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

        Ok(chat_response_to_llm(reply.body, reply.request_id))
    }

    /// Streamed, so a reasoning loop can be cut off mid-completion.
    ///
    /// This is the path every OpenAI-compatible provider takes -- OpenAI, OpenRouter,
    /// orcarouter, Copilot, and anything reached by pointing `OPENAI_BASE_URL` at it --
    /// which is why the repetition circuit breaker lives here rather than in one
    /// vendor's adapter.
    async fn chat_completion_streaming(
        &self,
        model: &str,
        messages: &[Message],
        tools: Option<&[Value]>,
        extra_body: &std::collections::HashMap<String, Value>,
        on_delta: &mut DeltaHandler<'_>,
    ) -> Result<LlmResponse> {
        openai_compat_call_streaming(
            &self.client,
            &self.base_url,
            &self.api_key,
            &self.extra_headers,
            model,
            messages,
            tools,
            extra_body,
            on_delta,
        )
        .await
    }
}
