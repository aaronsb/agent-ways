---
contract: adr/v1
kind: decision
verb: change
capability: matching
amends: [ADR-196#Decision]
basis:
  - operator: aaronsb
    level: directed
    said: "that is not a trade I want, and it sounds to me like skipping the catalog variant is the performant move"
    via: chat, session 02e97f86, 2026-10-01, declining a judge request that cut wrong passes at more cost and latency
  - operator: aaronsb
    level: directed
    said: "cap"
    via: chat, session 02e97f86, 2026-10-01, choosing between a candidate cap and a deadline that scales with candidate count
  - evidence: ADR-195
  - precedent: ADR-196
agent:
  name: Claude
  model: claude-opus-5-5
status: proposed
date: 2026-10-01
deciders:
  - aaronsb
related: [ADR-502]
---

# ADR-197: Cap the candidates the relevance gate judges per request

## Summary

- **Decided:** A judge request carries at most a profile-set number of candidates, 8 by default, taken in the matcher's order. Candidates past the cap are not judged and pass as the matcher decided. Each request that hits the cap is logged with the ways it left unjudged.
- **Trades away:** On a prompt with more candidates than the cap, the lowest-ranked ways reach the session unfiltered. The alternative, a deadline that grows with candidate count, would judge them all and make those prompts wait.
- **One-way?** No. The cap is a profile field; raising it to the request's whole candidate list restores the ADR-196 behaviour.
- **Probes:** *Confident (latency):* you would rather a few low-ranked ways pass unjudged on a busy prompt than wait past 2 s for a verdict on every one. *Not confident (overflow):* if the overflow turns out to be mostly noise, would you rather block it than pass it?
- **Inversion:** Between judging every candidate however long it takes and judging none. The answer sits nearer the first end, with a bound set by the latency the operator accepts.

## Context

ADR-196 sends every candidate of a prompt in one judge request with a 2 s deadline, and fails open past it. Measured on the deployed gate (ADR-195, second addendum), a call takes about 0.6 s plus 0.1 s per candidate, because the answer grows by about 20 output tokens per candidate and output dominates the call. Every deadline fallback in three benchmark runs was a prompt with 10 or more candidates. Those prompts are the ones with the most ways to filter, and on them the gate filtered nothing.

The operator ruled out spending more latency or cost per prompt for a better gate.

## Decision

1. **The cap.** A profile field, `max_candidates`, bounds the candidates in one judge request. The shipped profiles set 8, which the measurement puts near 1.4 s, inside the 2 s deadline. A user layer may override it like any other profile field.
2. **Which candidates.** The gate takes the first `max_candidates` of the ways it would judge, in the matcher's admission order: hits from an explicit trigger (`files:`, `commands:`, a keyword `pattern:`) before scored hits, each tree whole before the next, parents before children, siblings by their best score. Ways with `pattern_strict` are still not judged and do not count against the cap.
3. **The rest.** Candidates past the cap get no verdict and pass, as they did before the gate existed, except a way whose ancestor the judge blocked: a way's guidance presumes its parent's, so it is blocked with the parent. This amends ADR-196 §1 and §2: one request per prompt carries up to the cap rather than every candidate.
4. **Logging.** A request that hits the cap logs one `gate_capped` event with the number judged, the number left unjudged and their ids. `ways introspect dump` counts unjudged ways in its gate summary beside the verdicts and fallbacks.

## Consequences

### Positive

- Prompts with many candidates get verdicts on their strongest matches instead of a fallback that judges none.
- Gate latency stays bounded by the cap, whatever the matcher proposes.

### Negative

- The lowest-ranked candidates on a busy prompt pass unfiltered. The `gate_capped` events show how often and which ways.
- The cap is a second knob beside the deadline. Raising one without the other brings the fallbacks back.
- The admission order fills the cap with explicit-trigger hits first, so on a prompt with many of them the overflow is semantic hits, the kind the gate filters most. Whether the cap should fill with semantic hits first is open, and the `gate_capped` events are the data for it.

### Neutral

- `max_candidates` is a new profile field in `ways-agent-core`, which both the `ways` hook and the agent parse with `deny_unknown_fields`. Both binaries ship together in the release that adds it. An older agent given a user layer that sets the field rejects the file on every request, and the gate fails open with `agent_error` logged. The cap is applied by the client; the agent only reports it.

## Alternatives Considered

- **A deadline that grows with candidate count**, about 0.6 s plus 0.1 s per candidate. It judges every candidate, and a 15-candidate prompt waits about 2.5 s. Rejected by the operator on latency.
- **Blocking the overflow.** The overflow is the matcher's weakest matches and the gate blocks most candidates it judges, so blocking would often be right. It would also block ways no judge has read, which the gate never does elsewhere. Left open as the second probe; the `gate_capped` events are what would settle it.
- **Splitting the candidates across parallel requests.** Each request stays fast, but verdicts depend on which candidates share a request (ADR-195, second addendum), so splitting changes the verdicts as well as the latency, and it multiplies calls.
- **A smaller answer per candidate.** Measured as `p_relevant` alone: 3 fewer output tokens per candidate, since the JSON keys and ids dominate the answer. Not enough to move the deadline.
