---
contract: adr/v1
kind: decision
verb: change
capability: testing
basis:
  - operator: aaronsb
    level: directed
    said: "hmm. I don't think there's any use cases where need to run this nightly."
    via: chat, 2026-10-08
  - operator: aaronsb
    level: directed
    said: "if it's dispatched when we're staging a new release that seems like a reasonable approach"
    via: chat, 2026-10-08
  - evidence: "live-fixture.yml scheduled runs 2026-10-01 to 2026-10-08: 4 of 8 failed"
  - precedent: ADR-186
agent:
  name: claude
  model: claude-opus-5-5
status: accepted
date: 2026-10-08
deciders:
  - aaronsb
amends: "ADR-186#Decision"
related:
  - ADR-186
---

# ADR-508: The live fixture's release flavor and tier 2 run per release, not nightly

## Summary

- **Decided:** the release flavor of tier 1 and tier 2 run when a `ways-v*` release finishes building, and on `workflow_dispatch`. The nightly schedule is removed. The branch flavor stays the pull-request gate.
- **Trades away:** detection of drift between releases, chiefly a new Claude Code version breaking the latest release. That drift now shows up at the next release or when someone dispatches the run.
- **One-way?** No. Restoring the schedule is one line in the workflow.
- **Probes:** *Confident (per-release):* a run per release is the point where a result leads to work. *Not confident (other-components):* releases of components other than `ways` do not need the run.
- **Inversion:** between continuous drift monitoring and testing only on change. The decision tests each release and on demand.

## Context

ADR-186 §2 and §4 run the release flavor and tier 2 on a nightly schedule as well as on dispatch. Tier 2 spends API tokens on every run. Over the eight nights from 2026-10-01 the scheduled workflow failed four times, and no failure led to work. A nightly result nobody reads costs tokens and adds a red mark that carries no signal.

## Decision

`live-fixture.yml` drops its `schedule` trigger. It runs on `workflow_run` when the `Build ways CLI` workflow completes successfully for a `ways-v*` tag, which is after the release assets are published, so the release flavor tests the release just cut. Branch pushes that complete the build workflow are skipped by the job condition. `workflow_dispatch` stays, for a Claude Code change that matters or to verify a change to tier 2 against a branch. ADR-186 §2's "on the nightly schedule" and §4's "and on `schedule`" are replaced by this; everything else in ADR-186 stands.

## Consequences

### Positive

- Tier 2 spends tokens once per release, and a red run points at the release that caused it.

### Negative

- Drift between releases waits for the next release or a dispatch.

### Neutral

- The fixture's CLAUDE.md describes the trigger.

## Alternatives Considered

- **Keep tier 1 release nightly, move tier 2 to dispatch only.** Tier 1 costs no tokens, but its nightly failures led to no work either.
- **Weekly schedule.** Fewer runs with the same problem: a result nobody acts on.
- **Dispatch only, with a release checklist step.** Relies on someone remembering; the `workflow_run` trigger does it for every release.
