#!/usr/bin/env bash
set -euo pipefail

PR=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --pr)
            PR="$2"
            shift 2
            ;;
        -h|--help)
            echo "Usage: push_commits --pr N"
            exit 0
            ;;
        *)
            echo "Error: Unknown argument: $1" >&2
            exit 1
            ;;
    esac
done

if [[ -z "$PR" ]]; then
    echo "Error: --pr is required" >&2
    exit 1
fi

if ! [[ "$PR" =~ ^[0-9]+$ ]]; then
    echo "Error: --pr must be a positive integer, got: ${PR}" >&2
    exit 1
fi

BRANCH=$(gh pr view "$PR" --json headRefName -q '.headRefName')

if [[ -z "$BRANCH" ]]; then
    echo "Error: could not determine head branch for PR #${PR}" >&2
    exit 1
fi

if ! [[ "$BRANCH" =~ ^atoma/[a-zA-Z0-9._/-]+$ ]]; then
    echo "Error: branch '${BRANCH}' is not a valid Atoma-managed branch." >&2
    exit 1
fi

echo "Pushing commits to ${BRANCH} (PR #${PR}) ..." >&2
git push origin "HEAD:${BRANCH}"
echo "Pushed to ${BRANCH}."

DISPATCH_WORKFLOW="${ATOMA_DISPATCH_WORKFLOW-atoma-runner.yml}"
DISPATCH_AGENT="${ATOMA_PUSH_COMMITS_AGENT-reviewer}"

if [[ -z "$DISPATCH_AGENT" ]]; then
    echo "No follow-up agent configured; skipping workflow dispatch for PR #${PR}." >&2
    exit 0
fi

gh workflow run "$DISPATCH_WORKFLOW" \
    --field agent="$DISPATCH_AGENT" \
    --field number="$PR" \
    --field type="pr" \
    --field notify="" >&2

echo "${DISPATCH_AGENT} dispatched for PR #${PR}" >&2
