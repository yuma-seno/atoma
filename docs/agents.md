# Agents

An agent definition is a Markdown file with YAML frontmatter and an optional body.

## File format

```markdown
---
# YAML frontmatter
---

# Optional markdown body
```

Parser requirements:

- File must start with `---`.
- Frontmatter must close with a second `---` delimiter.
- YAML must parse into the current `AgentDef` contract.

## Frontmatter fields

Required:

- `name: string`
- `description: string`
- `model: string`

Optional:

- `provider: openai | github-copilot | anthropic`
- `knows_about: string[]`
- `callable_by: string[]`
- `mcp_servers: string[]`
- `extra_body: object`

`callable_by` semantics:

- `atoma validate` accepts only `user` and `agent`.
- Runtime does not enforce caller identity; this is a contract-level check.

## Body and role prompt

- If body exists, it is injected as `AGENT_ROLE_PROMPT` in the system template.
- If body is empty, Atoma falls back to `description` for `AGENT_ROLE_PROMPT`.

## Colleagues and validation semantics

`knows_about` entries are treated as colleague names and resolved to `<name>.md` in the same directory as the current agent file.

Validation behavior:

- Missing colleague file is an error.
- Unparseable colleague file is an error.
- Colleague file must include `agent` in `callable_by`, or validation fails.

## Prompt template variables

When Atoma builds the system prompt, these variables are available:

- `{{AGENT_NAME}}`
- `{{AGENT_ROLE_PROMPT}}`
- `{{WORKING_DIRECTORY}}`
- `{{COLLEAGUES_LIST}}`
- `{{AVAILABLE_TOOLS}}`
- `{{AVAILABLE_SKILLS}}`

`AVAILABLE_TOOLS` is derived from runtime tool definitions.

`AVAILABLE_SKILLS` contains skill metadata only (name and description), not full instructions.

## Complete valid example

```markdown
---
name: reviewer
description: Review pull requests for correctness and test gaps.
model: openai/gpt-5
provider: openai
callable_by:
  - user
  - agent
knows_about:
  - engineer
mcp_servers:
  - filesystem
  - github
extra_body:
  temperature: 0
---

Prioritize behavioral regressions and missing tests.
When you claim an issue, include a concrete file reference and a minimal reproduction path.
```
