---
description: modern CLI tool ecosystem on a workstation — installing and aliasing replacements like lsd, bat, fd, ripgrep, fzf, zoxide, delta, tldr, jq, gh via system package manager
vocabulary: lsd eza bat fd ripgrep rg fzf zoxide delta tldr jq gh batcat fdfind alias modern replacement cli homebrew brew pacman apt dnf batman MANPAGER
scope: agent
refire: 0.15
---
<!-- epistemic: convention -->
# Modern CLI Tools

Rust-era replacements for the POSIX basics — faster, colored, glyph-aware, with defaults that match how people actually use them. Install via the system package manager; alias where the user wants the old name.

## The core set

| Tool | Replaces | Alias |
|------|----------|-------|
| **lsd** or **eza** | `ls` | `ls`, `ll`, `la`, `lt` (tree) |
| **bat** | `cat` (+ MANPAGER) | `cat` (optional — some prefer to keep cat unchanged) |
| **fd** | `find` | leave as `fd` |
| **ripgrep** (`rg`) | `grep -r` | leave as `rg` |
| **fzf** | — | ctrl-r, ctrl-t, alt-c via shell integration |
| **zoxide** | `cd` (smart) | `cd` (via init) |
| **delta** | git diff pager | configured in gitconfig |
| **tldr** | man (simplified) | leave as `tldr` |
| **jq** | JSON processing | leave as `jq` |
| **gh** | GitHub CLI | leave as `gh`, then `gh auth login` |

Ask before aliasing over `ls`/`cat`/`cd` — some users want to keep originals untouched and call the new tool by its own name.

## lsd vs eza

Both are active. **eza** is the maintained fork of `exa`; **lsd** has more glyph-heavy defaults. Ask which — don't default silently.

## Package name drift

| Tool | brew | pacman | apt | dnf |
|------|------|--------|-----|-----|
| fd | `fd` | `fd` | `fd-find` (binary is `fdfind`) | `fd-find` |
| bat | `bat` | `bat` | `bat` (binary is `batcat`) | `bat` |
| ripgrep | `ripgrep` | `ripgrep` | `ripgrep` | `ripgrep` |
| lsd | `lsd` | `lsd` | `lsd` | `lsd` |
| zoxide | `zoxide` | `zoxide` | `zoxide` | `zoxide` |

On Debian/Ubuntu, `fd` and `bat` install as `fdfind` and `batcat` to avoid name collisions. Create a symlink or alias to the expected name in the conf.d include.

## Configuration notes

- **bat**: set `BAT_THEME` (ask which — Catppuccin Mocha, Dracula, OneHalfDark, TwoDark are common) and wire `MANPAGER`:
  ```zsh
  export BAT_THEME="Catppuccin Mocha"
  export MANPAGER="sh -c 'col -bx | bat -l man -p'"
  batman() { MANPAGER="$MANPAGER" man "$@"; }
  ```
- **zoxide**: `eval "$(zoxide init zsh)"` — overrides `cd` with smart-jumping behavior.
- **fzf**: shell-integration script path differs by installer (`$(brew --prefix)/opt/fzf/shell/` on macOS, `/usr/share/fzf/` on Linux). Source the completion and key-binding scripts from the conf.d include.
- **gh**: after install, prompt the user to run `gh auth login` — agent shouldn't try to drive the interactive OAuth flow.

## Bat extras

If available (`bat-extras` on brew, `bat-extras` AUR package, manual on apt): `batgrep`, `batdiff`, `batman`, `batwatch`, `batpipe`. Mention them and ask before installing.

## Sixel / image support

`libsixel`/`img2sixel` brings inline terminal images, but only renders in sixel-capable terminals (wezterm, kitty with icat, foot, xterm with sixel, mlterm). Check terminal capability first — no point installing if the emulator can't render.

## See Also

- workstation/shell/gitconfig(workstation) — delta is configured there, not here
- workstation/shell/shellrc(workstation) — aliases and MANPAGER live in sourced includes
- workstation/shell(workstation) — parent
