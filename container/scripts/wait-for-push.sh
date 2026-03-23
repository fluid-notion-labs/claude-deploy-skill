#!/usr/bin/env bash
# wait-for-push.sh <repo_dir> [timeout_seconds]
# Polls for new commits on origin. Exits 0 on change, 1 on timeout.
# Usage: wait-for-push.sh /path/to/repo [60]

REPO="${1:-.}"
TIMEOUT="${2:-60}"
INTERVAL=5
ELAPSED=0

if [[ ! -d "$REPO/.git" ]]; then
    echo "Error: not a git repo: $REPO" >&2
    exit 2
fi

BRANCH=$(git -C "$REPO" rev-parse --abbrev-ref HEAD)
BEFORE=$(git -C "$REPO" rev-parse HEAD)

echo "Waiting for push to $REPO [$BRANCH] (timeout: ${TIMEOUT}s)..."

while [[ $ELAPSED -lt $TIMEOUT ]]; do
    sleep $INTERVAL
    ELAPSED=$((ELAPSED + INTERVAL))

    git -C "$REPO" fetch origin "$BRANCH" -q 2>/dev/null
    AFTER=$(git -C "$REPO" rev-parse "origin/$BRANCH")

    if [[ "$BEFORE" != "$AFTER" ]]; then
        git -C "$REPO" pull --ff-only origin "$BRANCH" -q
        MSG=$(git -C "$REPO" log --oneline -1)
        echo "✓ change detected after ${ELAPSED}s: $MSG"
        exit 0
    fi

    echo "  ${ELAPSED}s / ${TIMEOUT}s — no change yet"
done

echo "✗ timed out after ${TIMEOUT}s" >&2
exit 1
