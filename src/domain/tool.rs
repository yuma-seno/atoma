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
///
/// # Where a server is
///
/// Three arrangements, and the fields say which:
///
/// | `command` | `url` | what it means |
/// |---|---|---|
/// | set | absent | a child process, spoken to over stdio |
/// | absent | set | something already running, spoken to over HTTP |
/// | set | set | atoma starts it, then speaks HTTP to it |
///
/// The third is not a curiosity. A server atoma starts is one whose stderr atoma
/// owns, which is what keeps `domain::tool_health`'s fallback channel working and
/// what lets a credential be routed to it -- and neither of those reaches a server
/// somebody else is running. Whether the conversation then happens over a pipe or
/// a socket is a separate question from who started it.
#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: String,
    /// The program to start. Empty when the server is already running.
    pub command: String,
    pub args: Vec<String>,
    /// The environment the child is started with. Empty when there is no child --
    /// which is why a remote server declaring one is refused rather than ignored:
    /// a credential you believe you routed and did not is the worst of the three
    /// outcomes.
    pub env: HashMap<String, String>,
    /// Where to reach the server over Streamable HTTP. `None` means stdio.
    pub url: Option<String>,
    /// Headers sent with every request to `url`.
    ///
    /// This is how a remote server is authenticated, and it is the whole of it.
    /// `env` cannot reach an endpoint somebody else is running, so a token gets
    /// there as a header or not at all.
    pub headers: HashMap<String, String>,
    pub hooks: Hooks,
    /// How much of one tool result from this server reaches the model, in
    /// characters. `None` means the client's default.
    ///
    /// Per server for the same reason the timeout is: a shell server returning a
    /// test suite's output and a filesystem server returning a config file are not
    /// the same question. See `domain::tool_output` for what the default is and
    /// why a cap exists at all.
    pub max_output_chars: Option<usize>,
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
