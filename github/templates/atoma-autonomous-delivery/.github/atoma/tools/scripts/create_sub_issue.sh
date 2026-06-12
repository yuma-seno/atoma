#!/usr/bin/env bash
set -euo pipefail

TITLE=""
BODY=""
PARENT=""
NOTIFY_AGENT=""
TRIGGER_AGENT=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --title)
            TITLE="$2"
            shift 2
            ;;
        --body)
            BODY="$2"
            shift 2
            ;;
        --parent-issue)
            PARENT="$2"
            shift 2
            ;;
        --notify-agent)
            NOTIFY_AGENT="$2"
            shift 2
            ;;
        --trigger-agent)
            TRIGGER_AGENT="$2"
            shift 2
            ;;
        -h|--help)
            echo "Usage: create_sub_issue --title TITLE --body BODY --parent-issue N [--notify-agent AgentName] [--trigger-agent AgentName]"
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

if [[ -z "$PARENT" ]]; then
    echo "Error: --parent-issue is required" >&2
    exit 1
fi

if ! [[ "$PARENT" =~ ^[0-9]+$ ]]; then
    echo "Error: --parent-issue must be a positive integer" >&2
    exit 1
fi

if [[ -z "$NOTIFY_AGENT" ]]; then
    NOTIFY_AGENT="${ATOMA_SUB_ISSUE_NOTIFY_AGENT-orchestrator}"
fi
if [[ -n "$NOTIFY_AGENT" ]] && ! [[ "$NOTIFY_AGENT" =~ ^[a-z][a-z0-9-]*$ ]]; then
    echo "Error: --notify-agent must be a lowercase role name" >&2
    exit 1
fi

if [[ -z "$TRIGGER_AGENT" ]]; then
    TRIGGER_AGENT="${ATOMA_SUB_ISSUE_TRIGGER_AGENT-engineer}"
fi
if [[ -n "$TRIGGER_AGENT" ]] && ! [[ "$TRIGGER_AGENT" =~ ^[a-z][a-z0-9-]*$ ]]; then
    echo "Error: --trigger-agent must be a lowercase role name" >&2
    exit 1
fi

META="<!-- atoma:parent=#${PARENT}"
if [[ -n "$NOTIFY_AGENT" ]]; then
    META="${META} atoma:notify=${NOTIFY_AGENT}"
fi
META="${META} -->"
FULL_BODY="${META}

${BODY}"

BODY_FILE=$(mktemp)
printf '%s' "$FULL_BODY" > "$BODY_FILE"

echo "Creating sub-issue under parent #${PARENT} ..." >&2
ISSUE_URL=$(gh issue create --title "$TITLE" --body-file "$BODY_FILE")
rm -f "$BODY_FILE"

ISSUE_NUM=$(echo "$ISSUE_URL" | grep -oE '[0-9]+$')

echo "Sub-issue created: #${ISSUE_NUM} (${ISSUE_URL})" >&2

# Add pending label to mark this as a sub-issue. Do NOT auto-trigger
# the worker agent — the orchestrator (or a human) will add the
# atoma/<trigger> label later via add_label.sh when ready.
echo "Adding label 'atoma/pending' to sub-issue #${ISSUE_NUM} (parent #${PARENT}) ..." >&2
gh issue edit "${ISSUE_NUM}" --add-label "atoma/pending" 2>/dev/null || true

if [[ -n "$TRIGGER_AGENT" ]]; then
  gh issue comment "$ISSUE_NUM" \
    --body "<!-- atoma:auto-triggered -->
**Atoma:** ${TRIGGER_AGENT} has been reserved for sub-task #${ISSUE_NUM}.

Parent Issue: #${PARENT}

The orchestrator will add the \`atoma/${TRIGGER_AGENT}\` label to start this sub-task. A human can also trigger it manually by adding the label." >&2
fi

echo "#${ISSUE_NUM} labelled atoma/pending (trigger: ${TRIGGER_AGENT:-none})" >&2

echo "$ISSUE_NUM"
