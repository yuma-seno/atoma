---
name: delivery/pipeline-setup
description: "Load when `checks` is empty in config.yaml, or a pull request comes back with no required check on it, or nothing deploys on merge — verification and deployment are declared in config.yaml, not written as workflow files."
---

# Setting up verification and deployment

Read this when `checks` in `.github/atomaton/config.yaml` is empty, when a pull
request comes back with no required check on it, or when nothing deploys on merge
and the work in front of you needs one of those. Not a procedure to follow on
request — a description of how this is arranged here, because the obvious approach
does not work.

## You cannot write a workflow. You do not need to.

`GITHUB_TOKEN` is refused a write under `.github/workflows/**`. Measured where this
was written: by identity rather than by a rule someone set, on every path and
branch tried, through `git push` and through the API alike, and with no permission
in the token's own set that grants it.

**What would show this has changed is a workflow write that is ACCEPTED**, or a
refusal whose own words name a permission the token could be granted. Neither is
an answer you are hoping for, so this is not a question to put to the endpoint:
make the change the paragraph implies. Everything a pipeline actually *does* is a
command, and a command goes in `.github/atomaton/config.yaml`, which you can write
— two workflows that already ship run them. The expected refusal is confirmation
rather than news: if one does arrive, quote its own words in your report and stop.
Do not retry it as another token, another request shape or another branch, and do
not ask a person to paste a workflow file in its place — a pipeline outside the
reviewed path is what the refusal exists to prevent, and the file you can write
already does the job.

## The environment

What the project needs *installed* is separate from what verifies it:

```yaml
environment:
  setup_commands:
    - bun install --frozen-lockfile
```

Write it here once and every job runs it: the agent's own shell, the checks, and
the deployment. That is the point of the separate block. Putting `bun install` at
the front of a `checks.from_pull_request` entry instead looks equivalent and is not —
the agent's shell then has the dependencies and CI installs them again, or the
reverse, and the two environments drift. A test that passes for the agent and
fails in CI comes back to an engineer as a defect that does not reproduce.

System packages belong here too, and only here. An agent cannot install one during
a run.

## Verification

`checks` has two arms and takes exactly one. Fill in Atomaton's lists and the shipped
workflow runs your commands:

```yaml
checks:
  from_pull_request:
    - name: verify
      commands:
        - bun run typecheck
        - bun test
```

Each entry becomes one job in `atomaton-check.yml`, running after the environment setup
above; its commands run in order and the first failure ends that job.
Whatever a contributor would type to check the project locally is what belongs
here — read the README, the package manifest's scripts, and any CONTRIBUTING
file before writing this, rather than guessing a stack.

**If `checks.your_workflow` already names a workflow, that one is correct.** A
repository with its own CI has it for reasons that are not in front of you. Leave
both alone, and in particular do not add a list beside it: a section
carrying both arms is reported as a configuration error rather than resolved by a
precedence rule, so the change comes back rejected instead of half-applied.

## Deployment

The same two arms, and the same rule — Atomaton's lists, or `deploy.your_workflow`,
never both. Here there are three lists, one per event that ships:

```yaml
deploy:
  on_merge:
    - name: staging
      branches: [develop]
      commands: ["./scripts/deploy.sh staging"]
  on_tag:
    - name: production
      tags: ["v*"]
      branches: [main]
      commands: ["./scripts/deploy.sh prod"]
  on_demand:
    - name: rollback
      commands: ["./scripts/rollback.sh"]
```

`tags` exists in `on_tag` and is required there. `branches` exists in both `on_merge`
and `on_tag`, meaning the same thing in each — which branch's reviewed content this
deployment ships — and omitting it means the default branch. A pattern is a literal
or a prefix followed by `*`.

A tag names a COMMIT, not a branch, so `on_tag`'s `branches` is what keeps a tag on
an unreviewed commit from deploying. `on_demand` entries answer to no event and run
only when named: `gh workflow run atomaton-deploy.yml -f target=rollback`.

Deployments run one at a time, in declared order, and the first failure stops the
rest. `$ATOMATON_DEPLOY_TARGET` holds the entry's name inside its commands.

**A branch named in `branches` must require a pull request**, in either list,
through a ruleset covering it. The run is refused otherwise: a branch anyone can
push to is a deployment anyone can run, with its credentials, on a commit nobody
read.

The shipped ruleset covers the default branch and nothing else, so writing
`branches: [develop]` means adding a ruleset for `develop` as well. That is a
repository setting rather than a file, so say so rather than trying to write it.

## Credentials

Never write a credential into `config.yaml` or into a command. Both are committed
in plain text.

A secret is added to the repository by a person, and then *named* in the list for
the place that needs it:

```yaml
checks:
  from_default_branch:
    - name: cloud-names
      secrets: ["AWS_ROLE_ARN"]
      commands: ["./scripts/check-env-names.sh \"$ATOMATON_PR_TREE\""]
deploy:
  on_tag:
    - name: production
      tags: ["v*"]
      secrets: ["AWS_ROLE_ARN"]
      commands: ["./scripts/deploy.sh prod"]
```

It arrives as an environment variable under that name, in that job and no other.
`tools.secrets` is separate on purpose and must not be merged with these: it is the
only list that enters the agent's own environment.

**`checks.from_pull_request` has no `secrets` key, and cannot.** Those commands are
the pull request's own and it may rewrite them, so a credential named beside one is
one the change being judged can read. A check that needs a credential goes in
`checks.from_default_branch`, whose commands come from the default branch and which
is handed the pull request as a path at `$ATOMATON_PR_TREE`.

**`tools.secrets` needs a second step, and the others do not.** Naming a secret
there authorises the run to hold it; it does not deliver it to any tool. The tool
that needs it must also name it in its own `env`, which is written under
`tools.servers` in the same file:

```yaml
tools:
  secrets: ["SLACK_TOKEN"]
  servers:
    slack:
      command: mcp-server-slack
      env:
        SLACK_TOKEN: "${SLACK_TOKEN}"
```

`tools.servers` is empty in a fresh config and is **additive**: it holds servers a
project adds, and overrides of the ones Atomaton ships. `slack` above is an addition,
so it declares the server in full, starting with the `command` that starts it.

**To route a credential to a server Atomaton ships** — `shell`, `github`, `web`,
`search`, `atomaton`, `atomaton_env`, `files`, `files_readonly` — write its
name with an `env` and nothing else. A shipped name is merged field by field, so
the command, the hooks and the timeout stay as they ship:

```yaml
tools:
  servers:
    <the shipped server's name>:
      env:
        API_TOKEN: "${API_TOKEN}"
```

Do not reconstruct the rest of a shipped entry to do this. You cannot read it in
`config.yaml` — it is not there — and a copied `command` or `args` freezes that
server at today's values, so the next upgrade moves nothing you pasted.

A reference, never a value. Every tool that does not name it — including `shell` —
cannot see it, and that is deliberate rather than a gap to fix. If a tool reports
a missing credential, the question is whether that server's `env` names it: for a
shipped server, an empty `tools.servers` means it was never routed, and that is
the answer rather than a sign something is missing from the file.

`config.yaml` is the only place this goes. There is no tools file in the
repository: the one `atoma` is handed is written at the start of each run, from
the shipped servers and whatever `tools.servers` adds, and deleted with the
runner. Routing a credential anywhere else produces a tool that never receives it,
with nothing reporting why.

`checks` and `deploy` need no routing step: their commands run in a workflow of
their own rather than beside an agent.

**Do not invent a secret name and hope.** If a deployment needs a credential you
cannot see, say so in your report and name exactly which one — a person adds it
and tells you what they called it. A guessed name produces a run that warns, then
fails somewhere unrelated.

**A name you add does not take effect until it is merged.** These lists are read
from the default branch, not from the branch a run is working on, so a credential
you declare will not be present in the run reviewing your own pull request. That
is intended. Do not conclude the mechanism is broken and work around it, and do
not try to test it by reading the value — say in your report which secret the
work now needs.

Prefer no credential at all where the platform allows it. `atomaton-deploy.yml`
declares `id-token: write`, so a cloud provider's OIDC login is available and is
better than any long-lived key.

## What commands cannot express

Reach for a person, not a workaround, when you need:

- a job's `permissions` beyond what the shipped workflows declare. Checks get
  `contents: read` and a `GITHUB_TOKEN` in `GH_TOKEN`, so `gh` works. Deployments
  get `contents: write` and `id-token: write`, so cutting a release and an OIDC
  login both work
- a deployment approval gate (`environment:`) — `environment:` takes no
  expression, so configuration cannot reach it
- a trigger outside merge, tag and manual dispatch — schedules in particular
  cannot come from configuration
- GitHub's own artifact store or cache

Most limits people assume are here are not. Service containers work through
`docker run`. A matrix works as a loop, losing only parallelism. Those are
commands; write them.

## The required check

If a ruleset requires a status check, its `context` must equal the job name that
produces it. `.github/atomaton/rulesets/main.json` ships already matched to
`atomaton-check.yml`. **Do not edit either side to make them agree** — a mismatch
does not fail a pull request, it leaves it waiting forever on a check that will
never report, and no amount of re-running fixes it. If they appear mismatched,
report it.

## Finishing

Changing `.github/atomaton/config.yaml` is a governed change: you may write it and
open the pull request, and a person merges it. That is expected, not an
obstruction. Say plainly in the pull request what will now run, on which events,
and which secrets a person still has to add.
