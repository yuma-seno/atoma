# Tool Servers Reference

Tool servers expose MCP (Model Context Protocol) tools to Atoma agents.
Server definitions live in a YAML file passed via `--tools-file`.
This file is required whenever an agent's `mcp_servers` list is non-empty.

---

## File Format

```yaml
# tools/tools.yaml
<server-name>:
  command: <executable>
  args: [<arg>, ...]
  env:
    KEY: value
  hooks:               # optional
    tool_allowlist: [<glob>, ...]
    tool_denylist:  [<glob>, ...]
    before_tool: <path>
    after_tool:  <path>
```

Each top-level key is a **server name** and must match an entry in the agent's `mcp_servers` list.

---

## Fields

| Field | Type | Required | Description |
|---|---|---|---|
| `command` | String | **Yes** | Executable to spawn (must be on `PATH` or an absolute path) |
| `args` | `[String]` | No | Arguments passed to the command |
| `env` | Object | No | Extra environment variables for the subprocess |
| `hooks` | Object | No | Access control and lifecycle hooks (see [Hooks](#hooks)) |

The server process must speak the MCP stdio protocol on its stdin/stdout.

---

## Examples

### Filesystem server (read-only access to current directory)

```yaml
filesystem:
  command: npx
  args: ["-y", "@modelcontextprotocol/server-filesystem", "."]
  env: {}
```

### Shell server with access control

```yaml
shell:
  command: npx
  args: ["-y", "mcp-shell", "."]
  env: {}
  hooks:
    # Block destructive patterns
    tool_denylist:
      - "shell__rm*"
      - "shell__sudo*"
    # Require human-in-the-loop approval for everything else
    before_tool: ./scripts/shell_guard.py
    after_tool:  ./scripts/audit_log.py
```

### Multiple servers in one file

```yaml
filesystem:
  command: npx
  args: ["-y", "@modelcontextprotocol/server-filesystem", "."]

shell:
  command: npx
  args: ["-y", "mcp-shell", "."]

github:
  command: npx
  args: ["-y", "@modelcontextprotocol/server-github"]
  env:
    GITHUB_PERSONAL_ACCESS_TOKEN: "${ATOMA_COPILOT_TOKEN}"
```

---

## Hooks

Hooks let you intercept every tool call for access control, auditing, or logging.
All hook paths are resolved **relative to the tools YAML file**.

### Allowlist and denylist

Filtering happens before the MCP call is made.
Patterns support a single trailing `*` wildcard.

| Field | Behavior |
|---|---|
| `tool_denylist` | Blocks any tool matching at least one pattern. Checked first. |
| `tool_allowlist` | If non-empty, blocks any tool **not** matching at least one pattern. Checked after denylist. |

The tool name used for matching is the **prefixed name** as seen by the agent
(e.g. `filesystem__read_file`, `shell__run_command`).

```yaml
hooks:
  tool_denylist:
    - "shell__rm*"          # block anything starting with shell__rm
    - "shell__sudo*"
  tool_allowlist:
    - "filesystem__read*"   # only allow filesystem reads
    - "filesystem__list*"
```

### `before_tool` hook

An executable invoked **before** each MCP call.
Receives JSON on stdin and must write JSON to stdout.

**stdin:**
```json
{
  "agent": "EngineerAgent",
  "tool": "shell__run_command",
  "arguments": { "command": "ls -la" }
}
```

**stdout (allow):**
```json
{ "allow": true }
```

**stdout (deny):**
```json
{ "allow": false, "reason": "Command not permitted by policy" }
```

Non-zero exit code or invalid JSON is treated as a **deny** (fail-closed).

### `after_tool` hook

An executable invoked **after** each successful MCP call.
Receives JSON on stdin; its stdout and exit code are ignored (logged on failure but does not abort the run).

**stdin:**
```json
{
  "agent": "EngineerAgent",
  "tool": "shell__run_command",
  "arguments": { "command": "ls -la" },
  "result": "total 32\ndrwxr-xr-x ..."
}
```

Typical uses: audit logging, metrics, Slack/Webhook notifications.

---

## Tool naming

Atoma prefixes each tool name with `<server-name>__` before exposing it to the LLM.
This avoids collisions when multiple servers expose a tool with the same name.

| Server name | Raw tool name | Prefixed name seen by LLM |
|---|---|---|
| `filesystem` | `read_file` | `filesystem__read_file` |
| `shell` | `run_command` | `shell__run_command` |

Use the **prefixed** names in `tool_allowlist` and `tool_denylist` patterns.

---

## Hook script examples

Sample Python hook scripts are provided in the repository under
[.atoma_sample/tools/scripts/](.atoma_sample/tools/scripts/).

**Minimal shell guard (`before_tool`):**

```python
#!/usr/bin/env python3
import json, sys

payload = json.load(sys.stdin)
tool = payload.get("tool", "")
args = payload.get("arguments", {})

# Example: block rm -rf
cmd = args.get("command", "")
if "rm -rf" in cmd:
    print(json.dumps({"allow": False, "reason": "rm -rf is not permitted"}))
else:
    print(json.dumps({"allow": True}))
```
