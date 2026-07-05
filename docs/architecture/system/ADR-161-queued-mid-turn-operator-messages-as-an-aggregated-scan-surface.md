---
status: Proposed
date: 2026-07-05
deciders:
  - aaronsb
  - claude
related:
  - ADR-160
  - ADR-155
  - ADR-130
  - ADR-123
---

# ADR-161: Queued mid-turn operator messages as an aggregated scan surface

## Context

The way matcher's highest-value surface is the **operator's own message**: operator
input is low-quantity and high-quality — a deliberate "let's use the mermaid way to
diagram this" carries far denser intent than any tool-use command or agent-authored
text. A relevant way firing on that message is the most common and most useful
matcher interaction.

Live observation of the ADR-160 late-interaction matcher surfaced that this surface
is, in practice, **not matched** when the operator most naturally produces it. Two
independent causes compound, both confirmed by read-only diagnostics against a real
session transcript (session `56ebbbc1`):

1. **Mid-turn messages never reach the scan hook.** `UserPromptSubmit` is a
   *once-per-turn* lifecycle event — it fires when a prompt starts a turn, not for a
   message the operator types while the agent is already working. Such a message is
   **queued** and flushed into the running turn at the next LLM pause. In the
   transcript it is recorded as a `type: "queue-operation"`, `operation: "enqueue"`
   entry (with `.content` and a `.timestamp`), and it *never* becomes a
   `type: "user"` prompt entry. `check-prompt.sh` therefore never scans it. This is
   documented Claude Code behavior, not a defect in our matcher: the matcher path
   (`scan/mod.rs` → `late_interaction::run`) is correct and would fire if handed the
   surface. Confirmed: top-of-turn operator messages *are* scanned (e.g.
   `meta/subagents` keyword-fired on a top-of-turn "subagent" message), while every
   mid-turn queued message in the session produced no fire.

2. **A lone fragment is too sparse for late interaction.** Operators type intent as a
   *burst of short messages*, not one paragraph. Run through the matcher, a single
   queued fragment ("as another test … we should use the mermaid style way to author
   a document about the flow …") reports **"late-interaction unavailable — surface
   too sparse to chunk"** and falls back to the single-vector gate (ADR-160's
   <2-chunk fail-safe), where the relevant way (`documentation/mermaid`, ~0.4) does
   not clear the calibrated gate. **Concatenating** the consecutive fragments of the
   same burst makes it a ≥2-chunk surface, at which point late interaction runs and
   fires: `documentation/mermaid` peak 0.477 / share 0.190 / confirm 0.579 and
   `softwaredev/visualization/diagrams` 0.450 / 0.233 / 0.526 — both admitted on
   **share**, which a single fragment cannot accumulate.

The consequence: fixing only cause 1 (scanning queued messages) would still not fire
the way the operator expected, because cause 2 drops each lone fragment to the
single-vector fallback. The fix must address both.

## Decision

Add a **queued-message scan lane** that treats the burst of mid-turn operator
messages as a first-class, aggregated matcher surface — restoring operator↔agent
symmetry that ADR-160 already intends (the matcher is channel-agnostic; only the
delivery of this surface was missing).

1. **Hook: `PostToolUse`.** It fires at the LLM pauses where queued messages flush,
   and it runs *before the agent's next action* — so a way admitted here (e.g. the
   mermaid way) can still steer the work the operator asked for, not merely annotate
   it after the fact. (`Stop` would be too late to steer the current turn.) This
   rides the existing `check-post.sh` PostToolUse dispatcher (ADR-123 Decision 5).

2. **Read the transcript, select queued operator text.** Find
   `type: "queue-operation"`, `operation: "enqueue"` entries with `timestamp` newer
   than a per-session **scan mark**. This is a precise, stable selector — genuine
   operator text only, never tool-result envelopes or system injections.

3. **Aggregate the burst into one surface** before matching, so short high-quality
   fragments accumulate into a ≥2-chunk late-interaction surface instead of each
   falling to the single-vector fallback. (Aggregation boundary — see the decision
   point below.)

4. **Match and inject through the existing engine.** Run the aggregated surface
   through the same `scan` path (ADR-160 late interaction, ADR-130 salience reducer,
   ADR-155 keyword/semantic lanes), fire via the same `ways show way` gate (so the
   refractory/refire rules apply — the lane cannot spam), and emit fired content as
   `PostToolUse` `additionalContext`.

5. **Advance the scan mark** to the newest consumed `timestamp`, so each queued
   message is matched at most once.

### The aggregation boundary

How to group queued fragments into a surface is the load-bearing choice; the spike
proves aggregation is *required*. The decision is **all-pending-since-mark, scanned
at each PostToolUse**: concatenate every queued fragment newer than the scan mark
into one surface, match, then advance the mark. It is the simplest policy and adds no
new machinery — it reuses the existing flow, and leans on the ADR-155 refire gate to
absorb the one failure mode (a burst still being typed fires on a partial surface,
and a later fragment re-scans an overlapping one; the near-duplicate re-fire is
collapsed by refractory). Revisit against telemetry only if partial-burst noise
proves real. The two richer alternatives — settle-then-scan and a rolling window —
are recorded under Alternatives Considered.

## Consequences

### Positive

- **The prime surface is matched.** Operator intent — the highest-value, lowest-noise
  input — becomes a first-class matcher surface however the operator types it.
- **Symmetry realized.** Operator messages get the same late-interaction treatment as
  agent actions, which ADR-160 already intends channel-wise.
- **No matcher change.** Reuses the ADR-160 engine, ADR-130 reducer, ADR-155 lanes,
  and the ADR-123 PostToolUse dispatcher; the new code is transcript selection +
  aggregation + a scan mark.

### Negative

- **Transcript reads on PostToolUse.** Adds a bounded transcript scan per tool pause;
  cost must stay negligible (tail-read from the mark, not a full re-parse).
- **A timeliness/completeness tradeoff** with no free optimum (the aggregation
  boundary above) — a real tuning surface, like ADR-160's operating points.
- **Coupling to a transcript shape.** `queue-operation` is a Claude Code
  implementation detail; if its schema changes the selector must follow. Isolate the
  selector so the blast radius is one function.

### Neutral

- Establishes a general seam for *event-shaped* operator input (a future external
  message source could feed the same aggregated-surface lane).
- Interacts with ADR-160's <2-chunk fallback: aggregation is precisely what lifts a
  fragmented operator burst out of the fallback into late interaction.

## Alternatives Considered

- **Scan queued messages without aggregation.** Rejected: the spike shows a lone
  fragment falls to the single-vector fallback and does not fire the relevant way —
  it would ship a fix that still misses the operator's actual input.
- **`Stop`-hook (end-of-turn) scan.** Rejected as the primary lane: it cannot steer
  the current turn (the way fires after the agent has acted). Viable only as a
  backstop for intent that arrived too late to act on.
- **Do nothing / file upstream only.** Rejected: waiting on a harness change leaves
  the matcher's prime surface dark indefinitely; the transcript already carries the
  data to close the gap in-repo.
- **Concatenate operator text into the *next* `UserPromptSubmit` query.** Rejected:
  defers matching to the next idle turn — far too late to steer, and conflates
  distinct turns.
- **Settle-then-scan aggregation** (wait for a quiet window before scanning the whole
  burst). Rejected as the default: more complete intent, but it fires after the agent
  may have already acted on the request — it trades the steer for completeness. A
  fallback worth reconsidering if partial-burst noise proves real under (A).
- **Rolling-window aggregation** (last *k* fragments / *T* seconds regardless of burst
  boundaries). Rejected: robust to fragmentation but blends unrelated intents into one
  surface, and the scan mark already bounds the window to genuinely-unscanned text.
