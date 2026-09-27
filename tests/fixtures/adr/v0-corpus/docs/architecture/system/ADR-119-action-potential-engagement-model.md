---
status: Superseded
date: 2026-04-12
revised: 2026-04-14
deciders:
  - aaronsb
  - claude
related:
  - ADR-113
  - ADR-118
  - ADR-123
superseded_by: ADR-123
---

# ADR-119: Action Potential Engagement Model

## Status: Superseded by ADR-123

**Superseded 2026-04-14.** The underlying decision — model agent engagement on the neuronal action potential so refractory periods produce natural disengagement from diminishing-value stimuli — is still load-bearing and shipping in production attend. The specific implementation shape described below (linear per-minute decay, time-windowed burst detection, `EngagementState` owning `Instant` timestamps, `step_multiplier` that scales the peak multiplier per fire past threshold) has been replaced by the shared curve engine introduced in [ADR-123](ADR-123-firing-dynamics-progression-axis-unification.md).

Concretely, what changed:

- **Linear decay → exponential half-life.** The old `current_multiplier` decayed `peak - elapsed_min × decay_per_minute` and clamped at 1.0. The new `Curve::ActionPotential` decays `1.0 + (peak - 1.0) × 0.5^(delta / multiplier_half_life)`. Attend's yaml field `decay_per_minute` is preserved for back-compat and converted to `multiplier_half_life` at load time via `ln(0.5) / ln(1 - rate) × 60`. In the load-bearing first few minutes post-burst the two shapes match closely; at the tail they diverge (the new curve never quite reaches 1.0, while the old linear one reached rest and clamped).
- **Time-windowed burst detection → event-count burst detection.** The old model counted fires within the last `burst_window` seconds. The new model counts fires whose exponential contribution to the multiplier hasn't decayed past an epsilon. For attend on wall-clock seconds the practical difference is negligible; for ways on chunky token-position ticks the new model is the only one that works at all. See ADR-123 Decision 2 for the full argument.
- **Per-fire scaling peak → fixed ceiling.** The old model computed `peak = 1 + steps × step_multiplier` where `steps = burst_count - burst_threshold + 1`, so additional fires past threshold kept raising the peak. The new model uses `peak_multiplier = 1 + step_multiplier` (= 2.25 at defaults) as a fixed ceiling. The scaling rarely activated in practice and the flat ceiling is simpler to reason about.
- **`Instant`/`Duration` → `Tick`/`TickDelta`.** The engine is now unit-agnostic. Attend interprets ticks as wall-clock seconds via `sensor_trait::epoch_secs()`; ways interprets them as token position. The engine does not know which.
- **Shared crate.** The engine lives in `sensor-trait::engagement` (and `sensor-trait::curve`) and is consumed by both attend's `SensorSlot` and ways' `session::way_fire_outcome`. Pre-ADR-123, attend had its own `EngagementState` in `sensor-trait` and ways had nothing of the kind — firing was a flat `token_distance_exceeded` step. Now it's the same engine in both tools.

The biology framing, the "party problem" motivation, the urgency-escape pattern, and the per-peer auto-grouping extension are all still load-bearing and carry over intact. The implementation details below are historical — they describe what was accepted on 2026-04-12, not what runs today. For the current implementation see `docs/attend-and-monitor/engagement.md` and [ADR-123](ADR-123-firing-dynamics-progression-axis-unification.md).

## Context

attend's disclosure governor uses linear cooldowns and rate windows to manage notification frequency. This prevents flooding but doesn't model the natural dynamics of productive engagement. An agent responding to peer messages has no signal that engagement value is declining — it will keep responding at the same threshold indefinitely, leading to unbounded context spend on diminishing-value conversations.

The current model:
- **Flat threshold**: a stimulus either meets the emission threshold or doesn't, regardless of recent activity
- **Linear cooldown**: fixed time between disclosures, no relationship to engagement history
- **No refractory period**: an agent that just finished a burst of peer messaging can immediately start another

This creates the "party problem" — agents can burn context on extended peer conversations with no natural disengagement signal. The human has to intervene or the context window runs out.

## Decision (as of 2026-04-12)

Model agent engagement after the neuronal action potential. The biological signal has properties that map directly to productive agent behavior:

### The Action Potential Shape

```
    Engagement
    (magnitude)
        ^
   +30  |        * peak
        |       / \
        |      /   \
        |     /     \
    0   |    /       \
        |   /         \
  -55   |--*           \          threshold
        | stimulus      \
  -70   |.................\___*___........  resting
        |                refractory
        +--------------------------------> time
```

### Phases

1. **Resting state** — agent at baseline awareness. Sensors poll, observations accumulate. No urgency.
2. **Stimulus & threshold** — an observation accumulates magnitude. Sub-threshold stimuli decay without triggering engagement. Only stimuli crossing threshold fire a response.
3. **Depolarization** — rapid engagement. Active responding. Should not be suppressed.
4. **Peak** — maximum engagement value. Highest information density.
5. **Repolarization** — diminishing returns. Continued engagement on the same topic yields less new information per context token spent.
6. **Refractory period** — after a burst of engagement, the threshold temporarily rises. The agent resists re-engaging with the same stimulus category. Urgent new stimuli (high magnitude) can still break through.

### Absolute vs relative refractory

- **Absolute** (first N seconds after burst): no disclosure from this sensor regardless of magnitude.
- **Relative** (decay window): disclosure possible but requires elevated magnitude. Casual follow-ups don't fire; urgent signals do.

### Urgency escape

Directed messages (sent with `--to <project>`) start at base magnitude 7.0 — above the elevated threshold even during burst refractory. This preserves urgency discrimination without special-case logic.

### Per-peer auto-grouping

sensor-peers pairs the refractory model with a per-peer magnitude boost: messages from peers who've sent multiple messages within `peer_activity_window` get 1.75× (2nd) or 2.5× (3rd+) their base magnitude. The boost lifts active conversation partners above the elevated threshold while uninvolved peers stay below it. Conversation topology emerges from observed traffic rather than explicit group configuration.

## What ADR-123 changed (2026-04-14)

The decision above stands. What changed is the shape of the state machine that implements it:

- `EngagementState` is now generic over a `Curve` variant. Attend uses `Curve::ActionPotential { burst_threshold, peak_multiplier, absolute_refractory, multiplier_half_life }`.
- The tick axis is supplied by the caller. Attend passes `sensor_trait::epoch_secs()` (wall-clock seconds); everything else the engine does is unit-agnostic.
- Burst detection is event-count based, not tick-windowed. This costs nothing for attend and unlocks the entire ways integration.
- The engine exposes `should_fire(tick, magnitude)`, `record_fire(tick, magnitude)`, `current_salience(tick)`, `current_multiplier(tick)`. Attend's `SensorSlot` calls these through `in_absolute_refractory(tick)` and `effective_threshold(base, tick)` helpers that preserve the pre-ADR-123 call-site shape.
- A sibling outward-gate consumer (ways) runs the same engine with a different curve (`Curve::Exponential`) on a different axis (token position). The unification argument is in [ADR-123 Decision 4](ADR-123-firing-dynamics-progression-axis-unification.md#4-ways-tick-unit-host-addressing-not-a-decay-theory).

### Attend yaml ↔ runtime mapping

The yaml keys in attend's engagement config are stable. At load time they map onto the curve variant:

| yaml key              | runtime parameter                                  |
|-----------------------|----------------------------------------------------|
| `burst_threshold`     | `burst_threshold`                                  |
| `step_multiplier`     | `peak_multiplier = 1.0 + step_multiplier`          |
| `absolute_refractory` | `absolute_refractory` (seconds)                    |
| `decay_per_minute`    | `multiplier_half_life = ln(0.5)/ln(1-rate) × 60`   |
| `burst_window`        | *no runtime effect* (DEPRECATED — flagged by `attend config lint`) |
| `peer_activity_window`| consumed directly by sensor-peers                  |

### What did NOT change

- The action potential framing (biology analogy, refractory semantics, urgency escape, auto-grouping).
- `attend tune`'s session survey → config derivation. It still emits `burst_window`/`decay_per_minute` in the pre-ADR-123 field names; the runtime converts at load time.
- `EngagementConfig` in `attend::config`, with all six yaml knobs preserved for back-compat.
- The disclosure governor (ADR-113) and focus groups (ADR-118) compose with the engagement gate the same way.

## Consequences

### Positive (preserved)

- Natural disengagement — agents stop engaging when returns diminish, without rules
- Burst tolerance — rapid-fire engagement during productive phases is not suppressed
- Urgency discrimination — truly urgent signals break through refractory
- Self-regulating — no human intervention needed for the party problem
- Biologically grounded — the model is well-studied, predictable, intuitive

### Positive (added by ADR-123)

- Single source of truth for firing dynamics. The math lives in one crate. Drift between attend and ways is structurally impossible.
- Exponential decay shape matches attention fade more closely than the old linear approximation
- Event-count burst detection is robust to axis granularity — same engine works for attend's smooth seconds and ways' chunky tokens
- Curve-as-parameter enables progressive disclosure and flat step curves for ways that want them

### Negative (from 2026-04-12)

- **Complexity** — more state per sensor. ADR-123 didn't reduce this; it shared it across tools.
- **Tuning** — refractory parameters still need empirical calibration. `attend tune` is a first pass; `ways tune` (deferred) would apply the same discipline to the ways side.
- **Opaque** — harder for users to understand why an agent isn't responding to a message (elevated threshold). Partially mitigated by `ways list` showing per-way re-fire distances; no equivalent yet for attend's refractory state in `attend status`.

## Implementation Status

The ADR-119 acceptance landed in attend via the original `EngagementState` in `sensor-trait` (linear decay, time-windowed burst detection, `Instant`-keyed). That implementation was retired in the 2026-04-14 ADR-123 work and replaced by `EngagementState` backed by `Curve::ActionPotential`. The present-day implementation ships:

- `sensor_trait::engagement::EngagementState` with serde derives for per-session persistence
- `sensor_trait::curve::Curve::ActionPotential` with event-count burst detection
- `sensor_trait::epoch_secs()` as the canonical attend tick source
- `attend::config::EngagementConfig` with yaml-stable field names converted at load time
- `attend tune` for empirical parameter derivation from real session history
- `attend config lint` / `--fix` to surface and remove the deprecated `burst_window` yaml key

### Not yet implemented (unchanged from 2026-04-12)

- Refractory state visible in `attend status` output (current sensor tick log shows it; the status table does not)
- Idle/motivation sensor for intrinsic self-prompting
- Motivation sensor wiring to a reflection-overdue way
- Long-horizon empirical tuning (initial defaults are from a single session survey)

## References

- **[ADR-123](ADR-123-firing-dynamics-progression-axis-unification.md)** — progression-axis unification, curve enum, shared engine.
- **ADR-113** — attend active awareness module; disclosure governor, emission thresholds.
- **ADR-118** — focus groups; scoping which stimuli reach an agent.
- **Game AI Pro Chapter 2**: "Informing Game AI Through the Study of Neurology" — action potential diagram and neurological grounding.
- **Cognitive Frameworks paper** — cognitive economics, cheapest path = correct path.
- `docs/attend-and-monitor/engagement.md` — the current implementer-and-author-friendly explainer.
