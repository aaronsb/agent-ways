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

Some files exist to *describe* or *derive from* something else — a README describes the codebase, a lockfile derives from a manifest, a generated client derives from a schema. When nothing in CI forces them to stay current, they drift silently: nothing fails, nobody notices, and the cost lands weeks later on whoever trusts the stale artifact.

The check below looks at one signal — how far the recorded history of these artifacts lags the history of what they track — and surfaces a note only when the gap is wide *and* nothing already in flight closes it. Silence means things are keeping pace.

## What it catches — and what it doesn't

- **Catches:** the artifact nobody has touched while its source moved on. The abandoned README. The generated file last regenerated dozens of commits ago.
- **Misses:** the artifact that's quietly *wrong* while still being edited — a stale count, a dead link, an enumeration that no longer matches the code. History age can't see content; that drift only surfaces when a human notices it.

Treat the note as "here's a good moment to look," not "here's a bug." A stable utility's README that hasn't changed in a year is often exactly right. If the note fires on something that turns out fine, that's the heuristic working honestly, not failing — but a surface that nags trains its reader to ignore it, so keep the threshold loose enough that a fire means something.

## When you touch one of these artifacts

Reconcile the parts that assert facts — counts, lists, supported-version tables, links — against the current source. That's exactly the drift this check is blind to, and the moment you're already in the file is the cheapest time to fix it.

## Scope

This is about *consistency* drift, not available upgrades. "Your dependencies have newer versions" is a different concern with its own tooling — see code/supplychain/depscan. Freshness asks "has this artifact kept pace with what it's supposed to track," not "is there something newer out there."

## See Also

- docs(softwaredev) — how to author and structure documentation
- code/supplychain/depscan(softwaredev) — outdated dependencies are a separate concern
