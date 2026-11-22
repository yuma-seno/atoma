---
name: engineering/secrets-in-a-check
description: Load when a check you were asked for needs a real credential to mean anything — why that check does not belong in the pull request, what to do instead, and how to ask a person before a secret is spent.
---

# When a check needs a real credential

The question is usually one line: does this endpoint accept our token, is this
scope enough, can this script read the thing it is meant to read. Every version of
it has the same shape — **the answer exists only at the far end of a credential**,
and this run does not have one.

## Why you cannot do it here

Three walls, and the first two are what you will hit:

- **The shell holds no repository secret.** Every tool server receives only the
  credentials its own configuration declares; `github` and `atomaton` hold a token
  and `shell` does not. So `curl`, `gh` and `printenv` have nothing to send, and
  `curl` and `gh` are refused outright for the routing reason
  (`engineering/environment` covers what to do about a refusal).
- **A pull request may not decide its own execution environment, and nothing that
  runs for one holds a secret.** That is not a rule you can work around: the step
  that runs a pull request's declared commands has *nowhere to write a credential*,
  which is a property of the workflow rather than today's configuration.
- **`fetch` is text only.** It can POST a form-encoded body and nothing else —
  no binary payload and no `Authorization` header.

Do not look for a way past these. Asking for a tool that would make the check
possible earlier is asking for **a credential in the hands of code nobody has read
yet**, which is the thing all three exist to prevent.

## What to do instead

**Write the tool, and let the tool's own call be the check.** Almost every check
like this is the first use of something that has to exist anyway — a server that
publishes a file, a client that reaches a new endpoint. Build it, and make it able
to **report what the far end said** rather than only succeeding or failing.

That is why `defaults.yaml`'s entries carry `env:`, and why a server that holds a
token is the right shape for this: **the credential stays in the server and never
reaches a shell.** The check then runs where the tool runs, after a person has
merged it.

**Verify the shape here, and the far end there.** Before the merge you can check
the request, the parameters and how the response is read, against a stub. Say in
the pull request what a person should run once it is merged, and what answer would
mean the design is wrong.

## Ask before a secret is spent

A call against a real endpoint is not free and not always reversible — it can
create something, spend a quota, or leave a record.

- **Say what you want to send, to where, and what it would create.** "One 2KB
  `.txt` to `uploads.github.com/user-attachments/assets` for `yuma-seno/atomaton`"
  is the level to aim for.
- **Prefer a request that cannot store anything.** Many endpoints answer almost
  everything from a deliberately incomplete one, so nothing is created and there
  is nothing to clean up afterwards. What it does not give you is the route's
  existence. `404` is what an unknown path answers, what a wrong host answers, and
  what a proxy in front of a real route answers when it will not forward the
  request; a validation error proves that something read your parameters, not
  that the thing reading them is the one you are asking about.
  **A successful call is not an answer** here any more than a `2xx` is one for
  `fetch` — the response has to name the thing you asked about, in the endpoint's
  own words, or you have learned that something replied and nothing more. When it
  does not, quote the status and the body exactly as they arrived and put the
  question to a person; do not write the route down as proven.
- **If something IS created, say how it is removed** before you create it, and do
  it in the same run.
- **A person's yes is for one call**, not a standing licence to use the credential
  whenever it helps. If a second call is needed, ask again and say what changed.

## What not to conclude

A refusal is not evidence that your check is unnecessary. It is evidence that the
check belongs somewhere else — and the reason it belongs there is worth writing
down, because the next reader will wonder the same thing:

- Put the decision about **where** the check runs in the issue or the pull request,
  not only in your report. "This cannot run here because the shell holds no
  credential" is a fact the next run should not have to rediscover one refusal at
  a time.
- If the check showed that a premise in the issue is wrong, say so. A design built
  on "we can verify this first" is a design that changes when it turns out nobody
  can.
