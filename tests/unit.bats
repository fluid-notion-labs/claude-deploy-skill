#!/usr/bin/env bats
# Unit tests for claude-deploy
# Run: bats tests/unit.bats  (from repo root)

bats_require_minimum_version 1.5.0

SCRIPT="${BATS_TEST_DIRNAME}/../claude-deploy"

# ── setup/teardown ────────────────────────────────────────────────────────────

setup() {
    TEST_DIR="$(mktemp -d)"
    export CONFIG_DIR="$TEST_DIR"
    export CLAUDE_DEPLOY_DIR="$TEST_DIR"
    export CLAUDE_DEPLOY_TEST=1
    source "$SCRIPT"
}

teardown() {
    rm -rf "$TEST_DIR"
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

# Run a snippet in a fresh subshell with the script sourced.
# Needed for tests that use `run` to capture exit codes/output.
run_sourced() {
    bash -c "
        export CLAUDE_DEPLOY_TEST=1
        export CONFIG_DIR='$CONFIG_DIR'
        source '$SCRIPT'
        $1
    "
}

# ── config_file ───────────────────────────────────────────────────────────────

@test "config_file: default -> config" {
    result=$(config_file "default")
    [ "$result" = "$CONFIG_DIR/config" ]
}

@test "config_file: named -> config-<n>" {
    result=$(config_file "myorg")
    [ "$result" = "$CONFIG_DIR/config-myorg" ]
}

# ── parse_profile ─────────────────────────────────────────────────────────────

@test "parse_profile: no args -> default, empty positional" {
    parse_profile
    [ "$PROFILE" = "default" ]
    [ "${#POSITIONAL[@]}" -eq 0 ]
}

@test "parse_profile: --org sets PROFILE" {
    parse_profile --org myorg
    [ "$PROFILE" = "myorg" ]
    [ "${#POSITIONAL[@]}" -eq 0 ]
}

@test "parse_profile: positional collected" {
    parse_profile owner/repo
    [ "$PROFILE" = "default" ]
    [ "${POSITIONAL[0]}" = "owner/repo" ]
}

@test "parse_profile: --org and positional together" {
    parse_profile owner/repo --org myorg
    [ "$PROFILE" = "myorg" ]
    [ "${POSITIONAL[0]}" = "owner/repo" ]
}

@test "parse_profile: --org before positional" {
    parse_profile --org myorg owner/repo
    [ "$PROFILE" = "myorg" ]
    [ "${POSITIONAL[0]}" = "owner/repo" ]
}

@test "parse_profile: multiple positionals" {
    parse_profile foo bar baz
    [ "${POSITIONAL[0]}" = "foo" ]
    [ "${POSITIONAL[1]}" = "bar" ]
    [ "${POSITIONAL[2]}" = "baz" ]
}

# ── load_config ───────────────────────────────────────────────────────────────

@test "load_config: missing config exits with error" {
    run run_sourced "load_config nonexistent"
    [ "$status" -ne 0 ]
    [[ "$output" =~ "no config for org" ]]
}

@test "load_config: sources APP_ID and ACCOUNT_TYPE" {
    make_config "default" "org" "99999"
    load_config "default"
    [ "$APP_ID" = "99999" ]
    [ "$ACCOUNT_TYPE" = "org" ]
}

@test "load_config: named org config" {
    make_config "myorg" "user" "42"
    load_config "myorg"
    [ "$APP_ID" = "42" ]
    [ "$ACCOUNT_TYPE" = "user" ]
}

# ── infer_single_org ──────────────────────────────────────────────────────────

@test "infer_single_org: no configs -> returns 1" {
    PROFILE="default"
    run infer_single_org
    [ "$status" -eq 1 ]
}

@test "infer_single_org: single org -> sets PROFILE" {
    make_config "acme"
    PROFILE="default"
    infer_single_org
    [ "$PROFILE" = "acme" ]
}

@test "infer_single_org: multiple orgs -> returns 1" {
    make_config "acme"
    make_config "widgets"
    PROFILE="default"
    run infer_single_org
    [ "$status" -eq 1 ]
}

# ── infer_profile ─────────────────────────────────────────────────────────────

@test "infer_profile: owner matches named config" {
    make_config "acme"
    PROFILE="default"
    infer_profile "acme/myrepo"
    [ "$PROFILE" = "acme" ]
}

@test "infer_profile: no owner match, single org -> infers it" {
    make_config "widgets"
    PROFILE="default"
    infer_profile "other/repo"
    [ "$PROFILE" = "widgets" ]
}

@test "infer_profile: PROFILE already set -> unchanged" {
    make_config "acme"
    PROFILE="acme"
    infer_profile "other/repo"
    [ "$PROFILE" = "acme" ]
}

@test "infer_profile: no match, multiple orgs -> stays default" {
    make_config "acme"
    make_config "widgets"
    PROFILE="default"
    infer_profile "other/repo"
    [ "$PROFILE" = "default" ]
}

# ── clipboard_copy ────────────────────────────────────────────────────────────
# clipboard_copy is a thin dispatch shim — we test the error path (no tools
# found) and trust the branching logic. The happy paths require real clipboard
# tools and are better covered by manual/integration testing.

@test "clipboard_copy: no tool available -> returns 1" {
    # Run with an empty PATH so no clipboard tool can be found
    run env PATH="/bin:/usr/bin" bash -c "
        CLAUDE_DEPLOY_TEST=1 CONFIG_DIR='$CONFIG_DIR' source '$SCRIPT'
        clipboard_copy hello
    "
    [ "$status" -eq 1 ]
}

@test "clipboard_copy: no tool -> error names all three tools" {
    run env PATH="/bin:/usr/bin" bash -c "
        CLAUDE_DEPLOY_TEST=1 CONFIG_DIR='$CONFIG_DIR' source '$SCRIPT'
        clipboard_copy hello
    "
    [[ "$output" =~ "wl-copy" ]]
    [[ "$output" =~ "xclip" ]]
    [[ "$output" =~ "xsel" ]]
}

@test "clipboard_copy: no tool -> suggests install" {
    run env PATH="/bin:/usr/bin" bash -c "
        CLAUDE_DEPLOY_TEST=1 CONFIG_DIR='$CONFIG_DIR' source '$SCRIPT'
        clipboard_copy hello
    "
    [[ "$output" =~ "Install" ]]
}

# ── copy_or_print ─────────────────────────────────────────────────────────────
# `run` spawns a subshell — use export -f to pass mock functions through.

@test "copy_or_print: clipboard fails -> prints to stdout" {
    clipboard_copy() { return 1; }
    export -f clipboard_copy
    run copy_or_print "mytoken" "Token copied"
    [ "$status" -eq 0 ]
    [ "$output" = "mytoken" ]
    unset -f clipboard_copy
}

@test "copy_or_print: clipboard succeeds -> content not on stdout" {
    clipboard_copy() { return 0; }
    export -f clipboard_copy
    run --separate-stderr copy_or_print "mytoken" "Token copied"
    [ "$status" -eq 0 ]
    # stdout should be empty; label goes to stderr
    [ -z "$output" ]
    [[ "$stderr" =~ "Token copied" ]]
    unset -f clipboard_copy
}

@test "copy_or_print: --always-print outputs content to stdout" {
    clipboard_copy() { return 0; }
    export -f clipboard_copy
    run --separate-stderr copy_or_print "mytoken" "Token copied" --always-print
    [ "$status" -eq 0 ]
    [ "$output" = "mytoken" ]
    unset -f clipboard_copy
}

# ── cmd_config ────────────────────────────────────────────────────────────────

@test "cmd_config: auto-update on -> AUTO_UPDATE=1" {
    POSITIONAL=(auto-update on)
    cmd_config
    grep -q "^AUTO_UPDATE=1$" "$CONFIG_DIR/config"
}

@test "cmd_config: auto-update off -> AUTO_UPDATE=0" {
    POSITIONAL=(auto-update off)
    cmd_config
    grep -q "^AUTO_UPDATE=0$" "$CONFIG_DIR/config"
}

@test "cmd_config: auto-update toggles 0 -> 1" {
    echo "AUTO_UPDATE=0" > "$CONFIG_DIR/config"
    POSITIONAL=(auto-update)
    cmd_config
    grep -q "^AUTO_UPDATE=1$" "$CONFIG_DIR/config"
}

@test "cmd_config: auto-update toggles 1 -> 0" {
    echo "AUTO_UPDATE=1" > "$CONFIG_DIR/config"
    POSITIONAL=(auto-update)
    cmd_config
    grep -q "^AUTO_UPDATE=0$" "$CONFIG_DIR/config"
}

@test "cmd_config: 1 is equivalent to on" {
    POSITIONAL=(auto-update 1)
    cmd_config
    grep -q "^AUTO_UPDATE=1$" "$CONFIG_DIR/config"
}

@test "cmd_config: unknown key -> exits non-zero" {
    run run_sourced "POSITIONAL=(unknown-key); cmd_config"
    [ "$status" -ne 0 ]
}

# ── cmd_open URL generation ───────────────────────────────────────────────────

@test "cmd_open: org account -> org URL" {
    make_config "acme" "org"
    run run_sourced "xdg-open() { echo \"\$1\"; }; export -f xdg-open; cmd_open --org acme"
    [[ "$output" =~ "organizations/acme/settings/installations" ]]
}

@test "cmd_open: user account -> personal URL" {
    make_config "nick" "user"
    run run_sourced "xdg-open() { echo \"\$1\"; }; export -f xdg-open; cmd_open --org nick"
    [[ "$output" =~ "settings/installations" ]]
    [[ ! "$output" =~ "organizations" ]]
}

@test "cmd_open: default profile -> personal URL regardless of ACCOUNT_TYPE" {
    make_config "default" "org"
    run run_sourced "xdg-open() { echo \"\$1\"; }; export -f xdg-open; cmd_open"
    [[ "$output" =~ "settings/installations" ]]
    [[ ! "$output" =~ "organizations" ]]
}

# ── cmd_status ────────────────────────────────────────────────────────────────

@test "cmd_status: shows profile, account type, app id" {
    make_config "acme" "org" "77777"
    run run_sourced "cmd_status --org acme"
    [ "$status" -eq 0 ]
    [[ "$output" =~ "acme" ]]
    [[ "$output" =~ "org" ]]
    [[ "$output" =~ "77777" ]]
}

@test "cmd_status: missing PEM shows missing" {
    make_config "acme" "org" "77777"
    rm "$CONFIG_DIR/private-key-acme.pem"
    run run_sourced "cmd_status --org acme"
    [[ "$output" =~ "missing" ]]
}

@test "cmd_status: present PEM shows exists" {
    make_config "acme" "org" "77777"
    run run_sourced "cmd_status --org acme"
    [[ "$output" =~ "exists" ]]
}
# (marker — not used, just EOF test)
