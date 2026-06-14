# Atoma GitHub Actions

Shared runtime actions for orchestrating LLM agents with MCP tools on GitHub.
These actions are building blocks — use them individually or together to build AI-powered workflows.

The canonical template that demonstrates how to compose these actions into a complete
multi-agent pipeline is at:

👉 **[yuma-seno/atoma-autonomous-delivery](https://github.com/yuma-seno/atoma-autonomous-delivery)**

That repository contains a ready-to-use `.github/` directory (workflows, agent definitions,
orchestration config, tool scripts) that you can copy into your own repository.

## Where to Start

- To **get started quickly**, go to [atoma-autonomous-delivery](https://github.com/yuma-seno/atoma-autonomous-delivery)
- To **understand each action's contract**, see the Common Runtime Actions section below
- To **use actions individually** in your own workflows, reference them as `yuma-seno/atoma/github/actions/<name>@v0.1.0`

## Directory Structure

```text
github/
└── actions/
    ├── setup-runtime/
    ├── prepare/
    ├── run/
    ├── post-result/
    ├── dispatch-next/
    └── parse-comment-command/
```

## How Each Action Can Be Used

All actions are pure composite actions with no hidden dependencies between them.
You can use any subset in your workflow.

| Action | Responsibility | Can omit when... |
|---|---|---|
| `setup-runtime` | Install MCP server dependencies (npm) | You don't use MCP or pre-install separately |
| `prepare` | Fetch GitHub events + restore session from `atoma-data` branch | You provide context-session.json yourself |
| `run` | Install Atoma CLI + execute agent | You call the LLM directly (no orchestration) |
| `post-result` | Post comment + persist session to `atoma-data` branch | You post results manually |
| `dispatch-next` | Read orchestration config + dispatch next agent | You want a single-agent workflow |
| `parse-comment-command` | Extract `/agent` from comment body | You parse commands yourself |

## Common Runtime Actions

### `setup-runtime`

Prepares the runtime based on the consumer repo's `.github/atoma`.

- Makes `.github/atoma/tools/scripts` executable and adds to PATH
- Restores npm cache
- Pre-installs MCP server dependencies

### `prepare`

Collects GitHub events and assembles the shared `context-session.json`.

- Fetches issue/PR event history
- Applies per-agent shared context policies
- Restores `session.json` from the `atoma-data` Git branch (an orphan branch used as persistent session storage)
- Determines whether to proceed to agent execution based on context differences

### `run`

Installs or builds the Atoma CLI and executes the agent.

- Loads agent definition and tools
- Builds environment variables for scripts from `orchestration.json`
- Passes `session.json` and `context-session.json` for inference
- Extracts the next agent directive from agent output

### `post-result`

Reflects execution results on GitHub and persists the session.

- Posts result comments to issues/PRs (with token usage and cost display)
- Adds 👀 reaction on agent handoff
- Records comment metadata in `session.json`
- Saves `session.json` to the `atoma-data` Git branch for persistent agent memory
- Posts failure comments on job failure

### `dispatch-next`

Reads the orchestration config and dispatches the next agent if one is requested.

- Tracks the auto-dispatch loop counter in `session.json` to prevent infinite handoff loops
- Loads the configured dispatch workflow from `orchestration.json`
- Dispatches the next agent via `gh workflow run`
- Posts a warning comment when the loop limit (5 consecutive no-new-event runs) is reached

Omitting this step from a workflow makes the run single-agent; useful for simple AI workflows that don't need multi-agent handoff.

### `setup-runtime`

### `parse-comment-command`

Extracts `/agent-name` style slash commands from the first line of a comment.
Separating this action keeps manual comment workflows thin.