---
name: ways-update
description: Update the agent-ways installation to the latest version — preview the update with `ways manage update --what-if` (versions, release notes, the commits, local changes, the actions, dev links), read it back to the user, then apply it with `ways manage update --commit`, which pulls the app source in $XDG_DATA, refreshes the binaries and corpus, and reprojects ~/.claude. Use when the user wants to update agent-ways, pull the latest ways/hooks, or refresh their ways framework install. Not for editing or authoring individual ways (that is the ways skill) or upgrading project dependencies.
allowed-tools: Bash, Read
---

# ways-update: Update the agent-ways install

In 1.0, `~/.claude` is a thin **projection** of an XDG application whose source
lives in `$XDG_DATA_HOME/agent-ways` (ADR-142). Updating means refreshing *that*
checkout, rebuilding what depends on it, and reprojecting — not touching `~/.claude`
directly. `ways manage update` does all of it, and reports before it changes
anything (ADR-511).

## Steps

### 1. Show the user what would change

```bash
ways manage update --what-if
```

This fetches and changes nothing else; `--what-if` reports even where `install.manage_mode: commit` makes the bare command apply. It prints each component's version
installed and at the target, the release notes in between, the commits grouped by
area, local changes in the app dir, the actions `--commit` would take, and any
binary links that point into a dev checkout. Summarise it for the user. Three
findings stop here for the user's decision:

- **Local changes** in the app dir. The pull stashes and restores them, and a
  conflict leaves them in the stash. Offer to commit them on a fork or stash them
  by hand first. Do not discard them.
- **The checkout has diverged** from its upstream. The fast-forward would stop.
  The user decides whether to push, rebase or merge. A checkout that is only
  *ahead* of its upstream is reported as ahead: the pull changes nothing there.
- **Development setup.** A `make dev-install` is active, and the update would
  point its links back at the app. The user runs `make dev-install` in the
  checkout afterwards to return to the dev build.

### 2. Apply it

```bash
ways manage update --commit
```

It pulls with `--ff-only`, refreshes the binaries (a pre-built release first, a
source build as the fallback), regenerates the corpus, reprojects every target and
checks the judge's keys. `--verbose` shows each command's raw output.

**A fork** (origin is the fork, `upstream` tracks agent-ways): the what-if
compares the checkout with the fork's own `origin/main`, so it shows none of the
upstream changes. Preview those with git, show them to the user, then merge
upstream by hand and apply:

```bash
APP="${XDG_DATA_HOME:-$HOME/.local/share}/agent-ways"
git -C "$APP" fetch upstream
git -C "$APP" log --oneline HEAD..upstream/main                      # what the merge brings in
git -C "$APP" merge upstream/main                                    # resolve conflicts in custom ways
ways manage update --commit
```

After the merge the checkout is ahead of `origin/main`, which the what-if reports
as ahead; the pull then changes nothing and the rest of the update runs.

### 3. Tell the user to restart

A running session keeps the old hooks and ways in memory. End with: **"Restart
Claude Code for the update to take effect."**

## Verify (optional)

```bash
ways --version          # the refreshed binary is on PATH
ways status             # binary / model / corpus / project detection
```

## Notes

- With no app source at `$XDG_DATA_HOME/agent-ways`, the command says so; run the
  installer one-liner instead. A pre-1.0 in-place clone needs migrating:
  https://github.com/aaronsb/agent-ways/blob/ways-v1.8.3/docs/migration-1.0.md
- See `docs/development.md` for the install-vs-dev-checkout distinction.
