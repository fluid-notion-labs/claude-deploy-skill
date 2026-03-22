# Dogfood Notes

Friction points encountered while building and using this skill. These are features waiting to be designed.

## Identity & Account Confusion

- Multiple GitHub accounts (`nhemsley`, `fluid-notion-systems`, `fluid-notion-labs`, `mousechief`) — hard to track which owns what
- Wrong App ID used across sessions — no single source of truth for config
- GitHub App created under personal account, org install requires separate approval flow
- Can't easily tell which repo is the "active" one across accounts

## Repo Sprawl

- `gh-pages-skill` started under `nhemsley`, should live under `fluid-notion-labs`
- `gh repo transfer` failed due to permissions — had to recreate manually
- No signal of which fork/account is canonical — just vibes

## Auth Friction

- Token expires in 1hr — need to re-run `gh-app-token.sh` each session
- App ID easy to get wrong — not surfaced prominently in GitHub UI
- PEM file path varies per machine — no standard location enforced
- SSH not available in Claude container — forced HTTPS + token workflow
- `api.github.com` blocked by container proxy — token must be generated locally

## Session Continuity

- Claude container resets between sessions — no persistent state
- Generated SSH keys are ephemeral — can't reuse across sessions
- Have to re-paste token and repo details each session

## What fu.garden Would Fix

- Single identity across federated instances — no account switching
- Canonical fork surfaced by usage/activity, not by who holds the keys
- Intent expressed through movement — herd finds the active repo naturally
- Config lives with the repo, not scattered across local machines
- No central auth bottleneck — each instance owns its own tokens
