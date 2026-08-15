use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::domain::ports::{LlmChoice, LlmPort, LlmResponse, LlmUsage};
use crate::domain::session::{Message, ToolCall, ToolCallFunction};
use crate::infra::llm::shared::{send_json_with_retry, ChatChoice, ChatResponse, Usage};

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
        let body = build_request_body(model, messages, tools, extra_body);

        tracing::debug!("Request URL: {}", url);
        tracing::debug!("Request body: {}", serde_json::to_string_pretty(&body)?);

        let raw: AnthropicResponse = send_json_with_retry("Anthropic", || {
            self.client
                .post(&url)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", ANTHROPIC_API_VERSION)
                .header("Content-Type", "application/json")
                .json(&body)
        })
        .await?;

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

/// Builds the full Anthropic Messages API request body, including
/// prompt-cache breakpoints.
///
/// Anthropic only caches a prompt when a content block is explicitly marked
/// `cache_control: {"type": "ephemeral"}` -- omitting it (the previous
/// behavior here) means EVERY request is billed as fully fresh, even when
/// the bulk of it is byte-identical to the previous call. This matters a
/// lot for this codebase: a single `atoma run` can iterate its tool-calling
/// loop up to `max_iterations` times (100-200 for some agents), and each
/// iteration re-sends the ENTIRE growing conversation so far.
///
/// Two breakpoints are set, matching Anthropic's own recommended pattern
/// for multi-turn tool-using agents:
///   1. the system prompt -- identical across every call for the same
///      agent/run (agent role + tool descriptions + colleagues), and
///      typically the largest static chunk of the prompt.
///   2. the last message in the conversation -- captures the entire
///      accumulated history up to this point. Each subsequent call within
///      the same run's iteration loop only appends new messages after this
///      point, so the cached prefix keeps growing and being reused across
///      iterations instead of being re-billed as fresh input every time.
fn build_request_body(
    model: &str,
    messages: &[Message],
    tools: Option<&[Value]>,
    extra_body: &std::collections::HashMap<String, Value>,
) -> Value {
    let (system, mut anthropic_messages) = messages_to_anthropic(messages);
    mark_last_message_cacheable(&mut anthropic_messages);

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
        body["system"] = serde_json::json!([
            { "type": "text", "text": sys, "cache_control": { "type": "ephemeral" } }
        ]);
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

    body
}

/// Attaches an ephemeral cache-control breakpoint to the LAST content block
/// of the last message. `cache_control` can only be attached to a content
/// BLOCK, not directly to a message, so a bare string `content` is first
/// converted into Anthropic's block-array form (a no-op for the model:
/// `"content": "text"` and `"content": [{"type":"text","text":"text"}]` are
/// equivalent other than allowing a `cache_control` field on the latter).
/// A no-op if `messages` is empty.
fn mark_last_message_cacheable(messages: &mut [Value]) {
    let Some(last) = messages.last_mut() else {
        return;
    };
    let Some(content) = last.get_mut("content") else {
        return;
    };
    match content {
        Value::String(s) => {
            *content = serde_json::json!([
                { "type": "text", "text": s, "cache_control": { "type": "ephemeral" } }
            ]);
        }
        Value::Array(blocks) => {
            if let Some(last_block) = blocks.last_mut().and_then(Value::as_object_mut) {
                last_block.insert(
                    "cache_control".to_string(),
                    serde_json::json!({ "type": "ephemeral" }),
                );
            }
        }
        _ => {}
    }
}

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
                let content = mcp_blocks_to_anthropic(content);
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
                let content = mcp_blocks_to_anthropic(content);
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

/// Rewrite MCP image blocks into Anthropic's shape, leaving everything else be.
///
/// MCP says `{"type":"image","data":...,"mimeType":...}`; Anthropic wants
/// `{"type":"image","source":{"type":"base64","media_type":...,"data":...}}`.
/// Content that is a plain string — every message that carries no picture —
/// passes through untouched.
///
/// Applied to user messages as well as tool results, because a picture reaches a
/// run from two directions: a tool that returns one, and a person who attached
/// one to the issue being worked on.
fn mcp_blocks_to_anthropic(content: Value) -> Value {
    let Value::Array(blocks) = content else {
        return content;
    };
    Value::Array(
        blocks
            .into_iter()
            .map(|block| {
                if block.get("type").and_then(Value::as_str) != Some("image") {
                    return block;
                }
                let (Some(data), Some(media_type)) = (
                    block.get("data").and_then(Value::as_str),
                    block.get("mimeType").and_then(Value::as_str),
                ) else {
                    // Not the shape we know how to move. Leaving it as it is
                    // sends something Anthropic will reject with a clear
                    // message, which beats silently dropping the picture.
                    return block;
                };
                serde_json::json!({
                    "type": "image",
                    "source": { "type": "base64", "media_type": media_type, "data": data },
                })
            })
            .collect::<Vec<_>>(),
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::session::ToolCall;

    fn cache_control_of(block: &Value) -> Option<&Value> {
        block.get("cache_control")
    }

    #[test]
    fn system_prompt_gets_a_cache_control_breakpoint() {
        let messages = vec![
            Message::system("you are a helpful agent"),
            Message::user("hi"),
        ];
        let body = build_request_body("claude-x", &messages, None, &Default::default());

        let system = body.get("system").expect("system should be present");
        let blocks = system.as_array().expect("system should be a block array");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["text"], "you are a helpful agent");
        assert_eq!(
            cache_control_of(&blocks[0]),
            Some(&serde_json::json!({ "type": "ephemeral" }))
        );
    }

    #[test]
    fn no_system_key_when_there_is_no_system_message() {
        let messages = vec![Message::user("hi")];
        let body = build_request_body("claude-x", &messages, None, &Default::default());
        assert!(body.get("system").is_none());
    }

    #[test]
    fn last_message_with_plain_string_content_is_converted_and_marked_cacheable() {
        let messages = vec![Message::user("first"), Message::user("second (latest)")];
        let body = build_request_body("claude-x", &messages, None, &Default::default());

        let out_messages = body["messages"].as_array().unwrap();
        assert_eq!(out_messages.len(), 2);
        // Earlier message untouched (still a plain string, no cache_control).
        assert_eq!(out_messages[0]["content"], "first");
        // Last message converted to block-array form with a cache_control breakpoint.
        let last_content = out_messages[1]["content"].as_array().unwrap();
        assert_eq!(last_content[0]["text"], "second (latest)");
        assert_eq!(
            cache_control_of(&last_content[0]),
            Some(&serde_json::json!({ "type": "ephemeral" }))
        );
    }

    #[test]
    fn tool_result_as_last_message_gets_cache_control_on_the_tool_result_block() {
        // A very common shape in real agent runs: the conversation's last
        // turn is a tool result (the model just made a tool call, the tool
        // ran, and its result was appended) -- per Anthropic's own
        // documented pattern for caching tool-using conversations, the
        // cache_control breakpoint belongs on the LAST block regardless of
        // its type, including "tool_result".
        let messages = vec![Message::tool("call_1", "issue #42: title, body...")];
        let body = build_request_body("claude-x", &messages, None, &Default::default());

        let out_messages = body["messages"].as_array().unwrap();
        assert_eq!(out_messages.len(), 1);
        assert_eq!(out_messages[0]["role"], "user");
        let blocks = out_messages[0]["content"].as_array().unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["type"], "tool_result");
        assert_eq!(blocks[0]["tool_use_id"], "call_1");
        assert_eq!(
            cache_control_of(&blocks[0]),
            Some(&serde_json::json!({ "type": "ephemeral" })),
            "cache_control must land on the tool_result block itself, not be skipped"
        );
    }

    #[test]
    fn last_message_with_existing_block_array_gets_cache_control_on_its_last_block() {
        let tool_call = ToolCall {
            id: "call_1".to_string(),
            type_: "function".to_string(),
            function: ToolCallFunction {
                name: "get_issue".to_string(),
                arguments: "{}".to_string(),
            },
        };
        let messages = vec![Message::assistant(Some("checking"), Some(vec![tool_call]))];
        let body = build_request_body("claude-x", &messages, None, &Default::default());

        let out_messages = body["messages"].as_array().unwrap();
        let blocks = out_messages[0]["content"].as_array().unwrap();
        assert_eq!(blocks.len(), 2, "expected a text block + a tool_use block");
        // cache_control lands on the LAST block only, not the first.
        assert_eq!(cache_control_of(&blocks[0]), None);
        assert_eq!(
            cache_control_of(&blocks[1]),
            Some(&serde_json::json!({ "type": "ephemeral" }))
        );
    }

    #[test]
    fn empty_messages_does_not_panic() {
        let body = build_request_body("claude-x", &[], None, &Default::default());
        assert_eq!(body["messages"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn extra_body_still_merges_alongside_cache_control_fields() {
        let mut extra = std::collections::HashMap::new();
        extra.insert("temperature".to_string(), serde_json::json!(0.5));
        let messages = vec![Message::user("hi")];
        let body = build_request_body("claude-x", &messages, None, &extra);
        assert_eq!(body["temperature"], 0.5);
    }
}

#[cfg(test)]
mod tool_image_tests {
    use super::mcp_blocks_to_anthropic;
    use serde_json::json;

    // MCP and Anthropic name the same thing differently; the picture is lost
    // unless something moves it across.
    #[test]
    fn an_mcp_image_block_becomes_an_anthropic_source_block() {
        let out = mcp_blocks_to_anthropic(json!([
            {"type": "text", "text": "Here is the screen:"},
            {"type": "image", "data": "AAAA", "mimeType": "image/png"},
        ]));
        assert_eq!(out[0]["type"], "text");
        assert_eq!(out[1]["source"]["type"], "base64");
        assert_eq!(out[1]["source"]["media_type"], "image/png");
        assert_eq!(out[1]["source"]["data"], "AAAA");
    }

    #[test]
    fn a_plain_string_result_passes_through() {
        let out = mcp_blocks_to_anthropic(json!("done"));
        assert_eq!(out, json!("done"));
    }

    // Sending something Anthropic rejects with a clear message beats dropping
    // the picture and reporting success.
    #[test]
    fn an_image_block_of_an_unknown_shape_is_left_alone() {
        let out = mcp_blocks_to_anthropic(json!([{"type": "image", "url": "http://x/y.png"}]));
        assert_eq!(out[0]["url"], "http://x/y.png");
    }
}

#[cfg(test)]
mod user_image_tests {
    use super::messages_to_anthropic;
    use crate::domain::session::Message;
    use serde_json::json;

    // The other direction a picture arrives from: attached to the issue, not
    // returned by a tool.
    #[test]
    fn a_user_message_picture_becomes_a_source_block() {
        let mut msg = Message::user("look at this");
        msg.content = Some(json!([
            {"type": "text", "text": "look at this"},
            {"type": "image", "data": "AAAA", "mimeType": "image/png"},
        ]));
        let (_, out) = messages_to_anthropic(&[msg]);
        assert_eq!(out[0]["role"], "user");
        assert_eq!(out[0]["content"][1]["source"]["media_type"], "image/png");
        assert_eq!(out[0]["content"][1]["source"]["data"], "AAAA");
    }
}
