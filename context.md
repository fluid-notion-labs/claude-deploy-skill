# claude-deploy — Session Context

## What this is

`claude-deploy` is a bash CLI that lets Claude push to GitHub repos using ephemeral GitHub App tokens (1hr expiry). Claude runs in a container with no SSH and `api.github.com` blocked — so auth happens locally, and the token is pasted into the session via a handover blob.

## Repo

`fluid-notion-labs/claude-deploy-skill`

## Commands

```sh
claude-deploy setup    [--org <n>]                # configure App ID + ingest PEM
claude-deploy token    <owner/repo> [--org <n>]   # get ephemeral token → clipboard
claude-deploy handover [<owner/repo>] [--org <n>] # full session blob → clipboard
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
  config                  # default org (also holds AUTO_UPDATE global)
  config-<org>            # named org config
  private-key-<org>.pem   # PEM copied in on setup
  repo/                   # git clone of claude-deploy-skill (used by update/auto-update)
```

Each config file contains `APP_ID` and `PEM_PATH`.

## Session start (every session)

Run locally:
```sh
claude-deploy handover <owner/repo> --org <org>
```
Paste the blob into Claude. Token expires in 1hr — re-run if it expires.

## Container constraints

- No SSH
- `api.github.com` blocked by egress proxy — all `gh api` calls must run locally
- `raw.githubusercontent.com` blocked
- No persistent state between sessions
- Git over HTTPS with `x-access-token:<TOKEN>@github.com` works fine

## Script internals

Key helpers (all require `load_config` to have run first):

- `generate_jwt()` — mints a JWT via `uvx`/pyjwt; needs `$PEM_PATH`, `$APP_ID`
- `get_install_id <repo>` — looks up GitHub App installation ID for a repo; needs `$JWT`
- `infer_single_org` — sets `$PROFILE` if exactly one org is configured
- `infer_profile <repo>` — sets `$PROFILE` from repo owner or single-org fallback
- `copy_or_print <content> <label>` — pipes to `wl-copy` or prints to stdout
- `parse_profile "$@"` — parses `--org` flag; sets globals `$PROFILE` and `$POSITIONAL[]`

## Update / auto-update

The update system has historically been tricky. Key details from the commit history:

- `cmd_update` and the auto-update block at the top both clone/pull `claude-deploy-skill` into `~/.config/claude-deploy/repo`, copy the script, syntax-check, then replace the binary.
- Auto-update re-execs via `exec "$DEST" "$COMMAND" "$@" --no-update` to avoid a loop — `--no-update` is stripped from args before dispatch.
- Auto-update is skipped for `update`, `watch`, and `config` commands.
- Several fixes were needed: `--no-update` loop prevention (`c97e73b`), `command -v` for install path resolution (`2834250`), syntax-check before replace (`c5c06ac`), git-based pull replacing raw curl (`2bb8963`).
- The raw.githubusercontent.com curl approach was abandoned — CDN caching caused stale updates and the domain is blocked in the Claude container anyway.
- Treat this subsystem with care — the re-exec + flag-stripping interaction is subtle.

## Recent refactors (this session)

- `--profile` alias removed; `--org` is the only flag (`86a608c`)
- `watch` command added, uses cwd by default (`ebfb63e`, `b3183ec`)
- `generate_jwt()` extracted — was duplicated in `token` and `handover` (`ad44e2c`)
- `get_install_id()` extracted — was duplicated in `token` and `handover` (`d447348`)
- `infer_single_org()` extracted — was duplicated in `infer_profile` and `handover` (`033ff41`)
- `copy_or_print()` extracted — was duplicated in `token` and `handover` (`25b6be3`)

## Remaining refactors (not yet done)

- `_do_update()` helper — `cmd_update` and the auto-update block share git+copy logic; skipped this session due to the subtlety of the re-exec flow (see above)
- `parse_profile` globals — `$PROFILE` and `$POSITIONAL[]` are set as globals; fragile but workable at this script size
- `cmd_config` always writes to `config_file "default"` regardless of `--org`; intentional (AUTO_UPDATE is global) but undocumented
- `cmd_open` uses `$PROFILE` as org name for the URL — breaks if default profile is used for a personal account
- `date -d` is GNU-only; not cross-platform
- Auto-update repo clone lives in `~/.config/` — `~/.cache/claude-deploy/repo` would be more XDG-correct
