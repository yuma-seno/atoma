#!/usr/bin/env bash
set -euo pipefail

ISSUE="${ISSUE_NUMBER:-}"
COMMENT=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --issue)
            ISSUE="$2"
            shift 2
            ;;
        --comment)
            COMMENT="$2"
            shift 2
            ;;
        -h|--help)
            echo "Usage: close_issue [--issue N] [--comment REASON]"
            exit 0
            ;;
        *)
            echo "Error: Unknown argument: $1" >&2
            exit 1
            ;;
    esac
done

if [[ -z "$ISSUE" ]]; then
    echo "Error: --issue is required (or set ISSUE_NUMBER env var)" >&2
    exit 1
fi

if ! [[ "$ISSUE" =~ ^[0-9]+$ ]]; then
    echo "Error: issue number must be a positive integer, got: ${ISSUE}" >&2
    exit 1
fi

if [[ -n "$COMMENT" ]]; then
    gh issue comment "${ISSUE}" --body "${COMMENT}"
fi

echo "Closing issue #${ISSUE} ..." >&2
gh issue close "${ISSUE}" --reason "completed"
echo "Issue #${ISSUE} closed."
