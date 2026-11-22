---
name: security/on-demand-audit
description: Load when asked to run a security audit of a repository, or when dispatching one — the whole procedure both the agent that requests it and the run that executes it follow. Scope, areas, lenses, scanner and advisory sources, the refutation pass, the findings format, the previous round's comparison, and what may be written on a public thread.
---

# Auditing a repository on request

One procedure, two readers. The agent that receives a request for an audit reads it
to split the work and to reconcile what comes back; the run that audits an area
reads it to find what is there. Reading it twice is the point — a dispatcher who has
not read the format cannot judge a report against it, and a sub-run that invents its
own format has reported to nobody.

## What this is, and what it is not

This is a **whole-codebase pass, run when a person asks for one**. Not a gate on a
change. A pipeline's deterministic checks own the every-change question — a secret
scan, a dependency audit — and they are better at it than reading: they are exact,
free, and cannot be argued out of a finding. So do not rebuild them here.

What is left, and what this procedure is for, is the class a diff cannot show:

- an authorization check missing on one of eleven sibling routes, where the diff
  that added the eleventh looks exactly like the ten before it;
- a boundary that is wrong by design — a component that may read the credential, a
  server that may write the tree — which is a property of the shape and not of any
  single change;
- data that crosses two components between a place that trusts it and a place that
  acts on it.

These are found by reading a whole area at once, which is expensive, which is why it
happens on request rather than on every change.

## 1. Establish the scope

Read the request, and the issue it was made on, before opening a file.

- **The request may name an area.** Audit that and stop there.
- **It may name a concern** — "check the authentication", "look at our deployment".
  Turn the concern into the paths and boundaries it is actually about, and say in
  the report which paths you took to be in scope.
- **It may name nothing.** Then establish the shape yourself, from what the
  repository answers: the manifests and lockfiles (which stack, which language), the
  entry points (how a request or an event gets in), the configuration, the
  deployment and CI definitions, and any place a credential is routed. That list is
  the scope you then propose.

Nothing audited means nothing reported, so the scope is the first thing the report
states — a reader has to be able to tell "there is nothing here" from "they did not
look here".

## 2. Split into areas

An audit is split along **boundaries**, never along file counts or folder names:

- **Trust boundaries.** Where data or a request moves from less trusted to more
  trusted: an HTTP handler into a service, a workflow from a fork into a job that
  holds a token, a webhook into the process that acts on it, a sub-agent's output
  into a parent's instructions.
- **Input and output boundaries.** Where something enters from outside the program
  (a body, a file, a URL, a dependency, an environment variable) and where something
  leaves it (a shell, a query, a template, a page, a log, a commit).
- **Ownership boundaries.** Which component is allowed to do a thing: which server
  holds which credential, which agent may write, which path only a person may merge.
  This is the boundary that a scanner never sees and that reading a whole area does.

One area per run, and a cross-boundary flow is the **last** area and covers the
whole path — the areas before it each established one end of the flow, and the run
that has both ends in front of it is the one that can see the join.

A small audit is one area — and it is still one sub-issue. The split exists so that a
large codebase can be read deeply, not so that a small one can be reported in
fragments; but the sub-issue is not the split, it is the return path. See step 11.

## 3. Lenses

Six ways to look at an area, applied to what is there. They are not a checklist to
produce a finding for each — most areas answer most lenses with nothing, and saying
so is part of the report.

| Lens | What to look for | What evidence looks like |
| --- | --- | --- |
| **Injection through a workflow** | A value from outside the repository expanded into a script that runs; a trigger that hands a fork's code a secret or a write token; a dependency, action or image that is not pinned; a job whose token may do more than the job needs | the workflow line, and where the value came from |
| **Injection through an agent's instructions** | Untrusted text (an issue body, a fetched page, a tool result, a file) reaching the instructions or the tool set of something that can act; an agent with more reach than its job needs; a sub-run that can write on behalf of one that cannot | the start of the data flow and its end, both |
| **Authorization and gates** | A route or an operation with no check; a check that a second path into the same operation skips; a gate whose condition can be met by the change it is meant to stop | the alternative path, described — a gate is only as strong as the ways around it |
| **Secrets** | A credential in source; a credential routed to a place that does not need it; a secret written to a log, an error, a commit or a comment; a command in a manifest that could read one | the whole path from where the secret is declared to where it may be read, as a list |
| **Supply chain, structurally** | A lockfile that disagrees with its manifest; a dependency taken from an unpinned source; an install-time script; a name that is one character from a popular package | the two files that disagree, or the declaration and its origin. **Advisory matching is not this lens** — it is step 4 |
| **Logic and ordering** | An assumption about order that a second caller breaks; a state that assumes an event will arrive; a check and the thing it protects separated by a window where the state changes | the concrete sequence in which the assumption fails |

## 4. Deterministic scanners, run not rebuilt

Before reading, find out what the ecosystem already has and **run it**. It is exact,
it is current, and it is cheaper than any amount of reading:

1. **Detect the stack** from what is checked in — the manifest and lockfile
   (`package.json`, `bun.lock`/`package-lock.json`, `Cargo.toml`, `go.mod`,
   `requirements.txt`/`poetry.lock`, `Gemfile.lock`, `pom.xml`), and the CI
   definitions, which usually say the command.
2. **Run that ecosystem's audit** — `bun audit`, `npm audit --json`, `cargo audit`,
   `govulncheck ./...`, `pip-audit`, `bundle audit`, `osv-scanner` where it is
   present. Several tools answering the same question is fine; a JSON form is worth
   preferring, because the identifier (`GHSA-…`, `CVE-…`) is what you verify in
   step 5.
3. **Record the command and its exit code** in the report. A scanner that found
   nothing and a scanner that did not run read identically otherwise.
4. **If the tool is not installed, do not install it.** Say which check could not
   run, and continue with the lenses. Changing the environment is not this role's
   work, and an audit that spent its run on an install audited nothing.

A secret scan works the same way: if the project already declares one, read what it
produced. Do not add one — that is a change, and this role makes none.

## 5. Where a claim about the outside world comes from

Four tiers, and a finding may only be stated as confirmed from the first two:

| Tier | Source | What it may be used for |
| --- | --- | --- |
| 0 | What you already know | **A hypothesis, and nothing else.** "Version X is affected by that bug" is the start of a check, never a finding |
| 1 | A scanner run here | Confirmed: the identifier, the package, the installed version, the fixed version |
| 2 | A primary advisory, fetched | Confirmed: the affected range, the conditions, the fix, and whether the project's use is reachable |
| 3 | A general search | Discovery only — finding the name or the advisory to fetch in step 2 |

Fetch the advisory itself for anything a finding rests on: the vulnerability's own
page (`https://osv.dev/vulnerability/<ID>`) or the advisory database it was
published in. A summary is written to be read at a glance, not to be relied on, and
a severity copied out of a search result is how a wrong finding is born.

The `research/web-search` skill covers how to fetch and how to search when the
address is not known.

## 6. Refute every candidate before it is reported

**This step is not optional and it is where most of the value is.** A whole-codebase
audit produces many candidates and few findings, and the ratio is the difference
between a report somebody acts on and one they learn to skim.

For each candidate, in writing, for yourself:

1. **Is the input reachable from a source the code does not control?** Trace it from
   the entry point. A value that only ever comes from the file it is in is not
   attacker-controlled, whatever it is called.
2. **Is there a guard upstream?** A middleware, an allowlist, a framework default, a
   type that cannot hold the bad value. Reading the one file where the sink sits is
   the commonest way to report a weakness the framework already prevents.
3. **Is it live?** Test fixtures, examples, demos, code behind a flag that is off,
   and functions nothing calls are not findings in a whole-codebase audit — they are
   the reason a whole-codebase audit needs this step.
4. **Does the impact survive the preconditions?** Name them. If one is unproven, the
   candidate is not a finding; either it becomes an unconfirmed note with the
   precondition stated, or it is dropped.
5. **Would the maintainer already know?** Sometimes the answer is a deliberate
   trade-off with the reason written beside it. If the comment says why, the finding
   is that the reason no longer holds — otherwise it is a report of a decision.

When the candidate list is long, hand the list and the paths it points at to
`delegate_readonly`: a sub-run that reads the named places and says which candidates
the code refutes comes back as one paragraph instead of the files themselves in this
session. What it refutes, drop. What it does not, still read yourself before it goes
in the report — the delegate's summary is an argument, not the evidence.

## 7. What is not a finding

Vulgarity of caution is not a finding. Each of these is dropped, not downgraded:

- **A test fixture, a deliberate demo, or an example.** The credential-shaped string
  in a test is usually the test's subject.
- **Code on no path.** Unused, behind a disabled flag, in a directory nothing
  imports.
- **A generic recommendation with no sink named.** "Validate input", "add rate
  limiting", "use HTTPS" — without the specific place and the specific consequence
  they are advice, and advice is not audit output.
- **Reachability you could not establish, stated as if you had.** This one becomes
  an unconfirmed note with what is missing, in its own section, never mixed into the
  findings.

## 8. The report

Every area reports in this shape, and each finding carries all eight parts. A
finding missing one of them is not finished — the location and the impact are what a
person decides from, and the evidence is what makes the location checkable.

```text
<Finding title — one sentence, the weakness in the maintainer's own terms>

Class:          what kind of weakness (missing authorization, injection at …, secret
                in source, unpinned dependency, …)
Location:       path/to/file.ext:LINE — the line the defect is on, and the file a
                reader opens to check it
Evidence:       what you read that shows it: the flow (source → … → sink) with the
                line of each step, or the two files that disagree
Impact:         what an attacker gains, and the preconditions that must hold
Severity:       high | medium | low
Confidence:     high | medium | low — and when it differs from severity, one clause
                saying why (e.g. "high impact, but the route is behind a feature flag",
                "certain it is unpinned, unproven that the installed version is affected")
Fix:            the smallest correction, in one or two sentences
Source:         the advisory URL a dependency finding rests on, or `code` for a
                design or flow finding
```

`Severity` and `Confidence` are separate on purpose. A report that folds them
together forces the reader to guess which one you meant, and the two lead to
different actions: high severity at low confidence is a question to settle before
anyone changes code; low severity at high confidence is a fix that waits its turn.

Above the findings, three short sections:

- **Scope** — what was audited, and what was deliberately not.
- **Checked and clean** — the lenses that answered nothing, and the scanners run with
  their exit codes. This is what lets a later round be compared with this one.
- **Unconfirmed** — the candidates the refutation pass could not settle, each with
  the missing piece named.

## 9. What may be written on the thread

Findings are reported in issues and comments, and **a public repository's issues are
public**. A finding written in full detail there is a weakness published before it is
fixed.

- **Default: class, location, evidence, impact, severity, confidence.** Enough for a
  maintainer to act and to judge; not a working payload.
- **Do not write**: a complete reproduction, a request that triggers it, a payload,
  or the exploit steps. Say what the weakness is and where, and offer the detail
  rather than posting it.
- **A live credential is never repeated anywhere.** Name the file and the line, say
  to rotate it, and do not quote the value — including in a code block, including
  truncated.
- **A private repository is already restricted to its members**, so the full detail
  can be written there. Decide from what the repository answers about itself; when
  that is unclear, write the minimal form. Too little detail costs one exchange; too
  much cannot be taken back.
- **The place for full detail on a public repository** is a draft security advisory,
  which the maintainers can open privately and publish when they choose. No tool in
  this toolset creates one, so it is a person's step: name it in the report as where
  the detail went or should go, and do not attempt it. Writing the advisory's
  contents into an issue instead would be the publication the draft exists to avoid.

## 10. Comparing against the previous round

A second audit is worth more than the first only if it says what changed, and that
needs the previous one to be findable:

- **Every finding's issue title begins `security-finding:`.** That prefix is the
  contract; it is what makes a list possible without any label a project has to
  remember to apply. Where a project also labels them, filtering by the label is
  cheaper, so use it when it is there.
- **A later round lists those issues, open and closed**, and decides one of three for
  each: **still present** (with the same location, or the new one), **fixed** (with
  the evidence — the guard now at `path:line`), or **no longer applies** (the code
  went away, or the reason it was reported no longer holds).
- **An open finding is carried, not re-reported.** The round says it is still there
  and moves on. Re-arguing a known finding on a second round is how the interesting
  part of a report gets buried.
- **A finding accepted as a risk stays open with its reason written on it.** That is
  where "we know, and we are not fixing it" lives — the audit does not argue with it,
  and it does not report it as new either.

## 11. Filing, and who does it

**One finding is one issue, and the agent that dispatched the audit files it.** Not
the area's run, and for two reasons: the same weakness is often visible from two
areas, and two runs filing it twice produces two issues that then both need closing;
and the comparison in step 10 needs one list, which only exists if one place writes
it.

So an area's run reports its findings, and the dispatcher — after every area is back
— files the issues, one per finding, titled with the prefix above, and writes the
reconciled report on the issue the audit was requested on. The reconciled report
repeats each finding once, names which area found it, and states the comparison
result for the previous round.

**Every area therefore goes out as a sub-issue, including the only one.** That is the
part that is easy to get wrong, because a small audit looks like it should not need
the ceremony. It is not ceremony: the dispatcher is a run, and it is re-invoked when
one of its children closes — a child closing, or a pull request merging, and nothing
else. A handoff directly to the agent that audits ends the dispatcher's run and starts
nothing on its return, so a round dispatched that way produces a report on a thread
nobody is reading and no `security-finding:` issues at all. The next round then
compares against nothing.

So: one area, one child, one line in the dispatch. The cost of the extra issue is one
GitHub object; the cost of the alternative is that the filing step never happens.

The alternative — the auditor filing the root issues itself when nobody dispatched it
— would put the list in two places and gives up the reconciliation that deduplicates
across areas. It is worse, and it is written down here only so a later round does not
reinvent it.

## 12. Running a round, from the dispatching side

1. **Read** the request, and establish the scope as in step 1.
2. **Cut the areas** as in step 2, and give each one its own sub-issue — one area
   included, for the re-invocation reason in step 11. Its body names the area (paths
   and boundaries), what the round is for, and where the previous round's findings
   are — the title prefix to search, not a list repeated by hand.
3. **Launch the ones that are independent.** Areas that share a boundary are worth
   launching together; the cross-boundary flow goes last, and it is the one that has
   to wait. When there is one area this step is one `launch_sub_agent` with one task.
4. **Reconcile** what comes back as in steps 10 and 11, and report.
5. **Hand the fixes over, do not make them.** What a person decides to fix goes to
   the agent the project uses for implementation — with the finding, its location,
   and the fix it proposed. The audit produced a decision; the change is a different
   run.
