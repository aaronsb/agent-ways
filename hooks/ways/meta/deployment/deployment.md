---
description: How agent-ways itself deploys into the home config dir — ~/.claude as a thin projection of an XDG application (source in $XDG_DATA_HOME/agent-ways), how the agent-ways installer/update/`ways reconcile` work under it, how to spot a legacy pre-1.0 in-place agent-ways clone that must `ways migrate` instead of pull, and where the migrator lives now that 1.9.0 removed it from the binary — surfaced only when installing, updating, migrating, or reconciling agent-ways itself, or resolving an existing ~/.claude conflict during agent-ways setup
vocabulary: agent-ways ~/.claude thin projection XDG application $XDG_DATA_HOME/agent-ways ways reconcile ways migrate reproject legacy in-place clone pre-1.0 agent-ways projected roots settings.json merge curl bash agent-ways installer existing .claude clobber sync-to-home topology ADR-142
pattern: agent-ways|~/\.claude|existing \.?claude|ways (reconcile|migrate)|make update|in-place clone|thin projection|xdg.?data
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Deployment

In 1.0, `~/.claude` is a **thin projection** of an XDG application, not the app itself (ADR-142). The source lives in `$XDG_DATA_HOME/agent-ways`; `~/.claude` gets symlinks to the projected roots (`skills/`, `agents/`, `commands/`, `hooks/ways/`, built binaries) plus a three-way merge into `settings.json`. Everything else the app ships — `scripts/`, `tools/`, `docs/`, `governance/` — **stays in `$XDG_DATA`** and is deliberately *not* projected. So `~/.claude` remains the user's own directory (their sessions, credentials, and settings survive); agent-ways just adds its links.

This supersedes the pre-1.0 world where `~/.claude` *was* the git clone. That "in-place" topology (and the `sync-to-home` subdirectory variant, ADR-140) is now
**legacy**: an install still on it needs to **migrate**, not update in place.

## How install and update work now

- **Fresh install** — the `curl … | bash` one-liner stages the app into `$XDG_DATA_HOME/agent-ways`, builds it, links the binaries onto `PATH`, and runs `ways reconcile` to materialize the projection and merge `settings.json`. An existing `~/.claude` is **preserved**, not clobbered — reconcile only adds/repairs the projected roots. There is no "clobber vs. keep" menu to reason about anymore.
- **Update** — pull the app source (or re-run the installer) in `$XDG_DATA_HOME/agent-ways`, then `ways reconcile` reprojects. Because the roots are symlinks into the app dir, a symlink projection is *live* the moment the source updates; reconcile is idempotent and silent when nothing changed.
- **Repair** — `ways reconcile` alone re-materializes any missing or stale projected root. It refuses to run against a legacy in-place clone (that would strand the user's checkout) — that case routes to the migrator.

## The one decision left: is this a legacy in-place clone?

The only fork worth establishing before giving a command is whether `~/.claude` is a **pre-1.0 in-place clone** (it has its own `.git` *and* ships the app source — `~/.claude/tools/`, `~/.claude/docs/`). If so, **do not `git pull` it** and do not reconcile it — point the user at migration.

`ways migrate` was removed in 1.9.0 (ADR-179) and lives at the `ways-v1.8.3` tag. Build it in a scratch clone; it acts on `~/.claude` and the XDG roots at runtime, so where it was built doesn't matter:

```
git clone --branch ways-v1.8.3 https://github.com/aaronsb/agent-ways /tmp/ways-migrator
cargo build --release --manifest-path /tmp/ways-migrator/tools/ways-cli/Cargo.toml
/tmp/ways-migrator/tools/target/release/ways migrate --what-if     # preview (read-only dry-run)
/tmp/ways-migrator/tools/target/release/ways migrate --execute     # relocate the clone to $XDG_DATA, build the projection
```

Migration is gated and backs up first. See `docs/migration-1.0.md` for the full walkthrough.

An un-migrated install still **works** — the transition fallbacks in `paths.rs` read the legacy cache and stats locations. What it can't do is `ways update` or `ways reconcile`; both refuse and point here.

## Why this way exists

The first touch for many adopters is `curl … | bash`, often with *a Claude reading the errors and guiding them*. That Claude is you. The pre-1.0 hazard was steering an existing-config user into a clobber; the 1.0 hazard is telling a legacy in-place user to `git pull` (which drifts them) instead of to migrate. Establish the projection model first, then give the command.

## See Also

- skills(meta) — skills are one of the projected roots
- `docs/development.md` — the same projection model, from a contributor's seat (install vs dev checkout vs sandbox)
- `docs/migration-1.0.md` — the `ways migrate` walkthrough, run from the `ways-v1.8.3` tag
