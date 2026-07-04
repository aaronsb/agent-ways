---
description: generating database documentation from the schema — a reference page and an entity-relationship diagram derived from the DDL, kept in sync in CI
vocabulary: schema documentation erd entity relationship diagram dbml docgen generate ddl reference data dictionary table column pg_dump introspect catalog
pattern: erd|entity.?relationship|dbml|schema.?(doc|diagram|reference|dictionary)|data.?dictionary|pg_dump.*(doc|diagram)
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Schema Documentation

Schema docs written by hand drift the moment a migration lands. **Generate them
from the DDL — the source of truth — so they can't lie.**

## Principles

- **Derive from the schema, not from memory.** Parse the DDL (`CREATE TABLE`,
  `COMMENT ON`, constraints) or introspect a dump. Never transcribe a schema by hand.
- **Parse textually, no live database.** A generator that reads the `.sql` rather
  than connecting runs in CI with nothing but the interpreter — deterministic,
  reviewable, no credentials.
- **Emit two things:** a *reference* (tables, columns, types, constraints,
  comments) and a *diagram*. They serve different readers — one is looked up, one
  is scanned for shape.
- **Regenerate on every deploy.** Wire it into the docs build so the page and the
  diagram are rebuilt from the current schema — a generated artifact nobody
  regenerates is just a slower kind of stale.

## Diagrams

DBML is a useful portable intermediate: it renders to an interactive ER diagram
and also pastes straight into dbdiagram.io. For a large schema, **color or group
tables by logical schema and pack disconnected tables** so the layout stays
compact instead of a tall strip. Keep a plain-text fallback (e.g. Mermaid) for
places an interactive viewer won't render, like a repo file browser.

## See Also

- visualization/diagrams(softwaredev) — general diagramming guidance
- documentation(documentation) — where generated reference pages live in the doc graph
- data/migrations(data) — regenerate after a schema change
