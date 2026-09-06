//! Agent runner — orchestrates agent definition loading, session management,
//! MCP connection, and the inference loop.

mod execution;

use anyhow::{Context, Result};
use std::io::IsTerminal;
use std::path::PathBuf;
use std::time::Duration;

use crate::application::tools::RuntimeTools;
use crate::domain::ports::{
    AgentDefPort, LlmPort, LlmUsage, McpFactory, PromptContext, SessionPort, SkillPort,
    TemplatePort, ToolDefPort, ToolPort,
};
use crate::domain::session::{Message, Session};
use crate::domain::skill::SkillCatalog;

// The two sentinel types stay unexported on purpose. `is_limit_stop` is the whole
// question anyone outside asks about them, and exporting the types invites each caller
// to answer it again with its own `downcast_ref` -- which is how one of the two callers
// came to know about only one ceiling.
pub use execution::{inference_loop, is_limit_stop, CompletionReason, InferenceResult};

// ── Bundled parameter structs ────────────────────────────────────────────────

/// User-supplied settings for a single `run` invocation.
pub struct RunSettings {
    pub agent_def_path: PathBuf,
    pub in_session: Option<PathBuf>,
    pub prompt_file: Option<PathBuf>,
    pub out_session: Option<PathBuf>,
    pub template_path: Option<PathBuf>,
    pub tools_file: Option<PathBuf>,
    pub skills_dir: Option<PathBuf>,
    /// A ceiling on turns, if the caller asked for one. `None` is unbounded.
    pub max_iterations: Option<u32>,
    /// A ceiling on wall-clock time, if the caller asked for one. `None` is unbounded.
    pub max_runtime: Option<Duration>,
}

/// Observable outcome of a completed run. Presentation belongs to the caller.
#[derive(Debug)]
pub enum RunOutcome {
    Completed {
        text: String,
        usage: LlmUsage,
        reason: CompletionReason,
        session_path: Option<PathBuf>,
    },
    SessionEnded,
}

/// External dependencies (ports) required by the runner.
pub struct RunDeps<'a> {
    pub llm: &'a dyn LlmPort,
    pub agent_def: &'a dyn AgentDefPort,
    pub session: &'a dyn SessionPort,
    pub tool_def: &'a dyn ToolDefPort,
    pub skill: &'a dyn SkillPort,
    pub mcp_factory: &'a dyn McpFactory,
    pub template: &'a dyn TemplatePort,
}

/// Run the agent: parse agent def, load session, connect MCP, run inference loop, save session.
pub async fn run(settings: RunSettings, deps: RunDeps<'_>) -> Result<RunOutcome> {
    let RunSettings {
        agent_def_path,
        in_session,
        prompt_file,
        out_session,
        template_path,
        tools_file,
        skills_dir,
        max_iterations,
        max_runtime,
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

    tracing::info!("Session has {} messages", session.messages.len());

    // 3. Connect to MCP servers and discover tools
    let external_tools: Option<Box<dyn ToolPort + Send>> = if agent.mcp_servers.is_empty() {
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

    let skill_catalog = match skills_dir.as_ref() {
        Some(path) => deps.skill.load(path)?,
        None => SkillCatalog::default(),
    };
    let skill_metadata = skill_catalog.metadata();
    let mut runtime_tools: Box<dyn ToolPort + Send> =
        Box::new(RuntimeTools::new(skill_catalog, external_tools)?);

    let tool_definitions = runtime_tools.tool_definitions();

    let tool_descriptions: Vec<String> = tool_definitions
        .iter()
        .map(|t| {
            let name = t
                .pointer("/function/name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            format!("- `{}`", name)
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

    let system_prompt = deps.template.build_system_prompt(&PromptContext {
        agent: &parsed_agent,
        tool_descriptions: &tool_descriptions,
        custom_template: custom_template.as_deref(),
        working_dir: &working_dir,
        colleagues: &colleagues,
        skills: &skill_metadata,
    });
    tracing::debug!("System prompt:\n{}", system_prompt);

    // 5. Replace system message
    session.messages.retain(|m| m.role != "system");
    session.messages.insert(0, Message::system(&system_prompt));

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
        session.messages.push(Message::user(text));
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
        &mut runtime_tools,
        max_iterations,
        max_runtime,
        agent.vision,
    )
    .await;

    let (response_text, total_usage, completion_reason) = match inference_result {
        Ok(InferenceResult::Completed {
            text,
            usage,
            reason,
        }) => (text, usage, reason),
        Ok(InferenceResult::SessionEnded) => {
            tracing::info!("Session suspended by tool request");
            // Save session and exit cleanly — no output needed
            if let Some(ref path) = out_path {
                deps.session.save(&session, path)?;
                tracing::info!("Session saved to: {:?} (suspended)", path);
            }
            return Ok(RunOutcome::SessionEnded);
        }
        Err(e) => {
            if is_limit_stop(&e) {
                tracing::warn!("{}", e);
                if let Some(ref path) = out_path {
                    if let Err(save_err) = deps.session.save(&session, path) {
                        tracing::error!("Failed to save session on soft stop: {}", save_err);
                    } else {
                        tracing::info!("Session saved to: {:?} (soft stop)", path);
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
        deps.session.save(&session, path)?;
        tracing::info!("Session saved to: {:?}", path);
    }

    Ok(RunOutcome::Completed {
        text: response_text,
        usage: total_usage,
        reason: completion_reason,
        session_path: out_path,
    })
}
