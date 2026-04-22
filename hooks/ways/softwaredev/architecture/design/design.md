---
description: software system design, architecture patterns, database schema, component modeling, proposals, RFCs, design deliberation
vocabulary: architecture pattern database schema modeling interface component modules factory observer strategy monolith microservice microservices domain layer coupling cohesion abstraction singleton proposal rfc sketch deliberation whiteboard
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Design Way

## Design Discussion Framework

1. **Context**: What problem are we solving?
2. **Constraints**: What limits our options?
3. **Options**: What approaches could work?
4. **Trade-offs**: What does each option cost/gain?
5. **Decision**: What do we choose and why?

When the design involves architectural trade-offs worth documenting, escalate to an ADR (see ADR Way).

## RFCs and Proposals

RFCs are the "before" to an ADR's "after" — they capture deliberation while the design is still open. Use them for changes that affect multiple teams or systems.

- **RFC**: Proposes a change, invites feedback, converges on a decision
- **ADR**: Records the decision after deliberation is complete
- Start with a sketch or whiteboard session, formalize as an RFC if the scope warrants it

## Common Patterns

| Pattern | When to Use | When NOT to Use |
|---------|-------------|-----------------|
| Factory | Complex object creation, multiple variants | Simple constructors suffice |
| Strategy | Swappable algorithms at runtime | Only one implementation exists |
| Observer | Event-driven decoupling | Tight coupling is acceptable |
| Repository | Data access abstraction | Direct queries are clearer |
| Adapter | Interface compatibility | Both sides under your control |

## Questions That Drive Design

- "What changes most frequently?" — isolate it behind an interface
- "What needs to be independently deployable?" — service boundary
- "Where are the natural boundaries?" — module/package split
- "What would make this testable?" — dependency injection point

## See Also

- architecture/adr(softwaredev) — escalate design trade-offs to ADRs
- architecture/threat-modeling(softwaredev) — security considerations during design
