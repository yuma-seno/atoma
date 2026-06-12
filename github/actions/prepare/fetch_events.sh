#!/usr/bin/env bash
# fetch_events.sh — Fetch all GitHub events for an Issue or PR as a JSON array.
#
# Outputs a JSON array sorted by created_at to events.json.
# Each event has: {id, event_type, content, author, created_at}
#
# Event types:
#   issue_opened           — Issue body
#   issue_comment          — Issue comment
#   pr_opened              — PR body
#   pr_diff                — PR current diff (stable ID: "pr-{number}-diff")
#   pr_comment             — PR conversation comment
#   pr_review_comment      — PR inline review comment
#   linked_issue_opened    — Linked Issue body (from PR)
#   linked_issue_comment   — Linked Issue comment (from PR)
#
# Environment:
#   TYPE           issue | pr                                (required)
#   NUMBER         Issue or PR number                       (required)
#   GH_TOKEN       GitHub token                             (required)
#   OWNER          repo owner (auto-detected if absent)
#   REPO           repo name (auto-detected if absent)
#   MAX_DIFF_CHARS max characters for PR diff (default: 30000)
#   OUT_FILE       output path (default: events.json)
set -euo pipefail

: "${TYPE:?TYPE is required (issue|pr)}"
: "${NUMBER:?NUMBER is required}"
: "${GH_TOKEN:?GH_TOKEN is required}"

OWNER="${OWNER:-$(gh repo view --json owner --jq '.owner.login')}"
REPO="${REPO:-$(gh repo view --json name --jq '.name')}"
MAX_DIFF_CHARS="${MAX_DIFF_CHARS:-30000}"
OUT_FILE="${OUT_FILE:-events.json}"

# ── Helpers ───────────────────────────────────────────────────────────────

fetch_issue_events() {
  local issue_num="$1"
  local opened_type="${2:-issue_opened}"   # "issue_opened" or "linked_issue_opened"
  local comment_type="${3:-issue_comment}" # "issue_comment" or "linked_issue_comment"
  local id_prefix="${4:-issue}"            # "issue" or "linked-issue"

  local issue
  issue=$(gh api "repos/${OWNER}/${REPO}/issues/${issue_num}")

  # Issue body event
  local opened_event
  opened_event=$(echo "$issue" | jq --arg opened_type "$opened_type" --arg id_prefix "$id_prefix" '{
    id: ($id_prefix + "-" + (.number | tostring)),
    event_type: $opened_type,
    content: ("Issue #\(.number): \(.title)\n" +
              (if (.labels | length) > 0 then "**Labels:** \([.labels[].name] | join(", "))\n" else "" end) +
              "\n" + (.body // "")),
    author: .user.login,
    created_at: .created_at
  }')

  # Issue comment events
  local comments
  comments=$(gh api "repos/${OWNER}/${REPO}/issues/${issue_num}/comments" \
    --paginate \
    | jq -s --arg comment_type "$comment_type" \
    'add // [] | [.[] | {
      id: .id,
      event_type: $comment_type,
      content: .body,
      author: .user.login,
      created_at: .created_at
    }]')

  echo "[$opened_event]" | jq -c --argjson comments "$comments" '. + $comments'
}

# ── Main ──────────────────────────────────────────────────────────────────

if [ "$TYPE" = "issue" ]; then
  # Fetch issue events, sort by created_at
  fetch_issue_events "$NUMBER" "issue_opened" "issue_comment" "issue" \
    | jq 'sort_by(.created_at)' > "$OUT_FILE"

  # Output resolved_number (same as NUMBER for issues)
  echo "resolved_number=${NUMBER}" >> "${GITHUB_OUTPUT:-/dev/null}"

elif [ "$TYPE" = "pr" ]; then
  # ── Fetch PR metadata ─────────────────────────────────────────────────
  PR=$(gh api "repos/${OWNER}/${REPO}/pulls/${NUMBER}")
  PR_BODY=$(echo "$PR"   | jq -r '.body // ""')
  PR_TITLE=$(echo "$PR"  | jq -r '.title')
  PR_AUTHOR=$(echo "$PR" | jq -r '.user.login')
  PR_CREATED=$(echo "$PR"| jq -r '.created_at')
  PR_UPDATED=$(echo "$PR"| jq -r '.updated_at')
  PR_LABELS=$(echo "$PR" | jq -r '[.labels[].name] | join(", ")')
  HEAD_SHA=$(echo "$PR"  | jq -r '.head.sha' | cut -c1-8)

  # Linked issue (written by create_pr tool as <!-- atoma-linked-issue: N -->)
  LINKED_ISSUE=$(echo "$PR_BODY" \
    | grep -oP '(?<=<!-- atoma-linked-issue: )\d+(?= -->)' 2>/dev/null || true)

  RESOLVED_NUMBER="${LINKED_ISSUE:-${NUMBER}}"
  echo "resolved_number=${RESOLVED_NUMBER}" >> "${GITHUB_OUTPUT:-/dev/null}"

  ALL_EVENTS="[]"

  # ── Linked Issue events (if any) ─────────────────────────────────────
  if [ -n "$LINKED_ISSUE" ]; then
    if LINKED_EVENTS=$(fetch_issue_events \
      "$LINKED_ISSUE" \
      "linked_issue_opened" \
      "linked_issue_comment" \
      "linked-issue" 2>/dev/null); then
      ALL_EVENTS=$(echo "$ALL_EVENTS" | jq -c --argjson ev "$LINKED_EVENTS" '. + $ev')
    else
      echo "::warning::Linked Issue #${LINKED_ISSUE} not found or inaccessible — skipping." >&2
      # Keep RESOLVED_NUMBER as the PR number if linked issue is inaccessible
      RESOLVED_NUMBER="${NUMBER}"
      echo "resolved_number=${RESOLVED_NUMBER}" >> "${GITHUB_OUTPUT:-/dev/null}"
    fi
  fi

  # ── PR body event ────────────────────────────────────────────────────
  # Use $'\n' (ANSI-C quoting) for actual newline characters.
  # Plain "\n" inside double quotes is a literal backslash-n in bash.
  NL=$'\n'
  PR_CONTENT="PR #${NUMBER}: ${PR_TITLE}"
  [ -n "$PR_LABELS" ]   && PR_CONTENT="${PR_CONTENT}${NL}**Labels:** ${PR_LABELS}"
  [ -n "$LINKED_ISSUE" ] && PR_CONTENT="${PR_CONTENT}${NL}**Linked Issue:** #${LINKED_ISSUE}"
  PR_CONTENT="${PR_CONTENT}${NL}${NL}${PR_BODY}"

  PR_OPENED_EVENT=$(jq -n \
    --arg id "pr-${NUMBER}" \
    --arg content "$PR_CONTENT" \
    --arg author "$PR_AUTHOR" \
    --arg created_at "$PR_CREATED" \
    '{id: $id, event_type: "pr_opened", content: $content, author: $author, created_at: $created_at}')
  ALL_EVENTS=$(echo "$ALL_EVENTS" | jq -c --argjson ev "[$PR_OPENED_EVENT]" '. + $ev')

  # ── PR diff event ────────────────────────────────────────────────────
  # Stable ID (no SHA) — updated in-place on each push.
  # created_at = PR updated_at so it sorts after the PR body but before post-push comments.
  DIFF=$(gh api "repos/${OWNER}/${REPO}/pulls/${NUMBER}" \
    -H "Accept: application/vnd.github.v3.diff" 2>/dev/null || true)

  if [ -n "$DIFF" ]; then
    DIFF_TRUNCATED="${DIFF:0:${MAX_DIFF_CHARS}}"
    DIFF_CONTENT="$(printf '```diff\n%s\n```' "${DIFF_TRUNCATED}")"
    if [ ${#DIFF} -gt "$MAX_DIFF_CHARS" ]; then
      DIFF_CONTENT="${DIFF_CONTENT}"$'\n\n'"*[Diff truncated at ${MAX_DIFF_CHARS} characters due to size]*"
    fi

    DIFF_EVENT=$(jq -n \
      --arg id "pr-${NUMBER}-diff" \
      --arg content "$DIFF_CONTENT" \
      --arg sha "$HEAD_SHA" \
      --arg created_at "$PR_UPDATED" \
      '{id: $id, event_type: "pr_diff", content: $content, sha: $sha,
        author: "github", created_at: $created_at}')
    ALL_EVENTS=$(echo "$ALL_EVENTS" | jq -c --argjson ev "[$DIFF_EVENT]" '. + $ev')
  fi

  # ── PR conversation comments ─────────────────────────────────────────
  PR_COMMENTS=$(gh api "repos/${OWNER}/${REPO}/issues/${NUMBER}/comments" \
    --paginate \
    --jq '[.[] | {id: .id, event_type: "pr_comment", content: .body,
                  author: .user.login, created_at: .created_at}]' \
    | jq -s 'add // []')
  ALL_EVENTS=$(echo "$ALL_EVENTS" | jq -c --argjson ev "$PR_COMMENTS" '. + $ev')

  # ── PR review submissions (overall review body / state) ──────────────
  PR_REVIEWS=$(gh api "repos/${OWNER}/${REPO}/pulls/${NUMBER}/reviews" \
    --paginate \
    --jq '[.[] | select(.submitted_at != null) | {
      id: ("pr-review-" + (.id | tostring)),
      event_type: "pr_review",
      content: ("Review state: " + .state + "\n\n" + (.body // "")),
      author: .user.login,
      created_at: .submitted_at
    }]' | jq -s 'add // []')
  ALL_EVENTS=$(echo "$ALL_EVENTS" | jq -c --argjson ev "$PR_REVIEWS" '. + $ev')

  # ── PR inline review comments ─────────────────────────────────────────
  PR_INLINE=$(gh api "repos/${OWNER}/${REPO}/pulls/${NUMBER}/comments" \
    --paginate \
    --jq '[.[] | {
      id: .id,
      event_type: "pr_review_comment",
      content: ("On `\(.path)` line \(.line // .original_line // "?"):\n\n\(.body)"),
      author: .user.login,
      created_at: .created_at
    }]' | jq -s 'add // []')
  ALL_EVENTS=$(echo "$ALL_EVENTS" | jq -c --argjson ev "$PR_INLINE" '. + $ev')

  # ── Sort by created_at ───────────────────────────────────────────────
  echo "$ALL_EVENTS" | jq 'sort_by(.created_at)' > "$OUT_FILE"

else
  echo "ERROR: TYPE must be 'issue' or 'pr', got '${TYPE}'" >&2
  exit 1
fi

COUNT=$(jq 'length' "$OUT_FILE")
echo "Fetched ${COUNT} events → ${OUT_FILE}" >&2
