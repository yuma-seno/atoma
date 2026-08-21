# Tools and Skills

Atoma runtime exposes two tool categories:

- External MCP tools loaded from `tools.yaml` and discovered from running servers.
- One built-in Atoma tool: `atoma_builtin__load_skill`.

The built-in skill loader is always present and is not configured in `tools.yaml`.

## External MCP tools

`tools.yaml` maps server names to process definitions.

Minimal shape:

```yaml
filesystem:
  command: npx
  args: ["-y", "@modelcontextprotocol/server-filesystem", "."]
  env: {}
  hooks:
    tool_allowlist: ["filesystem__read_file"]
    before_tool: ./scripts/before_hook
    after_tool: ./scripts/after_hook
```

At runtime:

- Atoma spawns each configured server.
- Calls MCP `tools/list` and registers each tool as `server__tool`.
- Converts MCP `inputSchema` to OpenAI-compatible `function.parameters`.

### How long a server has to answer

`request_timeout_secs` sets the limit for one `tools/list` or `tools/call` on that
server. Omit it and the client's default applies (60s, or `ATOMA_MCP_TIMEOUT`).

```yaml
shell:
  command: bun
  args: ["run", "./scripts/shell.ts"]
  request_timeout_secs: 3600
```

Set it when the server's work genuinely takes minutes — a shell server running a
build or a test suite, or one that loads a model before it can answer. **A tool's
own timeout argument does not raise this limit.** A `shell_execute` that accepts
`timeout_seconds: 600` still gets cut off at 60 unless the server says so here,
and the error names the tool rather than the limit that caused it.

Leave it alone otherwise. This is the only thing that notices a server which has
stopped responding, so a large value is a long wait before a stuck run says so.
Setting it on a server that answers in milliseconds trades away the detection and
buys nothing.

`0` means the default, the same as omitting it.

## Prefixes and reserved namespaces

- External tool names are always prefixed by server name: `server__name`.
- Prefix `atoma_builtin__` is reserved.
- If any external tool starts with `atoma_builtin__`, runtime fails during startup.

## Hooks and failure behavior

Hook order:

1. denylist/allowlist check
2. `before_tool` hook
3. MCP tool call
4. `after_tool` hook

Hook rules:

- `tool_allowlist` and `tool_denylist` may both be set. The denylist is checked first,
  so a tool matching both is blocked. Setting both logs a warning naming that order,
  because it is unusual rather than wrong — it used to be refused as "ambiguous" while the
  code, this document and a test all described the precedence.
- Pattern matching supports exact match or trailing `*` wildcard.
- `before_tool` is fail-closed:
  - non-zero exit, timeout, or invalid JSON blocks the call.
  - expected output JSON includes `allow: true`.
- `after_tool` is best effort:
  - failures are logged and do not fail the tool call.

Related env timeouts:

- `ATOMA_HOOK_TIMEOUT` (default 30s)
- `ATOMA_MCP_TIMEOUT` (default 60s) — the default for every server; a server's own
  `request_timeout_secs` wins over it
- `ATOMA_MCP_INIT_TIMEOUT` (default 120s)

Every one of these treats `0`, blank, and unparseable as "use the default".

## When a call times out

The call fails and the agent sees an error naming the tool. The server is not
told: it keeps working and eventually writes its answer, which arrives after
nobody is waiting for it.

Atoma reads past that answer and discards it, matching the JSON-RPC `id` to the
request in flight. It logs a `warn` naming both ids when it does, which is the
signal that a timeout fired on that server earlier in the run.

This matters because the alternative is silent: without the id check, the next
call on that server reads the abandoned answer, and every answer from then on
belongs to the previous question for the rest of the run. Nothing about the shape
of the result gives it away.

## Skill catalog format

Skills are loaded recursively from `--skills-dir`.

Each `.md` file must include:

- YAML frontmatter with non-empty `name` and `description`
- non-empty instruction body

Example:

```markdown
---
name: engineering/tdd
description: Test first.
---

Write a failing test before implementation.
```

Loader safety checks:

- symbolic links are rejected
- duplicate `name` values are rejected
- malformed frontmatter or empty instructions fail startup

## Progressive disclosure of skills

At prompt build time, Atoma reveals only skill metadata in `AVAILABLE_SKILLS`.

When an agent needs details, it calls:

- `atoma_builtin__load_skill` with `{"name":"..."}`

Tool result returns full skill instructions as normal tool output, which is persisted in session history like any other tool call.

## Built-in loader is not configurable via `tools.yaml`

`tools.yaml` can only define external MCP servers.

You cannot:

- remove `atoma_builtin__load_skill`
- rename it
- add custom parameters to it

Those behaviors are implemented directly in runtime code.
