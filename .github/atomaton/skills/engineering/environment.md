---
name: engineering/environment
description: Load when something you need is not installed, an install fails, or the work tree is broken — where a missing dependency or tool belongs, and how to get it without spending the run.
---

# When something you need is not there

Three questions, in this order. Getting the first one right is most of it.

## 1. Is it a library your project declares?

A package your code imports, named in `package.json`, `Cargo.toml`,
`requirements.txt`, `go.mod`, `pyproject.toml`.

**Edit the manifest and install it.** You can write the work tree, and the install
goes there too. This is ordinary work, part of the change, and it gets committed
and reviewed with the rest.

```
bun add zod          # or cargo add / pip install -r / go get
```

If the install command itself fails for lack of permission, it is not a library
question — go to 2.

## 2. Is it a system package, or a globally installed tool?

`apt-get install`, `npm install -g`, anything that writes outside the repository.

**You cannot install it.** You have no `sudo`, `/usr/local` is not writable, and
that is deliberate: an install done during a run does not exist on the next one, so
it would work once and then look like a flake.

It belongs in `environment.setup_commands` in `.github/atomaton/config.yaml`:

```yaml
environment:
  setup_commands:
    - sudo apt-get install -y libpq-dev
    - bun install --frozen-lockfile
```

**Add it, then say so in your report and stop.** That file is a governed path — a
person merges it — and until they do, the package is not there. Do not reload
hoping it will appear: the setup commands come from the default branch, so a line
you just added to your branch is not in them yet. You would get the same
environment back and spend a run finding out.

## 3. Is the environment broken, or does it need what you just declared?

You added a dependency to a manifest and want it installed by the project's own
command. Or you deleted `node_modules` and want it back. Or an install left
something half-done.

**`reload_environment`.** It re-runs `environment.setup_commands` as a
privileged step against your current work tree, then starts a new run.

The split is what makes it safe and what makes it useful: **the commands come from
the default branch, the data from your work tree.** So your edited manifest gets
installed by a command you did not write and cannot change.

Two things to know before you call it:

- **Your session ends immediately.** Commit what is worth keeping first. Leave
  notes in `/tmp/atomaton-workspace`, which survives into the next run.
- **There is a limit**, because each reload starts a new run and resets the
  run's time budget. The tool tells you where you stand. When it refuses, report
  what you found — that is the useful thing left.

## What not to do

**Do not install things ad hoc and carry on.** It works for the rest of your run
and for nobody afterwards, including CI. The check job runs
`environment.setup_commands` and nothing else, so a passing test in your shell and
a failing one in CI is exactly what that produces — and the failure comes back to
an agent that cannot reproduce it.

**Do not edit the setup commands to work around a failing install.** If
`bun install --frozen-lockfile` fails because the lockfile is stale, the answer is
the lockfile, not `--no-frozen-lockfile`.

**Do not guess whether something is installed.** Ask:

```
command -v pg_config
bun pm ls | grep zod
```

`$HOME` cannot be listed, so look things up by name rather than by browsing.
