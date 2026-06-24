# Atoma - Stateless MCP Orchestrator CLI

[![CI](https://github.com/yuma-seno/atoma/actions/workflows/ci.yml/badge.svg)](https://github.com/yuma-seno/atoma/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

Atoma is a lightweight, platform-independent CLI tool that orchestrates AI agents via [Model Context Protocol (MCP)](https://modelcontextprotocol.io). It connects LLMs (via OpenAI-compatible APIs like OpenRouter) with MCP server tools in a stateless inference loop.

## Installation

### Pre-built binaries

Download the latest binary for your platform from the [Releases](https://github.com/yuma-seno/atoma/releases) page:

| Platform | File |
|---|---|
| Linux (x86_64) | `atoma-linux-x86_64` |
| macOS (Apple Silicon) | `atoma-macos-arm64` |
| macOS (Intel) | `atoma-macos-x86_64` |
| Windows (x86_64) | `atoma-windows-x86_64.exe` |

```bash
# Linux example
curl -Lo atoma https://github.com/yuma-seno/atoma/releases/latest/download/atoma-linux-x86_64
chmod +x atoma
sudo mv atoma /usr/local/bin/
```

### Build from source

```bash
# From crates.io (once published)
cargo install atoma

# Or directly from source
git clone https://github.com/yuma-seno/atoma
cd atoma
cargo build --release
# binary: target/release/atoma
```

## Quick Start (Docker / Podman)

```bash
# 1. Build the image
docker build -t atoma -f Dockerfile .

# 2. Set your API key
export OPENAI_API_KEY="sk-your-key-here"

# 3. Run with a prompt via stdin
echo "Hello, what can you do?" | docker run --rm -i \
  -e OPENAI_API_KEY \
  -v "$PWD/.atoma_sample/agent-definitions:/defs" \
  atoma run --agent-def /defs/OrchestratorAgent.md

# Or via a prompt file
docker run --rm \
  -e OPENAI_API_KEY \
  -v "$PWD/.atoma_sample/agent-definitions:/defs" \
  -v "$PWD/prompt.txt:/prompt.txt" \
  atoma run --agent-def /defs/OrchestratorAgent.md --prompt-file /prompt.txt
```

## CLI Reference

```
atoma run \
  --agent-def <FILE>       # Agent definition (Markdown + YAML frontmatter) [required]
  [--in-session <FILE>]    # Input session JSON (prior conversation history)
  [--context-session <FILE>] # Extra session JSON injected only for this run (repeatable)
  [--out-session <FILE>]   # Output session JSON (omit to discard after run)
  [--prompt-file <FILE>]   # User prompt from file
                           # If omitted and stdin is not a TTY, reads from stdin
                           # If both omitted, runs with no new user message
  [--template <FILE>]      # Custom system prompt template (overrides built-in)
  [--tools-file <FILE>]    # Tools YAML file (required if mcp_servers is non-empty)
```

Session input and output are always **separate arguments**, making data flow explicit.
Specify the same path for both to update a session in place:

```bash
# Resume a conversation (read and write the same session file)
atoma run --agent-def ./agent.md \
  --in-session ./sess.json \
  --prompt-file ./followup.txt \
  --out-session ./sess.json

# Pipeline: stdin as prompt
git diff | atoma run --agent-def ./ReviewAgent.md \
  --in-session ./sess.json --out-session ./sess.json
```

Per-run context can be supplied separately from the durable session:

```bash
atoma run --agent-def ./agent.md \
  --in-session ./sess.json \
  --context-session ./issue-summary.json \
  --context-session ./current-diff.json \
  --out-session ./sess.json
```

Messages loaded via `--context-session` are inserted after the system prompt and
before the persistent session history, then discarded before `--out-session` is written.
This lets external workflows build arbitrary context bundles without polluting the
durable transcript.

## Agent Definition

Agent definitions are Markdown files with YAML frontmatter.
Pass the path via `--agent-def`.

```markdown
---
name: ReviewAgent
description: Code review and quality assurance specialist.
model: openrouter/anthropic/claude-3.5-sonnet
knows_about:
  - EngineerAgent
mcp_servers:
  - filesystem
  - shell
extra_body:
  temperature: 0.2      # any LLM API parameter goes here
  max_tokens: 8192
---

Optional custom role prompt body. When present, replaces {{AGENT_ROLE_PROMPT}}
in the system prompt template.
```

For the full field reference, provider selection rules, `extra_body` recipes
(Anthropic extended thinking, OpenAI reasoning effort, OpenRouter routing), and
template variables, see **[docs/agent-definition.md](docs/agent-definition.md)**.

## Tool Servers (`--tools-file`)

MCP server definitions live in a separate YAML file specified with `--tools-file`.
This is required whenever the agent's `mcp_servers` list is non-empty.

```yaml
# tools/tools.yaml
filesystem:
  command: npx
  args: ["-y", "@modelcontextprotocol/server-filesystem", "."]

shell:
  command: npx
  args: ["-y", "mcp-shell", "."]
  hooks:
    tool_denylist: ["shell__rm*", "shell__sudo*"]
    before_tool: ./scripts/shell_guard.py
```

For the full field reference, hook script protocol (stdin/stdout JSON format),
allowlist/denylist patterns, and tool naming conventions,
see **[docs/tool-servers.md](docs/tool-servers.md)**.

## Inference Loop

1. Read input session from `--in-session` (or start with empty session)
2. Parse agent definition → build system prompt
3. Connect to MCP servers → discover tools
4. Append user message: `--prompt-file` → stdin (if not a TTY) → none
5. **Loop**:
   - Call LLM API (OpenAI-compatible)
   - If `tool_calls` returned → execute via MCP stdio, add results, continue
   - If text response returned → print to stdout, save session to `--out-session`, exit

## Environment Variables

### OpenAI / OpenRouter (default provider)

| Variable | Required | Default | Description |
|---|---|---|---|
| `OPENAI_API_KEY` | Yes | — | API key |
| `OPENAI_BASE_URL` | No | `https://openrouter.ai/api/v1` | API base URL |
| `OPENAI_APP_NAME` | No | `atoma` | Shown in OpenRouter dashboard |

### Anthropic

| Variable | Required | Default | Description |
|---|---|---|---|
| `ANTHROPIC_API_KEY` | Yes | — | API key |
| `ANTHROPIC_BASE_URL` | No | `https://api.anthropic.com` | Override for proxies |

### GitHub Copilot

| Variable | Required | Description |
|---|---|---|
| `ATOMA_COPILOT_TOKEN` | Yes | Personal access token with `copilot` scope |
| `GITHUB_TOKEN` or `GH_TOKEN` | No | Fallback (not recommended; use `ATOMA_COPILOT_TOKEN` instead) |

### Provider selection

| Variable | Description |
|---|---|
| `ATOMA_PROVIDER` | Force provider: `openai`, `anthropic`, or `github-copilot`. Overridden by the `provider` field in agent frontmatter |

## Development

```bash
cargo build --release   # build binary
cargo test              # run all tests
```

For container builds (Docker / Podman), Podman rootless notes, and the overall
project structure, see **[docs/development.md](docs/development.md)**.
