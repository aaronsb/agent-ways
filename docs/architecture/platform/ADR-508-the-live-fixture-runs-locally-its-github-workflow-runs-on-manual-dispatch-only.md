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
  - operator: aaronsb
    level: directed
    said: "to be honest, just having it be a local action seems like the best approach right now, the more I think about it. it can stay a manual invocation on github"
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
considered:
  - operator: aaronsb
    said: "let's make it an optional step, based on the merits of what changed"
    via: chat, 2026-10-08
    covers: [release-step]
related:
  - ADR-186
---

# ADR-508: The live fixture runs locally; its GitHub workflow runs on manual dispatch only

## Summary

- **Decided:** the live fixture is a local action, `make test-live`, run when staging a release. `live-fixture.yml` keeps `workflow_dispatch` and loses its nightly schedule. The branch flavor of tier 1 stays the pull-request gate in `portability.yml`.
- **Trades away:** detection of drift between releases, chiefly a new Claude Code version breaking the latest release. That drift now shows up at the next release or when someone dispatches the run.
- **One-way?** No. Restoring the schedule is one line in the workflow.
- **Probes:** *Confident (local-first):* running the fixture locally while staging a release is where its result leads to work. *Not confident (release-step):* the release skill should name the run as a staging step.
- **Inversion:** between continuous drift monitoring and testing only on change. The decision tests on demand, by the person staging a release.

## Context

ADR-186 §2 and §4 run the release flavor and tier 2 on a nightly schedule as well as on dispatch. Tier 2 spends API tokens on every run. Over the eight nights from 2026-10-01 the scheduled workflow failed four times, and no failure led to work. A nightly result nobody reads costs tokens and adds a red mark that carries no signal.

## Decision

`live-fixture.yml` drops its `schedule` trigger and keeps `workflow_dispatch`. The fixture is run locally with `make test-live` (`TIER=1|2`, `FLAVOR=branch|release`) when staging a release, after a Claude Code change that matters, or to verify a tier 2 change. The GitHub workflow remains for a manual run on a clean runner. ADR-186 §2's "on the nightly schedule" and §4's "and on `schedule`" are replaced by this; everything else in ADR-186 stands.

## Consequences

### Positive

- Tier 2 spends tokens only when a person runs it, and the result lands in front of that person.

### Negative

- Drift between releases waits for the next release or a dispatch.

### Neutral

- The fixture's CLAUDE.md describes the trigger.

## Alternatives Considered

- **Keep tier 1 release nightly, move tier 2 to dispatch only.** Tier 1 costs no tokens, but its nightly failures led to no work either.
- **Weekly schedule.** Fewer runs with the same problem: a result nobody acts on.
- **Run after every `ways-v*` release build through `workflow_run`.** Automatic, but the result lands on GitHub after the release is out; a local run while staging catches the problem before the tag.
