# Skills

Skills are reusable instruction documents that agents load on demand. Atoma exposes skill metadata in the system prompt and keeps the full instructions out of context until the agent calls the built-in `atoma_builtin__load_skill` tool.

The built-in tool is always available. It is not declared in an agent definition or tools file, cannot be disabled by MCP hooks, and uses a reserved tool namespace. Skill calls and results are stored in the ordinary session message history.

## Configure a Catalog

Pass a skill directory on the command line:

```bash
atoma run --agent-def ./agent.md --skills-dir ./skills
```

Or configure it in `atoma.toml`:

```toml
[defaults]
skills_dir = "skills"

[profile.review]
skills_dir = "review-skills"
```

Priority is CLI argument, profile, then defaults. Skills are available to every agent in the run; agent frontmatter does not select or restrict them.

## Skill Format

Each `.md` file under the configured directory must contain YAML frontmatter and a non-empty Markdown body:

```markdown
---
name: engineering/tdd
description: Apply a focused red-green-refactor cycle to behavioral changes.
---

# Procedure

1. Identify the narrowest observable behavior.
2. Add a failing test.
3. Implement the smallest correction.
4. Run focused validation.
5. Refactor only after the test passes.
```

Names must be unique across the catalog. Empty names, descriptions, or instruction bodies fail at startup. Symbolic links are rejected so catalog traversal cannot escape the configured root.

## Prompt Template

Use `{{AVAILABLE_SKILLS}}` to expose the catalog's names and descriptions:

```markdown
# Available Skills

{{AVAILABLE_SKILLS}}
```

Atoma does not insert full skill instructions into the system prompt. The agent calls:

```json
{
  "name": "atoma_builtin__load_skill",
  "arguments": {
    "name": "engineering/tdd"
  }
}
```

The resulting tool message contains the full instructions and remains in the persisted session history. Loading is deterministic within a run because Atoma validates and snapshots the complete catalog before inference starts.
