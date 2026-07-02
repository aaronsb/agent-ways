---
description: database migration, schema change, alter table, add column, index creation, data migration
vocabulary: migration schema alter column index table constraint foreign key rollback seed data transform
scope: agent
---

## anchor

You are modifying a database schema. Migrations are hard to reverse in production — verify before writing.

## check

Before writing this migration:
- Have you read the **current schema** (not assumed it)?
- Is this migration **reversible**? If not, does it need to be?
- What existing data is affected — will this migration need a **data backfill**?
- Are there other services or queries that depend on the table/column being changed?
- Is there a migration naming convention or ordering system in this project?
