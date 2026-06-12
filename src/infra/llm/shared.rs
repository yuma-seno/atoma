use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;

use crate::domain::session::Message;

/// Shared HTTP response types (OpenAI-compatible wire format).
#[derive(Debug, Deserialize)]
pub struct ChatResponse {
    pub choices: Vec<ChatChoice>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
pub struct ChatChoice {
    pub message: Message,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone, Copy)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

/// Shared OpenAI-compatible HTTP call used by OpenAI and Copilot providers.
#[allow(clippy::too_many_arguments)]
pub async fn openai_compat_call(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    extra_headers: &[(String, String)],
    model: &str,
    messages: &[Message],
    tools: Option<&[Value]>,
    extra_body: &std::collections::HashMap<String, Value>,
) -> Result<ChatResponse> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let llm_messages: Vec<Value> = messages.iter().map(|m| m.to_llm_value()).collect();
    let mut body = serde_json::json!({
        "model": model,
        "messages": llm_messages,
    });

    if let Some(tools) = tools {
        body["tools"] = serde_json::json!(tools);
        body["tool_choice"] = serde_json::json!("auto");
    }

    if let Some(obj) = body.as_object_mut() {
        for (k, v) in extra_body {
            if k != "model" && k != "messages" {
                obj.insert(k.clone(), v.clone());
            }
        }
    }

    tracing::debug!("Request URL: {}", url);
    tracing::debug!("Request body: {}", serde_json::to_string_pretty(&body)?);

    let mut request = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json");

    for (name, value) in extra_headers {
        request = request.header(name.as_str(), value.as_str());
    }

    let response = request
        .json(&body)
        .send()
        .await
        .context("Failed to send chat completion request")?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "unknown error".to_string());
        anyhow::bail!("LLM API error ({}): {}", status, error_text);
    }

    let text = response
        .text()
        .await
        .context("Failed to read response body")?;

    // Some providers (e.g. OpenRouter) return HTTP 200 with an error object.
    // Detect this and provide a clear error message.
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
        if let Some(error_obj) = val.get("error") {
            let msg = error_obj
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown provider error");
            let code = error_obj
                .get("code")
                .and_then(|c| c.as_i64())
                .map(|c| format!(" (code: {})", c))
                .unwrap_or_default();
            anyhow::bail!("LLM provider error{}: {}", code, msg);
        }
    }

    let chat_resp: ChatResponse = serde_json::from_str(&text)
        .context("Failed to parse chat completion response. The model may be unavailable or returning an unexpected format.")?;

    Ok(chat_resp)
}

/// Exchange a GitHub PAT for a short-lived GitHub Copilot API token.
pub async fn exchange_copilot_token(
    client: &reqwest::Client,
    github_token: &str,
) -> Result<String> {
    const COPILOT_AUTH_URL: &str = "https://api.github.com/copilot_internal/v2/token";

    tracing::debug!("Exchanging GitHub token for Copilot token");

    #[derive(Deserialize)]
    struct TokenResponse {
        token: String,
    }

    let response = client
        .get(COPILOT_AUTH_URL)
        .header("Authorization", format!("token {}", github_token))
        .header("Accept", "application/json")
        .send()
        .await
        .context("Failed to request GitHub Copilot token")?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "unknown error".to_string());
        anyhow::bail!(
            "Failed to obtain GitHub Copilot token ({}): {}\n\
             Ensure your GitHub token has the 'copilot' scope and a Copilot subscription is active.",
            status,
            error_text
        );
    }

    let resp: TokenResponse = response
        .json()
        .await
        .context("Failed to parse GitHub Copilot token response")?;

    tracing::debug!("Successfully obtained Copilot token");
    Ok(resp.token)
}
