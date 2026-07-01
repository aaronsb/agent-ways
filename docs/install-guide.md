# Installation Guide

Most installs are a straight line — the one-liner in the [README](../README.md#quick-start) stages the app, builds it, and projects it into `~/.claude`:

```bash
curl -sL https://raw.githubusercontent.com/aaronsb/agent-ways/main/scripts/install.sh | bash -s -- --bootstrap
```

This guide is for the paths that aren't straight: an existing `~/.claude` you care about, a previous install, or a fork you want to keep in sync.

## The 1.0 model (why there's no "clobber" anymore)

Before 1.0, this repo *was* `~/.claude/` — installing meant cloning over the directory Claude Code already used, so the installer had to detect existing files and stop rather than destroy them. **1.0 dissolves that.** `~/.claude` is now a thin **projection** of an XDG application whose source lives in `$XDG_DATA_HOME/agent-ways` (see [ADR-142](architecture/system/ADR-142-agent-ways-1-0-xdg-application-distribution.md)). Installing only:

- symlinks the projected roots (`skills/`, `agents/`, `commands/`, `hooks/ways/`, built binaries) into `~/.claude`, and
- three-way-merges the hooks block into your `settings.json`.

Everything else you have in `~/.claude` — `settings.json` values you set, `.credentials.json`, `projects/`, `memory/`, `CLAUDE.md` — is **preserved by construction**. There is nothing to back up first and no clobber prompt, because the install never replaces your directory. (`scripts/` and `tools/` and the rest of the app stay in `$XDG_DATA` and are deliberately *not* projected.)

## Scenario: you already have a `~/.claude` you value

**Signs:** `~/.claude/` has `settings.json`, `projects/`, credentials, or sessions — with or without its own `.git/`.

Just run the one-liner. The projection coexists with your directory; your files are untouched and your `settings.json` keeps its model, theme, plugins, and permissions (only the hooks block is merged in, and it backs up first). No move-aside, no restore dance.

If `~/.claude/` is your **own** git repo (you version-control your config), that still works — the projection adds symlinks alongside your tracked files. Add the projected roots to your `.gitignore` if you don't want them tracked.

## Scenario: a previous agent-ways install

**A 1.0 projection install** (the app is in `$XDG_DATA_HOME/agent-ways`, `~/.claude` is not a repo) — update in place:

```bash
cd "$XDG_DATA_HOME/agent-ways" && git pull && make setup && ways reconcile
```

**A legacy pre-1.0 in-place clone** (`~/.claude` *is* the agent-ways git repo — it has its own `.git/` and ships `~/.claude/tools/`, `~/.claude/docs/`) — do **not** `git pull` it. Migrate it to the 1.0 model with the gated, backup-first migrator:

```bash
ways migrate --what-if     # preview (read-only)
ways migrate --execute     # relocate the clone to $XDG_DATA, build the projection
```

See the [Migration Guide](migration-1.0.md) for the full walkthrough and the deprecation window (the migrator ships through 1.0.x and is removed at 1.1).

## Scenario: you want a fork

**Recommended for anyone who plans to customize ways.** Fork on GitHub, then install *from your fork* by making it the app source:

```bash
# 1. Fork on GitHub (web UI), then clone your fork as the app source
git clone https://github.com/YOUR-USERNAME/agent-ways "$XDG_DATA_HOME/agent-ways"
cd "$XDG_DATA_HOME/agent-ways"

# 2. Track upstream for later
git remote add upstream https://github.com/aaronsb/agent-ways

# 3. Install from the fork — builds, links `ways` onto PATH, and projects into ~/.claude
./scripts/install.sh
```

Running the installer from inside the app dir is what links the `ways`/`attend`
binaries onto your `PATH`; `make setup` alone builds them but does not. Pull
upstream improvements later:

```bash
cd "$XDG_DATA_HOME/agent-ways"
git fetch upstream && git merge upstream/main   # resolve conflicts in your custom ways
make setup && ways reconcile                    # ways is on PATH from the install above
```

If you're actively *developing* agent-ways (not just carrying a few custom ways), use a standalone dev checkout instead and dogfood via reconcile — see [development.md](development.md).

## Legacy: the subdirectory topology

Pre-1.0, the way to keep an existing `~/.claude` untouched was the **subdirectory topology** (ADR-140): clone into `~/.claude/agent-ways` and project with `make sync-to-home`. Native projection now *is* that story — a fresh install already keeps your config intact — so the subdirectory topology is **superseded**. If you're on it, `ways migrate` moves you to the native projection. (The conceptual history lives in [docs/explanation/install-topologies/](explanation/install-topologies/), kept as a record of how the model evolved.)

## After installing

1. **Restart Claude Code** — ways activate on session start.
2. **Check engine status** — `ways status` shows binary, model, corpus, and project detection.
3. **Read the ways** — browse `~/.claude/hooks/ways/` (a projected symlink into the app) to see the loaded guidance.
4. **Config** — user config lives in `$XDG_CONFIG_HOME/ways/config.yaml` (a legacy `~/.claude/ways.json` is still honored). It controls which domains are active.

## What gets downloaded

`make setup` acquires binaries and the embedding model. Downloaded artifacts live in XDG-compliant locations, outside `~/.claude/`:

| Artifact | Size | Location | Source | Verification |
|----------|------|----------|--------|--------------|
| `ways` binary | ~3.6MB | `$XDG_DATA_HOME/agent-ways/bin/` (symlinked onto `PATH`) | GitHub Releases (or built from source) | SHA-256 checksum |
| `way-embed` binary | ~3MB | XDG cache (`…/user/`) | GitHub Releases | SHA-256 checksum |
| `minilm-l6-v2.gguf` model | ~21MB | XDG cache (`…/user/`) | GitHub Releases (or HuggingFace) | SHA-256 checksum |

The embedding model is a hard dependency — `ways` will not match without it. If the download fails, rerun `make setup` or fetch the model manually from GitHub Releases.
