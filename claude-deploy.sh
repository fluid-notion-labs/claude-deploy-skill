#!/usr/bin/env bash
# claude-deploy - manage Claude GitHub deploy credentials
set -e

CONFIG_DIR="${CLAUDE_DEPLOY_DIR:-$HOME/.config/claude-deploy}"
PROFILE="default"

usage() {
    cat << USAGE
Usage: claude-deploy <command> [options]

Commands:
  setup    Configure a profile (ingest PEM, store App ID)
  token    Generate a GitHub App token for a repo
  profiles List configured profiles
  status   Show config for current/specified profile

Options:
  --profile <name>   Use named profile (default: "default")

Examples:
  claude-deploy setup --profile fluid-notion-labs
  claude-deploy token fluid-notion-labs/claude-deploy-skill
  claude-deploy token fluid-notion-labs/claude-deploy-skill --profile fluid-notion-labs
  claude-deploy profiles
  claude-deploy status --profile fluid-notion-labs
USAGE
    exit 1
}

config_file() {
    if [[ "$PROFILE" == "default" ]]; then
        echo "$CONFIG_DIR/config"
    else
        echo "$CONFIG_DIR/config-$PROFILE"
    fi
}

load_config() {
    local cfg=$(config_file)
    if [[ ! -f "$cfg" ]]; then
        echo "Error: no config for profile '$PROFILE' at $cfg" >&2
        echo "Run: claude-deploy setup --profile $PROFILE" >&2
        exit 1
    fi
    source "$cfg"
}

cmd_setup() {
    local pem_src=""
    local app_id=""

    while [[ $# -gt 0 ]]; do
        case $1 in
            --profile) PROFILE=$2; shift 2 ;;
            --app-id)  app_id=$2; shift 2 ;;
            *)         pem_src=$1; shift ;;
        esac
    done

    mkdir -p "$CONFIG_DIR"
    chmod 700 "$CONFIG_DIR"

    # PEM
    if [[ -z "$pem_src" ]]; then
        read -rp "Path to PEM file: " pem_src
    fi
    pem_src="${pem_src/#\~/$HOME}"
    if [[ ! -f "$pem_src" ]]; then
        echo "Error: PEM not found at $pem_src" >&2
        exit 1
    fi
    local pem_dest="$CONFIG_DIR/private-key-$PROFILE.pem"
    cp "$pem_src" "$pem_dest"
    chmod 600 "$pem_dest"
    echo "✓ PEM stored at $pem_dest" >&2

    # App ID
    if [[ -z "$app_id" ]]; then
        read -rp "GitHub App ID: " app_id
    fi

    # Write config
    local cfg=$(config_file)
    cat > "$cfg" << CONF
APP_ID=$app_id
PEM_PATH=$pem_dest
CONF
    chmod 600 "$cfg"
    echo "✓ Profile '$PROFILE' saved to $cfg" >&2
}

cmd_token() {
    local repo=""

    while [[ $# -gt 0 ]]; do
        case $1 in
            --profile) PROFILE=$2; shift 2 ;;
            *)         repo=$1; shift ;;
        esac
    done

    if [[ -z "$repo" ]]; then
        echo "Usage: claude-deploy token <owner/repo> [--profile <name>]" >&2
        exit 1
    fi

    load_config

    local JWT
    JWT=$(uvx --with pyjwt --with cryptography python3 - << PYEOF
import jwt, time
pem = open("$PEM_PATH").read()
now = int(time.time())
print(jwt.encode({"iat": now - 60, "exp": now + 600, "iss": "$APP_ID"}, pem, algorithm="RS256"))
PYEOF
)

    echo "JWT generated, looking up installation..." >&2

    local INSTALL
    INSTALL=$(gh api "/repos/$repo/installation" \
        --header "Authorization: Bearer $JWT" \
        --header "Accept: application/vnd.github+json")

    local INSTALL_ID
    INSTALL_ID=$(echo "$INSTALL" | jq -r '.id')

    if [[ "$INSTALL_ID" == "null" || -z "$INSTALL_ID" ]]; then
        echo "Error: app not installed on $repo" >&2
        echo "$INSTALL" | jq . >&2
        exit 1
    fi

    echo "Installation ID: $INSTALL_ID" >&2

    local TOKEN
    TOKEN=$(gh api "/app/installations/$INSTALL_ID/access_tokens" \
        --method POST \
        --header "Authorization: Bearer $JWT" \
        --header "Accept: application/vnd.github+json" \
        --field "repositories[]=$(echo $repo | cut -d'/' -f2)" \
        --jq '.token')

    local EXPIRY
    EXPIRY=$(date -d '+1 hour' '+%H:%M %Z')

    if command -v wl-copy &> /dev/null; then
        echo -n "$TOKEN" | wl-copy
        echo "📋 Token copied to clipboard (valid until $EXPIRY)" >&2
    else
        echo "⚠️  wl-copy not found — copy token below manually" >&2
    fi

    echo "$TOKEN"
}

cmd_profiles() {
    echo "Configured profiles:" >&2
    local found=0
    for f in "$CONFIG_DIR"/config "$CONFIG_DIR"/config-*; do
        [[ -f "$f" ]] || continue
        local name
        name=$(basename "$f" | sed 's/^config-\?//')
        [[ -z "$name" ]] && name="default"
        echo "  $name ($f)"
        found=1
    done
    [[ $found -eq 0 ]] && echo "  none — run: claude-deploy setup"
}

cmd_status() {
    while [[ $# -gt 0 ]]; do
        case $1 in
            --profile) PROFILE=$2; shift 2 ;;
            *) shift ;;
        esac
    done
    load_config
    echo "Profile:  $PROFILE"
    echo "App ID:   $APP_ID"
    echo "PEM:      $PEM_PATH"
    [[ -f "$PEM_PATH" ]] && echo "PEM exists: ✓" || echo "PEM exists: ✗ NOT FOUND"
}

# Parse global --profile before subcommand
while [[ $# -gt 0 ]]; do
    case $1 in
        --profile) PROFILE=$2; shift 2 ;;
        setup)    shift; cmd_setup "$@"; exit ;;
        token)    shift; cmd_token "$@"; exit ;;
        profiles) shift; cmd_profiles; exit ;;
        status)   shift; cmd_status "$@"; exit ;;
        *)        usage ;;
    esac
done

usage
