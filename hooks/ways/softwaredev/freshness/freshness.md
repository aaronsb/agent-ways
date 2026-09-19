---
description: artifact freshness — surfacing files that describe or derive from something else but have drifted behind it
vocabulary: stale freshness drift outdated lagging behind neglected dormant readme docs documentation lockfile generated derived out of sync reconcile abandoned
trigger: session-start
macro: prepend
scope: agent
requires: ["Bash(git:*)"]
refire: 0.15
---
<!-- epistemic: heuristic -->
# Freshness Way

A README describes the code, a lockfile derives from a manifest, a generated client derives from a schema. Nothing fails when they fall behind. When a note appears above this text, it reports one of two signals: documentation whose git history lags HEAD by many commits with no branch carrying an update, and ADRs parked in Draft or Proposed past an age threshold. Silence means both are keeping pace.

The check reads history. It sees the abandoned artifact and misses the one that is still edited yet wrong: a stale count, a dead link, a list that no longer matches the code. When you are in one of these files, reconcile the parts that assert facts against the current source.

Treat a note as a moment to look. A stable utility's README that is a year old is often right. A parked ADR wants one of three moves: accept it, reject it, or write in it what it is waiting on.

## Scope

Consistency drift, not available upgrades. Newer dependency versions are code/supplychain/depscan's concern.

## See Also

- documentation(documentation) — how to author and structure documentation
- code/supplychain/depscan(softwaredev) — outdated dependencies are a separate concern
