# Configuration

Atoma resolves runtime settings from CLI flags, `atoma.toml`, and hard defaults.

## Generate `atoma.toml`

```bash
atoma init > atoma.toml
```

If you are running from source:

```bash
cargo run -- init > atoma.toml
```

Atoma discovers `atoma.toml` from the current directory upward through ancestor directories.

## Precedence rules

Run settings precedence (high to low):

1. CLI argument
2. selected profile in `atoma.toml`
3. `[defaults]` in `atoma.toml`
4. hard-coded default

Environment variable values loaded from config follow:

1. existing process environment
2. `[profile.<name>.env]`
3. top-level `[env]`

At runtime, config-defined env values are only injected when that key is not already set in the process.

## Provider selection

Provider resolution order:

1. `provider:` in agent frontmatter
2. `ATOMA_PROVIDER` environment variable
3. auto-detection

One provider, one credential, one endpoint, and one variable that moves it:

| Provider | Credential | Endpoint | Override |
| --- | --- | --- | --- |
| `openai` | `OPENAI_API_KEY` | `https://api.openai.com/v1` | `OPENAI_BASE_URL` |
| `openai-responses` | `OPENAI_API_KEY` | `https://api.openai.com/v1` | `OPENAI_BASE_URL` |
| `openrouter` | `OPENROUTER_API_KEY` | `https://openrouter.ai/api/v1` | `OPENROUTER_BASE_URL` |
| `orcarouter` | `ORCAROUTER_API_KEY` | `https://api.orcarouter.ai/v1` | `ORCAROUTER_BASE_URL` |
| `anthropic` | `ANTHROPIC_API_KEY` | `https://api.anthropic.com` | `ANTHROPIC_BASE_URL` |
| `github-copilot` | `ATOMA_COPILOT_TOKEN` | `https://api.githubcopilot.com` | `COPILOT_BASE_URL` |

`openai` and `openai-responses` are one vendor reached two ways — chat completions
and the Responses API — so they share a credential and an endpoint. That pair is also
how to reach a provider with no row of its own: point `OPENAI_BASE_URL` at anything
that speaks either dialect. What you give up by doing that instead of using a named
provider is that the run's log says `openai`, so where it went is only visible in the
environment.

**Auto-detection is by credential.** Exactly one present selects that provider; two
present is an error naming both, because which to use is not something the
credentials decide — name it with `ATOMA_PROVIDER` or the agent's `provider:`, or
remove the one this run should not use. When `OPENAI_API_KEY` is the one present, the
chat-completions row wins; `openai-responses` has to be asked for by name.

`github-copilot` also accepts `GITHUB_TOKEN` or `GH_TOKEN`, and those deliberately
take no part in auto-detection: a run that talks to GitHub has one anyway, so
detecting Copilot from them would make every such run ambiguous. Select
`github-copilot` explicitly to use them.

`ATOMA_APP_NAME` and `ATOMA_APP_URL` are sent to routers that attribute requests to
an application (OpenRouter reads them as `X-Title` and `HTTP-Referer`). Only the
providers that read them receive them.

## Request timeout and retries

`ATOMA_LLM_TIMEOUT` bounds a single completion request, in seconds (default `300`). Absent, non-numeric, and zero values fall back to the default.

Treat this as a stall detector rather than a generation budget. A provider that has returned nothing for several minutes has usually stopped responding altogether, and raising the ceiling only delays detecting that. Raise it when a model legitimately generates for longer than the default; lower it to abandon a stalling endpoint sooner.

Each request is attempted up to 3 times, with a 1s then 4s pause between attempts. Retried:

- connect, timeout, and body/decode errors
- HTTP 429 and 5xx
- a response body that was cut off mid-payload

Not retried, because the outcome cannot change:

- any other non-success status
- a provider error object returned under HTTP 200
- a body whose JSON structure does not match the expected response shape

Worst-case wall clock for one request is therefore roughly `3 × ATOMA_LLM_TIMEOUT`. Account for that when raising the timeout under a CI job limit.

## Representative config

```toml
[defaults]
agent_def = "agents/orchestrator.md"
tools_file = "tools.yaml"
skills_dir = "skills"
template = "prompt-template.md"
max_iterations = 80
output = "text"

[profile.review]
agent_def = "agents/reviewer.md"
max_iterations = 30
output = "json"

[env]
ATOMA_PROVIDER = "openrouter"

[profile.review.env]
ATOMA_PROVIDER = "anthropic"
```

## Paths and profile notes

- Path fields in config (`agent_def`, `tools_file`, `skills_dir`, `template`) are consumed as provided.
- `--profile` requires a discovered `atoma.toml`.
- Unknown profile names fail fast.
- Default `agent_def` is `agent.md` when none is supplied.
- Default `max_iterations` is `50`.
- Default `output` is `text`.

## Validation workflow

Validate definitions before run:

```bash
atoma validate --agent-def agents/orchestrator.md --tools-file tools.yaml
```

Validation checks:

- Agent definition parses.
- `knows_about` files exist and are parseable.
- Each `knows_about` target includes `agent` in `callable_by`.
- `callable_by` values are limited to `user` or `agent`.
- `extra_body` does not include reserved keys `model` or `messages`.
- `extra_body.tools`, when present, is an array so it can be merged with the runtime tool definitions. See [agents.md](agents.md).
- `mcp_servers` entries exist in `tools.yaml` when a tools file is given.
