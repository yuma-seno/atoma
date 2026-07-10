//! Agent runner — orchestrates agent definition loading, session management,
//! MCP connection, inference loop, and output formatting.

mod context;
mod execution;

use anyhow::{Context, Result};
use std::io::IsTerminal;
use std::path::PathBuf;

use crate::domain::config::OutputFormat;
use crate::domain::ports::{AgentDefPort, LlmPort, McpFactory, McpPort, SessionPort, ToolDefPort};
use crate::domain::session::{Message, Session};
use crate::infra::template;

pub use context::session_for_persistence;
pub(crate) use execution::{extract_comment_id, extract_directive_from_text};
pub use execution::{inference_loop, InferenceResult, MaxIterationsReached};

// ── Bundled parameter structs ────────────────────────────────────────────────

/// User-supplied settings for a single `run` invocation.
pub struct RunSettings {
    pub agent_def_path: PathBuf,
    pub in_session: Option<PathBuf>,
    pub context_sessions: Vec<PathBuf>,
    pub prompt_file: Option<PathBuf>,
    pub out_session: Option<PathBuf>,
    pub template_path: Option<PathBuf>,
    pub tools_file: Option<PathBuf>,
    pub max_iterations: u32,
    pub after_iteration_hook: Option<PathBuf>,
    pub output_format: OutputFormat,
}

/// External dependencies (ports) required by the runner.
pub struct RunDeps<'a> {
    pub llm: &'a dyn LlmPort,
    pub agent_def: &'a dyn AgentDefPort,
    pub session: &'a dyn SessionPort,
    pub tool_def: &'a dyn ToolDefPort,
    pub mcp_factory: &'a dyn McpFactory,
}

/// Run the agent: parse agent def, load session, connect MCP, run inference loop, save session.
pub async fn run(settings: RunSettings, deps: RunDeps<'_>) -> Result<()> {
    let RunSettings {
        agent_def_path,
        in_session,
        context_sessions,
        prompt_file,
        out_session,
        template_path,
        tools_file,
        max_iterations,
        after_iteration_hook,
        output_format,
    } = settings;

    // 1. Parse agent definition
    let parsed_agent = deps
        .agent_def
        .parse(&agent_def_path)
        .context("Failed to parse agent definition")?;
    let agent = &parsed_agent.frontmatter;

    tracing::info!("Loaded agent: {} (model: {})", agent.name, agent.model);

    // 2. Read or create session
    let mut session = match in_session.as_ref() {
        Some(path) => deps.session.load(path)?,
        None => Session::default(),
    };

    session
        .messages
        .retain(|message| !context::is_transient_context_message(message));

    tracing::info!("Session has {} messages", session.messages.len());

    let transient_context_messages =
        context::load_transient_context_messages(&context_sessions, deps.session)?;

    // 3. Connect to MCP servers and discover tools
    let mut mcp_registry: Option<Box<dyn McpPort + Send>> = if agent.mcp_servers.is_empty() {
        None
    } else {
        let tools_path = tools_file
            .as_ref()
            .context("Agent has mcp_servers configured but --tools-file was not specified")?;
        let tools_map = deps.tool_def.load(tools_path)?;

        let tool_defs: Vec<_> = agent
            .mcp_servers
            .iter()
            .map(|name| {
                tools_map.get(name).cloned().with_context(|| {
                    format!("Tool '{}' not found in tools file: {:?}", name, tools_path)
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let reg = deps.mcp_factory.build(&tool_defs).await?;
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

    // 4. Build system prompt
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
            let parsed = deps.agent_def.parse(&candidate).with_context(|| {
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

    // 5. Replace system message
    session.messages.retain(|m| m.role != "system");
    session.messages.insert(0, Message::system(&system_prompt));
    if !transient_context_messages.is_empty() {
        tracing::info!(
            "Injecting {} transient context message(s)",
            transient_context_messages.len()
        );
        // Append transient context (e.g. new GitHub events/comments) at the END
        // of the message list rather than right after the system message.
        // For a fresh session (no prior history) this is a no-op positionally.
        // For a RESUMED session with prior history, appending at the end is
        // required so the new context is the most recent turn the model sees
        // — otherwise it is buried before the session's own old history, and
        // (absent an explicit new --prompt/stdin) the model's last visible
        // turn stays its own previous reply, causing it to produce a shallow
        // continuation instead of reacting to the new information.
        session.messages.extend(transient_context_messages);
    }

    // 6. Resolve user prompt
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

    // 7. Run inference loop
    let tools: Option<Vec<serde_json::Value>> = if tool_definitions.is_empty() {
        None
    } else {
        Some(tool_definitions)
    };

    let out_path = out_session.or(in_session);

    let inference_result = inference_loop(
        deps.llm,
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
        Ok(InferenceResult::Completed { text, usage }) => (text, usage),
        Ok(InferenceResult::SessionEnded { usage: _ }) => {
            tracing::info!("Session suspended by tool request");
            // Save session and exit cleanly — no output needed
            if let Some(ref path) = out_path {
                let persisted = session_for_persistence(&session);
                deps.session.save(&persisted, path)?;
                tracing::info!("Session saved to: {:?} (suspended)", path);
            }
            return Ok(());
        }
        Err(e) => {
            if e.downcast_ref::<MaxIterationsReached>().is_some() {
                tracing::warn!("{}", e);
                if let Some(ref path) = out_path {
                    let persisted = session_for_persistence(&session);
                    if let Err(save_err) = deps.session.save(&persisted, path) {
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

    // 8. Save session
    if let Some(ref path) = out_path {
        let persisted = session_for_persistence(&session);
        deps.session.save(&persisted, path)?;
        tracing::info!("Session saved to: {:?}", path);
    }

    // 9. Output
    match output_format {
        OutputFormat::Text => {
            println!("{}", response_text);
        }
        OutputFormat::Json => {
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
            println!("{}", serde_json::to_string(&output)?);
        }
    }

    Ok(())
}
