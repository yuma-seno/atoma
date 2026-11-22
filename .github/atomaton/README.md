# `.github/atomaton/`

**This directory is yours.** It holds what a project says about itself — what to
verify, which agents exist, what they are told.

Three things first.

**Edit `config.yaml`.** It is the one file here you are expected to change, and
almost everything you will want is a setting in it —
[what `config.yaml` accepts](https://github.com/yuma-seno/atomaton/blob/main/docs/config/reference.md).

**A change takes effect on the next run.** There is nothing to build, install or
restart: once it is merged, the next agent to start is already working under the
new file.

**`.github/atomaton-runtime/` and `.github/workflows/` are not yours to edit.**
They are generated, and an upgrade replaces them wholesale and takes the edit with
it — [what is not here](#what-is-not-here). What you were trying to change is
almost always a setting in `config.yaml`.

Then the paths, because the path of each thing here is how you know what it is.

| Path | What it holds |
| --- | --- |
| `config.yaml` | Every setting this project declares. |
| `agent-definitions/<name>.md` | One agent: which model, which tools, and the role prompt. `<name>` is what `/<name>` dispatches. |
| `prompt-template.md` | The system prompt each role prompt is placed into. |
| `skills/<category>/<name>.md` | Instructions loaded on demand. `<category>/<name>` is the name an agent asks for. |
| `rulesets/main.json` | Branch protection, in GitHub's import format rather than ours. |
| `delegates/delegates.yaml` | Which server offers which delegate folders. Edit it to add one, or to change what they are called. |
| `delegates/<name>/agent.md` | One delegate: the provider, the model, and the role prompt its sub-run is started with. **The model is the field you are most likely to want.** |
| `delegates/<name>/tools.yaml` | What that sub-run may reach. Narrowing it here is what makes a delegate safe to give to an agent that must not write. |
| `scripts/` | Yours to create: the commands `config.yaml` names under `checks` and `deploy`. Nothing ships here, and nothing replaces it on upgrade. |

A command may of course live anywhere in your repository — `config.yaml` names a
path and does not care which. This is where to put one when you have no reason to
prefer somewhere else, and the reason to prefer this one is that it sits beside
the file that names it.

## The agents and skills that ship

**Both are a template, not a fixed set.** An agent exists exactly when a
definition for it does, and a skill exists exactly when a file for it does.
Nothing counts them, and nothing names them outside the files themselves.

| Agent | What it is for |
| --- | --- |
| `atomaton` | The agent a person reaches first: answers what it is asked, decomposes work into sub-issues, hands implementation to `engineer`, and aggregates what its children deliver. |
| `engineer` | Implements one engineer-ready leaf task, validates it, and opens a pull request. |
| `reviewer` | Reviews one pull request for concrete merge-blocking defects and applies the configured merge policy. |
| `architect` | Decides how to build something when it cannot be worked out from where the question was asked, and writes the decision down. Builds nothing. |
| `auditor` | Audits one area of the repository for security weaknesses when asked, and reports evidence-backed findings. Changes nothing. |

| Skill | When an agent loads it |
| --- | --- |
| `delivery/pipeline-setup` | `checks` is empty, a pull request comes back with no required check, or nothing deploys on merge. |
| `docs/customer-doc` | A run must deliver a document a customer will open — a docx report, a pptx deck or an xlsx estimate. |
| `engineering/environment` | Something needed is not installed, an install fails, or the work tree is broken. |
| `engineering/secrets-in-a-check` | A check you were asked for needs a real credential to mean anything. |
| `research/web-search` | The answer is not in this repository. |
| `security/on-demand-audit` | Asked to run a security audit, or dispatching one — the procedure both ends follow. |
| `security/gh-actions-surface` | An audit covers CI and workflow files. |
| `security/agent-prompt-surface` | An audit covers an agent, its prompts, or its tool configuration. |
| `writing/japanese` | Writing for a person in Japanese — the report, a question, a pull request body. |

`skills/project/` is yours outright — the template ships nothing there and an
upgrade will not fight you for it. The ones above are defaults the template also
expects you to tune. Adding an agent or a skill is a new file; the paths and the
frontmatter are in
[the documentation](https://github.com/yuma-seno/atomaton/blob/main/docs/agents/overview.md).

## What is not here

`.github/atomaton-runtime/` is the other half, and it is **not yours**: it is what
Atomaton runs, replaced wholesale on upgrade.

| Path | What it holds |
| --- | --- |
| `atomaton-runtime/tools/defaults.yaml` | The tool servers every run starts with, and the hooks that watch all of them. There to be read — it is the entry you override. |
| `atomaton-runtime/tools/mcp/*.ts` | The tool servers themselves. The file `atoma` is handed is written from `defaults.yaml` plus your `tools.servers` per run, and never lands in either directory. |
| `atomaton-runtime/tools/hooks/*.ts` | What inspects a tool call before or after it runs. |
| `atomaton-runtime/tools/packages.json` | The packages those shipped servers need. Yours for a server you added go in `tools.packages` in the config. |
| `atomaton-runtime/scripts/*.ts` | The programs the workflows run. |

`.github/workflows/` is where GitHub requires workflows to be, and is generated
too. So the rule has no exceptions: `.github/atomaton/` is the project's, and
everything else under `.github/` is the deliverable's.

The split is not tidiness. Deleting something in the runtime breaks a run outright:
the server does not start, and `atoma` stops before the first tool call. Editing
something in this directory degrades one at worst — a worse-informed agent, and the
run carries on. That line is drawn once, for the whole tree, rather than file by
file.

## The tool servers a run starts with

Every one of them, and they are not in `config.yaml`. They are in
`.github/atomaton-runtime/tools/defaults.yaml`, in the same schema as `tools.servers`
in your config, and that file is the one to read when you are about to override
one. What it declares is written into the file `atoma` is handed at the start of
every run.

The table below is what that file declares, and it is a list rather than a count on
purpose: the number was written down once and went stale the first time a server was
added.

| Server | What it is for |
| --- | --- |
| `files` | Reads, searches and changes files in the work tree. Its tools are named `read`, `grep`, `glob`, `edit`, `write`, `list`, with no server prefix. |
| `files_readonly` | The same server with the three that write left out, for agents that must not change anything. |
| `shell` | Runs one foreground command. Guarded, and holds no credentials of its own. |
| `github` | Issues, pull requests, comments, and every Git mutation. |
| `web` | Fetches a URL. Searching the web is a skill, not a tool. |
| `search` | Ranked search over this repository's issues and code. |
| `atomaton` | Atomaton's own operations: sub-issues, handoffs, stopping a run. |
| `attachments` | The three directions a file moves. `fetch_attachment` saves a file a person attached under /tmp, so a document can be opened and edited — and then published — instead of being pasted into the conversation; it reads only GitHub's two attachment hosts and sends no credential. `attach_file` uploads an image, a video or a document to GitHub's own attachment store and returns a URL and pasteable markdown for each, so it appears in a comment the way a drag-and-drop does; it is the one tool here that sends the run's GitHub token: to `api.github.com` for the repository's numeric id, and to `uploads.github.com` for the upload itself, which is the one host this change added. `publish_draft` puts drafts on a branch and returns a blob URL for each — plus, for an image, a raw URL and the `![alt](raw_url)` markdown that a comment renders in place. |
| `atomaton_env` | Rebuilding the run's environment, and concluding the run's own issue. |
| `delegate` | Runs one small piece of work in a sub-run and returns what it found. Offers the writing folders from `delegates.yaml`. |
| `delegate_readonly` | The same, with a sub-run that reads and searches and cannot change anything. |

An agent gets the ones its own `mcp_servers` names, and only those. A server
nobody names is never started, so there is nothing to gain by removing one — which
is why there is no way to.

The two delegate servers are one program, `mcp/delegate.ts`, each started with a
roster key. **Each folder under `delegates/` becomes one tool, named after the
folder**, so the writing server advertises `delegate` and `delegate_large` and the
read-only one advertises its own pair. A caller therefore chooses between them as
tools, and `delegates.yaml` is what says which folders each server offers.

The folders live under `delegates/`, with that roster. They are not agent
definitions: a delegate is started by the tool and never by a person, so it is
deliberately outside `agent-definitions/`, which is the namespace `/<name>`
dispatches from.

Two servers rather than one offering all four, and the split is the confinement:
an agent given the writing delegate can reach a write through it, so an agent that
must not write must not be given it.

The second folder of each pair exists because a delegated task is the one place a
free tier can be used at all: a top-level run starts from the issue, every comment
and the pull request's diff, which exceeds the tier's per-request cap before the
model says anything, whereas a delegate is handed only the task its caller wrote.

**Which of the two is free is read from the `model:` line, not from the name.** A
folder whose model ends in `/free` is described to the caller as free, with a limit
shared across every agent on that tier, and the caller is told to try it and read a
refusal as an answer. Point the folder at another model and the description moves
with it, without an agent definition being edited — which is why the instruction is
in the tool description rather than in a prompt. That is also the field a project
most wants to change here, and it is why these folders are under `.github/atomaton/`
rather than under `.github/atomaton-runtime/`, which an upgrade replaces.

**Why they are not in the file you edit.** Deleting one takes a capability from
every agent that named it, and `atoma` stops the run before a single tool starts
rather than continuing without it. `config.yaml` is the file this README calls
yours; sixty-six of its ninety-three lines used to be the tool servers, which is a
lot of machinery to keep in a file labelled that way. The line drawn was: **hide
what breaks when it is edited wrong, show what degrades.** A skill or a prompt
template edited badly makes an agent less well-informed and the run carries on;
those stay where you can reach them.

**What you can still do**, in `tools.servers`:

- **add one** — a name not in the table above is yours, and is merged in
- **override one** — the same name, field by field. Raising `request_timeout_secs`
  on `shell`, or routing a credential to `github` through `env`, says only that and
  inherits the rest, so an upgrade still moves the parts you did not touch

A server you add is started by something the runner may not have; `tools.packages`
in the config is where you name what to install for it. The shipped servers' own
packages are in `atomaton-runtime/tools/packages.json`, because they are not yours to
choose.

And in `tools.watch`, hooks of your own. They run after Atomaton's, on every call to
every server; they are added to what it watches rather than replacing it.

If you name a server that does not exist, `atoma` says so and lists the ones that
do — on the pull request, before anything runs.

## The paths are not configurable, deliberately

There is no setting that says where any of this lives. That is a decision, not an
omission, and it is the one place this deliverable chooses convention over
configuration.

**A path here is how a reader recognises what a file is.** `agent-definitions/`
says what it holds. A `definitions_dir` key pointing anywhere would say only that
somebody chose — and the next person has to open the config before they can read
the repository.

Three mechanisms are built around these two directories being roots, and a
redirected path breaks all three at once:

- The run grants both tool users read access to the machinery root. A file outside
  it is unreadable to the servers, and the symptom is "no server started" rather
  than "that path was wrong".
- `merge.governed_paths` defaults to `.github/**`. That is what keeps a change to
  a model, a tool list or a role prompt in a person's hands. Moving
  `agent-definitions/` elsewhere leaves that gate permanently, on a single
  approval that reads as tidying up.
- Upgrades replace these directories **by position** — `unzip -o` over them, then
  your `config.yaml` restored. A redirected tree is never upgraded again, and the
  new upstream copy lands beside it unread.

And one of them could never be a setting whatever was decided about the rest:
`.github/atomaton-runtime/scripts/` holds the programs that open the configuration. A
file cannot say where to find the thing that reads it.

## Why the core is configurable and this is not

`atoma`, the binary a run executes, takes every one of these paths as a
command-line argument and hardcodes no layout at all. That looks like the opposite
decision. It is not.

The core has no project of its own — it is handed a layout and works in whatever
it is given. **This deliverable is the layout.** The core's configurability is the
seam Atomaton passes these paths through, which is why they are settled here and
not again in your config.

So: settings describe policy — what to check, what may merge, what a tool may
reach. Paths describe structure, and structure is what a name is for.

## Changing any of this

Everything here except `config.yaml` is replaced wholesale on upgrade. Edit it and
the next upgrade takes your edit with it.

The file `atoma --tools-file` reads is in neither directory, so there is nothing
anywhere to edit: each run writes it into the runner's temp directory — the servers
above, with whatever `tools.servers` in the config adds or overrides — and
throws it away with the runner. It used to ship, which gave a repository a config
and a file generated from it that nothing here could regenerate — editing the
config changed nothing, and adding a server blocked every run. If an upgrade from a
release that shipped one left a `tools/tools.yaml` behind, nothing reads it; delete
it.

An upgrade only unpacks, so it deletes nothing either. A tree adopted before the
runtime moved still has `.github/scripts/` and `.github/atomaton/tools/` sitting
there, read by nothing — the workflows name the new paths. Delete them, so that
nobody edits a copy that no run will ever load.

`config.yaml` is yours: the upgrade deliberately restores it. If you need a
different agent, a different skill or a different tool, that is what a fork is
for — and `merge.governed_paths` is what puts such a change in front of a person
first.
