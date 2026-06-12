# atoma-human-gated

A template where humans decide phase transitions. AI does not automatically proceed to the next phase, and does not auto-start on issue opened or PR opened.

## Good Fit

- Early adoption phase where you want to start with manual GitHub operations
- Regulatory or audit requirements that minimize automatic delegation
- Using AI as a reviewer or implementer while keeping humans in charge of progress management

## Workflow Details

### [.github/workflows/atoma-manual-comment.yml](.github/workflows/atoma-manual-comment.yml)

The entry point where humans explicitly invoke agents.

- Trigger: comment on issue or PR
- Target: comments from non-Bot users
- Behavior: Interprets `/triager`, `/engineer`, `/reviewer` on the first line and calls `atoma-runner.yml`
- Use case: When you want a human to decide "who should run now in this context"

### [.github/workflows/atoma-runner.yml](.github/workflows/atoma-runner.yml)

A shared reusable workflow for all agent executions.

- Trigger: `workflow_call` or `workflow_dispatch`
- Behavior: checkout, runtime setup, shared context building, agent execution, result comment posting
- Feature: Skips agent execution if there are no differences in the shared context
- Meaning in this template: The runner itself is shared, but subsequent handoff is stopped by orchestration

## Agent Roles

- `triager`: Investigation, issue organization, presenting options for human decision
- `engineer`: Implementation, local changes, PR preparation
- `reviewer`: PR and diff review

## Expected Flow

1. A human comments `/triager` or `/engineer` on an issue
2. The human decides the next slash command based on the agent's results
3. When a PR is ready, the human comments `/reviewer`
4. All subsequent phase transitions are explicitly decided by the human

## What Is Intentionally Not Automated

- `create_pr` does not auto-start a follow-up agent
- `push_commits` does not auto-start a follow-up agent
- `create_sub_issue` only creates a sub-issue without auto-triggering
- Issue opened, PR opened, and changes requested alone do not start agents

## Why This Template Exists

Rather than a minimal configuration, this template explicitly defines human approval points for minimal practical operation.
It serves as a comparison baseline when adding reviewer auto-start or issue intake automation to other templates.