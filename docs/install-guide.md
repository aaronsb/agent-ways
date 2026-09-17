# Installation Guide

Most installs are a straight line — the one-liner in the [README](../README.md#quick-start) stages the app, builds it, and projects it into `~/.claude`:

```bash
curl -sL https://raw.githubusercontent.com/aaronsb/agent-ways/main/scripts/install.sh | bash -s -- --bootstrap
```

This guide is for the paths that aren't straight: an existing `~/.claude` you care about, a previous install, or a fork you want to keep in sync.

## The 1.0 model (why there's no "clobber" anymore)

Before 1.0, this repo *was* `~/.claude/` — installing meant cloning over the directory Claude Code already used, so the installer had to detect existing files and stop rather than destroy them. **1.0 dissolves that.** `~/.claude` is now a thin **projection** of an XDG application whose source lives in `$XDG_DATA_HOME/agent-ways` (see [ADR-142](architecture/system/ADR-142-agent-ways-1-0-xdg-application-distribution.md)). Installing only:

- symlinks the projected roots (`skills/`, `agents/`, `commands/`, `hooks/ways/`, built binaries) into `~/.claude`, and
- three-way-merges its owned slices into your `settings.json`: the hooks block, its `permissions.allow` entries (its own binaries), and a `permissions.deny` secret-path baseline (`~/.ssh`, `~/.aws`, `.env`, … — [ADR-152](architecture/system/ADR-152-framework-default-secret-path-deny-baseline.md); opt out with `secret_path_deny: false`).

Everything else you have in `~/.claude` (`settings.json` values you set, `.credentials.json`, `projects/`, `memory/`, `CLAUDE.md`) is **preserved by construction**, because the install never replaces your directory. There is no clobber prompt. (`scripts/` and `tools/` and the rest of the app stay in `$XDG_DATA` and are deliberately *not* projected.)

The one case that needs your attention: if a projected root path (`~/.claude/skills`, `agents`, `commands`, `hooks/ways`, or a file under `~/.claude/bin`) is already a real directory or file of your own, `ways reconcile` stops before touching anything and names it. Nothing is deleted. Move it aside yourself (copy anything you want to keep into a project's `.claude/skills/`), or run `ways reconcile --force` to rename each such path to a timestamped sibling (`skills.ways-backup-<seconds>`) and then link.

## Scenario: you already have a `~/.claude` you value

**Signs:** `~/.claude/` has `settings.json`, `projects/`, credentials, or sessions — with or without its own `.git/`.

Just run the one-liner. The projection coexists with your directory; your files are untouched and your `settings.json` keeps its model, theme, plugins, and your own permissions (agent-ways merges only its owned slices, the hooks block, its `permissions.allow` entries, and the secret-path `permissions.deny` baseline, and backs `settings.json` up first). No move-aside, no restore dance, unless you keep your own `skills/`, `agents/`, or `commands/` directory in `~/.claude`; in that case reconcile stops and tells you, as described above.

If `~/.claude/` is your **own** git repo (you version-control your config), that still works: the projection adds symlinks alongside your tracked files, as long as the repo does not itself track directories at the projected root paths. If it does, reconcile refuses rather than replacing them. Add the projected roots to your `.gitignore` if you don't want them tracked.

## Scenario: you want ways in one repository only

**Signs:** you keep your own skills, agents, or hooks in `~/.claude`, you run more than one Claude Code profile, or you want to try ways on one codebase before turning them on everywhere.

Pass a scope to the installer:

```bash
curl -sL https://raw.githubusercontent.com/aaronsb/agent-ways/main/scripts/install.sh | bash -s -- --bootstrap --scope=project --project=/path/to/repo
```

Project scope ([ADR-183](architecture/system/ADR-183-install-scope-project-scoped-hook-wiring-on-the-one-reconciler.md)) does three things differently from the default:

- It links only the hook tree and the binaries into `~/.claude` (`hooks/ways/`, `hooks/check-config-updates.sh`, `bin/*`). `skills/`, `agents/`, and `commands/` are not projected, so yours stay yours.
- It writes the hooks block into `<repo>/.claude/settings.local.json` instead of `~/.claude/settings.json`. Claude Code concatenates hooks across the user file and the project files, so ways fire only in sessions under that directory. Nothing is written to your user `settings.json`.
- It writes hooks only. The `permissions.allow` entries and the secret-path `permissions.deny` baseline are user-level policy and stay out of the project file.

What you give up: the shipped skills, agents, and commands (`ways-update`, `ways-tests`, `/ways`, and the rest). Copy any you want into `<repo>/.claude/skills/`. To update, pull the app source and run `ways reconcile`; the scope is remembered from the state the first run wrote, so a bare `ways reconcile` (which is what `ways update` runs) stays in project scope. Add a second repository with `ways reconcile --scope project --project /path/to/other`.

`settings.local.json.bak` lands next to the merged file; `ways init` gitignores it for new projects. Add the line yourself in an existing one.

## Scenario: a previous agent-ways install

**A 1.0 projection install** (the app is in `$XDG_DATA_HOME/agent-ways`, `~/.claude` is not a repo) — update in place:

```bash
cd "$XDG_DATA_HOME/agent-ways" && make update && ways reconcile
```

`make update` pulls, **force-rebuilds** the binaries, regenerates the corpus, and relinks — use it rather than `make setup`, which skips binaries that already exist and would leave you on the old build.

**A legacy pre-1.0 in-place clone** (`~/.claude` *is* the agent-ways git repo — it has its own `.git/` and ships `~/.claude/tools/`, `~/.claude/docs/`) — do **not** `git pull` it. Migrate it to the 1.0 model with the gated, backup-first migrator:

The migrator was removed in 1.9.0 (ADR-179) and lives at the `ways-v1.8.3` tag. Build it in a scratch clone and run it against your install:

```bash
git clone --branch ways-v1.8.3 https://github.com/aaronsb/agent-ways /tmp/ways-migrator
cargo build --release --manifest-path /tmp/ways-migrator/tools/ways-cli/Cargo.toml
/tmp/ways-migrator/tools/target/release/ways migrate --what-if   # preview (read-only)
/tmp/ways-migrator/tools/target/release/ways migrate --execute   # relocate the clone to $XDG_DATA, build the projection
```

See the [Migration Guide](migration-1.0.md) for the full walkthrough.

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

Running the installer from inside the app dir is what links the suite binaries (`ways`, `ways-audit`, `attend`, `attend-chat`) onto your `PATH`; `make setup` alone builds them but does not. Pull upstream improvements later:

```bash
cd "$XDG_DATA_HOME/agent-ways"
git fetch upstream && git merge upstream/main   # resolve conflicts in your custom ways
make update-binaries && ways reconcile          # force-rebuild (make setup would skip existing binaries)
```

If you're actively *developing* agent-ways (not just carrying a few custom ways), use a standalone dev checkout instead and dogfood via reconcile — see [development.md](development.md).

## Legacy: the subdirectory topology

Pre-1.0, the way to keep an existing `~/.claude` untouched was the **subdirectory topology** (ADR-140): clone into `~/.claude/agent-ways` and project with `make sync-to-home`. Native projection now *is* that story — a fresh install already keeps your config intact — so the subdirectory topology is **superseded**. If you're on it, the migrator at the `ways-v1.8.3` tag moves you to the native projection ([guide](migration-1.0.md)). (The conceptual history lives in [docs/explanation/install-topologies/](explanation/install-topologies/), kept as a record of how the model evolved.)

## After installing

1. **Restart Claude Code** — ways activate on session start.
2. **Check engine status** — `ways status` shows binary, model, corpus, and project detection.
3. **Read the ways** — browse `~/.claude/hooks/ways/` (a projected symlink into the app) to see the loaded guidance.
4. **Config** — user config lives in `$XDG_CONFIG_HOME/agent-ways/config.yaml` (a legacy `$XDG_CONFIG_HOME/ways/config.yaml` and `~/.claude/ways.json` are still honored). It controls which domains are active.

## What gets downloaded

`make setup` acquires binaries and the embedding model. Downloaded artifacts live in XDG-compliant locations, outside `~/.claude/`:

| Artifact | Size | Location | Source | Verification |
|----------|------|----------|--------|--------------|
| `ways` binary | ~3.6MB | `$XDG_DATA_HOME/agent-ways/bin/` (symlinked onto `PATH`) | GitHub Releases (or built from source) | SHA-256 checksum |
| `ways-audit` binary | ~2.6MB | `$XDG_DATA_HOME/agent-ways/bin/` (symlinked onto `PATH`) | GitHub Releases (or built from source) | SHA-256 checksum |
| `attend` / `attend-chat` binaries | ~2–3MB each | `$XDG_DATA_HOME/agent-ways/bin/` (symlinked onto `PATH`) | GitHub Releases (or built from source) | SHA-256 checksum |
| `way-embed` binary | ~3MB | XDG cache (`…/user/`) | GitHub Releases | SHA-256 checksum |
| `minilm-l6-v2.gguf` model | ~21MB | XDG cache (`…/user/`) | GitHub Releases (or HuggingFace) | SHA-256 checksum |

The embedding model is a hard dependency — `ways` will not match without it. If the download fails, rerun `make setup` or fetch the model manually from GitHub Releases.
