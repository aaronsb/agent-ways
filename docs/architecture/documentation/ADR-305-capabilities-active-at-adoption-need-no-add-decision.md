---
contract: adr/v1
kind: decision
verb: change
capability: adr
amends: [ADR-304#6]
basis:
  - operator: aaronsb
    level: directed
    said: "Declared start active (Recommended)"
    via: session 2026-09-27, answering the #582 options
  - evidence: "issue #582: ADR-186 adds a fixture to a testing capability that existed before any record described it"
agent:
  name: Claude
  model: claude-opus-5-5
status: proposed
date: 2026-09-27
deciders:
  - aaronsb
  - Claude
related:
  - 304
---

# ADR-305: Capabilities active at adoption need no add decision

## Summary

- **Decided:** a project lists the capabilities it already had when it adopted adr/v1 under `baseline` in `adr.yaml`. Those need no `add` decision. Every capability declared after adoption still needs one. Stale-citation warnings cover deprecated, rejected and abandoned targets as well as superseded ones.
- **Trades away:** a record of why each baseline capability exists. The vocabulary line is the only description.
- **One-way?** No. A later baseline `add` can still be written for any capability, and removing a name from `baseline` restores the check.
- **Probes:** *Confident:* no migrated record has to claim it added a capability that already existed. *Not confident:* whether `baseline` gets used to skip an `add` for a capability that is actually new.
- **Inversion:** at one end every capability needs a written `add`, which misstates history for a corpus that predates the contract. At the other end the vocabulary is its own authority and nothing needs an `add`, which lets new capabilities in unrecorded. The rule sits between them: capabilities that existed before adoption are exempt, and new ones are not.

## Context

ADR-304 §6 requires every capability in the vocabulary to have an accepted `add` decision. Most of agent-ways' capabilities (matching, attend, install, testing and others) worked long before any record described them. Migrating an old record as an `add` misstates it. ADR-186, for example, adds a fixture to a testing capability that already existed. #582 set out three options: a baseline `add` record per capability, declared capabilities starting active, and a `change` that may list several capabilities. The operator chose the second.

## Decision

### 1. Baseline capabilities

`adr.yaml` may carry `baseline`, a list of capability names that were active when the project adopted adr/v1. Every name must be in `capabilities`, and a name outside the vocabulary fails lint.

### 2. The add check, amended

The ADR-304 §6 bullet on accepted `add` decisions now reads:

- Every capability in the vocabulary, except those listed in `baseline`, has an accepted `add` decision. This warns while any v0 record remains and fails after, so a corpus that is still migrating does not fail on every capability.

A capability added after adoption goes into `capabilities` and not into `baseline`, so its `add` decision is still required.

The first `change` on a baseline capability has no prior record to supersede or amend, so the baseline stands in for that prior. Once a decision on the capability exists, earlier by date and then number, the next `change` must name it, as ADR-304 §3 requires.

### 3. Stale citations, widened

The ADR-304 §6 `doclint` bullet on superseded decisions now reads:

- A citation of a superseded, deprecated, rejected or abandoned record warns. For a superseded record, the warning names the successor.

`adr cite` already behaves this way. The amendment brings the text in line with it.

## Consequences

### Positive

- Migrating a record no longer requires inventing history. ADR-186 can migrate as a `change` on `testing`.
- An adopting project writes one list, not one record per capability.

### Negative

- ADR-123 still spans attend, matching and disclosure. Only `constrain` may list several capabilities, so ADR-123 stays on v0 until it is split or a rule for multi-capability changes is decided.

### Neutral

- Projects that declare no `baseline` behave as before.

## Alternatives Considered

- **A baseline `add` per capability.** This keeps the history complete, but at the cost of twelve records written after the fact, each dated at adoption.
- **A multi-capability `change`.** This would let ADR-123 migrate, but it loosens the one-change, one-capability rule, and it does not fix the missing `add` decisions.
