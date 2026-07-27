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

If iteration count exceeds `max_iterations`, Atoma returns an error and the CLI exits with status 2.

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
