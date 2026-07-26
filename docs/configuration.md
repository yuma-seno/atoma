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

Valid providers:

- `openai`
- `github-copilot`
- `anthropic`

Credential essentials:

- `openai`: requires `OPENAI_API_KEY`
- `github-copilot`: accepts `ATOMA_COPILOT_TOKEN`, `GITHUB_TOKEN`, or `GH_TOKEN`; only `ATOMA_COPILOT_TOKEN` participates in provider auto-detection
- `anthropic`: requires `ANTHROPIC_API_KEY`

When using only `GITHUB_TOKEN` or `GH_TOKEN`, select `github-copilot` explicitly in agent frontmatter or with `ATOMA_PROVIDER=github-copilot`.

Optional provider endpoints:

- `OPENAI_BASE_URL` (default: `https://openrouter.ai/api/v1`)
- `ANTHROPIC_BASE_URL` (default: `https://api.anthropic.com`)

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
OPENAI_BASE_URL = "https://openrouter.ai/api/v1"

[profile.review.env]
ATOMA_PROVIDER = "openai"
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
- `mcp_servers` entries exist in `tools.yaml` when a tools file is given.
