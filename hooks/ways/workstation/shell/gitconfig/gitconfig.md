---
description: global git configuration — identity, default branch, pull strategy, delta pager, credential helper, global gitignore
vocabulary: gitconfig global identity user.name user.email pull.rebase init.defaultBranch credential helper libsecret keychain delta pager core.pager gitignore push.autoSetupRemote
files: /\.gitconfig$|/\.config/git/config$|/\.gitignore_global$
commands: git\ config\ --global
scope: agent
refire: 0.15
---
<!-- epistemic: convention -->
# Global Git Config

`~/.gitconfig` is the user-level git identity and preferences file. Set once per machine, inherited by every repo unless overridden locally. Most of these are cheap defaults that prevent future papercuts.

## Identity

```bash
git config --global user.name "Full Name"
git config --global user.email "user@example.com"
```

Ask the user for both. If they commit to multiple identities (personal vs work), mention conditional includes (`[includeIf "gitdir:~/work/"]`) — but only set that up when they confirm the split.

## Modern defaults

```bash
git config --global init.defaultBranch main
git config --global pull.rebase true           # linear history on pull
git config --global push.autoSetupRemote true  # no more "--set-upstream" dance
git config --global push.default current
git config --global fetch.prune true           # delete refs for gone branches
git config --global rebase.autostash true      # stash/pop around rebase
```

## Delta pager

[delta](https://github.com/dandavison/delta) is installed as part of the modern CLI set (see `workstation/shell/tools`). Wire it into git:

```bash
git config --global core.pager delta
git config --global interactive.diffFilter "delta --color-only"
git config --global delta.navigate true
git config --global delta.line-numbers true
git config --global merge.conflictstyle diff3
```

## Credential helper (OS-specific)

| OS | Command |
|----|---------|
| macOS | `git config --global credential.helper osxkeychain` |
| Linux (GNOME) | `git config --global credential.helper /usr/lib/git-core/git-credential-libsecret` |
| Linux (generic) | `git config --global credential.helper "cache --timeout=3600"` |
| Windows | `git config --global credential.helper manager` |

For push via SSH, no helper is needed — keys live in ssh-agent (see `workstation/shell/sshagent`).

## Global gitignore

```bash
git config --global core.excludesfile ~/.gitignore_global
```

Sensible starter for `~/.gitignore_global`:

```gitignore
# OS
.DS_Store
Thumbs.db
Desktop.ini

# Editors
*.swp
*.swo
.idea/
.vscode/
*.sublime-workspace

# Python
__pycache__/
*.pyc
.venv/

# Direnv
.envrc
```

Keep project-specific ignores in per-project `.gitignore` — the global one is for noise that's never relevant anywhere.

## Verify

```bash
git config --list --global        # full dump
git config --global --get user.email
```

## See Also

- workstation/shell/tools(workstation) — delta is installed there
- workstation/shell/sshagent(workstation) — SSH-based auth uses the agent, no credential helper needed
- delivery/commits(softwaredev) — commit message conventions run on top of this identity
- workstation/shell(workstation) — parent
