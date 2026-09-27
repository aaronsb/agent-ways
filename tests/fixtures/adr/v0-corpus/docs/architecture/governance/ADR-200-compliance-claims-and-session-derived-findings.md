---
status: Accepted
date: 2026-07-02
deciders:
  - aaronsb
  - claude
related:
  - ADR-005
  - ADR-013
  - ADR-110
  - ADR-111
  - ADR-151
  - ADR-201
supersedes:
  - ADR-005
---

# ADR-200: Compliance claims and session-derived findings

## Context

The framework carries a provenance subsystem (ADR-005) and a rationale for it
(ADR-013): a way can carry metadata mapping its guidance to regulatory controls
(NIST 800-53, OWASP, ISO 27001, SOC 2, CIS, IEEE), stored today as a `provenance.yaml`
sidecar beside the way (ADR-110) and queried by a `ways governance` CLI (ADR-111).

In use, three problems surfaced — each worse than the last.

**It is misnamed.** Mapping practices to external standards and reporting on that
mapping is **compliance** work — demonstrating conformance to an outside rule — not
**governance**, which is the steering discipline (ADRs, ways, review gates) the
framework is already saturated with. Calling a narrow control-mapping tool
"governance" oversells it as the steering layer *and* strands the real governance
without the word. That category error is much of why the subsystem reads as bolted on.

**Nothing consumes it.** No CI job, hook, or reviewer runs it. The `governance-cite`
guidance tells the model to cite controls when recommending practices, but that is
advisory and manual. The machinery is a library with no callers.

**Its claims were never assessed** — the decisive problem. Each provenance sidecar is
hand-authored: the control IDs, the justifications, the `verified` date are all typed
by a person. Nothing ever assessed whether a way *actually* meets a control. In audit
vocabulary the distinction is exact and we collapsed it:

- An **implementation claim** is a statement that a control is *designed* to do its
  job. That is all the sidecars are. Auditors call this posture **SOC 2 Type I** —
  *suitably designed as of a point in time.*
- An **assessment finding** is what an assessor produces after examining real
  operation. NIST's assessment standard (SP 800-53A) records a finding with a
  determination of **satisfied** or **other than satisfied**. The richer report,
  **SOC 2 Type II**, covers *design **and** operating effectiveness over a period.*

A hand-written citation is an implementation claim (Type I) wearing the costume of a
finding (Type II). ADR-013 made this exact error out loud — it called provenance
"auditability … pull real control citations" and said it "turns Claude from a hack
into a professional." A claim on demand is not proof; it is an assertion awaiting an
assessment.

### The reframe

Separate the two epistemic statuses ADR-005/013 conflated, using the field's real
names, and state honestly what the subsystem is a *foundation for*:

- A **claim** is a control-design assertion (SOC 2 Type I; in NIST's OSCAL data model,
  a *Component Definition* — "here is how this component can satisfy the control").
- A **finding** is a session-derived assessment finding (SOC 2 Type II; OSCAL
  *Assessment Results*): evidence that the claimed guidance actually shaped real work.

The connective tissue between them is an **assurance case** — a spelled-out argument
tying a claim to its evidence so a reader can check the *reasoning*, not just the
proof. The notation whose vocabulary we use literally is **CAE (Claims–Arguments–
Evidence)**; its graphical sibling GSN is the same idea (GSN confusingly names the
evidence node a "Solution"). This is also, recursively, the model OSCAL itself uses —
a compliance-claim system built this way follows the standard shape of compliance
systems.

### The formation thesis

Agent-ways operates at the **first line** — in the Three Lines Model (the Institute of
Internal Auditors' 2020 update of the old "three lines of defense"), the roles that
own and manage risk *in the actual doing of the work*, distinct from second-line
functions that monitor them and third-line auditors who independently check them.

A first-line developer does not recite NIST 800-53 while committing. Through repeated
exposure to a way of working, the control's expectations become **tacit knowledge** —
Polanyi's "we can know more than we can tell," the expertise the hands hold that words
can't fully capture. A way is the **externalization** of that shape (in Nonaka's SECI
model, the tacit→explicit step; more generally, **codification**) delivered at the
point of work. That shaped habit is the expensive precondition every audit assumes but
none create: when the work already has the standard's shape, evidence can be *gathered*
later instead of *manufactured*.

So the compliance stack, top to bottom:

```
   certify   → independent attestation (SOC 2, SSAE 18) or an OSCAL SSP   ← third line / GRC toolchain — NOT us
   finding   → "the shaped work actually happened" (Type II)       ← ways-audit (ADR-151)
   claim     → "this way is designed to steer toward control X" (Type I)   ← provenance sidecars
 ──────────────────────────────────────────────────────────────────
   way       → externalized guidance that shapes the work           ← agent-ways  ★ first line, we live here
   work      → the actual activity
```

We own the bottom two natively. The compliance subsystem reaches up into claim and
finding and deliberately **stops below certify** — we are a first-line formation aid,
not a third-line assurance function. The honest top-level statement is therefore:
**foundation for, not implementation of, a GRC/OSCAL toolchain.** And that statement is
itself a claim awaiting its own findings — the system is an instance of its own thesis.

## Decision

Reframe the subsystem from asserted "governance traceability" into an
**operator-invoked compliance formation layer** with an honest claim/finding split.
This supersedes ADR-005 and refines (does not discard) ADR-013.

### 1. Claims are claims, not evidence

The provenance sidecars are **compliance claims** — control-*design* assertions (SOC 2
Type I; OSCAL Component Definition). They carry no weight beyond assertion. ADR-013's
framing of them as auditability or proof is retracted. A claim is a hypothesis awaiting
a finding.

**A claim must be assessable.** To seed claims across the corpus without recreating the
overclaim at scale, each claim carries a **determination criterion** — a plain
statement of what observable behavior in a session would let an assessor mark it
*satisfied* or *other than satisfied* (NIST 800-53A). A claim with no such criterion is
unfalsifiable: it can never graduate into a finding, and coverage of such claims is not
evidence of anything. Claim-authoring may be **agent-assisted** — `ways-audit` can
propose candidate controls for a way so the author is not hand-researching every way —
but it stays **human-grounded**: what a control *means*, and whether a way honestly
addresses it, is the author's epistemic call (ADR-013's collaborative authorship model).

### 2. Findings are session-derived

Evidence is produced, not authored:

```
claim (sidecar)  →  way fires → logged event  →  auditor reads (claim + firing + transcript)  →  finding
```

The firing-event log and session transcripts already exist. A **finding** records
`(claim-id, session-id, timestamp, determination, evidence-pointers)`, its
determination **satisfied** / **other than satisfied** in NIST 800-53A's own terms, the
pointer being the transcript and firing event so every finding is auditable back to its
source. Findings accumulate in an append-only ledger; each *other than satisfied*
finding becomes a **POA&M** (plan of action & milestones) entry — a tracked gap, not a
vague fail.

### 3. Two evidence tiers, honestly separated

Audit-evidence practice distinguishes **process evidence** (a step was performed) from
**outcome evidence** (the intended result was achieved). We adopt both, separately:

- **Process tier** — "control-aligned guidance was surfaced at the point of work." The
  firing log *alone* substantiates this: timestamped, deterministic, no model judgment.
  Defensible today; this is the MVP.
- **Outcome tier** — "and the work actually took that shape." This needs an auditor
  reading the transcript, is model-judged, is fallible, and carries a credibility
  problem: a model grading whether a model followed model-injected guidance. It is the
  stretch tier and must be labeled opinion, never proof.

Outcome-finding credibility rests on auditor trustworthiness — independence,
deterministic checks where possible, optional human counter-sign. This is the central
unsolved problem and is named as such, not hidden.

### 4. Operator-invoked, never forced

There is **no mandatory claim → finding → certify traversal.** Compliance review is a
station the operator pulls into deliberately, like `/ship` or `/wrap` — not a gate
anyone is dragged through. Concretely: a `compliance-reviewer` agent (sibling of
`code-reviewer`) plus skills that let the main agent direct it and do the loop-closing
work. It slots into delivery as an *optional* stage — `code-review → compliance-review
→ fix → ship` — where findings become labeled issues (the POA&M). The tooling that
backs it is `ways-audit` (ADR-151), not the core binary.

### 5. Borrow the vocabulary; do not implement the standard

Use OSCAL and assurance-case nouns (claim, finding, POA&M, assessment) for precision
and to shape output a real toolchain could ingest. We do **not** implement the OSCAL
schema, validate against it, or produce a System Security Plan (SSP). Using the
vocabulary must never read as a conformance claim to the standard itself.

Reference standards by ID and authoritative URL; never reproduce normative text. NIST
800-53/OSCAL/800-53A and OWASP are public; CIS is free-with-registration; **ISO 27001
and ISO 25010 are copyrighted** — cite the clause ID only.

### 6. Evidence is shaped to be handed up

Findings must be legible to the layer above (OSCAL Assessment-Results-shaped, control
IDs that resolve to real catalogs). Our job is to *feed* a certifier we are not — never
to terminate the chain with an attestation of our own.

### 7. The progressive-ways loop

Reframe `gaps` from "ways without provenance" to **"claimed controls with no
influencing way."** The claim set then *drives the ways backlog*: every control we
claim but do not yet steer toward is authoring work. This is what makes the subsystem a
living formation layer rather than a static bibliography — the claim creates the
obligation to build the way.

### 8. Rename on the operator surface

"Governance" leaves the operator-facing surface: the toolkit is `ways-audit`, its
domain concept is **compliance**, its artifacts are **claim / finding / POA&M**.
Storage stays the `provenance.yaml` sidecar (ADR-110), consumed by `ways-audit` rather
than the core binary. "Governance" survives only as the name of this ADR topic band
(200–299), whose `adr.yaml` description already reads "compliance mapping."

## Non-Goals

This section is load-bearing — the fence that stops a future session from growing this
into the thing it is not. **The system is incapable of overclaiming by construction:**
every artifact says "we *claim* X" or "we found *evidence* that guidance for X was
present/followed," never "you *conform to* X." It is a first-line formation aid, not a
third-line assurance function.

This is **not**:

- a GRC platform or compliance-management product;
- an OSCAL implementation, validator, or SSP generator;
- audit-grade evidence, an attestation, or an independent assessment;
- a continuous, automated, or enforced control;
- a certification, or any claim of conformance.

It **is**: an opinionated, operator-invoked lens that helps consider compliance-shaped
concerns at the point of work, curate ways toward controls the operator chooses to
claim, and keep a modest, honestly-labeled claims-and-findings trail — *a way of
working that leans compliance-shaped, not a compliance framework.*

## Relationship to prior ADRs

- **Supersedes ADR-005** (Governance Traceability for Ways). The mechanism — sidecar
  claims, generated manifest, reporting-not-enforcing — is retained and renamed; the
  framing of provenance as "auditability / proof" is replaced by the claim/finding
  split.
- **Refines ADR-013**, does not discard it. Kept: the activation-cue thesis (ways prime
  latent knowledge rather than inject it), the Type 1–4 way classification, the
  author-as-compiler model, and the formation intuition latent in its "Shift Lead
  Wisdom" type. Corrected: the epistemic overclaim that a citation on demand
  constitutes evidence or professional proof. Under this ADR, ADR-013's Type 1 ways
  carry *claims*; findings are what a session produces.
- **Relates to ADR-110** (provenance moved to a `provenance.yaml` sidecar) for storage,
  and **ADR-111** (CLI consolidation) for the command surface that becomes `ways-audit`.
- **Requires ADR-151** for packaging: the `ways-core` library crate and the `ways-audit`
  sibling binary.

## Consequences

### Positive

- The subsystem stops overclaiming and starts being true: claims are claims, findings
  are earned and carry a real determination.
- A defensible MVP exists immediately (process evidence from the firing log) without
  waiting on the hard outcome-audit problem.
- The formation-layer framing gives every downstream decision a coherent "why," and
  positions agent-ways to *feed* enterprise GRC toolchains rather than compete with them.
- The progressive-ways loop turns claims into an authoring backlog — the subsystem
  becomes generative.
- Operator-invoked design keeps zero forced ceremony; users who never touch it pay
  nothing.

### Negative

- Real work to realize: rename, de-assertion of existing sidecars, the `ways-audit`
  toolkit (ADR-151), the auditor agent + skills, and the finding-ledger format.
- The outcome-tier credibility problem is genuine and unsolved; that tier ships labeled
  as opinion or not at all.
- Borrowing OSCAL/assurance vocabulary invites the very "are you OSCAL-conformant?"
  overclaim the Non-Goals forbid; the honesty framing must be maintained vigilantly.

### Neutral

- The ADR domain folder stays "governance" (topic band); only the operator surface
  renames to "compliance."
- Existing provenance sidecars remain valid data — reclassified as claims, not rewritten.
- Whether the outcome tier ever ships is left open; the process tier stands on its own.

## Alternatives Considered

- **Keep it as "governance traceability" (status quo).** Rejected: misnamed,
  unconsumed, and epistemically overclaiming — the problems that motivated this ADR.
- **Delete it entirely.** Rejected: the data, policy docs, and CLI are a real
  foundation, and the formation-layer idea is valuable and rare. Deleting discards a
  good foundation to avoid fixing a label and an epistemic error.
- **Promote to a full compliance/GRC platform.** Rejected: that is the third line, not
  ours. Certification, SSPs, and independent assessment belong to tools built for them;
  we produce foundation-grade inputs for those tools.
- **Bake in a mandatory claim → finding → certify loop.** Rejected: forced traversal is
  exactly the bolt-on failure mode. The value is *availability* to the operator, like
  any session-defining skill.
- **Keep provenance framed as evidence (ADR-013 as written).** Rejected: it is the
  overclaim this ADR exists to correct.

## References

Cited by identifier; normative text of copyrighted standards (ISO) is not reproduced.

- **The Three Lines Model** — IIA, 2020 update of the "three lines of defense"
  (governance roles; "first line" = risk-owning roles in the work).
  https://www.theiia.org/en/content/communications/2020/july/20-july-2020-iia-issues-important-update-to-three-lines-model/
- **SOC 2 Type I vs Type II** — AICPA attestation under SSAE 18 / Trust Services
  Criteria (design at a point in time vs design *and* operating effectiveness over a
  period).
- **Assurance cases — CAE (Claims–Arguments–Evidence)** and **GSN** — ISO/IEC 15026-2;
  the claim → argument → evidence structure this ADR mirrors.
- **OSCAL** (NIST) — Catalog / Profile / Component Definition / Assessment Results /
  POA&M layered model. https://pages.nist.gov/OSCAL/learn/concepts/layer/
- **NIST SP 800-53A Rev. 5** — assessment findings; determinations "satisfied" /
  "other than satisfied." https://csrc.nist.gov/glossary/term/assessment_findings
- **PCAOB AS 1105** — *Audit Evidence* (the general standard; the process-vs-outcome
  framing is our application of it).
- **Tacit knowledge** — Polanyi, *The Tacit Dimension* (1966): "we can know more than we
  can tell." **Externalization / codification** — Nonaka & Takeuchi, SECI model.
- **Procedural vs declarative knowledge** — Ryle, *The Concept of Mind* (1949):
  knowing-how vs knowing-that.
- **ADR-005, ADR-013** — the prior decisions this ADR supersedes and refines.
