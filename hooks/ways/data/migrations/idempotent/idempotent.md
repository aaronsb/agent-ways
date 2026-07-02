---
description: writing idempotent, re-runnable schema migrations that survive a retry or partial failure without erroring on the second pass
vocabulary: idempotent rerun retry replay guard conditional if not exists create or replace on conflict drop if exists partial failure resume safe reentrant
pattern: idempoten|if not exists|create or replace|on conflict|re-?runnable|re-?run|drop .*if exists
embed_threshold: 0.37
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Idempotent Migrations

A migration can fail partway, get retried, or be re-scanned by a runner that
isn't sure whether it applied. **Write every migration so running it twice is a
no-op on the second pass, not an error.** That's what makes an interrupted
deploy recoverable instead of wedged.

## Guard every object

| Operation | Idempotent form |
|---|---|
| Create table / index / view | `CREATE TABLE IF NOT EXISTS`, `CREATE INDEX IF NOT EXISTS` |
| Add / drop column | `ADD COLUMN IF NOT EXISTS`, `DROP COLUMN IF EXISTS` |
| Replace a function / view | `CREATE OR REPLACE` |
| Seed / reference data | `INSERT … ON CONFLICT DO NOTHING` (or `MERGE`) |
| Anything without an `IF [NOT] EXISTS` (some constraints) | wrap in a catalog check — `DO $$ BEGIN IF NOT EXISTS (SELECT … FROM pg_constraint …) THEN … END IF; END $$;` |

## Record the version last

Insert the applied-version row into the ledger as the **final statement, inside
the same transaction** as the changes. If the body fails, the row is never
written and the migration is retried cleanly next run. Use `ON CONFLICT DO
NOTHING` on that insert too, so a re-scan doesn't error.

## Verify

Run the migration, then run it **again** against the same database. The second
run must complete with no error and no change. If it throws, an object isn't
guarded — fix it before the migration leaves your machine.

## See Also

- data/migrations/numbering(data) — the ledger this relies on
- data/migrations(data) — general migration discipline
