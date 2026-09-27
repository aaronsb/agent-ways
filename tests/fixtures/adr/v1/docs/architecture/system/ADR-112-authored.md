---
contract: adr/v1
kind: decision
verb: constrain
capability: [adr, ingest]
status: accepted
date: 2025-05-12
deciders: [developer, agent]
agent: {name: Claude, model: fixture-model}
basis:
  - operator: developer
    level: authored
    said: "Written by the operator"
    via: "PR #12"
  - standard: NIST SP 800-53 AU-2
  - upstream: kg ADR-304
considered:
  - operator: developer
    said: "yes"
    via: "PR #12"
    canary: missed
---

# ADR-112: Operator-authored constraint

## Summary

- **Decided:** the decision in plain terms.
- **Trades away:** what it gives up.
- **Probes:** *Confident:* the main point holds. *Not confident:* the edge case.
- **Inversion:** one end, the other end; is the middle right?

## 1. Decision

The decision.

## 2. Consequences

They follow.
