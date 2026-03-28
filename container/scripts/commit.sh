#!/usr/bin/env bash
# commit.sh — git add -A, commit, push, with automatic token refresh.
# Usage: bash commit.sh <repo-dir> <message> [<branch>]
#
# Before pushing, checks sentinel branch for a fresh token and updates
# the remote URL if one is available.

set -euo pipefail

REPO="${1:?Usage: commit.sh <repo-dir> <message> [<branch>]}"
MSG="${2:?commit message required}"
BRANCH="${3:-}"

cd "$REPO"

# Infer branch if not given
if [[ -z "$BRANCH" ]]; then
    BRANCH=$(git symbolic-ref --short HEAD)
fi

# Attempt token refresh before push
TOKEN=$(bash "$(dirname "$0")/get-latest-token.sh" 2>/dev/null || true)
if [[ -n "$TOKEN" ]]; then
    REMOTE_URL=$(git remote get-url origin 2>/dev/null || true)
    if [[ "$REMOTE_URL" == *"x-access-token:"* ]]; then
        AT="${REMOTE_URL#*@}"
        NEW_URL="https://x-access-token:${TOKEN}@${AT}"
        git remote set-url origin "$NEW_URL"
        echo "→ remote token refreshed" >&2
    fi
fi

git add -A
git commit -m "$MSG"
git push origin "$BRANCH"
