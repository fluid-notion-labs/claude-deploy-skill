# Claude Session Handover

Paste this doc at the start of a new Claude session to get up to speed instantly.

---

## Context

We have a workflow for Claude to push files to GitHub repos using ephemeral GitHub App tokens. Claude runs in a container with no SSH binary and `api.github.com` blocked by the egress proxy — so auth is done locally and the token is pasted in. All git operations use HTTPS token URLs directly against `github.com`.

## Repo

`fluid-notion-labs/claude-deploy-skill`  
This repo contains the skill itself, the `claude-deploy` bash script, and setup docs.

## Auth Flow (every session)

Run locally:
```sh
claude-deploy handover <owner/repo> --org <profile>
```

This generates a token, builds the full session context, and copies it to clipboard. Paste into Claude — done.

Token expires in 1 hour. Re-run `claude-deploy handover` if it expires.

## Local Setup (one time)

```sh
# Install
curl -fsSL https://raw.githubusercontent.com/fluid-notion-labs/claude-deploy-skill/main/claude-deploy \
  -o ~/.local/bin/claude-deploy && chmod +x ~/.local/bin/claude-deploy

# Configure profile
claude-deploy setup --profile fluid-notion-labs
# prompts for App ID and PEM path
```

## Config

- Config dir: `~/.config/claude-deploy/`
- Default config: `~/.config/claude-deploy/config`
- Named profile: `~/.config/claude-deploy/config-<profile>`
- Each config contains `APP_ID` and `PEM_PATH`
- PEM is copied into config dir on setup

## Key Details

- **PEM:** stored in `~/.config/claude-deploy/` per profile
- **GitHub App installed on:** `nhemsley` and `fluid-notion-labs`
- **Container constraints:** no SSH, no persistent state between sessions — `api.github.com` is blocked by egress proxy; use `git clone/push` via HTTPS token URL instead
- **Token is generated locally** via `claude-deploy handover` then pasted to Claude as a full context blob
- **wl-copy** used to auto-copy handover to clipboard on Wayland

## Repo Structure

```
claude-deploy-skill/
├── claude-deploy          # main CLI (setup/token/profiles/status)
├── SKILL.md               # Claude skill instructions
├── README.md
└── docs/
    ├── github-app-setup.md  # one-time GitHub App setup guide
    ├── dogfood.md           # friction notes / fu.garden product spec
    └── session-handover.md  # this file
```

## Current State

- `fluid-notion-labs/claude-deploy-skill` — active repo ✓
- `nhemsley/gh-pages-skill` — old repo, superseded, archive it
- `claude-deploy setup` — run this locally with `--profile fluid-notion-labs` if not done yet
- fu.garden — federated jj hosting concept, sketched in `docs/dogfood.md`
