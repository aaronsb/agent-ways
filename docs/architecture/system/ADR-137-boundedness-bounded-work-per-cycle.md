---
status: Draft
date: 2026-06-20
deciders:
  - aaronsb
  - claude
related:
  - ADR-113
---

# ADR-137: Boundedness — a unit of work must be bounded within its cycle

## Context

Several parts of this system run **cyclically**: a unit of work is invoked
repeatedly on a schedule, sharing a budget with everything else in that cycle.
A sensor polled by the awareness loop is the clearest case — it runs on a tick,
under a hard timeout, and a slow one stalls every other unit's cadence — but the
shape recurs anywhere there is a recurring budget per iteration.

The cooperative assumption underneath all of these is that **each unit finishes
quickly and yields.** That assumption silently breaks when a unit's work is
*unbounded* — when its cost is proportional to something it does not control:

- a collection that can grow without limit (scan all N items),
- a remote resource whose latency is serial and uncapped (page-after-page,
  request-after-request),
- a stream with no natural end.

When such a unit exceeds its budget the failure is usually **silent**: it is
killed mid-work and returns nothing, which is indistinguishable from "there was
nothing to report." Silent failure that masquerades as a quiet success is the
worst outcome — nobody is alerted, and the system looks healthy while a whole
class of observations never lands. We hit exactly this: a unit that scanned an
external collection took time proportional to the collection's size plus serial
network latency, blew its cycle budget, and was killed without a trace. The data
volume was trivial; the *latency against a fixed budget* was the wall.

The lesson is general and worth stating once, abstractly, so it doesn't have to
be rediscovered per-feature.

## Decision

**A unit of work that runs within a cycle must do bounded work per cycle.**
"Bounded" along three axes:

- **Time** — it completes comfortably within the cycle's budget, with margin, not
  at the edge of the timeout.
- **Volume** — the data it processes per cycle is capped, not proportional to a
  source that can grow without limit.
- **Scope** — it observes a bounded slice, not "everything," when "everything"
  has no fixed size.

**If a task is inherently unbounded, it does not belong inside the cycle.** An
open-ended collection, an uncapped remote, or a stream is the wrong thing to do
*inline* on a tick. The cyclic unit should instead **read a prepared, bounded
result** — a local file, a cached value, a small queue — that some other,
longer-lived shape produced at its own pace. Producing the result by doing the
unbounded work inline is the anti-pattern; consuming an already-bounded result is
the pattern. (This ADR deliberately does not prescribe *which* out-of-cycle shape
to use — that is a per-case choice, and conflating it with this principle is how
the principle got lost the first time.)

**Boundedness violations must be loud, not silent.** A unit killed for exceeding
its budget must surface that fact — a diagnostic line, a recorded over-budget
marker — so "over budget" can never be read as "nothing observed." A budget
guard that fails quietly is worse than no guard, because it converts a
performance problem into an invisible correctness problem.

## Consequences

### Positive

- The cycle stays responsive: no single unit can starve the others or stall the
  whole loop by running long.
- Cost becomes independent of external size — a unit scales the same whether the
  thing it watches has ten items or ten thousand.
- Failures are diagnosable. "Over budget" is visible, so it is fixed as a design
  problem instead of haunting the system as phantom missing data.

### Negative

- Some genuinely useful observations are inherently unbounded, and this forbids
  doing them inline. They must be pushed into a separate, longer-lived shape —
  that is *more* moving parts, not fewer, and a real cost to weigh before
  deciding the observation is worth having at all.

### Neutral

- This draws a line between what may run inside a cycle and what must run outside
  it, without naming the outside mechanism. The outside mechanism is a separate
  decision each time.

## Alternatives Considered

- **Raise the per-cycle budget (longer timeout).** Rejected: it only moves the
  cliff. A larger budget still has an edge a larger workload crosses, and a longer
  timeout lets one slow unit stall the whole cycle — the budget exists precisely
  to protect the shared cadence.
- **Let units run unbounded but asynchronously within the cycle.** Rejected: it
  breaks the cooperative, finishes-and-yields model the cycle depends on, and
  reintroduces the silent-overrun failure in a subtler form (work that never
  completes rather than work that is killed).
- **Cap the work but fail silently when the cap is hit (truncate quietly).**
  Rejected: silent truncation is the same invisible-correctness trap as the
  silent kill — the consumer believes it saw everything. If work is dropped, that
  must be observable.
