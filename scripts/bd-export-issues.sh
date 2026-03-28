#!/usr/bin/env bash
# bd-export-issues.sh
# Export bd (Beads) issues from the current repo into docs/issues/
#
# Usage: bd-export-issues.sh [--bd <path>] [--filter <bd-filter-args>]
#
# Outputs:
#   docs/issues/context.md   — human/Claude-readable summary list
#   docs/issues/context.json — full issue data for programmatic use
#
# Run from the repo root. Requires bd in PATH or --bd <path>.

set -euo pipefail

BD=${BD:-$(command -v bd 2>/dev/null || echo "")}
FILTER_ARGS=()
OUT_DIR="docs/issues"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --bd) BD="$2"; shift 2 ;;
    --filter) shift; FILTER_ARGS+=("$@"); break ;;
    *) echo "Unknown arg: $1" >&2; exit 1 ;;
  esac
done

if [[ -z "$BD" ]]; then
  echo "Error: bd not found. Install it or pass --bd <path>" >&2
  exit 1
fi

if ! "$BD" status &>/dev/null; then
  echo "Error: no bd database found in $(pwd)" >&2
  exit 1
fi

mkdir -p "$OUT_DIR"

echo "Exporting issues from $(pwd)..."

# --- JSON export ---
"$BD" list --json "${FILTER_ARGS[@]}" > "$OUT_DIR/context.json"
TOTAL=$(jq 'length' "$OUT_DIR/context.json")
echo "  $TOTAL issues → $OUT_DIR/context.json"

# --- Markdown export ---
REPO_NAME=$(basename "$(pwd)")
GENERATED=$(date -u '+%Y-%m-%dT%H:%M:%SZ')

{
  echo "# Issues — $REPO_NAME"
  echo ""
  echo "_Generated: $GENERATED — $TOTAL open issues_"
  echo ""

  # Group by status
  for STATUS in open in-progress blocked closed; do
    ITEMS=$(jq -r --arg s "$STATUS" '
      .[] | select(.status == $s) |
      "- **\(.id)** \(.title)" +
      (if .labels and (.labels | length > 0) then " `" + (.labels | join("` `")) + "`" else "" end) +
      (if .blocked_by and (.blocked_by | length > 0) then " _(blocked by: " + (.blocked_by | join(", ")) + ")_" else "" end)
    ' "$OUT_DIR/context.json" 2>/dev/null || true)

    if [[ -n "$ITEMS" ]]; then
      echo "## $(echo "$STATUS" | sed 's/-/ /g' | awk '{for(i=1;i<=NF;i++) $i=toupper(substr($i,1,1)) substr($i,2); print}')"
      echo ""
      echo "$ITEMS"
      echo ""
    fi
  done

  # Anything with an unknown status
  OTHER=$(jq -r '
    .[] | select(.status != "open" and .status != "in-progress" and .status != "blocked" and .status != "closed") |
    "- **\(.id)** \(.title) _(status: \(.status))_"
  ' "$OUT_DIR/context.json" 2>/dev/null || true)

  if [[ -n "$OTHER" ]]; then
    echo "## Other"
    echo ""
    echo "$OTHER"
    echo ""
  fi

} > "$OUT_DIR/context.md"

echo "  → $OUT_DIR/context.md"
echo "Done."
