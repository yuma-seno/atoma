#!/usr/bin/env python3
"""
build_context_session.py — Build a temporary context-session.json from GitHub events.

Philosophy: GitHub conversation is shared context rebuilt on each run.
The cached session.json remains agent-local state: assistant replies, tool calls,
and per-agent working memory. This keeps orchestration comments visible across
agents while preserving each agent's own tool-call history.

Algorithm:
  1. Load the cached per-agent session (optional) to inspect:
       - assistant github_comment_id values posted by this agent
       - the previously processed shared-context snapshot hash
  2. Filter fetched GitHub events:
       - keep issue/PR bodies, diffs, human comments, and other-agent comments
       - exclude this agent's own result comments
  3. Convert the filtered events into context-session.json user messages.
  4. Compute a snapshot hash for change detection.
  5. Write new_event_count/context_snapshot_hash/context_event_count to GITHUB_OUTPUT.

Usage:
  build_context_session.py \
    --events events.json \
    --agent-name orchestrator \
    [--session session.json] \
    --out context-session.json
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sys
from pathlib import Path

_AGENT_MARKER_RE = re.compile(r"^<!--\s*atoma:agent=([a-z][a-z0-9-]*)\s*-->$")
_GITHUB_CONTEXT_LAYER = "github-context"


def load_session(path: str | None) -> dict:
    if path and Path(path).exists():
        with open(path, encoding="utf-8") as f:
            return json.load(f)
    return {"messages": []}


def load_events(path: str) -> list[dict]:
    with open(path, encoding="utf-8") as f:
        raw = json.load(f)
    if not isinstance(raw, list):
        raise ValueError(f"events.json must be a JSON array, got {type(raw).__name__}")

    valid: list[dict] = []
    for i, event in enumerate(raw):
        if not isinstance(event, dict):
            print(f"Warning: event[{i}] is not an object — skipping: {event!r}", file=sys.stderr)
            continue
        if "id" not in event:
            print(f"Warning: event[{i}] has no 'id' field — skipping: {event!r}", file=sys.stderr)
            continue
        if "content" not in event:
            print(
                f"Warning: event[{i}] (id={event['id']}) has no 'content' field — skipping",
                file=sys.stderr,
            )
            continue
        valid.append(event)
    return valid


def load_orchestration_config(path: str | None) -> dict:
    if not path:
        return {}
    config_path = Path(path)
    if not config_path.exists():
        return {}
    with open(config_path, encoding="utf-8") as f:
        return json.load(f)


def normalize_id(val) -> str | None:
    if val is None:
        return None
    return str(val)


def build_own_comment_ids(session: dict, agent_name: str) -> set[str]:
    own_ids: set[str] = set()
    for msg in session.get("messages", []):
        meta = msg.get("atoma_metadata") or {}
        if msg.get("role") != "assistant" or "github_comment_id" not in meta:
            continue
        comment_agent = meta.get("agent")
        if comment_agent not in (None, agent_name):
            continue
        own_ids.add(normalize_id(meta["github_comment_id"]))
    return own_ids


def extract_result_comment_agent(event: dict) -> str | None:
    author = event.get("author", "")
    if not isinstance(author, str) or not author.endswith("[bot]"):
        return None

    content = event.get("content")
    if not isinstance(content, str) or not content:
        return None
    first_line = content.splitlines()[0].strip()
    match = _AGENT_MARKER_RE.match(first_line)
    if match:
        return match.group(1)
    return None


def previous_snapshot_hash(session: dict) -> str | None:
    metadata = session.get("metadata")
    if not isinstance(metadata, dict):
        return None
    github_context = metadata.get("github_context")
    if not isinstance(github_context, dict):
        return None
    value = github_context.get("snapshot_hash")
    return value if isinstance(value, str) else None


def is_self_event(event: dict, agent_name: str, own_comment_ids: set[str]) -> bool:
    event_id = normalize_id(event.get("id"))
    if event_id in own_comment_ids:
        return True
    return extract_result_comment_agent(event) == agent_name


def context_policy(config: dict, agent_name: str) -> tuple[set[str] | None, set[str]]:
    agents = config.get("agents")
    if not isinstance(agents, dict):
        return None, set()
    agent = agents.get(agent_name)
    if not isinstance(agent, dict):
        return None, set()
    shared_context = agent.get("shared_context")
    if not isinstance(shared_context, dict):
        return None, set()

    include = shared_context.get("include_event_types")
    exclude = shared_context.get("exclude_event_types")
    include_set = set(include) if isinstance(include, list) else None
    exclude_set = set(exclude) if isinstance(exclude, list) else set()
    return include_set, exclude_set


def filter_events_for_agent(
    events: list[dict],
    agent_name: str,
    own_comment_ids: set[str],
    config: dict,
) -> list[dict]:
    include_event_types, exclude_event_types = context_policy(config, agent_name)
    filtered: list[dict] = []
    for event in events:
        if is_self_event(event, agent_name, own_comment_ids):
            print(
                f"  Skipping current agent comment from shared context: id={event['id']}",
                file=sys.stderr,
            )
            continue

        event_type = event.get("event_type")
        if include_event_types is not None and event_type not in include_event_types:
            continue
        if event_type in exclude_event_types:
            continue

        filtered.append(event)
    return filtered


def event_to_user_message(event: dict) -> dict:
    meta: dict = {
        "source": "github",
        "layer": _GITHUB_CONTEXT_LAYER,
        "event_type": event["event_type"],
        "id": event["id"],
        "author": event.get("author", "unknown"),
        "created_at": event.get("created_at", ""),
    }
    if "sha" in event:
        meta["sha"] = event["sha"]

    return {
        "role": "user",
        "content": event["content"],
        "atoma_metadata": meta,
    }


def snapshot_hash_for_events(events: list[dict]) -> str:
    payload = json.dumps(events, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(payload.encode("utf-8")).hexdigest()


def build_context_session(
    session: dict,
    events: list[dict],
    agent_name: str,
    config: dict | None = None,
) -> tuple[dict, int, str, int]:
    own_comment_ids = build_own_comment_ids(session, agent_name)
    filtered_events = filter_events_for_agent(events, agent_name, own_comment_ids, config or {})
    current_hash = snapshot_hash_for_events(filtered_events)
    previous_hash = previous_snapshot_hash(session)

    if previous_hash == current_hash:
        changed_count = 0
    elif previous_hash is None:
        changed_count = len(filtered_events)
    else:
        changed_count = 1

    context_session = {
        "messages": [event_to_user_message(event) for event in filtered_events],
        "metadata": {
            "source": _GITHUB_CONTEXT_LAYER,
            "agent": agent_name,
            "snapshot_hash": current_hash,
            "event_count": len(filtered_events),
        },
    }
    return context_session, changed_count, current_hash, len(filtered_events)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Build a shared GitHub context-session.json for one Atoma agent"
    )
    parser.add_argument("--events", required=True, help="Path to events.json")
    parser.add_argument("--agent-name", required=True, help="Current agent name")
    parser.add_argument("--config", default=None, help="Path to orchestration config JSON")
    parser.add_argument("--session", default=None, help="Path to existing per-agent session.json")
    parser.add_argument("--out", required=True, help="Output path for context-session.json")
    args = parser.parse_args()

    try:
        session = load_session(args.session)
        events = load_events(args.events)
        config = load_orchestration_config(args.config)
        context_session, changed_count, snapshot_hash, event_count = build_context_session(
            session,
            events,
            args.agent_name,
            config,
        )

        with open(args.out, "w", encoding="utf-8") as f:
            json.dump(context_session, f, indent=2, ensure_ascii=False)
            f.write("\n")

        github_output = os.environ.get("GITHUB_OUTPUT", "")
        if github_output:
            with open(github_output, "a", encoding="utf-8") as f:
                f.write(f"new_event_count={changed_count}\n")
                f.write(f"context_snapshot_hash={snapshot_hash}\n")
                f.write(f"context_event_count={event_count}\n")

        print(
            f"Context build complete: {len(events)} events fetched, {event_count} shared messages, "
            f"changed={changed_count}",
            file=sys.stderr,
        )
        return 0

    except json.JSONDecodeError as e:
        print(f"ERROR: Failed to parse JSON: {e}", file=sys.stderr)
        return 1
    except OSError as e:
        print(f"ERROR: File I/O error: {e}", file=sys.stderr)
        return 1
    except Exception as e:  # noqa: BLE001
        print(f"ERROR: Unexpected error: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())