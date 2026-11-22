---
name: security/gh-actions-surface
description: Load when a security audit covers CI and workflow files — how a GitHub Actions workflow is reached from outside, which values it trusts, what a token in it may do, and what evidence each of those answers with.
---

# The workflow surface

A workflow file is code that runs with a token, triggered by events somebody outside
the repository can cause. It is a distinct surface from the application's own, and
almost all of its weaknesses are one of four shapes.

The rules below are about the shapes. Which of them your project actually exposes is
read from your workflows, not assumed.

## 1. A value from outside expanded into a script

Workflow files interpolate before the shell sees the line. `${{ }}` inside a `run:`
block is replaced by the value at parse time, so a value that comes from outside
becomes shell syntax rather than an argument to it.

The values to trace are the ones the event supplies: an issue or pull request
**title**, **body**, **branch name**, **commit message**, **author login**, a
**label**, a comment's body, a **workflow input**, and anything read out of a file
somebody else can write.

Two shapes are safe and are not findings:

- the value reaches the shell through `env:` and is used as `"$VARIABLE"` — the
  shell expands it as data, and quoting keeps one argument;
- the value reaches an action's input, where it is a string the action parses,
  provided the action does not itself pass it to a shell.

And one is not: a `run:` block holding `${{ }}` where the value is written into a
command, a filename, or a heredoc.

Evidence: the workflow file and line, the expression, and the event field the
expression reads. A finding that cannot name the event field has not established
that the value is external.

## 2. A trigger that trusts the wrong branch

`pull_request` runs the workflow from the pull request's own branch, with a read-only
token and no secrets. `pull_request_target` runs it from the base branch, **with the
base repository's token and secrets** — and is safe only if it never checks out and
runs the pull request's code.

The finding is the pair: a `pull_request_target` trigger whose job later checks out
the head branch, or fetches and executes anything from it. Naming that combination is
the whole of the evidence — each half alone is ordinary.

The same question for other triggers: `issues`, `issue_comment` and
`workflow_run` are all events an outside contributor can cause, and what makes them
safe is what the job then does with what those events carry.

## 3. A dependency that may move

Everything a workflow pulls in executes: an action (`uses:`), a container image, a
package installed by a step, a script fetched and run.

An action pinned to a tag or a branch is one the tag's owner can move, and a branch
is worse. Pinned to a full commit SHA is pinned. The finding is the line, and how it
is pinned; the fix is the SHA. An action from the same repository at a version branch
is a lower severity than a third party's, and that distinction belongs in the
severity rather than in whether it is reported.

Images are the same question with a tag: `:latest` and a mutable tag are not pinned,
a digest is.

## 4. A token that may do more than the job needs

`permissions:` at the workflow or job level is what the job's token may do, and the
repository default may be broader than the job needs. The finding is a job that
writes — comments, pushes, releases, packages, deployments — where nothing in the
job does the thing that permission allows; or a job that holds `write-all` or
inherits the default while only reading.

Trace it both ways: what the job's steps actually do (the `gh` calls, the git
commands, the API an action reaches) and what its token permits. A check that only
reads the `permissions:` block reports nothing on the workflow that omits it.

Two related shapes worth reading for:

- **`GITHUB_TOKEN` handed to a script that does not need it**, or passed into a
  command as an argument where it lands in a log.
- **A secret interpolated into a `run:` block**, which puts it in the shell's command
  line and in the job log together. It reaches the process through `env:` or not at
  all.

## 5. A runner that is not the repository's

A `runs-on` naming a self-hosted runner puts a job on a machine whose state the
repository does not control, and a job that runs a pull request's code there runs it
on that machine. Whether that is a finding depends on whether the trigger can be
caused from outside the project — the same question as section 2, asked of a
different target.

## What to record while reading

For each workflow in the area:

- its triggers, and for each one whether an outside contributor can cause it;
- every `${{ }}` in a `run:` block, and the event field behind it;
- every `uses:` and image, with how it is pinned;
- the job's permissions, and what its steps actually do;
- every secret it references, and where the value goes.

That table is the evidence for every finding this page produces, and it goes into
the report's scope section so a later round can be compared with it.
