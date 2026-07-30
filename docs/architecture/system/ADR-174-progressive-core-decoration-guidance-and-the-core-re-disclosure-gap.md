---
status: Draft
date: 2026-07-30
deciders:
  - aaronsb
  - claude
related:
  - ADR-123
---

# ADR-174: Progressive core — decoration guidance and the core re-disclosure gap

## Context

Two findings arrived together in one session. The second is the architectural one.

### Claude's prose decorates, and the existing surfaces did not stop it

An 11,500-word document drafted across a dozen turns was measured against the patterns that mark prose written to be admired rather than read. Significance clauses — a clause whose only job is telling the reader that the previous clause mattered — ran at **3.4 per thousand words** against **0.5** in prose that had already been reviewed. The document also carried **eleven** instances of the antithesis construction that `core.md` explicitly bans, and **118 em-dashes**, one per 98 words.

Both governing surfaces had fired. `core.md` fired at session start and contains zero instances of the construction it bans. The writing way fired at epoch 4 carrying "use em dashes sparingly." The document was written at epochs 45–57.

Two mechanisms explain the gap, and the supporting literature is consistent with both.

**Detection, not compliance, is the failure.** Models score poorly on noticing their own negative-constraint violations; IFEval-style negative constraints fail at 22–30% on frontier models, and constraint-verification work finds low negative F1 across the board. A rule restated more forcefully does not help when the writer cannot see the violation while producing it.

**Style rules decay across a long draft.** Bohr (arXiv:2511.13972) separates *initial control* from *expansion discipline* — whether a style survives a revision turn — and finds instruction-plus-example strongest on both, example-only carrying no expansion discipline at all. The observed pattern matches: the rule held while output was short and failed across an essay.

A rule containing an adverb compounds this. "Sparingly" has no threshold, so there is no moment at which compliance can be tested. "Cut any clause that explains why the previous clause matters" is a search that can actually be run.

### `core.md` is structurally excluded from re-disclosure

Investigating where the guidance should live surfaced the larger issue.

`tools/ways-cli/src/cmd/scan/state.rs` gates core on a boolean marker rather than a decay curve:

```rust
if !session::core_is_shown(session_id) {          // first time — show it
} else if let Some(tp) = transcript {
    let ctx_size = transcript_size_since_summary(tp);
    if ctx_size < 5000 && age > 30 {              // context was cleared — re-show
```

Core re-injects on one condition: the transcript since last summary is under 5000 bytes, which is a safety net for a context clear. In a long growing session `ctx_size` passes 5000 within a couple of turns and never returns, so core lands at turn 1 and is never refreshed until compaction.

Three consequences follow.

`refire: 0.15` in `core.md`'s frontmatter is inert on this path. Core is gated by `stamp_core` / `core_is_shown`, never enters the firing ledger, and does not appear in `ways list` — 70 entries in the observed session, none of them core.

The retention profile is inverted relative to the rest of the corpus. Every matched way gets a curve that *lowers* its suppression threshold as distance grows, so it becomes more eligible to re-fire the further the session runs. Core gets a gate that only opens when context is small. The file that applies to every turn has the weakest retention in the system.

This is not a defect in the safety net, which does the job it was written for. It is a gap: no path was ever built for core to re-disclose on distance, because core predates the firing-dynamics work in ADR-123.

Placing new always-relevant guidance in `core.md` would therefore place it in the one location with no re-disclosure at all.

## Decision

Adopt **progressive disclosure for core content**, using the existing parent/child pattern rather than a new mechanism, and deliver the decoration guidance across three tiers.

**Tier 1 — `core.md`.** Two bullets under Posture, sibling to the existing reasoning-tic rules, stating the rule in its shortest checkable form. Turn 1 only. This tier exists because posture shapes conversational output, which no artifact-boundary check can reach.

**Tier 2 — `meta/trust/prose/prose.md`.** A fourth child alongside `autonomy`, `delegation`, and `voice`, carrying the expanded account with paired before/after examples. Semantic and vocabulary triggers, `refire: 0.15`, and the parent boost when `trust` fires. This tier re-discloses on distance, which is what core cannot do.

**Tier 3 — `documentation/markdown/density/`.** A postcheck way, sibling to `documentation/markdown/reflow`, firing on the markdown just written. Its macro reports measured counts for that file rather than restating the rule. This tier exists because the failure is detection, and only a count closes that gap.

Firing thresholds: significance clauses at ≥3 per thousand words, or em-dashes at ≥15 per thousand, over a 150-word floor, with per-file suppression for the session. Calibrated against measurements in this repository rather than chosen. Loose deliberately — a surface that nags trains its reader to ignore it.

Two supporting changes. The writing way loses "use em dashes sparingly" as unmeasurable and gains a pointer to `trust/prose`. Em-dash count is **reported and not banned**: repository prose runs at 15.3 per thousand words against the draft's 10.7, so the punctuation is house style at volume rather than an anomaly, and only density is the tic.

This ADR records the core re-disclosure gap as a **finding, not a fix**. Giving core a distance-based re-disclosure path is a separate decision with its own cost — core is roughly 900 words, and re-injecting it on a curve risks exactly the nagging the threshold discipline above avoids. The progressive-core pattern routes around the gap without deciding it.

## Consequences

### Positive

- Always-relevant guidance gains a re-disclosing home without changing the core delivery contract.
- The measurement tier reports numbers rather than intentions, addressing the detection failure directly.
- Thresholds derive from measurements in this repository, so they can be re-derived and argued with.
- The core re-disclosure gap is now written down rather than resident in one session's context.

### Negative

- Three tiers to keep coherent. Guidance that drifts between them will contradict itself.
- The postcheck runs on every `Write`/`Edit`, adding a check to a hot path.
- Regex detection of a rhetorical pattern carries false positives. A document *about* these patterns scores high for legitimate reasons; `density.md` names this case and the path self-exclusion covers the corpus.
- The bare `, not X` form is unchecked. It over-fired on legitimate contrast in `core.md`, so narrowing it removed a real detection — the operator caught one by eye that the check misses.

### Neutral

- Core's `refire: 0.15` remains inert until the re-disclosure gap is separately decided. Leaving a field that does nothing is its own small debt.
- The prose linter prototype used to calibrate these thresholds is not shipped. Promoting it to `doclint` or a `ways` subcommand is deferred until the postcheck proves too easy to ignore.

## Alternatives Considered

**Put everything in `core.md`.** Rejected on the finding above: core has no re-disclosure path, so the guidance most needing to survive to turn 50 would land where it survives worst.

**Put everything in the writing way.** Rejected because that way fired at epoch 4 on a semantic mass-match rather than on writing, and never returned. Predictive matching picked the wrong moment, which is a routing failure the tier-3 reactive path avoids by construction.

**Ship a lint gate at the commit boundary instead of a way.** Deferred rather than rejected. The postcheck teaches during the work and is reversible; a commit gate is deterministic but arrives after the session has moved on. Revisit if the way proves ignorable.

**Give core a distance-based re-disclosure curve.** Deferred as a separate decision. It is the direct fix for the gap and it re-injects ~900 words per fire, which needs its own cost analysis and threshold work.

**Ban em-dashes outright.** Rejected on measurement. Repository prose runs higher than the draft that prompted this, and `core.md` is the densest file sampled at 20.4 per thousand. A check that flags every file gates nothing. The operator's personal prose guidance does ban them; that is a register decision for personal correspondence and deliberately not inherited.
