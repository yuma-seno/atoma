use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

use atoma::domain::ports::{ToolCallResult, ToolPort};

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
}

#[async_trait]
impl ToolPort for MockMcpRegistry {
    fn tool_definitions(&self) -> Vec<Value> {
        self.tools.clone()
    }

    /// One fixed server, because that is what this mock is: a registry with a list of
    /// tools and no routing beyond it. Named so a caller counting calls has something to
    /// count against, and `None` for a name it does not offer, which is what a real
    /// registry answers for a tool nobody serves.
    fn server_for(&self, prefixed_name: &str) -> Option<String> {
        self.responses
            .contains_key(prefixed_name)
            .then(|| "mock".to_string())
    }

    async fn call_tool(
        &mut self,
        _agent_name: &str,
        prefixed_name: &str,
        _arguments: &Value,
    ) -> Result<ToolCallResult> {
        let content = self.responses.get(prefixed_name).cloned().ok_or_else(|| {
            anyhow::anyhow!("MockMcpRegistry: no response for tool '{}'", prefixed_name)
        })?;
        let session_ends = self.session_ends_tools.contains(prefixed_name);
        Ok(ToolCallResult {
            content,
            session_ends,
            ..Default::default()
        })
    }
}
