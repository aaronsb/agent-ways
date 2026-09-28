---
contract: adr/v1
kind: decision
verb: change
capability: adr
amends: [ADR-304#1]
basis:
  - operator: aaronsb
    level: guided
    said: "should we move design notes into agent decision records?"
    via: session 2026-09-27, while deciding how stale citations in docs/design-notes should be treated
  - operator: aaronsb
    level: guided
    said: "Yes: evidence kind, specs as spec (Recommended)"
    via: "session 2026-09-27, selected from agent-written options; the label was written by the agent"
  - precedent: ADR-304
  - evidence: "issue #491: research that informs a decision lands in a prior-art document the decision links to, and has no home in the record corpus"
  - evidence: "docs/design-notes holds 13 undated, unnumbered notes (surveys, audits, specs and explorations) that decisions cite by path and that cite records by number"
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "Same kind for now (Recommended)"
    via: "session 2026-09-27, selected from agent-written options when the probes were asked; the label was written by the agent"
    covers: [exploration-fit]
  - operator: aaronsb
    said: "Accept both and proceed (Recommended)"
    via: "session 2026-09-27, selected from agent-written options when the probes were asked; the label was written by the agent"
    covers: []
status: accepted
date: 2026-09-27
deciders:
  - aaronsb
  - Claude
related:
  - 304
  - 306
---

# ADR-309: An evidence record kind for findings, surveys and explorations

## Summary

- **Decided:** the record corpus gains an `evidence` kind for findings, surveys, audits, measurements and the reasoning that came before a decision. An evidence record is frozen once accepted, takes no verb, names a capability, and may be cited by a decision's basis. The design notes move into the corpus: specifications become `spec` records, and everything else becomes `evidence`.
- **Trades away:** the looseness of an unnumbered notes folder. A note now needs frontmatter, a number and an accept step.
- **One-way?** No. A kind is configuration in `adr.yaml`, and evidence records can be moved back out.
- **Probes:** *Confident (frozen-history):* freezing evidence at acceptance keeps its citations true to their moment, so an exploration citing a record later superseded stays correct as history. *Not confident (exploration-fit):* whether explorations, the reasoning before a decision, belong in the same kind as measurements and surveys, or will want their own kind once there are more of them.
- **Inversion:** at one end evidence stays outside the corpus as loose notes, uncitable by number and unchecked. At the other end every note becomes a decision, and decisions fill with material that decides nothing. This decision gives findings their own kind: numbered and checked, but separate from decisions.

## Context

ADR-304 §1 declared record kinds as data, seeded `decision` and `spec`, and named an evidence kind as a likely next kind. §8 sends research, findings and benchmarks to evidence notes that decisions link to (#491). Those notes live in `docs/design-notes/`: 13 files without frontmatter or numbers. They are cited by path, and they cite records by number, including records since superseded, so `adr cite` reports them as stale.

## Decision

### 1. The evidence kind

`adr.yaml` declares:

```yaml
kinds:
  evidence:
    mutable_after_accept: [status, superseded_by, related]
    verb: forbidden
    requires: [capability]
    edges: { supersedes: evidence }
```

An evidence record records what was found, measured or reasoned at a point in time. It is frozen once accepted, like a decision. A later finding supersedes it and does not rewrite it. It carries no Summary requirement, no verb and no `agent` requirement.

### 2. Decisions cite evidence

A decision's `basis` edges may point at evidence records as well as decisions and specs (`basis: [decision, spec, evidence]`). A basis entry `evidence: ADR-N` that names a record must resolve to an evidence or spec record.

### 3. What moves

Each design note becomes a record in the area its content belongs to, with a number from that area's band:

- A note that specifies behaviour that is kept current becomes a `spec`.
- A survey, audit, measurement, plan or exploration becomes `evidence`, accepted as the record of its moment.
- Every path reference to a moved note is rewritten. The notes' own citations of superseded records stay as written, because they are history.

## Consequences

### Positive

- Findings are numbered and citable, and a decision's basis can point at one.
- `adr cite` stops flagging historical citations inside evidence, which is frozen by kind.

### Negative

- Writing a note now takes frontmatter and an accept step.
- The contract carries a third kind that tools and guidance must know about.

### Neutral

- `spec` is unchanged; it gains two records from the move.
- Closes #491.

## Alternatives Considered

- **Exclude the notes from `adr cite` and leave them where they are.** This silences the warnings, but the notes stay uncitable, and the kind ADR-304 anticipated stays missing.
- **A separate `note` kind for explorations.** More precise, but a fourth kind before there is evidence it's needed. The not-confident probe keeps the question open.
- **Make each note a decision.** Rejected: most notes decide nothing, and a decision without a decision in it misleads.
