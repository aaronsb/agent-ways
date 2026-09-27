---
status: Accepted
date: 2026-06-09
deciders:
  - aaronsb
  - claude
related: []
---

# ADR-301: Situated socialization as canonical framing and documentation prose refactor

## Context

The project's documentation describes its mechanisms almost entirely in invented vocabulary — ways, attend, disclosure, reheat, firing, salience floors, insistence. The terms are internally coherent, but they are defined only by reference to each other. A reader arriving from outside (or an agent reasoning about the project) has no anchor: nothing in the prose says what body of existing knowledge any mechanism belongs to, so every document carries a tax of re-explanation, and the project as a whole resists the one-sentence answer to "what is this?"

This was not a style choice. The author lacked the cross-field vocabulary at authoring time, because the relevant terms are scattered across four or five disciplines that rarely cite each other. They exist:

| Project term / mechanism | Established term | Source |
|---|---|---|
| Ways (the corpus and its delivery) | **Organizational socialization**, delivered as **situated learning** | Van Maanen & Schein 1979; Lave & Wenger 1991 |
| Ways as agent memory | **Procedural memory** retrieved into working memory | CoALA (Sumers et al. 2023); ACT-R |
| Salience decay + re-disclosure at a floor | **Forgetting curve** and **spaced repetition** | Ebbinghaus; SuperMemo/Anki lineage |
| Refractory gates, habituation, burst detection | **Base-level activation decay** | ACT-R |
| Attend's emission governor, quiet footnotes | **Calm technology**; **interruption cost**; **alarm management** | Weiser & Brown; Mark; ISA-18.2 |
| Peer presence, heartbeats, instance identity | **Workspace awareness** | CSCW (Dourish & Bellotti 1992) |
| Way authoring from team norms | **Externalization of tacit knowledge** | Nonaka & Takeuchi (SECI) |
| Progressive disclosure | Progressive disclosure (already converged — Anthropic uses the term for Skills) | — |

The name "ways" itself turns out to be load-bearing rather than arbitrary. It comes from the phrase *"that's the way we do it around here"*, and each word maps to architecture: **"we"** — multiple aligned actors (the peer layer); **"around"** — approximate boundaries (embedding-based matching rather than exact rules); **"here"** — local scope (project ways overriding global). The phrase is itself established management vocabulary ("ways of working").

Two findings from this framing belong in the record because they shape what the documentation should say:

1. **Re-enactment substitutes for internalization.** Human socialization theory assumes the newcomer persists — the organization pays the onboarding cost once and internalization does the rest. An LLM session cannot internalize (no weight updates, no carried memory); every session is a new hire. The system therefore re-enacts socialization mechanically, every session, at the moment of relevant action, on a spaced schedule that substitutes for the memory the agent does not have. This is the genuinely novel constraint relative to the human literature, and the honest justification for the firing-dynamics machinery.

2. **The durability split.** Ablation testing (removing the ways system) produces the same behavior across model tiers: agents become approval-seeking — constant follow-ups or exhaustive hedging. The established name for the cause is **preference uncertainty** (principal–agent theory): an agent that does not know its principal's norms can only ask or hedge. This separates the system's two functions by durability: the *scheduling* half (decay curves, re-disclosure) compensates for a model deficiency and may erode as models improve; the *routing* half (just-in-time delivery of local norms the model cannot know because it was never told) is structural and permanent. The scheduling half of ways is a patch on current models; the routing half is a permanent answer to preference uncertainty.

## Decision

Adopt **situated socialization for language-model agents** as the project's canonical framing, and refactor documentation prose to lead with established vocabulary.

The canonical one-paragraph description:

> Ways is organizational socialization for language-model agents. Because an LLM session cannot internalize norms — every session is a new hire — the system re-enacts socialization mechanically: local norms ("the way we do it around here") delivered situated, at the moment of relevant action, on a spaced schedule that substitutes for the memory the agent does not have. Attend is the awareness an employee would otherwise have ambiently: what is changing, who else is working, what deserves attention.

Concretely:

1. **Terminology anchors are normative.** The mapping table above moves into the documentation as a reference. Project-coined terms remain in use, but on first use in any document they are introduced as implementations of their established anchor ("salience decay — the forgetting curve applied to injected guidance — …"), never bare.
2. **Prose refactor, not content rewrite.** Each document in scope is revised to state *what the mechanism is* in established terms first, then *how this project implements it* in project terms. Content, diagrams, and examples are preserved; the register changes. Scope: `README.md`, `docs/hooks-and-ways/`, `docs/attend-and-monitor/`, `docs/design-notes/`, and skill/way prose that describes the system to readers (e.g. `skills/attend/SKILL.md` intro).
3. **ADRs are exempt.** Existing ADRs are historical decision records and remain untouched. New ADRs adopt the vocabulary going forward.
4. **The ways corpus is exempt.** The machine layer (`hooks/ways/**/*.md` bodies) is guidance for agents in other projects, already terse and functional; it is not part of the descriptive-noise problem.
5. **The durability split is documented.** The scheduling-vs-routing distinction and its maintenance implication (retune half-lives per model generation via `ways tune`; defend the routing pipeline) is written into the architecture overview so future maintainers know which parts to let atrophy.

## Consequences

### Positive

- The project becomes explainable in one sentence built entirely from terms with literatures behind them: *procedural memory for coding agents, maintained by spaced repetition, plus a perception loop with alarm management.*
- Prior art becomes searchable. Each mechanism now names the field it can be evaluated against, instead of appearing sui generis.
- Naming future work gets easier — deferred features inherit anchors (insistence tracker → escalation in alarm management; consequence model → projection/appraisal).
- The ablation observation ("agents become needy without ways") gains a precise causal account (preference uncertainty), strengthening the case the documentation makes.

### Negative

- The refactor touches most reader-facing documents; it is real editing effort and risks introducing inconsistency mid-flight if done piecemeal.
- Anchors are analogies with limits, not identities — e.g. CoALA's procedural memory includes agent code, ACT-R activation governs retrieval rather than injection. Over-claiming equivalence would trade one kind of noise for another. The refactor must say "this is X applied to Y," not "this is X."
- Academic register can curdle into jargon of a different flavor. The test for every revised paragraph is whether it got *easier* to read for a newcomer, not whether it cites more.

### Neutral

- Existing ADRs and the ways corpus are unchanged by design.
- The invented terms survive — they name the implementations, which the established terms do not do. This ADR settles their *introduction*, not their existence.
- The etymology of "ways" (we / around / here) becomes part of the documented narrative rather than conversational lore.

## Alternatives Considered

- **Glossary only** — add a terminology appendix, leave prose as-is. Rejected: a glossary is a patch on the register problem; readers hit the bare invented term first and the noise remains in every document.
- **Full rename to established terms** — rename components (ways → norms, attend → awareness, salience → activation). Rejected: the invented names are good — "ways" in particular encodes the design (we/around/here) — and the churn would touch every binary, hook, and document for negative gain.
- **Do nothing** — accept the invented vocabulary as the project's idiom. Rejected: the descriptive noise is a measured cost (the project's author cannot briefly explain the project), and the durability argument — the most important strategic fact about the system — currently exists nowhere in the documentation.
