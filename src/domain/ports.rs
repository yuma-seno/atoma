use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

use crate::domain::agent::ParsedAgentDef;
use crate::domain::session::{Message, Session};
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
#[derive(Default, Clone, Copy)]
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

// ── MCP port ──────────────────────────────────────────────────────────────────

/// Port for MCP tool execution.
///
/// `McpRegistry` in the `infra` layer implements this trait.
#[async_trait]
pub trait McpPort: Send {
    fn tool_definitions(&self) -> Vec<Value>;

    async fn call_tool_with_hooks(
        &mut self,
        agent_name: &str,
        prefixed_name: &str,
        arguments: &Value,
    ) -> Result<String>;
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

// ── MCP factory port ──────────────────────────────────────────────────────────

/// Port for constructing an MCP registry from a list of tool definitions.
#[async_trait]
pub trait McpFactory: Send + Sync {
    async fn build(&self, tool_defs: &[ToolDef]) -> Result<Box<dyn McpPort + Send>>;
}
