# atoma-spec-first

A template that places specification definition before implementation. It divides roles among planner, test-writer, implementer, and reviewer to prevent mixing specification and implementation context.

## Good Fit

- Wanting to emphasize TDD / specification by example
- Wanting to visualize acceptance criteria before implementation
- Wanting to narrow context by dividing agents into fine-grained roles

## Workflow Details

### [.github/workflows/atoma-entry.yml](.github/workflows/atoma-entry.yml)

The unified entry point that starts agents from new issues.

- Trigger: `issues: [opened, labeled]`
- Resolution order:
  1. **Label trigger**: When a label `atoma/<agent>` (e.g. `atoma/planner`) is added to an issue, the corresponding agent starts.
  2. **Body slash command**: If the first line of the issue body starts with `/<agent>` (e.g. `/planner`), the agent starts automatically on issue creation.
- Meaning: Humans can trigger agents either by writing a slash command in the issue body upfront, or by adding an `atoma/<agent>` label at any time after creation. This gives full control over when automation starts.

### [.github/workflows/atoma-manual-comment.yml](.github/workflows/atoma-manual-comment.yml)

The entry point for mid-process intervention or recovery.

- Trigger: issue comment created
- Behavior: Interprets slash commands and starts any agent
- Use case: Re-running or manually intervening with planner, test-writer, implementer, or reviewer

### [.github/workflows/atoma-reviewer-on-pr.yml](.github/workflows/atoma-reviewer-on-pr.yml)

A workflow that automatically starts PR reviews.

- Trigger: `pull_request` with `opened`, `synchronize`, or `ready_for_review`
- Condition: Non-draft PRs from non-Bot senders
- Behavior: Starts the reviewer

### [.github/workflows/atoma-implementer-on-changes-requested.yml](.github/workflows/atoma-implementer-on-changes-requested.yml)

Sends review rejections back to the implementer.

- Trigger: `pull_request_review` with `submitted`
- Condition: Review state is `changes_requested` and reviewer is not a Bot
- Behavior: Restarts the implementer

### [.github/workflows/atoma-runner.yml](.github/workflows/atoma-runner.yml)

All specification, implementation, and review phases converge into this reusable workflow.

- Trigger: `workflow_call` or `workflow_dispatch`
- Behavior: checkout, runtime setup, shared context building, agent execution, result comment posting
- Handoff: Follow-up for `create_pr` is the implementer; follow-up for `push_commits` is the reviewer
- Context policy: Planner and test-writer generally do not see `pr_diff`; implementer and reviewer see diffs

## Agent Roles

- `planner`: Converts issues into specifications and work order
- `test-writer`: Places executable specifications first
- `implementer`: Implements and fixes to satisfy specifications
- `reviewer`: Checks for specification deviation, quality, and design risks

## Expected Flow

1. The planner starts on issue opened, creates specifications, and hands off
2. The test-writer creates executable specifications and passes them to the implementer via PR
3. The implementer advances implementation to satisfy the specifications
4. The reviewer runs on PR review; changes requested sends back to the implementer
5. If needed, manual comments can re-run any phase

## Intentional Bias

- New issues are received by the planner
- The first assignee for a PR may be the test-writer rather than the implementer
- Follow-up for `create_pr` is the implementer
- Follow-up for `push_commits` is the reviewer
- Planner and test-writer focus on specifications and do not see unnecessary diffs