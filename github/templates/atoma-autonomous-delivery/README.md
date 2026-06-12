# atoma-autonomous-delivery

A template that drives progress from issue intake through implementation, review, and fixes among agents. Humans act as supervisors, but daily progress is assumed to be automated.

## Good Fit

- Wanting a standard pipeline from issue to implementation
- Wanting to use orchestrator-based decomposition and delegation
- Wanting the reviewer-engineer fix loop to run automatically

## Workflow Details

### [.github/workflows/atoma-entry.yml](.github/workflows/atoma-entry.yml)

The unified entry point that starts agents from new issues.

- Trigger: `issues: [opened, labeled]`
- Resolution order:
  1. **Label trigger**: When a label `atoma/<agent>` (e.g. `atoma/orchestrator`) is added to an issue, the corresponding agent starts.
  2. **Body slash command**: If the first line of the issue body starts with `/<agent>` (e.g. `/orchestrator`), the agent starts automatically on issue creation.
- Meaning: Humans can trigger agents either by writing a slash command in the issue body upfront, or by adding an `atoma/<agent>` label at any time after creation. This gives full control over when automation starts.

### [.github/workflows/atoma-manual-comment.yml](.github/workflows/atoma-manual-comment.yml)

The entry point for human intervention.

- Trigger: issue comment created
- Behavior: Interprets slash commands and passes any agent to `atoma-runner.yml`
- Use case: When you want to override the orchestrator's decision or re-run a specific agent

### [.github/workflows/atoma-reviewer-on-pr.yml](.github/workflows/atoma-reviewer-on-pr.yml)

The PR quality gate.

- Trigger: `pull_request` with `opened`, `synchronize`, or `ready_for_review`
- Condition: Non-draft PRs from non-Bot senders
- Behavior: Starts the reviewer

### [.github/workflows/atoma-engineer-on-changes-requested.yml](.github/workflows/atoma-engineer-on-changes-requested.yml)

A workflow that sends review rejections back to the engineer.

- Trigger: `pull_request_review` with `submitted`
- Condition: Review state is `changes_requested` and reviewer is not a Bot
- Behavior: Restarts the engineer (not implementer)
- Meaning: Drives the reviewer -> engineer fix loop via GitHub review events

### [.github/workflows/atoma-sub-issue-closed.yml](.github/workflows/atoma-sub-issue-closed.yml)

Sub-issue lifecycle manager.

- Trigger: `issues: [closed]`
- Behavior: When a sub-issue (identified by `atoma:parent=#<N>` in body) is closed, checks for remaining open siblings
- If siblings remain: posts a progress comment on the parent issue
- If all siblings complete: adds `atoma/orchestrator` label to the parent, re-invoking the orchestrator for aggregation

### [.github/workflows/atoma-runner.yml](.github/workflows/atoma-runner.yml)

A shared executor for all entry points above.

- Trigger: `workflow_call` or `workflow_dispatch`
- Behavior: checkout, runtime setup, shared context building, agent execution, result comment posting
- Features: Follow-up agents for `create_pr` and `push_commits` are the reviewer; `create_sub_issue` creates pending sub-issues (orchestrator controls when they start)

## Agent Roles

- `orchestrator`: Intake, problem decomposition, delegation, sub-issue lifecycle management, progress aggregation
- `engineer`: Implementation and fixes
- `reviewer`: Quality gate and fix handoff

## Expected Flow

1. The orchestrator starts when triggered (via label or body slash command)
2. The orchestrator assesses task granularity:
   - **Small tasks**: delegates directly via `/engineer`
   - **Medium/Large tasks**: decomposes into sub-issues via `create_sub_issue`
3. Sub-issues are created with `atoma/pending` label — orchestrator starts them one by one using `add_label.sh --label atoma/engineer --issue <N>`
4. Each sub-issue completion triggers progress notifications on the parent
5. When all sub-issues complete, the orchestrator is re-invoked for aggregation
6. The orchestrator consolidates results and either completes or plans the next phase

## Intentional Bias

- New issues are first received by the orchestrator
- The exit point for implementation is aligned to the reviewer
- The reviewer may automatically return issues to the engineer when needed
- Sub-issue delegation is included in practical operation