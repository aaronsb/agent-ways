---
description: choosing and scoping a data store — how much database machinery a project actually needs, from a flat file or SQLite up to a migrated multi-schema relational or graph database
vocabulary: database data store persistence sql sqlite postgres postgresql mysql duckdb mariadb schema table relational nosql document graph datastore db datamodel persistence
pattern: sqlite|postgres(ql)?|mysql|mariadb|duckdb|datastore|\.db\b|persistence.?layer|data.?(model|store|layer)|database
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: premise -->
# Data

**Scope the data layer to the actual need before reaching for machinery.** The cost of a database is not the store — it's the discipline the store pulls in: migrations, schema docs, backups, review. That discipline is worth it when the data is shared and long-lived, and pure overhead when it isn't. Match the tool to the pressure, not to habit.

| The data is… | Reach for | Skip |
|---|---|---|
| Ephemeral, single-process, small | an in-memory structure or a JSON/CSV file | migrations, ERDs, a server |
| Local, single-writer, needs queries | **SQLite** (one file, no server) | a migration framework, multi-schema |
| Shared, evolving, multi-writer | a server RDBMS (Postgres/MySQL) with **versioned migrations** | — |
| Densely interconnected, traversal-first | a graph store (or a graph extension) | forcing it into flat tables |

The deeper children below are for the bottom two rows. If someone says "let's just use SQLite for this," they need *this* orientation — not a lecture on migration consolidation or ER-diagram generation. Don't pull the heavy guidance until the need is real.

## Going deeper

| Concern | Way |
|---|---|
| Schema design & modeling | `data/modeling` *(forthcoming)* |
| Migrations — numbering, idempotency, consolidation | `data/migrations` |
| Schema docs & ER diagrams | `data/documentation` |

## See Also

- architecture/design(softwaredev) — where a store fits in the broader system design
- data/migrations(data) — the migration discipline, once you have a server DB
- data/documentation(data) — generating schema reference and diagrams
