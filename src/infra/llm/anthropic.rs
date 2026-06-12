use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::domain::ports::{LlmChoice, LlmPort, LlmResponse, LlmUsage};
use crate::domain::session::{Message, ToolCall, ToolCallFunction};
use crate::infra::llm::shared::{ChatChoice, ChatResponse, Usage};

const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";
const ANTHROPIC_API_VERSION: &str = "2023-06-01";
const ANTHROPIC_DEFAULT_MAX_TOKENS: u64 = 8192;

/// Client for Anthropic's Messages API (native, non-OpenAI-compatible wire format).
pub struct AnthropicClient {
    pub(crate) client: reqwest::Client,
    pub(crate) base_url: String,
    pub(crate) api_key: String,
}

impl AnthropicClient {
    pub fn from_env(client: reqwest::Client) -> Result<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .context("ANTHROPIC_API_KEY is required for the anthropic provider")?;
        let base_url =
            std::env::var("ANTHROPIC_BASE_URL").unwrap_or_else(|_| ANTHROPIC_BASE_URL.to_string());
        Ok(AnthropicClient {
            client,
            base_url,
            api_key,
        })
    }

    async fn call_anthropic(
        &self,
        model: &str,
        messages: &[Message],
        tools: Option<&[Value]>,
        extra_body: &std::collections::HashMap<String, Value>,
    ) -> Result<ChatResponse> {
        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));
        let (system, anthropic_messages) = messages_to_anthropic(messages);

        let max_tokens = extra_body
            .get("max_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(ANTHROPIC_DEFAULT_MAX_TOKENS);

        let mut body = serde_json::json!({
            "model": model,
            "max_tokens": max_tokens,
            "messages": anthropic_messages,
        });

        if let Some(sys) = system {
            body["system"] = Value::String(sys);
        }

        if let Some(tools) = tools {
            body["tools"] = Value::Array(tools_to_anthropic(tools));
            body["tool_choice"] = serde_json::json!({ "type": "auto" });
        }

        if let Some(obj) = body.as_object_mut() {
            for (k, v) in extra_body {
                if ![
                    "model",
                    "messages",
                    "max_tokens",
                    "system",
                    "tools",
                    "tool_choice",
                ]
                .contains(&k.as_str())
                {
                    obj.insert(k.clone(), v.clone());
                }
            }
        }

        tracing::debug!("Request URL: {}", url);
        tracing::debug!("Request body: {}", serde_json::to_string_pretty(&body)?);

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_API_VERSION)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to send Anthropic request")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            anyhow::bail!("Anthropic API error ({}): {}", status, error_text);
        }

        let raw: AnthropicResponse = response
            .json()
            .await
            .context("Failed to parse Anthropic response")?;

        Ok(anthropic_to_chat_response(raw))
    }
}

#[async_trait]
impl LlmPort for AnthropicClient {
    async fn chat_completion(
        &self,
        model: &str,
        messages: &[Message],
        tools: Option<&[Value]>,
        extra_body: &std::collections::HashMap<String, Value>,
    ) -> Result<LlmResponse> {
        let resp = self
            .call_anthropic(model, messages, tools, extra_body)
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

// ── Anthropic wire types ──────────────────────────────────────────────────────

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
    stop_reason: Option<String>,
    usage: AnthropicUsage,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
}

#[derive(Deserialize)]
struct AnthropicUsage {
    input_tokens: u64,
    output_tokens: u64,
}

// ── Anthropic translation helpers ─────────────────────────────────────────────

fn messages_to_anthropic(messages: &[Message]) -> (Option<String>, Vec<Value>) {
    let mut system_content: Option<String> = None;
    let mut out: Vec<Value> = Vec::new();

    for msg in messages {
        match msg.role.as_str() {
            "system" => {
                if let Some(Value::String(s)) = &msg.content {
                    system_content = Some(s.clone());
                }
            }
            "user" => {
                let content = msg.content.clone().unwrap_or(Value::String(String::new()));
                out.push(serde_json::json!({ "role": "user", "content": content }));
            }
            "assistant" => {
                let mut blocks: Vec<Value> = Vec::new();
                if let Some(Value::String(text)) = &msg.content {
                    if !text.is_empty() {
                        blocks.push(serde_json::json!({ "type": "text", "text": text }));
                    }
                }
                for tc in msg.tool_calls.iter().flatten() {
                    let input: Value = serde_json::from_str(&tc.function.arguments)
                        .unwrap_or(Value::Object(Default::default()));
                    blocks.push(serde_json::json!({
                        "type": "tool_use",
                        "id": tc.id,
                        "name": tc.function.name,
                        "input": input,
                    }));
                }
                if !blocks.is_empty() {
                    out.push(serde_json::json!({ "role": "assistant", "content": blocks }));
                }
            }
            "tool" => {
                let content = msg.content.clone().unwrap_or(Value::String(String::new()));
                let tool_use_id = msg.tool_call_id.as_deref().unwrap_or("unknown");
                out.push(serde_json::json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": tool_use_id,
                        "content": content,
                    }],
                }));
            }
            _ => {}
        }
    }

    (system_content, out)
}

fn tools_to_anthropic(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .map(|tool| {
            let func = tool.get("function").unwrap_or(tool);
            let name = func.get("name").and_then(Value::as_str).unwrap_or_default();
            let description = func
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let input_schema = func
                .get("parameters")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({ "type": "object", "properties": {} }));
            serde_json::json!({
                "name": name,
                "description": description,
                "input_schema": input_schema,
            })
        })
        .collect()
}

fn anthropic_to_chat_response(raw: AnthropicResponse) -> ChatResponse {
    let mut text_parts: Vec<String> = Vec::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();

    for block in raw.content {
        match block {
            AnthropicContentBlock::Text { text } => text_parts.push(text),
            AnthropicContentBlock::ToolUse { id, name, input } => {
                tool_calls.push(ToolCall {
                    id,
                    type_: "function".to_string(),
                    function: ToolCallFunction {
                        name,
                        arguments: serde_json::to_string(&input).unwrap_or_default(),
                    },
                });
            }
        }
    }

    let text = if text_parts.is_empty() {
        None
    } else {
        Some(text_parts.join(""))
    };
    let tool_calls = if tool_calls.is_empty() {
        None
    } else {
        Some(tool_calls)
    };

    let finish_reason = match raw.stop_reason.as_deref() {
        Some("end_turn") => Some("stop".to_string()),
        Some("tool_use") => Some("tool_calls".to_string()),
        Some("max_tokens") => Some("length".to_string()),
        other => other.map(|s| s.to_string()),
    };

    ChatResponse {
        choices: vec![ChatChoice {
            message: Message::assistant(text.as_deref(), tool_calls),
            finish_reason,
        }],
        usage: Some(Usage {
            prompt_tokens: raw.usage.input_tokens,
            completion_tokens: raw.usage.output_tokens,
            total_tokens: raw.usage.input_tokens + raw.usage.output_tokens,
        }),
    }
}
