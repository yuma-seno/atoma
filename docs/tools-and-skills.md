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

- `tool_allowlist` and `tool_denylist` are mutually exclusive. Setting both is invalid.
- Pattern matching supports exact match or trailing `*` wildcard.
- `before_tool` is fail-closed:
  - non-zero exit, timeout, or invalid JSON blocks the call.
  - expected output JSON includes `allow: true`.
- `after_tool` is best effort:
  - failures are logged and do not fail the tool call.

Related env timeouts:

- `ATOMA_HOOK_TIMEOUT` (default 30s)
- `ATOMA_MCP_TIMEOUT` (default 60s)
- `ATOMA_MCP_INIT_TIMEOUT` (default 120s)

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
