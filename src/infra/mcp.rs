use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};

use crate::domain::tool::{Hooks, ToolDef};
use crate::domain::tool_health::{self, HealthLog, Severity};
use crate::infra::hooks;

/// How long one `tools/list` or `tools/call` may take, for a server that does not
/// say otherwise.
///
/// Sixty seconds is right for a server that answers from memory or from one HTTP
/// call, which is most of them. It is wrong for two kinds that exist:
///
///   - a shell server, whose whole job is running a build or a test suite. Its
///     own `shell_execute` accepts `timeout_seconds` up to 3600 and defaults to
///     300 -- and every value above 60 was a lie, because this constant killed
///     the call first. The error named the tool, so it read as "the shell server
///     is broken" rather than "the client gave up".
///   - a server that loads a model on its first call. A 544MB reranker took 63.9s
///     to load, measured; this gave up at 60.0s and the answer arrived 15s later.
///
/// So the value belongs to the server, not to this file. `request_timeout_secs`
/// in the tools file is how a server says what it needs; this is what applies
/// when it says nothing.
const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 60;
/// How long a server has to answer `initialize`, which includes its own startup.
const DEFAULT_INIT_TIMEOUT_SECS: u64 = 120;

/// The default, overridable by `ATOMA_MCP_TIMEOUT` for a whole run.
///
/// Kept as the fallback rather than removed: an operator debugging a slow runner
/// wants one lever, not an edit to every entry in a tools file. A server that
/// declares its own value wins over both.
fn default_request_timeout() -> Duration {
    crate::infra::timeouts::from_env("ATOMA_MCP_TIMEOUT", DEFAULT_REQUEST_TIMEOUT_SECS)
}

fn init_timeout() -> Duration {
    crate::infra::timeouts::from_env("ATOMA_MCP_INIT_TIMEOUT", DEFAULT_INIT_TIMEOUT_SECS)
}

/// Split an MCP tool result's `content` into the text the model reads and the
/// image blocks it should see.
///
/// Images used to be folded into the text, where `serde_json::to_string` turned
/// each into a base64 blob the model could only read as characters — a picture
/// arriving as noise, and an expensive one. They now travel separately, in MCP's
/// own shape, for the LLM adapters to map.
///
/// Text keeps a `[image]` marker where each picture was, so its position in the
/// result stays legible: "the diagram below" means nothing once the diagram has
/// been lifted out.
///
/// Anything that is neither text nor image is still serialised into the text, as
/// before. An unknown block type is not a reason to lose it.
fn split_content(result: &Value) -> (String, Vec<Value>) {
    let Some(items) = result.get("content").and_then(|c| c.as_array()) else {
        return (
            serde_json::to_string(result).unwrap_or_default(),
            Vec::new(),
        );
    };

    let mut images = Vec::new();
    let parts: Vec<String> = items
        .iter()
        .map(|item| {
            if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                return text.to_string();
            }
            if item.get("type").and_then(Value::as_str) == Some("image") {
                images.push(item.clone());
                return "[image]".to_string();
            }
            serde_json::to_string(item).unwrap_or_default()
        })
        .collect();

    (parts.join("\n"), images)
}

#[derive(Debug, Clone)]
pub struct RegisteredTool {
    pub prefixed_name: String,
    pub schema: Value,
}

pub struct McpConnection {
    pub name: String,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    process: Option<Child>,
    next_id: u64,
    /// How long this server's `tools/list` and `tools/call` may take. Per server,
    /// because 60 seconds means "stalled" for `github` and "still compiling" for
    /// `shell`.
    request_timeout: Duration,
    /// What this server has said about its own trouble, waiting to go out with its
    /// next tool result. See `domain::tool_health` for why a result carries this at
    /// all.
    ///
    /// Behind a lock because two readers fill it: this connection, from
    /// `notifications/message` on stdout, and the detached task reading stderr.
    health: Arc<Mutex<HealthLog>>,
}

impl Drop for McpConnection {
    fn drop(&mut self) {
        if let Some(ref mut process) = self.process {
            match process.try_wait() {
                Ok(Some(_)) => {}
                _ => {
                    let _ = process.start_kill();
                }
            }
        }
    }
}

impl McpConnection {
    pub async fn spawn(config: &ToolDef) -> Result<Self> {
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args);

        // A tool server gets the credentials its own configuration names, and no
        // others.
        //
        // Not `env_clear()`. A tool server needs PATH to find its interpreter,
        // HOME for its package caches, LANG for its encoding, and an allowlist of
        // that would be long, runtime-specific, and wrong the first time someone
        // adds a Python server -- exactly the enumeration this codebase has been
        // burned by before. Removing the credentials leaves everything a runtime
        // needs untouched.
        //
        // The removal comes first and `envs` second, so a server that declares
        // one of these gets it back. That is the whole routing mechanism: `github`
        // says `GH_TOKEN: ${GH_TOKEN}` and receives it, `shell` says nothing and
        // does not.
        for name in crate::infra::credentials::credential_env_names() {
            cmd.env_remove(name);
        }
        cmd.envs(&config.env);
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut process = cmd
            .spawn()
            .with_context(|| format!("Failed to spawn MCP server: {}", config.name))?;

        let stdin = process
            .stdin
            .take()
            .context("Failed to capture MCP server stdin")?;
        let stdout = process
            .stdout
            .take()
            .context("Failed to capture MCP server stdout")?;
        let stderr = process.stderr.take();

        let mut conn = McpConnection {
            name: config.name.clone(),
            stdin,
            stdout: BufReader::new(stdout),
            process: Some(process),
            next_id: 1,
            request_timeout: config
                .request_timeout_secs
                .map(Duration::from_secs)
                .unwrap_or_else(default_request_timeout),
            health: Arc::new(Mutex::new(HealthLog::default())),
        };

        let init = tokio::time::timeout(
            init_timeout(),
            conn.send_request(
                "initialize",
                serde_json::json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": client_capabilities(),
                    "clientInfo": {
                        "name": env!("CARGO_PKG_NAME"),
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }),
            ),
        )
        .await;

        // Whether this server said it can report its own trouble over the protocol,
        // which is what the `logging/setLevel` below is worth sending for. The value
        // of the match rather than a flag set inside it: there is no moment where it
        // holds a guess.
        let server_logs = match init {
            Ok(Ok(response)) => {
                let result = response
                    .get("result")
                    .context("Initialize response missing result")?;
                let server_name = result
                    .pointer("/serverInfo/name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let server_version = result
                    .pointer("/serverInfo/version")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let protocol = result
                    .get("protocolVersion")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                tracing::info!(
                    "MCP server '{}' connected: {} v{} (protocol: {})",
                    config.name,
                    server_name,
                    server_version,
                    protocol,
                );

                let server_logs = result.pointer("/capabilities/logging").is_some();

                if let Some(stderr_handle) = stderr {
                    let server_label = config.name.clone();
                    let health = Arc::clone(&conn.health);
                    tokio::spawn(async move {
                        use tokio::io::AsyncBufReadExt;
                        let mut reader = BufReader::new(stderr_handle);
                        let mut line = String::new();
                        loop {
                            line.clear();
                            match reader.read_line(&mut line).await {
                                Ok(0) | Err(_) => break,
                                Ok(_) => {
                                    let text = line.trim_end();
                                    tracing::info!("[MCP:{}:stderr] {}", server_label, text);
                                    // The fallback channel, and today the only one
                                    // in use: no server this project ships
                                    // implements `logging` yet. Severity has to be
                                    // read out of the words, which is what makes
                                    // this the fallback rather than the primary.
                                    let severity = tool_health::severity_of_stderr(text);
                                    if severity != Severity::Routine {
                                        if let Ok(mut log) = health.lock() {
                                            log.record(severity, text);
                                        }
                                    }
                                }
                            }
                        }
                    });
                }

                server_logs
            }
            Ok(Err(e)) => {
                let stderr_msg = capture_stderr(stderr).await;
                anyhow::bail!(
                    "Failed to initialize MCP server '{}': {}{}",
                    config.name,
                    e,
                    stderr_msg,
                );
            }
            Err(_) => {
                let stderr_msg = capture_stderr(stderr).await;
                anyhow::bail!(
                    "MCP server '{}' initialization timed out ({}s){}",
                    config.name,
                    init_timeout().as_secs(),
                    stderr_msg,
                );
            }
        };

        conn.send_notification("notifications/initialized", serde_json::json!({}))
            .await?;

        // Warnings and worse, from a server that said it can send them.
        //
        // Without this the server picks: MCP lets it send at "a default level of
        // its choosing", which in practice is either nothing or every debug line.
        // `warning` is exactly what reaches the agent, so asking for it is asking
        // for what will be used, and nothing else crosses the transport.
        //
        // Best effort. A server that declared the capability and then refuses the
        // request is still a working tool server; failing the connection over its
        // log level would take away more than it protects.
        if server_logs {
            let level = serde_json::json!({ "level": "warning" });
            match tokio::time::timeout(
                conn.request_timeout,
                conn.send_request("logging/setLevel", level),
            )
            .await
            {
                Ok(Ok(_)) => tracing::debug!(
                    "MCP server '{}' will report at warning and above",
                    config.name,
                ),
                Ok(Err(e)) => tracing::debug!(
                    "MCP server '{}' declined logging/setLevel: {}",
                    config.name,
                    e,
                ),
                Err(_) => tracing::warn!(
                    "MCP server '{}' did not answer logging/setLevel",
                    config.name,
                ),
            }
        }

        Ok(conn)
    }

    pub async fn list_tools(&mut self) -> Result<Vec<RegisteredTool>> {
        let response = tokio::time::timeout(
            self.request_timeout,
            self.send_request("tools/list", serde_json::json!({})),
        )
        .await
        .with_context(|| format!("Timed out listing tools from MCP server: {}", self.name))?
        .with_context(|| format!("Failed to list tools from MCP server: {}", self.name))?;

        let tools = response
            .get("result")
            .and_then(|r| r.get("tools"))
            .and_then(|t| t.as_array())
            .context("MCP server did not return tools array")?;

        let registered = tools
            .iter()
            .map(|tool| {
                let tool_name = tool
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let prefixed = format!("{}__{}", self.name, tool_name);
                RegisteredTool {
                    prefixed_name: prefixed,
                    schema: tool.clone(),
                }
            })
            .collect();

        Ok(registered)
    }

    pub async fn call_tool(
        &mut self,
        tool_name: &str,
        arguments: &Value,
    ) -> Result<(String, Vec<Value>, bool)> {
        let response = tokio::time::timeout(
            self.request_timeout,
            self.send_request(
                "tools/call",
                serde_json::json!({
                    "name": tool_name,
                    "arguments": arguments,
                }),
            ),
        )
        .await
        .with_context(|| {
            format!(
                "Timed out calling tool '{}' on MCP server '{}'",
                tool_name, self.name
            )
        })?
        .with_context(|| {
            format!(
                "Failed to call tool '{}' on MCP server '{}'",
                tool_name, self.name
            )
        })?;

        let result = response
            .get("result")
            .context("MCP server did not return a result")?;

        // Check isError flag: when true, the MCP server encountered an error
        // and the content contains the error description. We propagate this
        // as an Err so the inference loop treats it as a tool failure rather
        // than passing error text to the LLM as a successful result.
        let is_error = result
            .get("isError")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let session_ends = result
            .get("_meta")
            .and_then(|m| m.get("session_ends"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let (content_parts, images) = split_content(result);

        // Everything this server has reported since its last result, attached to
        // this one. A tool's answer includes how well it could answer; the whole
        // argument is in `domain::tool_health`.
        //
        // On the error path too. An error is when the agent most needs to know the
        // server had already said something was wrong with it.
        let notes = match self.health.lock() {
            Ok(mut log) => log.drain(),
            Err(_) => Vec::new(),
        };
        let annotation = tool_health::annotation(&self.name, &notes);

        if is_error {
            anyhow::bail!(
                "Tool '{}' on MCP server '{}' reported an error: {}",
                tool_name,
                self.name,
                tool_health::with_annotation(content_parts, annotation),
            );
        }

        Ok((
            tool_health::with_annotation(content_parts, annotation),
            images,
            session_ends,
        ))
    }

    async fn send_request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        let request_str = serde_json::to_string(&request)?;
        tracing::debug!("[MCP:{}] Sending: {}", self.name, request_str);

        self.stdin
            .write_all(format!("{}\n", request_str).as_bytes())
            .await?;
        self.stdin.flush().await?;

        let response = self.read_response(id).await?;
        tracing::debug!(
            "[MCP:{}] Received: {}",
            self.name,
            serde_json::to_string(&response).unwrap_or_default()
        );

        if let Some(error) = response.get("error") {
            let msg = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown error");
            anyhow::bail!("MCP JSON-RPC error: {}", msg);
        }

        Ok(response)
    }

    /// The response to request `expected_id`, discarding anything else on the way.
    ///
    /// ## Why this checks the id
    ///
    /// It used to return the first thing it could parse, and the id it had just
    /// written was never read back. That is correct exactly as long as no request
    /// is ever abandoned -- and `tools/call` abandons one every time it times out.
    /// The dropped future stops reading; the server, which knows nothing about the
    /// timeout, finishes its work and writes the response anyway. It sits in the
    /// pipe. The next call reads it.
    ///
    /// From then on every answer belongs to the previous question, for the life of
    /// the run, and nothing detects it: the shape is valid, the content is
    /// plausible, and the id that would have given it away was not being looked
    /// at. Measured in a real run (2026-08-21, run 32436740948):
    ///
    /// ```text
    /// 01:35:11  search_issues called
    /// 01:36:11  ERROR Timed out calling tool 'search_issues'  <- client gives up
    /// 01:36:34  query "..." -> #461, #464, #408               <- server answers anyway
    /// 01:37:06  query "..." -> #408, #403, #464               <- the RETRY's answer,
    ///                                                            read by the call after it
    /// ```
    ///
    /// The agent got issues for a question it had already replaced. A wrong answer
    /// that looks like an answer is worse than the timeout that caused it.
    ///
    /// Discarding rather than resynchronising: a late response is the answer to a
    /// question nobody is waiting for any more. There is no caller to give it to.
    /// It is logged at `warn` because it means a timeout fired, which is worth
    /// seeing next to the tool that caused it.
    async fn read_response(&mut self, expected_id: u64) -> Result<Value> {
        loop {
            let value = self.read_json_value().await?;
            match classify(&value, expected_id) {
                Incoming::TheAnswer => return Ok(value),
                Incoming::Abandoned(id) => tracing::warn!(
                    "[MCP:{}] discarding a late response to request {} while waiting for {} -- an earlier call timed out and the server answered it afterwards",
                    self.name,
                    id,
                    expected_id,
                ),
                Incoming::NotAResponse => {
                    // Bound outside the macro: `tracing`s expansion has its own
                    // `Value` trait in scope, so `Value::as_str` inside the call
                    // resolves to that one and does not compile.
                    let method = value
                        .get("method")
                        .and_then(Value::as_str)
                        .unwrap_or("?");
                    // One notification is not traffic to skip. `notifications/message`
                    // is the server reporting on itself, with a severity it chose;
                    // it used to be discarded here, which is how a degraded server
                    // stayed quiet -- see `domain::tool_health`.
                    if method == LOG_NOTIFICATION {
                        if let Some((severity, message)) = log_note(value.get("params")) {
                            let kept = match self.health.lock() {
                                Ok(mut log) => log.record(severity, &message),
                                Err(_) => false,
                            };
                            let disposition = if kept {
                                "goes out with the next result"
                            } else {
                                "already reported"
                            };
                            tracing::info!(
                                "[MCP:{}:log] {} ({})",
                                self.name,
                                message,
                                disposition,
                            );
                        }
                    } else {
                        tracing::debug!(
                            "[MCP:{}] not a response to {}, reading past it: {}",
                            self.name,
                            expected_id,
                            method,
                        );
                    }
                }
            }
        }
    }

    /// One complete JSON value from the server's stdout.
    ///
    /// Accumulates lines because a server may pretty-print, which puts one value
    /// across many lines; `is_eof` is how serde says "valid so far, incomplete".
    async fn read_json_value(&mut self) -> Result<Value> {
        let mut buf = String::new();
        loop {
            let mut line = String::new();
            let n = self
                .stdout
                .read_line(&mut line)
                .await
                .context("Failed to read response from MCP server")?;
            if n == 0 {
                anyhow::bail!("MCP server closed connection");
            }
            buf.push_str(&line);
            match serde_json::from_str(&buf) {
                Ok(value) => return Ok(value),
                Err(e) if e.is_eof() => {
                    if buf.len() > 10_485_760 {
                        anyhow::bail!("MCP response exceeded maximum size (10MB)");
                    }
                    continue;
                }
                Err(e) => {
                    anyhow::bail!("Failed to parse MCP JSON-RPC response: {}", e);
                }
            }
        }
    }

    async fn send_notification(&mut self, method: &str, params: Value) -> Result<()> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        let request_str = serde_json::to_string(&request)?;
        self.stdin
            .write_all(format!("{}\n", request_str).as_bytes())
            .await?;
        self.stdin.flush().await?;
        Ok(())
    }
}

/// What a value read from a server is, relative to the request being awaited.
#[derive(Debug, PartialEq)]
enum Incoming {
    /// The response to the request in flight.
    TheAnswer,
    /// A response to a request that timed out, carrying the id it answers.
    Abandoned(u64),
    /// Nothing this client asked for: a notification, or an id it could not have
    /// issued. Read past either way.
    NotAResponse,
}

/// Whether a value read from the server answers request `expected_id`.
///
/// A function rather than a `match` inside the read loop because this is the
/// judgement that was missing, and a missing judgement is best kept somewhere a
/// test can reach. See `read_response`.
fn classify(value: &Value, expected_id: u64) -> Incoming {
    match value.get("id") {
        Some(Value::Number(n)) => match n.as_u64() {
            Some(id) if id == expected_id => Incoming::TheAnswer,
            // Includes an id this client never issued -- a negative or fractional
            // one, or a number beyond u64. It is not the answer being waited for,
            // which is the only thing that matters here.
            Some(id) => Incoming::Abandoned(id),
            // A negative, fractional, or oversized id. A server sending one is
            // broken, but it is still not the answer being waited for, and that is
            // the only question here.
            None => Incoming::NotAResponse,
        },
        // `id: null` is what a server sends when it could not parse the request
        // well enough to echo an id back. Only one request is ever in flight from
        // this client, so it is about that one: hand it to the caller, which turns
        // the `error` field into a message naming the tool.
        Some(Value::Null) => Incoming::TheAnswer,
        // A JSON-RPC notification -- `notifications/progress` and the like. Servers
        // may send these unprompted, so this is normal traffic, not a fault.
        _ => Incoming::NotAResponse,
    }
}

/// What this client tells a server it can do.
///
/// `logging` says it will listen to `notifications/message`. A server may withhold
/// those from a client that did not ask, so leaving this out is how a report of
/// degradation never arrives -- see `domain::tool_health`.
///
/// A function so a test can hold it. Nothing observable breaks when a capability
/// stops being declared: servers go quiet, tools keep answering, and the loss shows
/// up as an absence months later.
fn client_capabilities() -> Value {
    serde_json::json!({ "logging": {} })
}

/// The method name of MCP's log notification.
const LOG_NOTIFICATION: &str = "notifications/message";

/// A `notifications/message` turned into what an agent would need to read.
///
/// MCP's shape is `{ level, logger?, data }`, where `data` is any JSON. The level
/// is the server's own judgement, which is the whole reason this channel is
/// preferred over reading stderr: there is nothing to infer.
///
/// `None` for anything routine, so no caller decides that a second time. A
/// notification with no level is malformed and lands there too -- a missing
/// severity is not an urgent one.
fn log_note(params: Option<&Value>) -> Option<(Severity, String)> {
    let params = params?;
    let level = params.get("level").and_then(Value::as_str).unwrap_or("");
    let severity = tool_health::severity_of_level(level);
    if severity == Severity::Routine {
        return None;
    }

    // `data` is free-form by specification. A string is the message; anything else
    // is serialised, because a server that reports in an object is still reporting,
    // and dropping it would lose the one thing this exists to carry.
    let data = match params.get("data") {
        Some(Value::String(text)) => text.clone(),
        Some(other) => serde_json::to_string(other).unwrap_or_default(),
        None => String::new(),
    };
    let message = match params.get("logger").and_then(Value::as_str) {
        Some(logger) if !logger.trim().is_empty() => format!("{}: {}", logger.trim(), data),
        _ => data,
    };
    Some((severity, message))
}

async fn capture_stderr(stderr: Option<ChildStderr>) -> String {
    if let Some(mut handle) = stderr {
        let mut buf = String::new();
        match tokio::time::timeout(
            std::time::Duration::from_millis(500),
            handle.read_to_string(&mut buf),
        )
        .await
        {
            Ok(Ok(_)) if !buf.trim().is_empty() => {
                format!("\n--- stderr ---\n{}\n---------------", buf.trim_end())
            }
            _ => String::new(),
        }
    } else {
        String::new()
    }
}

/// Manages multiple MCP connections and routes tool calls by tool prefix.
pub struct McpRegistry {
    connections: HashMap<String, McpConnection>,
    tools: Vec<RegisteredTool>,
    hooks: HashMap<String, Hooks>,
}

impl McpRegistry {
    pub async fn from_configs(configs: &[ToolDef]) -> Result<Self> {
        let mut seen = std::collections::HashSet::new();
        for config in configs {
            if !seen.insert(&config.name) {
                anyhow::bail!(
                    "Duplicate MCP server name: '{}'. Each server must have a unique name.",
                    config.name,
                );
            }
        }

        let mut connections = HashMap::new();
        let mut all_tools = Vec::new();

        for config in configs {
            let mut conn = McpConnection::spawn(config).await?;
            let tools = conn.list_tools().await?.into_iter().filter(|tool| {
                hooks::access_denial_reason(&config.hooks, &tool.prefixed_name).is_none()
            });
            all_tools.extend(tools);
            connections.insert(config.name.clone(), conn);
        }

        let hooks: HashMap<String, Hooks> = configs
            .iter()
            .map(|c| (c.name.clone(), c.hooks.clone()))
            .collect();

        // Both lists together is allowed -- the denylist is checked first -- and worth
        // saying out loud once, here, rather than refused as it used to be.
        for (name, h) in &hooks {
            hooks::describe_hooks(name, h);
        }

        Ok(McpRegistry {
            connections,
            tools: all_tools,
            hooks,
        })
    }

    /// Return OpenAI-compatible tool definitions for all registered tools.
    pub fn tool_definitions(&self) -> Vec<Value> {
        self.tools
            .iter()
            .map(|t| {
                let mut def = serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.prefixed_name,
                        "description": t.schema.get("description")
                            .and_then(|d| d.as_str())
                            .unwrap_or(""),
                    }
                });
                if let Some(input_schema) = t.schema.get("inputSchema") {
                    def["function"]["parameters"] = input_schema.clone();
                }
                def
            })
            .collect()
    }

    /// Call a tool by its prefixed name, running access-control hooks.
    ///
    /// Order: denylist/allowlist → before_tool hook → MCP call → after_tool hook.
    pub async fn call_tool_with_hooks(
        &mut self,
        agent_name: &str,
        prefixed_name: &str,
        arguments: &Value,
    ) -> Result<crate::domain::ports::ToolCallResult> {
        let server_name = prefixed_name.split("__").next().unwrap_or("");
        let hooks = self.hooks.get(server_name).cloned();

        if let Some(ref h) = hooks {
            hooks::check_access(h, prefixed_name)?;

            if let Some(ref script) = h.before_tool {
                let payload = serde_json::json!({
                    "agent": agent_name,
                    "tool": prefixed_name,
                    "arguments": arguments,
                });
                hooks::run_before_hook(script, payload).await?;
            }
        }

        let (content, images, session_ends) = self.call_tool(prefixed_name, arguments).await?;

        if let Some(ref h) = hooks {
            if let Some(ref script) = h.after_tool {
                let payload = serde_json::json!({
                    "agent": agent_name,
                    "tool": prefixed_name,
                    "arguments": arguments,
                    "result": content,
                });
                hooks::run_after_hook(script, payload).await;
            }
        }

        Ok(crate::domain::ports::ToolCallResult {
            content,
            images,
            session_ends,
        })
    }

    pub(crate) async fn call_tool(
        &mut self,
        prefixed_name: &str,
        arguments: &Value,
    ) -> Result<(String, Vec<Value>, bool)> {
        let (server_name, tool_name) = prefixed_name
            .split_once("__")
            .context("Invalid tool name format (expected server__tool)")?;

        let conn = self
            .connections
            .get_mut(server_name)
            .with_context(|| format!("Unknown MCP server: {}", server_name))?;

        conn.call_tool(tool_name, arguments).await
    }
}

// ── Port implementation ───────────────────────────────────────────────────────

#[async_trait::async_trait]
impl crate::domain::ports::ToolPort for McpRegistry {
    fn tool_definitions(&self) -> Vec<serde_json::Value> {
        self.tool_definitions()
    }

    async fn call_tool(
        &mut self,
        agent_name: &str,
        prefixed_name: &str,
        arguments: &serde_json::Value,
    ) -> anyhow::Result<crate::domain::ports::ToolCallResult> {
        self.call_tool_with_hooks(agent_name, prefixed_name, arguments)
            .await
    }
}

// ── MCP factory ───────────────────────────────────────────────────────────────

/// Factory adapter implementing `McpFactory` for constructing `McpRegistry`.
pub struct McpRegistryFactory;

#[async_trait::async_trait]
impl crate::domain::ports::McpFactory for McpRegistryFactory {
    async fn build(
        &self,
        tool_defs: &[crate::domain::tool::ToolDef],
    ) -> anyhow::Result<Box<dyn crate::domain::ports::ToolPort + Send>> {
        let registry = McpRegistry::from_configs(tool_defs).await?;
        Ok(Box::new(registry))
    }
}

#[cfg(test)]
mod split_content_tests {
    use super::split_content;
    use serde_json::json;

    // A picture used to reach the model as `serde_json::to_string` of the whole
    // block: thousands of base64 characters it could only read as characters.
    #[test]
    fn an_image_leaves_the_text_and_travels_on_its_own() {
        let result = json!({"content": [
            {"type": "text", "text": "Here is the screen:"},
            {"type": "image", "data": "AAAA", "mimeType": "image/png"},
        ]});
        let (text, images) = split_content(&result);
        assert_eq!(text, "Here is the screen:\n[image]");
        assert_eq!(images.len(), 1);
        assert_eq!(images[0]["data"], "AAAA");
        assert!(
            !text.contains("AAAA"),
            "base64 must not be left in the text"
        );
    }

    #[test]
    fn a_text_only_result_is_unchanged_and_carries_no_images() {
        let result = json!({"content": [{"type": "text", "text": "done"}]});
        assert_eq!(split_content(&result), ("done".to_string(), Vec::new()));
    }

    // An unknown block type is not a reason to lose it.
    #[test]
    fn an_unknown_block_is_still_serialised_into_the_text() {
        let result = json!({"content": [{"type": "audio", "data": "BBBB"}]});
        let (text, images) = split_content(&result);
        assert!(text.contains("audio"));
        assert!(images.is_empty());
    }

    #[test]
    fn a_result_without_content_falls_back_to_the_whole_value() {
        let result = json!({"unexpected": true});
        let (text, images) = split_content(&result);
        assert!(text.contains("unexpected"));
        assert!(images.is_empty());
    }
}

#[cfg(test)]
mod classify_tests {
    use super::{classify, Incoming};
    use serde_json::json;

    /// The case that was broken. A `tools/call` timed out, the server finished and
    /// wrote its answer anyway, and the next call read it -- so every answer from
    /// then on belonged to the previous question. Nothing detected it, because the
    /// id was written and never read back.
    #[test]
    fn a_late_response_to_an_abandoned_request_is_not_the_answer() {
        let stale = json!({"jsonrpc": "2.0", "id": 7, "result": {"content": []}});
        assert_eq!(classify(&stale, 8), Incoming::Abandoned(7));
    }

    #[test]
    fn the_response_with_the_matching_id_is_the_answer() {
        let fresh = json!({"jsonrpc": "2.0", "id": 8, "result": {"content": []}});
        assert_eq!(classify(&fresh, 8), Incoming::TheAnswer);
    }

    /// A server may send these at any time, so they are read past rather than
    /// treated as an answer or as a fault.
    #[test]
    fn a_notification_has_no_id_and_answers_nothing() {
        let progress = json!({"jsonrpc": "2.0", "method": "notifications/progress"});
        assert_eq!(classify(&progress, 8), Incoming::NotAResponse);
    }

    /// The one case where a mismatched id must still be delivered: the server
    /// could not parse the request well enough to echo an id, so it sent `null`.
    /// Skipping it would wait out the whole timeout for a reply already received.
    #[test]
    fn a_null_id_carries_an_error_about_the_request_in_flight() {
        let parse_error = json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700}});
        assert_eq!(classify(&parse_error, 8), Incoming::TheAnswer);
    }

    /// An id this client cannot have issued. Not the answer being waited for,
    /// which is all this needs to decide.
    #[test]
    fn an_id_that_is_not_a_u64_is_never_the_answer() {
        for id in [json!(-1), json!(1.5)] {
            let value = json!({"jsonrpc": "2.0", "id": id, "result": {}});
            assert_ne!(classify(&value, 8), Incoming::TheAnswer, "{id}");
        }
    }

    /// Reading past a stale response has to reach the real one, however many are
    /// queued: two consecutive timeouts leave two answers in the pipe.
    #[test]
    fn several_stale_responses_are_all_read_past() {
        let queued = [
            json!({"id": 5, "result": {}}),
            json!({"id": 6, "result": {}}),
        ];
        for value in &queued {
            assert!(matches!(classify(value, 7), Incoming::Abandoned(_)));
        }
        assert_eq!(
            classify(&json!({"id": 7, "result": {}}), 7),
            Incoming::TheAnswer
        );
    }
}

#[cfg(test)]
mod log_note_tests {
    use super::{log_note, LOG_NOTIFICATION};
    use crate::domain::tool_health::Severity;
    use serde_json::json;

    /// The protocol channel, and why it is the primary one: the server said how bad
    /// it was, so nothing has to be guessed from the words.
    #[test]
    fn a_warning_carries_the_servers_own_severity() {
        let params = json!({"level": "warning", "data": "reranker unavailable"});
        assert_eq!(
            log_note(Some(&params)),
            Some((Severity::Warning, "reranker unavailable".to_string())),
        );
    }

    /// Routine traffic is not the agent's business. Dropped here so no caller has to
    /// decide it a second time.
    #[test]
    fn an_info_message_is_not_a_report() {
        let params = json!({"level": "info", "data": "listening"});
        assert_eq!(log_note(Some(&params)), None);
    }

    /// A missing severity is not an urgent one -- and treating it as urgent would put
    /// a malformed server's chatter into every result.
    #[test]
    fn a_notification_without_a_level_is_dropped() {
        assert_eq!(log_note(Some(&json!({"data": "something"}))), None);
        assert_eq!(log_note(None), None);
    }

    #[test]
    fn the_logger_name_is_kept_when_there_is_one() {
        let params = json!({"level": "error", "logger": "search.index", "data": "no such path"});
        let (severity, message) = log_note(Some(&params)).expect("an error is a report");
        assert_eq!(severity, Severity::Error);
        assert_eq!(message, "search.index: no such path");
    }

    /// `data` is free-form by specification. A server reporting in an object is still
    /// reporting; dropping it would lose the one thing this exists to carry.
    #[test]
    fn structured_data_survives_as_text() {
        let params = json!({"level": "warning", "data": {"stage": "rerank", "fell_back": true}});
        let (_, message) = log_note(Some(&params)).unwrap();
        assert!(message.contains("rerank"), "{message}");
        assert!(message.contains("fell_back"), "{message}");
    }

    /// A report with no `data` at all still reaches the agent as its level, rather
    /// than being lost for having said nothing.
    #[test]
    fn a_bare_level_is_still_a_report() {
        let (severity, message) = log_note(Some(&json!({"level": "error"}))).unwrap();
        assert_eq!(severity, Severity::Error);
        assert_eq!(message, "", "empty here; `HealthLog::record` is what refuses it");
    }

    /// Pinned because the string is the protocol's, not this project's: a typo would
    /// silently return the code to discarding every report.
    #[test]
    fn the_method_name_is_the_one_mcp_defines() {
        assert_eq!(LOG_NOTIFICATION, "notifications/message");
    }

    /// The declaration a server reads before it decides whether to report anything.
    /// Undeclaring it breaks nothing that fails: servers just go quiet.
    #[test]
    fn the_client_says_it_will_listen_to_log_notifications() {
        let capabilities = super::client_capabilities();
        assert!(
            capabilities.get("logging").is_some(),
            "a server may send nothing to a client that did not ask: {capabilities}",
        );
    }
}
