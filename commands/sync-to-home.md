---
description: Sync a subdirectory agent-ways clone into ~/.claude (subdirectory topology, ADR-140)
allowed-tools: Bash
---

# /sync-to-home

Project this agent-ways repo clone up into `~/.claude` for the **subdirectory
topology** (ADR-140): the repo lives in a subdirectory of `~/.claude` while
`~/.claude` stays your own config dir. Run after pulling upstream changes.

The mechanism is deterministic and lives in `make sync-to-home` — your job is the
judgment around it, not the bash.

## What to do

1. **Confirm this is the subdirectory topology.** Run `git rev-parse --show-toplevel`
   and compare to `$HOME/.claude`. If the repo root *is* `~/.claude`, this is the
   in-place topology — stop and tell the user to use `make update` instead; there is
   nothing to project.

2. **Get consent — this mutates `~/.claude`.** The sync replaces the hooks block in
   `~/.claude/settings.json`, adds ways permissions, and copies skills/agents/
   commands/hooks/binaries into `~/.claude` (backing up first to
   `~/.claude/backups/`). Confirm the user wants this before proceeding.

3. **Run the make target** from the repo root:

   ```bash
   make sync-to-home
   ```

   For the advanced symlink variant (so a future `git pull` *is* the whole update,
   with no re-sync), use `make sync-to-home-link` instead — but only if the user
   asked for it; copy is the default.

4. **Report** what changed: the backup location, that settings.json was merged
   (model/theme/plugins/credentials untouched), and that Claude Code must be
   restarted to activate the changes. Surface any warning the script printed
   (e.g. a binary rebuild that fell back to the existing `bin/`).

## Not for

- The in-place topology (`~/.claude` *is* the repo) — that's `make update`.
- Re-implementing the projection in bash — it lives in `scripts/sync-to-home.sh`
  behind `make sync-to-home`. Drive the target; don't duplicate it.
