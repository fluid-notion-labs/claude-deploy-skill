# claude-deploy-skill

Claude skill for pushing files to GitHub repos using ephemeral GitHub App tokens.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/fluid-notion-labs/claude-deploy-skill/main/claude-deploy \
  -o ~/.local/bin/claude-deploy && chmod +x ~/.local/bin/claude-deploy
```

> If you have an old `gh-app-token` in `~/.local/bin`, remove it: `rm ~/.local/bin/gh-app-token`

## First Time Setup

```sh
claude-deploy setup --org fluid-notion-labs
```

Prompts for App ID and PEM path. Copies PEM to `~/.config/claude-deploy/` and saves config.

## Every Session

```sh
claude-deploy token <owner/repo> --org fluid-notion-labs
```

Token printed to stdout and copied to clipboard via `wl-copy`. Paste into Claude.

## Commands

```sh
claude-deploy setup    [--org <n>]              # configure app ID and ingest PEM
claude-deploy token    <owner/repo> [--org <n>] # get ephemeral token
claude-deploy profiles                          # list configured orgs
claude-deploy status   [--org <n>]              # show current config
```

## Starting a Claude Session

Tell Claude at the start of each session:

```
App ID: <app_id>
Repo: <owner/repo>
Org: <org>
```

Then run `claude-deploy token` and paste the result.

## Setup

See [docs/github-app-setup.md](docs/github-app-setup.md) for one-time GitHub App configuration.

## Links

- [Create a GitHub App](https://github.com/settings/apps/new)
- [Your GitHub Apps](https://github.com/settings/apps)
- [fluid-notion-labs org](https://github.com/fluid-notion-labs)
- [Cloudflare Workers](https://workers.cloudflare.com)
