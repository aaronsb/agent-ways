# Vocabulary

This project coined its own terms before finding the established ones. The invented names stay — they name the *implementations* — but every reader-facing document introduces them through the established concept they implement. This file is the normative reference for those anchors (ADR-301).

## What this project is, in established terms

> Ways is organizational socialization for language-model agents. Because an LLM session cannot internalize norms — every session is a new hire — the system re-enacts socialization mechanically: local norms ("the way we do it around here") delivered situated, at the moment of relevant action, on a spaced schedule that substitutes for the memory the agent does not have. Attend is the awareness an employee would otherwise have ambiently: what is changing, who else is working, what deserves attention.

One sentence, no invented words: *procedural memory for coding agents, maintained by spaced repetition, plus a perception loop with alarm management.*

## Terminology anchors

| Project term / mechanism | Established term | Source |
|---|---|---|
| Ways (the corpus and its delivery) | **Organizational socialization**, delivered as **situated learning** | Van Maanen & Schein 1979; Lave & Wenger 1991 |
| Ways as agent memory | **Procedural memory** retrieved into working memory | CoALA (Sumers et al. 2023); ACT-R |
| Salience decay + re-disclosure at a floor | **Forgetting curve** and **spaced repetition** | Ebbinghaus; SuperMemo/Anki lineage |
| Refractory gates, habituation, burst detection | **Base-level activation decay** | ACT-R |
| Attend's emission governor, quiet footnotes | **Calm technology**; **interruption cost**; **alarm management** | Weiser & Brown; Mark; ISA-18.2 |
| Peer presence, heartbeats, instance identity | **Workspace awareness** | CSCW (Dourish & Bellotti 1992) |
| Way authoring from team norms | **Externalization of tacit knowledge** | Nonaka & Takeuchi (SECI) |
| Way matching and fixture testing | **Information retrieval**, precision-first evaluation | Cranfield paradigm |
| Relevance gate (the judge on matched ways) | **Retrieve-then-rerank**, with an **LLM-as-a-judge** as the reranker | Nogueira & Cho 2019; Zheng et al. 2023 |
| The ways agent (resident daemon holding the key) | A per-user **credential agent** | `ssh-agent`, `gpg-agent` |
| Projection into `~/.claude` | A **symlink farm** over an application installed in XDG directories | GNU Stow; XDG Base Directory Specification |
| Progressive disclosure | Progressive disclosure (industry-converged term) | — |
| Agent behavior without ways (follow-ups, hedging) | **Preference uncertainty** | Principal–agent theory |

## Why "ways"

The name comes from the phrase *"that's the way we do it around here"*, and each word is load-bearing:

- **"we"** — multiple aligned actors. The peer layer: instances, presence, sessions as colleagues rather than isolated processes.
- **"around"** — approximate, not exact. Norms have fuzzy boundaries, so the matcher is an embedding model with thresholds, not a rulebook with exact triggers.
- **"here"** — local, not universal. Project ways override global ways; the same agent in a different directory is in a different "here."

"Ways of working" is itself established management vocabulary for team norms.

## The constraint the human literature doesn't have

Socialization theory assumes the newcomer persists: the organization pays the onboarding cost once, and internalization does the rest. An LLM session cannot internalize — no weight updates, no carried memory; every session is a new hire. So this system substitutes **re-enactment for internalization**: socialization performed mechanically, every session, at the moment of relevant action, on a spaced schedule tuned to fake a memory the agent does not have. That constraint is why the firing-dynamics machinery exists.

## Register rules for documentation

1. **Anchor on first use.** A project-coined term is introduced with its established anchor the first time a document uses it — "salience decay (the forgetting curve applied to injected guidance)" — never bare.
2. **What it is, then how we built it.** State the mechanism in established terms first, then the project's implementation in project terms.
3. **Analogies have limits.** Write "this is X applied to Y," never "this is X." CoALA's procedural memory includes agent code; ACT-R activation governs retrieval, not injection. Over-claiming equivalence is a different flavor of noise.
4. **The newcomer test.** A revised paragraph earns its revision by getting *easier* to read for someone new, not by citing more.

## See also

- [ADR-301](architecture/documentation/ADR-301-situated-socialization-as-canonical-framing-and-documentation-prose-refactor.md) — the decision behind this file
- [hooks-and-ways/rationale.md](hooks-and-ways/rationale.md) — why the system exists, including which halves are durable
