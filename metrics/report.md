# Agent metrics

Read from the sessions stored on this branch. Nothing here is recorded specially: every number is something the agents already wrote down while working.

Generated 2026-11-23.

A session appears in a dated window only if it recorded when its runs ended. Sessions from before run recording existed are counted under All time alone, so the dated windows are thinner than the project was — that gap closes as new sessions arrive, not by anything changing here.

## Runs

| window | runs | gave up | median seconds | longest | median round trips | median seconds each |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Last 7 days | 2 | 0% | 325 | 325 | 54 | 6.0 |
| Last 30 days | 2 | 0% | 325 | 325 | 54 | 6.0 |
| Last year | 2 | 0% | 325 | 325 | 54 | 6.0 |
| All time | 2 | 0% | 325 | 325 | 54 | 6.0 |

**Gave up** is every ending that is not `completed` — a ceiling reached, a person asking, a provider hanging up, a loop cut short. Each one is a mechanism deciding the run should not continue, which is worth watching whether or not it was right.

What is left over is not the same as work delivered. `completed` is the core saying its own loop ended rather than being stopped — it says nothing about whether the run produced a report. **Ran to an end without a report**, in each window below, is that question asked separately.

| ended because | runs |
| --- | ---: |
| `completed` | 2 |

## Last 7 days

1 session.

**Ran to an end without a report:** 0 of 1 session whose last run the core recorded as `completed` — 0%. These are not runs that gave up: nothing stopped them, they simply ended without writing a closing line, so the work is in a saved session and nowhere a person or the next agent reads.

| | p50 | p90 | p99 | max | total |
| --- | ---: | ---: | ---: | ---: | ---: |
| messages per session | 154 | 154 | 154 | 154 | 154 |

| agent | sessions | share |
| --- | ---: | ---: |
| `engineer` | 1 | 100% |

**Refused** is the machinery saying no — a denylist, an allowlist, a hook. A guard working is not a tool breaking, and a reader cannot act on the two the same way, so they are counted apart. **Failed** is everything else that came back as an error, by string match, so it is an estimate.

| tool | calls | failed | refused | failure rate |
| --- | ---: | ---: | ---: | ---: |
| `bash` | 56 | 1 | 3 | 1.8% |
| `read` | 14 | 0 | 0 | 0% |
| `edit` | 4 | 0 | 0 | 0% |
| `get_pr` | 2 | 0 | 0 | 0% |
| `load_skill` | 2 | 0 | 0 | 0% |
| `write` | 2 | 0 | 0 | 0% |
| `commit_and_push` | 1 | 0 | 0 | 0% |
| `create_pr` | 1 | 0 | 0 | 0% |
| `get_check_runs` | 1 | 0 | 0 | 0% |
| `grep` | 1 | 0 | 0 | 0% |
| `list_issues` | 1 | 0 | 0 | 0% |
| `list_prs` | 1 | 0 | 0 | 0% |
| `request_close_issue` | 1 | 0 | 0 | 0% |

What the agents do when they reach for a shell. `search` without a matching `open` is the shape that produced this project's most expensive runs; `edit` against `verify` is the shape that turned out not to occur at all.

| act | calls | share |
| --- | ---: | ---: |
| `other` | 54 | 96.4% |
| `edit` | 2 | 3.6% |

| skill | loads | share |
| --- | ---: | ---: |
| `delivery/pipeline-setup` | 1 | 50% |
| `writing/japanese` | 1 | 50% |

## Last 30 days

1 session.

**Ran to an end without a report:** 0 of 1 session whose last run the core recorded as `completed` — 0%. These are not runs that gave up: nothing stopped them, they simply ended without writing a closing line, so the work is in a saved session and nowhere a person or the next agent reads.

| | p50 | p90 | p99 | max | total |
| --- | ---: | ---: | ---: | ---: | ---: |
| messages per session | 154 | 154 | 154 | 154 | 154 |

| agent | sessions | share |
| --- | ---: | ---: |
| `engineer` | 1 | 100% |

**Refused** is the machinery saying no — a denylist, an allowlist, a hook. A guard working is not a tool breaking, and a reader cannot act on the two the same way, so they are counted apart. **Failed** is everything else that came back as an error, by string match, so it is an estimate.

| tool | calls | failed | refused | failure rate |
| --- | ---: | ---: | ---: | ---: |
| `bash` | 56 | 1 | 3 | 1.8% |
| `read` | 14 | 0 | 0 | 0% |
| `edit` | 4 | 0 | 0 | 0% |
| `get_pr` | 2 | 0 | 0 | 0% |
| `load_skill` | 2 | 0 | 0 | 0% |
| `write` | 2 | 0 | 0 | 0% |
| `commit_and_push` | 1 | 0 | 0 | 0% |
| `create_pr` | 1 | 0 | 0 | 0% |
| `get_check_runs` | 1 | 0 | 0 | 0% |
| `grep` | 1 | 0 | 0 | 0% |
| `list_issues` | 1 | 0 | 0 | 0% |
| `list_prs` | 1 | 0 | 0 | 0% |
| `request_close_issue` | 1 | 0 | 0 | 0% |

What the agents do when they reach for a shell. `search` without a matching `open` is the shape that produced this project's most expensive runs; `edit` against `verify` is the shape that turned out not to occur at all.

| act | calls | share |
| --- | ---: | ---: |
| `other` | 54 | 96.4% |
| `edit` | 2 | 3.6% |

| skill | loads | share |
| --- | ---: | ---: |
| `delivery/pipeline-setup` | 1 | 50% |
| `writing/japanese` | 1 | 50% |

## Last year

1 session.

**Ran to an end without a report:** 0 of 1 session whose last run the core recorded as `completed` — 0%. These are not runs that gave up: nothing stopped them, they simply ended without writing a closing line, so the work is in a saved session and nowhere a person or the next agent reads.

| | p50 | p90 | p99 | max | total |
| --- | ---: | ---: | ---: | ---: | ---: |
| messages per session | 154 | 154 | 154 | 154 | 154 |

| agent | sessions | share |
| --- | ---: | ---: |
| `engineer` | 1 | 100% |

**Refused** is the machinery saying no — a denylist, an allowlist, a hook. A guard working is not a tool breaking, and a reader cannot act on the two the same way, so they are counted apart. **Failed** is everything else that came back as an error, by string match, so it is an estimate.

| tool | calls | failed | refused | failure rate |
| --- | ---: | ---: | ---: | ---: |
| `bash` | 56 | 1 | 3 | 1.8% |
| `read` | 14 | 0 | 0 | 0% |
| `edit` | 4 | 0 | 0 | 0% |
| `get_pr` | 2 | 0 | 0 | 0% |
| `load_skill` | 2 | 0 | 0 | 0% |
| `write` | 2 | 0 | 0 | 0% |
| `commit_and_push` | 1 | 0 | 0 | 0% |
| `create_pr` | 1 | 0 | 0 | 0% |
| `get_check_runs` | 1 | 0 | 0 | 0% |
| `grep` | 1 | 0 | 0 | 0% |
| `list_issues` | 1 | 0 | 0 | 0% |
| `list_prs` | 1 | 0 | 0 | 0% |
| `request_close_issue` | 1 | 0 | 0 | 0% |

What the agents do when they reach for a shell. `search` without a matching `open` is the shape that produced this project's most expensive runs; `edit` against `verify` is the shape that turned out not to occur at all.

| act | calls | share |
| --- | ---: | ---: |
| `other` | 54 | 96.4% |
| `edit` | 2 | 3.6% |

| skill | loads | share |
| --- | ---: | ---: |
| `delivery/pipeline-setup` | 1 | 50% |
| `writing/japanese` | 1 | 50% |

## All time

1 session.

**Ran to an end without a report:** 0 of 1 session whose last run the core recorded as `completed` — 0%. These are not runs that gave up: nothing stopped them, they simply ended without writing a closing line, so the work is in a saved session and nowhere a person or the next agent reads.

| | p50 | p90 | p99 | max | total |
| --- | ---: | ---: | ---: | ---: | ---: |
| messages per session | 154 | 154 | 154 | 154 | 154 |

| agent | sessions | share |
| --- | ---: | ---: |
| `engineer` | 1 | 100% |

**Refused** is the machinery saying no — a denylist, an allowlist, a hook. A guard working is not a tool breaking, and a reader cannot act on the two the same way, so they are counted apart. **Failed** is everything else that came back as an error, by string match, so it is an estimate.

| tool | calls | failed | refused | failure rate |
| --- | ---: | ---: | ---: | ---: |
| `bash` | 56 | 1 | 3 | 1.8% |
| `read` | 14 | 0 | 0 | 0% |
| `edit` | 4 | 0 | 0 | 0% |
| `get_pr` | 2 | 0 | 0 | 0% |
| `load_skill` | 2 | 0 | 0 | 0% |
| `write` | 2 | 0 | 0 | 0% |
| `commit_and_push` | 1 | 0 | 0 | 0% |
| `create_pr` | 1 | 0 | 0 | 0% |
| `get_check_runs` | 1 | 0 | 0 | 0% |
| `grep` | 1 | 0 | 0 | 0% |
| `list_issues` | 1 | 0 | 0 | 0% |
| `list_prs` | 1 | 0 | 0 | 0% |
| `request_close_issue` | 1 | 0 | 0 | 0% |

What the agents do when they reach for a shell. `search` without a matching `open` is the shape that produced this project's most expensive runs; `edit` against `verify` is the shape that turned out not to occur at all.

| act | calls | share |
| --- | ---: | ---: |
| `other` | 54 | 96.4% |
| `edit` | 2 | 3.6% |

| skill | loads | share |
| --- | ---: | ---: |
| `delivery/pipeline-setup` | 1 | 50% |
| `writing/japanese` | 1 | 50% |

## Degraded answers

A tool can answer and report that it answered badly — a search that came back unranked, a log that went nowhere. The call succeeded, so it is neither a failure nor a refusal, and it is easy for one of these to run for months with nobody reading it. **Read `last seen` before `reports`**: an old count is a fixed fault. That date is when the session file was last written, which is the run that touched it last and not necessarily the run that reported the problem — so it errs recent.

Nothing reported a problem alongside an answer.

## Never used

Over all time, because something used once a year is still used. Each of these sits in the prompt of every run and returns nothing.

Every declared server has been called at least once.

Skills never loaded:

- `docs/customer-doc`
- `engineering/environment`
- `engineering/secrets-in-a-check`
- `research/web-search`
- `security/agent-prompt-surface`
- `security/gh-actions-surface`
- `security/on-demand-audit`
