# Agent Definition Reference

An agent definition is a Markdown file with YAML frontmatter.
Pass it to Atoma with `--agent-def <FILE>`.

```
---
<YAML frontmatter>
---

Optional role prompt body (Markdown)
```

---

## Frontmatter Fields

| Field | Type | Required | Description |
|---|---|---|---|
| `name` | String | **Yes** | Agent identifier used in logs and colleague lists |
| `description` | String | **Yes** | Short role description; used in colleague lists and as fallback role prompt |
| `model` | String | **Yes** | Model ID passed verbatim to the API (e.g. `openrouter/anthropic/claude-3.5-sonnet`) |
| `provider` | String | No | Force a specific provider: `openai`, `github-copilot`, or `anthropic`. Overrides `ATOMA_PROVIDER` env var and auto-detection |
| `knows_about` | `[String]` | No | Other agent names this agent can delegate to. Each name must correspond to a `<name>.md` in the same directory |
| `mcp_servers` | `[String]` | No | Tool server keys (must match entries in `--tools-file`) |
| `extra_body` | Object | No | Arbitrary key-value pairs merged into the LLM API request body. See [Extra Body](#extra-body) |
| `metadata` | Object | No | Arbitrary data for external tooling (e.g. Atoma-Actions). Ignored by Atoma itself |

### Body (role prompt)

Everything after the closing `---` of the frontmatter is treated as the **role prompt body**.
It is injected into the system prompt template as `{{AGENT_ROLE_PROMPT}}`.
When the body is omitted, the `description` field is used instead.

```markdown
---
name: ReviewAgent
description: Code review specialist.
model: openrouter/anthropic/claude-3.5-sonnet
---

You are a senior code reviewer. Focus on correctness, readability, and security.
Always cite specific line numbers when pointing out issues.
```

---

## Extra Body

`extra_body` is merged verbatim into the outgoing LLM API request (JSON object).
The reserved fields `model` and `messages` cannot be overridden.
For the Anthropic provider, `max_tokens`, `system`, `tools`, and `tool_choice` are also reserved.

Use `extra_body` to pass any provider-specific parameter not yet exposed as a dedicated field.

### Temperature / top-p (all providers)

```yaml
extra_body:
  temperature: 0.2
  top_p: 0.9
```

### Max tokens

```yaml
extra_body:
  max_tokens: 16384
```

For the Anthropic provider the default is 8192.
For OpenAI-compatible providers no default is set (the API default applies).

### Anthropic — Extended Thinking (Claude 3.7+)

Pass the `thinking` object as defined in [Anthropic's extended thinking docs](https://docs.anthropic.com/en/docs/build-with-claude/extended-thinking).

```yaml
model: anthropic/claude-3-7-sonnet-20250219
extra_body:
  thinking:
    type: enabled
    budget_tokens: 10000
  # max_tokens must be greater than budget_tokens
  max_tokens: 16000
```

> **Note:** Extended thinking requires `max_tokens > budget_tokens`.
> Tool use is supported with extended thinking, but streaming is not (Atoma does not use streaming).

### OpenAI — Reasoning Effort (o-series models)

```yaml
model: o3-mini
extra_body:
  reasoning_effort: medium   # low | medium | high
```

### OpenRouter — Provider Routing

OpenRouter accepts additional routing hints in the request body.
See [OpenRouter docs](https://openrouter.ai/docs/provider-routing) for the full list.

```yaml
model: openrouter/anthropic/claude-3.5-sonnet
extra_body:
  # Prefer specific providers
  provider:
    order: ["Anthropic", "AWS Bedrock"]
    allow_fallbacks: false

  # Require certain features
  provider:
    require_parameters: true

  # Route based on price / performance
  provider:
    sort: price
```

### OpenRouter — Extended Thinking via OpenRouter

When using Claude through OpenRouter with extended thinking:

```yaml
model: openrouter/anthropic/claude-3-7-sonnet
extra_body:
  thinking:
    type: enabled
    budget_tokens: 10000
  max_tokens: 16000
```

OpenRouter forwards the `thinking` parameter to Anthropic automatically.

---

## System Prompt Template

The built-in template (Japanese by default) is rendered automatically.
Override it with `--template <FILE>`.

Available placeholder variables:

| Variable | Content |
|---|---|
| `{{AGENT_NAME}}` | `name` from frontmatter |
| `{{AGENT_ROLE_PROMPT}}` | Markdown body (or `description` if body is absent) |
| `{{COLLEAGUES_LIST}}` | Bullet list of `knows_about` agents with their descriptions |
| `{{AVAILABLE_TOOLS}}` | Bullet list of MCP-discovered tools with descriptions |
| `{{WORKING_DIR}}` | Current working directory at invocation time |

Example minimal template:

```
You are {{AGENT_NAME}}.

{{AGENT_ROLE_PROMPT}}

Working directory: {{WORKING_DIR}}

## Available tools
{{AVAILABLE_TOOLS}}
```

---

## Provider Selection

Atoma selects the LLM provider in this priority order:

1. `provider` field in agent frontmatter
2. `ATOMA_PROVIDER` environment variable
3. Auto-detection based on available environment variables:
   - `ANTHROPIC_API_KEY` → `anthropic`
   - `ATOMA_COPILOT_TOKEN` → `github-copilot`
   - `OPENAI_API_KEY` → `openai` (default, includes OpenRouter)

### Environment variables per provider

**OpenAI / OpenRouter (default)**

| Variable | Default | Description |
|---|---|---|
| `OPENAI_API_KEY` | — | API key |
| `OPENAI_BASE_URL` | `https://openrouter.ai/api/v1` | API base URL |
| `OPENAI_APP_NAME` | `atoma` | Shown in OpenRouter dashboard |

**Anthropic**

| Variable | Default | Description |
|---|---|---|
| `ANTHROPIC_API_KEY` | — | API key |
| `ANTHROPIC_BASE_URL` | `https://api.anthropic.com` | API base URL (override for proxies) |

**GitHub Copilot**

| Variable | Description |
|---|---|
| `ATOMA_COPILOT_TOKEN` | Personal access token with `copilot` scope |
| `GITHUB_TOKEN` / `GH_TOKEN` | Fallback (not recommended for auto-detection) |

---

## Complete Example

```markdown
---
name: EngineerAgent
description: |
  Expert software engineer. Implements features, fixes bugs, and writes tests
  following the project's conventions.
model: openrouter/anthropic/claude-3.5-sonnet
provider: openai          # use OpenAI-compatible path even for this model
knows_about:
  - OrchestratorAgent
  - ReviewAgent
mcp_servers:
  - filesystem
  - shell
extra_body:
  temperature: 0.1
  max_tokens: 8192
metadata:
  team: backend
  tier: worker
---

You are a senior software engineer working on a Rust project.
Always write idiomatic, well-tested Rust code.
Prefer small, focused commits with clear messages.
```
