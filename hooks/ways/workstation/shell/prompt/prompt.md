---
description: interactive shell prompt appearance — nerd fonts, oh-my-posh, powerline themes, PS1 customization
vocabulary: oh-my-posh ohmyposh omp starship powerline nerd font MesloLGS FiraCode glyph icon prompt theme PS1 PROMPT montys jandedobbeleer
commands: oh-my-posh\b
files: \.omp\.(json|yaml|toml)$
scope: agent
refire: 0.15
---
<!-- epistemic: convention -->
# Shell Prompt

Glyph-rich prompts (oh-my-posh, starship, powerlevel10k) render branch, git status, and system icons from a **nerd font**. Pointing the terminal at a plain font is why a freshly installed theme shows boxes instead of icons.

## Two separate things

1. **The font.** Installed at the OS level, then *selected* inside the terminal emulator's settings (alacritty, kitty, wezterm, iTerm2, GNOME Terminal, Windows Terminal). The shell cannot set this — only the emulator can.
2. **The prompt engine.** Runs in the shell, emits escape sequences that reference glyphs by codepoint.

If either is wrong, the result is boxes. Check both before blaming the theme.

## Installing nerd fonts

| Package manager | Example: MesloLGS NF |
|-----------------|----------------------|
| brew (macOS) | `brew install --cask font-meslo-lg-nerd-font` |
| pacman (Arch) | `pacman -S ttf-meslo-nerd` |
| apt (Debian/Ubuntu) | no first-class package — use `getnf` or install from nerdfonts.com |
| dnf (Fedora) | `dnf install meslo-lg-nerd-fonts` (or `getnf`) |

Common picks: **MesloLGS NF**, **FiraCode NF**, **JetBrainsMono NF**, **Hack NF**. Ask which.

## oh-my-posh

```bash
# install
brew install jandedobbeleer/oh-my-posh/oh-my-posh          # macOS
curl -s https://ohmyposh.dev/install.sh | bash -s          # Linux

# wire into zsh — belongs in a sourced include, not raw in ~/.zshrc
eval "$(oh-my-posh init zsh --config ~/.config/oh-my-posh/themes/montys.omp.json)"
```

Theme files ship with the binary at `$(brew --prefix oh-my-posh)/themes/` or `/usr/local/share/oh-my-posh/themes/`. If a named theme (e.g. `montys`) isn't in the bundle, fetch from `github.com/JanDeDobbeleer/oh-my-posh` — ask before grabbing.

## Remind the user

After font install, the terminal emulator must be restarted and its font setting changed. This is a GUI click the shell can't do — tell the user what to look for ("Preferences → Profile → Font") and which font name to pick (the Nerd Font variant, not the plain one).

## See Also

- workstation/shell/shellrc(workstation) — the omp init line lives in a sourced include
- workstation/shell(workstation) — parent
