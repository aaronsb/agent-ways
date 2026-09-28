---
status: Accepted
date: 2026-08-05
deciders:
  - aaronsb
  - claude
related:
  - 165
  - 128
---

# ADR-176: Contract identification as the develop-loop front gate

## Context

A cross-check against socratic (m4vic/socratic) — a self-interrogation skill that makes an agent question itself before writing code — surfaced one gap the corpus does not already cover better.

Socratic's core move is a **pre-code contract**: before building, it self-interrogates across engineering domains, self-answers what it can from the codebase, and emits a short surface — what it assumed, the few open questions, the top risks, the plan — then builds and verifies. Most of that is already ours, usually with a stronger mechanism: `meta/choices` owns self-answer-vs-escalate, `softwaredev/delivery/implement` owns the briefing, the `develop` stable tail owns build→review→fix, and the per-domain ways own the question banks. The disclosure-plus-decay mechanism (ADR-160) re-injects to course-correct later turns, which socratic's load-once skill cannot do.

The one thing missing: a **cheap, always-on gate at the front of a build that establishes what the build is bound to** — below the ceremony threshold of the `implement` briefing, which fires ADR-side on substantial work. ADR-165 gave the develop loop a variable front (design / prototype / adr, ordered by where the uncertainty lives) and a stable tail, but no first move that asks *what are we contracted on?* before the front order is even chosen.

Socratic answers that question by **generating** a contract through self-interrogation — which assumes task-start is where the thinking begins. Three months into a many-months build, that assumption is false. The binding already exists — in an ADR, an issue's acceptance criteria, a backlog item, a design note, last session's plan or TaskList. The contract is not absent; it is **elsewhere, in the ledger**. So the move is not *create a contract* but *identify what already binds this build, and load it*. This is the same posture `core.md` already states — "when you can't locate what a vague command refers to, the referent is itself the uncertainty — name it, don't hunt for it" — and the ledger philosophy behind ADR-128: memory does not get to shortcut finding the artifact that already holds the decision.

## Decision

Introduce **contract identification** as an explicit gate at the front of the develop loop — the first move, run before the variable front order (design / prototype / adr) is selected — expressed as a new thin way in the develop front.

The gate is **recovery-first, not creation-first**. It reconciles the work-in-hand against the project's existing ledger — ADRs, issues and their acceptance criteria, backlog items, design notes, the prior session's plan or open TaskList — and produces a short contract surface: what this work commits us to, its acceptance test, and what remains open. Three branches:

- **Binding found** → surface the contract cheaply and proceed. No authoring.
- **Binding partial** → the gaps *are* the open questions. Batch them (via `meta/choices`); do not invent answers.
- **No binding at all** → the absence is the finding. Name it, and route to a binding-establishing stage — `adr`, `design`, or the `implement` briefing — to create one. (Not `prototype`: a prototype burns down uncertainty but produces no binding.) Do not build against nothing.

Authoring a contract is the no-binding branch, never the default path. This deliberately inverts socratic: socratic generates the contract by self-interrogation; we recover it from the ledger and generate only on absence. The inversion is what keeps the gate cheap on routine turn-40 work and honest to the ledger — a self-generated contract that duplicates an ADR already on disk is the memory-shortcut ADR-128 warns against, wearing a new costume.

The gate wires into three existing loop-control surfaces (ADR-165) rather than standing alone:

- **`/start`** already checks tracking state on session open; it extends to "what binds the work being resumed."
- **`/develop`** runs the gate before selecting the front order — the reconciliation is what *tells* you where the uncertainty lives.
- **`/implement`** consumes the recovered contract in its briefing instead of re-deriving intent from scratch.

## Consequences

### Positive

- A cheap, always-on "what are we contracted on?" gate exists below the `implement` briefing threshold, covering routine work the briefing was too heavy to gate.
- Long-project sessions stop building against nothing — the gate forces a reconciliation with the ledger before code, even when no contract was created in the current session.
- The ledger is reused, not duplicated — recovery reads ADRs / issues / plans rather than regenerating intent, keeping memory-shortcut pressure off (ADR-128).
- Absence of a binding becomes a first-class, named signal that routes to the existing front, rather than a silent gap the build papers over.

### Negative

- One more front stage to teach, and a risk of ceremony if it reads as mandatory on trivial edits. Mitigated by two facts: recovery is cheap (a ledger read, not an interrogation), and it is a develop-*loop* stage, not a global hook — trivial edits that never enter the loop never trigger it.

### Neutral

- Requires a new way file in the develop front plus wiring into the `start` / `develop` / `implement` skills; it refines the ADR-165 loop rather than replacing any part of it.
- Leans on the `tracking` way's session-start check as the resumed-work half of recovery.

## Alternatives Considered

- **Fold it into the `implement` briefing** (extend the briefing to fire earlier and cheaper). Rejected: the briefing sits above the routine-work threshold by design; stretching it down loads briefing ceremony onto small tasks, which is exactly the friction the gate is meant to avoid. The gap is *below* the briefing, not inside it.
- **Extend `tracking` + `start` only** (treat it purely as session-orientation). Rejected: it is a build-front concept, not just an open-the-session one. It must fire on fresh work started mid-session, not only when picking up prior state at session open — which is a develop-loop responsibility, not a bookend one.
- **Adopt socratic's generate-a-contract model wholesale.** Rejected: creation-first duplicates the ledger and imports socratic's 697-question checklist — an enumerated tick-list that the corpus deliberately resists (it discloses judgment, not checklists; ADR-128 / the memory doctrine). Recovery-first takes the one idea socratic exposed without the mechanism the framework is built to reject.
