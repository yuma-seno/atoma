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
    /// How long one `tools/list` or `tools/call` on this server may take, in
    /// seconds. `None` means the client's default.
    ///
    /// Per server because the right value is a property of what the server does,
    /// and a single number cannot be right for all of them. A `github` server that
    /// has not answered in a minute has stopped answering. A `shell` server that
    /// has not answered in a minute is compiling -- and its own `shell_execute`
    /// advertises `timeout_seconds` up to 3600, which was unreachable while one
    /// constant capped every server at 60.
    ///
    /// Raising it is not free: this is the only thing that notices a server which
    /// has stopped responding, so a large value means a long wait before a stuck
    /// run says so. Which is why it is opt-in per server rather than a bigger
    /// default, and why a server that answers quickly should not set it.
    pub request_timeout_secs: Option<u64>,
}
