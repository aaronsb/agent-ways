---
description: how migration files are numbered, ordered, and tracked — the applied-migrations ledger, gap-tolerant sequencing, and avoiding number collisions
vocabulary: numbering sequence order version ledger applied filename prefix zero-pad monotonic gap collision timestamp schema_migrations sequential
pattern: schema_migrations|migration.?(number|order|sequenc|version|ledger)|zero.?pad|\b0\d\d_|applied_at
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Migration Numbering

A migration's **number is its identity**. The rules that keep a history sane are
about never disturbing a number once it exists, because downstream databases
have already recorded it.

## The rules

- **Monotonic, append-only.** New migrations take the next number. Never renumber
  or reuse one — a database that already applied `042` will never see your new `042`.
- **Zero-pad the filename** (`001_`, `002_`) so lexical order matches numeric
  order, but **compare as integers in code** (strip leading zeros) — `10#$n` in
  bash, `int(...)` elsewhere.
- **Gaps are fine.** If `002` and `010` were never used or later dropped, readers
  skip them. Don't backfill or renumber to close a gap.
- **One ledger table** (commonly `schema_migrations`) records applied versions.
  The runner applies only the files whose number isn't in the ledger yet.

## Collisions

Two branches both adding `081` collide. Resolve it **before merge**: renumber the
not-yet-landed one to the next free number. For large teams where this happens
constantly, timestamp-prefixed names (`20260702_…`) trade readability for
collision-freedom — pick sequential for small teams, timestamps for busy ones.

## See Also

- data/migrations/idempotent(data) — the ledger insert must be idempotent too
- data/migrations/checkpoint(data) — consolidating a long, gappy history
