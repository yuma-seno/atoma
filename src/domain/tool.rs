use std::collections::HashMap;

/// Hook configuration for a tool server.
///
/// Defines access control and lifecycle scripts for tool calls.
#[derive(Debug, Clone, Default)]
pub struct Hooks {
    /// Glob patterns for allowed tools. Empty = all tools allowed.
    pub tool_allowlist: Vec<String>,
    /// Glob patterns for blocked tools. Checked before the allowlist.
    pub tool_denylist: Vec<String>,
    /// Path to a script invoked before each tool call.
    ///
    /// Receives JSON on stdin: `{"agent": "...", "tool": "...", "arguments": {...}}`
    /// Must respond with JSON: `{"allow": true}` or `{"allow": false, "reason": "..."}`
    /// Non-zero exit or invalid JSON is treated as a deny (fail-closed).
    pub before_tool: Option<String>,
    /// Path to a script invoked after each successful tool call.
    ///
    /// Receives JSON on stdin: `{"agent": "...", "tool": "...", "arguments": {...}, "result": "..."}`
    /// Output is ignored. Non-zero exit is logged but does not fail the run.
    pub after_tool: Option<String>,
}

/// A fully resolved tool server definition.
///
/// Parsing from YAML and loading from disk is handled by
/// `crate::infra::persistence::tool_def`.
#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub hooks: Hooks,
}
