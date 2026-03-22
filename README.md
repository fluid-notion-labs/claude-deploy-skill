# gh-pages-skill

Claude skill for pushing files to GitHub Pages using ephemeral GitHub App tokens.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/nhemsley/gh-pages-skill/main/gh-app-token.sh \
  -o ~/.local/bin/gh-app-token && chmod +x ~/.local/bin/gh-app-token
```

## Usage

```sh
gh-app-token ~/.config/claude-deploy/private-key.pem <APP_ID> <owner/repo>
```

Paste the token into Claude. Done.

## Setup

See [docs/github-app-setup.md](docs/github-app-setup.md) for one-time GitHub App configuration.

## Links

- [Create a GitHub App](https://github.com/settings/apps/new)
- [Your GitHub Apps](https://github.com/settings/apps)
- [Generate a token](https://github.com/settings/tokens)
- [Cloudflare Workers](https://workers.cloudflare.com)
