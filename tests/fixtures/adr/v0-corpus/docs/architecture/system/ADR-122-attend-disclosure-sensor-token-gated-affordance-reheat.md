---
status: Accepted
date: 2026-04-13
deciders:
  - aaronsb
  - claude
related:
  - ADR-104
  - ADR-113
  - ADR-117
  - ADR-119
---

# ADR-122: Attend disclosure sensor — token-gated affordance reheat

## Context

`attend` exposes interactive surfaces Claude must know how to use — `attend send`, `--to`, `--focus`, `--broadcast`, focus groups, mark-read semantics, the "silence is a valid reply" norm, and the load-bearing rule that `attend run` belongs to Monitor rather than Bash. None of this is re-taught at runtime. Claude learns it from `SKILL.md` at session start and may have forgotten the actionable shape by turn 80. When a peer message arrives via Monitor, the notification reports *that* something happened but not *what affordances exist* to act on it. The user pays an inference turn for Claude to either improvise, ask, or — most commonly — silently fail to engage with a channel that was supposed to be two-way.

This is the same shape of problem ADR-104 solved for ways: instructional context that mattered at session start is not reliably present three reflection windows later. Ways answered with token-gated re-disclosure — track the token position of the last disclosure, fire again when drift crosses a threshold. Attend has the same problem on a different surface (interactive messaging affordances) and the same answer should apply, reusing the ADR-104 model rather than inventing a parallel mechanism.

The substrate separation principle from ADR-113 constrains the form of the answer. Sensing when Claude has drifted far enough from a teaching to need it again is cheap integer arithmetic — it does not need inference. The teaching itself, the thing that gets emitted into Claude's conversation, pays the token cost every time it fires, so it has to be terse by design. The cheap substrate (a sensor) decides when; the expensive substrate (Claude's next turn) only sees the result when surfacing is warranted.

The design note at `docs/design-notes/attend-messaging-disclosure-reheat.md` walks through the full design iteration. This ADR captures the decision in the form the rest of the system can cite.

## Decision

Attend gains a new workspace crate, `sensor-disclosure`, implementing `sensor_trait::Sensor`. The sensor's signal is **token distance since last disclosure for each registered component**, rather than environmental change. Its shape is otherwise ordinary: poll returns observations, the accumulator aggregates, the engagement and governor machinery from ADR-117 and ADR-119 apply unchanged, and each observation becomes one stdout line wrapped by `emit::emit_batch`.

**The sensor is a sensor in the ordinary sense of the word.** It is not a new architectural layer, not an emit-pipeline stage, not a framework extension. This framing is load-bearing: no new cross-sensor coupling, no new trait, no pre-emit hook. The disclosure batches alongside peer-message arrivals *emergently* — when both sensors are ready to disclose in the same governor window, the existing batch assembly puts them in one `emit_batch` call and Monitor groups them into one notification via its 200ms batching window. When timing does not align, the disclosure sensor fires standalone at its own tick. Either outcome delivers the teaching to Claude.

**Poll mechanics.** Each poll shells out once to `ways context --json` — the same integration pattern `sensor-context` already uses — and reads the `tokens_used` field. For each registered component, the sensor walks an in-memory `HashMap<Component, u64>` ledger:

- **No marker present** (first encounter in this attend process) → emit the component's disclosure text at full magnitude, stamp the baseline.
- **Marker present, distance ≥ 25% of the model's current context window** → emit the disclosure text, re-stamp the new baseline.
- **Otherwise** → no observation for that component this poll.

The threshold is the same `REDISCLOSE_PCT = 25` value ways uses for its own re-disclosure. One dial governs both; the reheat semantics are symmetric across the two substrates.

**State is in-process memory only.** The ledger is a field on the sensor struct. It is not persisted to `~/.cache/attend/state/`, not to the `/tmp/.claude-sessions-.../` signal tree, not anywhere. This is a deliberate departure from ways' file-backed markers and from attend's existing `state.rs` checkpointing. Any persistent ledger inherits a "stale state causes havoc" failure mode: a crashed attend leaves a marker file behind, the next `attend run` reads it, and suppresses teaching Claude needs. The only defensible way to guarantee clean-restart semantics is to never write the marker in the first place. Because the ledger data is tiny (one `u64` per component) and non-load-bearing, losing it on restart is the *desired* behavior — a fresh attend process re-teaches every component on its first poll, which is the right response to an unexplained process lifecycle event.

**Component registry from day one.** Even though `messaging` is the only component at introduction, the sensor is structured as a registry — a `Component` enum with a variant per component, each variant carrying a static ID and the bundled markdown body via `include_str!`. Adding a component (`focus`, `scenes`, `peers-introspection`, etc.) is an enum variant plus a markdown file, not a refactor. Per-component state is keyed in the shared ledger. The registry is also the natural seam for per-component semantics if any component ever needs them — today they all follow the same trigger rule.

**Content is bundled into the binary.** Each component's disclosure text lives at `sensor-disclosure/src/disclosures/{component}.md`, included via `include_str!`. Same posture as ways body files authored alongside their logic: it ships with the code that uses it, it cannot go missing at runtime, it is versioned in git alongside the sensor that depends on it. The alternative — reading from a path under the skill directory — was rejected because it adds a missing-file failure mode without compensating benefit.

**Emit format uses the existing `[attend sensor=disclosure priority=high]` prefix.** An earlier draft of the design note committed to matching ways' re-disclosure header pattern for visual-identity reasons. Sensor-native framing made that commitment costly (it would require special-casing `emit::emit_batch` for one sensor, against the grain of how every other attend sensor presents itself). The decision is therefore uniform with attend sensors rather than uniform with ways re-disclosures. The content carries a `reheat:` self-identifying tag on the first line of each disclosure, which is sufficient without header-format rhyme.

**Content is terse by construction.** Each disclosure block pays its own token cost every time it fires. The messaging block is authored in ways-body density: one-line assertions with terse rationale, no prose expansion. Every line stays under ~200 characters, comfortably inside Monitor's ~400-character stdout line ceiling. A sister fix in the same PR (`sensor-peers` chunking) applies the same constraint to peer messages, ensuring the ceiling is respected end-to-end rather than only on disclosure output.

## Consequences

### Positive

- **First-run disclosure is automatic.** The first poll after `attend run` start finds every component with a missing marker and fires them all. No separate startup-banner injection, no special-case code in `main.rs`. The sensor framework handles it.
- **Reheat semantics match ways.** One dial (`REDISCLOSE_PCT = 25`), one model, one mental frame for "Claude is being reheated on something." Reinforcement rather than two parallel protocols.
- **Zero framework change.** The sensor plugs into the existing slot machinery in `tools/attend/src/sensors/mod.rs` exactly like every other sensor. `SensorSlot`, `AdaptiveInterval`, `DeltaAccumulator`, `EngagementState`, and the disclosure governor all apply unchanged. Validated end-to-end in a live multi-agent session during PR development.
- **Clean restart by construction.** A restarted attend process starts with an empty ledger and re-teaches every component on first poll. No stale-state failure mode. No recovery logic needed. The cost of forgetting is exactly one re-teach, which is the correct response to an unexpected process restart anyway.
- **Batching with peer messages is emergent.** When the disclosure sensor and `sensor-peers` both cross their thresholds in the same governor window, the main loop's batch assembly groups them into one `emit_batch` call, and Monitor groups them into one notification via the 200ms batching window. The "teach-alongside-arrival" behavior we wanted comes for free from the existing scheduling without any coupling code.
- **Extensible.** Adding a second component (`focus`, `scenes`, `peers-introspection`) is an enum variant plus a markdown file. No refactor, no cross-sensor plumbing, no framework changes.

### Negative

- **Subprocess call on every poll.** Reading `tokens_used` via `ways context --json` is a process spawn per poll (default 60s base interval, 20s minimum). This is the same cost `sensor-context` already pays against the same command — we are doubling the rate, not inventing it. In aggregate the cost is on the order of 1–2 process spawns per minute during active sessions, which is within the noise floor of the existing sensor loop.
- **Content fidelity depends on author discipline.** The disclosure text is bundled Rust source via `include_str!` and pays tokens every fire. Drift toward prose expansion is a real failure mode. Mitigation: explicit terse-by-construction standard documented in this ADR and in the design note; line-length audit as part of code review.
- **No per-component threshold override yet.** All components share the 25% threshold. If some component ever needs more aggressive reheat (e.g., a critical affordance that Claude forgets faster than token drift predicts), there is no knob. Add only if real usage shows the need.
- **First-run-always-fires is a property.** A mid-session `attend run` restart re-teaches every component on first poll, even if the current working window already contains the teaching from an earlier run. Not a bug — it is the clean-restart guarantee — but it is an observable side effect worth naming: attend restarts cost a re-disclosure per registered component, billable to the user's token budget.

### Neutral

- **The `state.rs` persistence model is deliberately not extended.** Context-percentage tracking and git state stay persistent (they have different failure characteristics), but disclosure markers do not. Future components in this sensor inherit the in-memory constraint by default.
- **Design note is retained as companion prose.** The design note at `docs/design-notes/attend-messaging-disclosure-reheat.md` stays as the detailed walkthrough of the four-way iteration that landed on the sensor-native framing. This ADR cites it rather than re-deriving its argument.
- **Governor interaction is unchanged.** The disclosure sensor participates in the existing disclosure governor the same way any other sensor does. No new rate-limiting knobs, no new cooldown windows, no new global budget.

## Alternatives Considered

### A persistent ledger under the session signal tree

Store the disclosure marker as a file at `/tmp/.claude-sessions-{uid}/{session_id}/attend-disclose/{component}/.value`, mirroring ways' own `way-tokens/` layout one directory over. Symmetric with ways, visible to other session-aware tooling, survives attend process restarts within a session.

Rejected because the "stale file causes havoc" failure mode cannot be defended cheaply. A crashed attend leaves a marker behind; the next `attend run` reads it and suppresses teaching Claude actually needs. The ledger data is tiny and non-load-bearing — losing it on restart is the desired outcome, not a cost. In-process memory is strictly simpler and strictly safer for this specific data.

### Extend `state.rs` `StateSnapshot` to carry disclosure markers

The existing `StateSnapshot` already persists `disclosed_thresholds` and `reply_hint_shown` to `~/.cache/attend/state/{session_id}.state`. Adding a `disclosure_ledger` field would reuse the existing checkpoint infrastructure.

Rejected on the same failure-mode grounds as the persistent-file alternative, plus an aesthetic objection: `StateSnapshot` fields are about context-percentage ways-style thresholds. Piling the disclosure ledger into that struct muddies the semantics and couples two different kinds of state into the same serialization format.

### Emit-pipeline interceptor / pre-flush hook

Add a hook in `emit::emit_batch` that inspects the outgoing batch and, if it contains a peer-message event and the messaging ledger threshold is exceeded, prepends a disclosure block before flushing. This was the first implementation direction explored.

Rejected because it is a bespoke emit-pipeline stage that does not fit the existing sensor framework. Every other awareness capability in attend is a sensor. Making disclosure an emit-layer special case would introduce a new architectural seam, complicate the sensor framework's mental model, and commit us to a second mechanism that runs on a different substrate than the rest.

### `ReactiveSensor` trait for cross-sensor coupling

Introduce a new trait alongside `Sensor` for sensors that need to read other sensors' observations this tick before producing their own. Disclosure would be the first implementation, running last in poll order and inspecting a "this-tick's-observations-so-far" buffer exposed by the tick scheduler.

Rejected as speculative abstraction. A reactive-sensor trait is a meaningful commitment — it changes the sensor contract, the scheduler loop, and the way sensor authors reason about tick ordering. It is not justified by a single use case. Moreover, once the disclosure signal was reframed as "token distance since last disclosure" rather than "reaction to a peer event," the cross-sensor coupling disappeared: the disclosure sensor has its own interoceptive signal (ledger distance) and does not need to observe other sensors at all. The batching-with-peer-messages behavior emerges from the existing scheduling without any coupling.

### Topic-drift or semantic-drift modeling

Trigger reheat not on raw token count but on some measure of *semantic* drift — e.g., whether the current conversation topic has shifted significantly from the topic at the last disclosure. Conceptually appealing because token count is an imperfect proxy for "has Claude forgotten the teaching."

Rejected on the substrate-separation principle from ADR-113 and the discipline named in `docs/attend-and-monitor/authoring-sensors.md`: sensors report facts the framework can verify, not guesses the sensor produced. Token count is measurable from the transcript with no inference cost. Semantic drift is an inference problem the sensor cannot solve cheaply, and if solved by an embedding model the embedding itself introduces new failure modes and a new dependency. "Simulated judgment" is the wrong substrate for the thing this sensor is meant to do.

### Persistent storage cap with TTL

Compromise between in-memory and fully persistent: write disclosure markers to disk but expire them automatically after some wall-clock window (e.g., 1 hour). Reduces stale-state risk by ensuring markers do not outlive the process state they describe for long.

Rejected for four independent reasons:

1. **TTL does not close the stale-state failure mode.** During the TTL window, a crashed-and-restarted attend still inherits suppression state it cannot explain. The window only narrows the exposure; it does not eliminate it. The in-memory solution eliminates it entirely by construction.
2. **It introduces a file-I/O failure surface the in-memory design avoids entirely.** A disk-backed ledger has to handle read errors, write errors, disk-full conditions, filesystem corruption, concurrent-access races between multiple attend instances, and the inevitable edge cases around partial writes during a crash. The ledger data is a `HashMap<Component, u64>` — three to five entries in the extensible case. Accepting all of that failure surface to persist a handful of integers is a bad trade.
3. **TTL tuning is a configuration surface with no defensible default.** "How long should my disclosure markers live?" is a question with no good answer: short TTLs defeat the purpose (the next `attend run` re-teaches anyway), long TTLs reintroduce the stale-state problem we are trying to avoid, and middle values have no principled basis. The in-memory solution has no such tuning knob — the lifetime of the ledger is exactly the lifetime of the attend process, which is the only defensible scope.
4. **It would be inconsistent with the rest of attend's persistence story.** `state.rs`'s existing `StateSnapshot` does not TTL its checkpoints — they persist indefinitely and are only overwritten on the next clean write. Introducing TTL semantics exclusively for disclosure markers would create a new and inconsistent pattern the rest of the codebase does not share. The simpler path is to keep the existing persistence discipline (opt in deliberately, never TTL) and put the disclosure ledger cleanly outside that discipline entirely.

## Validation

This ADR is promoted from a working sketch that has already been exercised end-to-end:

- All 57 workspace unit tests pass, including 4 in `sensor-disclosure` covering extraction, registry, and formatting.
- The sensor registers in the attend tick loop alongside `context`, `processes`, `git`, and `peers`.
- First-run disclosure fired correctly on sensor startup in three separate runs (initial rebuild, content-tightening rebuild, chunking-fix rebuild), each confirmed via live Monitor notification.
- Peer-conversation validation run between two Claude instances exercised the messaging affordances surface in real time, with the disclosure block arriving cleanly as one batched Monitor notification in both directions.
- The chunking fix in `sensor-peers` was validated by requesting a deliberate ~800-char message from the peer; it arrived as three correctly-numbered chunks with clean word boundaries.

The design note's "promote to ADR after the model holds up across a few real sessions" threshold is met.
