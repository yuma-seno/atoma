use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

use crate::domain::agent::ParsedAgentDef;
use crate::domain::session::{Message, Session};
use crate::domain::skill::{SkillCatalog, SkillMetadata};
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
    pub finish_reason: Option<FinishReason>,
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

/// Why a completion stopped.
///
/// The vocabulary used to exist only as the arms of a `match` in the runner, with each
/// adapter expected to produce one of those strings and nothing checking that it had.
/// Two consequences, both reachable:
///
/// - the Anthropic adapter passed unmapped stop reasons through as-is, so an agent with
///   `extra_body: stop_sequences: [...]` — which that adapter does not reserve — got a
///   perfectly good completion turned into "LLM returned unexpected finish_reason:
///   stop_sequence", and the text was discarded;
/// - the Responses adapter collapsed every `incomplete` reason except
///   `max_output_tokens` to `stop`, so filtered output arrived as an empty `stop` and was
///   reported as "LLM returned empty response … 3 times in a row" after two paid retries,
///   naming the wrong cause.
///
/// As an enum, an adapter cannot invent a fifth value and the runner's match is checked
/// by the compiler. What each dialect calls these stays in that dialect's adapter, which
/// is the only place that knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinishReason {
    /// The model finished of its own accord.
    Stop,
    /// The output hit a token ceiling.
    Length,
    /// The provider refused to return what the model produced.
    ContentFilter,
    /// The model asked for tools.
    ToolCalls,
}

impl FinishReason {
    /// Read the OpenAI chat-completions spelling, which is the canonical one.
    ///
    /// `None` for anything else, so a provider inventing a value is visible rather than
    /// silently becoming `Stop`.
    pub fn from_openai(raw: &str) -> Option<Self> {
        match raw {
            "stop" => Some(Self::Stop),
            "length" => Some(Self::Length),
            "content_filter" => Some(Self::ContentFilter),
            "tool_calls" => Some(Self::ToolCalls),
            _ => None,
        }
    }

    /// The name to put in a message a person reads.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::Length => "length",
            Self::ContentFilter => "content_filter",
            Self::ToolCalls => "tool_calls",
        }
    }
}

impl std::fmt::Display for FinishReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
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

// ── Template port ─────────────────────────────────────────────────────────────

/// Port for rendering the system prompt.
///
/// Every other dependency the runner has arrives through `RunDeps`; this one was reached
/// for directly as `crate::infra::template`, the only `infra` import in `application`
/// outside tests. That is not a crash waiting to happen, it is a hole in the arrangement
/// the rest of the file keeps: with a port, a test can render its own prompt, and nothing
/// in `application` knows how the built-in template is stored.
pub trait TemplatePort: Send + Sync {
    fn build_system_prompt(&self, context: &PromptContext<'_>) -> String;
}

/// Everything the prompt is built from.
///
/// A struct because it was six positional parameters, four of them strings or slices of
/// strings — an order a caller can get wrong silently.
pub struct PromptContext<'a> {
    pub agent: &'a ParsedAgentDef,
    pub tool_descriptions: &'a [String],
    /// Overrides the built-in template entirely when present.
    pub custom_template: Option<&'a str>,
    pub working_dir: &'a str,
    pub colleagues: &'a [(String, String)],
    pub skills: &'a [SkillMetadata],
}

// ── MCP factory port ──────────────────────────────────────────────────────────

/// Port for constructing an MCP registry from a list of tool definitions.
#[async_trait]
pub trait McpFactory: Send + Sync {
    async fn build(&self, tool_defs: &[ToolDef]) -> Result<Box<dyn ToolPort + Send>>;
}
