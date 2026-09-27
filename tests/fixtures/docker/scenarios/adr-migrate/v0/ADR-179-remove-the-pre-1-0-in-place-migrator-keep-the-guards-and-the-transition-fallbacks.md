---
status: Accepted
date: 2026-08-17
deciders:
  - aaronsb
  - claude
related:
  - ADR-142
  - ADR-144
  - ADR-153
---

# ADR-179: Remove the pre-1.0 in-place migrator; keep the guards and the transition fallbacks

## Context

ADR-144 §5 shipped `ways migrate` (plan / `--what-if` / `--execute`) to move a
pre-1.0 in-place `~/.claude` clone onto the 1.0 projection model, and gave it an
explicit deprecation lifecycle: dormant at 1.0.0, escalating SessionStart
pressure across 1.0.x, removed at 1.1.0. The escape hatch was named in the same
section — the migrator lives forever at the last tag that ships it, so removal
never means a user cannot migrate.

The removal has been deferred twice. 1.1.0 shipped with the migrator still
present; 1.2.0 (`691b0e6`) corrected the docs and moved the window to 1.3.0.
That target passed too. The line is now at **1.8.3** and the migrator is still
compiled in — roughly 1,000 lines across `migrate.rs` and `migrate_exec.rs`,
plus the CLI variant, a reconcile parameter that exists only for it, and
migration instructions in the README, the install guide, `install.sh`, the
`deployment` way, and the `ways-update` skill.

The deferrals bought adopter time. They have also left the escalation curve's
end state unreached: eight minor releases past the announced window, every
surface still presents `ways migrate` as a live command, and the deprecation
notices in those surfaces name removal targets (1.1, 1.3) that came and went.

ADR-144 §5 also drew a second line that the removal must respect. Its "the cliff
is a ramp" clause says removing the migrator removes *assisted migration*, not
*function*: a post-removal binary on an un-migrated `~/.claude` still reads
correctly through the transition fallbacks — cache from the legacy
`claude-ways` dir, events from `~/.claude/stats`. Those fallbacks live in
`ways-core/src/paths.rs` and are cheap, non-destructive, and inert on a current
install.

Two guards share the migrator's detector, `reconcile::is_legacy_in_place`.
`ways reconcile` refuses to run against an in-place clone because projecting
symlinks over a live repo would strand the user's checkout; `ways update`
refuses for the same reason. Both currently route the user to `ways migrate`.
ADR-144 §5 listed "migrator **and in-place check** removed" as one step. Removing
the checks would let `ways reconcile` clobber an in-place clone — an actively
destructive outcome for exactly the users the never-strand clause protects.

## Decision

Remove the migrator. Keep the in-place guards and the transition fallbacks.

**Removed:**

- `tools/ways-cli/src/cmd/migrate.rs` and `tools/ways-cli/src/cmd/migrate_exec.rs`,
  their `mod` declarations, the `Commands::Migrate` clap variant, and its
  dispatch arm.
- `ways_core::paths::cache_root_canonical`, which exists solely as the
  migrator's rename destination. Runtime reads use the fallback-aware
  `cache_root`.
- `reconcile::run`'s `allow_in_place` parameter and the bypass it gates. The
  migrator is the only caller that ever passed `true`; with it gone the guard is
  unconditional.
- The migration walkthrough in `docs/migration-1.0.md`, the `ways migrate`
  invocations in `scripts/install.sh`, `skills/ways-update/SKILL.md`, the
  `deployment` way, README, and `docs/install-guide.md`.

**Kept:**

- `reconcile::is_legacy_in_place` and both guards. Their messages retarget from
  "run `ways migrate`" to the tag escape hatch. A user who reaches these guards
  is told what their install is and where the migrator still lives.
- The `paths.rs` transition fallbacks — `LEGACY_CACHE` resolution in
  `resolve_cache`, the `~/.claude/stats/events.jsonl` fallback in
  `resolve_events`, and the `events_log_sources` union (ADR-153 §1). These are
  the ramp ADR-144 §5 promised. The union also recovers orphaned `session_start`
  lines on *migrated* installs whose shell hooks predated the path fix, so it
  earns its keep independent of the in-place case.
- The legacy `~/.claude/ways.json` disabled layer, a lower-precedence config
  read with no coupling to the migrator.

**Escape hatch:** `ways-v1.8.3` is the last tag shipping the migrator. Migrating
after removal means `git clone --branch ways-v1.8.3 … && ways migrate --execute`,
then updating. `docs/migration-1.0.md` is rewritten around that route rather than
deleted, and the guards point at it.

Removal lands in **1.9.0**.

## Consequences

### Positive

- About 1,000 lines of transitional code leave the binary, along with a
  destructive code path (whole-`~/.claude` relocation) that no current install
  exercises.
- `reconcile::run` loses a boolean parameter whose only non-default caller is
  being deleted, so the in-place guard becomes unconditional and the function's
  contract simplifies.
- Every user-facing surface stops advertising a deprecation target that has
  already passed. The migration story becomes one route (the tag) instead of a
  live command shadowed by stale removal dates.

### Negative

- A pre-1.0 adopter who has not migrated by 1.9.0 now has a two-step path (clone
  the tag, migrate, update) instead of one command. This is the cost ADR-144 §5
  accepted when it named the tag as the escape hatch.
- The migrator's crash-safe phase machinery and its tests go with it. Reviving
  the capability would mean recovering it from the tag.

### Neutral

- ADR-144 §5's lifecycle is executed, not superseded — its escalation dates were
  the cadence, not the contract. This ADR records the divergence on one point:
  the in-place *check* stays.
- The `deployment` way keeps its legacy-in-place branch. The detection guidance
  is still correct; only the remedy changes.

## Alternatives Considered

- **Remove the guards too, as ADR-144 §5 literally specified.** Rejected: with
  the guard gone, `ways reconcile` on an in-place clone projects over a live git
  repo and strands the checkout. Deleting assistance is within the lifecycle;
  adding a destructive path is not.
- **Remove the `paths.rs` transition fallbacks in the same change.** Rejected for
  now: they are inert on a current install, and dropping them turns "reads
  correctly, unassisted" into "silently re-fetches the model and stops reading
  its own stats." The `events_log_sources` union has a second justification that
  outlives the in-place case. If these come out, it is as their own decision with
  its own reasoning.
- **Defer again to 2.0.0.** Rejected: two deferrals have already passed with no
  signal that a third would be used differently, and each one leaves the shipped
  docs quoting a removal date that has expired.
- **Keep the migrator indefinitely as a dormant command.** Rejected: it carries a
  destructive code path and a phase machine that no test run outside its own
  fixtures exercises, and its presence is why the docs still carry a transition
  narrative eight releases past 1.0.
