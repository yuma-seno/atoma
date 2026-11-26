# Agent metrics

Read from the sessions stored on this branch. Nothing here is recorded specially: every number is something the agents already wrote down while working.

Generated 2026-11-26.

A session appears in a dated window only if it recorded when its runs ended. Sessions from before run recording existed are counted under All time alone, so the dated windows are thinner than the project was — that gap closes as new sessions arrive, not by anything changing here.

## Runs

| window | runs | gave up | median seconds | longest | median round trips | median seconds each |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Last 7 days | 21 | 23.8% | 93 | 665 | 7 | 6.0 |
| Last 30 days | 21 | 23.8% | 93 | 665 | 7 | 6.0 |
| Last year | 21 | 23.8% | 93 | 665 | 7 | 6.0 |
| All time | 21 | 23.8% | 93 | 665 | 7 | 6.0 |

**Gave up** is every ending that is not `completed` — a ceiling reached, a person asking, a provider hanging up, a loop cut short. Each one is a mechanism deciding the run should not continue, which is worth watching whether or not it was right.

What is left over is not the same as work delivered. `completed` is the core saying its own loop ended rather than being stopped — it says nothing about whether the run produced a report. **Ran to an end without a report**, in each window below, is that question asked separately.

| ended because | runs |
| --- | ---: |
| `completed` | 16 |
| `failed` | 5 |

## Last 7 days

8 sessions.

**Ran to an end without a report:** 3 of 8 sessions whose last run the core recorded as `completed` — 37.5%. These are not runs that gave up: nothing stopped them, they simply ended without writing a closing line, so the work is in a saved session and nowhere a person or the next agent reads.

**7,893,358 tokens** over 8 runs that reported them, **98.6% of it prompt** — what the agents were made to read, not what they wrote. Anything spent on making runs cheaper belongs on that side. No money here, deliberately: of the four providers only one reports a cost, and a price table goes quietly stale and then prints confident wrong numbers.

**90.4% of that prompt was served from cache**, over the runs whose provider reported it. A cached prompt token costs a fraction of a fresh one, so this is most of what separates the counts above from the bill — and it is the figure that moves when what gets resent changes.

| | p50 | p90 | p99 | max | total |
| --- | ---: | ---: | ---: | ---: | ---: |
| tokens per run | 1,009,482 | 2,080,636 | 2,080,636 | 2,080,636 | 7,893,358 |
| messages per session | 154 | 247 | 247 | 247 | 1,130 |

| agent | sessions | share |
| --- | ---: | ---: |
| `engineer` | 4 | 50% |
| `reviewer` | 3 | 37.5% |
| `atomaton` | 1 | 12.5% |

**Refused** is the machinery saying no — a denylist, an allowlist, a hook. A guard working is not a tool breaking, and a reader cannot act on the two the same way, so they are counted apart. **Failed** is everything else that came back as an error, by string match, so it is an estimate.

| tool | calls | failed | refused | failure rate |
| --- | ---: | ---: | ---: | ---: |
| `bash` | 317 | 4 | 4 | 1.3% |
| `read` | 101 | 0 | 0 | 0% |
| `edit` | 50 | 1 | 0 | 2% |
| `grep` | 42 | 0 | 0 | 0% |
| `get_pr` | 8 | 0 | 0 | 0% |
| `search_code` | 8 | 0 | 0 | 0% |
| `commit_and_push` | 6 | 0 | 0 | 0% |
| `create_pr` | 5 | 1 | 0 | 20% |
| `delegate_readonly` | 5 | 0 | 0 | 0% |
| `get_check_runs` | 5 | 0 | 0 | 0% |
| `load_skill` | 5 | 0 | 0 | 0% |
| `get_pr_diff` | 4 | 0 | 0 | 0% |
| `request_close_issue` | 4 | 0 | 0 | 0% |
| `write` | 4 | 0 | 0 | 0% |
| `check_merge_readiness` | 3 | 0 | 0 | 0% |
| `create_root_issue` | 3 | 0 | 0 | 0% |
| `fetch` | 3 | 0 | 0 | 0% |
| `get_pr_reviews` | 3 | 0 | 0 | 0% |
| `list` | 3 | 0 | 0 | 0% |
| `delegate_readonly_large` | 2 | 0 | 0 | 0% |
| `get_branch` | 2 | 0 | 0 | 0% |
| `get_issue_comments` | 2 | 0 | 0 | 0% |
| `glob` | 2 | 0 | 0 | 0% |
| `list_pr_review_comments` | 2 | 0 | 0 | 0% |
| `list_prs` | 2 | 0 | 0 | 0% |
| `get_issue` | 1 | 0 | 0 | 0% |
| `list_issues` | 1 | 0 | 0 | 0% |
| `merge_pr` | 1 | 0 | 0 | 0% |

What the agents do when they reach for a shell. `search` without a matching `open` is the shape that produced this project's most expensive runs; `edit` against `verify` is the shape that turned out not to occur at all.

| act | calls | share |
| --- | ---: | ---: |
| `other` | 187 | 59% |
| `open` | 64 | 20.2% |
| `search` | 49 | 15.5% |
| `edit` | 16 | 5% |
| `verify` | 1 | 0.3% |

| skill | loads | share |
| --- | ---: | ---: |
| `writing/japanese` | 2 | 40% |
| `delivery/pipeline-setup` | 1 | 20% |
| `environment` | 1 | 20% |
| `japanese` | 1 | 20% |

## Last 30 days

8 sessions.

**Ran to an end without a report:** 3 of 8 sessions whose last run the core recorded as `completed` — 37.5%. These are not runs that gave up: nothing stopped them, they simply ended without writing a closing line, so the work is in a saved session and nowhere a person or the next agent reads.

**7,893,358 tokens** over 8 runs that reported them, **98.6% of it prompt** — what the agents were made to read, not what they wrote. Anything spent on making runs cheaper belongs on that side. No money here, deliberately: of the four providers only one reports a cost, and a price table goes quietly stale and then prints confident wrong numbers.

**90.4% of that prompt was served from cache**, over the runs whose provider reported it. A cached prompt token costs a fraction of a fresh one, so this is most of what separates the counts above from the bill — and it is the figure that moves when what gets resent changes.

| | p50 | p90 | p99 | max | total |
| --- | ---: | ---: | ---: | ---: | ---: |
| tokens per run | 1,009,482 | 2,080,636 | 2,080,636 | 2,080,636 | 7,893,358 |
| messages per session | 154 | 247 | 247 | 247 | 1,130 |

| agent | sessions | share |
| --- | ---: | ---: |
| `engineer` | 4 | 50% |
| `reviewer` | 3 | 37.5% |
| `atomaton` | 1 | 12.5% |

**Refused** is the machinery saying no — a denylist, an allowlist, a hook. A guard working is not a tool breaking, and a reader cannot act on the two the same way, so they are counted apart. **Failed** is everything else that came back as an error, by string match, so it is an estimate.

| tool | calls | failed | refused | failure rate |
| --- | ---: | ---: | ---: | ---: |
| `bash` | 317 | 4 | 4 | 1.3% |
| `read` | 101 | 0 | 0 | 0% |
| `edit` | 50 | 1 | 0 | 2% |
| `grep` | 42 | 0 | 0 | 0% |
| `get_pr` | 8 | 0 | 0 | 0% |
| `search_code` | 8 | 0 | 0 | 0% |
| `commit_and_push` | 6 | 0 | 0 | 0% |
| `create_pr` | 5 | 1 | 0 | 20% |
| `delegate_readonly` | 5 | 0 | 0 | 0% |
| `get_check_runs` | 5 | 0 | 0 | 0% |
| `load_skill` | 5 | 0 | 0 | 0% |
| `get_pr_diff` | 4 | 0 | 0 | 0% |
| `request_close_issue` | 4 | 0 | 0 | 0% |
| `write` | 4 | 0 | 0 | 0% |
| `check_merge_readiness` | 3 | 0 | 0 | 0% |
| `create_root_issue` | 3 | 0 | 0 | 0% |
| `fetch` | 3 | 0 | 0 | 0% |
| `get_pr_reviews` | 3 | 0 | 0 | 0% |
| `list` | 3 | 0 | 0 | 0% |
| `delegate_readonly_large` | 2 | 0 | 0 | 0% |
| `get_branch` | 2 | 0 | 0 | 0% |
| `get_issue_comments` | 2 | 0 | 0 | 0% |
| `glob` | 2 | 0 | 0 | 0% |
| `list_pr_review_comments` | 2 | 0 | 0 | 0% |
| `list_prs` | 2 | 0 | 0 | 0% |
| `get_issue` | 1 | 0 | 0 | 0% |
| `list_issues` | 1 | 0 | 0 | 0% |
| `merge_pr` | 1 | 0 | 0 | 0% |

What the agents do when they reach for a shell. `search` without a matching `open` is the shape that produced this project's most expensive runs; `edit` against `verify` is the shape that turned out not to occur at all.

| act | calls | share |
| --- | ---: | ---: |
| `other` | 187 | 59% |
| `open` | 64 | 20.2% |
| `search` | 49 | 15.5% |
| `edit` | 16 | 5% |
| `verify` | 1 | 0.3% |

| skill | loads | share |
| --- | ---: | ---: |
| `writing/japanese` | 2 | 40% |
| `delivery/pipeline-setup` | 1 | 20% |
| `environment` | 1 | 20% |
| `japanese` | 1 | 20% |

## Last year

8 sessions.

**Ran to an end without a report:** 3 of 8 sessions whose last run the core recorded as `completed` — 37.5%. These are not runs that gave up: nothing stopped them, they simply ended without writing a closing line, so the work is in a saved session and nowhere a person or the next agent reads.

**7,893,358 tokens** over 8 runs that reported them, **98.6% of it prompt** — what the agents were made to read, not what they wrote. Anything spent on making runs cheaper belongs on that side. No money here, deliberately: of the four providers only one reports a cost, and a price table goes quietly stale and then prints confident wrong numbers.

**90.4% of that prompt was served from cache**, over the runs whose provider reported it. A cached prompt token costs a fraction of a fresh one, so this is most of what separates the counts above from the bill — and it is the figure that moves when what gets resent changes.

| | p50 | p90 | p99 | max | total |
| --- | ---: | ---: | ---: | ---: | ---: |
| tokens per run | 1,009,482 | 2,080,636 | 2,080,636 | 2,080,636 | 7,893,358 |
| messages per session | 154 | 247 | 247 | 247 | 1,130 |

| agent | sessions | share |
| --- | ---: | ---: |
| `engineer` | 4 | 50% |
| `reviewer` | 3 | 37.5% |
| `atomaton` | 1 | 12.5% |

**Refused** is the machinery saying no — a denylist, an allowlist, a hook. A guard working is not a tool breaking, and a reader cannot act on the two the same way, so they are counted apart. **Failed** is everything else that came back as an error, by string match, so it is an estimate.

| tool | calls | failed | refused | failure rate |
| --- | ---: | ---: | ---: | ---: |
| `bash` | 317 | 4 | 4 | 1.3% |
| `read` | 101 | 0 | 0 | 0% |
| `edit` | 50 | 1 | 0 | 2% |
| `grep` | 42 | 0 | 0 | 0% |
| `get_pr` | 8 | 0 | 0 | 0% |
| `search_code` | 8 | 0 | 0 | 0% |
| `commit_and_push` | 6 | 0 | 0 | 0% |
| `create_pr` | 5 | 1 | 0 | 20% |
| `delegate_readonly` | 5 | 0 | 0 | 0% |
| `get_check_runs` | 5 | 0 | 0 | 0% |
| `load_skill` | 5 | 0 | 0 | 0% |
| `get_pr_diff` | 4 | 0 | 0 | 0% |
| `request_close_issue` | 4 | 0 | 0 | 0% |
| `write` | 4 | 0 | 0 | 0% |
| `check_merge_readiness` | 3 | 0 | 0 | 0% |
| `create_root_issue` | 3 | 0 | 0 | 0% |
| `fetch` | 3 | 0 | 0 | 0% |
| `get_pr_reviews` | 3 | 0 | 0 | 0% |
| `list` | 3 | 0 | 0 | 0% |
| `delegate_readonly_large` | 2 | 0 | 0 | 0% |
| `get_branch` | 2 | 0 | 0 | 0% |
| `get_issue_comments` | 2 | 0 | 0 | 0% |
| `glob` | 2 | 0 | 0 | 0% |
| `list_pr_review_comments` | 2 | 0 | 0 | 0% |
| `list_prs` | 2 | 0 | 0 | 0% |
| `get_issue` | 1 | 0 | 0 | 0% |
| `list_issues` | 1 | 0 | 0 | 0% |
| `merge_pr` | 1 | 0 | 0 | 0% |

What the agents do when they reach for a shell. `search` without a matching `open` is the shape that produced this project's most expensive runs; `edit` against `verify` is the shape that turned out not to occur at all.

| act | calls | share |
| --- | ---: | ---: |
| `other` | 187 | 59% |
| `open` | 64 | 20.2% |
| `search` | 49 | 15.5% |
| `edit` | 16 | 5% |
| `verify` | 1 | 0.3% |

| skill | loads | share |
| --- | ---: | ---: |
| `writing/japanese` | 2 | 40% |
| `delivery/pipeline-setup` | 1 | 20% |
| `environment` | 1 | 20% |
| `japanese` | 1 | 20% |

## All time

8 sessions.

**Ran to an end without a report:** 3 of 8 sessions whose last run the core recorded as `completed` — 37.5%. These are not runs that gave up: nothing stopped them, they simply ended without writing a closing line, so the work is in a saved session and nowhere a person or the next agent reads.

**7,893,358 tokens** over 8 runs that reported them, **98.6% of it prompt** — what the agents were made to read, not what they wrote. Anything spent on making runs cheaper belongs on that side. No money here, deliberately: of the four providers only one reports a cost, and a price table goes quietly stale and then prints confident wrong numbers.

**90.4% of that prompt was served from cache**, over the runs whose provider reported it. A cached prompt token costs a fraction of a fresh one, so this is most of what separates the counts above from the bill — and it is the figure that moves when what gets resent changes.

| | p50 | p90 | p99 | max | total |
| --- | ---: | ---: | ---: | ---: | ---: |
| tokens per run | 1,009,482 | 2,080,636 | 2,080,636 | 2,080,636 | 7,893,358 |
| messages per session | 154 | 247 | 247 | 247 | 1,130 |

| agent | sessions | share |
| --- | ---: | ---: |
| `engineer` | 4 | 50% |
| `reviewer` | 3 | 37.5% |
| `atomaton` | 1 | 12.5% |

**Refused** is the machinery saying no — a denylist, an allowlist, a hook. A guard working is not a tool breaking, and a reader cannot act on the two the same way, so they are counted apart. **Failed** is everything else that came back as an error, by string match, so it is an estimate.

| tool | calls | failed | refused | failure rate |
| --- | ---: | ---: | ---: | ---: |
| `bash` | 317 | 4 | 4 | 1.3% |
| `read` | 101 | 0 | 0 | 0% |
| `edit` | 50 | 1 | 0 | 2% |
| `grep` | 42 | 0 | 0 | 0% |
| `get_pr` | 8 | 0 | 0 | 0% |
| `search_code` | 8 | 0 | 0 | 0% |
| `commit_and_push` | 6 | 0 | 0 | 0% |
| `create_pr` | 5 | 1 | 0 | 20% |
| `delegate_readonly` | 5 | 0 | 0 | 0% |
| `get_check_runs` | 5 | 0 | 0 | 0% |
| `load_skill` | 5 | 0 | 0 | 0% |
| `get_pr_diff` | 4 | 0 | 0 | 0% |
| `request_close_issue` | 4 | 0 | 0 | 0% |
| `write` | 4 | 0 | 0 | 0% |
| `check_merge_readiness` | 3 | 0 | 0 | 0% |
| `create_root_issue` | 3 | 0 | 0 | 0% |
| `fetch` | 3 | 0 | 0 | 0% |
| `get_pr_reviews` | 3 | 0 | 0 | 0% |
| `list` | 3 | 0 | 0 | 0% |
| `delegate_readonly_large` | 2 | 0 | 0 | 0% |
| `get_branch` | 2 | 0 | 0 | 0% |
| `get_issue_comments` | 2 | 0 | 0 | 0% |
| `glob` | 2 | 0 | 0 | 0% |
| `list_pr_review_comments` | 2 | 0 | 0 | 0% |
| `list_prs` | 2 | 0 | 0 | 0% |
| `get_issue` | 1 | 0 | 0 | 0% |
| `list_issues` | 1 | 0 | 0 | 0% |
| `merge_pr` | 1 | 0 | 0 | 0% |

What the agents do when they reach for a shell. `search` without a matching `open` is the shape that produced this project's most expensive runs; `edit` against `verify` is the shape that turned out not to occur at all.

| act | calls | share |
| --- | ---: | ---: |
| `other` | 187 | 59% |
| `open` | 64 | 20.2% |
| `search` | 49 | 15.5% |
| `edit` | 16 | 5% |
| `verify` | 1 | 0.3% |

| skill | loads | share |
| --- | ---: | ---: |
| `writing/japanese` | 2 | 40% |
| `delivery/pipeline-setup` | 1 | 20% |
| `environment` | 1 | 20% |
| `japanese` | 1 | 20% |

## Degraded answers

A tool can answer and report that it answered badly — a search that came back unranked, a log that went nowhere. The call succeeded, so it is neither a failure nor a refusal, and it is easy for one of these to run for months with nobody reading it. **Read `last seen` before `reports`**: an old count is a fixed fault. That date is when the session file was last written, which is the run that touched it last and not necessarily the run that reported the problem — so it errs recent.

Nothing reported a problem alongside an answer.

## Never used

Over all time, because something used once a year is still used. Each of these sits in the prompt of every run and returns nothing.

Every declared server has been called at least once.

Skills never loaded:

- `agent-prompt-surface`
- `customer-doc`
- `gh-actions-surface`
- `on-demand-audit`
- `pipeline-setup`
- `secrets-in-a-check`
- `web-search`
