use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

use crate::domain::agent::ParsedAgentDef;
use crate::domain::session::{Message, Session};
use crate::domain::skill::SkillCatalog;
use crate::domain::tool::ToolDef;

// ── LLM port ──────────────────────────────────────────────────────────────────

/// Response from a single LLM completion request.
pub struct LlmResponse {
    pub choices: Vec<LlmChoice>,
    pub usage: Option<LlmUsage>,
}

/// A single choice returned by the LLM.
pub struct LlmChoice {
    pub message: Message,
    pub finish_reason: Option<String>,
}

/// Token usage statistics.
#[derive(Debug, Default, Clone, Copy)]
pub struct LlmUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

/// Port for LLM chat completion.
///
/// Each infra provider (`OpenAIClient`, `CopilotClient`, `AnthropicClient`)
/// implements this trait. The `application` layer depends only on this port.
#[async_trait]
pub trait LlmPort: Send + Sync {
    async fn chat_completion(
        &self,
        model: &str,
        messages: &[Message],
        tools: Option<&[Value]>,
        extra_body: &HashMap<String, Value>,
    ) -> Result<LlmResponse>;
}

// ── Tool port ─────────────────────────────────────────────────────────────────

/// Result from a single MCP tool call.
#[derive(Debug, Default)]
pub struct ToolCallResult {
    pub content: String,
    /// Image blocks the tool returned, in MCP's own wire shape
    /// (`{"type":"image","data":"<base64>","mimeType":"image/png"}`).
    ///
    /// Kept beside `content` rather than folded into it because every consumer
    /// of a tool result reads text — logs, hooks, the built-in skill tool — and
    /// only the message the model receives cares about pictures. MCP's shape is
    /// stored as-is so nothing here has to pick a provider's; each LLM adapter
    /// maps it to its own, which is where that knowledge already lives.
    pub images: Vec<Value>,
    pub session_ends: bool,
}

/// Unified port for tools visible to the LLM.
///
/// Implementations may be external MCP servers or Atoma built-in tools.
#[async_trait]
pub trait ToolPort: Send {
    fn tool_definitions(&self) -> Vec<Value>;

    async fn call_tool(
        &mut self,
        agent_name: &str,
        name: &str,
        arguments: &Value,
    ) -> Result<ToolCallResult>;
}

// ── Persistence ports ─────────────────────────────────────────────────────────

/// Port for loading and saving agent sessions.
pub trait SessionPort: Send + Sync {
    fn load(&self, path: &Path) -> Result<Session>;
    fn save(&self, session: &Session, path: &Path) -> Result<()>;
}

/// Port for parsing agent definition files.
pub trait AgentDefPort: Send + Sync {
    fn parse(&self, path: &Path) -> Result<ParsedAgentDef>;
}

/// Port for loading tool definition files.
pub trait ToolDefPort: Send + Sync {
    fn load(&self, path: &Path) -> Result<HashMap<String, ToolDef>>;
}

/// Port for loading and validating a skill catalog.
pub trait SkillPort: Send + Sync {
    fn load(&self, root: &Path) -> Result<SkillCatalog>;
}

// ── MCP factory port ──────────────────────────────────────────────────────────

/// Port for constructing an MCP registry from a list of tool definitions.
#[async_trait]
pub trait McpFactory: Send + Sync {
    async fn build(&self, tool_defs: &[ToolDef]) -> Result<Box<dyn ToolPort + Send>>;
}
