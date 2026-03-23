#!/usr/bin/env bash
# loaded by bats via `load helpers` — sources claude-deploy in test mode

SCRIPT="$BATS_TEST_DIRNAME/../claude-deploy"

# Each test runs in a subshell — source the script fresh per test context.
# Call this at the top of setup() after setting CONFIG_DIR/TEST_DIR.
source_script() {
    CLAUDE_DEPLOY_TEST=1 source "$SCRIPT"
}

make_config() {
    local profile="$1" account_type="${2:-org}" app_id="${3:-12345}"
    local file
    if [[ "$profile" == "default" ]]; then
        file="$CONFIG_DIR/config"
    else
        file="$CONFIG_DIR/config-$profile"
    fi
    cat > "$file" <<EOF
APP_ID=$app_id
PEM_PATH=$CONFIG_DIR/private-key-$profile.pem
ACCOUNT_TYPE=$account_type
AUTO_UPDATE=0
EOF
    touch "$CONFIG_DIR/private-key-$profile.pem"
}
