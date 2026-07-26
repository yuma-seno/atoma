use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;

use crate::domain::session::Message;

const MAX_HTTP_ATTEMPTS: u8 = 3;

fn is_transient(error: &reqwest::Error) -> bool {
    error.is_timeout() || error.is_connect() || error.is_body() || error.is_decode()
}

async fn retry_delay(attempt: u8, reason: &str) {
    tracing::warn!(
        "Transient LLM HTTP failure on attempt {}/{}: {}. Retrying.",
        attempt,
        MAX_HTTP_ATTEMPTS,
        reason,
    );
    tokio::time::sleep(Duration::from_millis(250 * u64::from(attempt))).await;
}

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

    let mut response_text = None;
    for attempt in 1..=MAX_HTTP_ATTEMPTS {
        let mut request = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json");

        for (name, value) in extra_headers {
            request = request.header(name.as_str(), value.as_str());
        }

        let response = match request.json(&body).send().await {
            Ok(response) => response,
            Err(error) if attempt < MAX_HTTP_ATTEMPTS && is_transient(&error) => {
                retry_delay(attempt, &error.to_string()).await;
                continue;
            }
            Err(error) => return Err(error).context("Failed to send chat completion request"),
        };

        if !response.status().is_success() {
            let status = response.status();
            let retryable =
                status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            if attempt < MAX_HTTP_ATTEMPTS && retryable {
                retry_delay(attempt, &format!("LLM API error ({status}): {error_text}")).await;
                continue;
            }
            anyhow::bail!("LLM API error ({}): {}", status, error_text);
        }

        match response.text().await {
            Ok(text) => {
                response_text = Some(text);
                break;
            }
            Err(error) if attempt < MAX_HTTP_ATTEMPTS && is_transient(&error) => {
                retry_delay(attempt, &error.to_string()).await;
            }
            Err(error) => return Err(error).context("Failed to read response body"),
        }
    }
    let text = response_text.context("LLM HTTP retry loop exhausted without a response")?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn retries_when_a_success_response_body_is_truncated() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let server_requests = Arc::clone(&requests);

        let server = tokio::spawn(async move {
            for attempt in 1..=2 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = vec![0; 8192];
                let _ = stream.read(&mut request).await.unwrap();
                server_requests.fetch_add(1, Ordering::SeqCst);

                if attempt == 1 {
                    stream
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{",
                        )
                        .await
                        .unwrap();
                } else {
                    let body = r#"{"choices":[{"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],"usage":null}"#;
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body,
                    );
                    stream.write_all(response.as_bytes()).await.unwrap();
                }
            }
        });

        let response = openai_compat_call(
            &reqwest::Client::new(),
            &format!("http://{address}"),
            "test-key",
            &[],
            "test-model",
            &[Message::user("hello")],
            None,
            &HashMap::new(),
        )
        .await
        .unwrap();

        server.await.unwrap();
        assert_eq!(requests.load(Ordering::SeqCst), 2);
        assert_eq!(response.choices.len(), 1);
    }
}
