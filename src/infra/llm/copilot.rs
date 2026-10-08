use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::domain::ports::{DeltaHandler, LlmPort, LlmResponse};
use crate::domain::session::Message;
use crate::infra::llm::shared::{
    chat_response_to_llm, delay_before_retry, is_transient, openai_compat_call,
    openai_compat_call_streaming, MAX_HTTP_ATTEMPTS,
};
/// Exchange a GitHub credential for a Copilot token.
///
/// Two things this did not do, and both were silent.
///
/// **It did not retry.** Every other call atoma makes goes through
/// `send_json_with_retry` or `send_sse_with_retry`; this one built its own `reqwest` call
/// and sent it once. A 429 or a 5xx from GitHub therefore ended the run before the first
/// inference, on a request that would have succeeded a second later — and it is the one
/// call a Copilot run cannot do without, because nothing else reaches the inference
/// endpoint.
///
/// **It did not read a `Retry-After`.** GitHub's API sends one on a rate limit, and the
/// helper exists now, so the two paths agree about what a provider's hint means.
///
/// What it still does not do is read Copilot's own error `code`, because there is no
/// published vocabulary to read it against — the endpoint is internal, unlike the four
/// documented providers. The status and the body text go into the message instead, which
/// is what a person debugging this has to work from.
async fn exchange_copilot_token(client: &reqwest::Client, github_token: &str) -> Result<String> {
    const COPILOT_AUTH_URL: &str = "https://api.github.com/copilot_internal/v2/token";

    tracing::debug!("Exchanging GitHub token for Copilot token");

    #[derive(Deserialize)]
    struct TokenResponse {
        token: String,
    }

    for attempt in 1..=MAX_HTTP_ATTEMPTS {
        let response = match client
            .get(COPILOT_AUTH_URL)
            .header("Authorization", format!("token {}", github_token))
            .header("Accept", "application/json")
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) if attempt < MAX_HTTP_ATTEMPTS && is_transient(&error) => {
                delay_before_retry(attempt, None, &format!("Copilot token exchange: {error}"))
                    .await;
                continue;
            }
            Err(error) => return Err(error).context("Failed to request GitHub Copilot token"),
        };

        if !response.status().is_success() {
            let status = response.status();
            let retryable =
                status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error();
            let hint = response.headers().clone();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            if attempt < MAX_HTTP_ATTEMPTS && retryable {
                delay_before_retry(
                    attempt,
                    Some(&hint),
                    &format!("Copilot token exchange: API error ({status}): {error_text}"),
                )
                .await;
                continue;
            }
            // 404 and 403 are the two a person will actually meet here, and they have
            // different causes: no Copilot subscription, or a GitHub token without the
            // scope. The status is kept so the sentence can say which.
            anyhow::bail!(
                "Failed to obtain GitHub Copilot token ({}): {}\n\
                 Ensure your GitHub token has the 'copilot' scope and a Copilot subscription is active.",
                status,
                error_text
            );
        }

        return match response.json::<TokenResponse>().await {
            Ok(resp) => {
                tracing::debug!("Successfully obtained Copilot token");
                Ok(resp.token)
            }
            Err(error) if attempt < MAX_HTTP_ATTEMPTS && is_transient(&error) => {
                // A truncated or half-read body is the one parse failure worth another
                // request: the exchange itself may have succeeded and only the read
                // failed. A shape mismatch would fail identically, so it falls through.
                delay_before_retry(
                    attempt,
                    None,
                    &format!("Copilot token exchange: unreadable response body: {error}"),
                )
                .await;
                continue;
            }
            Err(error) => Err(error).context("Failed to parse GitHub Copilot token response"),
        };
    }

    // Unreachable in practice: the loop returns on the last attempt either way. Present
    // rather than `unreachable!()` so a future edit that adds an attempt cannot panic a
    // run on the path that starts it.
    anyhow::bail!("Failed to obtain GitHub Copilot token: no attempt succeeded")
}

/// Client for GitHub Copilot (OpenAI-compatible wire protocol with Copilot auth).
pub struct CopilotClient {
    pub(crate) client: reqwest::Client,
    pub(crate) base_url: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) copilot_token: String,
}

impl CopilotClient {
    /// Exchange a GitHub credential for a Copilot token, then hold both it and the
    /// endpoint.
    ///
    /// The endpoint is a parameter now. It was a `const` — the only provider whose
    /// address could not be changed, for no reason anyone had written down.
    ///
    /// The fallback to `GITHUB_TOKEN`/`GH_TOKEN` stays, and deliberately does not
    /// take part in provider detection: a run that talks to GitHub has one of those
    /// anyway, so detecting Copilot from them would make every run ambiguous. They
    /// work when this provider was asked for by name.
    pub async fn connect(
        client: reqwest::Client,
        base_url: String,
        headers: Vec<(String, String)>,
        github_token: String,
    ) -> Result<Self> {
        let copilot_token = exchange_copilot_token(&client, &github_token).await?;
        Ok(CopilotClient {
            client,
            base_url,
            headers,
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
        let reply = openai_compat_call(
            &self.client,
            &self.base_url,
            &self.copilot_token,
            &self.headers,
            model,
            messages,
            tools,
            extra_body,
        )
        .await?;

        Ok(chat_response_to_llm(reply.body, reply.request_id))
    }

    /// Streamed, on the same terms as `OpenAIClient`: this endpoint speaks the same
    /// dialect, so it gets the same interruption.
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
            &self.copilot_token,
            &self.headers,
            model,
            messages,
            tools,
            extra_body,
            on_delta,
        )
        .await
    }
}
