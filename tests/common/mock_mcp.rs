use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

use atoma::domain::ports::{McpPort, ToolCallResult};

/// A mock MCP registry that returns predefined tool results.
pub struct MockMcpRegistry {
    tools: Vec<Value>,
    responses: HashMap<String, String>,
    session_ends_tools: std::collections::HashSet<String>,
}

impl MockMcpRegistry {
    pub fn new() -> Self {
        Self {
            tools: Vec::new(),
            responses: HashMap::new(),
            session_ends_tools: std::collections::HashSet::new(),
        }
    }

    /// Register a tool definition visible to the LLM.
    pub fn with_tool(mut self, name: &str, description: &str) -> Self {
        let tool = serde_json::json!({
            "type": "function",
            "function": {
                "name": name,
                "description": description,
                "parameters": {
                    "type": "object",
                    "properties": {}
                }
            }
        });
        self.tools.push(tool);
        self
    }

    /// Register a fixed response for a tool call.
    pub fn with_response(mut self, tool_name: &str, result: &str) -> Self {
        self.responses
            .insert(tool_name.to_string(), result.to_string());
        self
    }

    /// Mark a tool as session-ending (simulates _meta.session_ends: true).
    pub fn with_session_ends(mut self, tool_name: &str) -> Self {
        self.session_ends_tools.insert(tool_name.to_string());
        self
    }
}

#[async_trait]
impl McpPort for MockMcpRegistry {
    fn tool_definitions(&self) -> Vec<Value> {
        self.tools.clone()
    }

    async fn call_tool_with_hooks(
        &mut self,
        _agent_name: &str,
        prefixed_name: &str,
        _arguments: &Value,
    ) -> Result<ToolCallResult> {
        let content = self
            .responses
            .get(prefixed_name)
            .cloned()
            .ok_or_else(|| {
                anyhow::anyhow!("MockMcpRegistry: no response for tool '{}'", prefixed_name)
            })?;
        let session_ends = self.session_ends_tools.contains(prefixed_name);
        Ok(ToolCallResult {
            content,
            session_ends,
        })
    }
}
