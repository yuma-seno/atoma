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

- prints final assistant text

`--output json`:

- prints JSON with `response`, `usage`, `finish_reason`, `session_path`

Exit behavior summary:

| Situation | Exit |
| --- | --- |
| Final response (`stop`/`end_turn`/`length`) | `0` |
| Tool requested `session_ends` | `0` |
| Max iterations reached | `2` |
| Other error (provider/tool/config/runtime) | non-zero error |

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
