# Post-ADR-126 Simplification Plan

Written 2026-04-22, after ADR-126 (window-relative refire) landed in PR #71.

ADR-126 notably simplified the *authoring surface* — one-line `refire:` replaces the three-line `curve:` block, and intent is expressed as a fraction of session capacity rather than host-specific token counts. But the surrounding system is still layered: the firing engine still carries dead variants, checks and ways run separate "how often re-disclose" systems with different math, and lint reimplements runtime dispatch logic.

This note captures three targeted simplifications to pursue next, with a strict constraint.

## The Provision (Non-Negotiable)

**A simpler authoring experience must not prevent emergent complexity from the ways.**

The value of the ways system comes from small primitives composing into rich behavior: progressive disclosure through sub-directories, macros that inject arbitrary dynamic state, multi-language locale stubs as coordinate aliases, project-local overrides, state-based triggers on environmental conditions. Simplification should target the *common path* — the 90% case where an author is adding a straightforward way — while preserving escape hatches for intentional complexity.

Any change that makes the common case simpler at the cost of closing off a valid advanced pattern is rejected. Concretely:

- Authors writing a basic way should not need to know anything about `ActionPotential` curves — but an operator who genuinely needs burst-detection semantics should still have a path.
- A check file should feel like "a check on its parent way's activity" — but if a check needs its own re-fire cadence distinct from the parent, that should remain expressible.
- Lint's fire-eligibility predicate should be one line for the common case — but it must still correctly identify fire-bearing ways across every trigger combination currently supported at runtime.

When in doubt: preserve the expressive primitive, simplify the naming or the default path *around* it.

## Opportunity 1 — Collapse Unused `Curve` Variants

`sensor-trait` currently defines four variants:

- `Exponential { half_life }` — used by 100% of ways post-migration
- `ActionPotential { burst_threshold, peak_multiplier, absolute_refractory, multiplier_half_life }` — unused in the tree
- `ProgressiveStaircase { steps }` — unused in the tree
- `Flat { suppression }` — unused in the tree

Three of the four variants are aspirational. They cost enum-match surface in every consumer (`ways-cli`, `attend`, and any future crate that reads sensor-trait). Each new consumer pays for dead code.

**Proposal:** Move `ActionPotential`, `ProgressiveStaircase`, and `Flat` to a new module `sensor_trait::curve_advanced` behind a feature flag. The default build exposes only `Curve::Exponential`. Feature-flag consumers can opt in when they have real use cases.

**Preserving emergent complexity:** The advanced variants stay in the codebase, documented, testable, and activatable with a one-line Cargo toggle. Future authors don't re-derive the math — it's there waiting.

**Risk:** If a future tree-wide change would have benefited from `Flat` (e.g., wall-clock suppression for an attend-style handler), the feature flag adds friction. Mitigation: document the flag, and if the flag has been active on any branch for >3 months, fold it back into default.

## Opportunity 2 — Unify Checks and Refire

Checks (`*.check.md`) currently use an ADR-103 scoring curve — epoch-distance-based, exponential decay with a `threshold: 2.0` default — conceptually adjacent to refire but mathematically separate. Two systems, two mental models, two sets of tuning knobs. When authoring a parent way with a check, the mental mode-switch between "refire cadence" and "check scoring" is real cognitive tax.

**Proposal:** Investigate whether checks' re-fire semantics can be re-expressed as refire + a multiplier on parent-fire epoch-distance. If the math maps cleanly, unify: one refire concept, one tuning surface, one lint contract.

**Preserving emergent complexity:** Checks genuinely differ from ways — they fire on `PreToolUse` against tool-call context, not on prompt content. The unification is at the *cadence* layer only, not the *trigger* layer. If the cadence math doesn't cleanly map (e.g., checks need epoch-distance-aware scoring that doesn't reduce to fraction-of-window), abandon the unification and instead expose a shared vocabulary at the authoring layer ("both use `refire:` and the engine interprets based on file kind").

**Risk:** Forcing a unification for aesthetic reasons could degrade check behavior that's already tuned. Only pursue if ADR-103's design notes reveal a clean mapping. Status quo is acceptable.

## Opportunity 3 — Expose Fire-Eligibility as a Lint Predicate

`tools/ways-cli/src/cmd/lint/per_file.rs` currently computes `is_fire_bearing` with five disjuncts matching the runtime's trigger channels (description+vocabulary OR pattern OR files OR commands OR trigger). This is a reimplementation of dispatch logic that lives elsewhere — if the runtime adds a new trigger channel, the lint check silently goes out of sync.

**Proposal:** Add `Frontmatter::fires_on_something()` as a pure method on the parsed frontmatter. Both the lint check and the runtime dispatch call it. One source of truth.

**Preserving emergent complexity:** The predicate stays a pure function of frontmatter — it doesn't depend on session state, config, or runtime context. Lint stays standalone-runnable without a live session. New trigger channels still compose naturally; they get added to the predicate once.

**Risk:** Minimal. Pure refactor. Drift between lint and runtime is eliminated.

## Sequencing

Opportunity 3 is the cheapest (pure refactor, no design change) and should go first.

Opportunity 1 is scoped work (feature flag setup, move three types, update consumers). Medium effort, clear diff.

Opportunity 2 is the investigate-first opportunity. Start by reading ADR-103 in detail and sketching a math mapping. Only proceed if the mapping is clean — don't force a unification that breaks check behavior.

None of these are blocking anything. All are quality-of-life reductions in system convolution, ordered by the constraint that emergent complexity must remain accessible.

## Explicit Non-Goals

- **Do not** remove `refire_presets` in favor of a hardcoded table. The config-overlay pattern is load-bearing for project-scoped tuning.
- **Do not** collapse progressive disclosure into refire. Two different concerns: ADR-125 (which ways are reachable) vs ADR-126 (how often they re-fire once reachable).
- **Do not** remove the numeric form of `refire:`. Presets are a portability layer; numeric is the primitive. Both must coexist.
- **Do not** extract `ways lint` into a separate binary. The single-binary `ways` surface (ADR-111) is intentional.
