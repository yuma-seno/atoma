# atoma-review-gate

A template that starts automation from the PR boundary. Human judgment is retained for issue-side initiation, while the reviewer runs automatically only when PRs are opened or updated.

## Good Fit

- Prioritizing PR quality management over issue triage
- Wanting humans to decide when to start implementation while automating only reviews
- Wanting to insert an AI reviewer into an existing development workflow

## Workflow Details

### [.github/workflows/atoma-manual-comment.yml](.github/workflows/atoma-manual-comment.yml)

A manual entry point that accepts `/engineer` and `/reviewer` on issues or PRs.

- Trigger: issue comment created
- Target: comments from non-Bot users
- Behavior: Interprets slash commands and passes them to `atoma-runner.yml`
- Use case: When you want to start an engineer from an issue, or explicitly re-run a reviewer on a PR

### [.github/workflows/atoma-reviewer-on-pr.yml](.github/workflows/atoma-reviewer-on-pr.yml)

A PR-boundary workflow that automatically starts the reviewer.

- Trigger: `pull_request` with `opened`, `synchronize`, or `ready_for_review`
- Condition: Non-draft PRs from non-Bot senders
- Behavior: Always passes `reviewer` to `atoma-runner.yml`
- Meaning: Ensures the quality gate is always applied when a PR is opened or updated

### [.github/workflows/atoma-runner.yml](.github/workflows/atoma-runner.yml)

Both comment-triggered and PR-triggered executions converge here.

- Trigger: `workflow_call` or `workflow_dispatch`
- Behavior: checkout, runtime setup, shared context building, agent execution, result comment posting
- Feature: No-op if there are no differences in the shared context, reducing the risk of runaway executions from duplicate triggers
- Note: In this template, follow-up agents for `create_pr` and `push_commits` are also aligned to the reviewer

## Agent Roles

- `engineer`: Implementation and PR creation
- `reviewer`: PR diff review and quality gate

## Expected Flow

1. A human comments `/engineer` on an issue to start implementation
2. The engineer creates changes and opens a PR
3. The reviewer starts automatically on PR opened or synchronize
4. Based on the reviewer's results, the human decides whether to merge or call `/engineer` again

## What Is Intentionally Not Automated

- The reviewer does not automatically restart the engineer
- Nothing happens on issue opened alone
- Sub-issue orchestration is not included

## Why This Template Exists

This template is for cases where you want to reliably automate only PR reviews without disrupting the human-driven implementation workflow.