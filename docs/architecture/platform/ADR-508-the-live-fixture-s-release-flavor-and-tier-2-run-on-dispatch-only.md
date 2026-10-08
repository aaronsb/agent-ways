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

# ADR-508: The live fixture's release flavor and tier 2 run on dispatch only

## Summary

- **Decided:** the release flavor of tier 1 and tier 2 run on `workflow_dispatch` only. The nightly schedule is removed. The branch flavor stays the pull-request gate.
- **Trades away:** unattended detection of drift between `main`, the latest release and Claude Code's newest version. Drift now shows up when someone dispatches the run, typically before a release or after a Claude Code upgrade.
- **One-way?** No. Restoring the schedule is one line in the workflow.
- **Probes:** *Confident:* no one acts on a nightly result between releases. *Not confident:* a release should dispatch the release flavor as part of its checklist.
- **Inversion:** between continuous drift monitoring and testing only on change. The decision tests on change and on demand.

## Context

ADR-186 §2 and §4 run the release flavor and tier 2 on a nightly schedule as well as on dispatch. Tier 2 spends API tokens on every run. Over the eight nights from 2026-10-01 the scheduled workflow failed four times, and no failure led to work. A nightly result nobody reads costs tokens and adds a red mark that carries no signal.

## Decision

`live-fixture.yml` drops its `schedule` trigger and keeps `workflow_dispatch`. ADR-186 §2's "on the nightly schedule" and §4's "and on `schedule`" no longer hold; everything else in ADR-186 stands. Dispatch the workflow when a release is cut, when Claude Code's latest version changes in a way that matters, or to verify a change to tier 2 against a branch.

## Consequences

### Positive

- No token spend or red runs without a person asking for them.

### Negative

- Drift on either side waits until someone dispatches the run.

### Neutral

- The fixture's README and CLAUDE.md describe the dispatch-only trigger.

## Alternatives Considered

- **Keep tier 1 release nightly, move tier 2 to dispatch only.** Tier 1 costs no tokens, but its nightly failures led to no work either.
- **Weekly schedule.** Fewer runs with the same problem: a result nobody acts on.
