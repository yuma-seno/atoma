# Runtime

This page explains how `atoma run` executes, stores context, and exits.

## Inference loop

Per iteration, Atoma:

1. sends current session messages to the selected provider
2. reads the first choice
3. if `tool_calls` are present, executes them sequentially
4. appends assistant/tool messages to session
5. repeats until a final completion condition is reached

Completion handling:

- `stop` or `end_turn`: successful completion
- `length`: returns truncated text with completion reason `length`
- `content_filter`: run fails
- unknown finish reason: run fails

## Contentless completions

A completion carrying neither text nor tool calls is treated as a provider-side
misfire, not as a decision by the model. There is nothing in it to append to the
session, so Atoma re-sends the same request.

This is bounded: after 2 consecutive contentless completions the run fails. The
bound applies to both shapes — an empty `tool_calls` array, and `stop`/`end_turn`
with empty text. Any productive completion resets the counter.

For transport-level failures and the request timeout, see
[configuration.md](configuration.md).

## Where a run stops

A run has no ceiling unless the caller asks for one. It ends when the agent says it
is finished, when a tool asks for the session to end, or when the loop is detected to
be broken -- the same tool call failing identically three times in a row.

No default ceiling, because the only one available to a default is a count of turns,
and a count of turns is a proxy for "this run has stopped getting anywhere" that is
wrong in both directions. Measured in one repository: a task finished in 17 tool
calls, and the same task framed thirteen times larger was cut off at 200 turns having
made 169 distinct searches and repeated only 6 -- working, and stopped for it.

Three opt-in stops exist for callers who need one:

| Flag | Meaning |
| --- | --- |
| `--max-runtime-secs N` | Stop after N seconds of wall clock. |
| `--max-iterations N` | Stop after N turns. |
| `--stop-file FILE` | Stop once FILE exists. |

The two ceilings can also be set as `max_runtime_secs` or `max_iterations` under
`[defaults]` or a profile in `atoma.toml`, though a runtime limit usually belongs on
the command line: it describes the circumstances of one invocation, not the agent.
`--stop-file` has no configuration key at all — a path fixed in a config file is a
path that may already exist when a run starts, which would stop every run on its
first turn.

None of the three is a failure. Atoma checks all of them between exchanges, where the
conversation is whole and every tool call has its result, saves the session, returns
the corresponding error, and the CLI exits with status 2. The session is resumable
with `--in-session`.

All three share that status, so it cannot say which of them happened. Which one it was
is in `ended_because`, in the `--output json` envelope and in the session's `atoma_runs`
— see [Output modes and exit behavior](#output-modes-and-exit-behavior).

`--max-runtime-secs` is the one to reach for under a CI job with its own timeout. Set
it below that timeout: a run that is killed by the job never reaches the step that
saves the session, so its work is gone rather than resumable.

`--stop-file` is for a caller that has to be able to change its mind — a person
watching a run go the wrong way, an hour before any budget would notice. Nothing
polls it for you: the caller creates the file when it decides, from wherever it is.

A file rather than a signal, for the same reason the ceilings are checked where they
are. `SIGTERM` arrives in the middle of a request; the file is read at the top of a
turn. And what needs to reach a running agent usually comes from another machine,
where a signal cannot go and a shared path or a small poller can.

## Tool history and `session_ends`

Tool calls are persisted as ordinary conversation history:

- assistant message with `tool_calls`
- tool messages with `tool_call_id`

If any tool returns `_meta.session_ends: true`, runtime ends the loop cleanly and returns `SessionEnded`.

In this path:

- session can still be saved
- no assistant text response is printed
- process exits successfully
- with `--output json`, the envelope is printed like any other ending, with `response`
  and `finish_reason` as `null` and `ended_because` as `completed` — this is how a run
  that finished by opening a pull request ends, and it is a success

## Session semantics

`--in-session`:

- if file exists, load JSON session
- if not, start with empty session

`--out-session`:

- when set, save final session there
- when omitted but `--in-session` is set, save back to `--in-session`

System message behavior:

- Atoma rebuilds system prompt each run
- existing `system` messages are removed and replaced

Prompt source behavior:

- `--prompt-file` has priority
- otherwise stdin is read when piped
- otherwise run continues with existing session only

## Output modes and exit behavior

`--output text` (default):

- prints the final assistant text, and nothing else
- prints nothing at all when a tool ended the session, when a ceiling or a stop file
  ended the run, or when the run failed

`--output json` prints one envelope on stdout on **every** exit path: a completion, a
session a tool ended, each of the three deliberate stops, and a failure. The key set is
the same on all of them, so a caller does not have to know which happened before it can
read an answer.

| Field | Type | Meaning |
| --- | --- | --- |
| `response` | string \| null | Final assistant text. `null` on every ending that has none — a session a tool ended, a deliberate stop, a failure. |
| `finish_reason` | `stop` \| `length` \| null | Why the model stopped. `null` wherever `response` is. |
| `usage.prompt_tokens` | number | Summed over the inferences this run made. Partial on a run that did not finish, which is what that run actually spent. |
| `usage.completion_tokens` | number | As above. |
| `usage.total_tokens` | number | As above. |
| `usage.cached_prompt_tokens` | number \| null | How much of the prompt the provider served from its cache. `null` means no inference reported one, which is not the same as zero and must not be read as a cache that did nothing. |
| `usage.written_prompt_tokens` | number \| null | How much of the prompt the provider wrote into its cache. `null` on the same terms. |
| `session_path` | string \| null | Where the session was written, when `--in-session` or `--out-session` asked for one. |
| `ended_because` | string | Why the run ended: one of the five words below. |
| `seconds` | number | Wall clock for the whole run, including parsing the agent definition and starting every tool server. |
| `iterations` | number | Round trips to the model. Counted as each response arrives, so a contentless completion that was re-requested counts — it was waited on and it was billed. |

`ended_because` is the entire vocabulary:

| Word | Meaning | Exit |
| --- | --- | --- |
| `completed` | The agent returned text, or a tool ended the session with `_meta.session_ends`. Both are successes. | `0` |
| `iterations` | `--max-iterations` was reached. | `2` |
| `runtime` | `--max-runtime-secs` was reached. | `2` |
| `stopped` | The path given to `--stop-file` existed. | `2` |
| `failed` | Anything else: the provider, a tool, a loop cut short for repeating itself. | non-zero error |

Read `ended_because`, not the exit status. The three deliberate stops share `2` with each
other and with clap's own argument-parsing error, so the status cannot tell them apart
and no further statuses are being added: a number has too little room to say which
ceiling was hit, while the word says it exactly.

The same five words, with `seconds` and `iterations`, are also appended to the session
file as `atoma_runs` — one record per run, so a resumed session carries the history of
every run that touched it. Both are built from the same measurements. The difference is
that the envelope describes one run and is always printed, while `atoma_runs` is the
history and exists only when `--in-session` or `--out-session` asked for a file.

The two stderr log lines are unchanged and still carry the same numbers for callers that
grep: `ATOMA_TOKEN_USAGE` once per run, `ATOMA_INFERENCE_USAGE` once per round trip. They
are no longer the only place the cache counts exist.

## Machine-readable lines

Three lines are addressed to the environment rather than to a person. Each is an `ATOMA_`
name, a colon, then space-separated `key=value`:

```
ATOMA_TOKEN_USAGE: prompt=… completion=… total=… cached=… written=…
ATOMA_INFERENCE_USAGE: iteration=… prompt=… completion=… cached=… written=… request=…
ATOMA_CONFIG_FINDING: kind=dead_guard severity=warn server=files_ro pattern=files_ro__* tools=read,grep
ATOMA_LLM_ERROR: label=Anthropic code=free_rate_limited type=orcarouter_api_error
```

**The fields are the contract and the sentence beside them is not.** Parse `kind=`,
`code=`, `severity=` and the fields beside them; never the English. A caller that greps
the prose is a caller whose tooling atoma breaks by rewording a warning, which is why
these lines carry fields and no sentence at all — the paragraph that a person reads is a
separate, neighbouring log line.

`ATOMA_LLM_ERROR` is written when a provider delivers an error **object** — under HTTP
200, or as a chunk in the middle of a stream, which is where a rate limit hit halfway
through an answer arrives, since the status was already sent when the stream opened. Its
fields are:

| Field | Meaning |
| --- | --- |
| `label` | Which call failed: `Anthropic`, `OpenAI Responses`, `LLM`. The only field that is atoma's rather than the provider's. |
| `code` | The provider's `code` — an identifier, unlike the message, which is prose and is localized by at least one provider here. Absent when the provider sent none, including when it sent an empty one. |
| `type` | The provider's `type`. Absent on the same terms. |

There is deliberately no table of what a `code` means. The same number is terminal for one
vendor and transient for another, and the codes arrive as strings here and integers there;
collecting the values from real runs is what decides whether a table is worth having.
Nothing in atoma branches on these fields yet — they are logged so that they can be.

The same run also carries what `atoma` did about it. Every provider that says *when* to
come back gets obeyed: a `Retry-After` in seconds is honoured up to two minutes, and only
where the provider sent none does the run fall back to its own backoff. The retry line
says which of the two it used.

## Troubleshooting

| Symptom | Likely cause | Recovery action |
| --- | --- | --- |
| `--profile requires an atoma.toml` | Profile requested without config discovery | Run from a directory under your `atoma.toml`, or pass explicit CLI flags |
| `Agent has mcp_servers configured but --tools-file was not specified` | Agent requests MCP servers and no tools file was given | Pass `--tools-file` or remove `mcp_servers` from agent |
| `Tool 'X' not found in tools file` | Agent `mcp_servers` and tools YAML keys do not match | Align names exactly and re-run `atoma validate` |
| Hook script not found | Relative hook path cannot be resolved from tools file directory | Fix path in `tools.yaml` and ensure file exists |
| `Unknown skill` from `atoma_builtin__load_skill` | Skill name not present in loaded catalog | Use one of the names listed in `AVAILABLE_SKILLS` |
| `LLM returned empty response ... times in a row` | Provider kept returning contentless completions | Check provider/endpoint health; on OpenRouter, pin routing with `extra_body.provider` |
| `Failed to parse ... response` | Body did not match the expected shape (truncated bodies are retried automatically) | Verify the model is served in an OpenAI-compatible format at the configured base URL |
| Single request takes far longer than expected | Upstream endpoint stalled; each attempt waits `ATOMA_LLM_TIMEOUT` | Lower `ATOMA_LLM_TIMEOUT` to fail faster, and route away from the stalling endpoint |
