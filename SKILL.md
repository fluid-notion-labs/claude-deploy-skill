---
name: claude-deploy
description: Work on and push to GitHub repos from a Claude session using ephemeral GitHub App tokens. Use this skill whenever the user pastes a claude-deploy handover blob, wants to clone a repo, edit files, and push changes back to GitHub.
---

# claude-deploy Skill

Claude can clone, edit, and push to GitHub repos using a token from the `claude-deploy` tool run locally by the user.

## Session Start

When the user starts a session, they paste a handover blob like this:

```
## claude-deploy session — <owner>/<repo>
Repo: `<owner>/<repo>`
Token (valid until HH:MM TZ):
<token>
Clone and push:
  git clone https://x-access-token:<token>@github.com/<owner>/<repo>.git
  git remote set-url origin https://x-access-token:<token>@github.com/<owner>/<repo>.git
  git push origin main
```

On receiving this, Claude should:

1. Note the org/owner and token
2. Ask the user which repo they want to work on (the handover repo, or another in the same org)
3. Clone it: `git clone https://x-access-token:<token>@github.com/<owner>/<repo>.git /home/claude/<repo>`
4. Confirm what's in the repo and ask what they'd like to do

## Pushing Changes

Always use `x-access-token:` prefix — bare token fails on push:

```sh
git config user.email "claude@anthropic.com"
git config user.name "Claude"
git remote set-url origin https://x-access-token:<token>@github.com/<owner>/<repo>.git
git add -A
git commit -m "<message>"
git push origin main
```

## Constraints

- `api.github.com` is blocked — no GitHub API calls from bash
- `raw.githubusercontent.com` is blocked — no curl of raw files
- No SSH — HTTPS token URLs only
- Token expires in 1 hour — user runs `claude-deploy handover` again if it expires
- Token is scoped to all repos the app is installed on in the org

## Token Expired

If a push returns 403/401, tell the user:
```
Token expired. Run: claude-deploy handover <owner>/<repo>
Then paste the new handover here.
```
