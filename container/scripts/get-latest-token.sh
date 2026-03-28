#!/usr/bin/env bash
# get-latest-token.sh
# Print the latest valid GitHub token from the sentinel branch.
# Usage: bash get-latest-token.sh [<repo-dir>]
#
# Exits 0 and prints token to stdout on success.
# Exits 1 with message to stderr if no valid token found.

set -euo pipefail

REPO="${1:-/home/claude/claude-deploy-skill}"
WT="$REPO/.git/claude-sentinel-wt"

if [[ ! -d "$WT" ]]; then
  echo "No sentinel worktree at $WT — is the watcher running?" >&2
  exit 1
fi

# Sync worktree
git -C "$WT" fetch origin claude-deploy-sentinels -q 2>/dev/null || true
git -C "$WT" reset --hard origin/claude-deploy-sentinels -q 2>/dev/null || true

NOW=$(date -u '+%Y-%m-%dT%H:%M:%SZ')

# Find latest tok- file with a non-expired token (files are named tok-<org>-<ts>, sort descending)
for f in $(ls "$WT"/tok-* 2>/dev/null | sort -r); do
  expires=$(grep "^expires:" "$f" 2>/dev/null | awk '{print $2}')
  token=$(grep "^token:" "$f" 2>/dev/null | awk '{print $2}')
  [[ -z "$expires" || -z "$token" ]] && continue
  # Compare ISO strings lexicographically — works for UTC timestamps
  if [[ "$expires" > "$NOW" ]]; then
    echo "$token"
    exit 0
  fi
done

echo "No valid token found in sentinel branch (all expired or none present)" >&2
exit 1
