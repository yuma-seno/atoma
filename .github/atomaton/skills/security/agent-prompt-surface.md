---
name: security/agent-prompt-surface
description: Load when a security audit covers an autonomous agent, its prompts, or its tool configuration — how text that an outside party controls reaches instructions something acts on, what each agent may reach, and how a sub-run can carry reach its caller does not have.
---

# The agent and prompt surface

Where a program is written in code, an agent's behaviour is written in text — and
some of that text comes from outside. The surface is the path between the two, plus
the set of tools at the end of it.

This applies to any setup where a model is given instructions and tools: an agent
definition, a prompt template, a skill, a sub-agent, or a code-review bot. The
questions are the same and the names differ.

## 1. Text that reaches instructions

The first question is always: **can something an outside party controls end up in a
place that is read as an instruction?**

The usual carriers, none of them suspicious on its own:

- an issue or pull request title or body, a comment;
- a page or a document fetched from the network, or a file a person attached;
- a tool result — a search hit, a log line, a diff, a file's contents;
- a repository file, in a repository whose contributors are not all trusted;
- another agent's message, which is text like any other by the time it arrives.

The question is not whether the text is in the prompt — everything a run reads is —
but whether anything acts on what it says. Text that is summarised, quoted, or
matched against a pattern is data. Text that the agent is told to follow, or that
lands where the agent's own instructions live, is a route.

The distinction that decides most of these:

- **An agent that reports is a lower risk than an agent that acts.** A run that
  writes a comment nobody executes has given an attacker a paragraph. A run that
  merges, pushes, publishes or deploys has given them that action.
- **Text injected into a session is a candidate, not yet a finding.** The finding is
  the effect: what can the injected instruction make this run do, with the tools it
  has? That is section 2, and it is why the two sections are read together.

Evidence: the origin of the text (which field, which host, which tool), the code or
prompt that puts it in front of a model, and the instruction at the agent's disposal
that it could then act on.

## 2. What the agent may reach

An agent's tool set is its blast radius, and it is declared. Read it as a whole set
rather than a list of conveniences:

- **Reading tools are not writable.** A run given both `files` and a shell can change
  the tree by either. A run given only a read-only server and a shell has not lost
  the ability to write — the shell has it. If the intent is "this agent must not
  change anything", the shell has to be absent or the platform has to be able to
  withhold the write, and that is the check to run.
- **A token travels with the server that holds it.** An agent that can call the
  GitHub tools can comment, label, push and merge, whatever its role prompt says,
  because the prompt is not an enforcement mechanism. Where a project relies on the
  prompt for that, say so as a finding: the constraint is stated and unenforced.
- **An agent that can dispatch is an agent that can reach anything a dispatchable
  agent reaches.** The set a run can invoke transitively matters as much as the set
  it holds directly, and it is the one more likely to have been forgotten.
- **A sub-run must be able to do less than its caller, never more.** If an agent
  given only read tools has a delegate that holds a writing server, the read-only
  promise is false and the finding is the configuration, not the individual call.

The `permissions` question generalises: for each agent, what is the smallest thing
that would let it do its job, and what does this one have beyond that.

## 3. Instructions that are not enforced

A role prompt saying "do not merge" is a statement of intent. Anything that
genuinely must not happen needs a mechanism outside the prompt:

- a tool the run does not have;
- a policy the tool itself applies, and refuses on;
- a path or branch that a merge requires a different identity for.

Where a project's answer to a risk is a sentence in a prompt, the finding is that
sentence being the whole answer. It is a real finding, severity depending on what the
instruction guards.

## 4. Where a shell is reachable from text

A shell is what turns an injected instruction into an action on the machine, and a
closed loop of "read a thing, run what it says" is the shape to look for:

- a script that fetches and executes;
- a workflow that reads a value from an untrusted source and evaluates it;
- an agent whose job is to follow instructions found in what it reads — a setup
  guide, a config file, a repository's own documentation — with a shell in hand.

Whether the guard in front of the shell helps is a separate finding, and the honest
answer is usually that a text-matching guard is not a boundary: give the agent
nothing to match against, or accept that a determined instruction gets through and
set the reach accordingly.

## 5. What a session carries forward

A session persists its conversation, and that conversation contains whatever the run
read. Two consequences:

- **Injected text keeps working after the turn it arrived in**, and after a
  compaction — the summary is written by the same model reading the same text.
- **Text written into a place the next run reads** — an issue comment, a file in the
  repository, a session store — reaches a run that never saw its origin. A finding
  here is a persistent injection, and it is more severe than a one-turn one.

## What to record while reading

For the agents in the area:

- the tools each holds, and which of them can write, reach the network, or change
  the repository;
- what each can invoke that it does not hold directly;
- every place outside text enters, and what the run does next;
- every constraint stated in a prompt, and whether anything else enforces it.

That list is the evidence, and it belongs in the report's scope section.
