---
status: Accepted
date: 2026-07-02
deciders:
  - aaronsb
  - claude
related:
  - ADR-110
  - ADR-151
  - ADR-200
---

# ADR-201: Findings assembled as classifier-ready assessment records

## Context

ADR-200 separated a **compliance claim** (a control-*design* assertion; SOC 2 Type I;
OSCAL Component Definition) from an **assessment finding** (session-derived evidence
with a determination of *satisfied* / *other than satisfied*; NIST SP 800-53A; OSCAL
Assessment Results). It also required, in §1, that every claim carry a **determination
criterion** — the observable session behavior that would let an assessor decide the
finding. ADR-151 packages the tool that produces findings: `ways-audit`.

ADR-200 left one thing unresolved: **who fills the determination, and when.** Two
tempting answers are both wrong.

- **The tool determines inline.** If `ways-audit` reads a claim and its firing evidence
  and writes `satisfied` itself, it manufactures the very evidence ADR-200 forbids — a
  model grading whether a model followed model-injected guidance, stamped as fact. This
  recreates the ADR-013 overclaim at the finding layer.
- **A human always determines.** If every finding must wait on a person to read the
  transcript and rule, the assessment surface is an unbounded labor sink. It does not
  scale past a handful of ways, and the corpus is designed to hold hundreds (ADR-200 §7).

The honest position is neither. The tool should **assemble** the finding — gather the
inputs and leave the determination *unset* — producing a record that some *other*
system will classify. That classifying system is deliberately **out of scope**: we do
not design it, ship it, or constrain what it is. In NIST 800-53A the assessor is a
*role*, not a person; generalize it to a **classifier** and leave it entirely external.
`ways-audit`'s whole responsibility is to make the record **cleanly classifiable** —
well-structured and easily accessible — and then stop.

## Decision

### 1. `ways-audit` assembles findings; it never determines them

The finding pipeline reads `(claim + its determination criterion + firing events +
transcript pointers)` and writes a finding record with the **determination slot empty**.
The tool's output is an *assessable record*, not an assessment. No code path in
`ways-audit` writes a determination value — that boundary is the load-bearing honesty of
this ADR.

### 2. An assembled finding is a classifier-ready record

Frame the finding record as one row of a supervised-classification dataset — *features*
(the observable inputs) and a *label* (the determination) filled later:

| Role | Field | Source |
|------|-------|--------|
| feature | `criterion` (the claim's `satisfied_when`) | claim sidecar (ADR-110) |
| feature | `evidence.firing` — counts, sessions, timestamps | firing-event log |
| feature | `evidence.transcript_refs` — session + pointer | session transcripts |
| feature | `tier` — `process` or `outcome` (ADR-200 §3) | the criterion's kind |
| **label** | `determination` — `satisfied` / `other-than-satisfied` / *unset* | a classifier |
| provenance | `assessed_by`, `assessed_at`, `basis` | the classifier, when it labels |

*Informally:* the ledger is a dataset with an empty label column. The framing is not
decorative — it is why the record stores the criterion and the raw evidence *beside* the
empty determination, so a finding is classifiable and auditable, not a bare verdict.

The record also carries empty **provenance** slots — `assessed_by`, `assessed_at`,
`basis` — so that when something does fill the label, *what* filled it and *on what* are
recorded next to it. `ways-audit` writes those slots empty and never fills them.

### 3. The classifier is out of scope — a different, undesigned system

We do not design, ship, or constrain the system that classifies these records. It is a
separate concern for a later day, and possibly a different tool entirely. This ADR fixes
only the **record** and the **boundary**: `ways-audit` produces clean, accessible,
classifiable rows and stops. Whatever eventually reads them — a deterministic check for a
*process-tier* criterion, a human reviewer, a trained model for an *outcome-tier* one
(ADR-200 §3) — is not our design problem here, and nothing in this decision presumes
which it will be. The one guarantee we make is structural: the dataset is easy to get at
and unambiguous to label.

## Consequences

### Positive

- Resolves ADR-200's open question without either overclaim (tool-determines) or an
  unbounded human labor sink (human-only).
- The determination boundary is enforced in code, not just prose: `ways-audit` has no
  path that writes a determination.
- The scope is small and honest: the tool ships a dataset, not an assessment engine.
  Classification is someone else's problem, on someone else's schedule.
- The ledger is dual-purpose at no extra authoring cost — an audit trail *and* a labeled-
  when-classified dataset — and stays useful even if the classifier is never built.

### Negative

- The finding record carries more structure (criterion + evidence + empty label and
  provenance slots) than a bare verdict, and the schema must stay stable enough to be a
  dataset over time.
- A dataset with a perpetually-empty label column is inert until *something* classifies
  it; this ADR deliberately does not deliver that something, so findings have no
  determinations until a separate effort supplies a classifier.

### Neutral

- The classifying system is entirely out of scope — not designed, not stubbed, not
  constrained here. This ADR fixes only the record shape and the assemble-never-determine
  boundary.
- `satisfied_when` moves from ADR-200 §1's "proposed, not yet in schema" note into the
  actual claim schema (ADR-151 §3) as part of realizing this ADR.

## Alternatives Considered

- **Tool writes the determination inline.** Rejected: manufactures evidence; the exact
  error ADR-200 was written to stop, relocated one layer down.
- **Determinations are human-only.** Rejected: an unbounded labor sink that cannot cover
  a corpus of hundreds; also forecloses the deterministic process-tier check that needs
  no human at all.
- **No stored criterion/evidence — store only the verdict.** Rejected: a bare verdict is
  neither auditable back to its basis nor usable as classifier training data, discarding
  the record's second purpose for nothing saved.

## References

- **ADR-200** — the claim/finding model this refines (§1 assessability, §2 findings, §3
  the two evidence tiers).
- **ADR-151** — the `ways-core` claim schema and the `ways-audit` binary that assembles
  findings.
- **ADR-110** — the `provenance.yaml` claim sidecar the criterion is stored in.
- **NIST SP 800-53A** — assessment findings and the *satisfied* / *other than satisfied*
  determination; the assessor as a role.
- **OSCAL Assessment Results** — the machine-readable finding layer this record shape
  anticipates.
