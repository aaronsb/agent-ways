---
description: How agent-ways 1.0 deploys into the home config dir — ~/.claude as a thin projection of an XDG application (source in $XDG_DATA_HOME/agent-ways), how install and update work under it, and how to spot a legacy pre-1.0 in-place clone that must migrate instead of pull — surfaced when install, update, or an existing ~/.claude conflict comes up
vocabulary: install setup deploy deployment update ~/.claude existing config clobber projection xdg reconcile migrate legacy in-place upgrade reinstall fresh greenfield brownfield sync-to-home
pattern: install|reconcile|migrate|clobber|topology|~/\.claude|existing \.?claude|make update|deploy|projection
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Deployment

In 1.0, `~/.claude` is a **thin projection** of an XDG application, not the app
itself (ADR-142). The source lives in `$XDG_DATA_HOME/agent-ways`; `~/.claude` gets
symlinks to the projected roots (`skills/`, `agents/`, `commands/`, `hooks/ways/`,
built binaries) plus a three-way merge into `settings.json`. Everything else the
app ships — `scripts/`, `tools/`, `docs/`, `governance/` — **stays in `$XDG_DATA`**
and is deliberately *not* projected. So `~/.claude` remains the user's own directory
(their sessions, credentials, and settings survive); agent-ways just adds its links.

This supersedes the pre-1.0 world where `~/.claude` *was* the git clone. That
"in-place" topology (and the `sync-to-home` subdirectory variant, ADR-140) is now
**legacy**: an install still on it needs to **migrate**, not update in place.

## How install and update work now

- **Fresh install** — the `curl … | bash` one-liner stages the app into
  `$XDG_DATA_HOME/agent-ways`, builds it, links the binaries onto `PATH`, and runs
  `ways reconcile` to materialize the projection and merge `settings.json`. An
  existing `~/.claude` is **preserved**, not clobbered — reconcile only adds/repairs
  the projected roots. There is no "clobber vs. keep" menu to reason about anymore.
- **Update** — pull the app source (or re-run the installer) in
  `$XDG_DATA_HOME/agent-ways`, then `ways reconcile` reprojects. Because the roots
  are symlinks into the app dir, a symlink projection is *live* the moment the source
  updates; reconcile is idempotent and silent when nothing changed.
- **Repair** — `ways reconcile` alone re-materializes any missing or stale projected
  root. It refuses to run against a legacy in-place clone (that would strand the
  user's checkout) — that case routes to the migrator.

## The one decision left: is this a legacy in-place clone?

The only fork worth establishing before giving a command is whether `~/.claude` is
a **pre-1.0 in-place clone** (it has its own `.git` *and* ships the app source —
`~/.claude/tools/`, `~/.claude/docs/`). If so, **do not `git pull` it** and do not
reconcile it — point the user at migration:

```
ways migrate --what-if     # preview (read-only dry-run)
ways migrate --execute     # relocate the clone to $XDG_DATA, build the projection
```

Migration is gated and backs up first. See `docs/migration-1.0.md` for the full
walkthrough and the deprecation window (the migrator ships through 1.0.x and is
removed at 1.1; an un-migrated install then bases on a pre-1.1 `ways-v1.0.x` tag).

## Why this way exists

The first touch for many adopters is `curl … | bash`, often with *a Claude reading
the errors and guiding them*. That Claude is you. The pre-1.0 hazard was steering an
existing-config user into a clobber; the 1.0 hazard is telling a legacy in-place user
to `git pull` (which drifts them) instead of to migrate. Establish the projection
model first, then give the command.

## See Also

- localize(meta) — sibling adopter deployment-time choice (output language)
- skills(meta) — skills are one of the projected roots
- `docs/development.md` — the same projection model, from a contributor's seat (install vs dev checkout vs sandbox)
- `docs/migration-1.0.md` — the `ways migrate` walkthrough and deprecation lifecycle
