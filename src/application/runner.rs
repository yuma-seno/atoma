use anyhow::{bail, Context, Result};
use std::io::IsTerminal;
use std::path::PathBuf;

use crate::domain::config::OutputFormat;
use crate::domain::ports::{
    AgentDefPort, LlmPort, LlmUsage, McpFactory, McpPort, SessionPort, ToolDefPort,
};
use crate::domain::session::{Message, Session, ToolCall};
use crate::infra::{hooks, template};

const TRANSIENT_CONTEXT_FLAG: &str = "transient_context";
const TRANSIENT_CONTEXT_LAYER: &str = "context-session";

/// Sentinel error returned when the inference loop runs out of iterations.
/// Distinguished from real errors so the caller can exit with code 2 (soft stop).
#[derive(Debug)]
pub struct MaxIterationsReached(pub u32);

impl std::fmt::Display for MaxIterationsReached {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Inference loop exceeded maximum iterations ({})", self.0)
    }
}

impl std::error::Error for MaxIterationsReached {}

/// Run the agent: parse agent def, load session, connect MCP, run inference loop, save session.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    agent_def_path: PathBuf,
    in_session: Option<PathBuf>,
    context_sessions: Vec<PathBuf>,
    prompt_file: Option<PathBuf>,
    out_session: Option<PathBuf>,
    template_path: Option<PathBuf>,
    tools_file: Option<PathBuf>,
    max_iterations: u32,
    after_iteration_hook: Option<PathBuf>,
    output_format: OutputFormat,
    llm: &dyn LlmPort,
    agent_def_port: &dyn AgentDefPort,
    session_port: &dyn SessionPort,
    tool_def_port: &dyn ToolDefPort,
    mcp_factory: &dyn McpFactory,
) -> Result<()> {
    // 1. Parse agent definition
    let parsed_agent = agent_def_port
        .parse(&agent_def_path)
        .context("Failed to parse agent definition")?;
    let agent = &parsed_agent.frontmatter;

    tracing::info!("Loaded agent: {} (model: {})", agent.name, agent.model);

    // 2. Read or create session
    let mut session = match in_session.as_ref() {
        Some(path) => session_port.load(path)?,
        None => Session::default(),
    };

    session
        .messages
        .retain(|message| !is_transient_context_message(message));

    tracing::info!("Session has {} messages", session.messages.len());

    let transient_context_messages =
        load_transient_context_messages(&context_sessions, session_port)?;

    // 3. Connect to MCP servers and discover tools
    let mut mcp_registry: Option<Box<dyn McpPort + Send>> = if agent.mcp_servers.is_empty() {
        None
    } else {
        let tools_path = tools_file
            .as_ref()
            .context("Agent has mcp_servers configured but --tools-file was not specified")?;
        let tools_map = tool_def_port.load(tools_path)?;

        let tool_defs: Vec<_> = agent
            .mcp_servers
            .iter()
            .map(|name| {
                tools_map.get(name).cloned().with_context(|| {
                    format!("Tool '{}' not found in tools file: {:?}", name, tools_path)
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let reg = mcp_factory.build(&tool_defs).await?;
        tracing::info!(
            "Connected to {} MCP server(s), discovered {} tool(s)",
            tool_defs.len(),
            reg.tool_definitions().len()
        );
        Some(reg)
    };

    let tool_definitions = mcp_registry
        .as_ref()
        .map(|r| r.tool_definitions())
        .unwrap_or_default();

    let tool_descriptions: Vec<String> = tool_definitions
        .iter()
        .map(|t| {
            let name = t
                .pointer("/function/name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let desc = t
                .pointer("/function/description")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            format!("- `{}`: {}", name, desc)
        })
        .collect();

    // 5. Build system prompt
    let custom_template: Option<String> = if let Some(ref path) = template_path {
        let t = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read template file: {:?}", path))?;
        Some(t)
    } else {
        None
    };
    let working_dir = std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| ".".to_string());

    let agent_def_dir = agent_def_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let colleagues: Vec<(String, String)> = agent
        .knows_about
        .iter()
        .map(|name| {
            let candidate = agent_def_dir.join(format!("{}.md", name));
            let parsed = agent_def_port.parse(&candidate).with_context(|| {
                format!(
                    "Agent '{}' listed in knows_about but its definition was not found at: {:?}",
                    name, candidate
                )
            })?;
            Ok((name.clone(), parsed.frontmatter.description))
        })
        .collect::<Result<Vec<_>>>()?;

    let system_prompt = template::build_system_prompt(
        &parsed_agent,
        &tool_descriptions,
        custom_template.as_deref(),
        &working_dir,
        &colleagues,
    );
    tracing::debug!("System prompt:\n{}", system_prompt);

    // 6. Replace system message with a fresh one
    session.messages.retain(|m| m.role != "system");
    session.messages.insert(0, Message::system(&system_prompt));
    if !transient_context_messages.is_empty() {
        tracing::info!(
            "Injecting {} transient context message(s)",
            transient_context_messages.len()
        );
        session.messages.splice(1..1, transient_context_messages);
    }

    // 7. Resolve user prompt: --prompt-file > stdin (non-TTY) > none
    let prompt_text: Option<String> = if let Some(ref prompt_path) = prompt_file {
        let text = std::fs::read_to_string(prompt_path)
            .with_context(|| format!("Failed to read prompt file: {:?}", prompt_path))?;
        tracing::info!("Read prompt from file: {:?}", prompt_path);
        Some(text)
    } else if !std::io::stdin().is_terminal() {
        use std::io::Read;
        let mut text = String::new();
        std::io::stdin()
            .read_to_string(&mut text)
            .context("Failed to read prompt from stdin")?;
        if text.trim().is_empty() {
            None
        } else {
            tracing::info!("Read prompt from stdin ({} bytes)", text.len());
            Some(text)
        }
    } else {
        tracing::debug!("No prompt provided; running with existing session only");
        None
    };

    if let Some(ref text) = prompt_text {
        if let Some(comment_id) = extract_comment_id(text) {
            let metadata = serde_json::json!({ "comment_id": comment_id });
            session
                .messages
                .push(Message::user_with_metadata(text, metadata));
        } else {
            session.messages.push(Message::user(text));
        }
    }

    // 8. Run inference loop
    let tools: Option<Vec<serde_json::Value>> = if tool_definitions.is_empty() {
        None
    } else {
        Some(tool_definitions)
    };

    // Resolve the output session path now so we can use it in both the
    // normal path and the MaxIterationsReached soft-stop path.
    let out_path = out_session.or(in_session);

    let inference_result = inference_loop(
        llm,
        &agent.name,
        &agent.model,
        &mut session,
        tools.as_deref(),
        &agent.extra_body,
        &mut mcp_registry,
        max_iterations,
        after_iteration_hook
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .as_deref(),
    )
    .await;

    let (response_text, total_usage) = match inference_result {
        Ok(val) => val,
        Err(e) => {
            if e.downcast_ref::<MaxIterationsReached>().is_some() {
                // Soft stop: save the session so delta mode can resume on next trigger.
                tracing::warn!("{}", e);
                if let Some(ref path) = out_path {
                    let persisted = session_for_persistence(&session);
                    if let Err(save_err) = session_port.save(&persisted, path) {
                        tracing::error!("Failed to save session on max-iterations: {}", save_err);
                    } else {
                        tracing::info!("Session saved to: {:?} (max-iterations soft stop)", path);
                    }
                }
                return Err(e);
            }
            return Err(e);
        }
    };

    tracing::info!(
        "ATOMA_TOKEN_USAGE: prompt={} completion={} total={}",
        total_usage.prompt_tokens,
        total_usage.completion_tokens,
        total_usage.total_tokens,
    );

    // 9. Save session (only if an output path was specified)
    if let Some(ref path) = out_path {
        let persisted = session_for_persistence(&session);
        session_port.save(&persisted, path)?;
        tracing::info!("Session saved to: {:?}", path);
    }

    // 10. Print final response to stdout
    match output_format {
        OutputFormat::Text => {
            println!("{}", response_text);
        }
        OutputFormat::Json | OutputFormat::JsonPretty => {
            let directive = extract_directive_from_text(&response_text);
            let output = serde_json::json!({
                "response": response_text,
                "usage": {
                    "prompt_tokens": total_usage.prompt_tokens,
                    "completion_tokens": total_usage.completion_tokens,
                    "total_tokens": total_usage.total_tokens,
                },
                "directive": directive,
                "session_path": out_path.map(|p| p.to_string_lossy().to_string()),
                "max_iterations_reached": false,
            });
            if matches!(output_format, OutputFormat::JsonPretty) {
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                println!("{}", serde_json::to_string(&output)?);
            }
        }
    }

    Ok(())
}

fn load_transient_context_messages(
    context_sessions: &[PathBuf],
    session_port: &dyn SessionPort,
) -> Result<Vec<Message>> {
    let mut messages = Vec::new();

    for path in context_sessions {
        let context_session = session_port
            .load(path)
            .with_context(|| format!("Failed to load context session: {:?}", path))?;

        let mut loaded: Vec<Message> = context_session
            .messages
            .into_iter()
            .filter(|message| message.role != "system")
            .map(mark_transient_context_message)
            .collect();

        tracing::info!(
            "Loaded {} transient context message(s) from {:?}",
            loaded.len(),
            path
        );

        messages.append(&mut loaded);
    }

    Ok(messages)
}

fn mark_transient_context_message(mut message: Message) -> Message {
    let mut metadata = match message.atoma_metadata.take() {
        Some(serde_json::Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };

    metadata.insert(
        TRANSIENT_CONTEXT_FLAG.to_string(),
        serde_json::Value::Bool(true),
    );
    metadata
        .entry("layer".to_string())
        .or_insert_with(|| serde_json::Value::String(TRANSIENT_CONTEXT_LAYER.to_string()));

    message.atoma_metadata = Some(serde_json::Value::Object(metadata));
    message
}

fn is_transient_context_message(message: &Message) -> bool {
    message
        .atoma_metadata
        .as_ref()
        .and_then(|value| value.get(TRANSIENT_CONTEXT_FLAG))
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

fn session_for_persistence(session: &Session) -> Session {
    let mut persisted = session.clone();
    persisted
        .messages
        .retain(|message| !is_transient_context_message(message));
    persisted
}

/// Execute all tool calls from an LLM response, appending results to the session.
///
/// Tool calls are executed sequentially. Future improvement: parallel execution
/// once McpPort supports `&self` (internal sync) for its call method.
async fn execute_tool_calls(
    agent_name: &str,
    tool_calls: &[ToolCall],
    session: &mut Session,
    mcp_registry: &mut Option<Box<dyn McpPort + Send>>,
) -> Result<()> {
    session
        .messages
        .push(Message::assistant(None, Some(tool_calls.to_vec())));

    let registry = mcp_registry
        .as_mut()
        .context("LLM requested tool calls but no MCP servers are configured")?;

    for tool_call in tool_calls {
        let tool_name = &tool_call.function.name;

        let arguments =
            match serde_json::from_str::<serde_json::Value>(&tool_call.function.arguments) {
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
                tracing::debug!("Tool '{}' result ({} chars)", tool_name, result.len());
                session.messages.push(Message::tool(&tool_call.id, &result));
            }
            Err(e) => {
                let msg = format!("Error: {}", e);
                tracing::error!("Tool '{}' failed: {}", tool_name, e);
                session.messages.push(Message::tool(&tool_call.id, &msg));
            }
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn inference_loop(
    llm_client: &dyn LlmPort,
    agent_name: &str,
    model: &str,
    session: &mut Session,
    tools: Option<&[serde_json::Value]>,
    extra_body: &std::collections::HashMap<String, serde_json::Value>,
    mcp_registry: &mut Option<Box<dyn McpPort + Send>>,
    max_iterations: u32,
    after_iteration_hook: Option<&str>,
) -> Result<(String, LlmUsage)> {
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
            execute_tool_calls(agent_name, &calls, session, mcp_registry).await?;

            if let Some(hook) = after_iteration_hook {
                if let Some(new_content) =
                    hooks::run_after_iteration_hook(hook, agent_name, iteration).await
                {
                    tracing::info!(
                        "after_iteration hook produced {} chars; appending as user message",
                        new_content.len()
                    );
                    session.messages.push(Message::user(&new_content));
                }
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
                    return Ok((text, total_usage));
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
                    return Ok((text, total_usage));
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

/// Parse an optional GitHub comment ID from the first line of a prompt.
///
/// Expected format: `<!-- atoma:comment_id=12345 -->`
pub(crate) fn extract_comment_id(text: &str) -> Option<u64> {
    let first_line = text.lines().next()?;
    let inner = first_line
        .trim()
        .strip_prefix("<!-- atoma:comment_id=")?
        .strip_suffix(" -->")?;
    inner.parse::<u64>().ok()
}

/// Extract a directive (agent name) from the first command-like line of text.
///
/// Accepts `/agent-name` and common markdown variants like `/`agent-name``.
pub(crate) fn extract_directive_from_text(text: &str) -> Option<String> {
    let command_pattern =
        regex_lite::Regex::new(r"^/(?P<agent>[a-z][a-z0-9-]+)(?:\b|\s|$)").ok()?;
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        // Strip markdown list markers
        let cleaned = regex_lite::Regex::new(r"^(?:[-*+]\s+|>\s*)+")
            .ok()
            .map(|re| re.replace(line, ""))
            .unwrap_or_else(|| line.into());
        let cleaned = cleaned.trim();

        // Try plain, backtick variants
        let variants = [cleaned.to_string(), format!("/{}", cleaned.trim_start_matches('/'))];
        for variant in &variants {
            if let Some(cap) = command_pattern.captures(variant) {
                let agent = cap.name("agent")?.as_str().to_string();
                // Validate agent name format
                if regex_lite::Regex::new(r"^[a-z][a-z0-9-]*$")
                    .ok()
                    .map(|re| re.is_match(&agent))
                    .unwrap_or(false)
                {
                    return Some(agent);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_comment_id_present() {
        let text = "<!-- atoma:comment_id=42 -->\nSome content";
        assert_eq!(extract_comment_id(text), Some(42));
    }

    #[test]
    fn test_extract_comment_id_absent() {
        let text = "Just a plain message";
        assert_eq!(extract_comment_id(text), None);
    }

    #[test]
    fn test_extract_comment_id_malformed() {
        let text = "<!-- atoma:comment_id=abc -->";
        assert_eq!(extract_comment_id(text), None);
    }

    #[test]
    fn test_extract_comment_id_large() {
        let text = "<!-- atoma:comment_id=9999999 -->";
        assert_eq!(extract_comment_id(text), Some(9_999_999));
    }

    #[test]
    fn test_mark_transient_context_message_preserves_metadata() {
        let message = Message::user_with_metadata("ctx", serde_json::json!({ "id": 1 }));
        let marked = mark_transient_context_message(message);
        let metadata = marked
            .atoma_metadata
            .as_ref()
            .and_then(|value| value.as_object())
            .unwrap();

        assert_eq!(metadata.get("id").and_then(|value| value.as_i64()), Some(1));
        assert_eq!(
            metadata
                .get(TRANSIENT_CONTEXT_FLAG)
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            metadata.get("layer").and_then(|value| value.as_str()),
            Some(TRANSIENT_CONTEXT_LAYER)
        );
    }

    #[test]
    fn test_session_for_persistence_removes_transient_context_messages() {
        let mut session = Session::default();
        session.messages.push(Message::system("sys"));
        session
            .messages
            .push(mark_transient_context_message(Message::user("ctx")));
        session.messages.push(Message::user("persisted"));

        let persisted = session_for_persistence(&session);

        assert_eq!(persisted.messages.len(), 2);
        assert_eq!(
            persisted.messages[1]
                .content
                .as_ref()
                .and_then(|value| value.as_str()),
            Some("persisted")
        );
        assert!(persisted
            .messages
            .iter()
            .all(|message| !is_transient_context_message(message)));
    }
}
