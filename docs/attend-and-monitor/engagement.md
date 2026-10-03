# Engagement — the action potential firing gate

Attend's engagement model governs *when a sensor is allowed to fire a disclosure*. The idea it implements is established: in cognitive architectures like ACT-R, base-level activation decays with disuse, and recent activity changes how easily the next stimulus gets through. Attend applies that idea per sensor, borrowing its shape from the neuronal action potential: resting baseline, rapid rise on stimulus, refractory period after a burst, gradual return to rest. The biology gives us a predictable, well-studied shape for a phenomenon we actually care about — how productive engagement with a stimulus decays naturally over time.

Engagement gates the **event lane** only: `context`, `git`, `processes` and `disclosure`. The message lane (`peers` and `keepwarm`) skips it, so a peer message or a keepwarm wake is never held by a refractory (ADR-136, see [`delivery.md`](delivery.md)).

This page covers the model in prose and diagrams, explains what each parameter does, and walks through how it interacts with the disclosure governor. The canonical architecture is **[ADR-123](../architecture/ways/ADR-123-firing-dynamics-progression-axis-unification.md)** — the progression-axis unification that moved the firing-dynamics core into a shared crate consumed by both attend and ways. This page is the attend-specific, implementer-and-author-friendly explainer for how attend instantiates that core.

## The problem engagement solves

Without engagement, attend's disclosure logic is a flat threshold. A sensor observes something, accumulates magnitude, crosses the emission threshold, fires. Rinse and repeat. This produces the "party problem": an agent responding to a lively conversation has no built-in signal that the return on engagement is declining. It will keep responding at the same threshold indefinitely until the human intervenes or the context window runs out.

The action potential model adds **per-sensor memory of recent activity**. After a sensor has fired a burst of disclosures, its effective threshold *rises* for a while, then decays back to baseline — habituation, implemented as arithmetic. High-magnitude stimuli still break through. Low-magnitude follow-ups are silently suppressed. The sensor disengages from the fading topic on its own.

Said another way: refractory isn't silence, it's *raised bar*. Nothing is censored; thresholds are just temporarily harder to cross.

## The biological analogy

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
  -55   |--*           \          ← threshold (normal)
        | stimulus      \
        |                \_____________ ← elevated threshold
        |                               (relative refractory)
  -70   |.................\___*___........ ← resting potential
        |                 ^
        |                 └── absolute refractory
        +--------------------------------> time
```

- **Resting state**: sensor at baseline, polling on schedule, accumulating nothing. Threshold is whatever the author configured (default 1.5–2.5).
- **Stimulus**: an observation arrives. Magnitude accumulates. Below threshold, it's quiet. At or above threshold, the sensor becomes a disclosure candidate.
- **Depolarization / peak**: the sensor fires. Observations reach the agent.
- **Absolute refractory**: for ~60 seconds after a burst, nothing from this sensor fires, regardless of magnitude. The agent is processing what it just received; another signal would interfere.
- **Relative refractory**: for the next several minutes, the threshold is temporarily multiplied by an elevation factor. Routine events that would normally fire get silently accumulated. Only truly high-magnitude events break through.
- **Decay**: the elevation factor decays exponentially back toward 1.0 over the configured half-life. After enough quiet time, the sensor is fully at rest again.

The biological action potential has an overshoot and hyperpolarization phase too, but attend's model is the simplified practical version: threshold rise + exponential decay, no overshoot.

## Progression axis: wall-clock seconds for attend

Attend's firing engine operates on an abstract monotonic **progression axis**. The engine does not know what a tick is — attend supplies one by convention. For attend, a tick is **one second of wall-clock time** (`sensor_trait::epoch_secs()`, which reads `SystemTime::now().duration_since(UNIX_EPOCH)`).

Why wall clock: attend steers external timing — peer conversations, build events, ambient awareness — which all live outside any single model's token space. Multiple attend instances may need to compare events across their own independent progressions, and wall clock is the only axis that's guaranteed common across all of them. This is the multi-observer case argued in [ADR-123 Decision 4](../architecture/ways/ADR-123-firing-dynamics-progression-axis-unification.md#4-ways-tick-unit-host-addressing-not-a-decay-theory).

The consequence for attend authors: **all engagement parameters are in seconds**. `absolute_refractory: 60` means 60 wall-clock seconds. Half-lives are in wall-clock seconds. If you ever see a parameter expressed as a "tick count" in the code, interpret it as seconds for attend specifically.

## Curves as first-class

ADR-123 made the firing-dynamics shape a first-class parameter rather than a built-in assumption. The engine knows four curve variants — `Exponential`, `ActionPotential`, `ProgressiveStaircase`, `Flat` — and attend uses exactly one of them: **`Curve::ActionPotential`**. This page describes that variant. The other three are used elsewhere (ways is mostly `Exponential`); they don't appear in attend's runtime.

The `ActionPotential` curve has four parameters:

```rust
Curve::ActionPotential {
    burst_threshold: usize,             // fires in recent history to trigger a burst
    peak_multiplier: f64,               // refractory multiplier at burst peak
    absolute_refractory: TickDelta,     // hard-suppression window (seconds for attend)
    multiplier_half_life: TickDelta,    // exponential decay half-life for the multiplier
}
```

The config keys predate ADR-123 (see [`configuration.md`](configuration.md)). Attend converts them at load time:

- `peak_multiplier = 1.0 + step_multiplier` — the old "peak at exactly burst_threshold" value (2.25 at defaults) becomes a fixed ceiling rather than a growing scale.
- `multiplier_half_life = ln(0.5) / ln(1 - decay_per_minute) × 60` — converts the pre-ADR-123 per-minute linear-decay rate into an exponential half-life in seconds. At the default `decay_per_minute = 0.1`, the half-life is ≈ 395 s (≈ 6.6 min).
- `absolute_refractory` and `burst_threshold` pass through unchanged.

There is no `burst_window` key: the window is implicit (see "Event-count burst detection" below). A `burst_window` left in a config file is an unknown key, which makes the whole `engagement` section fall back to the layers beneath it; `ways settings fix attend.engagement` removes it.

## Event-count burst detection

The most visible difference between the pre-ADR-123 model and the current one is how "burst" is detected.

**Before ADR-123**, a burst was "N firings within a tick-span window." Attend counted firings that fell within the last `burst_window` seconds, and the third one triggered the elevated threshold. This works for fine-grained axes like wall-clock seconds, but it breaks completely for chunky axes like ways' token-position — a single Read tool call can advance the tick by 5k–20k in one step, swallowing any reasonable window.

**After ADR-123**, a burst is "N firings whose contribution to the multiplier hasn't decayed past an epsilon." The engine asks each history entry: is your exponential contribution under `multiplier_half_life` still above ~1%? If yes, you count toward burst detection. If no, you age out. The "burst window" is an emergent property of `multiplier_half_life`, not a standalone parameter.

For attend on wall-clock seconds, this makes essentially no practical difference — `multiplier_half_life` of 395 s produces an effective burst window of ~15 min (the point at which the contribution falls below epsilon), close to the 900 s window the old model used. The unification lets ways and attend share the same engine without attend having to carry time-specific assumptions into a shared crate.

Defaults at config load:

- `burst_threshold = 3` — three fires in the live-event window triggers the refractory.
- `peak_multiplier = 2.25` — from `1.0 + step_multiplier=1.25`, the old "peak at exactly burst_threshold."
- `absolute_refractory = 60` seconds — one Claude turn of complete silence.
- `multiplier_half_life ≈ 395` seconds — derived from `decay_per_minute = 0.1`.

## Absolute vs relative refractory

```mermaid
stateDiagram-v2
    [*] --> Rest
    Rest --> Accumulating: poll returns event
    Accumulating --> FireReady: magnitude ≥ threshold<br/>AND not in refractory
    Accumulating --> Rest: quiet poll<br/>or decayed
    FireReady --> Disclosed: governor permits
    FireReady --> Accumulating: governor holding
    Disclosed --> AbsoluteRefractory: record_fire(tick, 1.0)
    AbsoluteRefractory --> RelativeRefractory: absolute_refractory<br/>seconds elapsed
    RelativeRefractory --> Rest: multiplier decayed to ~1.0<br/>via multiplier_half_life
    RelativeRefractory --> Disclosed: high-magnitude event<br/>breaks elevated threshold<br/>AND governor permits

    classDef rest fill:#2d8e5e,color:#ffffff,stroke:#4a5568
    classDef process fill:#2d7d9a,color:#ffffff,stroke:#4a5568
    classDef waiting fill:#fbbf24,color:#1a1a1a,stroke:#4a5568
    classDef boundary fill:#f6821f,color:#1a1a1a,stroke:#4a5568
    classDef core fill:#7c3aed,color:#ffffff,stroke:#4a5568

    class Rest rest
    class Accumulating process
    class FireReady waiting
    class Disclosed core
    class AbsoluteRefractory boundary
    class RelativeRefractory waiting
```

**Absolute refractory** is a hard wall. For `absolute_refractory` seconds after any firing, the sensor cannot disclose at all — not even on a maximum-magnitude event. During this window the engine's `current_multiplier(tick)` returns `f64::INFINITY`, which `in_absolute_refractory(tick)` recognizes as "gate fully closed." Events still arrive and still accumulate, but none of them fire.

**Relative refractory** is an exponentially-decaying multiplier on top of the base threshold. After the absolute window passes, the sensor's effective threshold is `base_threshold × current_multiplier(tick)`. The multiplier starts at `peak_multiplier` (2.25 at defaults) immediately after the burst and decays as `1 + (peak - 1) × 0.5^(delta / multiplier_half_life)`. Events that would normally fire at magnitude 2.0 now need ~4.5 magnitude to break through immediately after a burst, falling to ~3.2 at one half-life, back to baseline over 4–5 half-lives.

The result: natural disengagement on fading topics, preserved break-through for genuinely urgent new stimuli.

## How attend calls the engine

Attend's `SensorSlot` owns an `EngagementState` from `sensor-trait`. The runtime queries are:

```rust
// During SensorSlot::poll()
let tick = sensor_trait::epoch_secs();
if slot.engagement.in_absolute_refractory(tick) {
    // hard-blocked — don't even accumulate filtered events
    return;
}
let multiplier = slot.engagement.current_multiplier(tick);
// elevated gate during relative refractory, rest-gate (0.0) otherwise

// At the batch-disclosure point in the main loop, after governor permits
slots[i].engagement.record_fire(tick, 1.0);
```

The `record_fire(tick, magnitude)` call replaces the pre-ADR-123 `record_disclosure()` — same moment in the flow, new API shape. Magnitude is `1.0` (unit-weight) for attend; the engine supports weighted fires for callers that want them, but attend's sensors all contribute equally to the burst count.

## Per-peer boost

`sensor-peers` multiplies a message's magnitude by how often the same peer has written within `peer_activity_window` (default 900 s): ×1.0 for the first message, ×1.75 for the second, ×2.5 from the third. Since peer messages ride the message lane, the boost only raises the line's priority. It does not decide whether a message is shown: every message is shown once. The sensor counts per peer in its own sliding window rather than through the curve engine, because the boost is per peer, not per sensor.

## Tuning with `attend tune`

The default engagement parameters are sized for a typical Claude session, but they're derivable from real data. `attend tune` surveys recent sessions under `~/.claude/projects/` (the 10 most-recent active projects, 5 most-recent sessions each by default), computes percentiles on turn cycle durations (assistant → user, user → user), and proposes engagement parameters grounded in actual usage:

```
$ attend tune
[tune] surveying 34 sessions across 10 projects

=== attend tune — session survey ===
  projects surveyed:  10
  sessions parsed:    34
  turn samples:       661

  assistant → user (think time):
    median=32s  p75=106s  p90=296s
  user → user (full cycle):
    median=78s  p75=210s  p90=489s

=== derived engagement config ===
engagement:
  burst_threshold: 3     # yours, unchanged
  step_multiplier: 1.25     # yours, unchanged
  absolute_refractory: 32     # median think time
  decay_per_minute: 0.0256     # peak decays over ~2× burst-window equivalent
  peer_activity_window: 1467    # sized from u2u p90 × burst_threshold

(pass --apply to write these values to your attend config)
```

`burst_threshold` and `step_multiplier` are yours: tune reads them from your user config and derives the other three from them and the survey.

- `absolute_refractory` is the median think time, clamped to 15–300 s.
- `peer_activity_window` is the user→user p90 times `burst_threshold`, clamped to 300–3600 s.
- `decay_per_minute` is chosen so a linear relaxation from the peak would reach rest over twice that window.

Nothing is applied until you pass `--apply`, which writes only those three keys to your user-scope config through the settings writer, leaving comments and other keys as they were. Re-run it when your session rhythm changes.

Tune's arithmetic is linear and the engine's decay is exponential. At load time `decay_per_minute: 0.0256` becomes `multiplier_half_life ≈ 1611` s (~27 min). The two shapes are close in the first minutes after a burst and diverge in the tail, where the exponential never quite returns to 1.0.

## Interaction with the disclosure governor

Engagement is a **per-sensor** gate. The disclosure governor is a **global** gate. On the event lane both must permit a firing for it to happen. The message lane has its own permissive governor and no engagement gate.

- **Sensor refractory says NO** → event accumulates silently. No disclosure fires.
- **Sensor refractory says YES, governor cooldown active** → sensor is "ready" but held. The loop logs `N sensors ready but governor holding`. When the cooldown rolls, accumulated sensors fire together.
- **Both say YES** → disclosure fires. Events are emitted as Monitor notifications, governor records the disclosure, engagement records the burst count via `record_fire(tick, 1.0)`.

The governor protects against *global flood* — too many sensors all trying to fire at once. Engagement protects against *per-sensor spam* — one sensor firing too frequently on declining-value stimuli. They're complementary.

## Design rules of thumb

When should you tune these parameters vs leave defaults?

- **Leave defaults unless you've run `attend tune`.** The defaults are reasonable for a typical dev workflow. Random tweaking is unlikely to improve things.
- **Lower `burst_threshold` if you're getting spammed.** If a particular sensor is firing too often, lower the threshold to 2 so refractory kicks in faster.
- **Higher `step_multiplier` if refractory isn't strong enough.** If elevated threshold still lets chatty events through, raise from 1.25 to 1.5 (which becomes `peak_multiplier = 2.5` at runtime).
- **Longer `absolute_refractory` if you need more silence.** 60 seconds is one Claude turn. If you want a full pause between bursts, raise it to 120–180.
- **Faster decay if you want quick return to normal.** Raising `decay_per_minute` from 0.1 to 0.33 lowers the half-life from ~395 s (~6.6 min) to ~103 s (~1.7 min). Equivalently, if you're hand-editing and think in half-lives, pick the half-life directly and solve the inverse.

All tuning is via attend config, using the ADR-115 overlay pattern — user scope, project scope, or both.

## Shared engine, cross-tool

The shift in this page's framing (vs earlier versions) is that engagement no longer exclusively belongs to attend. After ADR-123, the engagement state machine — `EngagementState` with a `Curve::ActionPotential` — is a shared crate (`sensor-trait::engagement`) consumed by both attend and ways. Attend uses it with wall-clock-seconds ticks and action-potential refractory. Ways uses the same engine with token-position ticks and `Curve::Exponential` outward-gate salience. Same math, different axis, different curve variant.

For the ways-side equivalent of this page see [ADR-123 Decision 4](../architecture/ways/ADR-123-firing-dynamics-progression-axis-unification.md#4-ways-tick-unit-host-addressing-not-a-decay-theory) and the context-decay presentation-economics model in [`../hooks-and-ways/context-decay.md`](../hooks-and-ways/context-decay.md). The shared engine means any future improvement to burst detection, refractory decay, or curve shapes lands in one place and reaches both tools.

## Related

- **[ADR-123](../architecture/ways/ADR-123-firing-dynamics-progression-axis-unification.md)** — the progression-axis unification and curve-as-parameter framing
- **ADR-119** — the original action potential model (pre-unification; superseded by ADR-123 for the math, preserved for the biology-analogy framing) <!-- adr-cite-ignore -->
- [`loop.md`](loop.md) — where engagement state sits in the loop iteration
- [`authoring-sensors.md`](authoring-sensors.md) — how sensor authors design around engagement
- [`configuration.md`](configuration.md) — full config schema for engagement parameters
