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
concern:
  - said: "No resolution named"
  - said: "Both"
    resolve: "x"
    answer: {said: a, via: b}
    withdrawn: "y"
  - said: "Silent withdrawal"
    resolve: "x"
    withdrawn: ""
  - said: "Half an answer"
    resolve: "x"
    answer: {said: "ok"}
  - said: "Still open, about hook latency"
    resolve: "Measure it"
---

# ADR-157: Malformed and open concerns

## Summary

- **Decided:** the decision in plain terms.
- **Trades away:** what it gives up.
- **Probes:** *Confident:* the main point holds. *Not confident:* the edge case.
- **Inversion:** one end, the other end; is the middle right?

## 1. Decision

The decision.

## 2. Consequences

They follow.
