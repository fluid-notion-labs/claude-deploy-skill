# claude-deploy — Session Context

## What this is

`claude-deploy` is a bash CLI that lets Claude push to GitHub repos using ephemeral GitHub App tokens (1hr expiry). Claude runs in a container with no SSH and `api.github.com` blocked — auth happens locally, token is pasted into the session via a handover blob which also embeds this file inline.

## Repo

`fluid-notion-labs/claude-deploy-skill`

## Commands

```sh
claude-deploy setup    [--org <n>]                # configure App ID + ingest PEM
claude-deploy token    [<owner/repo>] [--org <n>] # get ephemeral token → clipboard (infers repo from cwd)
claude-deploy handover [<owner/repo>] [--org <n>] # full session blob → clipboard (embeds context.md)
claude-deploy watch    [--commands]           # poll cwd repo every 5s; --commands runs .claude-deploy-run sentinels
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

## Token refresh mid-session

If a session is already running and the token expires, use `claude-deploy token` (run from inside the target repo) — it infers owner/repo from the git remote and puts a fresh token on the clipboard. Paste it into the session directly; no full handover needed. Use `handover` only when starting a new session or switching repos.

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

## Sentinel run workflow

Claude creates sentinel files on the `claude-deploy-sentinels` orphan branch (never pollutes main). Each sentinel is named `run-<main-ref>-<timestamp>` and tracks a state machine: `new` → `running` → `success`/`failure`.

**Sentinel file format:**
```
status: new
main-ref: a4f3c2d
created: 2026-03-23T17:48:05
capture: assets/images
msg: fetched moon tarot images

./scripts/fetch-images.sh --output assets/images
```

**State machine:**
- `new` — Claude created, pushed to `claude-deploy-sentinels`
- `running` — watch picked it up, script executing
- `success` / `failure` — script exited 0 / non-0; log appended; `result-ref` added if capture landed on main

**watch --commands flow:**
1. `git fetch origin claude-deploy-sentinels` — fast, no checkout
2. `git ls-tree` + `git show` to grep `status: new` — no full pull until needed
3. Pull sentinel branch, mark `running`, commit+push
4. Checkout main, run script, commit captured files to main
5. Checkout sentinel branch, update status, append log, commit+push
6. Return to main

**Creating a sentinel (Claude does this):**
```
status: new
main-ref: $(git rev-parse HEAD)
created: $(date -u +%Y-%m-%dT%H:%M:%S)
capture: path/to/results
msg: commit message for results on main

script body here
```
Push to `claude-deploy-sentinels` branch. Watch picks it up within poll interval.

After every `git push`, Claude generates a diff HTML and presents it inline in the chat using `present_files`. This applies to **any repo** Claude is working in via claude-deploy — it is not a script command, it is Claude's standard operating procedure.

```sh
git diff HEAD~1 | diff2html -i stdin -o stdout --cs dark -s line \
  | sed 's|<title>.*</title>|<title>diff</title>|; s|<h1>.*</h1>||' \
  > /tmp/last-diff.html
# then: present_files ["/tmp/last-diff.html"]
```

Requires `diff2html-cli`: `npm install -g diff2html-cli`

When waiting for the user to push (e.g. after a sentinel run), Claude can block using:

```sh
bash /home/claude/claude-deploy-skill/container/scripts/wait-for-push.sh <repo_dir> [timeout_seconds]
# exits 0 on change, 1 on timeout (default 60s, interval 5s)
# fetches origin, pulls on change, prints elapsed ticks
```

Tests cover: `config_file`, `parse_profile`, `load_config`, `infer_single_org`, `infer_profile`, combined infer flows, `clipboard_copy` error path, `copy_or_print`, `cmd_config`, `cmd_open`, `cmd_status`, `cmd_profiles`.

## Context workflow

- This file lives at `docs/context/context.md` and is embedded inline in every handover blob
- Completed work goes in `## Done (this session)` below
- When Done exceeds ~20 items, archive: append to `docs/context/archive.md`, clear Done
- Update this file as part of every commit that changes behaviour

## Session start

At the start of every session:
1. Run the bootstrap block from the handover blob — clones skill repo to `/home/claude/claude-deploy-skill`
2. Echo status summary in this format (no preamble):

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
- `watch --commands` sentinel spam bug — sentinel commits are landing on main branch instead of staying on `claude-deploy-sentinels`. Root cause not yet confirmed but likely: `_sentinel_run` checks out main to run the script (line 653), and if `s_capture` path exists it commits+pushes to main (line 671) — that part is intentional. But the sentinel status updates (running/success/failure) should only ever touch the sentinel branch. Need to audit whether any sentinel state write is accidentally targeting main. Also: the `— no change` heartbeat line prints every 5s tick unconditionally — should suppress and only print on change or every ~60s.
- `watch` spinner — replace per-tick `— no change` echo with an in-place spinner (`\r` overwrite, frames `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`). Only break to a new line on actual events (new commit, sentinel fired). Keeps terminal clean during long idle periods.
- `queue` command — reads sentinel branch (no checkout, via `git show origin/claude-deploy-sentinels:...`) and prints a table of sentinels with status, created timestamp, and first line of script body. Add `--log <sentinel-name>` flag to dump full log output for a completed sentinel. Useful for Claude to check on queued commands without switching branches.
- **TUI rewrite (Rust/Ratatui)** — replace the bash script with a Rust binary using Ratatui. Everything goes TUI-first with a `--command`/headless mode for scripted use. Binary lives in `container/bin/` compiled for Linux x86_64. Feature parity plan:
  - Feature-grep the current bash script first — extract all commands, flags, config paths, sentinel format, git operations — before writing any Rust
  - **Watch pane** — queue list (scrollable), live log tail of selected sentinel, spinner on running items. Auto-polls sentinel branch via `git fetch` + `git ls-tree`, no checkout. Keys: arrows=select, enter=expand log, q=quit, r=refresh
  - **Org/repo browser** — list configured orgs (from `~/.config/claude-deploy/`), expand to show accessible repos via GitHub App installation. Select repo → opens watch view for that repo. Replaces `profiles`, `status`, `open` commands
  - **Token handover automation** — TUI detects token expiry (tracks 1hr from issue time stored in config), auto-regenerates via `generate_jwt` + `get_install_id` + exchange, pastes new token into a visible panel. No manual `claude-deploy token` needed mid-session. Consider a `--token-server` mode: local HTTP endpoint Claude can hit to get a fresh token without user interaction
  - **Command mode** (`claude-deploy <cmd> [args]`) — all TUI views accessible as non-interactive commands for scripting and backward compat: `queue`, `watch`, `token`, `handover`, `setup`, `profiles`, `status`
  - Config/auth layer stays identical — same PEM paths, same `~/.config/claude-deploy/` layout, same JWT generation logic, just ported to Rust

## Done (this session)

- `token` infers owner/repo from cwd git remote (same as handover)
- Context doc: token refresh mid-session documented; handover vs token usage clarified
- bash completion added (`completions/claude-deploy.bash`); installed automatically by `update` into `~/.local/share/bash-completion/completions/`
- `_list_profile_names` extracted; `profiles --names` added; completion calls binary instead of reimplementing config glob
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
- `diff` command removed — diff is now Claude's post-commit SOP (any repo), not a script command; documented in Post-commit workflow section
- `watch --commands` rebuilt — sentinel branch state machine: orphan `claude-deploy-sentinels` branch, `run-<main-ref>-<ts>` files, `new`→`running`→`success/failure`, captured results committed to main as clean commits, fast grep poll via `git show` without checkout
- `handover` infers owner/repo from git remote origin if run inside a git repo
- `handover` bootstrap block added — clones `<org>/claude-deploy-skill` (falls back to `fluid-notion-labs/claude-deploy-skill`) at session start so `container/scripts/` is immediately available
- File editing primitives researched — no new tool needed; use `str_replace` for unique matches, `sed -i`/`python3` via `bash_tool` for everything else; `create_file` only for >50% file changes; documented in `docs/research/editing.md`
