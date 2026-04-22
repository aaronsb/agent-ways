---
description: personal developer shell environment setup on a workstation — interactive prompt, modular shellrc, modern CLI tool ecosystem, global git identity, persistent ssh-agent
vocabulary: workstation zsh bash shell dotfile rcfile PATH XDG prompt oh-my-posh theme bootstrap fresh machine setup personal home config homebrew pacman apt
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: premise -->
# Workstation Shell

Setting up the shell layer of a personal developer machine — the things a user cares about on a fresh install, not project-level config.

## Children

| Concern | Way |
|---------|-----|
| Interactive prompt appearance | `workstation/shell/prompt` |
| Modular shellrc, PATH, XDG | `workstation/shell/shellrc` |
| Modern CLI tools (lsd, bat, fd, rg, fzf, ...) | `workstation/shell/tools` |
| Global git identity and defaults | `workstation/shell/gitconfig` |
| Persistent ssh-agent | `workstation/shell/sshagent` |

## Principles

- **Detect first, install second.** Find the OS and package manager before recommending a command. Homebrew on macOS, pacman on Arch, apt on Debian/Ubuntu, dnf on Fedora. Package names drift across distros — `fd` vs `fd-find`, `bat` vs `batcat`, `rg` vs `ripgrep`.
- **Layer on top, don't clobber.** Distros and Homebrew already set sensible PATH and XDG defaults. Add to them; don't replace them.
- **Graceful degradation.** Every sourced config should `command -v <tool>` before configuring it. A missing tool must silently no-op, not throw.
- **No plugin managers by default.** Plain shell + sourced files beats oh-my-zsh, zinit, antibody for a user who wants to maintain their own setup. If the user asks for one, fine — but don't introduce one uninvited.
- **Ask before installing.** Font family, which CLI replacements, git identity, SSH key type — these are preferences, not defaults. Confirm before running package manager commands.
- **Offer the walkthrough shape.** Interactive setup is a sequence: detect → confirm → install → verify. At each step, report what was found and wait for confirmation before moving on. Don't batch-install the whole stack.

## See Also

- environment/ssh(softwaredev) — using SSH non-interactively from Claude (distinct from local agent setup)
- environment/config(softwaredev) — project-level configuration
- delivery/commits(softwaredev) — git commits consume the identity configured here
