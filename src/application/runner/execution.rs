//! Inference loop and tool execution logic.

use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::collections::HashMap;

use crate::domain::ports::{LlmPort, LlmUsage, McpPort};
use crate::domain::session::{Message, Session, ToolCall};
/// Sentinel error returned when the inference loop runs out of iterations.
#[derive(Debug)]
pub struct MaxIterationsReached(pub u32);

impl std::fmt::Display for MaxIterationsReached {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Inference loop exceeded maximum iterations ({})", self.0)
    }
}

impl std::error::Error for MaxIterationsReached {}

/// Result of the inference loop.
pub enum InferenceResult {
    /// Normal completion with final text response.
    Completed { text: String, usage: LlmUsage },
    /// A tool requested session suspension (session_ends: true).
    /// The session has been saved; caller should exit cleanly.
    #[allow(dead_code)]
    SessionEnded { usage: LlmUsage },
}

/// Execute all tool calls from an LLM response, appending results to the session.
///
/// Returns `true` if any tool requested session suspension.
///
/// Tool calls are executed sequentially. Parallel execution requires McpPort
/// to adopt `&self` with internal synchronization.
async fn execute_tool_calls(
    agent_name: &str,
    tool_calls: &[ToolCall],
    session: &mut Session,
    mcp_registry: &mut Option<Box<dyn McpPort + Send>>,
) -> Result<bool> {
    session
        .messages
        .push(Message::assistant(None, Some(tool_calls.to_vec())));

    let registry = mcp_registry
        .as_mut()
        .context("LLM requested tool calls but no MCP servers are configured")?;

    let mut session_ends = false;

    for tool_call in tool_calls {
        let tool_name = &tool_call.function.name;

        let arguments = match serde_json::from_str::<Value>(&tool_call.function.arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!(
                    "Invalid JSON arguments for tool '{}': {}\nRaw: {}",
                    tool_name, e, tool_call.function.arguments,
                );
                tracing::error!("{}", msg);
                session.messages.push(Message::tool(&tool_call.id, &msg));
                continue;
            }
        };

        tracing::info!("Executing tool: {} (id: {})", tool_name, tool_call.id);

        match registry
            .call_tool_with_hooks(agent_name, tool_name, &arguments)
            .await
        {
            Ok(result) => {
                tracing::debug!(
                    "Tool '{}' result ({} chars)",
                    tool_name,
                    result.content.len()
                );
                session
                    .messages
                    .push(Message::tool(&tool_call.id, &result.content));
                if result.session_ends {
                    tracing::info!(
                        "Tool '{}' requested session suspension — will end after this iteration",
                        tool_name
                    );
                    session_ends = true;
                }
            }
            Err(e) => {
                let msg = format!("Error: {}", e);
                tracing::error!("Tool '{}' failed: {}", tool_name, e);
                session.messages.push(Message::tool(&tool_call.id, &msg));
            }
        }
    }

    Ok(session_ends)
}

/// Run the inference loop: call LLM, handle tool calls or final response.
#[allow(clippy::too_many_arguments)]
pub async fn inference_loop(
    llm_client: &dyn LlmPort,
    agent_name: &str,
    model: &str,
    session: &mut Session,
    tools: Option<&[Value]>,
    extra_body: &HashMap<String, Value>,
    mcp_registry: &mut Option<Box<dyn McpPort + Send>>,
    max_iterations: u32,
) -> Result<InferenceResult> {
    let mut total_usage = LlmUsage::default();

    for iteration in 1..=max_iterations {
        tracing::info!(
            "Inference iteration {}/{} ({} messages in session)",
            iteration,
            max_iterations,
            session.messages.len()
        );

        let response = llm_client
            .chat_completion(model, &session.messages, tools, extra_body)
            .await?;

        if let Some(u) = response.usage {
            total_usage.prompt_tokens += u.prompt_tokens;
            total_usage.completion_tokens += u.completion_tokens;
            total_usage.total_tokens += u.total_tokens;
        }

        let choice = response
            .choices
            .into_iter()
            .next()
            .context("No choices returned from LLM")?;

        let finish_reason = choice.finish_reason.as_deref().unwrap_or("stop").to_owned();
        let tool_calls = choice.message.tool_calls;
        let content = choice.message.content;

        if let Some(calls) = tool_calls {
            if calls.is_empty() {
                tracing::warn!("LLM returned empty tool_calls array — continuing");
                continue;
            }
            if finish_reason != "tool_calls" {
                tracing::warn!(
                    "LLM returned finish_reason '{}' with tool_calls — processing anyway",
                    finish_reason
                );
            }

            tracing::info!("LLM requested {} tool call(s)", calls.len());
            let session_ends =
                execute_tool_calls(agent_name, &calls, session, mcp_registry).await?;

            if session_ends {
                tracing::info!("Tool requested session suspension; ending inference loop");
                return Ok(InferenceResult::SessionEnded { usage: total_usage });
            }
        } else {
            match finish_reason.as_str() {
                "stop" | "end_turn" => {
                    let text = content
                        .as_ref()
                        .and_then(|c| c.as_str())
                        .unwrap_or("")
                        .to_owned();
                    if text.is_empty() {
                        bail!("LLM returned empty response (finish_reason: stop)");
                    }
                    session.messages.push(Message::assistant(Some(&text), None));
                    tracing::info!("LLM returned final response ({} chars)", text.len());
                    return Ok(InferenceResult::Completed {
                        text,
                        usage: total_usage,
                    });
                }
                "length" => {
                    let text = content
                        .as_ref()
                        .and_then(|c| c.as_str())
                        .unwrap_or("")
                        .to_owned();
                    session.messages.push(Message::assistant(Some(&text), None));
                    tracing::warn!(
                        "LLM response truncated (finish_reason: length, {} chars)",
                        text.len()
                    );
                    return Ok(InferenceResult::Completed {
                        text,
                        usage: total_usage,
                    });
                }
                "content_filter" => bail!("LLM response was blocked by content filter"),
                "tool_calls" => {
                    bail!("LLM returned finish_reason 'tool_calls' but no tool_calls in message")
                }
                other => bail!("LLM returned unexpected finish_reason: {}", other),
            }
        }
    }

    Err(anyhow::Error::new(MaxIterationsReached(max_iterations)))
}
