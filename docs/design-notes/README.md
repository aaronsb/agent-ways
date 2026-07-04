# Design Notes

Design notes are prose-first architectural framing documents. They differ from ADRs in one key way: **ADRs record decisions; design notes record readings of the system**.

Design notes are reader-facing prose, so they follow the project's register rules: project-coined terms are introduced through the established concepts they implement (see [vocabulary.md](../vocabulary.md)).

## When to write a design note

Write a design note when you need to capture a framework, principle, or north star that:

- Justifies multiple related decisions rather than being one itself
- Requires prose exposition to make the implied decisions comprehensible
- Establishes vocabulary or conceptual language that future ADRs will cite
- Reads the system as a whole rather than deciding a specific tradeoff

Write an ADR instead when you have a specific decision with identifiable alternatives and tradeoffs.

## Difference from ADRs

| | ADR | Design Note |
|---|---|---|
| Form | Structured (context, decision, consequences, alternatives) | Prose, as long as needed |
| Lifecycle | Status: Draft → Accepted → Deprecated/Superseded | No status lifecycle |
| Answers | "Why did we choose X over Y?" | "How should we read this part of the system?" |
| Cites | Other ADRs | Other ADRs, design notes, external references |
| Cited by | Other ADRs, code comments | ADRs, other design notes |

## Relationship to ADRs

Design notes and ADRs complement each other. A design note establishes a framing; the ADRs it motivates cite the note for their Context sections. This keeps ADRs focused on their specific decision while the broader framing lives in one stable place.

ADRs can reference design notes as foundational context. Design notes reference ADRs when they describe the system as currently decided. A design note does not supersede, deprecate, or block ADRs — it only provides reading-level context. If the reading turns out to be wrong, the note is updated or retired, and affected ADRs are revisited.

## Index

- [Cognitive Loop and the Awareness Layer](./cognitive-loop-and-awareness-layer.md) — reading of the system's cognitive architecture: turn-based temporal accounting, substrate separation, the awareness layer, insistence as informational pressure, agency preservation
- [Attend: Messaging Disclosure with Token-Gated Reheat](./attend-messaging-disclosure-reheat.md) — how attend re-teaches its own messaging affordances over a session's lifetime: spaced repetition over token distance, delivered through the existing sensor pipeline
- [Autonomy as a Layered System: Goals, Signposts, and the Initiation Pattern](./autonomy-goals-signposts-initiation.md) — reading the autonomy surface: the built-in goal loop as primitive, the initiation pattern, signpost events, the substrate ladder, the gate taxonomy, and the three-layer safety stack
- [The Lexical Gate as a Conditional Threshold](./lexical-gate-as-conditional-threshold.md) — reading the ADR-155 keyword gate as a per-keyword detector whose operating point (`b = γ·T`) is placed by a single per-way threshold that also governs the semantic lane; the separability geometry behind the pattern-hygiene remedies, the one coupling the control surface cannot yet break, and the relationship to log-odds lexical–semantic fusion
