---
description: consolidating a long migration history into a single generated checkpoint baseline, and proving the baseline faithful by replay-and-diff before retiring the old files
vocabulary: consolidate consolidation checkpoint baseline squash compact collapse rebaseline migration history archived replay diff drift snapshot faithful
pattern: consolidat|checkpoint.?(baseline|migration|schema)|baseline.?(migration|schema|snapshot)|squash.?migration|compact.?migration|re-?baseline|collapse.?migration
embed_threshold: 0.42
scope: agent, subagent
refire: rare
---
<!-- epistemic: heuristic -->
# Migration Checkpoints

When a history grows to dozens of files, every fresh install replays all of
them. Periodically **collapse the resting state into one generated baseline** —
but consolidation is a place to be careful: get it wrong and every future
install starts from a subtly wrong schema.

## Generate the baseline — never hand-merge

Replay the old baseline **plus every migration** into a throwaway database, then
dump the schema and seed data. That dump *is* the new baseline. Hand-merging
migration files by eye drifts from what the migrations actually produce.

## Prove it faithful before retiring anything

Build a second throwaway database from the **candidate baseline alone** and diff
it against the replayed original:

- full schema dump (tables, columns, types, constraints, indexes)
- per-table row counts, and normalized seed data
- for graph/extension stores, anything a plain dump misses (e.g. an AGE label
  catalog) — carried into the baseline verbatim, since it can't round-trip

They must match exactly. If they don't, the baseline is wrong — stop.

## Keep the old path working

- **Retain the old files** (move to `archived/`, don't delete). A database created
  before the checkpoint still needs them; the runner scans `archived/` first.
- The baseline **records every consolidated version in the ledger**, so a fresh
  install marks them applied and skips straight to the next real migration.

## Observability

A checkpoint apply has a fingerprint: every consolidated version shares one
`applied_at` timestamp cluster (one baseline wrote them together), versus spread
timestamps for an incremental replay. Query it to confirm which path a database took.

## See Also

- data/migrations/numbering(data) — the ledger the baseline pre-populates
- data/documentation(data) — regenerate schema docs from the new baseline
