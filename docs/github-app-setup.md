# GitHub App Setup for Claude Deploy

One-time setup to allow Claude to push to GitHub repos using ephemeral 1hr tokens. No long-lived credentials.

## 1. Create the GitHub App

Go to: [github.com/settings/apps/new](https://github.com/settings/apps/new)

Fill in:
- **GitHub App name**: `claude-deploy` (or anything)
- **Homepage URL**: `https://github.com` (placeholder is fine)
- **Webhook**: uncheck **Active**

Under **Permissions → Repository permissions**:
- **Contents**: Read & write

Under **Where can this GitHub App be installed**:
- `Only on this account` for personal use
- `Any account` if you want to install on orgs later

Click **Create GitHub App**.

Note your **App ID** at the top of the App page.

## 2. Generate a Private Key

On the App page → scroll to **Private keys** → [Generate a private key](https://github.com/settings/apps).

A `.pem` file downloads. Store it safely:
```sh
mkdir -p ~/.config/claude-deploy
mv ~/Downloads/claude-deploy*.pem ~/.config/claude-deploy/private-key.pem
chmod 600 ~/.config/claude-deploy/private-key.pem
```

## 3. Install the App on a Repo

On the App page: **Install App** → your account → **Only select repositories** → pick the target repo → **Install**.

Repeat for each repo you want Claude to push to.

## 4. Using with a GitHub Organization

Organizations require explicit App installation and have extra permission layers.

### Install on the org

From the App page: **Install App** → select the org (not your personal account).

If the org doesn't appear:
- You must be an **owner** of the org — check `https://github.com/orgs/<org>/people`
- The org may require third-party app approval — check `https://github.com/organizations/<org>/settings/oauth_application_policy`

### Create repos under the org

```sh
gh repo create <org>/<repo> --public
```

### Transferring existing repos

`gh repo transfer` requires admin on both source and target. If it fails, recreate:
```sh
gh repo create <org>/<repo> --public
# then push fresh
```

### Multiple accounts

Avoid multiple GitHub accounts where possible — auth switching is painful. Use orgs under your main account instead. Contributor accounts are fine for collaboration but own everything under one login.

## 5. Get a Token (per session)

```sh
gh-app-token ~/.config/claude-deploy/private-key.pem <APP_ID> <owner/repo>
```

Paste the token into Claude. It expires in 1 hour — the script prints the expiry time.

Claude pushes via:
```
https://x-access-token:<TOKEN>@github.com/<owner>/<repo>.git
```

## 6. Files

- [`gh-app-token.sh`](../gh-app-token.sh) — generates the token (requires `uvx`, `gh`, `jq`)
- [`SKILL.md`](../SKILL.md) — Claude skill that uses the token to push

## Troubleshooting

| Error | Cause | Fix |
|-------|-------|-----|
| `404 Not Found` on installation lookup | App not installed on repo | Steps 3/4 above |
| `422 Validation Failed` on deploy key | Malformed public key format | Use `Encoding.OpenSSH` not `Encoding.Raw` |
| `403 Forbidden` from proxy | Container can't reach `api.github.com` | Run token script locally, paste token |
| JWT errors | Clock skew or wrong App ID | Check App ID, `iat` uses `now - 60` buffer |
| Org not visible in Install App | Not an org owner | Check [org membership](https://github.com/orgs/) |
| Transfer failed | Insufficient admin rights | Recreate repo in target org instead |
