#!/usr/bin/env bash
# dev-install.sh — update claude-deploy + build and install claude-deploy-sentinel
# Run from the repo root after pulling latest.
set -e

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="${HOME}/.local/bin"

echo "→ repo: $REPO_DIR"
echo "→ bin:  $BIN_DIR"
mkdir -p "$BIN_DIR"

# Pull latest
echo
echo "── git pull ──────────────────────────────"
git -C "$REPO_DIR" pull --ff-only

# Install claude-deploy bash script
echo
echo "── claude-deploy ─────────────────────────"
install -m 755 "$REPO_DIR/claude-deploy" "$BIN_DIR/claude-deploy"
echo "✓ claude-deploy → $BIN_DIR/claude-deploy"

# Build + install sentinel binary
echo
echo "── claude-deploy-sentinel ────────────────"
cargo install --path "$REPO_DIR/sentinel" --root "$HOME/.local" --locked 2>&1 \
    || cargo install --path "$REPO_DIR/sentinel" --root "$HOME/.local"
echo "✓ claude-deploy-sentinel → $BIN_DIR/claude-deploy-sentinel"

echo
echo "✓ done"
