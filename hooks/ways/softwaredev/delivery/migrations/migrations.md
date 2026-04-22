---
description: database migrations, schema changes, table alterations, rollback procedures
vocabulary: migration schema alter table column index rollback seed ddl prisma alembic knex flyway
pattern: migrat|schema|database.?change|alter.?table|alembic|prisma.?migrate|knex.?migrate|flyway|liquibase
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Migrations Way

## What Claude Should Produce

- **Always include both up and down** (create and rollback)
- **One logical change per migration** — don't combine table creation with data transforms
- **Never modify existing migrations** — create a new one

## When Generating Migrations

1. Detect the project's migration tool (Prisma, Alembic, Knex, Flyway, ActiveRecord, raw SQL)
2. Follow its naming convention and directory structure
3. Include a comment explaining what this migration does and why

## Warn the User When

- Migration touches a likely-large table (users, events, logs) — suggest online DDL or batched approach
- Migration is irreversible (dropping column/table) — confirm intent, note that rollback cannot restore data
- Data migration mixed with schema migration — recommend separating them

## Rollback Verification

After writing the migration, verify: if you run `up` then `down`, is the schema unchanged? If not, flag it.
