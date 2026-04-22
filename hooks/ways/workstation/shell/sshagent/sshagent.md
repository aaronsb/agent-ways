---
description: persistent SSH agent on a workstation — ssh-add, keychain integration, socket path, launchd or systemd agent, key generation, ~/.ssh/config defaults
vocabulary: ssh-agent ssh-add keychain apple-use-keychain launchd systemd gnome-keyring SSH_AUTH_SOCK ed25519 AddKeysToAgent UseKeychain IdentityFile persistent socket keygen
files: /\.ssh/config$
commands: ssh-add\b|ssh-agent\b|ssh-keygen\b
embed_threshold: 0.40
scope: agent
refire: 0.15
---
<!-- epistemic: convention -->
# Persistent SSH Agent

The agent holds decrypted private keys in memory so `ssh` / `scp` / `git push` don't reprompt every connection. The mistake to avoid: launching a fresh agent in every new shell, ending up with duplicate agents the user's keys aren't loaded into. Solution: one persistent agent per user session, referenced by a well-known socket.

**Scope note:** this is about running and managing an agent on the *local* workstation. For using SSH non-interactively (BatchMode, ConnectTimeout, remote command execution), see `environment/ssh(softwaredev)`.

## macOS — launchd + Keychain

macOS ships an `ssh-agent` that launchd starts on demand. Store passphrases in Keychain so they survive reboot:

```bash
ssh-add --apple-use-keychain ~/.ssh/id_ed25519
```

Then in `~/.ssh/config`:

```ssh-config
Host *
    AddKeysToAgent yes
    UseKeychain yes
    IdentityFile ~/.ssh/id_ed25519
```

No shell-side `eval $(ssh-agent)` needed — launchd handles it.

## Linux — one agent per user session

The common failure is `eval "$(ssh-agent -s)"` in `~/.zshrc`, which spawns a new agent per shell. Fix with a well-known socket the first shell creates and subsequent shells reuse:

```zsh
# in 04-ssh.zsh
if [ -z "$SSH_AUTH_SOCK" ] || ! ssh-add -l >/dev/null 2>&1; then
  export SSH_AUTH_SOCK="${XDG_RUNTIME_DIR:-/tmp}/ssh-agent.sock"
  if ! ssh-add -l >/dev/null 2>&1; then
    rm -f "$SSH_AUTH_SOCK"
    eval "$(ssh-agent -s -a "$SSH_AUTH_SOCK")" >/dev/null
  fi
fi
```

Preferred alternatives when available:
- **systemd user service**: `systemctl --user enable --now ssh-agent` with `SSH_AUTH_SOCK="$XDG_RUNTIME_DIR/ssh-agent.socket"` exported in the shell.
- **gnome-keyring / KDE KWallet**: if the desktop session already runs an agent, respect it — check `ssh-add -l` before spawning another.

And in `~/.ssh/config`:

```ssh-config
Host *
    AddKeysToAgent yes
    IdentityFile ~/.ssh/id_ed25519
```

## Key generation (if the user has no key)

```bash
ssh-keygen -t ed25519 -C "user@machine-name"
```

Defaults are fine. Ed25519 is the recommended algorithm — smaller, faster, and stronger than RSA for all practical use. Ask the user whether they want a passphrase (yes for shared machines, optional for a personal device with FDE).

## File permissions

SSH is strict — wrong permissions and it silently refuses to use a key:

```bash
chmod 700 ~/.ssh
chmod 600 ~/.ssh/id_ed25519 ~/.ssh/config
chmod 644 ~/.ssh/id_ed25519.pub ~/.ssh/known_hosts
```

## Verify

```bash
ssh-add -l                          # should list the loaded key(s)
ssh -T git@github.com 2>&1          # should greet by username, not ask for password
```

If `ssh-add -l` returns "The agent has no identities," the socket is live but keys aren't loaded — run `ssh-add ~/.ssh/id_ed25519` (macOS: `--apple-use-keychain`).

## See Also

- environment/ssh(softwaredev) — using SSH from Claude non-interactively (BatchMode, ConnectTimeout)
- workstation/shell/shellrc(workstation) — the agent setup goes in `04-ssh.zsh`
- workstation/shell(workstation) — parent
