---
status: Rejected
date: 2026-04-22
deciders:
  - aaronsb
  - claude
related:
  - ADR-105
  - ADR-107
  - ADR-108
  - ADR-125
---

# ADR-127: Full-body embedding corpus for way matching

## Context

Way matching (ADR-108) embeds a keyword-curated string per way — the
`description` field plus the `vocabulary` list — rather than the way's body
prose. When specific parent/child promotions failed (e.g. `environment/deps`
dominating `supplychain/depscan/node`), the instinct was that a denser text
signal would sharpen discrimination. That instinct carried an implicit premise:
**routing quality is limited by the text-matching dimension and can be improved
by making the embedded representation smarter.**

This ADR tests that premise by productizing full-body embedding in two shapes,
and documents what the test revealed about where routing quality actually lives.

## Considered

Two variants against the existing `description + vocabulary` baseline, evaluated
on a 90-way global corpus with 18 prompts (16 actionable across three firing
bands, 2 negative):

- **full-truncated** — embed the first ~256 tokens of each way's body
- **full-chunked** — embed overlapping chunks of the full body, max-pool to a
  single vector

## Results

| Variant | Rebuild | Cost × | Clear 8 | Cofire 5 | Bound 3 | Top-1 |
|---|---|---|---|---|---|---|
| baseline | 569 ms | 1.00× | 6/8 | 4/5 | 1/3 | 11/16 |
| full-truncated | 3918 ms | 6.89× | 7/8 | 3/5 | 1/3 | 11/16 |
| full-chunked | 5252 ms | 9.23× | 6/8 | 3/5 | 2/3 | 11/16 |

All three variants tie at 11/16 top-1 hits. Full-body variants fix some
parent-promotion failures and introduce new ones at the same rate:

- **P1** `audit npm dependencies for known vulnerabilities` — baseline promotes
  parent `environment/deps` over expected `supplychain/depscan/node`. Full-body
  variants correctly return the child.
- **P7** `decide between two approaches for the user schema` — baseline promotes
  `meta/knowledge` over expected `architecture/design`. Full-body variants
  correctly return design.
- **P9** `catch me up on what happened in my inbox overnight` — baseline
  correctly returns `ea/briefing` at 0.597; both full-body variants are pulled
  to `ea/email` at 0.297–0.341.
- **P10** `find a free 30-minute slot on my calendar` — all three correct, but
  baseline confidence 0.671 vs full-body 0.414/0.420.

Chunked vs truncated is a wash on recall. Chunking costs 34% more than
truncation, confirming the content swap is doing any work, not the
chunking+pooling mechanism.

The softwaredev-only pilot's apparent win was sample homogeneity: technical
prose is dense enough that full-body's broader signal dominates keyword
curation. Once the corpus includes conversational (`ea/`), operational
(`itops/`), and reflective (`meta/`) trees, full-body wins and losses cancel.

## Decision

**Rejected.** Do not productize full-body embedding — neither truncated nor
chunked.

The premise the experiment was testing — that text-matching density is the axis
of improvement — is not supported by the data. Net-zero aggregate recall across
a 7–9× rebuild-time penalty is the surface reading. The deeper reading is
structural.

Siblings in the authored graph share vocabulary *by design* — they are authored
into the same subgraph. No text-only representation, keyword-curated or
full-body, can reliably disambiguate them. P9 is the cleanest demonstration:
`ea/briefing` and `ea/email` are graph neighbors with overlapping surface
language, and baseline only picks correctly through keyword luck. Full-body's
denser signal reveals the underlying ambiguity rather than resolving it, which
is why confidence regresses on correct hits (P10: 0.671 → 0.414).

The text-matching axis is exhausted.

## Forward path

The real value in way routing is the authored graph itself (ADR-125):
parent/child structure, progressive disclosure (ADR-105), firing history, and
neighborhood context. Embeddings are one coordinate on each node, not the
routing algorithm.

**Tactical (bridging):**
- Targeted vocabulary adjustments when a specific sibling collision becomes
  painful.
- Per-way `embed_threshold` tuning under ADR-125.

**Strategic:** graph-aware routing. Subsequent ADRs should explore how
disclosure state, parent context, and recency break ties that text matching
cannot. This ADR's contribution to that direction is negative evidence — the
text-matching axis has been tested and yielded.

## Provenance

Full results, per-prompt top-5 for all 18 prompts, and timing distribution were
produced by the harness at `experiments/chunked-embeddings/` on branch
`experiment/full-body-embeddings`. Both were discarded with this ADR; the
evidence above is the preserved record.
