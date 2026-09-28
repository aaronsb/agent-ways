---
contract: adr/v1
kind: evidence
capability: attend
status: accepted
date: 2026-04-13
deciders:
  - aaronsb
related: []
---

# ADR-400: Attend messaging disclosure with token-gated reheat

> **Cites:** ADR-104, ADR-113
> **Motivates:** ADR for attend disclosure registry *(planned, draft after working sketch)*

## What this note is

This note describes how `attend` should teach Claude how to *use* it — and, more importantly, how it should *re-teach* Claude over the lifetime of a session as the relevant instructions drift out of effective context. The reheat mechanic is spaced repetition applied to injected instructions: guidance loses influence as context scrolls past — the forgetting curve, operating over token distance rather than days — so the system re-presents it as it approaches the point of no longer being load-bearing.
It is the same problem ADR-104 solved for ways: instructional context that mattered at session start is not reliably present three reflection windows later. Ways answered with token-gated re-disclosure. Attend has the same shape of problem at a different surface — the *interactive messaging* surface — and the answer is structurally the same. Reusing the model preserves substrate separation (the cheap binary tracks token distance; the expensive substrate just reads what the cheap one decided to surface) and reinforces a single mental model for "Claude is being reheated on something."

## The problem

`attend` exposes interactive surfaces Claude must know how to use: `attend send`, `--to`, `--focus`, `--broadcast`, focus groups, mark-read semantics, when to ignore noise versus when to respond. Today, none of this is taught at runtime. Claude either learned it from `SKILL.md` at session start (and may have forgotten by turn 80) or never saw it (the skill loaded a sensor pipeline but not the reply protocol).

The cost of this gap is asymmetric. When a peer message arrives via Monitor, Claude reads the notification but may not remember *that it can reply, or how*. The notification tells Claude *something happened*; it does not tell Claude *what affordances exist to act on it*. The user pays an inference turn for Claude to either improvise, ask, or — most commonly — silently fail to engage.

The fix is to attach the affordance description to the moment Claude needs it: when a message arrives. This is the same situated-learning bet the ways system makes — teach at the moment of relevant action, not in an orientation document. And to attach it again, later, when enough of Claude's working window has scrolled past that the previous teaching is no longer load-bearing.

## The reheat principle, applied

Three properties carry over from ADR-104 verbatim:

1. **Re-disclosure threshold is token distance, not turn count.** The model from `ways-cli/src/session.rs` (`REDISCLOSE_PCT = 25`) is the threshold. One dial governs both ways' and attend's reheats; attend reads its current token position via subprocess call to `ways context --json` the same way `sensor-context` already does.
2. **Markers are cheap and non-load-bearing.** A `HashMap<Component, u64>` in the disclosure sensor's own struct. No disk, no session signal tree — clean-restart semantics by construction.
3. **The cheap substrate decides, the expensive substrate only reads.** Token tracking, threshold math, and emission all happen in the sensor-tick binary. Inference only sees the result.

**The disclosure lives inside a new sensor — not a special pipeline stage.** A `DisclosureSensor` implementing the existing `Sensor` trait from `sensor-trait`, just like `sensor-git` and `sensor-context`. Its signal is unusual (token distance since last disclosure for each registered component) but its shape is ordinary: poll returns observations, accumulator aggregates, governor gates, batch emits. This framing is load-bearing — it means no new architectural layer, no cross-sensor coupling, no pre-emit hook, no framework change. It also means batching with a peer-message arrival is **emergent**: when the disclosure sensor and peer sensor both cross their thresholds in the same governor window, they naturally co-emit in one Monitor notification. When the timing doesn't align, the disclosure fires on its own tick, alone.

## The DisclosureSensor

A new `sensor-disclosure` workspace crate, sibling to `sensor-git`, `sensor-peers`, `sensor-context`, and `sensor-processes`. Its `DisclosureSensor` implements the `Sensor` trait from `sensor-trait`. Nothing about the attend tick loop, emit pipeline, governor, or engagement state changes — the sensor plugs into the existing slot machinery exactly like every other sensor.

**State.** `HashMap<Component, u64>` on the sensor struct. Component ID → the `tokens_used` value at which this component last emitted. No disk, no `state.rs`, no session signal tree. Clean-restart semantics follow automatically: a new attend process starts with an empty map and re-teaches every component on its first opportunity.

**Poll.** On each tick the sensor is scheduled, it shells out once to `ways context --json` and reads the `tokens_used` field. For each registered component it walks the ledger:

- **No marker present** (first encounter after `attend run` start) → emit the component's disclosure text at full magnitude, stamp the baseline.
- **Marker present, distance ≥ 25% of the current model's context window** → emit the disclosure text at full magnitude, stamp the new baseline.
- **Otherwise** → no observation for this component.

Magnitude is fixed per observation (matching `sensor-context`'s discrete-threshold pattern), so the accumulator crosses the emission threshold immediately when a component qualifies. "Urgency slowly building up" is the conceptual progress toward the threshold; the mechanical fire is a single discrete step once it crosses.

**Interval.** Base `60s`, min `20s`, decay threshold `3` — roughly the same cadence as `sensor-context`, because the subprocess call is the same operation and the observable state changes at the same pace. Adaptive ramp-up isn't load-bearing here (token position only moves forward, never backward) but the existing knobs apply uniformly.

**Why the sensor framing is the right one.** Three properties fall out for free:

1. **First-run disclosure is automatic.** The first poll after `attend run` start finds every component with a missing marker and fires them all. No separate startup-banner injection, no special-case code in `main.rs`.
2. **Batching with peer messages is emergent.** When the disclosure sensor and `sensor-peers` both mark themselves `ready_to_disclose()` within the same governor window, the main loop's batch assembly puts them in the same `emit::emit_batch` call and Monitor groups them in its 200ms window. When timing doesn't align, the disclosure fires standalone at its own tick. Either outcome is fine — the teaching reaches Claude.
3. **The disclosure governor already applies.** The governor is attend's interruption-cost control — calm technology enforced as a rate limit, because every notification costs an inference pass — and no new rate-limiting knob is needed. If the global disclosure budget is tight, the disclosure sensor waits its turn like everything else.

**Attend process session id.** The attend process resolves its session id via `own_session_id()` at `tools/attend/src/main.rs:1381` (process ancestry, no env var), but `DisclosureSensor` has no use for it — nothing is keyed by session id because nothing is written anywhere. The sensor is scoped to the *attend process*, which is strictly narrower than the session itself.

## Visual identity

The disclosure sensor emits through the standard `emit::emit_batch` path, so the wrapping header is `[attend sensor=disclosure priority=high] <text>` — attend's canonical sensor format, not ways' re-disclosure header pattern. An earlier draft of this note committed to "the same visual identity as ways re-disclosures," but sensor-native framing makes that commitment costly (it would require special-casing emit formatting for one sensor, against the grain of how every other attend sensor presents itself).

The decision is therefore: **uniform with attend sensors, not uniform with ways re-disclosures.** The `sensor=disclosure` name is still self-identifying — Claude can learn a single rule ("a `sensor=disclosure` emit is attend teaching me an affordance") without collapsing it into the ways reheat model. Consistency across attend sensors beats visual rhyme with ways re-disclosures when the cost of the rhyme is an emit-pipeline carve-out.

This becomes a reviewable decision when the design note is promoted to an ADR. If experience shows Claude benefits from the visual rhyme, the sensor can gain a custom emit header as a follow-up.

## Component registry

Even though `messaging` is the only component for now, the implementation is a registry from day one — an enum variant per component, each carrying a static component ID and the bundled markdown content as associated data. The reasoning:

- The next component is not hypothetical. `focus` (how Claude joins/leaves attention groups), `scenes` (how Claude switches between defined sensor pipelines), and `peers-introspection` (how attend models the broader session graph) are all plausible next entries.
- The cost of the registry today is an enum and a short iterator. The cost of refactoring after the second component is added is greater than that.
- Per-component state lives in the same `HashMap<Component, u64>` ledger inside the sensor. Adding a component is: add an enum variant, add a markdown file to `sensor-disclosure/src/disclosures/`, include it via `include_str!`.

The earlier qualifying-event-predicate concept is gone — in the sensor framing, every component follows the same trigger (first-encounter OR distance ≥ threshold), so no per-component predicate is needed. Components that want different semantics in the future can opt out of the shared poll and run their own logic inside the same sensor.

## Source of the instructional text

Each component's block is **bundled into the `sensor-disclosure` crate** via `include_str!("disclosures/{component}.md")`. Same posture as way bodies authored alongside their logic: it ships with the code that uses it, it cannot go missing at runtime, it is versioned in git alongside the sensor that depends on it. The alternative — reading the markdown from a path under `~/.claude/skills/attend/disclosures/` — was considered and rejected because it adds a missing-file failure mode without any compensating benefit (the skill directory is not edited at runtime by users, and rebuild is cheap).

The content of the messaging block is the *hard part of this work*. It must teach Claude how to use the messaging surface in a way that is worth its tokens. Concretely it should cover:

- That replies are sent with `attend send <text>` and that `--to`, `--focus`, and `--broadcast` exist.
- The semantics of focus groups and when to use them versus `--to`.
- When silence is the right answer (acknowledged-but-silent, per the cognitive-loop note).
- When to mark a message read versus letting it expire naturally.
- What *not* to do — most importantly, never to invoke `attend run` from Bash; it belongs to Monitor.

The block should be terse. It is paying its own token cost every time it fires; verbose teaching is self-defeating. Aim for a structure closer to ways body text than to SKILL.md prose: assertion-density first, examples only where the assertion is ambiguous without them.

## Non-goals

To be explicit about what this note does *not* propose:

- **No multi-component disclosure batching today.** Only `messaging` exists at first. The registry exists so the second component is cheap to add, not so multiple components can fire on a single event. A single disclosure per emit is the contract until experience demands otherwise.
- **No turn-based or wall-clock thresholds.** The substrate is token distance, identical to ways. Wall-clock and turn count are not the right unit because the cost being managed (Claude forgetting an affordance) is a function of context drift, not seconds or turns.
- **No global rate limiting layered on top of the existing disclosure governor.** The governor from the cognitive-loop note already enforces emit cadence on the underlying sensor events; attaching a disclosure to a passing event does not increase event count, only event width. No new governor knob is required.
- **No persistent storage of disclosure markers, anywhere.** The ledger is in-process memory and dies with the attend process. Forgetting on restart is a feature, not a bug — a new attend process is an opportunity to re-orient Claude, and the bundled markdown is the source of truth that travels with the binary. The `state.rs` persistence model (used for context-percentage tracking and git state) is deliberately not extended to hold disclosure markers, because the "stale file causes havoc" failure mode cannot be defended against cheaply.
- **No replacement for SKILL.md.** SKILL.md remains the authoritative reference Claude reads at skill load. The disclosure block is a runtime reheat of the parts of that reference Claude needs *during interactive use*, not a substitute for the full document.

## Implementation work order

Work proceeds on the existing `feat/attend-messaging-reheat` branch as:

1. **Create the `sensor-disclosure` crate.** Workspace member alongside `sensor-git` / `sensor-peers` / etc. Implements `Sensor` from `sensor-trait`. Contains the `Component` enum, the `HashMap<Component, u64>` ledger, the `ways context --json` subprocess call, and per-component bundled markdown under `src/disclosures/`.
2. **Author `sensor-disclosure/src/disclosures/messaging.md`.** This is the load-bearing content step; treat it with the same care a ways body deserves.
3. **Register the new sensor in `attend`.** Add to the workspace default feature set and slot it into the sensor registration in `tools/attend/src/main.rs` alongside the other sensors.
4. **Promote this design note to an ADR draft** once the working sketch validates the model, with the ADR cross-citing this note rather than re-deriving its argument.

## References

- [ADR-104](../ways/ADR-104-token-gated-way-re-disclosure-for-long-context-windows.md) — Token-gated way re-disclosure (the model this note reuses)
- [ADR-113](ADR-113-attend-active-awareness-module.md) — `attend`: active awareness module
- [Cognitive loop and the awareness layer](../practice/ADR-600-cognitive-loop-and-the-awareness-layer.md) — the broader frame this work sits within
