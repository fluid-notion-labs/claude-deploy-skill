# bash completion for claude-deploy
# Installed automatically by: claude-deploy update
# Manual install: source this file from ~/.bashrc, or drop into
#   ~/.local/share/bash-completion/completions/claude-deploy

_claude_deploy_complete() {
    local cur prev words cword
    if declare -f _init_completion > /dev/null 2>&1; then
        _init_completion || return
    else
        # fallback if bash-completion framework not loaded
        words=("${COMP_WORDS[@]}")
        cword=$COMP_CWORD
        cur="${words[$cword]}"
        prev="${words[$((cword - 1))]}"
    fi

    local commands="setup token handover watch queue open update config profiles status"

    # top-level: complete command name
    if [[ $cword -eq 1 ]]; then
        COMPREPLY=($(compgen -W "$commands" -- "$cur"))
        return
    fi

    local cmd="${words[1]}"

    case "$cmd" in
        token|handover)
            case "$prev" in
                --org) _claude_deploy_orgs; return ;;
            esac
            # positional 1: owner/repo — no useful completion, but offer --org
            if [[ $cword -eq 2 && "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "--org" -- "$cur"))
            elif [[ $cword -ge 3 ]]; then
                COMPREPLY=($(compgen -W "--org" -- "$cur"))
            fi
            ;;
        setup)
            case "$prev" in
                --org) _claude_deploy_orgs; return ;;
                --pem) COMPREPLY=($(compgen -f -- "$cur")); return ;;
            esac
            COMPREPLY=($(compgen -W "--org --pem" -- "$cur"))
            ;;
        open|status)
            case "$prev" in
                --org) _claude_deploy_orgs; return ;;
            esac
            COMPREPLY=($(compgen -W "--org" -- "$cur"))
            ;;
        watch)
            COMPREPLY=($(compgen -W "--commands" -- "$cur"))
            ;;
        queue)
            case "$prev" in
                --log)
                    # Complete sentinel names from remote branch if possible
                    local sentinels
                    sentinels=$(git ls-tree -r --name-only "origin/claude-deploy-sentinels" 2>/dev/null | grep "^run-" || true)
                    COMPREPLY=($(compgen -W "$sentinels" -- "$cur"))
                    return
                    ;;
            esac
            COMPREPLY=($(compgen -W "--log --all" -- "$cur"))
            ;;
        config)
            if [[ $cword -eq 2 ]]; then
                COMPREPLY=($(compgen -W "auto-update" -- "$cur"))
            elif [[ $cword -eq 3 && "${words[2]}" == "auto-update" ]]; then
                COMPREPLY=($(compgen -W "on off" -- "$cur"))
            fi
            ;;
        update|profiles)
            # no args
            ;;
    esac
}

# Complete --org values by calling the binary (single source of truth)
_claude_deploy_orgs() {
    local orgs
    orgs=$(claude-deploy profiles --names 2>/dev/null)
    COMPREPLY=($(compgen -W "$orgs" -- "$cur"))
}

complete -F _claude_deploy_complete claude-deploy
