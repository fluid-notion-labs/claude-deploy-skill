#!/usr/bin/env bash
# gh-app-token.sh
# Usage: ./gh-app-token.sh <app_id> <owner/repo>
# Assumes PEM at ~/.config/claude-deploy/private-key.pem
set -e

PEM_PATH="${CLAUDE_DEPLOY_PEM:-$HOME/.config/claude-deploy/private-key.pem}"
APP_ID=$1
REPO=$2

if [[ -z "$APP_ID" || -z "$REPO" ]]; then
    echo "Usage: gh-app-token <app_id> <owner/repo>" >&2
    exit 1
fi

if [[ ! -f "$PEM_PATH" ]]; then
    echo "Error: PEM not found at $PEM_PATH" >&2
    echo "Set CLAUDE_DEPLOY_PEM to override path" >&2
    exit 1
fi

JWT=$(uvx --with pyjwt --with cryptography python3 - << PYEOF
import jwt, time
pem = open("$PEM_PATH").read()
now = int(time.time())
print(jwt.encode({"iat": now - 60, "exp": now + 600, "iss": "$APP_ID"}, pem, algorithm="RS256"))
PYEOF
)

echo "JWT generated, looking up installation..." >&2

INSTALL=$(gh api "/repos/$REPO/installation" \
    --header "Authorization: Bearer $JWT" \
    --header "Accept: application/vnd.github+json")

INSTALL_ID=$(echo "$INSTALL" | jq -r '.id')

if [[ "$INSTALL_ID" == "null" || -z "$INSTALL_ID" ]]; then
    echo "Error: app not installed on $REPO" >&2
    echo "$INSTALL" | jq . >&2
    exit 1
fi

echo "Installation ID: $INSTALL_ID" >&2

TOKEN=$(gh api "/app/installations/$INSTALL_ID/access_tokens" \
    --method POST \
    --header "Authorization: Bearer $JWT" \
    --header "Accept: application/vnd.github+json" \
    --field "repositories[]=$(echo $REPO | cut -d'/' -f2)" \
    --jq '.token')

echo "Token valid until: $(date -d '+1 hour' '+%H:%M %Z')" >&2
echo "$TOKEN"
