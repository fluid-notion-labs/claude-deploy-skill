# claude-deploy-skill

Claude skill for pushing files to GitHub Pages using ephemeral GitHub App tokens.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/fluid-notion-labs/claude-deploy-skill/main/gh-app-token.sh \
  -o ~/.local/bin/gh-app-token && chmod +x ~/.local/bin/gh-app-token
```

## Usage

```sh
gh-app-token <app_id> <owner/repo>
```

Token is printed to stdout and copied to clipboard via `wl-copy` if available. Paste into Claude. Done.

## Setup

See [docs/github-app-setup.md](docs/github-app-setup.md) for one-time GitHub App configuration.

## Links

- [Create a GitHub App](https://github.com/settings/apps/new)
- [Your GitHub Apps](https://github.com/settings/apps)
- [fluid-notion-labs org](https://github.com/fluid-notion-labs)
- [Cloudflare Workers](https://workers.cloudflare.com)
