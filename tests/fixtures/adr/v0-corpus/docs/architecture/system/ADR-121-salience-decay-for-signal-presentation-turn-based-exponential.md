---
status: Superseded
date: 2026-04-12
revised: 2026-04-14
deciders:
  - aaronsb
  - claude
related:
  - ADR-113
  - ADR-114
  - ADR-119
  - ADR-123
superseded_by: ADR-123
---

# ADR-121: Salience decay for signal presentation — turn-based exponential

## Status: Superseded by ADR-123

**Superseded 2026-04-14** while the specific attend-side application remained deferred. This ADR introduced two ideas: (1) firing dynamics decompose into an **inward gate** ("should this new stimulus fire?") and an **outward gate** ("should this already-fired signal still be presented?"), and (2) the outward gate for attend's peer signals should be turn-based exponential salience decay with a configurable floor. The inward/outward gate framing became the load-bearing architectural contribution of [ADR-123](ADR-123-firing-dynamics-progression-axis-unification.md), which generalized both gates across tools via the shared curve engine.

The outward-gate decision shipped first **on the ways side, not the attend side**:

- Ways now runs `Curve::Exponential` as its outward gate via `session::way_fire_outcome()`, gating re-fire on `current_salience(current_tick) < REFIRE_FLOOR` (default 0.5). Every way declares an explicit `curve:` block in its frontmatter. Tick is token position, not turn count, because ways' progression axis is the host's addressing unit (see ADR-123 Decision 4).
- Attend's peer-signal presentation still ships the pre-ADR-121 behavior: signals appear in notifications indefinitely until the 30-day disk cleanup removes them. The turn-based aging described below is **still deferred**. When it lands, it will use the same `EngagementState` / `Curve::Exponential` infrastructure the ways side already uses, but with a turn-count progression axis instead of tokens.

So this ADR's framing is shipping (in a different place than originally targeted) and its application is pending (in the place originally targeted). The body below describes the 2026-04-12 decision; the "What ADR-123 changed" section below reconciles it with what actually exists today.

## Context

ADR-119 (action potential engagement model) governs *when new stimuli break through* to fire a disclosure. Disk retention (default 30 days) governs *how long signal files live on disk*. Neither mechanism addresses the middle case: a signal that has already been disclosed, is still on disk, and is still within the "present to the agent" set at every turn regardless of whether it's still load-bearing.

In a long session, this manifests as presentation bloat — signals from the first few turns keep appearing in notifications 40, 60, 80 turns later, long after their context has been overtaken. A survey of 18 recent active sessions shows the distribution is long-tailed: median 12 turns, p75 45, p90 84, max 133. Short sessions never see the problem; long sessions see it badly.

Time-based aging is tempting but wrong at this scale. Turn pacing varies by an order of magnitude — a 10-minute think turn and a 15-second reply both count as "one turn" and should age the same. A time-based window would wobble with pacing; a turn-based window tracks the actual thing we care about: how many rounds of decision-making have passed since this signal was useful.

Disk retention (30 days) is intentionally time-based because at that horizon time is a fine proxy for "bulk turns" — variance averages out across thousands of turns. Presentation aging operates at a much finer grain where the turn/time distinction matters, so the two mechanisms use different units.

## Decision (as of 2026-04-12)

Signals carry a **salience** that starts at 1.0 when they arrive and decays exponentially with turns elapsed since arrival. Below a configurable floor, a signal stops appearing in notifications — but the file stays on disk until the 30-day cleanup removes it. Salience is reset to 1.0 whenever a signal is re-engaged (replied to, referenced).

The decay is parameterized by a **half-life in turns** — the number of turns after which an un-engaged signal has dropped to 50% salience. Default: 20 turns. Presentation floor: 0.3.

Re-engagement semantics preserve the "keep the active thread alive" behavior: an ongoing peer conversation keeps its signals hot as long as either side is replying.

Salience decay is a presentation-layer mechanism. It does not delete, mutate, or hide signals at the storage layer. It only gates what the peer sensor emits into notifications.

### The inward/outward gate framing

This ADR introduced the explicit language of **two gates**, and that framing is the one that landed:

| Gate | Question | Mechanism | ADR-119 role | ADR-121 role |
|---|---|---|---|---|
| Inward | "Should this *new* event fire?" | Refractory period, elevated threshold | Primary concern | n/a |
| Outward | "Should this *already-fired* signal still be shown?" | Salience decay with a floor | n/a | Primary concern |

ADR-119 and ADR-121 compose: a signal that fires in a hot conversation passes both gates — engagement approves it (inward), and salience is at 1.0 (outward, just arrived). Later, salience falls below the floor and the signal stops appearing; if something re-engages it, salience resets and it reappears. These are independent mechanisms answering independent questions against the same underlying per-subject state.

## What ADR-123 changed (2026-04-14)

The inward/outward gate framing is now first-class in `sensor-trait`:

```rust
impl EngagementState {
    pub fn should_fire(&self, current_tick: Tick, magnitude: f64) -> bool { /* inward gate */ }
    pub fn current_salience(&self, current_tick: Tick) -> f64 { /* outward gate */ }
}
```

Both query methods answer against the same `EngagementState` but consult different portions of the curve — refractory for the inward gate, salience decay for the outward gate. A `Curve::ActionPotential` (attend sensors) has both an inward refractory multiplier and a trivial outward salience (always 1.0 — attend sensors don't use the outward gate yet). A `Curve::Exponential` (ways) has a trivial inward gate (multiplier 1.0) and a meaningful outward salience that decays smoothly. A `Curve::Flat` has an outward step function. Curves with both sides populated — e.g., a combined curve where refractory throttles bursts AND salience fades the fired guidance — are possible but not currently used.

### The ways side shipped first

Ways' `session::way_fire_outcome()` calls `EngagementState::current_salience(current_tick)` and returns `FireOutcome::Suppressed` when salience exceeds `REFIRE_FLOOR = 0.5`. Every way declares a `curve:` block in its frontmatter; at fire time the engine loads or creates per-way state at `{session_dir}/way-engagement/{way_id}.json`, queries the outward gate, and records the fire if it's allowed. This is ADR-121's original "outward gate fades, floor gates presentation" model — just with token position as the axis instead of turn count, because ways' host-addressing unit is tokens.

### The attend side is still deferred

Attend's peer signals still present indefinitely. The planned implementation when it lands:

1. Each signal file (or a sidecar) carries an `arrival_turn` field.
2. A live turn counter reads the session transcript to derive `current_turn`.
3. The peer sensor's emit path constructs an `EngagementState::new(Curve::Exponential { half_life: turns_to_half })` per signal, records a fire at `arrival_turn`, and queries `current_salience(current_turn)`. Below the floor, the signal is suppressed from presentation.
4. Re-engagement (reply/reference) bumps `arrival_turn` to the current turn, resetting the outward gate.

The same engine, the same curve variant, a different tick axis. No code the ways side doesn't already exercise.

### Why the ways side came first

Two reasons. First, the ways side had a concrete acute pain point (the 25% global threshold from ADR-104 didn't scale with per-way cadence differences) while attend's presentation aging was a latent issue only visible in very long sessions. Second, the ways side forced the progression-axis argument — because token position is chunky and wall-clock-second burst detection breaks on it, the unification effort needed the curve engine to be unit-agnostic from the start. Attend's signal salience would have been a second instance of the same pattern in the same unit; building it first wouldn't have surfaced the chunky-axis problem.

## Consequences

### Positive (preserved, now concrete on the ways side)

- **Graceful fade, not a cliff.** Exponential decay means no hard "at turn 45 everything drops" behavior. Holds for ways' token-axis implementation today and for attend's turn-axis implementation when it lands.
- **Short sessions unaffected.** A way with `half_life: 30000` tokens never re-fires in a short session that accumulates less than 30k tokens — equivalent to the "first-turn signals are still ~66% salient at median session end" guarantee.
- **Symmetric with ADR-119.** Inward and outward gates composed against the same state. This is now codified in `EngagementState::should_fire` / `current_salience` rather than being a design intention.
- **Turn-based matches mental model** (for the attend application). Still true; still deferred.

### Positive (added by ADR-123)

- The inward/outward distinction is now a code contract, not just a design intention. A caller that wants the inward gate calls `should_fire`; a caller that wants the outward gate calls `current_salience`. Both are cheap to query against the same state.
- The curve shape is a first-class parameter. `Curve::ProgressiveStaircase` — which this ADR described as an alternative not-yet-pursued — is now a trivially-available shape for any way that wants declared re-fire deltas instead of smooth exponential fade.

### Negative

- **Mechanism complexity.** Unchanged. Per-subject state with curve-driven decay is more machinery than a flat presentation rule.
- **Turn counter dependency** (for the attend application, when it lands). Still deferred, still coupled to Claude Code's JSONL transcript format when implemented.
- **Re-engagement detection is heuristic.** Still deferred.
- **Threshold tuning.** The 20-turn default from the session survey above is still the tentative value for the attend application. `REFIRE_FLOOR = 0.5` is the current ways equivalent.
- **Asymmetric rollout.** Shipping on one tool before the other means the documentation (this ADR, plus `docs/attend-and-monitor/salience.md`) describes a design that's only half-implemented. Readers need to be told which half.

### Neutral

- **Orthogonal to disk retention.** The 30-day disk cleanup is unchanged. Salience decay sits entirely above it, in the presentation path — still true.
- **Configurable.** All parameters will live in attend config following the ADR-115 overlay pattern when the attend side lands; all ways parameters live in per-way frontmatter today.

## Alternatives Considered (2026-04-12)

### Hard cutoff at N turns

Reject signals older than turn `current - N`. Simpler to implement (no per-signal salience), but produces a cliff. Rejected. This aligns with ADR-123's choice to make `Curve::Flat` an opt-in rather than a default — step functions are valid first-class shapes when a way genuinely wants them, but not the default.

### Time-based aging (minutes or hours)

Use `u2u_median` (~82s) as one-turn-equivalent and compute salience from wall clock. Cheaper because no turn counter needed. Rejected because turn pacing varies too much at this granularity. The ADR-123 progression-axis framing generalizes this: each tool picks its axis based on its host's addressing unit. Wall clock is right for attend's multi-observer peer signals; turns are right for attend's per-signal presentation aging; tokens are right for ways' per-way re-fire gating.

### Linear decay over N turns

`salience = max(0, 1 - (turns_since / N))`. Even simpler than exponential. Rejected because linear decay implies "every turn contributes equally to aging," which misrepresents the real curve. Exponential matches "relevance halves repeatedly," which is closer to how attention actually works. ADR-123 codified this by making `Curve::Exponential` the primary outward-gate shape.

### Per-category half-lives

Different signal types (peer message vs. build event vs. git change) could age at different rates. Tempting but premature — start with one half-life across all signal types. ADR-123 made this free on the ways side (each way declares its own curve) but preserved the single-global-parameter shape on the attend side, to keep the deferred implementation scope minimal.

### Subsume into ADR-119's refractory machinery

Action potential already tracks engagement dynamics. Why not extend the refractory curve to cover presentation aging too? Rejected because the two mechanisms answer different questions. ADR-123 validated this rejection by making `should_fire` and `current_salience` distinct query methods against the same `EngagementState` — two gates, one state, cleanly separated rather than conflated.

## Implementation Plan

### Ways side (shipped via ADR-123)

1. ✅ `EngagementState::current_salience(tick)` query method against `Curve::Exponential`
2. ✅ `REFIRE_FLOOR` constant (default 0.5) in `tools/ways-cli/src/session.rs`
3. ✅ Per-way state persistence as `{session_dir}/way-engagement/{way_id}.json`
4. ✅ `FireOutcome` enum gating way firing on salience floor
5. ✅ Every way declares an explicit `curve:` block in its frontmatter
6. ✅ `ways list` and `ways rethink` visualize per-way re-fire distances from each curve's `refire_delta(REFIRE_FLOOR)`

### Attend side (still deferred)

1. Per-signal `arrival_turn` metadata (side table or sidecar — keep wire format stable)
2. Live turn counter parsing the session JSONL
3. Per-signal `EngagementState<Curve::Exponential>` in `sensor-peers`
4. Presentation gate consulting `current_salience(current_turn) >= floor`
5. Re-engagement detection (reply via `re:` field from ADR-120, content-match as best-effort)
6. Config plumbing for `attention.half_life_turns` and `attention.presentation_floor`

## References

- **[ADR-123](ADR-123-firing-dynamics-progression-axis-unification.md)** — progression-axis unification; where the inward/outward framing landed in code.
- **ADR-113** — attend active awareness module; the disclosure governor.
- **ADR-114** — attend as insistent way trigger type; integration for signal handlers.
- **ADR-119** — action potential engagement model; the inward gate.
- `docs/attend-and-monitor/salience.md` — the implementer-facing description of the planned attend application.
- `docs/hooks-and-ways/context-decay.md` — the presentation-economics model that motivates the outward gate on the ways side.
