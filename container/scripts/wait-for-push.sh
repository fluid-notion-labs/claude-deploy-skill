#!/usr/bin/env bash
# wait-for-push.sh <repo_dir> [timeout_seconds] [--branch <branch>]
# Polls for new commits on a branch. Exits 0 on change, 1 on timeout.
#
# Options:
#   --branch <n>    branch to watch (default: current branch)
#   --sentinel <n>  watch for a specific sentinel to complete
#
# Examples:
#   wait-for-push.sh /repo 60
#   wait-for-push.sh /repo 60 --branch claude-deploy-sentinels
#   wait-for-push.sh /repo 120 --sentinel run-abc123-20260327T000000-a1b2

REPO="."
TIMEOUT=60
INTERVAL=5
BRANCH=""
SENTINEL_NAME=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --branch)   BRANCH="$2"; shift 2 ;;
        --sentinel) SENTINEL_NAME="$2"; shift 2 ;;
        --*)        echo "Unknown flag: $1" >&2; exit 2 ;;
        *)
            if [[ -d "$1/.git" ]]; then REPO="$1"
            elif [[ "$1" =~ ^[0-9]+$ ]]; then TIMEOUT="$1"
            fi
            shift ;;
    esac
done

if [[ ! -d "$REPO/.git" ]]; then
    echo "Error: not a git repo: $REPO" >&2
    exit 2
fi

# Sentinel watch mode
if [[ -n "$SENTINEL_NAME" ]]; then
    WATCH_BRANCH="claude-deploy-sentinels"
    ELAPSED=0
    echo "Waiting for sentinel $SENTINEL_NAME (timeout: ${TIMEOUT}s)..."

    while [[ $ELAPSED -lt $TIMEOUT ]]; do
        sleep $INTERVAL
        ELAPSED=$((ELAPSED + INTERVAL))

        git -C "$REPO" fetch origin "$WATCH_BRANCH" -q 2>/dev/null

        STATUS=$(git -C "$REPO" show "origin/${WATCH_BRANCH}:${SENTINEL_NAME}" 2>/dev/null \
            | grep "^status:" | head -1 | sed 's/^status:[[:space:]]*//')

        case "$STATUS" in
            success|failure|abandoned)
                echo "✓ $SENTINEL_NAME → $STATUS (${ELAPSED}s)"
                git -C "$REPO" show "origin/${WATCH_BRANCH}:${SENTINEL_NAME}" 2>/dev/null \
                    | awk '/^# --- log ---/{found=1; next} found{sub(/^# ?/,""); print}'
                # Check for a fresh token while we're here
                FRESH_TOKEN=$(bash "$(dirname "$0")/get-latest-token.sh" "$REPO" 2>/dev/null || true)
                if [[ -n "$FRESH_TOKEN" ]]; then
                    echo ""
                    echo "🔑 fresh token available — update remotes with:"
                    echo "   TOKEN=$FRESH_TOKEN"
                    echo "   git remote set-url origin https://x-access-token:\$TOKEN@github.com/<owner>/<repo>.git"
                fi
                [[ "$STATUS" == "success" ]] && exit 0 || exit 1
                ;;
            new|claiming|running)
                echo "  ${ELAPSED}s / ${TIMEOUT}s — $STATUS" ;;
            *)
                echo "  ${ELAPSED}s / ${TIMEOUT}s — not found yet" ;;
        esac
    done

    echo "✗ timed out after ${TIMEOUT}s (last: ${STATUS:-unknown})" >&2
    exit 1
fi

# Branch watch mode
[[ -z "$BRANCH" ]] && BRANCH=$(git -C "$REPO" rev-parse --abbrev-ref HEAD)
BEFORE=$(git -C "$REPO" rev-parse "origin/$BRANCH" 2>/dev/null || git -C "$REPO" rev-parse HEAD)
ELAPSED=0

echo "Waiting for push to $REPO [$BRANCH] (timeout: ${TIMEOUT}s)..."

while [[ $ELAPSED -lt $TIMEOUT ]]; do
    sleep $INTERVAL
    ELAPSED=$((ELAPSED + INTERVAL))

    git -C "$REPO" fetch origin "$BRANCH" -q 2>/dev/null
    AFTER=$(git -C "$REPO" rev-parse "origin/$BRANCH" 2>/dev/null)

    if [[ "$BEFORE" != "$AFTER" ]]; then
        git -C "$REPO" pull --ff-only origin "$BRANCH" -q 2>/dev/null || true
        echo "✓ change detected after ${ELAPSED}s:"
        git -C "$REPO" log --oneline "${BEFORE}..${AFTER}" 2>/dev/null | head -5 | sed 's/^/  /'
        exit 0
    fi

    echo "  ${ELAPSED}s / ${TIMEOUT}s — no change yet"
done

echo "✗ timed out after ${TIMEOUT}s" >&2
exit 1
