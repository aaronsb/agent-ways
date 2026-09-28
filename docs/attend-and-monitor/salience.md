# Salience — the outward-gate signal aging mechanism

This page covers the **presentation-layer aging** mechanism: how a signal's visibility in the conversation fades as the progression axis advances, even while the signal file remains on disk. The shape is the forgetting curve applied to notifications — relevance decays exponentially with quiet, and re-engagement resets it, the same spacing logic spaced-repetition systems use to decide when something needs showing again. It's the cousin — not the opposite — of attend's inward-gate engagement model in [`engagement.md`](engagement.md). Both sides of the gate share a single engine; this page is the outward-side explainer.

**Status.** The framing comes from [ADR-121](../architecture/attend/ADR-121-salience-decay-for-signal-presentation-turn-based-exponential.md). The mechanism it describes was unified with attend's inward gate in [ADR-123](../architecture/ways/ADR-123-firing-dynamics-progression-axis-unification.md): both now consume the same `sensor_trait::Curve` type with the same `salience_at(delta)` query. **Ways was the first concrete implementation**, shipping cross-tool via ADR-123. **Attend's sensor-peers application now ships too** (issue #22) — each peer signal passes through a per-id `EngagementState<Curve::Exponential>` before emitting, so aged backlog fades without being deleted and threaded replies (`re:<id>`) reset the parent's salience. This page describes the decision and points at both production implementations as canonical references. <!-- adr-cite-ignore -->

## The problem the outward gate solves

Without an outward gate, every signal that ever arrived keeps trying to be shown. Disk retention (30 days, see [`signals.md`](signals.md)) prevents bulk accumulation at geological scale. Engagement refractory ([`engagement.md`](engagement.md)) prevents runaway firing at the inward edge. But neither addresses the middle case: a signal that *was* emitted, *is* still referenced, and *keeps getting re-presented* every time the presentation path runs — long after it has stopped being load-bearing.

In a three-hour session, a signal that arrived in the first few minutes is still being surfaced after two hours of unrelated work — even though by then the cursor has moved over an order of magnitude more context. The signal isn't stale in any disk-cleanup sense (it's hours old, not days), but it's stale in a *relevance* sense. The cursor has moved on.

ADR-121's framing, preserved verbatim under ADR-123: **signals need presentation-layer aging, measured in the progression axis, decaying smoothly.** The only change from ADR-121 to ADR-123 is that "measured in turns" is now "measured in whatever tick unit the caller supplies" — seconds for attend, tokens for ways. <!-- adr-cite-ignore -->

## Inward and outward gates, one engine

ADR-119 and ADR-121 originally framed engagement and salience as two mechanisms. ADR-123 exposed them as two queries against the same state: <!-- adr-cite-ignore -->

| | Inward gate (engagement) | Outward gate (salience) |
|---|---|---|
| **Question** | "Should this new event fire?" | "Has the last-fired guidance faded enough that re-injection is warranted?" |
| **Direction** | Stimulus → decision to fire | Already-fired state → decision to re-present |
| **Engine query** | `should_fire(tick, magnitude)` | `current_salience(tick)` |
| **What it consults** | The curve's multiplier portion (raised threshold, refractory) | The curve's salience portion (smooth decay toward 0) |
| **Resets on** | A successful fire (starts relative refractory) | A re-fire (salience returns to 1.0) |

They are different questions against a shared `EngagementState`. `Curve::Flat` collapses them into the same binary answer, preserving the original ways `redisclose` step-function semantics for ways that opt into it. `Curve::Exponential` separates them cleanly: the inward multiplier is always 1.0 (no refractory), and the outward salience smoothly decays as `0.5^(delta/half_life)`. `Curve::ActionPotential` is the attend case: a non-trivial inward multiplier alongside an outward salience that stays at 1.0 until a re-fire.

A reader who already understands engagement from [`engagement.md`](engagement.md) should read this page as "the other thing the curve knows how to answer." The engine does not hold two models. It holds one.

## The shape of the outward curve

```
    salience
         ^
     1.0 |*
         | \
         |  \
         |   \
     0.5 |    \  ← half_life (delta where salience halves)
         |     \_
         |       \___
     0.0 |          \______________________
         +------------------------------------> tick delta
```

The salience curve is a smooth exponential: it starts at 1.0 the moment a fire is recorded and decays by half for every `half_life` ticks of quiet. No rebound, no re-engagement mid-decay unless the engine receives a fresh `record_fire(tick, magnitude)` — which resets salience to 1.0 and starts the curve over from the new tick.

The query semantics:

```rust
// current_salience returns the outward curve's value at the current tick.
pub fn current_salience(&self, current: Tick) -> f64 { ... }
```

Callers compare this against a **re-fire floor**. Different callers pick different floors based on what "faded enough" means for their presentation surface.

## Ways' concrete application (shipping today)

Ways is the first tool that consumes the outward gate in production. The surface:

- Every way declares an explicit `curve:` in its frontmatter. Most declare `Curve::Exponential { half_life: N }` in tokens. See [ADR-123 §7](../architecture/ways/ADR-123-firing-dynamics-progression-axis-unification.md#7-frontmatter-schema-migration--no-shims).
- On each match, `session::way_fire_outcome(way, session, curve)` asks the engine whether the way should re-fire. Under the hood:

```rust
// tools/ways-cli/src/session.rs
pub const REFIRE_FLOOR: f64 = 0.5;

let state = load_engagement(way_id, session_id, curve);
let current_tick = get_token_position(session_id);
match state.current_salience(current_tick) {
    s if s >= REFIRE_FLOOR => FireOutcome::Suppressed,
    _ if state.last_fire.is_none() => FireOutcome::FirstFire,
    _ => FireOutcome::ReFire,
}
```

- On a successful fire, `session::record_way_fire` writes `record_fire(current_tick, 1.0)` into the per-way state at `{session_dir}/way-engagement/{way_id}.json`.

For ways with `Curve::Exponential { half_life: H }` and the 0.5 floor, re-fire happens at exactly delta `H` — the half-life *is* the re-fire distance. Ways' "20–30K intervals" footer in `ways list` is exactly this: the range of `half_life` values across the active way set.

Per-way visualization in `ways list` and `ways rethink` uses `Curve::refire_delta(floor)` to render each row's bar and forecast position from its own threshold, not a shared global. See [ADR-123 §2](../architecture/ways/ADR-123-firing-dynamics-progression-axis-unification.md#2-pluggable-curve-as-first-class-parameter) for the curve types and [`engagement.md`](engagement.md) for the engine's other queries.

## Attend's concrete application (shipping now)

sensor-peers consults a per-signal outward gate inside `read_signals` before each observation leaves the poll. The state lives in a small sibling module, `tools/sensor-peers/src/salience.rs`:

```rust
// tools/sensor-peers/src/salience.rs
pub struct SignalSalience {
    states: HashMap<String, EngagementState>,  // keyed by signal id
    half_life: TickDelta,                      // from attend's signals: config
    floor: f64,
}

impl SignalSalience {
    pub fn gate(&mut self, signal_id: &str, arrival_tick: Tick, now: Tick) -> bool {
        let state = self.states.entry(signal_id.to_string()).or_insert_with(|| {
            let mut s = EngagementState::new(Curve::Exponential { half_life: self.half_life });
            s.record_fire(arrival_tick, 1.0);
            s
        });
        state.current_salience(now) >= self.floor
    }

    pub fn reset(&mut self, signal_id: &str, now: Tick) { ... }
}
```

The key design points:

- **Signal id** is the filename stem — the same shape `re:<id>` references in ADR-120 threaded replies, so reply resets are direct hash lookups. No rename table, no ownership bookkeeping.
- **Arrival tick** comes from the file's on-disk mtime, not the first time this observer happens to scan the directory. A peer who joins a focus-group room mid-session sees old signals as already-decayed — the backlog-filter behavior ADR-121 designed, now working against real backlogs instead of hypotheticals. <!-- adr-cite-ignore -->
- **Re-engagement reset** runs unconditionally at scan time: if the content is a threaded 5-field signal (`re:<id>|`), the parent signal's `EngagementState` is bumped back to 1.0 at the current tick before the current signal's own gate check runs. The reset persists into the checkpoint so reconnection does not re-age the parent.
- **Checkpoint persistence** uses the existing `sensor_trait::Sensor::export_state`/`import_state` wire format, adding a new `signal_salience` row key that carries `<signal_id>\t<json>`. Old checkpoints without the key parse cleanly — the sensor just starts fresh on those signals.
- **No engine changes.** The full implementation is one new file in sensor-peers, one new config block, and ~30 lines of wiring inside `read_signals`. That's the payoff ADR-123 designed for.

### Configuration

Operators tune the gate through attend's `signals:` block:

```yaml
signals:
  half_life_seconds: 1800   # exponential decay half-life (30 min default)
  presentation_floor: 0.3   # suppress below this salience
```

Defaults are conservative first-value picks, subject to `attend tune` once survey coverage exists. `attend config lint` recognizes both keys; unknown sub-keys surface as warnings and can be removed with `--fix`.

### Why the session-length distribution still matters

ADR-121 grounded its half-life default in real session data: <!-- adr-cite-ignore -->

```
min=1  median=12  p75=45  p90=84  max=133  mean=32.6  turns
```

This was turns, not seconds, under the ADR-121 framing. Under ADR-123 the shape of the argument is the same, the axis is different: attend picks a wall-clock half-life that behaves reasonably against the same distribution when converted through the observed turn-cadence. `attend tune` already surveys turn cadence from real transcripts and derives engagement parameters; the outward-gate half-life is the natural next parameter for it to derive from the same analysis (tracked as a follow-up to issue #22). <!-- adr-cite-ignore -->

The intuition to preserve: **short sessions never see aging (nothing to prune), medium sessions see the oldest material drop out cleanly near the end, long sessions experience meaningful pruning.** Whatever wall-clock half-life attend eventually picks should produce this behavior against the live distribution, not against a one-off sweep. The 1800 s default is the placeholder — deliberately uncalibrated — until tune closes the loop.

### Replacing `mark_existing_as_seen`

The legacy startup flow called `peer_sensor.mark_existing_as_seen(focus)` before the tick loop started, pre-populating `seen_signals` with every signal file already on disk. This prevented a startup blast but was all-or-nothing — old *and* recent pre-existing signals were silenced alike. The salience gate replaces that coarse filter: every pre-existing file now flows through `read_signals`, the gate evaluates each one against its on-disk mtime, and only below-floor signals are suppressed. Recent-but-pre-existing signals (e.g., a focus-group message from 5 minutes ago at startup) now surface where they previously would not. The `mark_existing_as_seen` method was removed entirely as part of issue #22.

### Known v1 limitation: resurfacing already-presented parents

The `seen_signals` invariant still prevents the same observer from surfacing the same signal twice, so a `re:` reply that resets the parent's salience does not cause the parent to re-appear in *this* observer's notification stream. The reset *does* affect:

- Any other observer joining the same shared signal dir after the reply arrived.
- Any future session of the same observer that re-reads the shared dir from checkpoint state.

Widening the reset to also re-surface already-presented parents in the current observer is a targeted follow-up, not a v1 requirement — the primary ADR-121 win is backlog filtering on new-observer entry, which the above implementation fully delivers. <!-- adr-cite-ignore -->

## Re-engagement resets

A signal that's been replied to or referenced is a signal that's still relevant. The reset semantics are the same for attend's future implementation as for ways' current one:

- When an agent or human replies to a signal (using the `re:` threading field from ADR-120), the signal's `arrival_tick` is updated to the current tick and its salience returns to 1.0 via `record_fire`.
- When a message references a prior signal by content (best-effort matching, not yet implemented), the same reset happens.
- When a signal is re-disclosed because a new agent joins and sees the backlog, the arrival_tick for *that agent's view* resets — because the signal just entered their attention.

Re-engagement is the "thread still alive" signal. As long as either side is responding, the thread stays hot. When neither side has touched it for `half_life` ticks of quiet, salience drops below the floor and the signal stops being presented.

In ways' case, "re-engagement" is structurally different: a way re-fires when a new match brings it back to the cursor, which resets its salience from whatever it had decayed to back to 1.0. Same mechanism, same engine call, different triggering event. That's the unification working as intended.

## Why axis choice varies by tool

Salience decay is a **curve shape** concern; what delta the curve is evaluated against is an **axis choice** concern. They're orthogonal:

- Attend's axis is wall-clock seconds because attend coordinates events across multiple peers. Each peer has its own internal progression, but wall clock is the only axis guaranteed common across all peers. This is the multi-observer dimensionality argument in [ADR-123 Decision 4](../architecture/ways/ADR-123-firing-dynamics-progression-axis-unification.md#4-ways-tick-unit-host-addressing-not-a-decay-theory).
- Ways' axis is token position because ways steers a single-observer host (one session, one monotonic token stream) and token position is the unit the host uses internally to address content. The model can observe it directly via RoPE; wall clock is external to the model's attention.

The salience curve doesn't care which axis it's running against. A `Curve::Exponential { half_life: 30000 }` is 30000 whatever-the-caller-supplies. Attend reads 30000 as seconds (half an hour); ways reads 30000 as tokens (roughly one medium interaction). Both are correct for their domain because the engine is unit-agnostic.

## Alternatives rejected in ADR-121 (still valid under ADR-123) <!-- adr-cite-ignore -->

Preserved here for posterity. The arguments didn't change when the engine unified; if anything, the unification makes the rejections stronger because some of the alternatives would have blocked cross-tool sharing.

- **Hard cutoff at N ticks.** Simpler than exponential, but produces a cliff — at delta `N`, everything that was above zero drops to nothing abruptly. Exponential matches the "relevance declines gradually" intuition. (Ways still supports `Curve::Flat` as an explicit opt-in for ways that genuinely want discontinuous behavior; it's not a fallback.)
- **Linear decay over N ticks.** Treats every tick's contribution to aging as equal. The first few ticks of irrelevance matter more than the last few — exponential's "halves repeatedly" shape is closer to how attention actually works. This was a load-bearing argument for ADR-123's exponential unification of attend's old linear refractory relaxation.
- **Per-category half-lives.** Different decay rates for different signal types. Premature — start with one half-life per way or per signal class, add per-category overrides if real use demands it. `Curve::ActionPotential` with different `multiplier_half_life` values is the escape hatch if per-category ever becomes needed.
- **Subsume into engagement's refractory machinery.** Conflates the inward and outward gates, making both harder to reason about. ADR-123 keeps them as distinct queries against the same state for exactly this reason.
- **Time-based aging for ways.** Wobbles with the model's actual context consumption, which doesn't track wall clock. This is the argument that made ways pick token position, and it applies to salience decay the same way it applies to engagement refractory.

## What this page is not

- **Not the canonical architecture.** That's [ADR-123](../architecture/ways/ADR-123-firing-dynamics-progression-axis-unification.md), with [ADR-121](../architecture/attend/ADR-121-salience-decay-for-signal-presentation-turn-based-exponential.md) as the original outward-gate decision. <!-- adr-cite-ignore -->
- **Not the engine documentation.** That's `sensor_trait::curve::Curve` in source, with unit tests as the executable spec.
- **Not a parameter-tuning guide for attend.** Attend's outward-gate parameters don't exist in production yet — `attend config lint` will surface them alongside engagement parameters when sensor-peers consumes them.

## Related

- [`engagement.md`](engagement.md) — the inward-gate side of the same engine, attend-specific.
- [`signals.md`](signals.md) — disk-side retention (the time-based bulk cousin).
- [`loop.md`](loop.md) — where the presentation gate will sit in the attend tick loop when sensor-peers consumes it.
- [`configuration.md`](configuration.md) — attend's config surface; the `signals:` block lives there alongside `engagement:`.
- **ADR-121** — the salience-decay decision record. Status under ADR-123: reframed in place; ways was the first concrete realization, sensor-peers the second. <!-- adr-cite-ignore -->
- **ADR-123** — the progression-axis unification that made ADR-121's mechanism a cross-tool facility instead of an attend-local one. <!-- adr-cite-ignore -->
- **`tools/ways-cli/src/session.rs`** — the `way_fire_outcome` path is the ways-side consumer, anchored to token position.
- **`tools/sensor-peers/src/salience.rs`** — the attend-side consumer, anchored to wall-clock seconds. Mirror structure, same engine call.
