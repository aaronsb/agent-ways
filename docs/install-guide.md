# Installation Guide

This guide covers installing agent-ways when the path isn't a straight line — existing files, custom config, previous installs, or forks you want to keep in sync.

If you're starting fresh with no `~/.claude/` directory, you don't need this guide:

```bash
git clone https://github.com/aaronsb/agent-ways ~/.claude
cd ~/.claude && make setup
```

Restart Claude Code. Done.

## The problem

Claude Code stores its configuration in `~/.claude/`. This repo *is* `~/.claude/`. That means installing it replaces the directory that Claude Code already uses — and you might have files there you care about.

The installer detects this and stops rather than silently clobbering your work. This guide helps you understand what you're dealing with and pick the right path forward.

## Scenario 1: You have settings but no git tracking

**Signs:** `~/.claude/` exists with files like `settings.json`, `CLAUDE.md`, or `projects/`, but no `.git/` directory.

This is common — Claude Code creates `~/.claude/` on first run with your settings.

**What to save:**
- `settings.json` — your permissions and preferences
- `CLAUDE.md` — your global instructions (if you wrote any)
- `projects/` — per-project memory and settings
- `memory/` — auto-memory files (if using memory)

**Steps:**

```bash
# 1. Back up what matters
mkdir -p ~/claude-config-backup
cp -a ~/.claude/settings.json ~/claude-config-backup/ 2>/dev/null
cp -a ~/.claude/settings.local.json ~/claude-config-backup/ 2>/dev/null
cp -a ~/.claude/CLAUDE.md ~/claude-config-backup/ 2>/dev/null
cp -a ~/.claude/projects ~/claude-config-backup/ 2>/dev/null
cp -a ~/.claude/memory ~/claude-config-backup/ 2>/dev/null

# 2. Move the old directory aside
mv ~/.claude ~/.claude-pre-install

# 3. Install
git clone https://github.com/aaronsb/agent-ways ~/.claude
cd ~/.claude && make setup

# 4. Restore your files
cp ~/claude-config-backup/settings.json ~/.claude/ 2>/dev/null
cp ~/claude-config-backup/settings.local.json ~/.claude/ 2>/dev/null
cp ~/claude-config-backup/CLAUDE.md ~/.claude/ 2>/dev/null
cp -a ~/claude-config-backup/projects ~/.claude/ 2>/dev/null
cp -a ~/claude-config-backup/memory ~/.claude/ 2>/dev/null
```

Your settings are `.gitignore`d in this repo, so they won't conflict with updates.

## Scenario 2: You have your own git tracking

**Signs:** `~/.claude/.git/` exists, but it's your own repo — not a clone or fork of agent-ways.

This means you're already version-controlling your Claude config. Good instinct. The question is whether you want to adopt this framework or keep your own.

**Option A: Adopt this framework (replace your tracking)**

```bash
# Back up everything
cp -a ~/.claude ~/.claude-my-version

# Remove your .git and install ours
rm -rf ~/.claude/.git
rm -rf ~/.claude    # or mv to backup
git clone https://github.com/aaronsb/agent-ways ~/.claude
cd ~/.claude && make setup

# Cherry-pick your customizations back in
# (compare ~/.claude-my-version/ with ~/.claude/ and copy what you want)
```

**Option B: Keep your tracking, adopt selectively**

Read through this repo and copy the parts you want into your own config. The key pieces:
- `hooks/` — the event-driven way system
- `hooks/ways/` — the actual guidance content
- `tools/ways-cli/` — unified CLI (matching, scanning, linting, governance)
- `tools/way-embed/` — embedding engine (separate binary, wraps llama.cpp)
- `settings.json` — hook registration (merge with yours)

This is more work but gives you full control.

## Scenario 3: Previous agent-ways install

**Signs:** `~/.claude/.git/` exists and origin points to `aaronsb/agent-ways` or your fork of it.

You're already installed. Just update:

```bash
cd ~/.claude && git pull
make setup
```

The installer detects this automatically and runs the update path.

## Scenario 4: You want a fork

**Recommended for anyone who plans to customize ways.**

```bash
# 1. Fork on GitHub first (use the web UI)
# 2. Clone your fork
git clone https://github.com/YOUR-USERNAME/agent-ways ~/.claude

# 3. Set up upstream tracking
cd ~/.claude
git remote add upstream https://github.com/aaronsb/agent-ways

# 4. Set up semantic matching
make setup
```

To pull upstream improvements later:

```bash
cd ~/.claude
git fetch upstream
git merge upstream/main
# Resolve any conflicts in your custom ways
make setup
```

The built-in update checker (`hooks/check-config-updates.sh`) detects forks and nudges you when upstream has new commits.

## Scenario 5: Keep your existing config, install alongside it

**Signs:** `~/.claude/` has sessions, credentials, or settings you want to keep untouched, and you'd rather not make it a git repo at all.

This topology keeps the agent-ways repo in a subdirectory (e.g. `~/.claude/directory`) and syncs its outputs into `~/.claude` without replacing it. Your sessions, credentials, and settings stay where they are.

**Steps:**

```bash
# 1. Clone into a subdirectory of ~/.claude (or anywhere you prefer)
git clone https://github.com/aaronsb/agent-ways ~/.claude/directory

# 2. Build binaries and fetch the embedding model
cd ~/.claude/directory && make setup

# 3. Open Claude Code in the repo directory, then run:
/sync-to-home
```

`/sync-to-home` copies skills, agents, commands, hooks, and binaries into `~/.claude` and merges only the hooks block and ways permissions into your existing `settings.json`, leaving your model, theme, plugins, and credentials untouched.

**Keeping up to date:**

```bash
cd ~/.claude/directory
git pull
/sync-to-home
```

Run `/sync-to-home` after every pull. Hooks and binaries that are symlinked to the source are skipped automatically.

**Trade-offs vs the canonical install:**

| | Canonical (`~/.claude` is the repo) | Subdirectory |
|---|---|---|
| Updates | `git pull` | `git pull` + `/sync-to-home` |
| Sessions/credentials isolation | Shared with repo | Fully separate |
| Fork sync | GitHub fork-sync button works | Works, fork stays independent |
| `make install` / `make update` | Direct | Not used — use `/sync-to-home` |

## The nuclear option

If you just want it installed and don't care what's there:

```bash
scripts/install.sh --dangerously-clobber
```

This backs up `~/.claude/` to `~/.claude-backup-YYYYMMDD-HHMMSS/` and replaces it entirely. You'll be asked to type `clobber` to confirm (unless piped/non-interactive).

Your backup is a complete copy — you can always restore:

```bash
rm -rf ~/.claude
mv ~/.claude-backup-20260323-141500 ~/.claude
```

## After installing

1. **Restart Claude Code** — ways activate on session start
2. **Check engine status** — `ways status` shows binary, model, corpus, and project detection
3. **Review ways.json** — `~/.claude/ways.json` controls which domains are active
4. **Read the ways** — browse `~/.claude/hooks/ways/` to understand what guidance is loaded

## What gets downloaded

`make setup` acquires binaries and the embedding model. Downloaded artifacts live in XDG-compliant locations, outside `~/.claude/`:

| Artifact | Size | Location | Source | Verification |
|----------|------|----------|--------|--------------|
| `ways` binary | ~3.6MB | `bin/ways` | GitHub Releases (or built from source) | SHA-256 checksum |
| `way-embed` binary | ~3MB | `~/.cache/claude-ways/user/` | GitHub Releases | SHA-256 checksum |
| `minilm-l6-v2.gguf` model | ~21MB | `~/.cache/claude-ways/user/` | GitHub Releases (or HuggingFace) | SHA-256 checksum |

The `ways` binary lands in `bin/` (gitignored) and is symlinked into `~/.local/bin/` by `make install`. The repo itself stays clean and diffable.

The embedding model is a hard dependency — `ways` will not match without it. If the download fails, rerun `make setup` or fetch the model manually from GitHub Releases.
