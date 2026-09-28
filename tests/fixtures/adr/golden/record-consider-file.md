path: docs/architecture/system/ADR-115-named-probes.md
---
contract: adr/v1
kind: decision
verb: add
capability: adr
status: accepted
date: 2025-05-20
deciders: [developer, agent]
agent: {name: Claude, model: fixture-model}
basis:
  - evidence: fixture measurement
considered:
  - operator: developer
    said: "\"Fine\" (Recommended)"
    via: session 2025-05-20, selected from agent-written options
    covers: [band-hint, inversion]
    canary: caught
---

# ADR-115: Named probes

## Summary

- **Decided:** the decision.
- **Probes:** *Confident (identity-stable):* a. *Not confident (band-hint):* b.
- **Inversion:** c.

## 1. Decision

The decision.
