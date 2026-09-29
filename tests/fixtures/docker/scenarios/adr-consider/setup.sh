# adr-consider setup: a project on the adr/v1 contract with one proposed
# decision the operator started. Its one probe checks the operator's intent
# (ADR-304 §12, note of 2026-09-28 on #624).
mkdir -p docs/scripts docs/architecture/system
cp "$HOME/.claude/hooks/ways/documentation/adr/adr-tool" docs/scripts/adr
chmod +x docs/scripts/adr
cat > docs/architecture/adr.yaml <<'YAML'
project_name: Consider Scenario
contract: adr/v1
domains:
  system: {range: [100, 199], name: System, description: Runtime, folder: system}
defaults: {deciders: [developer, agent]}
kinds:
  decision:
    mutable_after_accept: [status, enacted, superseded_by, considered, concern, observable]
    verb: required
    requires: [capability, basis, agent]
    sections: [Summary]
    edges: {supersedes: decision, amends: decision, extends: decision, basis: [decision, spec]}
  spec:
    mutable_after_accept: all
    verb: forbidden
    requires: [capability]
    edges: {supersedes: spec, decided_by: decision}
capabilities:
  cache: The response cache
YAML
cat > docs/architecture/system/ADR-100-add-response-cache.md <<'MD'
---
contract: adr/v1
kind: decision
verb: add
capability: cache
status: proposed
date: 2025-05-01
deciders: [developer, agent]
agent: {name: Claude, model: scenario-model}
basis:
  - operator: developer
    level: guided
    said: "responses are slow; see whether a cache helps"
    via: session 2025-05-01
  - evidence: p95 latency 840 ms on the list endpoint
---

# ADR-100: Add a response cache

## Summary

- **Decided:** cache list-endpoint responses for 30 seconds.
- **Trades away:** up to 30 seconds of staleness on lists.
- **One-way?** No. The cache is behind a flag.
- **Probes:** *Confident (fresh-enough):* you asked for faster list pages; is list data up to 30 seconds old fine for what you need?
- **Inversion:** one end caches nothing and scales the database; the other caches everything with invalidation. This sits between them.

## 1. Decision

Cache list responses for 30 seconds behind a flag.
MD
git add -A && git commit -qm "adr scenario setup"
