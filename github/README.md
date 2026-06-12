# Atoma GitHub Distribution

Atoma's GitHub distribution consists of shared runtime actions and self-contained templates.
This README is the overall entry point. For operational details and workflow specifics, refer to each template's README.

## Where to Start

- To choose a template, read this README first, then each template's README
- To understand workflow semantics, see the Workflow section in each template README
- To understand runtime action responsibilities, see the Common Runtime Actions section below

## Directory Structure

```text
github/
├── actions/
│   ├── setup-runtime/
│   ├── prepare/
│   ├── run/
│   ├── finalize/
│   └── parse-comment-command/
└── templates/
    ├── atoma-human-gated/
    ├── atoma-review-gate/
    ├── atoma-autonomous-delivery/
    └── atoma-spec-first/
```

## Template Selection Guide

| Template | Entry trigger | Auto-review | Auto fix loop | Best for |
| --- | --- | --- | --- | --- |
| [atoma-human-gated](templates/atoma-human-gated/README.md) | Manual (comment or label) | None | None | Teams that want humans to control phases |
| [atoma-review-gate](templates/atoma-review-gate/README.md) | Manual (comment or label) | Per PR | None | Teams that want to automate only reviews |
| [atoma-autonomous-delivery](templates/atoma-autonomous-delivery/README.md) | Issue body slash cmd or label | Per PR | reviewer -> engineer | Teams that want end-to-end automation from issue intake |
| [atoma-spec-first](templates/atoma-spec-first/README.md) | Issue body slash cmd or label | Per PR | reviewer -> implementer | Teams that want spec-first / TDD division of labor |

## Template Roles

### [atoma-human-gated](templates/atoma-human-gated/README.md)

A minimal configuration where humans choose between `/triager`, `/engineer`, and `/reviewer`.
Automatic handoff is mostly disabled, making it suitable for teams introducing AI as an assistant.

### [atoma-review-gate](templates/atoma-review-gate/README.md)

Keeps issue-side triggering manual, but automatically runs the reviewer as soon as a PR is opened.
The shortest path to adding an AI reviewer to an existing workflow.

### [atoma-autonomous-delivery](templates/atoma-autonomous-delivery/README.md)

The orchestrator receives new issues and drives them forward through implementation and review among agents.
Humans act as supervisors while routine progress is automated.

### [atoma-spec-first](templates/atoma-spec-first/README.md)

Separates planner, test-writer, implementer, and reviewer roles, placing specification definition before implementation.
The most information-rich template, also serving as a multi-agent reference.

## How to Use a Template

Copy the chosen template contents to your repository root:

```bash
cp -r github/templates/atoma-review-gate/. /path/to/your-repo/
```

Each template includes:

- `.github/workflows/atoma-*.yml`
- `.github/atoma/agent-definitions/*.md`
- `.github/atoma/tools/tools.yaml`
- `.github/atoma/tools/scripts/*`
- `.github/atoma/orchestration.json`
- `.github/atoma/orchestration.schema.json`

## Understanding the Common Workflow Pattern

Templates combine thin trigger workflows with a shared reusable workflow:

1. An entry workflow is triggered by issue comments (`atoma-manual-comment.yml`), issue open/label (`atoma-entry.yml`), or PR events
2. The entry workflow calls `atoma-runner.yml`
3. `atoma-runner.yml` executes `setup-runtime -> prepare -> run -> finalize` in sequence
4. Handoff to the next agent is determined by `.github/atoma/orchestration.json` and the agent output directive

Even if multiple triggers fire on the same PR update, the `prepare` action checks for shared context differences to suppress no-op runs.

### Entry via Issue Body or Label

`atoma-entry.yml` supports two ways to trigger agents from issues:

1. **Slash command in issue body**: If the first line of a new issue body is `/orchestrator` (or any agent name), that agent starts immediately.
2. **Label trigger**: Adding an `atoma/<agent>` label (e.g. `atoma/engineer`) to any issue starts the corresponding agent. This works on both new and existing issues.

This gives humans full control: write the slash command upfront for automation, or create the issue first and add the label later when ready.

### Sub-issues

`create_sub_issue` creates sub-issues with an `atoma/pending` label. The sub-issue is **not** automatically triggered — the orchestrator (or a human) must explicitly add the trigger label to start the worker agent.

**Sequential execution**: For tasks with dependencies, the orchestrator adds labels one at a time:

```bash
add_label.sh --label atoma/engineer --issue <SUB_ISSUE_NUM_1>
# wait for completion...
add_label.sh --label atoma/engineer --issue <SUB_ISSUE_NUM_2>
```

**Auto-aggregation**: `atoma-sub-issue-closed.yml` detects when the last sibling sub-issue closes and re-invokes the orchestrator by adding an `atoma/orchestrator` label to the parent issue.

## Combining Templates

When mixing templates, consider not just workflows but also `.github/atoma`:

- Workflow: when to invoke which agent
- `orchestration.json`: how `create_pr`, `push_commits`, and `create_sub_issue` handoffs work
- Agent definition: each agent's autonomy level and output contract
- Shared context: who sees PR diffs and who retains issue context

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
- Restores `session.json` from cache
- Determines whether to proceed to agent execution based on context differences

### `run`

Installs or builds the Atoma CLI and executes the agent.

- Loads agent definition and tools
- Builds environment variables for scripts from `orchestration.json`
- Passes `session.json` and `context-session.json` for inference
- Extracts the next agent directive from agent output

### `finalize`

Reflects execution results on GitHub and connects to the next run.

- Posts result comments to issues/PRs
- Records comment metadata in `session.json`
- Saves session cache
- Dispatches the next agent if a directive exists
- Posts failure comments on job failure

### `parse-comment-command`

Extracts `/agent-name` style slash commands from the first line of a comment.
Separating this action keeps manual comment workflows thin.