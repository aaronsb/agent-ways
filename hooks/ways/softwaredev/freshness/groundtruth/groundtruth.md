---
description: deriving how a system actually behaves from executable artifacts (code, migrations, runtime config) rather than from docs, ADRs or specs, and capturing a golden-master baseline before a refactor or migration so the diff afterward proves the behavior did not change
vocabulary: ground truth source of truth authoritative security review reconcile docs vs code spec vs implementation stale drift what does the system actually do baseline supersede golden master oracle refactor migration behavior preserved no behavior change same outputs recapture intended delta pinning
pattern: source.?of.?truth|ground.?truth|security.?review|reconcile|docs?.vs.?code|spec.vs.?implementation|actually (do|behave|work|enforce)|is (this|the|that).{0,30}(up.?to.?date|still (true|accurate|current))|stale (adr|doc|spec)|golden.?master
scope: agent, subagent
refire: 0.2
---
<!-- epistemic: heuristic -->
# Ground Truth Way

The parent way catches the artifact that *stopped changing* while its source moved on. This child is about the opposite, sneakier case: the document that is still being read, edited, and trusted while being **semantically wrong** — the ADR whose described mechanism the code never implemented, the design doc whose enumeration the schema quietly overran. History age can't see it; only reading both sides can.

## The move

When you need to state how a system *actually* behaves — and especially when you're building a model to measure other things against (an audit, a security review, a contract for downstream work) — derive ground truth from the **executable** artifacts:

- code that runs, **schema/migrations** that seeded the live state, config the runtime actually reads.

Treat the prose layer — ADRs, design notes, specs, READMEs, docstrings — as **claims to verify**, not as truth. Read it, but confirm each load-bearing assertion against the executable side before you rely on it.

The same holds for what you *write out*: a citation, an attribution, or a named term is itself a claim. Verify it against the cited artifact — the ADR you're quoting, the standard you're naming, the API you're describing — not your memory of it. An unchecked citation is an assertion in costume.

This inverts the usual reflex ("the ADR says X, so X"). The failure mode it prevents is the one that compounds: a wrong premise adopted early mis-shapes everything built on it. If the yardstick is stale, every measurement taken with it is off.

## Each divergence is a finding

A gap between what a doc claims and what the code does is not noise to silently reconcile in your head — it's a result. Surface it. It's usually one of:

- **doc stale, code right** — the doc describes an intent the code moved past (update/supersede the doc),
- **code wrong, doc right** — the code violates a still-valid decision (fix the code), or
- **both adrift** — the decision itself needs revisiting.

Naming *which* of these it is, with the evidence, is most of the value.

## When the drift is pervasive: baseline, don't patch

Design docs accrete like migrations. When enough of a domain's docs have drifted that patching each one leaves the reader reconciling overlapping half-truths, the clean move is the same one a migration chain eventually makes: **find the sum of what the code actually does, write one fresh baseline that says it, and supersede the drifted predecessors** — preserving them as history, not deleting them. A baseline that describes the implemented system (with the remaining code/doc gaps tracked as explicit work) beats five aspirational documents nobody trusts.

## The golden-master oracle

Before a refactor, a migration, or a re-platforming, "it builds and the tests pass" says nothing about preserved behavior. Capture the current observable outputs (endpoint responses, persisted shapes, message payloads, computed results) over a representative input set, one baseline per consumer, normalized to mask only volatile leaves such as timestamps and ids. Review the capture and land it as its own slice, before any code change. Capture it read-only: once the migration has overwritten the system, that evidence is gone. On a codebase with no tests this baseline is often the only executable gate; say so, and treat effort estimates as floors.

After the change, re-capture and diff. The gate passes when everything matches the baseline except an enumerated list of intended deltas, each row naming the change and why it is deliberate. An unexplained diff, or an additive-only edit to the pinning tests, is justified before the gate goes green. Never re-baseline silently.

## See Also

- freshness(softwaredev) — the parent: history-age drift in derived artifacts
- adr(documentation) — superseding and baselining ADRs through the proper workflow
- code/testing/gates/assertions(softwaredev) — the shape of the baseline assertion: snapshot and golden baselines, the known-bug marker
