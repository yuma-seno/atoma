#!/usr/bin/env bash
set -euo pipefail

TITLE=""
DESCRIPTION=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --title)
            TITLE="$2"
            shift 2
            ;;
        --description)
            DESCRIPTION="$2"
            shift 2
            ;;
        -h|--help)
            echo "Usage: create_pr --title TITLE --description DESCRIPTION"
            exit 0
            ;;
        *)
            echo "Error: Unknown argument: $1" >&2
            exit 1
            ;;
    esac
done

if [[ -z "$TITLE" ]]; then
    echo "Error: --title is required" >&2
    exit 1
fi

LINKED_ISSUE="${ISSUE_NUMBER:-}"
RUN_ID="${GITHUB_RUN_ID:-$(date +%s)}"

if [[ -n "$LINKED_ISSUE" ]]; then
    BRANCH="atoma/issue-${LINKED_ISSUE}-${RUN_ID}"
else
    BRANCH="atoma/run-${RUN_ID}"
fi

if ! [[ "$BRANCH" =~ ^[a-zA-Z0-9/_-]+$ ]]; then
    echo "Error: Derived branch name contains invalid characters: ${BRANCH}" >&2
    exit 1
fi

BASE_BRANCH=$(git symbolic-ref refs/remotes/origin/HEAD 2>/dev/null \
    | sed 's|refs/remotes/origin/||' \
    || git remote show origin 2>/dev/null | grep 'HEAD branch' | sed 's/.*: //' \
    || echo "main")

echo "Pushing to branch: ${BRANCH}" >&2
git push origin "HEAD:${BRANCH}" >&2

BODY="${DESCRIPTION}"
if [[ -n "$LINKED_ISSUE" ]]; then
    BODY="${BODY}

Closes #${LINKED_ISSUE}

<!-- atoma-linked-issue: ${LINKED_ISSUE} -->"
fi

BODY_FILE=$(mktemp)
printf '%s' "$BODY" > "$BODY_FILE"

echo "Creating PR on GitHub..." >&2
PR_URL=$(gh pr create \
    --title "$TITLE" \
    --body-file "$BODY_FILE" \
    --head "$BRANCH" \
    --base "$BASE_BRANCH")

rm -f "$BODY_FILE"

echo "$PR_URL"
echo "PR created: $PR_URL" >&2

PR_NUM=$(echo "$PR_URL" | grep -oE '[0-9]+$')
DISPATCH_WORKFLOW="${ATOMA_DISPATCH_WORKFLOW-atoma-runner.yml}"
DISPATCH_AGENT="${ATOMA_CREATE_PR_AGENT-reviewer}"

if [[ -z "$DISPATCH_AGENT" ]]; then
    echo "No follow-up agent configured; skipping workflow dispatch for PR #${PR_NUM}." >&2
    exit 0
fi

gh workflow run "$DISPATCH_WORKFLOW" \
    --field agent="$DISPATCH_AGENT" \
    --field number="$PR_NUM" \
    --field type="pr" \
    --field notify="" >&2

echo "${DISPATCH_AGENT} dispatched for PR #${PR_NUM}" >&2
