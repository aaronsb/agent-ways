# adr-consider setup: a project on the adr/v1 contract with one proposed
# decision the operator started, so accepting it needs their consideration.
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
- **Probes:** *Confident:* the latency win is real. *Not confident:* whether 30 seconds of staleness is acceptable to users.
- **Inversion:** one end caches nothing and scales the database; the other caches everything with invalidation. This sits between. Is the middle right?

## 1. Decision

Cache list responses for 30 seconds behind a flag.
MD
git add -A && git commit -qm "adr scenario setup"
