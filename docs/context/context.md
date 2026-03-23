# claude-deploy — Session Context

## What this is

`claude-deploy` is a bash CLI that lets Claude push to GitHub repos using ephemeral GitHub App tokens (1hr expiry). Claude runs in a container with no SSH and `api.github.com` blocked — auth happens locally, token is pasted into the session via a handover blob which also embeds this file inline.

## Repo

`fluid-notion-labs/claude-deploy-skill`

## Commands

```sh
claude-deploy setup    [--org <n>]                # configure App ID + ingest PEM
claude-deploy token    <owner/repo> [--org <n>]   # get ephemeral token → clipboard
claude-deploy handover [<owner/repo>] [--org <n>] # full session blob → clipboard (embeds context.md)
claude-deploy watch                                # poll cwd repo every 5s, print new commits
claude-deploy open     [--org <n>]                # xdg-open GitHub App install settings
claude-deploy update                              # self-update from main branch
claude-deploy config   auto-update [on|off]       # toggle auto-update
claude-deploy profiles                            # list configured orgs
claude-deploy status   [--org <n>]                # show config for org
```

## Config layout

```
~/.config/claude-deploy/
  config                  # default org (holds APP_ID, PEM_PATH, ACCOUNT_TYPE, AUTO_UPDATE)
  config-<org>            # named org config (same fields)
  private-key-<org>.pem   # PEM copied in on setup
~/.cache/claude-deploy/
  repo/                   # git clone of claude-deploy-skill (used by update/auto-update)
```

## Container constraints

- No SSH
- `api.github.com` blocked by egress proxy — all `gh api` calls must run locally
- `raw.githubusercontent.com` blocked
- No persistent state between sessions
- Git over HTTPS with `x-access-token:<TOKEN>@github.com` works fine

## Script internals

Key helpers (all require `load_config` to have run first):

- `generate_jwt()` — mints a JWT via `uvx`/pyjwt; needs `$PEM_PATH`, `$APP_ID`
- `get_install_id <repo>` — looks up GitHub App installation ID; needs `$JWT`
- `infer_single_org` — sets `$PROFILE` if exactly one org is configured
- `infer_profile <repo>` — sets `$PROFILE` from repo owner or single-org fallback
- `clipboard_copy <content>` — tries wl-copy → xclip → xsel, errors if none found
- `copy_or_print <content> <label> [--always-print]` — clipboard or stdout fallback
- `parse_profile "$@"` — parses `--org` flag; sets globals `$PROFILE` and `$POSITIONAL[]`
- `_do_update [--quiet] [--dest <path>]` — shared git+copy+syntax-check for update/auto-update

## Update / auto-update

- `_do_update` clones/pulls `claude-deploy-skill` into `~/.cache/claude-deploy/repo`
- Auto-update re-execs via `exec "$DEST" "$COMMAND" "$@" --no-update` — `--no-update` stripped before dispatch
- Skipped for `update`, `watch`, `config` commands
- Syntax-check before binary replace prevents corrupt installs

## Testing

```sh
bats tests/unit.bats   # 49 tests — pure unit, no network/clipboard required
```

Tests cover: `config_file`, `parse_profile`, `load_config`, `infer_single_org`, `infer_profile`, combined infer flows, `clipboard_copy` error path, `copy_or_print`, `cmd_config`, `cmd_open`, `cmd_status`, `cmd_profiles`.

## Context workflow

- This file lives at `docs/context/context.md` and is embedded inline in every handover blob
- Completed work goes in `## Done (this session)` below
- When Done exceeds ~20 items, archive: append to `docs/context/archive.md`, clear Done
- Update this file as part of every commit that changes behaviour

## Session start

At the start of every session, echo a status summary in this format (no preamble):

```
Repo: fluid-notion-labs/claude-deploy-skill

Recent:
- <last ~8 done items from Done list>

Open:
- <items from Open>

Next up:
- <inferred or explicit next tasks>
```

## Open

- `parse_profile` globals — `$PROFILE` and `$POSITIONAL[]` are intentionally global (infer functions mutate PROFILE post-parse); documented with comment in script

## Done (this session)

- `--profile` alias removed; `--org` is the only flag
- `watch` command added, uses cwd by default
- Extracted: `generate_jwt()`, `get_install_id()`, `infer_single_org()`, `copy_or_print()`
- `clipboard_copy()` extracted — tries wl-copy → xclip → xsel with clear error
- `_do_update()` extracted — deduplicates auto-update block and cmd_update
- `cmd_open` personal account bug fixed — `ACCOUNT_TYPE=org|user` stored in config at setup
- `cmd_config --org` behaviour documented — intentionally global
- `date -d` made portable — GNU + BSD (`date -v+1H`) fallback
- XDG cache path — update repo now at `~/.cache/claude-deploy/repo`
- `parse_profile` globals documented with rationale comment
- `set -e` bug fixed in `copy_or_print` (`[[ ]] && echo` → `|| true`)
- `CLAUDE_DEPLOY_TEST=1` guard added — script sourceable for unit tests
- bats unit test suite added — 49 tests, all passing
- Handover blob now embeds `docs/context/context.md` inline via git clone
- Context workflow codified; docs restructured to `docs/context/`
- Session start echo added — Claude now outputs recent/open/next summary at handover start
