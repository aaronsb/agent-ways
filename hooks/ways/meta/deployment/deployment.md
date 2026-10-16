---
description: How agent-ways itself deploys into the home config dir — ~/.claude as a thin projection of an XDG application (source in $XDG_DATA_HOME/agent-ways), how the agent-ways installer/update/`ways manage repair` work under it, how to spot a legacy pre-1.0 in-place agent-ways clone that must `ways migrate` instead of pull, and where the migrator lives now that 1.9.0 removed it from the binary — surfaced only when installing, updating, migrating, or reconciling agent-ways itself, or resolving an existing ~/.claude conflict during agent-ways setup
vocabulary: agent-ways ~/.claude thin projection XDG application $XDG_DATA_HOME/agent-ways ways manage repair reinstall ways migrate reproject legacy in-place clone pre-1.0 agent-ways projected roots settings.json merge curl bash agent-ways installer existing .claude clobber subdirectory topology ADR-142 activation opt-out reprojection disabled adopters re-materialize
pattern: agent-ways|~/\.claude|existing \.?claude|ways (manage|reconcile|migrate|update)|in-place clone|thin projection|xdg.?data
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Deployment

In 1.0, `~/.claude` is a **thin projection** of an XDG application, not the app itself (ADR-142). The source lives in `$XDG_DATA_HOME/agent-ways`; `~/.claude` gets symlinks to the projected roots (`skills/`, `agents/`, `commands/`, `hooks/ways/`, built binaries) plus a three-way merge into `settings.json`. Everything else the app ships — `scripts/`, `tools/`, `docs/`, `governance/` — **stays in `$XDG_DATA`** and is deliberately *not* projected. So `~/.claude` remains the user's own directory (their sessions, credentials, and settings survive); agent-ways adds its links, and it refuses to replace a real directory or file it finds at a projected root.

This supersedes the pre-1.0 world where `~/.claude` *was* the git clone. That "in-place" topology (and the subdirectory variant, ADR-140) is now
**legacy**: an install still on it needs to **migrate**, not update in place.

## How install and update work now

- **Fresh install** — the `curl … | bash` one-liner stages the app into `$XDG_DATA_HOME/agent-ways`, builds it, links the binaries onto `PATH`, and runs `ways manage repair` to materialize the projection and merge `settings.json`. An existing `~/.claude` is **preserved**: reconcile adds or repairs the projected roots, and when a projected root path is already a real directory or file it stops before touching anything and names the path. `ways manage repair --commit --force` renames each such path to a timestamped sibling (`<name>.ways-backup-<seconds>`) and then links; nothing is ever deleted. There is no "clobber vs. keep" menu to reason about anymore.
- **Update** — `ways manage update` fetches and reports what an update would change (versions, release notes, the commits by area, local changes, the actions, dev links) and changes nothing. Read it back to the user. `ways manage update --commit` pulls the app source, refreshes the binaries and reprojects. Because the roots are symlinks into the app dir, a symlink projection is *live* the moment the source updates; reconcile is idempotent and silent when nothing changed.
- **Repair** — `ways manage repair` reports what it would link, merge and withdraw, and changes nothing; `ways manage repair --commit` re-materializes any missing or stale projected root. Every `ways manage` verb that changes the install reports first and applies under `--commit` (ADR-511). On a legacy in-place clone it stops at the clone's real directories and changes nothing; that case needs the migrator (below).
- **Reinstall** — `ways manage reinstall` puts the installed version back from the source it came from, keeping the user's config and state. It fetches and verifies the new copy before it removes anything, so a failed fetch leaves the install as it was. Without `--yes` it prints the plan. All of these live under `ways manage` (ADR-509), whose help lists them.

## Targets: where the install is active (ADR-184)

Installation and activation are separate states. The projection lands in each **target**, a Claude Code config directory recorded under `targets:` in the user config. `ways manage repair` converges every enabled target and withdraws from every disabled one: our symlinks unlinked, our hooks block and permissions removed through the merge base that wrote them, nothing else touched. With no `targets` key the one target is `~/.claude`, enabled.

- `ways manage target list` lists targets and their converged state; `ways status` says it on its first line.
- `ways manage target plan <dir>` previews activation: every root as linked, link, relink, or refused, and the settings merge as kept, added, replaced, removed. Nothing is touched.
- `ways manage target add <dir>` prints the plan and stops (exit 3) when a real path sits at a root or an entry of the user's would go. `--force` moves real paths aside.
- `ways manage target disable <dir>` and `remove <dir>` withdraw.
- A project switches ways off for itself with `enabled: false` in `.claude/ways.yaml`.

When a user asks whether agent-ways will touch something they own, the answer is the plan. Run it and read it back to them before `add`. The installer still activates the default target on its own in this release; the handoff to the targets bootstrap is the next increment of ADR-184.

## The one decision left: is this a legacy in-place clone?

The only fork worth establishing before giving a command is whether `~/.claude` is a **pre-1.0 in-place clone** (it has its own `.git` *and* ships the app source — `~/.claude/tools/`, `~/.claude/docs/`). If so, **do not `git pull` it** and do not reconcile it — point the user at migration.

`ways migrate` was removed in 1.9.0 (ADR-179) and lives at the `ways-v1.8.3` tag. Build it in a scratch clone; it acts on `~/.claude` and the XDG roots at runtime, so where it was built doesn't matter:

```
git clone --branch ways-v1.8.3 https://github.com/aaronsb/agent-ways /tmp/ways-migrator
cargo build --release --manifest-path /tmp/ways-migrator/tools/ways-cli/Cargo.toml
/tmp/ways-migrator/tools/target/release/ways migrate --what-if     # preview (read-only dry-run)
/tmp/ways-migrator/tools/target/release/ways migrate --execute     # relocate the clone to $XDG_DATA, build the projection
```

Migration is gated and backs up first. See https://github.com/aaronsb/agent-ways/blob/ways-v1.8.3/docs/migration-1.0.md for the full walkthrough.

An un-migrated install is **not read**: `paths.rs` resolves the 1.0 locations only (ADR-506). `ways manage repair` stops at the real directories an in-place clone has at the projection roots, and `ways manage update` needs the app source in `$XDG_DATA_HOME`. Point the user at https://github.com/aaronsb/agent-ways/blob/ways-v1.8.3/docs/migration-1.0.md.

## Why this way exists

The first touch for many adopters is `curl … | bash`, often with *a Claude reading the errors and guiding them*. That Claude is you. The pre-1.0 hazard was steering an existing-config user into a clobber; the 1.0 hazard is telling a legacy in-place user to `git pull` (which drifts them) instead of to migrate. Establish the projection model first, then give the command.

## See Also

- skills(meta) — skills are one of the projected roots
- `docs/development.md` — the same projection model, from a contributor's seat (install vs dev checkout vs sandbox)
- https://github.com/aaronsb/agent-ways/blob/ways-v1.8.3/docs/migration-1.0.md — the `ways migrate` walkthrough, run from the `ways-v1.8.3` tag
