# claude-deploy — Handover Instructions

## Setup

```sh
# Clone target repo
git clone https://x-access-token:<TOKEN>@github.com/<owner>/<repo>.git /home/claude/<repo>
cd /home/claude/<repo>
git config user.email "claude@anthropic.com" && git config user.name "Claude"
git remote set-url origin https://x-access-token:<TOKEN>@github.com/<owner>/<repo>.git
```

## Committing

Always use `commit.sh` — it auto-refreshes the token before pushing:

```sh
bash /home/claude/claude-deploy-skill/container/scripts/commit.sh /home/claude/<repo> "<message>"
```

Then generate a diff:

```sh
git -C /home/claude/<repo> diff HEAD~1 \
  | npx --yes diff2html-cli -i stdin -o stdout --cs dark -s line \
  | sed 's|<title>.*</title>|<title>diff</title>|; s|<h1>.*</h1>||' \
  > /tmp/last-diff.html
```

## Token auto-refresh

`claude-deploy-sentinel watch` pushes a fresh `tok-<org>-<ts>` file to the sentinel branch 7min before expiry. `commit.sh` reads the latest token from the sentinel worktree before every push — no manual intervention needed while the watcher is running.

## On 401/403

First try getting a fresh token from the sentinel worktree:

```sh
TOKEN=$(bash /home/claude/claude-deploy-skill/container/scripts/get-latest-token.sh)
git -C /home/claude/<repo> remote set-url origin https://x-access-token:$TOKEN@github.com/<owner>/<repo>.git
```

If that fails (watcher not running / all tokens expired), ask user to run `claude-deploy token`.

## Sentinel worktree

```sh
# Queue a sentinel
cd /home/claude/claude-deploy-skill/.git/claude-sentinel-wt
git fetch origin claude-deploy-sentinels -q && git reset --hard origin/claude-deploy-sentinels -q
# ... write sentinel file, git add, commit, push origin HEAD:claude-deploy-sentinels

# Watch for result
bash /home/claude/claude-deploy-skill/container/scripts/wait-for-push.sh \
    /home/claude/claude-deploy-skill 300 --sentinel <name>
```

## Container constraints

- No SSH
- `api.github.com` blocked
- `raw.githubusercontent.com` blocked
- No persistent state between sessions
- Git over HTTPS with `x-access-token:<TOKEN>@github.com` works fine
