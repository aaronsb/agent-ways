---
contract: adr/v1
kind: decision
verb: add
capability: adr
status: accepted
date: 2025-05-01
deciders: [developer, agent]
agent: {name: Claude, model: fixture-model}
basis:
  - operator: developer
    level: guided
    said: "agent-ways leads adoption, and its own corpus is the test"
    via: session 2025-05-01
  - evidence: fixture triage of 108 records
considered:
  - operator: developer
    said: "looks good; the capability list is fine for now"
    via: PR #1
    covers: [probe-2, inversion]
    canary: caught
concern:
  - said: "The capability list may be friction for small repos"
    resolve: "Operator confirms the list is acceptable"
    answer: {said: "fine for now", via: "PR #1"}
  - said: "Section references accept two forms"
    resolve: "Pick one canonical form"
    withdrawn: "Both forms are unambiguous; no longer a concern"
---

# ADR-100: Adopt the adr/v1 contract

## Summary

Context for the record.

## 1. Decision

The decision.

## 2. Consequences

They follow.
