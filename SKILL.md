---
name: claude-deploy
description: Work on and push to GitHub repos from a Claude session using ephemeral GitHub App tokens. Use this skill whenever the user pastes a claude-deploy handover blob.
---

# claude-deploy Skill

Claude can clone, edit, and push to GitHub repos using a token from `claude-deploy` run locally by the user.

## Session Start

The user pastes a handover blob. It looks like this:

```
## claude-deploy session — <owner>/<repo>

Repo: `<owner>/<repo>`
Token (valid until HH:MM TZ):
```
<token>
```

Clone and push:
```sh
git clone https://x-access-token:<token>@github.com/<owner>/<repo>.git /home/claude/<repo>
...
```

Constraints: no SSH, api.github.com blocked, raw.githubusercontent.com blocked, token expires 1hr.
On 401/403: tell user to run `claude-deploy handover` and paste new blob.

---

<context.md content embedded here>
```

On receiving this blob, Claude should:

1. Clone the repo: `git clone https://x-access-token:<token>@github.com/<owner>/<repo>.git /home/claude/<repo>`
2. Set git identity and token remote (commands are in the blob)
3. Read `docs/context/context.md` from the cloned repo for full project context
4. Ask what the user wants to do

## Pushing Changes

```sh
git add -A && git commit -m "<message>" && git push origin main
```

Always use `x-access-token:` prefix — bare token fails on push.

## After Every Commit

Update `docs/context/context.md`:
- Check off completed items under `## Done (this session)`
- Add new `## Open` items if outstanding work exists
- If `Done` exceeds ~20 items: append to `docs/context/archive.md` and clear Done

## Constraints

- `api.github.com` blocked — no GitHub API calls from bash
- `raw.githubusercontent.com` blocked — no curl of raw files
- No SSH — HTTPS token URLs only
- Token expires in 1 hour — user runs `claude-deploy handover` again if expired
- Token scoped to all repos the app is installed on in the org

## Token Expired

If push returns 401/403:
```
Token expired. Run: claude-deploy handover <owner>/<repo>
Then paste the new blob.
```
