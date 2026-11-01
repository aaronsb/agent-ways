---
description: generating database documentation from the schema — a reference page and an entity-relationship diagram derived from the DDL, kept in sync in CI
vocabulary: schema documentation erd entity relationship diagram dbml docgen generate ddl reference data dictionary table column pg_dump introspect catalog dbdiagram generator plain-text regenerate parse
pattern: \berd\b|entity.?relationship|\bdbml\b|schema.?(doc|diagram|reference|dictionary)|data.?dictionary|pg_dump.{0,30}(doc|diagram)
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Schema Documentation

Schema docs written by hand go out of date as soon as a migration lands. **Generate them from the DDL, which is the schema's source of truth, so they always match the schema.**

## Principles

- **Derive the docs from the schema.** Parse the DDL (`CREATE TABLE`, `COMMENT ON`, constraints) or introspect a dump. Never transcribe a schema by hand.
- **Read the SQL as text instead of connecting to a database.** A generator that reads the `.sql` files runs in CI with only its interpreter installed. Its output is deterministic and reviewable, and it needs no credentials.
- **Emit two things:** a *reference* (tables, columns, types, constraints, comments) and a *diagram*. They serve different readers: one reader looks up a column in the reference, and another scans the diagram to see how the tables relate.
- **Regenerate on every deploy.** Wire the generator into the docs build, so that every deploy rebuilds the page and the diagram from the current schema.

## Diagrams

DBML is a portable intermediate format: it renders to an interactive ER diagram and also pastes straight into dbdiagram.io. For a large schema, **color or group tables by logical schema and pack disconnected tables** so the layout stays compact instead of stretching into one tall column. Keep a plain-text fallback (e.g. Mermaid) for places an interactive viewer won't render, like a repo file browser.

## See Also

- visualization/diagrams(softwaredev) — general diagramming guidance
- documentation(documentation) — where generated reference pages live in the doc graph
- data/migrations(data) — regenerate after a schema change
