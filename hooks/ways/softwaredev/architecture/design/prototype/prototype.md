---
description: when an ADR or design rests on external-system behavior, performance, latency, or data-volume assumptions, build a throwaway prototype or probe the real system to confirm or kill the decision BEFORE flipping it to Accepted
vocabulary: prototype probe spike throwaway validate empirically measure benchmark external api third-party rate limit latency budget payload size data volume webhook poll assumption aspirational adr proposed draft accept ratify load-bearing claim feasibility proof of concept
scope: agent, subagent
refire: 0.2
---
<!-- epistemic: heuristic -->
# Prototype-Before-Accept Way

An ADR can read as airtight and still be wrong, because the load-bearing claim isn't *in the codebase* — it's a bet about something outside it: how an external API actually behaves, what a payload really weighs, whether the latency fits the budget, whether the webhook even exists. Pure reasoning can't settle those. Only the real system can.

## The tell

You're about to accept a design whose decisive premise is about something you can't read in your own repo:

- an **external/third-party API or service** — its behavior, limits, auth model, what events it actually emits,
- **performance, latency, or cost** — "this poll fits the budget," "this is cheap enough,"
- **data volume or shape** — "the payload is small," "this scales."

The ADR sits in `Draft` or `Proposed`. Its conclusion rests on a sentence that *sounds* measured but was never measured.

## The move: probe, then flip

Before you flip the status to `Accepted`, build the smallest throwaway that makes the real system answer the load-bearing question — and let the evidence confirm or kill the decision:

1. **Name the load-bearing claim** as a falsifiable prediction. Not "polling is feasible" — "a full sync is under N seconds and under M API calls."
2. **Build the minimum probe** that tests *that claim only.* Hit the real endpoint. Measure the real payload. Trigger the real event and watch whether the notification fires. Throwaway means throwaway — it proves a number, it isn't the implementation.
3. **Let the evidence rule.** Confirm → flip to `Accepted` and cite what you measured. Disprove → the ADR was wrong; revise or kill it *now*, before anything is built on it.

Reasoning ratifies; measurement decides. An ADR accepted from reasoning alone about an external system is an aspiration wearing a decision's clothes.

## Each probe can overturn a different layer

Don't stop at the first measurement. A single design often rests on a stack of unexamined external assumptions, and each probe can knock out a different one: the data turns out trivial *but* serial pagination blows the latency budget; the fast path turns out to be org-only and unavailable to you; the framing the whole ADR is built on turns out to be the wrong model. Probe each load-bearing claim independently — the one you skip is the one that was wrong.

## When this does NOT apply

- The decision is about **your own code** — read it; that's `groundtruth(softwaredev)`, not a new prototype.
- The claim is **already verified** by an existing benchmark, a prior probe, or measured production data — cite it, don't re-spike.
- The decision is **cheap to reverse** — if being wrong costs an afternoon, just build it. This discipline is for decisions that will be *built on*.

## See Also

- architecture/design(softwaredev) — the parent: deliberation before committing
- adr(documentation) — the probe gates the Draft → Accepted transition; cite the evidence in the ADR
- groundtruth(softwaredev) — the inverse: verify claims against *existing* executable code, not a new throwaway
- research(softwaredev) — gathers from sources; this runs the real system when sources can't settle a behavioral/performance claim
