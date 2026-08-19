# Getting Started

This guide gets you from zero to a successful `atoma run` using only the current CLI contract.

## Prerequisites

- Rust toolchain `1.95.0` (see `rust-toolchain.toml`).
- One provider credential, which is also what selects the provider:
  - `OPENAI_API_KEY` → `openai`
  - `OPENROUTER_API_KEY` → `openrouter`
  - `ORCAROUTER_API_KEY` → `orcarouter`
  - `ANTHROPIC_API_KEY` → `anthropic`
  - `ATOMA_COPILOT_TOKEN` → `github-copilot`

Setting two of them is an error rather than a precedence. Name the provider with
`ATOMA_PROVIDER` or an agent definition's `provider:` when more than one is
available.

`GITHUB_TOKEN` and `GH_TOKEN` are accepted as GitHub Copilot credentials only when you explicitly select `github-copilot` with agent frontmatter or `ATOMA_PROVIDER`.

## Install options

From source in this repository:

```bash
# Build once
cargo build

# Or install a local binary
cargo install --path . --locked
```

If you do not install globally, use `cargo run -- <subcommand>` in examples below.

## First agent

Create a minimal definition:

```markdown
---
name: assistant
description: A concise assistant.
model: openai/gpt-5-mini
callable_by:
  - user
---
```

Save it as `agent.md`, then validate:

```bash
cargo run -- validate --agent-def agent.md
```

Success criteria:

- Output includes `Validation passed.`
- Exit code is 0.

## First run

```bash
echo "Write a 3-bullet summary of this repository." > prompt.txt
OPENAI_API_KEY=your_key_here cargo run -- run --agent-def agent.md --prompt-file prompt.txt
```

Success criteria:

- CLI prints assistant text to stdout.
- Exit code is 0.

If you want machine-readable output:

```bash
OPENAI_API_KEY=your_key_here cargo run -- run --agent-def agent.md --prompt-file prompt.txt --output json
```

## Preserve and resume session

Use the same file for both input and output:

```bash
OPENAI_API_KEY=your_key_here cargo run -- run \
  --agent-def agent.md \
  --in-session .atoma/session.json \
  --out-session .atoma/session.json \
  --prompt-file prompt.txt
```

Behavior:

- If the session file does not exist, Atoma starts with an empty session.
- If it exists, Atoma loads prior messages and appends new history.
- At completion, Atoma writes the updated session atomically.

## Next steps

- Configure defaults and profiles: [configuration.md](configuration.md)
- Define multi-agent contracts: [agents.md](agents.md)
- Add MCP tools and dynamic skills: [tools-and-skills.md](tools-and-skills.md)
- Learn runtime failure handling: [runtime.md](runtime.md)
