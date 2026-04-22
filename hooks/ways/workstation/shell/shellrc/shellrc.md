---
description: modular shell startup configuration — zshrc or bashrc as a thin loader sourcing conf.d includes, XDG base directories, PATH deduplication
vocabulary: zshrc bashrc rcfile conf.d sourced modular loader include XDG_CONFIG_HOME XDG_DATA_HOME XDG_CACHE_HOME PATH typeset profile startup interactive login
files: /\.zshrc$|/\.bashrc$|/\.zshenv$|/\.zprofile$|/\.zsh/conf\.d/|/\.bashrc\.d/
scope: agent
refire: 0.15
---
<!-- epistemic: convention -->
# Shellrc

A monolithic `~/.zshrc` is hard to maintain. A thin rc file that sources individual config fragments from `~/.zsh/conf.d/` (or equivalent for bash) is inspectable, reorderable, and friendly to selective disable-by-rename.

## The loader pattern

```zsh
# ~/.zshrc — loader only
for f in ~/.zsh/conf.d/*.zsh(N); do
  source "$f"
done
```

Lexicographic order by filename is the ordering contract. Prefix with numbers: `00-xdg.zsh`, `01-path.zsh`, `02-env.zsh`. Each file has a one-line header comment stating its purpose.

## Suggested layout

| File | Purpose |
|------|---------|
| `00-xdg.zsh` | XDG base directory variables |
| `01-path.zsh` | PATH composition + dedup |
| `02-env.zsh` | EDITOR, LANG, PAGER, etc. |
| `03-pkg.zsh` | Package manager shell hooks (e.g. `brew shellenv`) |
| `04-ssh.zsh` | SSH agent (see `workstation/shell/sshagent`) |
| `05-go.zsh` | GOPATH, GOBIN |
| `06-node.zsh` | Node / fnm / nvm |
| `07-aliases.zsh` | Aliases for modern CLI tools |
| `08-completions.zsh` | Completion system init |
| `09-omp.zsh` | oh-my-posh init (see `workstation/shell/prompt`) |

## XDG base dirs

```zsh
export XDG_CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"
export XDG_DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
export XDG_CACHE_HOME="${XDG_CACHE_HOME:-$HOME/.cache}"
export XDG_STATE_HOME="${XDG_STATE_HOME:-$HOME/.local/state}"
```

The `${VAR:-default}` pattern respects what the OS or login manager already set.

## PATH composition

```zsh
typeset -U path PATH     # dedupe — zsh-specific
path=(
  $HOME/.local/bin
  $HOME/.cargo/bin
  $GOBIN
  $HOME/.npm-global/bin
  /opt/homebrew/bin /opt/homebrew/sbin
  /usr/local/bin
  $path                  # distro defaults stay last
)
```

Prepend user paths, keep distro defaults tail. `typeset -U path` is the zsh dedup trick — paths stay unique without manual checks.

## Graceful degradation

Every config fragment should guard against missing tools:

```zsh
if command -v zoxide >/dev/null; then
  eval "$(zoxide init zsh)"
fi
```

A missing tool must silently no-op, not raise. This is what lets the same conf.d set work on a freshly bootstrapped machine and a fully-installed one.

## See Also

- workstation/shell/prompt(workstation) — 09-omp.zsh loads oh-my-posh
- workstation/shell/sshagent(workstation) — 04-ssh.zsh loads the agent
- workstation/shell/tools(workstation) — 07-aliases.zsh configures modern CLI tools
- workstation/shell(workstation) — parent
