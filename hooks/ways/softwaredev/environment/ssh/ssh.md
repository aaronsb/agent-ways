---
description: SSH remote access, key management, secure file transfer, non-interactive authentication
vocabulary: ssh remote key scp rsync bastion jumphost tunnel forwarding batchmode noninteractive
pattern: ssh|remote.?server|remote.?host|sshpass
commands: ^ssh\ |^scp\ |^rsync.*:|\bsshpass\b
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: constraint -->
# SSH Way

## Core Principle

Claude cannot interact with password prompts. SSH commands must be non-interactive or they will hang indefinitely.

## Before SSH'ing

Assume the user has intentional access configured. Quick validation:

```bash
# Check if ssh-agent has keys loaded
ssh-add -l

# Test connection (fails fast, no hang)
ssh -o BatchMode=yes -o ConnectTimeout=5 host exit && echo "OK"
```

If connection fails, ask before troubleshooting - don't assume setup is broken.

## Non-Interactive Flags (Always Use)

```bash
ssh -o BatchMode=yes \
    -o ConnectTimeout=10 \
    -o StrictHostKeyChecking=accept-new \
    user@host "command"
```

| Flag | Purpose |
|------|---------|
| `BatchMode=yes` | Fail immediately if auth needs interaction |
| `ConnectTimeout=10` | Don't hang on unreachable hosts |
| `StrictHostKeyChecking=accept-new` | Accept new hosts, reject changed keys |

## Tiered Scenarios

### Dev / Personal

Keys typically in `~/.ssh/`, ssh-agent running. Straightforward:

```bash
ssh dev-server "cd /app && git pull"
scp file.txt dev-server:/tmp/
```

### Homelab / Internal

Multiple hosts, possibly jump hosts. Use SSH config:

```bash
# ~/.ssh/config
Host homelab-*
    User admin
    IdentityFile ~/.ssh/homelab_key

Host *.internal
    ProxyJump bastion
```

Then simply: `ssh homelab-web "systemctl status nginx"`

### Enterprise / Legacy

May require sshpass for password-based systems:

```bash
# Password from environment (not argument - visible in ps)
export SSHPASS="$PASSWORD"
sshpass -e ssh -o StrictHostKeyChecking=no legacy-server "command"
```

**sshpass cautions:**
- Password visible in environment (but not process list with `-e`)
- Prefer key-based auth when possible
- If password required, get from env var or file, never hardcode

## If Setup Doesn't Exist

When user wants SSH but it's not configured, suggest:

```bash
# Generate key (if none exists)
ssh-keygen -t ed25519 -C "user@machine"

# Copy to remote (one-time, interactive OK for setup)
ssh-copy-id user@host

# Add to agent for session
eval "$(ssh-agent -s)"
ssh-add ~/.ssh/id_ed25519
```

Then retest with `ssh -o BatchMode=yes host exit`.

## File Transfers

```bash
# scp (simple)
scp -o BatchMode=yes local.txt host:/path/

# rsync (better for large/repeated transfers)
rsync -avz -e "ssh -o BatchMode=yes" ./dir/ host:/path/
```

## Command Patterns

```bash
# Single command
ssh host "command"

# Multiple commands
ssh host "cd /app && git pull && npm install"

# Heredoc for complex scripts
ssh host bash <<'EOF'
cd /var/log
grep ERROR app.log | tail -20
EOF
```

## What NOT to Do

- Don't attempt interactive password entry
- Don't hardcode passwords in commands
- Don't skip BatchMode (will hang on unexpected prompts)
- Don't ignore ConnectTimeout (will hang on network issues)
- Don't assume broken setup - ask the user first
