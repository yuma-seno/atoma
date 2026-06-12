#!/usr/bin/env bash
set -euo pipefail

LABEL=""
ISSUE="${ISSUE_NUMBER:-}"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --label)
            LABEL="$2"
            shift 2
            ;;
        --issue)
            ISSUE="$2"
            shift 2
            ;;
        -h|--help)
            echo "Usage: add_label --label LABEL [--issue N]"
            exit 0
            ;;
        *)
            echo "Error: Unknown argument: $1" >&2
            exit 1
            ;;
    esac
done

if [[ -z "$LABEL" ]]; then
    echo "Error: --label is required" >&2
    exit 1
fi

if [[ -z "$ISSUE" ]]; then
    echo "Error: --issue is required (or set ISSUE_NUMBER env var)" >&2
    exit 1
fi

if ! [[ "$ISSUE" =~ ^[0-9]+$ ]]; then
    echo "Error: issue number must be a positive integer, got: ${ISSUE}" >&2
    exit 1
fi

label_re='^[A-Za-z0-9:_/ -]+$'
if ! [[ "$LABEL" =~ $label_re ]]; then
    echo "Error: label contains invalid characters: ${LABEL}" >&2
    exit 1
fi

echo "Adding label '${LABEL}' to issue #${ISSUE} ..." >&2
gh issue edit "${ISSUE}" --add-label "${LABEL}"
echo "Label added: ${LABEL}"
