use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};

use crate::domain::tool::{Hooks, ToolDef};
use crate::infra::hooks;

fn request_timeout() -> Duration {
    std::env::var("ATOMA_MCP_TIMEOUT")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(60))
}

fn init_timeout() -> Duration {
    std::env::var("ATOMA_MCP_INIT_TIMEOUT")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(120))
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
        };

        let init = tokio::time::timeout(
            init_timeout(),
            conn.send_request(
                "initialize",
                serde_json::json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {
                        "name": env!("CARGO_PKG_NAME"),
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }),
            ),
        )
        .await;

        match init {
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

                if let Some(stderr_handle) = stderr {
                    let server_label = config.name.clone();
                    tokio::spawn(async move {
                        use tokio::io::AsyncBufReadExt;
                        let mut reader = BufReader::new(stderr_handle);
                        let mut line = String::new();
                        loop {
                            line.clear();
                            match reader.read_line(&mut line).await {
                                Ok(0) | Err(_) => break,
                                Ok(_) => tracing::info!(
                                    "[MCP:{}:stderr] {}",
                                    server_label,
                                    line.trim_end()
                                ),
                            }
                        }
                    });
                }
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
        }

        conn.send_notification("notifications/initialized", serde_json::json!({}))
            .await?;
        Ok(conn)
    }

    pub async fn list_tools(&mut self) -> Result<Vec<RegisteredTool>> {
        let response = tokio::time::timeout(
            request_timeout(),
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

    pub async fn call_tool(&mut self, tool_name: &str, arguments: &Value) -> Result<(String, bool)> {
        let response = tokio::time::timeout(
            request_timeout(),
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

        let content_parts = result
            .get("content")
            .and_then(|c| c.as_array())
            .map(|items| {
                items
                    .iter()
                    .map(|item| {
                        item.get("text")
                            .and_then(|t| t.as_str())
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| serde_json::to_string(item).unwrap_or_default())
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_else(|| serde_json::to_string(result).unwrap_or_default());

        if is_error {
            anyhow::bail!(
                "Tool '{}' on MCP server '{}' reported an error: {}",
                tool_name,
                self.name,
                content_parts,
            );
        }

        Ok((content_parts, session_ends))
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

        let response = self.read_response().await?;
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

    async fn read_response(&mut self) -> Result<Value> {
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
            let tools = conn.list_tools().await?;
            all_tools.extend(tools);
            connections.insert(config.name.clone(), conn);
        }

        let hooks: HashMap<String, Hooks> = configs
            .iter()
            .map(|c| (c.name.clone(), c.hooks.clone()))
            .collect();

        // Validate hook configurations at registration time.
        for (name, h) in &hooks {
            hooks::validate_hooks(h)
                .with_context(|| format!("Invalid hooks for MCP server '{}'", name))?;
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

        let (content, session_ends) = self.call_tool(prefixed_name, arguments).await?;

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
            session_ends,
        })
    }

    pub(crate) async fn call_tool(
        &mut self,
        prefixed_name: &str,
        arguments: &Value,
    ) -> Result<(String, bool)> {
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
impl crate::domain::ports::McpPort for McpRegistry {
    fn tool_definitions(&self) -> Vec<serde_json::Value> {
        self.tool_definitions()
    }

    async fn call_tool_with_hooks(
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
    ) -> anyhow::Result<Box<dyn crate::domain::ports::McpPort + Send>> {
        let registry = McpRegistry::from_configs(tool_defs).await?;
        Ok(Box::new(registry))
    }
}
