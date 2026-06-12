import importlib.util
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("build_context_session.py")
SPEC = importlib.util.spec_from_file_location("build_context_session", MODULE_PATH)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MODULE)


class BuildContextSessionTests(unittest.TestCase):
    def test_filters_only_current_agent_comments(self):
        session = {
            "messages": [
                {
                    "role": "assistant",
                    "content": "done",
                    "atoma_metadata": {
                        "github_comment_id": 101,
                        "agent": "orchestrator",
                    },
                }
            ]
        }
        events = [
            {
                "id": 101,
                "event_type": "issue_comment",
                "content": "<!-- atoma:agent=orchestrator -->\n/orchestrator handled",
                "author": "github-actions[bot]",
                "created_at": "2026-05-27T10:00:00Z",
            },
            {
                "id": 102,
                "event_type": "issue_comment",
                "content": "<!-- atoma:agent=engineer -->\n/engineer please implement",
                "author": "github-actions[bot]",
                "created_at": "2026-05-27T10:01:00Z",
            },
            {
                "id": 103,
                "event_type": "issue_comment",
                "content": "<!-- atoma:sub-result:#7 -->\n/orchestrator sub-task #7 completed.",
                "author": "github-actions[bot]",
                "created_at": "2026-05-27T10:02:00Z",
            },
        ]

        context_session, changed_count, _, _ = MODULE.build_context_session(
            session,
            events,
            "orchestrator",
        )

        kept_ids = [msg["atoma_metadata"]["id"] for msg in context_session["messages"]]
        self.assertEqual(kept_ids, [102, 103])
        self.assertGreater(changed_count, 0)

    def test_reuses_snapshot_hash_to_skip_unchanged_context(self):
        events = [
            {
                "id": "issue-1",
                "event_type": "issue_opened",
                "content": "Issue #1: test",
                "author": "alice",
                "created_at": "2026-05-27T09:00:00Z",
            }
        ]

        initial_session = {"messages": []}
        _, _, snapshot_hash, _ = MODULE.build_context_session(initial_session, events, "engineer")

        next_session = {
            "messages": [],
            "metadata": {
                "github_context": {
                    "snapshot_hash": snapshot_hash,
                }
            },
        }
        context_session, changed_count, next_hash, event_count = MODULE.build_context_session(
            next_session,
            events,
            "engineer",
        )

        self.assertEqual(changed_count, 0)
        self.assertEqual(next_hash, snapshot_hash)
        self.assertEqual(event_count, 1)
        self.assertEqual(context_session["metadata"]["snapshot_hash"], snapshot_hash)

    def test_human_comment_with_agent_marker_is_not_filtered(self):
        session = {"messages": []}
        events = [
            {
                "id": 301,
                "event_type": "issue_comment",
                "content": "<!-- atoma:agent=orchestrator -->\nComment copied by a human",
                "author": "alice",
                "created_at": "2026-05-27T12:00:00Z",
            }
        ]

        context_session, _, _, event_count = MODULE.build_context_session(
            session,
            events,
            "orchestrator",
        )

        self.assertEqual(event_count, 1)
        self.assertEqual(context_session["messages"][0]["atoma_metadata"]["id"], 301)

    def test_applies_agent_shared_context_policy(self):
        session = {"messages": []}
        events = [
            {
                "id": "pr-1",
                "event_type": "pr_opened",
                "content": "PR body",
                "author": "alice",
                "created_at": "2026-05-27T12:00:00Z",
            },
            {
                "id": "pr-1-diff",
                "event_type": "pr_diff",
                "content": "diff",
                "author": "github",
                "created_at": "2026-05-27T12:01:00Z",
            },
            {
                "id": 401,
                "event_type": "pr_review",
                "content": "needs work",
                "author": "bob",
                "created_at": "2026-05-27T12:02:00Z",
            },
        ]
        config = {
            "agents": {
                "test-writer": {
                    "shared_context": {
                        "include_event_types": ["pr_opened", "pr_review"],
                        "exclude_event_types": ["pr_diff"],
                    }
                }
            }
        }

        context_session, _, _, event_count = MODULE.build_context_session(
            session,
            events,
            "test-writer",
            config,
        )

        kept_types = [msg["atoma_metadata"]["event_type"] for msg in context_session["messages"]]
        self.assertEqual(event_count, 2)
        self.assertEqual(kept_types, ["pr_opened", "pr_review"])


if __name__ == "__main__":
    unittest.main()