path: docs/architecture/system/ADR-114-open-concern.md
---
contract: adr/v1
kind: decision
verb: constrain
capability: ingest
status: accepted
date: 2025-05-14
deciders: [developer, agent]
agent: {name: Claude, model: fixture-model}
basis:
  - evidence: load test at 10x volume
concern:
  - said: "Batch size may starve small tenants"
    resolve: "Measure p99 latency for a small tenant"
---

# ADR-114: Cap batch size

## Summary

- **Decided:** the decision in plain terms.
- **Trades away:** what it gives up.
- **Probes:** *Confident:* the main point holds. *Not confident:* the edge case.
- **Inversion:** one end, the other end; is the middle right?

## 1. Decision

Cap batches at 500 documents.
