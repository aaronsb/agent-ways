---
status: Draft
date: 2026-04-14
deciders:
  - aaronsb
  - claude
related:
  - ADR-113
  - ADR-114
  - ADR-117
  - ADR-119
  - ADR-121
---

# ADR-123: Firing dynamics — progression-axis unification for attend and ways

## Context

Two tools in this workspace implement firing dynamics independently:

- **attend** has landed the action-potential engagement model (ADR-119) as `EngagementState` in `sensor-trait`, and has a drafted salience decay for signal presentation (ADR-121). Both use wall-clock time — `Instant`, `Duration`, `decay_per_minute`.
- **ways** has a rudimentary model in `ways-cli/src/session.rs`: token-position distance as a percentage of the context window, per-way markers, epoch counters, and a fire-count counter. The suppression is a step function — `REDISCLOSE_PCT: u64 = 25` — with no curves, multiplier decay, burst detection beyond the flat counter, or re-engagement reset.

The math is identical across both tools: exponential decay, refractory-period multipliers, burst detection over a windowed history. Only the units differ — seconds for attend, token positions for ways. A naive unification would generalize the engine over a `Clock` trait (`Instant` for attend, `TokenPosition` for ways), but that smuggles time semantics into the engine where none exist.

There is a cleaner abstraction hiding under the question: **attend's wall clock and ways' token count are both instances of the same thing — a monotonic progression axis supplied by the caller.** Neither is privileged. The engine doesn't need to know what progression means; it only needs a `u64` that strictly increases. Each caller labels the axis for its own context.

This reframe matters beyond the refactor. It means:

- **Future progression axes come for free.** Turn count, commits-since-branch, bytes-written, lines-changed — any monotonic can be wired in without touching the engine. ways itself is designed to be hostable by any turn-based coding agent that exposes the necessary data; different hosts may expose different natural axes, and the engine must not assume any single one.
- **Ways' axis choice is motivated by the host's own addressing unit, not by any specific decay theory.** Firing decisions should be keyed on the axis the host uses to address what ways is injecting. For transformer-based hosts, that axis is token position — it is the unit attention uses to address content, regardless of what decay shape attention happens to apply (see Decision 4 for the full argument, including its limits). Wall clock is external to the host's addressing; turn count is a coarser aggregate; token position is the host's own unit. This generalizes: a future host that addresses content differently supplies a different axis, and the engine adapts automatically.
- **Curves become first-class.** When the engine stops owning time semantics, the shape of decay/refractory becomes a parameter instead of a built-in — which enables progressive disclosure, staircase re-firing, and other patterns that ADR-119 and ADR-121 could not express because they were each hard-coded to one curve.
- **Event-count burst detection, not tick-windowed.** A subtle consequence of progression-axis genericity: some axes are granular (wall-clock seconds), others chunky (token position can jump thousands in a single tool call). Time-windowed burst detection degenerates on chunky axes — a single event can swallow the window in one step. Burst detection therefore must be event-count based with its window defined implicitly by the decay curve itself, not by a separate tick span. This is developed in Decision 2.

Additionally, ways currently only fires predictively — against user prompts (`check-prompt.sh`), task spawns (`check-task-pre.sh`), about-to-edit file paths (`check-file-pre.sh`), about-to-run bash commands (`check-bash-pre.sh`). It has no reactive firing path. Observations like "the file that was just written is now 900 lines long" are structurally invisible. Extending into `PostToolUse` and `PostToolUseFailure` requires an evaluation model that isn't semantic embedding — it's metric predicates over observed state. Both modes should compose under the same firing-dynamics engine rather than growing a parallel track.

## Decision

### 1. Unit-agnostic progression-axis engine

The firing-dynamics engine operates over a caller-supplied monotonic progression axis. The public API uses progression-flavored types, not time-flavored ones:

```rust
pub type Tick = u64;
pub type TickDelta = u64;

pub struct EngagementState {
    curve: Curve,
    history: VecDeque<(Tick, f64)>,  // (tick, magnitude) per fire
    last_fire: Option<Tick>,
}
```

All curve-specific parameters live inside the `Curve` variant (Decision 2). `EngagementState` itself holds only the curve plus the event history. Burst detection, refractory, decay, and salience are all derived by the curve from `(current_tick, history)`.

No `Instant`, no `Duration`, no `decay_per_minute`. The word "clock" does not appear in the engine surface. Each caller supplies its own meaning:

- **attend** interprets ticks as wall-clock seconds. `epoch_secs()` replaces `Instant::now()` at call sites.
- **ways** interprets ticks as token positions. `get_token_position()` supplies ticks.
- **Future callers** interpret ticks as whatever monotonic matches their cadence.

Burst detection and decay evaluation are derived from the tick history — `current_tick - entry_tick` compared against `burst_window`, curve evaluated against tick delta. The engine cannot and does not distinguish between "two seconds" and "two tokens" — both are `2u64`, and the curve parameters are in the same unit as the ticks.

### 2. Pluggable curve as first-class parameter

`Curve` is a sealed enum — cheap to match, serializable in frontmatter, no trait-object dispatch. All decay parameters across all variants are expressed as **`half_life` in caller ticks**, for a single consistent mental model:

```rust
pub enum Curve {
    /// Smooth exponential decay. Salience = 0.5^(delta / half_life).
    Exponential { half_life: TickDelta },

    /// Action potential — event-count burst detection raises a refractory
    /// multiplier which decays back toward 1.0 over tick distance.
    ///
    /// The "burst window" is NOT a tick span. It is defined implicitly by
    /// which history entries still have non-trivial multiplier contribution
    /// (via multiplier_half_life). This is robust to chunky progression axes
    /// where a single event can advance the tick by thousands.
    ActionPotential {
        burst_threshold: usize,             // fires in recent history to trigger a burst
        peak_multiplier: f64,               // multiplier at burst peak (e.g., 1.5 or 2.0)
        absolute_refractory: TickDelta,     // hard suppression after a burst
        multiplier_half_life: TickDelta,    // half-life of multiplier decay toward 1.0
    },

    /// Explicit re-fire schedule at tick deltas with diminishing salience.
    /// Progressive disclosure as a first-class pattern.
    ProgressiveStaircase { steps: Vec<(TickDelta, f64)> },

    /// Discontinuous step: suppressed for N ticks, then fully recovered.
    /// Valid shape for ways that want all-or-nothing suppression without decay.
    Flat { suppression: TickDelta },
}

impl Curve {
    pub fn salience_at(&self, delta: TickDelta) -> f64 { /* ... */ }
    pub fn multiplier_at(&self, delta: TickDelta, history: &[(Tick, f64)], current: Tick) -> f64 { /* ... */ }
}
```

- **`Exponential`** — primary salience-decay shape. Ways' likely default once tuned.
- **`ActionPotential`** — the ADR-119 engagement model, ported via event-count burst detection (see below). Usable for attend's current sensors and for ways that want inward-gate refractory on top of outward-gate decay.
- **`ProgressiveStaircase`** — new shape. A way declares `[(0, 1.0), (15_000, 0.5), (40_000, 0.2)]` and re-fires at those deltas with the declared salience.
- **`Flat`** — explicit step function. Not a shim, not a backward-compatibility translation. A valid first-class opt-in for ways that genuinely want discontinuous behavior.

**Burst detection is event-count based, not tick-windowed.** This is the critical adaptation for chunky progression axes. Time-windowed burst detection (ADR-119's original form) assumed events progress smoothly against the tick axis. On wall-clock axes this holds. On token-position axes it does not — a single `Read` tool call can advance the tick by 5k–20k in one step, swallowing any reasonable `burst_window` in one event. Burst detection therefore windows by *event count in recent history*, with "recent" defined implicitly by the decay of the refractory multiplier itself: once an event's contribution has decayed past a small epsilon (e.g., multiplier drops back below 1.01), it no longer counts toward burst detection. Attend's time-windowed behavior is preserved as a degenerate case — short `multiplier_half_life` produces a tight effective window; chunky ways use longer `multiplier_half_life` and event count stays meaningful regardless of tick jumps.

This is why `EngagementState` holds only `(curve, history, last_fire)` — there is no separate `burst_window` field. The window is a derived property of the curve's multiplier decay, not a standalone parameter.

### 3. Inward and outward gates, cleanly separated

The engine exposes two query methods that answer different questions against the same state:

- **`should_fire(current_tick, stimulus_magnitude) -> bool`** — the inward gate. "Should this new event be allowed to fire?" Consults the refractory portion of the curve. This is the question ADR-119 answers for attend sensors.
- **`current_salience(current_tick) -> f64`** — the outward gate. "Has the last-fired guidance faded enough that re-injection is warranted?" Consults the decay portion of the curve. This is the question ADR-121 answers for attend signals.

ways' current flat `redisclose` conflates these. The unified engine separates them cleanly so pre- and post-firing decisions consult the same state and curve but ask different questions. `Curve::Flat` collapses them into the same answer (binary suppressed/allowed), preserving current semantics for ways that don't opt in to a richer shape.

### 4. Ways' tick unit: host addressing, not a decay theory

Ways' tick is token position for transformer-based hosts. The justification is the host's addressing unit, not a specific theory of how attention decays.

**The principle:** firing dynamics should be keyed on the axis the host agent uses to address what ways is injecting. For a transformer, that axis is token position — it is the unit the model uses to locate and refer to content via its attention mechanism, regardless of what decay shape attention happens to apply in practice. Wall clock is external to the host's addressing (the model cannot observe it). Turn count is an aggregate over token positions that wobbles with per-turn context density (a long think turn and a short reply both count as "one turn" but displace very different amounts of addressable context). Token position is the host's own unit.

**On RoPE specifically:** rotary positional embedding does provide a baseline long-term inner-product decay with relative distance, which is a convenient piece of theoretical grounding — it means token distance is not an arbitrary choice but corresponds to a real axis the model uses. However, this should not be overstated. Modern attention heads in trained LLMs (Claude included) are demonstrably capable of overriding RoPE's baseline decay via needle-in-a-haystack retrieval — a highly salient token at position `P - 100_000` can still receive near-full attention when the current decision depends on it. The model's attention does *not* fade on a clean exponential curve along token distance; it fades until a specific attention head decides the content is critical, at which point the effective salience spikes back up internally.

What this means for ways: token position is the *closest available proxy* for "how much context has accumulated between a way's injection and the current decision point," which is the quantity firing dynamics should track. It is not a direct model of attention decay. The firing engine assumes the guidance fades in effective impact as context accumulates; the model's actual attention mechanism may or may not honor that assumption for any specific token. That's fine — the firing decision is about *presentation economics* (when to re-inject guidance so it's freshly available), not about *modeling attention internals*.

**Generalization beyond transformers:** ways is designed to be hostable by any turn-based coding agent that exposes the needed hooks and data. Different hosts may address content differently — a sliding-window chunked-conversation agent might use chunk count; a stateless agent might only expose turn count. The principle holds: use whatever axis the host uses to address injected content. Token position is the right answer for transformer hosts; it is not the right answer for all possible hosts. The engine's unit-agnosticism is what makes this portability possible.

**For attend:** wall clock is correct because attend steers external timing — peer-conversation cadence, build events, ambient awareness — which lives outside the host's token space entirely. Attend's wall-clock axis is correct for attend's domain for the same reason ways' token-position axis is correct for ways' domain: each axis matches the addressing unit of the thing being steered.

There is a deeper reason wall clock earns its meaning in attend specifically — one that explains *why* ways cannot simply adopt the same axis even if it wanted to. **Wall clock only becomes a meaningful coordinate when there is more than one observer.** A single agent running its own monotonic token counter has a single progression axis, a 1-D line; there is no second axis to project onto, and wall clock adds no information that token position does not already encode. Ways is single-observer — one session, one monotonic token stream — so wall clock is superfluous.

Attend introduces the second observer. Each peer has its own internal progression (its own session, its own token count, its own turn stream) that is disconnected from every other peer's. Peer A's "token position 5000" and peer B's "token position 3000" are not comparable — they live in different coordinate systems with different origins. The *only* axis that is shared across peers, and therefore the only axis on which their events can be meaningfully compared, is wall clock. "Peer A said X at t=100s, peer B said Y at t=102s" places both events in a common frame; "peer A at its own position 5000, peer B at its own position 3000" does not, because the positions are on non-overlapping number lines.

This is a dimensionality argument: two disparate monotonic counts, when compared in the same space, require an additional shared axis to form a coordinate system in which their relationship can be expressed. Wall clock is that shared axis — not because time is metaphysically special, but because it is the coordinate frame that is guaranteed to be common across all peers regardless of their internal progression. Attend's wall-clock axis is not arbitrary; it is the *unique* axis that multi-peer coordination requires. In a single-observer system, that requirement does not exist, and wall clock collapses to a redundant label on the one monotonic that already exists.

This is the clean division: single-observer systems need one progression axis (whatever the host uses internally — token position for transformer-hosted ways); multi-observer systems need wall clock on top as the shared coordinate frame that lets disparate internal progressions be compared. Both choices are emergent from the observer topology, not from any preference about units.

### 5. Reactive firing via `postcheck.sh`

Hooks are extended with `PostToolUse` and `PostToolUseFailure` matchers. A new hook script — `hooks/ways/check-post.sh` — is wired to these events. Each way may ship an optional `postcheck.sh` alongside its `macro.sh`:

```
hooks/ways/softwaredev/code/quality/
├── quality.md
├── macro.sh          (existing)
└── postcheck.sh      (new, optional)
```

On `PostToolUse` / `PostToolUseFailure`, `check-post.sh` walks ways with a `postcheck.sh`, runs each with `tool_response` as stdin, and treats exit 0 as "request firing." The firing request then flows through the standard inward gate — the engine's `should_fire` consults the way's curve and current refractory state. Postcheck reactive requests are not privileged over predictive matches; both go through the same gate.

Reactive firing uses the observed post-state, not intent. A `postcheck.sh` can check file size after a write, test exit codes after a test run, `gh pr` status after a merge, or any other metric its way cares about. The evaluation is metric-predicate style — cheap, deterministic, side-effect-free — not semantic re-embedding.

The two modes compose without any extra machinery: a way that fires predictively at turn T (inward gate passes, salience = 1.0) will not re-fire reactively at turn T+1 unless salience has decayed below the re-injection floor. The engine already handles this via the curve query; the reactive path just adds another set of match sources.

### 6. Shared-crate home and refactor strategy

The firing-dynamics core lives in `sensor-trait` (or migrates to a renamed `firing-dynamics` crate if the scope outgrows the current name — naming question decided during implementation). Both attend and ways-cli depend on it.

The refactor touches three crates in sequence:

1. Rename progression types inside `sensor-trait` — `Instant`/`Duration` → `Tick`/`TickDelta` (both `u64`). Factor the curve enum. Adapt attend's internal uses to go through the renamed API.
2. Port attend's call sites (`SensorSlot::poll`, `ready_to_disclose`, `current_multiplier`, etc.) to supply `epoch_secs()` as the tick source. Existing attend parameters map onto `Curve::Exponential` (for signal salience) and `Curve::ActionPotential` (for engagement). Semantics preserved exactly.
3. Wire ways-cli to the shared crate. Per-way `EngagementState<Curve>` replaces the current flat `token_distance_exceeded` check. Tick source is `get_token_position()`. Existing `redisclose: N` frontmatter is parsed as `Curve::Flat { suppression: N }` for backward compatibility.

### 7. Frontmatter schema migration — no shims

Way frontmatter gains a required `curve:` field. The existing `redisclose: N` field is **removed from the schema entirely**. There is no shim layer, no dual-parser, no silent translation of old syntax. Existing ways are migrated to explicit `curve:` declarations as an explicit step of the implementation plan, and the old `redisclose` parser is deleted.

```yaml
curve:
  type: Exponential
  half_life: 50000   # tokens
```

Or:

```yaml
curve:
  type: ProgressiveStaircase
  steps:
    - [0,      1.0]
    - [15000,  0.5]
    - [40000,  0.2]
```

Or (explicit step function, for ways that want discontinuous behavior):

```yaml
curve:
  type: Flat
  suppression: 15000   # tokens
```

**Why no shim:** shims are carryovers from older, pre-unified thinking and carry a translation burden that obscures the actual choice each way is making. Forcing explicit `curve:` declarations during migration is a one-time cost that leaves every way in the codebase with a single, self-documenting firing model. The translation layer is tech debt the moment it ships. Every way must declare its curve; the migration step is part of the refactor, not a deferred clean-up.

### 8. Empirical tuning: `ways tune`

A new `ways tune` subcommand mirrors `attend tune`. It surveys recent sessions via `~/.claude/stats/events.jsonl` (which already captures way-fire events via `log_event` in `session.rs` and the `inject-subagent.sh` logging path), computes per-way cadence statistics, and suggests curve parameters grounded in the user's actual usage. `--apply` rewrites the relevant `curve:` entries in frontmatter or a centralized overrides file.

Tuning parameters come from real session distributions, not guesses — the same discipline ADR-119 used when it sized attend's parameters to "Claude's actual turn cadence, not biological neuron kinetics."

## Consequences

### Positive

- **Single source of truth for firing dynamics.** The math lives in one crate. Drift between attend and ways is structurally impossible.
- **RoPE-aligned semantics for ways.** The firing engine operates on the same axis the model's attention decays on. Firing decisions are mechanistically matched to how the model treats the injected content.
- **Progressive disclosure as a first-class pattern.** Not a workaround, not a special-case hack — a named curve variant that any way can opt into.
- **Reactive firing composes cleanly.** Post-tool-use evaluation uses the same inward/outward gates as predictive firing. No parallel system, no new state to reconcile.
- **Empirical tunability.** `ways tune` grounds curve parameters in the user's actual session distribution. Defaults are starting points, not final answers.
- **Future axes for free.** Turn count, commit count, line count — any future trigger surface supplies a monotonic and picks parameters in its own unit. No engine change.
- **Inward/outward separation preserves ADR-119/121's clarity argument.** "Should this new event fire?" and "Should this already-fired signal still be visible?" are different questions, answered against the same state but through different queries.

### Negative

- **Refactor footprint spans three crates.** `sensor-trait`, `attend`, `ways-cli`. The rename from time-flavored to progression-flavored types touches attend's entire engagement path. This must land as a coherent change or a carefully staged sequence — partial adoption leaves the code confused about what "tick" means.
- **Curve enum adds one match layer.** Cheap, but not free. Each `salience_at` / `multiplier_at` is a dispatch per call.
- **Tuning defaults are guesses until real session data is surveyed.** Initial ways parameters will be rough — the calibration pass after landing is where the real defaults come from.
- **Schema migration is a required step, not optional.** Every existing way must be rewritten to declare `curve:` explicitly before the new parser can land. This is one-time work with no graceful rollout — the old and new parsers cannot coexist. The upside is zero shim tech debt; the downside is one high-risk migration commit.
- **Exponential parameter conversion at the rename boundary is non-trivial.** Attend's existing parameters are expressed as `decay_per_minute` rates; converting them to `half_life` in ticks requires `half_life = ln(0.5) / ln(1 - rate_per_minute)` then a unit conversion into seconds. For `decay_per_minute = 0.1`, the half-life is `ln(0.5) / ln(0.9) ≈ 6.58 minutes ≈ 395 seconds`. **This is not `0.1 / 60`** — that would only be correct for linear decay; exponential decay requires the compounding formula. Every attend parameter needs recomputation through this formula at the rename boundary, and the migration must validate behavior is preserved (not just syntactically, but numerically).

### Neutral

- **Not backward-compatible — and intentionally so.** `redisclose: N` is removed. Every existing way migrates to explicit `curve:`. This is tech-debt-free by construction; the ADR commits to one coherent schema rather than a dual-parser shim.
- **Crate naming question deferred.** The firing-dynamics core lives somewhere — either `sensor-trait` (kept as a home) or a renamed/extracted crate. Decision falls out of implementation, not ADR.
- **Reactive firing can land in stages.** The progression-axis unification and the `PostToolUse` hook extension are independent work items that can sequence separately if staging helps. This ADR establishes both as a single architectural direction; the implementation can still split along natural seams.

## Alternatives Considered

### `Clock` trait generic over `Instant` and `TokenPosition`

Parameterize the engine over a `Clock` trait with associated types for `Time` and `Duration`. Each tool supplies a `Clock` impl. Rejected because this keeps time semantics inside the engine — the type names themselves ("Clock", "Time", "Duration") imply wall-clock reasoning, and the engine has no business reasoning about wall clock for ways. The progression-flavored rename is the same refactor without the conceptual leak.

### Separate implementations per tool

Keep attend's engagement model in `sensor-trait`, add a parallel model to `ways-cli`. Rejected because the math is identical; two implementations would drift, and the code reuse Aaron surfaced in this conversation is real and worth capturing.

### Turn count as ways' axis

Use the number of user turns as ways' progression unit, matching ADR-121's choice for attend signals. Rejected because token position is mechanistically matched to the transformer's attention decay (RoPE) and turn count is not. A think turn and a short reply both count as one turn but displace different amounts of context; token position is the exact measure.

### Wall clock for ways

Use wall-clock seconds for ways' axis, matching attend. Rejected because the model's attention mechanism has no wall-clock dimension. Firing decisions should be made on an axis the model actually observes. Wall clock is external to the thing ways is shaping.

### Keep flat `redisclose`, add reactive firing only

Land `postcheck.sh` and `PostToolUse` hooks without the dynamics unification. Rejected because flat suppression cannot express the inward/outward gate separation that ADR-119 and ADR-121 demonstrated is necessary — and because the reactive firing path needs the same inward gate to avoid spam-firing on every tool call.

### LLM-based reactive evaluation via `type: prompt` hooks

Use Claude Code's `"type": "prompt"` hook as a PostToolUse filter. Run an LLM pass on the tool result and ask which ways to fire. Rejected as the default path because of cost per tool call. Reserved for specific high-value hooks (likely `PostToolUse:Task` on subagent returns, where the tool call is expensive enough that one more LLM eval is noise). The primary reactive path is `postcheck.sh` metric predicates — cheap and deterministic.

### Tick-windowed burst detection

Use `burst_window: TickDelta` to scope which history entries count toward a burst, matching ADR-119's original shape. Rejected because progression axes with chunky advancement (token position in ways) can jump thousands of ticks in a single event, swallowing any reasonable window in one step and breaking burst detection. Event-count-based windowing with the implicit bound defined by the decay curve's multiplier collapse is robust to axis granularity — it works identically for attend's smooth seconds and ways' chunky tokens.

### Backward-compatible `redisclose → Flat` shim

Keep `redisclose: N` in the schema as sugar that translates to `Curve::Flat { suppression: N }`. Rejected on Aaron's direction: shims are carryovers from older, pre-unified thinking. Forcing an explicit `curve:` migration is a one-time cost that leaves the codebase self-documenting. A translation layer obscures what each way is actually declaring and would need to be cleaned up eventually anyway.

## Implementation Plan

1. **sensor-trait rename pass.** Introduce `Tick: u64`, `TickDelta: u64`. Replace `Instant` / `Duration` with `u64` throughout `EngagementState`. Remove `decay_per_minute` and `burst_window` as engine concepts — they move into `Curve::ActionPotential` as `multiplier_half_life` and are derived from event-count windowing.
2. **Curve enum.** Factor the existing burst/multiplier/decay math into `Curve::ActionPotential` (with event-count burst detection), `Curve::Exponential`, `Curve::ProgressiveStaircase`, and `Curve::Flat`. Expose `salience_at` and `multiplier_at` as the two queries. All decay across all variants is parameterized by `half_life`.
3. **attend parameter conversion.** Convert attend's existing `decay_per_minute` values to `multiplier_half_life` in ticks using `half_life_seconds = (ln(0.5) / ln(1 - rate_per_minute)) × 60`. Recompute every engagement parameter; do not simple-divide. Validate preserved behavior via existing refractory tests before renaming anything else.
4. **attend migration.** Adapt `SensorSlot` and its consumers to the renamed API. Tick source wraps `SystemTime::now()` → seconds-since-epoch as `u64`. Verify refractory and salience behavior is unchanged end-to-end on a real session replay.
5. **ways frontmatter migration (required, no shim).** Rewrite every existing way's frontmatter to declare explicit `curve:`. Ways with `redisclose: N` become either `Curve::Flat { suppression: N }` (if N was chosen for step-function reasons) or `Curve::Exponential { half_life: N }` (if smooth decay is wanted). Remove the `redisclose` field from every way and from the schema.
6. **ways-cli integration.** Per-way `EngagementState` in `session.rs`. Tick source is `get_token_position()`. Replace `token_distance_exceeded` with `curve.salience_at(delta) >= floor`. Delete the old `REDISCLOSE_PCT` constant and its consumers.
7. **Frontmatter schema update.** Extend `frontmatter.rs` to parse the new `curve:` block. Delete the `redisclose` parser entirely — no dual-parser. Lint-ways validates the new schema and errors loudly on any leftover `redisclose:` field.
8. **Hook wiring.** Update `check-prompt.sh`, `check-task-pre.sh`, `check-file-pre.sh`, `check-bash-pre.sh` to consult the engine's inward gate before injecting. Stamp tick on fire via the renamed API.
9. **Reactive firing path.** Add `PostToolUse` and `PostToolUseFailure` to `settings.json`. New `check-post.sh` walks ways with `postcheck.sh`, runs each, pipes through the inward gate.
10. **`ways tune`.** Subcommand that surveys `events.jsonl`, computes per-way cadence, suggests curve parameters. `--apply` writes them into frontmatter.
11. **Empirical calibration.** After landing, run `ways tune` against real session data and commit the calibrated defaults as the production starting point.

## Open Questions

- **Crate home.** Generalize `sensor-trait` in place, or extract a new crate? Lean: keep in `sensor-trait` until the scope clearly outgrows the name. Renaming is a follow-up, not this ADR.
- **Curve dispatch.** Enum (chosen here) vs trait object. Enum wins on serialization and cheap matching; trait object would be needed only if third-party curves matter. Not yet.
- **Default curve for ways with no `curve:` field after migration.** After the migration step deletes `redisclose`, the schema could either (a) require every way to declare `curve:` explicitly and error on omission, or (b) provide a sensible default (e.g., `Curve::Exponential { half_life: <25% of context window in tokens for the active model> }`). Lean: (a). Explicit over implicit, matches the "no shim" directive.
- **Whether `ProgressiveStaircase` ships in the initial implementation or lands as a follow-up once `Exponential` and `ActionPotential` are proven portable.** Lean: ship it in the initial implementation because it demonstrates that the curve-as-parameter shape actually buys something beyond refactoring.
- **Validation test for attend parameter conversion.** The exponential-decay conversion formula is straightforward, but validating that the renamed attend produces numerically equivalent refractory/salience curves on real session replays is the actual acceptance criterion. How is that validated — side-by-side simulation, golden trace, or live comparison? Decide during implementation.
- **Epsilon for "event has decayed out of burst consideration".** Event-count burst detection needs a small threshold below which a history entry stops contributing. `multiplier > 1.01` is a reasonable first pick but arbitrary. Empirical tuning via `ways tune` should inform the final value.

## References

- **ADR-113** — attend active awareness module; origin of the disclosure governor.
- **ADR-114** — attend as insistent way trigger type; the integration point.
- **ADR-117** — sensor crate extraction and feature flags; precedent for cargo-workspace splits.
- **ADR-119** — action potential engagement model; the inward gate, currently shipping in attend.
- **ADR-121** (Draft) — salience decay for signal presentation; the outward gate for attend signals.
- **Cognitive Frameworks paper** — cognitive economics principle (`cheapest path = correct path`); the reason ways wants model-aligned decay rather than a parallel steering layer.
- **Rotary Positional Embedding (RoPE)** — Su et al., *RoFormer: Enhanced Transformer with Rotary Position Embedding* (arXiv:2104.09864) — the mechanism that makes token position the correct axis for ways.
