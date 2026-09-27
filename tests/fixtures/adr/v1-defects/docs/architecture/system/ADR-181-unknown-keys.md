---
contract: adr/v1
kind: decision
verb: add
capability: adr
date: 2025-06-01
deciders: [developer, agent]
agent: {name: Claude, model: fixture-model}
status: proposed
basis:
  - evidence: data
considered:
  - operator: developer
    said: ok
    via: "PR 3"
    cover: [probe-1]
concern:
  - said: "typo in a key"
    resolv: "x"
    answer: {said: ok, via: "PR 3"}
---

# ADR-181: Unknown keys in considered and concern

## Summary

- **Decided:** the decision in plain terms.
- **Trades away:** what it gives up.
- **Probes:** *Confident:* the main point holds. *Not confident:* the edge case.
- **Inversion:** one end, the other end; is the middle right?

## 1. Decision

The decision.

## 2. Consequences

They follow.
