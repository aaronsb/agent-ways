---
contract: adr/v1
kind: decision
verb: change
capability: adr
amends: [ADR-304#6]
basis:
  - operator: aaronsb
    level: guided
    said: "Declared start active (Recommended)"
    via: "session 2026-09-27: the operator selected option 1 of 4 on #582; the label was written by the agent"
  - operator: aaronsb
    level: guided
    said: "the prior decision (the base decision) was that we could no longer test agent-ways correctly, directly on the host, it had to go into a container"
    via: session 2026-09-27, reviewing PR #583
  - operator: aaronsb
    level: guided
    said: "Accept as built (Recommended)"
    via: "session 2026-09-27: the operator selected this for decisions 3 to 9 of the ADR-304 stack handoff, which included widening the §6 stale-citation wording; the label was written by the agent"
  - evidence: "issue #582: ADR-186 changes a testing capability that existed before any record described it"
  - evidence: "adr cite already warns on superseded, deprecated, rejected, abandoned and archived targets (cmd_cite.py, V1_NON_ACTIVE_STATUSES)"
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "ok. so basically, I think it's the correct direction and implements the change to adr as discussed."
    via: session 2026-09-27, reviewing PR #583
    covers: []
status: accepted
date: 2026-09-27
deciders:
  - aaronsb
  - Claude
related:
  - 304
---

# ADR-305: Capabilities active at adoption need no add decision

## Summary

- **Decided:** a project records under `baseline` in `adr.yaml` the date it adopted adr/v1 and the capabilities it already had then. Those capabilities need no `add` decision, and a `change` on one with no prior record to name stands on the baseline. A capability declared after adoption still needs an `add`.
- **Trades away:** a record of why each baseline capability exists. The vocabulary line is its only description.
- **One-way?** No. Removing a name from `baseline` restores both checks for it.
- **Probes:** *Confident:* the outcome for a record does not depend on which other records have been migrated, since only decisions dated after adoption can be its prior, and every one of those was written as v1. *Not confident:* whether `baseline` will be used to skip an `add` for a capability that is actually new.
- **Inversion:** at one end every capability needs a written `add`, which misstates history for a corpus that predates the contract. At the other end the vocabulary is its own authority and nothing needs an `add`, which lets new capabilities in unrecorded. This decision exempts only what existed before adoption.

## Context

ADR-304 §6 requires every capability in the vocabulary to have an accepted `add` decision. Most of agent-ways' capabilities (matching, attend, install, testing and others) worked long before any record described them. Migrating an old record as an `add` misstates it. ADR-186 is the example the operator gave: it moved testing off the host and into a container, which changed a testing capability that no record had added. #582 set out three options: a baseline `add` record per capability, declared capabilities starting active, and a `change` that may list several capabilities. The operator chose the second, and the agent designed the mechanism within that direction.

## Decision

### 1. Baseline capabilities

`adr.yaml` may carry:

```yaml
baseline:
  adopted: 2026-09-27
  capabilities: [docs, testing]
```

`adopted` is a `YYYY-MM-DD` date. Every name in `capabilities` must be in the vocabulary. Either defect fails lint.

### 2. The add check, amended

The ADR-304 §6 bullet on accepted `add` decisions now reads:

- Every capability in the vocabulary, except those listed in `baseline`, has an accepted `add` decision. This warns while any v0 record remains and fails after, so a corpus that is still migrating does not fail on every capability.

### 3. A change on a baseline capability

ADR-304 §3 requires a `change` to supersede or amend a prior decision on the same capability. A `change` with no `supersedes` or `amends` edge on a baseline capability instead stands on the baseline when either:

- it is dated on or before `adopted`, or
- no earlier decision on that capability is dated after `adopted`. Earlier means by date, then number. `constrain` decisions do not count, and neither do rejected or abandoned ones.

Only decisions dated after adoption count as a prior, so migrating an older record never changes the outcome for another record. A record with no date, or a date that is not `YYYY-MM-DD`, cannot stand on the baseline.

### 4. Stale citations, widened

The ADR-304 §6 `doclint` bullet on superseded decisions now reads:

- A citation of a superseded, deprecated, rejected, abandoned or archived record warns. For a superseded record, the warning names the successor.

`adr cite` already behaves this way. The amendment brings the text in line with it.

## Consequences

### Positive

- Migrating a record no longer requires inventing history. ADR-186 migrates as a `change` on `testing`.
- An adopting project writes one list and one date, not one record per capability.

### Negative

- ADR-123 spans attend, matching and disclosure. Only `constrain` may list several capabilities, so ADR-123 stays on v0 until it is split or a rule for multi-capability changes is decided.
- A post-adoption `change` need not name a pre-adoption record on the same capability, even after that record is migrated. The v0 prior warning still applies when it does name one.

### Neutral

- Projects that declare no `baseline` behave as before.

## Alternatives Considered

- **A baseline `add` per capability.** This keeps the history complete, but at the cost of twelve records written after the fact, each dated at adoption.
- **A multi-capability `change`.** This would let ADR-123 migrate, but it loosens the one-change, one-capability rule, and it does not fix the missing `add` decisions.
- **Count every v1 decision as a prior.** This was the first version of this PR. Which change counted as first then depended on the order in which records were migrated, and a frozen record could start failing when an older one migrated.
