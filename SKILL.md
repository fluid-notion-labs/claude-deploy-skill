---
name: gh-pages-deploy
description: Deploy files to a GitHub Pages repo from within a Claude session using a session-scoped SSH deploy key. Use this skill whenever the user wants to publish, update, or push files to GitHub Pages, a GitHub repo, or any git remote — even if they don't say "deploy" explicitly. Triggers on: "push to github", "update my site", "publish this", "put this on github pages", "update the repo", or any request to make Claude-generated content available at a stable URL.
---

# GitHub Pages Deploy Skill

Push files to a GitHub Pages repo from a Claude session using a per-session SSH deploy key. No persistent credentials required — generate a fresh key each session, register it as a repo deploy key, push, revoke when done.

## Workflow

### 1. Generate session keypair

```python
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat, PrivateFormat, NoEncryption
import base64, os

key = Ed25519PrivateKey.generate()
pub = key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
priv = key.private_bytes(Encoding.PEM, PrivateFormat.OpenSSH, NoEncryption())

os.makedirs('/root/.ssh', exist_ok=True)
with open('/root/.ssh/claude_session', 'wb') as f:
    f.write(priv)
os.chmod('/root/.ssh/claude_session', 0o600)

pub_b64 = base64.b64encode(pub).decode()
print(f'ssh-ed25519 {pub_b64} claude-session')
```

### 2. Register deploy key — give user this one-liner

```sh
echo "ssh-ed25519 <PUBKEY> claude-session" | gh api repos/{owner}/{repo}/keys --method POST --input - -f title="claude-session" -F read_only=false
```

Or interactively:
```sh
gh api repos/{owner}/{repo}/keys \
  --method POST \
  -f title="claude-session" \
  -f key="ssh-ed25519 <PUBKEY> claude-session" \
  -F read_only=false
```

User runs this on their local machine (requires `gh` CLI authenticated). They paste back the key ID from the response.

### 3. Configure SSH in container

```bash
cat > /root/.ssh/config << 'EOF'
Host github.com
  IdentityFile /root/.ssh/claude_session
  StrictHostKeyChecking no
EOF
chmod 600 /root/.ssh/config
```

### 4. Clone or init repo

```bash
# Clone
git clone git@github.com:{owner}/{repo}.git /home/claude/repo

# Or if pushing to existing local content:
cd /home/claude/repo
git remote set-url origin git@github.com:{owner}/{repo}.git
```

### 5. Commit and push

```bash
cd /home/claude/repo
git config user.email "claude-session@anthropic"
git config user.name "Claude"
git add -A
git commit -m "{message}"
git push origin {branch}
```

### 6. Revoke deploy key (optional but good practice)

Give user:
```sh
gh api repos/{owner}/{repo}/keys/{key_id} --method DELETE
```

---

## Notes

- Key lives only for the session — container resets wipe it
- `read_only=false` required to push
- GitHub Pages serves from `main` or `gh-pages` branch depending on repo settings
- For `username.github.io` repos, branch is always `main`
- ssh-keygen not available in container — use the Python cryptography approach above
- `gh` CLI not available in container — user runs `gh` commands locally
