# claude-deploy — Session Context

## What this is

`claude-deploy` is a bash CLI that lets Claude push to GitHub repos using ephemeral GitHub App tokens (1hr expiry). Claude runs in a container with no SSH and `api.github.com` blocked — auth happens locally, token is pasted into the session via a handover blob which also embeds this file inline.

## Repo

`fluid-notion-labs/claude-deploy-skill`

## Commands

```sh
claude-deploy setup    [--org <n>]                  # configure App ID + ingest PEM
claude-deploy token    [<owner/repo>] [--org <n>]   # get ephemeral token → clipboard (infers repo from cwd)
claude-deploy handover [<owner/repo>] [--org <n>]   # full session blob → clipboard (embeds context.md)
claude-deploy watch    [--commands]                  # poll cwd repo every 5s; --commands runs sentinels
claude-deploy queue    [--all] [--log <name>]        # list sentinels; --all includes completed; --log dumps execution log
claude-deploy open     [--org <n>]                   # xdg-open GitHub App install settings
claude-deploy update                                 # self-update from main branch
claude-deploy config   auto-update [on|off]          # toggle auto-update
claude-deploy profiles                               # list configured profiles
claude-deploy status   [--org <n>]                   # show current config
```

## Config layout

```
~/.config/claude-deploy/
  config                  # default org (holds APP_ID, PEM_PATH, ACCOUNT_TYPE, AUTO_UPDATE)
  config-<org>            # named org config
  private-key-<org>.pem   # PEM copied in on setup
~/.cache/claude-deploy/
  repo/                   # git clone of claude-deploy-skill (used by update/auto-update)
```

## Token refresh mid-session

Use `claude-deploy token` (run inside the target repo) — infers owner/repo from git remote, puts fresh token on clipboard. Use `handover` only when starting a new session or switching repos.

## Container constraints

- No SSH
- `api.github.com` blocked by egress proxy
- `raw.githubusercontent.com` blocked
- No persistent state between sessions
- Git over HTTPS with `x-access-token:<TOKEN>@github.com` works fine
- `xxd` not available — use `od -An -tx1 | tr -d ' \n'` for hex encoding

## Script internals

Key helpers (all require `load_config` to have run first):

- `generate_jwt()` — mints a JWT via `uvx`/pyjwt
- `get_install_id <repo>` — looks up GitHub App installation ID
- `infer_single_org` — sets `$PROFILE` if exactly one org configured
- `infer_profile <repo>` — sets `$PROFILE` from repo owner or single-org fallback
- `clipboard_copy <content>` — tries wl-copy → xclip → xsel
- `copy_or_print <content> <label> [--always-print]` — clipboard or stdout fallback
- `parse_profile "$@"` — parses `--org` flag; sets globals `$PROFILE` and `$POSITIONAL[]`
- `_do_update [--quiet] [--dest <path>]` — shared git+copy+syntax-check for update/auto-update
- `_sentinel_new_name [repo]` — generates `run-<ref8>-<ts>-<rand4>` filename

## Sentinel system

Sentinels are commands Claude queues for the user's machine to run. They live on an orphan branch `claude-deploy-sentinels` — completely separate from main history.

### Worktree architecture (no branch switching)

`claude-deploy watch --commands` sets up a **git worktree** at `.git/claude-sentinel-wt/` pointing to the sentinel branch. All sentinel reads/writes happen in that directory. The main working tree **never changes branch**. Safe under Ctrl-C.

The Rust binary `claude-deploy-sentinel` (`sentinel/` crate) handles sentinel operations. It uses a `Backend` trait with `GitShellBackend` as the concrete impl. Future: `GixBackend`, `JjBackend` (jj workspaces — same concept, native).

### Sentinel filename

`run-<main-ref-8>-<YYYYMMDDTHHmmss>-<rand4>`  
e.g. `run-a4f3c2d1-20260327T141200-3f9a`

Random suffix prevents collisions. `_sentinel_new_name` generates it in bash; `sentinel::new_name()` in Rust.

### Sentinel file format

```
status: new
main-ref: a4f3c2d1
created: 2026-03-27T14:12:00
worker: hostname-pid          # set during claim
ran: 2026-03-27T14:12:05      # set when running starts
completed: 2026-03-27T14:12:10
result-ref: b2c19fa4          # SHA on main if capture landed
capture: path/to/results      # optional — dir to commit to main after run
msg: commit message for captured results

script body here
# --- log ---
# execution output (prefixed with #)
```

### State machine

`new` → `claiming` → `running` → `success` / `failure` / `abandoned`

- `claiming` — optimistic lock: worker writes `worker: hostname-pid`, pushes. Non-ff push = lost race.
- `abandoned` — watcher marks sentinels stuck in `running`/`claiming` beyond timeout (default 10min).

### Creating a sentinel (Claude does this)

```sh
# In the repo, on the sentinel branch (via worktree or checkout):
ref=$(git rev-parse --short=8 HEAD)
ts=$(date -u '+%Y%m%dT%H%M%S')
rand=$(head -c 2 /dev/urandom | od -An -tx1 | tr -d ' \n' | head -c 4)
name="run-${ref}-${ts}-${rand}"

cat > "$name" << 'SENTINEL'
status: new
main-ref: <ref>
created: <ts>
msg: what this does

script body here
SENTINEL

git add "$name" && git commit -m "sentinel: new $name" && git push origin claude-deploy-sentinels
```

### Polling for results (Claude does this)

```sh
bash /home/claude/claude-deploy-skill/container/scripts/wait-for-push.sh \
    /home/claude/claude-deploy-skill 300 \
    --sentinel <sentinel-name>
# streams status every 5s, prints log on completion
# exits 0 on success, 1 on failure/timeout
```

### `claude-deploy-sentinel` Rust binary

Located at `sentinel/` in the repo. Commands:
- `claude-deploy-sentinel queue [--all] [--log <name>]`
- `claude-deploy-sentinel watch [--commands] [--interval <s>]`
- `claude-deploy-sentinel create <script> [--capture <path>] [--msg <msg>]`
- `claude-deploy-sentinel reap [--timeout <s>]`
- `claude-deploy-sentinel prune [--dry-run] [--keep-failed <days>] ...`

Binary captured to `bin/claude-deploy-sentinel` on main after each successful build sentinel.

## Post-commit workflow

After every `git push`, Claude generates a diff HTML and presents it inline:

```sh
git diff HEAD~1 | npx --yes diff2html-cli -i stdin -o stdout --cs dark -s line \
  | sed 's|<title>.*</title>|<title>diff</title>|; s|<h1>.*</h1>||' \
  > /tmp/last-diff.html
# then: present_files ["/tmp/last-diff.html"]
```

## Context workflow

- This file lives at `docs/context/context.md` and is embedded inline in every handover blob
- Completed work goes in `## Done (this session)` below
- When Done exceeds ~20 items, archive: append to `docs/context/archive.md`, clear Done
- Update this file as part of every commit that changes behaviour

## Session start

At the start of every session:
1. Run the bootstrap block from the handover blob
2. Echo status summary:

```
Repo: fluid-notion-labs/claude-deploy-skill

Recent:
- <last ~8 done items>

Open:
- <open items>

Next up:
- <inferred next tasks>
```

## Open

- `watch` spinner — replace per-tick output with in-place spinner (`\r`, frames `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`)
- `prune` delete not yet implemented — `Backend` needs `delete_sentinels(&[names])` method
- sentinel `main-ref` checkout semantics — currently watcher pulls latest main and runs there; option to checkout exact `main-ref` for reproducibility (deferred, needs design)
- `parse_profile` globals — `$PROFILE` and `$POSITIONAL[]` intentionally global; documented

## Done (this session)

- `claude-deploy-sentinel` Rust binary working end-to-end — watch, claim, run, log, worktree
- worktree detached HEAD detection + repair on startup
- `git worktree prune` before add — fixes stale registration errors
- reads always via `origin/` ref (never stale disk) — fixes watch seeing already-completed sentinels as new
- claim pulls worktree first + re-checks status before claiming
- binary name fixed to `claude-deploy-sentinel` throughout
- sentinel crate `.gitignore` added (target/, Cargo.lock)
- `wait-for-push.sh --sentinel` mode proven working

- bash completion: detects if bash-completion installed, warns with steps if not
- sentinel hardening: optimistic claim (`status: claiming` + `worker` field), abandoned reaper, `_sentinel_branch_ensure` fixes, `_sentinel_set_field` corruption fix (python3 insert), main-branch safety guard, capture push safety
- sentinel unique filenames: `run-<ref8>-<ts>-<rand4>`, `_sentinel_new_name` helper, `od` instead of `xxd`
- `queue` command (bash): sentinel table with status icons, `--all`, `--log`, completion
- `wait-for-push.sh`: `--sentinel <name>` mode streams status + prints log on completion; `--branch` flag
- `claude-deploy-sentinel` Rust crate scaffolded: `Backend` trait, `GitShellBackend`, worktree-based sentinel ops (no branch switching — `.git/claude-sentinel-wt/`), `queue`/`watch`/`create`/`reap`/`prune` commands
- binary name fixed to `claude-deploy-sentinel`
- merged `sentinel-hardening` → `main`; working on `sentinel-rust` branch
